use std::collections::{BTreeSet, HashMap, HashSet};

use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;
use std::sync::LazyLock;

use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{Handle, HandleKind, HandleMetadata, NodeId};
use crate::parse::{LabelCandidate, PendingEdge};

// ---------------------------------------------------------------------------
// Resolution cascade types (Phase 6, RESOLVE-02..06)
// ---------------------------------------------------------------------------

/// Result of cascade resolution for a single unresolved pending edge.
#[derive(Clone, Debug)]
pub(crate) struct CascadeResult {
    /// Index into the original pending_edges slice.
    pub(crate) edge_index: usize,
    /// Candidate handle identities found by structural transforms.
    pub(crate) candidates: Vec<String>,
}

/// Regex for zero-pad normalization of compound labels like `KB-D01`, `OQ-01`.
/// Validates compound label prefixes before trailing digits are normalized.
static COMPOUND_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Z][A-Z0-9_]*(?:-[A-Z][A-Z0-9_]*)*-?$")
        .expect("compound label regex must compile")
});

// ---------------------------------------------------------------------------
// Regex for version handle detection in filenames
// ---------------------------------------------------------------------------

/// Matches filenames like `formal-model-v3.md`, `proof-v17.md`.
static VERSION_FILENAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+)-v(\d+)\.md$").expect("version filename regex must compile")
});

// ---------------------------------------------------------------------------
// Resolve result types
// ---------------------------------------------------------------------------

struct ResolveResult {
    labels_resolved: usize,
    labels_skipped: usize,
}

/// Overall statistics from the full resolution pipeline.
pub(crate) struct ResolveStats {
    pub(crate) namespaces: HashSet<String>,
    pub(crate) labels_resolved: usize,
    pub(crate) labels_skipped: usize,
    pub(crate) versions_resolved: usize,
    pub(crate) pending_edges_resolved: usize,
    pub(crate) pending_edges_unresolved: usize,
}

/// Resolution outcome for a discovered reference.
/// Phase 4: type definition only.
/// Phase 6: populated by resolution cascade (RESOLVE-02..06).
#[derive(Clone, Debug, Serialize)]
#[allow(dead_code)] // Variants used by Phase 6 resolution cascade
pub(crate) enum Resolution {
    /// Exact match to a known handle.
    Exact(NodeId),
    /// Match via structural transform with candidate list (Phase 6).
    Fuzzy { candidates: Vec<NodeId> },
    /// No match found.
    Unresolved,
}

// ---------------------------------------------------------------------------
// Namespace inference (HANDLE-05, KB-D4)
// ---------------------------------------------------------------------------

/// Infer which label prefixes are real namespaces based on sequential cardinality.
///
/// A prefix is confirmed if it has N >= 3 distinct sequential numbers across
/// M >= 2 distinct files. Config overrides (`handles.confirmed`, `handles.rejected`)
/// take precedence over inference. Prefixes with only large isolated numbers
/// (e.g., SHA-256, AVX-512) are rejected.
pub(crate) fn infer_namespaces(
    candidates: &[LabelCandidate],
    config: &AnnealConfig,
) -> HashSet<String> {
    let mut confirmed: HashSet<String> = config.handles.confirmed.iter().cloned().collect();
    let rejected: HashSet<&str> = config.handles.rejected.iter().map(String::as_str).collect();

    // Group candidates by prefix, borrowing from the candidates slice
    let mut by_prefix: HashMap<&str, Vec<(u32, &Utf8Path)>> = HashMap::new();
    for c in candidates {
        by_prefix
            .entry(&c.prefix)
            .or_default()
            .push((c.number, &c.file_path));
    }

    for (prefix, occurrences) in &by_prefix {
        if rejected.contains(prefix) || confirmed.contains(*prefix) {
            continue;
        }

        let numbers: BTreeSet<u32> = occurrences.iter().map(|(n, _)| *n).collect();
        let distinct_files: HashSet<&&Utf8Path> = occurrences.iter().map(|(_, f)| f).collect();

        if numbers.len() < 3 || distinct_files.len() < 2 {
            continue;
        }

        // Reject prefixes with only large isolated numbers (e.g., SHA-256, AVX-512)
        let min_num = numbers.iter().next().copied().unwrap_or(0);
        if min_num > 100 && !has_sequential_run(&numbers) {
            continue;
        }

        confirmed.insert((*prefix).to_string());
    }

    confirmed
}

/// Check whether a set of numbers contains at least 3 consecutive values.
fn has_sequential_run(numbers: &BTreeSet<u32>) -> bool {
    let nums: Vec<u32> = numbers.iter().copied().collect();
    if nums.len() < 3 {
        return false;
    }
    let mut run_len = 1u32;
    for window in nums.windows(2) {
        if window[1] == window[0] + 1 {
            run_len += 1;
            if run_len >= 3 {
                return true;
            }
        } else {
            run_len = 1;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Label resolution (HANDLE-03, HANDLE-06)
// ---------------------------------------------------------------------------

/// Resolve label candidates into graph nodes and edges.
///
/// For each candidate whose prefix is in a confirmed namespace:
/// - Create a Label handle node if not already present
/// - Create an edge from the file node to the label node with the candidate's edge kind
///
/// Candidates with unconfirmed prefixes are skipped silently (HANDLE-06).
fn resolve_labels(
    graph: &mut DiGraph,
    candidates: &[LabelCandidate],
    namespaces: &HashSet<String>,
    node_index: &mut HashMap<String, NodeId>,
) -> ResolveResult {
    let mut labels_resolved: usize = 0;
    let mut labels_skipped: usize = 0;

    for candidate in candidates {
        if !namespaces.contains(&candidate.prefix) {
            labels_skipped += 1;
            continue;
        }

        let label_id = format!("{}-{}", candidate.prefix, candidate.number);

        let label_node = if let Some(&existing) = node_index.get(&label_id) {
            existing
        } else {
            let node = graph.add_node(Handle {
                id: label_id.clone(),
                kind: HandleKind::Label {
                    prefix: candidate.prefix.clone(),
                    number: candidate.number,
                },
                status: None,
                file_path: Some(candidate.file_path.clone()),
                metadata: HandleMetadata::default(),
            });
            node_index.insert(label_id, node);
            labels_resolved += 1;
            node
        };

        let file_id = candidate.file_path.to_string();
        if let Some(&source_node) = node_index.get(&file_id) {
            graph.add_edge(source_node, label_node, candidate.edge_kind);
        }
    }

    ResolveResult {
        labels_resolved,
        labels_skipped,
    }
}

// ---------------------------------------------------------------------------
// Version handle resolution (HANDLE-04, KB-D2, KB-D3)
// ---------------------------------------------------------------------------

/// Resolve version handles by matching versioned artifact naming conventions.
///
/// Scans existing File handles for files matching `*-v{N}.md`. For each match:
/// 1. Extract the base name and version number
/// 2. Create a Version handle node
/// 3. Add Supersedes edges forming a supersession chain (v3 -> v2 -> v1)
///
/// Returns the count of version handles created.
pub(crate) fn resolve_versions(
    graph: &mut DiGraph,
    node_index: &mut HashMap<String, NodeId>,
) -> usize {
    // Collect versioned files from existing File handles
    // Group by base name: base -> Vec<(version, file_node_id)>
    let mut versioned: HashMap<String, Vec<(u32, NodeId)>> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        if let HandleKind::File(ref path) = handle.kind {
            // Get just the filename
            let filename = path.file_name().unwrap_or("");
            if let Some(caps) = VERSION_FILENAME_RE.captures(filename) {
                let base = caps.get(1).map_or("", |m| m.as_str()).to_string();
                if let Ok(version) = caps.get(2).map_or("0", |m| m.as_str()).parse::<u32>() {
                    versioned.entry(base).or_default().push((version, node_id));
                }
            }
        }
    }

    let mut count: usize = 0;

    for (base, mut versions) in versioned {
        // Sort by version number ascending
        versions.sort_by_key(|(v, _)| *v);

        let mut prev_version_node: Option<NodeId> = None;

        for (version, file_node) in &versions {
            let version_id = format!("{base}-v{version}");

            // D-09: Version handles inherit status from their parent file handle
            let file_status = graph.node(*file_node).status.clone();

            // Create Version handle node
            let version_node = graph.add_node(Handle {
                id: version_id.clone(),
                kind: HandleKind::Version {
                    artifact: *file_node,
                    version: *version,
                },
                status: file_status,
                file_path: None,
                metadata: HandleMetadata::default(),
            });
            node_index.insert(version_id, version_node);
            count += 1;

            // Add Supersedes edge: this version supersedes the previous one
            if let Some(prev) = prev_version_node {
                graph.add_edge(version_node, prev, EdgeKind::Supersedes);
            }

            prev_version_node = Some(version_node);
        }
    }

    count
}

// ---------------------------------------------------------------------------
// Pending edge resolution (HANDLE-01, HANDLE-02)
// ---------------------------------------------------------------------------

/// Resolve pending edges by looking up target identities in the node index.
///
/// For each pending edge, if the target identity maps to a known node, creates
/// the edge. If not found and the target looks like a bare filename (contains
/// `.md` but no `/`), attempts filesystem-based resolution via
/// `resolve_file_path` (D-02), then falls back to the corpus-wide filename
/// index for unambiguous matches.
///
/// Returns the count of edges resolved.
pub(crate) fn resolve_pending_edges(
    graph: &mut DiGraph,
    pending: &[PendingEdge],
    node_index: &HashMap<String, NodeId>,
    root: &Utf8Path,
    filename_index: &HashMap<String, Vec<Utf8PathBuf>>,
) -> usize {
    let mut resolved: usize = 0;

    for edge in pending {
        let resolved_target = node_index.get(&edge.target_identity).copied().or_else(|| {
            // D-02: Bare filename resolution
            if std::path::Path::new(&edge.target_identity)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                && !edge.target_identity.contains('/')
            {
                // Try relative-to-referring-file and root-relative resolution
                let referring_file = graph.node(edge.source).file_path.clone();
                if let Some(ref referring) = referring_file
                    && let Some(resolved_path) =
                        resolve_bare_filename(&edge.target_identity, referring, root, node_index)
                {
                    return Some(resolved_path);
                }

                // Fallback: corpus-wide filename index lookup (unambiguous only)
                if let Some(paths) = filename_index.get(&edge.target_identity)
                    && paths.len() == 1
                {
                    return node_index.get(&paths[0].to_string()).copied();
                }
            }
            None
        });

        if let Some(target_id) = resolved_target {
            // Handle inverse direction: swap source and target for inverse edges
            if edge.inverse {
                graph.add_edge(target_id, edge.source, edge.kind);
            } else {
                graph.add_edge(edge.source, target_id, edge.kind);
            }
            resolved += 1;
        }
    }

    resolved
}

// ---------------------------------------------------------------------------
// File path resolution (Pitfall 4)
// ---------------------------------------------------------------------------

/// Resolve a file path reference relative to the referring file's directory (D-02).
///
/// Joins the reference path relative to the referring file's parent directory,
/// normalizes it (resolving `..` components), and makes it relative to root.
/// Returns `None` when the normalized path escapes the corpus root.
pub(crate) fn resolve_file_path(
    reference: &str,
    referring_file: &Utf8Path,
    root: &Utf8Path,
) -> Option<Utf8PathBuf> {
    // Get the parent directory of the referring file
    let parent = referring_file.parent().unwrap_or(Utf8Path::new(""));

    // Join relative to root, then relative to parent
    let absolute = root.join(parent).join(reference);

    // Normalize the path (resolve .. components)
    let normalized = normalize_path(&absolute);

    // Make relative to root
    normalized
        .strip_prefix(root)
        .ok()
        .map(Utf8Path::to_path_buf)
}

/// Normalize a UTF-8 path by resolving `.` and `..` components without
/// requiring the path to exist on disk.
fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component.as_str() {
            "." => {}
            ".." => {
                components.pop();
            }
            c => components.push(c),
        }
    }
    if components.is_empty() {
        Utf8PathBuf::from(".")
    } else {
        let mut result = Utf8PathBuf::new();
        for (i, c) in components.iter().enumerate() {
            if i == 0 && c.is_empty() {
                // Preserve leading slash for absolute paths
                result.push("/");
            } else {
                result.push(c);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Build node index from graph
// ---------------------------------------------------------------------------

/// Attempt root-relative resolution for a bare filename.
///
/// If `resolve_file_path` returns `None` (not found relative to referring file),
/// try a direct root-relative join.
fn resolve_bare_filename(
    reference: &str,
    referring_file: &Utf8Path,
    root: &Utf8Path,
    node_index: &HashMap<String, NodeId>,
) -> Option<NodeId> {
    // First try relative to referring file's directory
    if let Some(found) = resolve_file_path(reference, referring_file, root) {
        let key = found.as_str();
        if let Some(&node_id) = node_index.get(key) {
            return Some(node_id);
        }
    }

    // Fallback: try at root level
    node_index.get(reference).copied()
}

/// Build a mapping from handle identity strings to `NodeId`s.
///
/// Identity mappings:
/// - File handles: identity = relative path string
/// - Section handles: identity = "{file}#{heading-slug}"
/// - Label handles: identity = "PREFIX-NUMBER"
/// - Version handles: identity = "{base}-vN"
pub(crate) fn build_node_index(graph: &DiGraph) -> HashMap<String, NodeId> {
    let mut index = HashMap::with_capacity(graph.node_count());
    for (node_id, handle) in graph.nodes() {
        index.insert(handle.id.clone(), node_id);
    }
    index
}

// ---------------------------------------------------------------------------
// Top-level resolve orchestrator
// ---------------------------------------------------------------------------

/// Resolve all handles: namespace inference, label nodes, version nodes, pending edges.
pub(crate) fn resolve_all(
    graph: &mut DiGraph,
    candidates: &[LabelCandidate],
    pending: &[PendingEdge],
    config: &AnnealConfig,
    root: &Utf8Path,
    filename_index: &HashMap<String, Vec<Utf8PathBuf>>,
) -> ResolveStats {
    let namespaces = infer_namespaces(candidates, config);
    let mut node_index = build_node_index(graph);

    let label_result = resolve_labels(graph, candidates, &namespaces, &mut node_index);
    let versions_resolved = resolve_versions(graph, &mut node_index);

    // node_index already contains label and version nodes (mutated in-place above)
    let pending_resolved = resolve_pending_edges(graph, pending, &node_index, root, filename_index);
    let pending_unresolved = pending.len() - pending_resolved;

    ResolveStats {
        namespaces,
        labels_resolved: label_result.labels_resolved,
        labels_skipped: label_result.labels_skipped,
        versions_resolved,
        pending_edges_resolved: pending_resolved,
        pending_edges_unresolved: pending_unresolved,
    }
}

// ---------------------------------------------------------------------------
// Resolution cascade (Phase 6, RESOLVE-02..06)
// ---------------------------------------------------------------------------

/// Try stripping a root prefix from the target to match a known handle.
///
/// If target starts with `root_prefix/`, strip it and look up the remainder.
/// Unambiguous match returns `(Some(node_id), vec![stripped])`.
fn try_root_prefix_strip(
    target: &str,
    node_index: &HashMap<String, NodeId>,
    root_prefix: &str,
) -> (Option<NodeId>, Vec<String>) {
    if root_prefix.is_empty() || root_prefix == "." {
        return (None, Vec::new());
    }

    // Normalize: ensure prefix ends with /
    let prefix_with_slash = if root_prefix.ends_with('/') {
        root_prefix.to_string()
    } else {
        format!("{root_prefix}/")
    };

    if !target.starts_with(&prefix_with_slash) {
        return (None, Vec::new());
    }

    let stripped = &target[prefix_with_slash.len()..];
    if stripped.is_empty() {
        return (None, Vec::new());
    }

    if let Some(&node_id) = node_index.get(stripped) {
        (Some(node_id), vec![stripped.to_string()])
    } else {
        // Check for ambiguous matches (multiple keys ending with the stripped portion)
        let candidates: Vec<String> = node_index
            .keys()
            .filter(|k| {
                k.ends_with(stripped)
                    && (k.len() == stripped.len()
                        || k.as_bytes().get(k.len() - stripped.len() - 1) == Some(&b'/'))
            })
            .cloned()
            .collect();

        if candidates.len() == 1 {
            let node_id = node_index[&candidates[0]];
            (Some(node_id), candidates)
        } else if candidates.is_empty() {
            (None, Vec::new())
        } else {
            (None, candidates)
        }
    }
}

/// Try matching a versioned filename against other versions in the index.
///
/// If target matches `{base}-v{N}.md`, finds all `{base}-v{M}.md` where M != N,
/// sorted by version descending (latest first).
fn try_version_stem(target: &str, node_index: &HashMap<String, NodeId>) -> Vec<String> {
    let Some(caps) = VERSION_FILENAME_RE.captures(target) else {
        return Vec::new();
    };

    let base = caps.get(1).map_or("", |m| m.as_str());
    let this_version: u32 = match caps.get(2).map_or("0", |m| m.as_str()).parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut matches: Vec<(u32, String)> = Vec::new();

    for key in node_index.keys() {
        // Extract filename from potential path
        let filename = key.rsplit('/').next().unwrap_or(key);
        if let Some(kcaps) = VERSION_FILENAME_RE.captures(filename) {
            let kbase = kcaps.get(1).map_or("", |m| m.as_str());
            if let Ok(kver) = kcaps.get(2).map_or("0", |m| m.as_str()).parse::<u32>()
                && kbase == base
                && kver != this_version
            {
                matches.push((kver, key.clone()));
            }
        }
    }

    // Sort by version descending (latest first)
    matches.sort_by(|a, b| b.0.cmp(&a.0));
    matches.into_iter().map(|(_, s)| s).collect()
}

/// Build canonical candidate identities for a zero-padded label.
///
/// Preserves the separator style used by the query first, then tries the
/// alternate form so both `OQ-01`/`OQ-1` and `KB-D01`/`KB-D-01` can resolve.
pub(crate) fn split_trailing_numeric_suffix(target: &str) -> Option<(&str, &str)> {
    let digit_start = target
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    let (prefix, digits) = target.split_at(digit_start);
    (!digits.is_empty()).then_some((prefix, digits))
}

pub(crate) fn zero_padded_label_candidates(target: &str) -> Option<[String; 2]> {
    let (prefix, digits) = split_trailing_numeric_suffix(target)?;
    if !digits.starts_with('0') || !COMPOUND_LABEL_RE.is_match(prefix) {
        return None;
    }

    let number: u32 = digits.parse().ok()?;
    let has_separator = prefix.ends_with('-');
    let prefix = prefix.trim_end_matches('-');
    let primary = if has_separator {
        format!("{prefix}-{number}")
    } else {
        format!("{prefix}{number}")
    };
    let alternate = if has_separator {
        format!("{prefix}{number}")
    } else {
        format!("{prefix}-{number}")
    };
    Some([primary, alternate])
}

/// Try normalizing a zero-padded label to its canonical form.
///
/// `OQ-01` -> `OQ-1`, `KB-D01` -> `KB-D1`.
fn try_zero_pad_normalize(
    target: &str,
    node_index: &HashMap<String, NodeId>,
) -> Option<(NodeId, String)> {
    let [primary, alternate] = zero_padded_label_candidates(target)?;

    node_index
        .get(&primary)
        .map(|&node_id| (node_id, primary.clone()))
        .or_else(|| {
            node_index
                .get(&alternate)
                .map(|&node_id| (node_id, alternate))
        })
}

/// Run deterministic structural transforms on unresolved pending edges.
///
/// Called after `resolve_all()`. For each unresolved edge, tries strategies
/// in order: root-prefix strip, version stem, zero-pad normalize.
/// Root-prefix matches that are unambiguous create graph edges.
/// All other matches produce candidate lists for diagnostic enrichment.
pub(crate) fn cascade_unresolved(
    graph: &mut DiGraph,
    pending: &[PendingEdge],
    node_index: &HashMap<String, NodeId>,
    root_prefix: &str,
) -> Vec<CascadeResult> {
    let mut results = Vec::new();

    for (idx, edge) in pending.iter().enumerate() {
        // Skip already-resolved edges
        if node_index.contains_key(&edge.target_identity) {
            continue;
        }

        // Skip section refs
        if edge.target_identity.starts_with("section:") {
            continue;
        }

        let mut candidates = Vec::new();
        // Strategy 1: Root-prefix strip
        let (rp_resolved, rp_candidates) =
            try_root_prefix_strip(&edge.target_identity, node_index, root_prefix);
        if let Some(node_id) = rp_resolved {
            // Unambiguous root-prefix match: create graph edge
            if edge.inverse {
                graph.add_edge(node_id, edge.source, edge.kind);
            } else {
                graph.add_edge(edge.source, node_id, edge.kind);
            }
        }
        candidates.extend(rp_candidates);

        // Strategy 2: Version stem
        let vs_candidates = try_version_stem(&edge.target_identity, node_index);
        candidates.extend(vs_candidates);

        // Strategy 3: Zero-pad normalize
        if let Some((_node_id, canonical)) =
            try_zero_pad_normalize(&edge.target_identity, node_index)
        {
            candidates.push(canonical);
        }

        if !candidates.is_empty() || rp_resolved.is_some() {
            results.push(CascadeResult {
                edge_index: idx,
                candidates,
            });
        }
    }

    results
}

#[cfg(test)]
mod cascade_tests {
    use super::*;

    fn build_test_graph_and_index() -> (DiGraph, HashMap<String, NodeId>) {
        let mut graph = DiGraph::new();

        let foo = graph.add_node(Handle::test_file("foo.md", None));
        let bar = graph.add_node(Handle::test_file("bar.md", None));
        let fmv17 = graph.add_node(Handle::test_file("formal-model-v17.md", None));
        let fmv5 = graph.add_node(Handle::test_file("formal-model-v5.md", None));
        let fmv4 = graph.add_node(Handle::test_file("formal-model-v4.md", None));
        let oq1 = graph.add_node(Handle::test_label("OQ", 1, None));
        let kbd1 = graph.add_node(Handle::test_label("KB-D", 1, None));

        let _ = (foo, bar, fmv17, fmv5, fmv4, oq1, kbd1);

        let index = build_node_index(&graph);
        (graph, index)
    }

    fn make_pending(source: NodeId, target: &str) -> PendingEdge {
        PendingEdge {
            source,
            target_identity: target.to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(1),
        }
    }

    #[test]
    fn cascade_root_prefix_strip_resolves() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("other.md", None));
        let edge_count_before = graph.edge_count();
        let edges = vec![make_pending(source, ".design/foo.md")];

        let results = cascade_unresolved(&mut graph, &edges, &index, ".design");
        assert_eq!(results.len(), 1);
        assert!(results[0].candidates.contains(&"foo.md".to_string()));
        assert_eq!(
            graph.edge_count(),
            edge_count_before + 1,
            "root-prefix strip should create a graph edge"
        );
    }

    #[test]
    fn cascade_root_prefix_ambiguous_no_resolve() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_file("sub/foo.md", None));
        let _b = graph.add_node(Handle::test_file("other/foo.md", None));
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edge_count_before = graph.edge_count();

        // Neither sub/foo.md nor other/foo.md will match a root-prefix strip of ".design/foo.md"
        // because the stripped "foo.md" doesn't exist in index (only sub/foo.md and other/foo.md do).
        let index = build_node_index(&graph);
        let edges = vec![make_pending(source, ".design/foo.md")];
        let _results = cascade_unresolved(&mut graph, &edges, &index, ".design");

        assert_eq!(
            graph.edge_count(),
            edge_count_before,
            "ambiguous or non-matching should not create a graph edge"
        );
    }

    #[test]
    fn cascade_version_stem_suggests_alternatives() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edges = vec![make_pending(source, "formal-model-v11.md")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert_eq!(results.len(), 1);
        let cands = &results[0].candidates;
        assert!(
            cands.contains(&"formal-model-v17.md".to_string()),
            "should suggest v17, got {cands:?}",
        );
    }

    #[test]
    fn cascade_version_stem_sorted_latest_first() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edges = vec![make_pending(source, "proof-v3.md")];

        // No proof-v*.md exist, so no candidates expected
        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert!(results.is_empty());

        // Use formal-model which has v4, v5, v17
        let edges = vec![make_pending(source, "formal-model-v11.md")];
        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert_eq!(results.len(), 1);

        let cands = &results[0].candidates;
        // Should be sorted latest first: v17, v5, v4
        let v17_pos = cands
            .iter()
            .position(|c| c == "formal-model-v17.md")
            .expect("v17 candidate");
        let v5_pos = cands
            .iter()
            .position(|c| c == "formal-model-v5.md")
            .expect("v5 candidate");
        let v4_pos = cands
            .iter()
            .position(|c| c == "formal-model-v4.md")
            .expect("v4 candidate");
        assert!(
            v17_pos < v5_pos && v5_pos < v4_pos,
            "expected latest-first order: v17@{v17_pos}, v5@{v5_pos}, v4@{v4_pos}",
        );
    }

    #[test]
    fn cascade_zero_pad_resolves() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edges = vec![make_pending(source, "OQ-01")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert_eq!(results.len(), 1);
        assert!(results[0].candidates.contains(&"OQ-1".to_string()));
    }

    #[test]
    fn cascade_zero_pad_compound_prefix() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        // KB-D-01 is the zero-padded form of KB-D-1 (prefix KB-D, number 01)
        let edges = vec![make_pending(source, "KB-D-01")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert_eq!(results.len(), 1);
        assert!(
            results[0].candidates.contains(&"KB-D-1".to_string()),
            "expected KB-D-1 in candidates, got {:?}",
            results[0].candidates
        );
    }

    #[test]
    fn cascade_non_matching_produces_empty() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edges = vec![make_pending(source, "totally-unknown-ref")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert!(results.is_empty(), "no strategies should match");
    }

    #[test]
    fn cascade_root_prefix_creates_edge() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("other.md", None));
        let edge_count_before = graph.edge_count();
        let edges = vec![make_pending(source, ".design/foo.md")];

        let results = cascade_unresolved(&mut graph, &edges, &index, ".design");
        assert_eq!(results.len(), 1);
        assert!(results[0].candidates.contains(&"foo.md".to_string()));
        assert_eq!(
            graph.edge_count(),
            edge_count_before + 1,
            "root-prefix match should create graph edge"
        );
    }

    #[test]
    fn cascade_skips_already_resolved() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        // "foo.md" is already in the index
        let edges = vec![make_pending(source, "foo.md")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert!(
            results.is_empty(),
            "already resolved edges should be skipped"
        );
    }

    #[test]
    fn cascade_skips_section_refs() {
        let (mut graph, index) = build_test_graph_and_index();
        let source = graph.add_node(Handle::test_file("ref.md", None));
        let edges = vec![make_pending(source, "section:some-heading")];

        let results = cascade_unresolved(&mut graph, &edges, &index, "");
        assert!(results.is_empty(), "section refs should be skipped");
    }
}
