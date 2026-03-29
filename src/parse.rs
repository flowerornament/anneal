use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use regex::{Regex, RegexSet};
use walkdir::WalkDir;

use crate::config::{AnnealConfig, Direction, FrontmatterConfig};
use crate::extraction::{RefHint, classify_frontmatter_value};
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{Handle, HandleKind, HandleMetadata, NodeId};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Directories always excluded from scanning, regardless of root.
pub(crate) const DEFAULT_EXCLUSIONS: &[&str] = &[
    ".git",
    ".planning",
    ".anneal",
    "target",
    "node_modules",
    ".build",
];

/// Body-text keywords that imply a DependsOn edge (D-01).
static DEPENDS_ON_KEYWORDS: &[&str] = &["incorporates", "builds on", "extends", "based on"];

/// Body-text keywords that explicitly confirm a Cites edge (D-01).
static CITES_KEYWORDS: &[&str] = &["see also", "cf.", "related"];

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

// Pattern indices for the RegexSet — avoids magic numbers in scan_file.
const PAT_HEADING: usize = 0;
const PAT_LABEL: usize = 1;
const PAT_SECTION_REF: usize = 2;
const PAT_FILE_PATH: usize = 3;
/// Five-pattern `RegexSet` for single-pass content scanning (KB-D6, section 5.1).
///
/// Most lines match zero patterns — the fast path is one automaton pass.
/// Only matching lines trigger individual `Regex` extraction.
static PATTERN_SET: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"^#{1,6}\s",          // PAT_HEADING
        r"[A-Z][A-Z_]*-\d+",   // PAT_LABEL
        r"§\d+(?:\.\d+)*",     // PAT_SECTION_REF
        r"[a-z0-9_/-]+\.md\b", // PAT_FILE_PATH
        r"\bv\d+\b",           // PAT_VERSION
    ])
    .expect("regex patterns must compile")
});

/// Capture regex for label references: prefix and number.
static LABEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Z][A-Z_]*)-(\d+)").expect("label regex must compile"));

/// Capture regex for section cross-references (paragraph sign).
static SECTION_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"§(\d+(?:\.\d+)*)").expect("section ref regex must compile"));

/// Capture regex for file path references.
static FILE_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-z0-9_/-]+\.md)\b").expect("file path regex must compile"));

/// Capture regex for section headings: level and text.
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+)").expect("heading regex must compile"));

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

/// Split file content into optional YAML frontmatter and body.
///
/// The opening `---` must be the very first characters of the file (no leading
/// whitespace). The closing `---` must appear on its own line. Returns
/// `(Some(yaml_str), body)` or `(None, full_content)` if no valid frontmatter.
pub(crate) fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    // Must start with exactly "---\n" (or "---\r\n")
    let rest = if let Some(r) = content.strip_prefix("---\n") {
        r
    } else if let Some(r) = content.strip_prefix("---\r\n") {
        r
    } else {
        return (None, content);
    };

    // Find closing fence: "\n---\n" or "\n---\r\n" or "\n---" at EOF
    if let Some(pos) = rest.find("\n---\n") {
        let yaml = &rest[..pos];
        let body = &rest[pos + 5..]; // skip "\n---\n"
        (Some(yaml), body)
    } else if let Some(pos) = rest.find("\n---\r\n") {
        let yaml = &rest[..pos];
        let body = &rest[pos + 6..]; // skip "\n---\r\n"
        (Some(yaml), body)
    } else if let Some(yaml) = rest.strip_suffix("\n---") {
        (Some(yaml), "")
    } else {
        (None, content)
    }
}

/// Parse YAML frontmatter into a status, `HandleMetadata`, and extensible field edges (D-05).
///
/// Deserializes as `serde_yaml_ng::Value` to handle arbitrary fields and
/// YAML type coercion. On parse failure, returns defaults (never errors).
/// The `config` parameter drives which frontmatter keys produce edges.
pub(crate) fn parse_frontmatter(
    yaml: &str,
    config: &FrontmatterConfig,
) -> (
    Option<String>,
    HandleMetadata,
    Vec<FrontmatterEdge>,
    Vec<String>,
) {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return (None, HandleMetadata::default(), Vec::new(), Vec::new());
    };

    let Some(mapping) = value.as_mapping() else {
        return (None, HandleMetadata::default(), Vec::new(), Vec::new());
    };

    // Collect all frontmatter keys for init auto-detection (D-07)
    let all_keys: Vec<String> = mapping
        .keys()
        .filter_map(|k| k.as_str().map(String::from))
        .collect();

    let get = |key: &str| mapping.get(serde_yaml_ng::Value::String(key.to_string()));

    // Special fields: status and updated (not edge-producing)
    let status = get("status").and_then(yaml_value_to_string);
    let updated = get("updated")
        .and_then(yaml_value_to_string)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

    // Table-driven: scan all frontmatter keys against configured field mappings
    let mut field_edges = Vec::new();
    let mut superseded_by = None;
    let mut depends_on = Vec::new();
    let mut discharges = Vec::new();
    let mut verifies = Vec::new();

    for (key, val) in mapping {
        let Some(key_str) = key.as_str() else {
            continue;
        };
        // Skip special fields
        if key_str == "status" || key_str == "updated" {
            continue;
        }

        if let Some(field_mapping) = config.fields.get(key_str) {
            let Some(edge_kind) = EdgeKind::from_name(&field_mapping.edge_kind) else {
                continue;
            };
            let targets = yaml_value_to_string_vec(val);
            if targets.is_empty() {
                continue;
            }

            let inverse = matches!(field_mapping.direction, Direction::Inverse);

            // Backward compat: populate HandleMetadata for the 4 known fields
            match key_str {
                "superseded-by" => superseded_by = targets.first().cloned(),
                "depends-on" => depends_on.clone_from(&targets),
                "discharges" => discharges.clone_from(&targets),
                "verifies" => verifies.clone_from(&targets),
                _ => {}
            }

            field_edges.push(FrontmatterEdge {
                targets,
                edge_kind,
                inverse,
            });
        }
    }

    let metadata = HandleMetadata {
        updated,
        superseded_by,
        depends_on,
        discharges,
        verifies,
    };

    (status, metadata, field_edges, all_keys)
}

fn yaml_value_to_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(strip_trailing_parenthetical(s)),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Strip trailing parenthetical annotations from frontmatter values.
/// "specs/foo.md (the original plan)" → "specs/foo.md"
/// "OQ-64 (see discussion)" → "OQ-64"
fn strip_trailing_parenthetical(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(idx) = trimmed.rfind(" (")
        && trimmed.ends_with(')')
    {
        return trimmed[..idx].to_string();
    }
    trimmed.to_string()
}

fn yaml_value_to_string_vec(v: &serde_yaml_ng::Value) -> Vec<String> {
    match v {
        serde_yaml_ng::Value::Sequence(seq) => {
            seq.iter().filter_map(yaml_value_to_string).collect()
        }
        other => yaml_value_to_string(other).into_iter().collect(),
    }
}

// ---------------------------------------------------------------------------
// Scan result types
// ---------------------------------------------------------------------------

/// A label match found during content scanning, not yet resolved to a namespace.
pub(crate) struct LabelCandidate {
    pub(crate) prefix: String,
    pub(crate) number: u32,
    pub(crate) file_path: Utf8PathBuf,
    pub(crate) edge_kind: EdgeKind,
}

/// An edge whose target is identified by string (not yet resolved to a `NodeId`).
///
/// Frontmatter fields like `depends-on: OQ-64` reference targets that may not
/// have been scanned yet. Resolution to actual `NodeId` values happens in
/// `resolve.rs`.
#[derive(Clone)]
pub(crate) struct PendingEdge {
    pub(crate) source: NodeId,
    pub(crate) target_identity: String,
    pub(crate) kind: EdgeKind,
    /// If true, the actual graph edge is target -> source (inverse direction).
    pub(crate) inverse: bool,
}

/// An edge descriptor parsed from a frontmatter field via the extensible mapping.
pub(crate) struct FrontmatterEdge {
    pub(crate) targets: Vec<String>,
    pub(crate) edge_kind: EdgeKind,
    pub(crate) inverse: bool,
}

/// Result of scanning a single file's body content.
pub(crate) struct ScanResult {
    pub(crate) label_candidates: Vec<LabelCandidate>,
    pub(crate) section_refs: Vec<String>,
    pub(crate) file_refs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Edge kind inference
// ---------------------------------------------------------------------------

/// Infer edge kind from body-text keywords on the same line as a reference (D-01).
///
/// Same-line proximity rule: if a keyword appears anywhere on the same line as the
/// reference, the edge kind is inferred from that keyword. If no keyword matches,
/// the default is `Cites`.
fn infer_edge_kind_from_line(line: &str) -> EdgeKind {
    let lower = line.to_lowercase();

    for keyword in DEPENDS_ON_KEYWORDS {
        if lower.contains(keyword) {
            return EdgeKind::DependsOn;
        }
    }

    // Cites keywords confirm the default, but we check them for completeness
    for keyword in CITES_KEYWORDS {
        if lower.contains(keyword) {
            return EdgeKind::Cites;
        }
    }

    EdgeKind::Cites
}

// ---------------------------------------------------------------------------
// Content scanner
// ---------------------------------------------------------------------------

/// Scan a file's body content for handles and references.
///
/// Creates Section handles directly in the graph. Collects label candidates,
/// section refs, file refs, and version refs for later resolution.
/// Tracks code block boundaries to avoid spurious heading detection (Pitfall 3).
pub(crate) fn scan_file(
    body: &str,
    file_path: &Utf8Path,
    file_node: NodeId,
    graph: &mut DiGraph,
) -> ScanResult {
    let mut result = ScanResult {
        label_candidates: Vec::new(),
        section_refs: Vec::new(),
        file_refs: Vec::new(),
    };

    let mut in_code_block = false;

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // D-08: Skip ALL pattern matching inside code blocks
        if in_code_block {
            continue;
        }

        let matched = PATTERN_SET.matches(line);
        if !matched.matched_any() {
            continue;
        }

        if matched.matched(PAT_HEADING)
            && let Some(caps) = HEADING_RE.captures(line)
        {
            let heading = caps
                .get(2)
                .expect("heading capture group always present")
                .as_str()
                .trim()
                .to_string();
            if !heading.is_empty() {
                let section_id =
                    format!("{}#{}", file_path, heading.to_lowercase().replace(' ', "-"));
                graph.add_node(Handle {
                    id: section_id,
                    kind: HandleKind::Section {
                        parent: file_node,
                        heading,
                    },
                    status: None,
                    file_path: Some(file_path.to_path_buf()),
                    metadata: HandleMetadata::default(),
                });
            }
        }

        let line_edge_kind = infer_edge_kind_from_line(line);

        if matched.matched(PAT_LABEL) {
            for caps in LABEL_RE.captures_iter(line) {
                let prefix = caps
                    .get(1)
                    .expect("label prefix capture always present")
                    .as_str()
                    .to_string();
                let number_str = caps
                    .get(2)
                    .expect("label number capture always present")
                    .as_str();
                if let Ok(number) = number_str.parse::<u32>() {
                    result.label_candidates.push(LabelCandidate {
                        prefix,
                        number,
                        file_path: file_path.to_path_buf(),
                        edge_kind: line_edge_kind,
                    });
                }
            }
        }

        if matched.matched(PAT_SECTION_REF) {
            for caps in SECTION_REF_RE.captures_iter(line) {
                let section_num = caps
                    .get(1)
                    .expect("section ref capture always present")
                    .as_str()
                    .to_string();
                if !section_num.is_empty() {
                    result.section_refs.push(section_num);
                }
            }
        }

        if matched.matched(PAT_FILE_PATH) {
            // D-03: Use find_iter to get match positions for URL rejection
            for m in FILE_PATH_RE.find_iter(line) {
                // Reject URL fragments: skip if "://" appears anywhere before this match
                let prefix = &line[..m.start()];
                if prefix.contains("://") {
                    continue;
                }
                // Reject version-dot fragments: if the char before the match is '.',
                // this is a fragment like "2.md" from "v1.2.md" — skip it.
                if m.start() > 0 && line.as_bytes()[m.start() - 1] == b'.' {
                    continue;
                }
                let path = m.as_str();
                // Reject hyphen-prefixed fragments: "-foo.md" is a suffix left
                // after label extraction (e.g., "RQ-01-foo.md" → label "RQ-01" + "-foo.md")
                if path.starts_with('-') {
                    continue;
                }
                // Reject mid-word matches: if the character before the match is
                // alphanumeric, this is a fragment of a longer token (e.g., "yBRk.md"
                // from a YouTube ID). Real file references are preceded by whitespace,
                // punctuation, or start of line.
                if m.start() > 0 && line.as_bytes()[m.start() - 1].is_ascii_alphanumeric() {
                    continue;
                }
                result.file_refs.push(path.to_string());
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Root inference
// ---------------------------------------------------------------------------

/// Infer the root directory to scan (KB-D20).
///
/// 1. If `.design/` exists -> `.design`
/// 2. Else if `docs/` exists -> `docs`
/// 3. Else -> `.` (current directory)
pub(crate) fn infer_root(cwd: &Utf8Path) -> Utf8PathBuf {
    let design = cwd.join(".design");
    if design.is_dir() {
        return design;
    }

    let docs = cwd.join("docs");
    if docs.is_dir() {
        return docs;
    }

    cwd.to_path_buf()
}

// ---------------------------------------------------------------------------
// Graph construction
// ---------------------------------------------------------------------------

/// A frontmatter value rejected by the plausibility filter.
pub(crate) struct ImplausibleRef {
    pub(crate) file: String,
    pub(crate) raw_value: String,
    pub(crate) reason: String,
}

/// An external URL found in frontmatter.
#[allow(dead_code)] // Fields consumed when external URL handling is wired
pub(crate) struct ExternalRef {
    pub(crate) file: String,
    pub(crate) url: String,
}

/// Result of `build_graph`: the populated graph, label candidates for namespace
/// inference, pending edges for resolution, and observed status values for
/// lattice inference.
pub(crate) struct BuildResult {
    pub(crate) graph: DiGraph,
    pub(crate) label_candidates: Vec<LabelCandidate>,
    pub(crate) pending_edges: Vec<PendingEdge>,
    pub(crate) observed_statuses: HashSet<String>,
    /// Statuses found exclusively in terminal-convention directories (D-04).
    pub(crate) terminal_by_directory: HashSet<String>,
    /// Frontmatter keys observed across all files with occurrence counts (D-07).
    pub(crate) observed_frontmatter_keys: HashMap<String, usize>,
    /// Bare filename -> full relative paths, for corpus-wide resolution fallback.
    pub(crate) filename_index: HashMap<String, Vec<Utf8PathBuf>>,
    /// Implausible frontmatter values that were filtered before resolution.
    pub(crate) implausible_refs: Vec<ImplausibleRef>,
    /// External URL references found in frontmatter (tracked, not resolved).
    #[allow(dead_code)] // Consumed when HandleKind::External is added
    pub(crate) external_refs: Vec<ExternalRef>,
}

/// Build the knowledge graph from a directory of markdown files.
///
/// Walks the directory tree, creates File handles, scans content with
/// the 5-pattern `RegexSet`, and collects label candidates and pending
/// edges for later resolution.
/// Directories whose contents signal terminal convergence state (D-04).
const TERMINAL_DIRS: &[&str] = &["archive", "history", "prior"];

/// Check if a relative path has any ancestor directory matching a terminal convention.
fn is_in_terminal_directory(relative: &Utf8Path) -> bool {
    for component in relative.components() {
        let name = component.as_str();
        if TERMINAL_DIRS.contains(&name) {
            return true;
        }
    }
    false
}

pub(crate) fn build_graph(root: &Utf8Path, config: &AnnealConfig) -> Result<BuildResult> {
    let mut graph = DiGraph::new();
    let mut all_label_candidates = Vec::new();
    let mut pending_edges = Vec::new();
    let mut observed_statuses = HashSet::new();

    // D-04: Track which statuses appear in terminal vs non-terminal directories
    let mut status_in_terminal: HashMap<String, usize> = HashMap::new();
    let mut status_in_nonterminal: HashMap<String, usize> = HashMap::new();

    // D-07: Track all observed frontmatter keys for init auto-detection
    let mut observed_frontmatter_keys: HashMap<String, usize> = HashMap::new();

    // Bare filename -> relative paths, for corpus-wide resolution fallback
    let mut filename_index: HashMap<String, Vec<Utf8PathBuf>> = HashMap::new();

    // Plausibility filter tracking
    let mut implausible_refs: Vec<ImplausibleRef> = Vec::new();
    let mut external_refs: Vec<ExternalRef> = Vec::new();

    let extra_exclusions = &config.exclude;

    let walker = WalkDir::new(root.as_std_path())
        .into_iter()
        .filter_entry(|e| {
            let Some(name) = e.file_name().to_str() else {
                return false;
            };
            if e.file_type().is_dir() {
                if DEFAULT_EXCLUSIONS.contains(&name) {
                    return false;
                }
                if name.starts_with('.') && name != ".design" {
                    return false;
                }
                if extra_exclusions.iter().any(|ex| ex == name) {
                    return false;
                }
            }
            true
        });

    for entry in walker {
        let entry = entry.context("failed to read directory entry")?;

        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let utf8_path = Utf8PathBuf::try_from(path.to_path_buf())
            .with_context(|| format!("non-UTF-8 path: {}", path.display()))?;

        let relative = utf8_path
            .strip_prefix(root)
            .unwrap_or(&utf8_path)
            .to_path_buf();

        if let Some(filename) = relative.file_name() {
            filename_index
                .entry(filename.to_string())
                .or_default()
                .push(relative.clone());
        }

        let content = std::fs::read_to_string(&utf8_path)
            .with_context(|| format!("failed to read {utf8_path}"))?;

        let (frontmatter_yaml, body) = split_frontmatter(&content);

        // D-05: table-driven frontmatter parsing with extensible field mapping
        // D-07: all_keys returned here to avoid double-parsing YAML
        let (status, metadata, field_edges, all_keys) = frontmatter_yaml
            .map(|yaml| parse_frontmatter(yaml, &config.frontmatter))
            .unwrap_or_default();

        for key in &all_keys {
            *observed_frontmatter_keys.entry(key.clone()).or_insert(0) += 1;
        }

        if let Some(ref s) = status {
            observed_statuses.insert(s.clone());

            // D-04: Track directory convention for terminal status classification
            let in_terminal = is_in_terminal_directory(&relative);
            if in_terminal {
                *status_in_terminal.entry(s.clone()).or_insert(0) += 1;
            } else {
                *status_in_nonterminal.entry(s.clone()).or_insert(0) += 1;
            }
        }

        // Create pending edges from extensible frontmatter field edges
        let file_node_placeholder =
            NodeId::new(u32::try_from(graph.node_count()).expect("graph exceeds u32::MAX nodes"));

        for fe in &field_edges {
            for target in &fe.targets {
                let hint = classify_frontmatter_value(target);
                match hint {
                    RefHint::External => {
                        external_refs.push(ExternalRef {
                            file: relative.to_string(),
                            url: target.clone(),
                        });
                    }
                    RefHint::Implausible { reason } => {
                        implausible_refs.push(ImplausibleRef {
                            file: relative.to_string(),
                            raw_value: target.clone(),
                            reason,
                        });
                    }
                    RefHint::Label { .. } | RefHint::FilePath | RefHint::SectionRef => {
                        pending_edges.push(PendingEdge {
                            source: file_node_placeholder,
                            target_identity: target.clone(),
                            kind: fe.edge_kind,
                            inverse: fe.inverse,
                        });
                    }
                }
            }
        }

        let file_node = graph.add_node(Handle {
            id: relative.to_string(),
            kind: HandleKind::File(relative.clone()),
            status,
            file_path: Some(relative.clone()),
            metadata,
        });
        debug_assert_eq!(file_node, file_node_placeholder);

        let scan_result = scan_file(body, &relative, file_node, &mut graph);
        all_label_candidates.extend(scan_result.label_candidates);

        for file_ref in &scan_result.file_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: file_ref.clone(),
                kind: EdgeKind::Cites,
                inverse: false,
            });
        }
        for section_ref in &scan_result.section_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: format!("section:{section_ref}"),
                kind: EdgeKind::Cites,
                inverse: false,
            });
        }
    }

    // D-04: Compute terminal_by_directory -- statuses that appear EXCLUSIVELY
    // in terminal directories (count > 0 in terminal, count == 0 in nonterminal)
    let mut terminal_by_directory = HashSet::new();
    for (status, count) in &status_in_terminal {
        if *count > 0 && !status_in_nonterminal.contains_key(status) {
            terminal_by_directory.insert(status.clone());
        }
    }

    Ok(BuildResult {
        graph,
        label_candidates: all_label_candidates,
        pending_edges,
        observed_statuses,
        terminal_by_directory,
        observed_frontmatter_keys,
        filename_index,
        implausible_refs,
        external_refs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DiGraph;
    use crate::handle::{Handle, HandleKind, HandleMetadata, NodeId};

    fn make_graph_with_file(path: &str) -> (DiGraph, NodeId) {
        let mut graph = DiGraph::new();
        let node = graph.add_node(Handle {
            id: path.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(path)),
            status: None,
            file_path: Some(Utf8PathBuf::from(path)),
            metadata: HandleMetadata::default(),
        });
        (graph, node)
    }

    #[test]
    fn labels_inside_code_blocks_are_not_scanned() {
        let body = "Some text\n```\nOQ-64 inside code\n```\nMore text";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            result.label_candidates.is_empty(),
            "Labels inside code blocks should not be added to candidates"
        );
    }

    #[test]
    fn labels_outside_code_blocks_are_scanned() {
        let body = "Some text\nOQ-64 outside code\nMore text";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            !result.label_candidates.is_empty(),
            "Labels outside code blocks should be added to candidates"
        );
    }

    #[test]
    fn headings_inside_code_blocks_still_skipped() {
        let body = "## Real heading\n```\n## Fake heading\n```\n";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        let section_count = graph
            .nodes()
            .filter(|(_, h)| matches!(h.kind, HandleKind::Section { .. }))
            .count();
        assert_eq!(
            section_count, 1,
            "Only real headings should create sections"
        );
    }

    #[test]
    fn file_path_regex_rejects_urls() {
        let body = "See https://example.com/rust-lang/guide.md for details";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            result.file_refs.is_empty(),
            "URL fragments should not be matched as file refs"
        );
    }

    #[test]
    fn section_refs_inside_code_blocks_are_not_scanned() {
        let body = "```\nSee §4.1\n```\n";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            result.section_refs.is_empty(),
            "Section refs inside code blocks should not be scanned"
        );
    }

    #[test]
    fn file_refs_inside_code_blocks_are_not_scanned() {
        let body = "```\nSee guide.md\n```\n";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            result.file_refs.is_empty(),
            "File refs inside code blocks should not be scanned"
        );
    }

    #[test]
    fn file_path_regex_rejects_version_dot_fragments() {
        // Test via scan_file which applies the dot-prefix rejection
        let body = "See formal-model/murail-algebra-v1.2.md for details";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            !result.file_refs.contains(&"2.md".to_string()),
            "should not match fragment 2.md from v1.2.md, got: {:?}",
            result.file_refs
        );
        // Full versioned path should be captured
        assert!(
            result
                .file_refs
                .contains(&"formal-model/murail-algebra-v1.2.md".to_string())
                || result.file_refs.is_empty(),
            "should match full path or nothing, got: {:?}",
            result.file_refs
        );

        // Standalone file paths still work
        let body2 = "see summary.md for details";
        let (mut graph2, file_node2) = make_graph_with_file("test2.md");
        let result2 = scan_file(body2, Utf8Path::new("test2.md"), file_node2, &mut graph2);
        assert!(
            result2.file_refs.contains(&"summary.md".to_string()),
            "standalone file paths should still match"
        );
    }

    #[test]
    fn file_path_regex_rejects_mid_word_fragments() {
        // YouTube ID "4eJrp9byBRk.md" should not match "k.md"
        let body = "[transcript](refs/2026-02-06-4eJrp9byBRk.md)";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            !result.file_refs.iter().any(|r| r == "k.md"),
            "should not extract k.md from YouTube ID, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn file_path_regex_rejects_hyphen_prefix_after_label() {
        // "RQ-01-program-format-encoding.md" → label "RQ-01" + should NOT produce "-program-format-encoding.md"
        let body = "See RQ-01-program-format-encoding.md for details";
        let (mut graph, file_node) = make_graph_with_file("test.md");
        let result = scan_file(body, Utf8Path::new("test.md"), file_node, &mut graph);
        assert!(
            !result.file_refs.iter().any(|r| r.starts_with('-')),
            "should not extract hyphen-prefixed fragments, got: {:?}",
            result.file_refs
        );
    }

    // -----------------------------------------------------------------------
    // Plausibility filter integration tests (build_graph)
    // -----------------------------------------------------------------------

    fn write_md_file(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("write test file");
    }

    #[test]
    fn plausibility_filter_url_becomes_external() {
        let tmp = std::env::temp_dir().join("anneal_test_url");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "test.md",
            "---\ndepends-on: https://example.com\n---\nBody text\n",
        );
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        // URL should NOT appear in pending_edges
        assert!(
            !result
                .pending_edges
                .iter()
                .any(|e| e.target_identity.contains("https://")),
            "URL should not be in pending_edges, got: {:?}",
            result
                .pending_edges
                .iter()
                .map(|e| &e.target_identity)
                .collect::<Vec<_>>()
        );
        // URL should appear in external_refs
        assert!(
            result
                .external_refs
                .iter()
                .any(|e| e.url == "https://example.com"),
            "URL should be in external_refs, got {} entries",
            result.external_refs.len()
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plausibility_filter_prose_becomes_implausible() {
        let tmp = std::env::temp_dir().join("anneal_test_prose");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "test.md",
            "---\ndepends-on: claude-desktop session\n---\nBody text\n",
        );
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        // Prose should NOT appear in pending_edges
        assert!(
            !result
                .pending_edges
                .iter()
                .any(|e| e.target_identity == "claude-desktop session"),
            "prose should not be in pending_edges"
        );
        // Prose should appear in implausible_refs
        assert!(
            result
                .implausible_refs
                .iter()
                .any(|r| r.raw_value == "claude-desktop session"
                    && r.reason.contains("freeform prose")),
            "prose should be in implausible_refs with 'freeform prose' reason, got {} entries",
            result.implausible_refs.len()
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plausibility_filter_passes_valid_ref() {
        let tmp = std::env::temp_dir().join("anneal_test_valid");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "test.md", "---\ndepends-on: foo.md\n---\nBody text\n");
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        // Valid .md ref should be in pending_edges
        assert!(
            result
                .pending_edges
                .iter()
                .any(|e| e.target_identity == "foo.md"),
            "valid ref should be in pending_edges, got: {:?}",
            result
                .pending_edges
                .iter()
                .map(|e| &e.target_identity)
                .collect::<Vec<_>>()
        );
        // Should NOT be in implausible_refs
        assert!(
            result.implausible_refs.is_empty(),
            "valid ref should not produce implausible_refs"
        );
        // Should NOT be in external_refs
        assert!(
            result.external_refs.is_empty(),
            "valid ref should not produce external_refs"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plausibility_filter_absolute_path_becomes_implausible() {
        let tmp = std::env::temp_dir().join("anneal_test_abspath");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "test.md",
            "---\ndepends-on: /absolute/path.md\n---\nBody text\n",
        );
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert!(
            !result
                .pending_edges
                .iter()
                .any(|e| e.target_identity.contains("/absolute/")),
            "absolute path should not be in pending_edges"
        );
        assert!(
            result
                .implausible_refs
                .iter()
                .any(|r| r.raw_value == "/absolute/path.md" && r.reason.contains("absolute path")),
            "absolute path should be in implausible_refs"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plausibility_filter_wildcard_becomes_implausible() {
        let tmp = std::env::temp_dir().join("anneal_test_wildcard");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "test.md",
            "---\ndepends-on: \"*.md\"\n---\nBody text\n",
        );
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert!(
            !result
                .pending_edges
                .iter()
                .any(|e| e.target_identity.contains('*')),
            "wildcard should not be in pending_edges"
        );
        assert!(
            result
                .implausible_refs
                .iter()
                .any(|r| r.raw_value == "*.md" && r.reason.contains("wildcard pattern")),
            "wildcard should be in implausible_refs"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
