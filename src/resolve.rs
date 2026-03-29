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
                        resolve_bare_filename(&edge.target_identity, referring, root)
                {
                    return node_index.get(&resolved_path.to_string()).copied();
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
/// Returns `Some(normalized)` if the resolved path exists, `None` otherwise.
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

    // Check if the file exists
    if normalized.exists() {
        // Make relative to root
        normalized
            .strip_prefix(root)
            .ok()
            .map(Utf8Path::to_path_buf)
    } else {
        None
    }
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
) -> Option<Utf8PathBuf> {
    // First try relative to referring file's directory
    if let Some(found) = resolve_file_path(reference, referring_file, root) {
        return Some(found);
    }
    // Fallback: try at root level
    let root_path = root.join(reference);
    if root_path.exists() {
        root_path.strip_prefix(root).ok().map(Utf8Path::to_path_buf)
    } else {
        None
    }
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
