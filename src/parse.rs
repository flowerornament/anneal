use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use regex::{Regex, RegexSet};
use walkdir::WalkDir;

use crate::config::AnnealConfig;
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

/// Parse YAML frontmatter into a status and `HandleMetadata`.
///
/// Deserializes as `serde_yaml_ng::Value` to handle arbitrary fields and
/// YAML type coercion. On parse failure, returns defaults (never errors).
pub(crate) fn parse_frontmatter(yaml: &str) -> (Option<String>, HandleMetadata) {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return (None, HandleMetadata::default());
    };

    let Some(mapping) = value.as_mapping() else {
        return (None, HandleMetadata::default());
    };

    let get = |key: &str| mapping.get(serde_yaml_ng::Value::String(key.to_string()));

    let status = get("status").and_then(yaml_value_to_string);
    let updated = get("updated")
        .and_then(yaml_value_to_string)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());
    let superseded_by = get("superseded-by").and_then(yaml_value_to_string);
    // depends-on, discharges, verifies: each accepts a string or list of strings
    let depends_on = get("depends-on").map_or_else(Vec::new, yaml_value_to_string_vec);
    let discharges = get("discharges").map_or_else(Vec::new, yaml_value_to_string_vec);
    let verifies = get("verifies").map_or_else(Vec::new, yaml_value_to_string_vec);

    let metadata = HandleMetadata {
        updated,
        superseded_by,
        depends_on,
        discharges,
        verifies,
    };

    (status, metadata)
}

fn yaml_value_to_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
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
/// `resolve.rs` (Plan 03).
pub(crate) struct PendingEdge {
    pub(crate) source: NodeId,
    pub(crate) target_identity: String,
    pub(crate) kind: EdgeKind,
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

        let matched = PATTERN_SET.matches(line);
        if !matched.matched_any() {
            continue;
        }

        if !in_code_block
            && matched.matched(PAT_HEADING)
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
            for caps in FILE_PATH_RE.captures_iter(line) {
                let path = caps
                    .get(1)
                    .expect("file path capture always present")
                    .as_str()
                    .to_string();
                if !path.is_empty() {
                    result.file_refs.push(path);
                }
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

/// Result of `build_graph`: the populated graph, label candidates for namespace
/// inference, pending edges for resolution, and observed status values for
/// lattice inference.
pub(crate) struct BuildResult {
    pub(crate) graph: DiGraph,
    pub(crate) label_candidates: Vec<LabelCandidate>,
    pub(crate) pending_edges: Vec<PendingEdge>,
    pub(crate) observed_statuses: HashSet<String>,
}

/// Build the knowledge graph from a directory of markdown files.
///
/// Walks the directory tree, creates File handles, scans content with
/// the 5-pattern `RegexSet`, and collects label candidates and pending
/// edges for later resolution.
pub(crate) fn build_graph(root: &Utf8Path, config: &AnnealConfig) -> Result<BuildResult> {
    let mut graph = DiGraph::new();
    let mut all_label_candidates = Vec::new();
    let mut pending_edges = Vec::new();
    let mut observed_statuses = HashSet::new();

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

        let content = std::fs::read_to_string(&utf8_path)
            .with_context(|| format!("failed to read {utf8_path}"))?;

        let (frontmatter_yaml, body) = split_frontmatter(&content);

        let (status, metadata) = frontmatter_yaml.map(parse_frontmatter).unwrap_or_default();

        if let Some(ref s) = status {
            observed_statuses.insert(s.clone());
        }

        // Create pending edges from frontmatter relationship fields
        let file_node_placeholder =
            NodeId::new(u32::try_from(graph.node_count()).expect("graph exceeds u32::MAX nodes"));
        if let Some(ref target) = metadata.superseded_by {
            pending_edges.push(PendingEdge {
                source: file_node_placeholder,
                target_identity: target.clone(),
                kind: EdgeKind::Supersedes,
            });
        }
        let mut push_edges = |targets: &[String], kind: EdgeKind| {
            for target in targets {
                pending_edges.push(PendingEdge {
                    source: file_node_placeholder,
                    target_identity: target.clone(),
                    kind,
                });
            }
        };
        push_edges(&metadata.depends_on, EdgeKind::DependsOn);
        push_edges(&metadata.discharges, EdgeKind::Discharges);
        push_edges(&metadata.verifies, EdgeKind::Verifies);

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
            });
        }
        for section_ref in &scan_result.section_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: format!("section:{section_ref}"),
                kind: EdgeKind::Cites,
            });
        }
    }

    Ok(BuildResult {
        graph,
        label_candidates: all_label_candidates,
        pending_edges,
        observed_statuses,
    })
}
