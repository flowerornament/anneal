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
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
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

/// Five-pattern `RegexSet` for single-pass content scanning (KB-D6, section 5.1).
///
/// Most lines match zero patterns -- the fast path is one automaton pass.
/// Only matching lines trigger individual `Regex` extraction.
static PATTERN_SET: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"^#{1,6}\s",          // 0: section headings
        r"[A-Z][A-Z_]*-\d+",   // 1: label references
        r"§\d+(?:\.\d+)*",     // 2: section cross-references
        r"[a-z0-9_/-]+\.md\b", // 3: file path references
        r"\bv\d+\b",           // 4: version references
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

/// Capture regex for version references.
static VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bv(\d+)\b").expect("version regex must compile"));

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
pub fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
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

/// Parsed representation of YAML frontmatter fields relevant to anneal.
#[derive(Debug, Default)]
pub struct ParsedFrontmatter {
    pub status: Option<String>,
    pub updated: Option<chrono::NaiveDate>,
    pub superseded_by: Option<String>,
    pub depends_on: Vec<String>,
    pub discharges: Vec<String>,
    pub verifies: Vec<String>,
}

/// Parse YAML frontmatter into structured fields.
///
/// Deserializes as `serde_yaml_ng::Value` first to handle arbitrary fields
/// and YAML type coercion (Pitfall 8). On parse failure, returns defaults
/// with a warning -- never errors (Pitfall 2).
pub fn parse_frontmatter(yaml: &str) -> ParsedFrontmatter {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return ParsedFrontmatter::default();
    };

    let Some(mapping) = value.as_mapping() else {
        return ParsedFrontmatter::default();
    };

    let mut fm = ParsedFrontmatter::default();

    // Helper to look up a key in the mapping
    let get = |key: &str| mapping.get(serde_yaml_ng::Value::String(key.to_string()));

    // status: extract as string, handling YAML type coercion
    if let Some(v) = get("status") {
        fm.status = yaml_value_to_string(v);
    }

    // updated: parse as NaiveDate
    if let Some(s) = get("updated").and_then(yaml_value_to_string) {
        fm.updated = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok();
    }

    // superseded-by: note hyphen
    if let Some(v) = get("superseded-by") {
        fm.superseded_by = yaml_value_to_string(v);
    }

    // depends-on: string or list of strings
    if let Some(v) = get("depends-on") {
        fm.depends_on = yaml_value_to_string_vec(v);
    }

    // discharges: string or list of strings
    if let Some(v) = get("discharges") {
        fm.discharges = yaml_value_to_string_vec(v);
    }

    // verifies: string or list of strings
    if let Some(v) = get("verifies") {
        fm.verifies = yaml_value_to_string_vec(v);
    }

    fm
}

/// Convert a YAML value to a string, handling type coercion.
fn yaml_value_to_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Convert a YAML value to a vec of strings (handles both single value and list).
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
pub struct LabelCandidate {
    pub prefix: String,
    pub number: u32,
    pub file_path: Utf8PathBuf,
    pub edge_kind: EdgeKind,
}

/// An edge whose target is identified by string (not yet resolved to a `NodeId`).
///
/// Frontmatter fields like `depends-on: OQ-64` reference targets that may not
/// have been scanned yet. Resolution to actual `NodeId` values happens in
/// `resolve.rs` (Plan 03).
pub struct PendingEdge {
    pub source: NodeId,
    pub target_identity: String,
    pub kind: EdgeKind,
}

/// Result of scanning a single file's body content.
pub struct ScanResult {
    pub label_candidates: Vec<LabelCandidate>,
    pub section_refs: Vec<String>,
    pub file_refs: Vec<String>,
    pub version_refs: Vec<u32>,
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
pub fn scan_file(
    body: &str,
    file_path: &Utf8Path,
    file_node: NodeId,
    graph: &mut DiGraph,
) -> ScanResult {
    let mut result = ScanResult {
        label_candidates: Vec::new(),
        section_refs: Vec::new(),
        file_refs: Vec::new(),
        version_refs: Vec::new(),
    };

    let mut in_code_block = false;

    for line in body.lines() {
        // Track fenced code blocks (Pitfall 3)
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // Check which patterns match this line
        let matches: Vec<usize> = PATTERN_SET.matches(line).into_iter().collect();

        if matches.is_empty() {
            continue;
        }

        // Pattern 0: Section headings (skip inside code blocks)
        if !in_code_block
            && matches.contains(&0)
            && let Some(caps) = HEADING_RE.captures(line)
        {
            let heading = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();
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

        // Infer edge kind from body-text keywords for this line
        let line_edge_kind = infer_edge_kind_from_line(line);

        // Pattern 1: Label references (collected for namespace inference)
        if matches.contains(&1) {
            for caps in LABEL_RE.captures_iter(line) {
                let prefix = caps.get(1).map_or("", |m| m.as_str()).to_string();
                let number_str = caps.get(2).map_or("0", |m| m.as_str());
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

        // Pattern 2: Section cross-references
        if matches.contains(&2) {
            for caps in SECTION_REF_RE.captures_iter(line) {
                let section_num = caps.get(1).map_or("", |m| m.as_str()).to_string();
                if !section_num.is_empty() {
                    result.section_refs.push(section_num);
                }
            }
        }

        // Pattern 3: File path references
        if matches.contains(&3) {
            for caps in FILE_PATH_RE.captures_iter(line) {
                let path = caps.get(1).map_or("", |m| m.as_str()).to_string();
                if !path.is_empty() {
                    result.file_refs.push(path);
                }
            }
        }

        // Pattern 4: Version references
        if matches.contains(&4) {
            for caps in VERSION_RE.captures_iter(line) {
                let ver_str = caps.get(1).map_or("0", |m| m.as_str());
                if let Ok(ver) = ver_str.parse::<u32>() {
                    result.version_refs.push(ver);
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
pub fn infer_root(cwd: &Utf8Path) -> Utf8PathBuf {
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
// Directory exclusion
// ---------------------------------------------------------------------------

/// Check whether a directory entry should be excluded from scanning.
fn is_excluded(entry: &walkdir::DirEntry, extra_exclusions: &[String]) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return true; // non-UTF-8 names are excluded
    };

    if entry.file_type().is_dir() {
        // Default exclusions
        if DEFAULT_EXCLUSIONS.contains(&name) {
            return true;
        }

        // Hidden directories (except .design)
        if name.starts_with('.') && name != ".design" {
            return true;
        }

        // User-configured exclusions
        if extra_exclusions.iter().any(|ex| ex == name) {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Graph construction
// ---------------------------------------------------------------------------

/// Build the knowledge graph from a directory of markdown files.
///
/// Walks the directory tree rooted at `root`, creates File handles for each
/// `.md` file, scans content with the 5-pattern `RegexSet`, and collects
/// label candidates and pending edges for later resolution.
///
/// Returns a 3-tuple:
/// - The populated `DiGraph`
/// - All `LabelCandidate` instances across files (for namespace inference)
/// - All `PendingEdge` instances from frontmatter fields (for resolution)
pub fn build_graph(
    root: &Utf8Path,
    config: &AnnealConfig,
) -> Result<(DiGraph, Vec<LabelCandidate>, Vec<PendingEdge>)> {
    let mut graph = DiGraph::new();
    let mut all_label_candidates = Vec::new();
    let mut pending_edges = Vec::new();
    let mut observed_statuses = HashSet::new();

    let extra_exclusions = &config.exclude;

    let walker = WalkDir::new(root.as_std_path())
        .into_iter()
        .filter_entry(|e| {
            // We need to capture extra_exclusions for the closure, but filter_entry
            // doesn't allow easy access to our config. Instead, we inline the check.
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
                // Note: extra_exclusions checked below per-file since filter_entry
                // captures are limited. Directory exclusions are still effective via
                // the is_excluded helper for non-walkdir paths.
            }

            true
        });

    for entry in walker {
        let entry = entry.context("failed to read directory entry")?;

        // Skip directories
        if entry.file_type().is_dir() {
            continue;
        }

        // Only process .md files
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        // Check extra exclusions on parent directories
        if let Some(parent) = path.parent() {
            let skip = parent.ancestors().any(|ancestor| {
                ancestor
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| extra_exclusions.iter().any(|ex| ex == name))
            });
            if skip {
                continue;
            }
        }

        // Convert to Utf8Path
        let utf8_path = Utf8PathBuf::try_from(path.to_path_buf())
            .with_context(|| format!("non-UTF-8 path: {}", path.display()))?;

        // Make path relative to root for identity
        let relative = utf8_path
            .strip_prefix(root)
            .unwrap_or(&utf8_path)
            .to_path_buf();

        // Read file content
        let content = std::fs::read_to_string(&utf8_path)
            .with_context(|| format!("failed to read {utf8_path}"))?;

        // Split frontmatter
        let (frontmatter_yaml, body) = split_frontmatter(&content);

        // Parse frontmatter
        let fm = frontmatter_yaml.map_or_else(ParsedFrontmatter::default, parse_frontmatter);

        // Track observed statuses for lattice inference
        if let Some(ref status) = fm.status {
            observed_statuses.insert(status.clone());
        }

        // Create File handle
        let handle = Handle {
            id: relative.to_string(),
            kind: HandleKind::File(relative.clone()),
            status: fm.status,
            file_path: Some(relative.clone()),
            metadata: HandleMetadata {
                updated: fm.updated,
                superseded_by: fm.superseded_by.clone(),
                depends_on: fm.depends_on.clone(),
                discharges: fm.discharges.clone(),
                verifies: fm.verifies.clone(),
            },
        };

        let file_node = graph.add_node(handle);

        // Create pending edges from frontmatter fields
        if let Some(ref target) = fm.superseded_by {
            // superseded-by: the *target* supersedes *this* file
            // So the edge goes from the superseding file to this one
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: target.clone(),
                kind: EdgeKind::Supersedes,
            });
        }

        for target in &fm.depends_on {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: target.clone(),
                kind: EdgeKind::DependsOn,
            });
        }

        for target in &fm.discharges {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: target.clone(),
                kind: EdgeKind::Discharges,
            });
        }

        for target in &fm.verifies {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: target.clone(),
                kind: EdgeKind::Verifies,
            });
        }

        // Scan body content
        let scan_result = scan_file(body, &relative, file_node, &mut graph);

        all_label_candidates.extend(scan_result.label_candidates);

        // Collect file refs as pending edges
        for file_ref in &scan_result.file_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: file_ref.clone(),
                kind: EdgeKind::Cites,
            });
        }

        // Collect section refs as pending edges
        for section_ref in &scan_result.section_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: format!("section:{section_ref}"),
                kind: EdgeKind::Cites,
            });
        }
    }

    Ok((graph, all_label_candidates, pending_edges))
}
