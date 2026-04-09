use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;
use walkdir::WalkDir;

use crate::config::{AnnealConfig, Direction, FrontmatterConfig};
use crate::extraction::{
    DiscoveredRef, FileExtraction, ImplausibleReason, LineIndex, RefHint, RefSource, SourceSpan,
    classify_frontmatter_value, extract_file_snippet_from_body, extract_label_snippet_from_content,
};
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

/// Capture regex for label references: prefix and number.
static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Captures compound prefixes like ST-OQ from ST-OQ-1, as well as simple OQ from OQ-1.
    Regex::new(r"([A-Z][A-Z_]*(?:-[A-Z][A-Z_]*)*)-(\d+)").expect("label regex must compile")
});

/// Capture regex for section cross-references (paragraph sign).
static SECTION_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"§(\d+(?:\.\d+)*)").expect("section ref regex must compile"));

/// Capture regex for file path references.
static FILE_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-z0-9_/-]+\.md)\b").expect("file path regex must compile"));

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

/// Parsed frontmatter result: status, metadata, field edges, all keys, and parse
/// success (D-05, D-07).
#[derive(Default)]
pub(crate) struct FrontmatterParseResult {
    pub(crate) status: Option<String>,
    pub(crate) metadata: HandleMetadata,
    pub(crate) field_edges: Vec<FrontmatterEdge>,
    /// All frontmatter keys, for init auto-detection (D-07).
    pub(crate) all_keys: Vec<String>,
    /// `true` when YAML deserialization failed (section 7.2 silent-failure tracking).
    pub(crate) yaml_failed: bool,
}

/// Parse YAML frontmatter into a status, `HandleMetadata`, and extensible field edges (D-05).
///
/// Deserializes as `serde_yaml_ng::Value` to handle arbitrary fields and
/// YAML type coercion. On parse failure, returns defaults with `yaml_failed`
/// set to `true` so callers can track files with malformed YAML.
/// The `config` parameter drives which frontmatter keys produce edges.
pub(crate) fn parse_frontmatter(yaml: &str, config: &FrontmatterConfig) -> FrontmatterParseResult {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return FrontmatterParseResult {
            yaml_failed: true,
            ..FrontmatterParseResult::default()
        };
    };

    let Some(mapping) = value.as_mapping() else {
        return FrontmatterParseResult {
            yaml_failed: true,
            ..FrontmatterParseResult::default()
        };
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
            let edge_kind = EdgeKind::from_name(&field_mapping.edge_kind);
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

    FrontmatterParseResult {
        status,
        metadata,
        field_edges,
        all_keys,
        yaml_failed: false,
    }
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
    /// 1-based line number in the source file. Frontmatter edges use line 1 as
    /// pragmatic fallback; body edges carry the line from the cmark scanner.
    pub(crate) line: Option<u32>,
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
    /// Section references with their 1-based line numbers.
    pub(crate) section_refs: Vec<(String, u32)>,
    /// File path references with their 1-based line numbers.
    pub(crate) file_refs: Vec<(String, u32)>,
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

struct BodyRefRecorder<'a> {
    file_path: &'a Utf8Path,
    file_path_str: &'a str,
    line: u32,
    result: &'a mut ScanResult,
    discovered_refs: &'a mut Vec<DiscoveredRef>,
}

impl BodyRefRecorder<'_> {
    fn record(&mut self, raw: &str, hint: RefHint, edge_kind: &EdgeKind) {
        let discovered_edge_kind = match &hint {
            RefHint::Label { prefix, number } => {
                self.result.label_candidates.push(LabelCandidate {
                    prefix: prefix.clone(),
                    number: *number,
                    file_path: self.file_path.to_path_buf(),
                    edge_kind: edge_kind.clone(),
                });
                edge_kind.clone()
            }
            RefHint::FilePath => {
                self.result.file_refs.push((raw.to_string(), self.line));
                edge_kind.clone()
            }
            RefHint::SectionRef => {
                let section_num = raw
                    .strip_prefix("section:")
                    .or_else(|| raw.strip_prefix('§'))
                    .unwrap_or(raw);
                self.result
                    .section_refs
                    .push((section_num.to_string(), self.line));
                EdgeKind::Cites
            }
            RefHint::External | RefHint::Implausible { .. } => return,
        };

        self.discovered_refs.push(DiscoveredRef {
            raw: raw.to_string(),
            hint,
            source: RefSource::Body,
            edge_kind: discovered_edge_kind,
            inverse: false,
            span: Some(SourceSpan {
                file: self.file_path_str.to_string(),
                line: self.line,
            }),
        });
    }
}

// ---------------------------------------------------------------------------
// Content scanner (pulldown-cmark)
// ---------------------------------------------------------------------------

/// Scan a file's body content for handles and references using pulldown-cmark.
/// Uses pulldown-cmark's event stream for structural markdown parsing.
/// Code blocks and inline code are structurally skipped (no regex toggling).
/// Markdown links and wiki-links are extracted from Link events. HTML blocks
/// are scanned with regex patterns. Text events within the same block element
/// are concatenated before regex matching.
///
/// Returns both a `ScanResult` (for backward compat) and a `Vec<DiscoveredRef>`
/// for the new typed extraction pipeline.
pub(crate) fn scan_file_cmark(
    body: &str,
    file_path: &Utf8Path,
    file_node: NodeId,
    graph: &mut DiGraph,
    line_index: &LineIndex,
) -> (ScanResult, Vec<DiscoveredRef>) {
    let mut result = ScanResult {
        label_candidates: Vec::new(),
        section_refs: Vec::new(),
        file_refs: Vec::new(),
    };
    let mut discovered_refs: Vec<DiscoveredRef> = Vec::new();

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts.insert(Options::ENABLE_WIKILINKS);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(body, opts);

    // State tracking
    let mut in_code_block = false;
    let mut heading_text: Option<String> = None;
    let mut text_accumulator = String::new();
    let mut block_start_offset: usize = 0;
    let mut in_html_block = false;
    let mut html_accumulator = String::new();
    let mut html_block_start_offset: usize = 0;

    let file_path_str = file_path.as_str();

    for (event, range) in parser.into_offset_iter() {
        #[allow(clippy::match_same_arms)] // Code/Math arms intentionally explicit for documentation
        match event {
            // -- Code block: skip everything inside --
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
            }

            // Inline code spans and display/inline math: skip entirely.
            // These are listed explicitly (not in the wildcard) to document
            // that they are intentionally unsearched, unlike other text-bearing events.
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}

            // -- HTML blocks: accumulate and scan with regex --
            Event::Start(Tag::HtmlBlock) => {
                in_html_block = true;
                html_accumulator.clear();
                html_block_start_offset = range.start;
            }
            Event::End(TagEnd::HtmlBlock) => {
                in_html_block = false;
                if !html_accumulator.is_empty() {
                    scan_text_for_refs(
                        &html_accumulator,
                        html_block_start_offset,
                        file_path,
                        file_path_str,
                        line_index,
                        &mut result,
                        &mut discovered_refs,
                    );
                    html_accumulator.clear();
                }
            }

            // -- Headings --
            Event::Start(Tag::Heading { .. }) => {
                heading_text = Some(String::new());
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading_text.take() {
                    let heading = heading.trim().to_string();
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
            }

            // -- Links (markdown and wiki-links) --
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                if in_code_block {
                    continue;
                }
                let dest = dest_url.as_ref();
                let offset = range.start;
                let line = line_index.offset_to_line(offset);

                match link_type {
                    LinkType::WikiLink { .. } => {
                        // Wiki-links: [[target]] - dest_url contains the target
                        if !dest.is_empty() {
                            BodyRefRecorder {
                                file_path,
                                file_path_str,
                                line,
                                result: &mut result,
                                discovered_refs: &mut discovered_refs,
                            }
                            .record(
                                dest,
                                classify_body_ref(dest),
                                &EdgeKind::Cites,
                            );
                        }
                    }
                    _ => {
                        // Standard links: [text](target.md)
                        if !dest.is_empty()
                            && !dest.starts_with('#')
                            && !dest.starts_with("http://")
                            && !dest.starts_with("https://")
                            && !dest.starts_with("mailto:")
                        {
                            // Strip fragment identifiers from file paths
                            let clean_dest = if let Some(pos) = dest.find('#') {
                                &dest[..pos]
                            } else {
                                dest
                            };
                            if !clean_dest.is_empty() {
                                BodyRefRecorder {
                                    file_path,
                                    file_path_str,
                                    line,
                                    result: &mut result,
                                    discovered_refs: &mut discovered_refs,
                                }
                                .record(
                                    clean_dest,
                                    classify_body_ref(clean_dest),
                                    &EdgeKind::Cites,
                                );
                            }
                        }
                    }
                }
            }

            // -- Block element boundaries --
            Event::Start(Tag::Paragraph | Tag::Item | Tag::BlockQuote(_) | Tag::TableCell) => {
                if !in_code_block {
                    text_accumulator.clear();
                    block_start_offset = range.start;
                }
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Item | TagEnd::BlockQuote(_) | TagEnd::TableCell,
            ) => {
                if !in_code_block && !text_accumulator.is_empty() {
                    scan_text_for_refs(
                        &text_accumulator,
                        block_start_offset,
                        file_path,
                        file_path_str,
                        line_index,
                        &mut result,
                        &mut discovered_refs,
                    );
                    text_accumulator.clear();
                }
            }

            // -- Text events: accumulate for block-level scanning --
            Event::Text(text) => {
                if in_code_block {
                    continue;
                }
                if in_html_block {
                    html_accumulator.push_str(text.as_ref());
                    continue;
                }
                if let Some(ref mut h) = heading_text {
                    h.push_str(text.as_ref());
                }
                text_accumulator.push_str(text.as_ref());
            }

            Event::Html(html) => {
                if in_html_block {
                    html_accumulator.push_str(html.as_ref());
                } else {
                    // Standalone HTML line outside HtmlBlock
                    scan_text_for_refs(
                        html.as_ref(),
                        range.start,
                        file_path,
                        file_path_str,
                        line_index,
                        &mut result,
                        &mut discovered_refs,
                    );
                }
            }

            Event::InlineHtml(html) => {
                if !in_code_block {
                    scan_text_for_refs(
                        html.as_ref(),
                        range.start,
                        file_path,
                        file_path_str,
                        line_index,
                        &mut result,
                        &mut discovered_refs,
                    );
                }
            }

            Event::SoftBreak | Event::HardBreak => {
                if let Some(ref mut h) = heading_text {
                    h.push(' ');
                }
                text_accumulator.push(' ');
            }

            // All other events: skip
            _ => {}
        }
    }

    // Flush any remaining accumulated text (e.g., body without block wrappers)
    if !text_accumulator.is_empty() && !in_code_block {
        scan_text_for_refs(
            &text_accumulator,
            block_start_offset,
            file_path,
            file_path_str,
            line_index,
            &mut result,
            &mut discovered_refs,
        );
    }

    (result, discovered_refs)
}

/// Classify a body text reference into a `RefHint`.
///
/// Unlike `classify_frontmatter_value`, this does not check for prose or
/// comma lists since body refs come from regex matches on specific patterns.
fn classify_body_ref(value: &str) -> RefHint {
    // Label pattern
    if let Some(caps) = LABEL_RE.captures(value)
        && let Ok(number) = caps[2].parse::<u32>()
    {
        return RefHint::Label {
            prefix: caps[1].to_string(),
            number,
        };
    }

    // Section ref
    if value.starts_with("section:") {
        return RefHint::SectionRef;
    }

    // File path (.md extension)
    if std::path::Path::new(value)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return RefHint::FilePath;
    }

    // Default: FilePath (handle identity)
    RefHint::FilePath
}

/// Scan accumulated text for labels, section refs, file paths using the same
/// regex patterns as the old scanner. Creates both `LabelCandidate`/ScanResult
/// entries and `DiscoveredRef` entries.
fn scan_text_for_refs(
    text: &str,
    block_start_offset: usize,
    file_path: &Utf8Path,
    file_path_str: &str,
    line_index: &LineIndex,
    result: &mut ScanResult,
    discovered_refs: &mut Vec<DiscoveredRef>,
) {
    let line = line_index.offset_to_line(block_start_offset);
    let mut recorder = BodyRefRecorder {
        file_path,
        file_path_str,
        line,
        result,
        discovered_refs,
    };

    // Edge kind inference from full accumulated text
    let edge_kind = infer_edge_kind_from_line(text);

    // Labels
    for caps in LABEL_RE.captures_iter(text) {
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
            recorder.record(
                &format!("{prefix}-{number}"),
                RefHint::Label { prefix, number },
                &edge_kind,
            );
        }
    }

    // Section refs
    for caps in SECTION_REF_RE.captures_iter(text) {
        let section_num = caps
            .get(1)
            .expect("section ref capture always present")
            .as_str()
            .to_string();
        if !section_num.is_empty() {
            recorder.record(&format!("§{section_num}"), RefHint::SectionRef, &edge_kind);
        }
    }

    // File paths
    for m in FILE_PATH_RE.find_iter(text) {
        let prefix = &text[..m.start()];
        if prefix.contains("://") {
            continue;
        }
        if m.start() > 0 && text.as_bytes()[m.start() - 1] == b'.' {
            continue;
        }
        let path = m.as_str();
        if path.starts_with('-') {
            continue;
        }
        if m.start() > 0 && text.as_bytes()[m.start() - 1].is_ascii_alphanumeric() {
            continue;
        }
        recorder.record(path, RefHint::FilePath, &edge_kind);
    }
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
    pub(crate) reason: ImplausibleReason,
    pub(crate) line: Option<u32>,
}

/// An external URL found in frontmatter.
pub(crate) struct ExternalRef {
    #[allow(dead_code)]
    pub(crate) file: String,
    #[allow(dead_code)]
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
    /// Per-file typed extraction output (populated alongside existing PendingEdge flow).
    pub(crate) extractions: Vec<FileExtraction>,
    /// Precomputed snippets for file handles, keyed by relative file path.
    pub(crate) file_snippets: HashMap<String, String>,
    /// Precomputed snippets for label handles, keyed by label identity.
    pub(crate) label_snippets: HashMap<String, String>,
    /// Files whose YAML frontmatter failed to deserialize (§7.2 silent-failure tracking).
    #[allow(dead_code)] // Consumed by status/check reporting once surfaced
    pub(crate) malformed_frontmatter: Vec<String>,
    /// Count of directory entries skipped because their filename was not valid UTF-8 (§7.3).
    #[allow(dead_code)] // Consumed by status/check reporting once surfaced
    pub(crate) skipped_non_utf8: usize,
}

/// Build the knowledge graph from a directory of markdown files.
///
/// Walks the directory tree, creates File handles, scans content with
/// pulldown-cmark, and collects label candidates and pending edges for
/// later resolution.
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
    let mut external_nodes: HashMap<String, NodeId> = HashMap::new();
    let mut extractions: Vec<FileExtraction> = Vec::new();
    let mut file_snippets: HashMap<String, String> = HashMap::new();
    let mut label_snippets: HashMap<String, String> = HashMap::new();
    let mut malformed_frontmatter: Vec<String> = Vec::new();

    // §7.3: Track non-UTF-8 filenames silently skipped by the walker filter.
    // Uses Cell so the FnMut closure can increment without &mut self conflicts.
    let skipped_non_utf8: Cell<usize> = Cell::new(0);

    let extra_exclusions = &config.exclude;

    let walker = WalkDir::new(root.as_std_path())
        .into_iter()
        .filter_entry(|e| {
            let Some(name) = e.file_name().to_str() else {
                skipped_non_utf8.set(skipped_non_utf8.get() + 1);
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
        if let Some(snippet) = extract_file_snippet_from_body(body) {
            file_snippets.insert(relative.to_string(), snippet);
        }

        // Compute frontmatter line count for LineIndex offset calculation.
        // LineIndex::from_content expects the opening --- plus yaml content lines;
        // it adds the closing --- itself via +1 in base_line.
        #[allow(clippy::cast_possible_truncation)] // frontmatter line count won't exceed u32::MAX
        let frontmatter_line_count = frontmatter_yaml.map_or(0, |yaml| {
            // +1 for the opening --- line only (closing --- handled by LineIndex)
            yaml.lines().count() as u32 + 1
        });

        // D-05: table-driven frontmatter parsing with extensible field mapping
        // D-07: all_keys returned here to avoid double-parsing YAML
        let fm = frontmatter_yaml
            .map(|yaml| parse_frontmatter(yaml, &config.frontmatter))
            .unwrap_or_default();

        let FrontmatterParseResult {
            status,
            metadata,
            field_edges,
            all_keys,
            yaml_failed,
        } = fm;

        // §7.2: Track files whose YAML frontmatter was present but malformed.
        if yaml_failed {
            malformed_frontmatter.push(relative.to_string());
        }

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
        let mut file_external_targets: Vec<String> = Vec::new();

        // Cache classify_frontmatter_value results so the second loop can reuse them.
        let mut hint_cache: HashMap<&str, RefHint> = HashMap::new();
        for fe in &field_edges {
            for target in &fe.targets {
                hint_cache
                    .entry(target.as_str())
                    .or_insert_with(|| classify_frontmatter_value(target));
            }
        }

        for fe in &field_edges {
            for target in &fe.targets {
                let hint = &hint_cache[target.as_str()];
                match hint {
                    RefHint::External => {
                        external_refs.push(ExternalRef {
                            file: relative.to_string(),
                            url: target.clone(),
                        });
                        file_external_targets.push(target.clone());
                    }
                    RefHint::Implausible { reason } => {
                        implausible_refs.push(ImplausibleRef {
                            file: relative.to_string(),
                            raw_value: target.clone(),
                            reason: *reason,
                            line: None,
                        });
                    }
                    RefHint::Label { .. } | RefHint::FilePath | RefHint::SectionRef => {
                        pending_edges.push(PendingEdge {
                            source: file_node_placeholder,
                            target_identity: target.clone(),
                            kind: fe.edge_kind.clone(),
                            inverse: fe.inverse,
                            line: Some(1),
                        });
                    }
                }
            }
        }

        let file_node = graph.add_node(Handle {
            id: relative.to_string(),
            kind: HandleKind::File(relative.clone()),
            status: status.clone(),
            file_path: Some(relative.clone()),
            metadata: metadata.clone(),
        });
        assert_eq!(
            file_node, file_node_placeholder,
            "node insertion order changed between placeholder computation and add_node"
        );

        for target in file_external_targets {
            let external_node = if let Some(existing) = external_nodes.get(&target).copied() {
                existing
            } else {
                let node_id = graph.add_node(Handle {
                    id: target.clone(),
                    kind: HandleKind::External {
                        url: target.clone(),
                    },
                    status: None,
                    file_path: Some(relative.clone()),
                    metadata: HandleMetadata::default(),
                });
                external_nodes.insert(target.clone(), node_id);
                node_id
            };

            graph.add_edge(file_node, external_node, EdgeKind::Cites);
        }

        // Use pulldown-cmark scanner for production body scanning
        let line_index = LineIndex::from_content(body, frontmatter_line_count);
        let (scan_result, body_refs) =
            scan_file_cmark(body, &relative, file_node, &mut graph, &line_index);

        let mut seen_label_ids = HashSet::new();
        for candidate in &scan_result.label_candidates {
            let label_id = format!("{}-{}", candidate.prefix, candidate.number);
            if !seen_label_ids.insert(label_id.clone()) || label_snippets.contains_key(&label_id) {
                continue;
            }
            if let Some(snippet) = extract_label_snippet_from_content(&content, &label_id) {
                label_snippets.insert(label_id, snippet);
            }
        }
        all_label_candidates.extend(scan_result.label_candidates);

        // Build FileExtraction with DiscoveredRef for frontmatter + body refs
        let mut discovered_refs = Vec::new();
        for fe in &field_edges {
            for target in &fe.targets {
                let hint = hint_cache[target.as_str()].clone();
                discovered_refs.push(DiscoveredRef {
                    raw: target.clone(),
                    hint,
                    source: RefSource::Frontmatter {
                        field: fe.edge_kind.as_str().to_string(),
                    },
                    edge_kind: fe.edge_kind.clone(),
                    inverse: fe.inverse,
                    span: None,
                });
            }
        }
        discovered_refs.extend(body_refs);
        extractions.push(FileExtraction {
            file: relative.to_string(),
            status,
            metadata,
            refs: discovered_refs,
            all_keys: all_keys.clone(),
        });

        for (file_ref, line) in &scan_result.file_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: file_ref.clone(),
                kind: EdgeKind::Cites,
                inverse: false,
                line: Some(*line),
            });
        }
        for (section_ref, line) in &scan_result.section_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: format!("section:{section_ref}"),
                kind: EdgeKind::Cites,
                inverse: false,
                line: Some(*line),
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
        extractions,
        file_snippets,
        label_snippets,
        malformed_frontmatter,
        skipped_non_utf8: skipped_non_utf8.get(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DiGraph;
    use crate::handle::{Handle, HandleKind, HandleMetadata};

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
        // URL should appear in extraction as RefHint::External
        assert_eq!(result.extractions.len(), 1);
        let ext = &result.extractions[0];
        assert_eq!(ext.refs.len(), 1);
        assert_eq!(ext.refs[0].raw, "https://example.com");
        assert_eq!(ext.refs[0].hint, RefHint::External);

        let external_nodes: Vec<_> = result
            .graph
            .nodes()
            .filter(|(_, handle)| matches!(handle.kind, HandleKind::External { .. }))
            .collect();
        assert_eq!(external_nodes.len(), 1, "expected one external handle node");
        assert_eq!(external_nodes[0].1.id, "https://example.com");

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
                    && r.reason == ImplausibleReason::FreeformProse),
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
                .any(|r| r.raw_value == "/absolute/path.md"
                    && r.reason == ImplausibleReason::AbsolutePath),
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
                .any(|r| r.raw_value == "*.md" && r.reason == ImplausibleReason::WildcardPattern),
            "wildcard should be in implausible_refs"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_graph_populates_file_extraction() {
        let tmp = std::env::temp_dir().join("anneal_test_extraction");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_md_file(
            &tmp,
            "source.md",
            "---\nstatus: draft\ndepends-on: target.md\n---\nBody text.\n",
        );
        write_md_file(
            &tmp,
            "target.md",
            "---\nstatus: active\n---\nTarget body.\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        // Must have extractions for both files
        assert_eq!(
            result.extractions.len(),
            2,
            "should have one FileExtraction per file, got {}",
            result.extractions.len()
        );

        // Find the source.md extraction (it has the depends-on reference)
        let source_ext = result
            .extractions
            .iter()
            .find(|e| e.status.as_deref() == Some("draft"))
            .expect("should find extraction for source.md (status=draft)");

        // source.md should have one DiscoveredRef for "target.md"
        assert_eq!(
            source_ext.refs.len(),
            1,
            "source.md should have 1 discovered ref, got {}",
            source_ext.refs.len()
        );

        let ref0 = &source_ext.refs[0];
        assert_eq!(ref0.raw, "target.md", "raw value should be target.md");
        assert_eq!(
            ref0.hint,
            RefHint::FilePath,
            "target.md should classify as FilePath"
        );
        assert!(!ref0.inverse, "forward edge should not be inverse");

        // Check RefSource is Frontmatter
        match &ref0.source {
            RefSource::Frontmatter { field } => {
                assert!(!field.is_empty(), "field name should not be empty");
            }
            RefSource::Body => panic!("frontmatter ref should have Frontmatter source, not Body"),
        }

        // target.md extraction should have no refs (no frontmatter edges)
        let target_ext = result
            .extractions
            .iter()
            .find(|e| e.status.as_deref() == Some("active"))
            .expect("should find extraction for target.md (status=active)");
        assert!(
            target_ext.refs.is_empty(),
            "target.md should have 0 discovered refs"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_graph_extraction_includes_all_ref_types() {
        let tmp = std::env::temp_dir().join("anneal_test_extraction_types");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_md_file(
            &tmp,
            "multi.md",
            "---\nstatus: draft\ndepends-on:\n  - https://example.com\n  - claude-desktop session\n  - valid.md\n---\nBody.\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();
        assert_eq!(result.extractions.len(), 1);

        let ext = &result.extractions[0];
        assert_eq!(
            ext.refs.len(),
            3,
            "should have 3 discovered refs (URL + prose + valid)"
        );

        // Check classifications
        let hints: Vec<_> = ext.refs.iter().map(|r| &r.hint).collect();
        assert!(
            hints.iter().any(|h| matches!(h, RefHint::External)),
            "should have External ref for URL"
        );
        assert!(
            hints
                .iter()
                .any(|h| matches!(h, RefHint::Implausible { .. })),
            "should have Implausible ref for prose"
        );
        assert!(
            hints.iter().any(|h| matches!(h, RefHint::FilePath)),
            "should have FilePath ref for valid.md"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_graph_populates_snippet_indexes() {
        let tmp = std::env::temp_dir().join("anneal_test_snippet_indexes");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_md_file(
            &tmp,
            "guide.md",
            "---\nstatus: draft\n---\n# Overview\nFirst paragraph line.\nStill same paragraph.\n\n## Details\nSee OQ-64 here.\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert_eq!(
            result.file_snippets.get("guide.md").map(String::as_str),
            Some("First paragraph line. Still same paragraph.")
        );
        assert_eq!(
            result.label_snippets.get("OQ-64").map(String::as_str),
            Some("Details: See OQ-64 here.")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// End-to-end: URLs in frontmatter produce no E001 diagnostic and appear
    /// as `RefHint::External` in the extractions array. Exercises the full
    /// pipeline from build_graph through check_existence.
    #[test]
    fn url_in_frontmatter_no_e001_and_external_in_extraction() {
        let tmp = std::env::temp_dir().join("anneal_test_url_e2e");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // A file whose only frontmatter ref is a URL — should produce zero E001
        write_md_file(
            &tmp,
            "spec.md",
            "---\nstatus: active\ndepends-on:\n  - https://rfc.example.org/123\n  - http://archive.org/old\n---\nBody.\n",
        );
        // A file with a mix: one valid .md ref + one URL
        write_md_file(
            &tmp,
            "impl.md",
            "---\nstatus: draft\ndepends-on:\n  - spec.md\n  - https://docs.example.com/api\n---\nBody.\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        // URLs must never enter pending_edges (which is what produces E001)
        let url_edges: Vec<_> = result
            .pending_edges
            .iter()
            .filter(|e| e.target_identity.starts_with("http"))
            .collect();
        assert!(
            url_edges.is_empty(),
            "URLs should never enter pending_edges (source of E001), got: {:?}",
            url_edges
                .iter()
                .map(|e| &e.target_identity)
                .collect::<Vec<_>>()
        );

        // URLs should be in external_refs
        assert_eq!(
            result.external_refs.len(),
            3,
            "should have 3 external refs (2 from spec.md + 1 from impl.md)"
        );

        // --- Extraction pipeline ---
        assert_eq!(result.extractions.len(), 2);

        // spec.md: two URLs, both External
        let spec_ext = result
            .extractions
            .iter()
            .find(|e| e.status.as_deref() == Some("active"))
            .expect("spec.md extraction");
        assert_eq!(spec_ext.refs.len(), 2);
        assert!(
            spec_ext.refs.iter().all(|r| r.hint == RefHint::External),
            "all spec.md refs should be External"
        );

        // impl.md: one FilePath + one External
        let impl_ext = result
            .extractions
            .iter()
            .find(|e| e.status.as_deref() == Some("draft"))
            .expect("impl.md extraction");
        assert_eq!(impl_ext.refs.len(), 2);
        assert!(
            impl_ext.refs.iter().any(|r| r.hint == RefHint::FilePath),
            "impl.md should have a FilePath ref"
        );
        assert!(
            impl_ext.refs.iter().any(|r| r.hint == RefHint::External),
            "impl.md should have an External ref"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // scan_file_cmark tests
    // -----------------------------------------------------------------------

    fn cmark_scan(body: &str) -> (ScanResult, Vec<DiscoveredRef>, DiGraph) {
        let mut graph = DiGraph::new();
        let node = graph.add_node(Handle {
            id: "test.md".to_string(),
            kind: HandleKind::File(Utf8PathBuf::from("test.md")),
            status: None,
            file_path: Some(Utf8PathBuf::from("test.md")),
            metadata: HandleMetadata::default(),
        });
        let line_index = LineIndex::from_content(body, 0);
        let (result, refs) = scan_file_cmark(
            body,
            Utf8Path::new("test.md"),
            node,
            &mut graph,
            &line_index,
        );
        (result, refs, graph)
    }

    #[test]
    fn cmark_code_block_skipping() {
        let body = "## Heading\nSome OQ-64 ref\n```\nOQ-99 in code\n```\n";
        let (result, _refs, graph) = cmark_scan(body);

        // Should extract heading and OQ-64, but NOT OQ-99
        let labels: Vec<_> = result
            .label_candidates
            .iter()
            .map(|c| (c.prefix.as_str(), c.number))
            .collect();
        assert!(
            labels.contains(&("OQ", 64)),
            "should extract OQ-64, got: {labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l.1 == 99),
            "should NOT extract OQ-99 from code block, got: {labels:?}"
        );

        // Should have created a section for "Heading"
        let section_count = graph
            .nodes()
            .filter(|(_, h)| matches!(h.kind, HandleKind::Section { .. }))
            .count();
        assert_eq!(section_count, 1, "should create one section handle");
    }

    #[test]
    fn cmark_inline_code_skipping() {
        let body = "See `OQ-64` inline\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            result.label_candidates.is_empty(),
            "should NOT extract OQ-64 from inline code, got: {:?}",
            result
                .label_candidates
                .iter()
                .map(|c| format!("{}-{}", c.prefix, c.number))
                .collect::<Vec<_>>()
        );
        assert!(
            refs.is_empty(),
            "should have no discovered refs from inline code"
        );
    }

    #[test]
    fn cmark_markdown_link_extraction() {
        let body = "[link](foo.md)\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            result.file_refs.iter().any(|(r, _)| r == "foo.md"),
            "should extract foo.md from link, got: {:?}",
            result.file_refs
        );
        assert!(
            refs.iter()
                .any(|r| r.raw == "foo.md" && r.hint == RefHint::FilePath),
            "should have DiscoveredRef for foo.md"
        );
    }

    #[test]
    fn cmark_wikilink_extraction() {
        let body = "[[wiki-target]]\n";
        let (_result, refs, _) = cmark_scan(body);
        // Wiki-links produce a DiscoveredRef
        assert!(!refs.is_empty(), "should extract wiki-link, got no refs");
        assert!(
            refs.iter().any(|r| r.raw == "wiki-target"),
            "should have DiscoveredRef for wiki-target, got: {:?}",
            refs.iter().map(|r| &r.raw).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cmark_html_block_scanning() {
        let body = "<div>OQ-64</div>\n";
        let (result, _refs, _) = cmark_scan(body);
        let labels: Vec<_> = result
            .label_candidates
            .iter()
            .map(|c| (c.prefix.as_str(), c.number))
            .collect();
        assert!(
            labels.contains(&("OQ", 64)),
            "should extract OQ-64 from HTML block, got: {labels:?}"
        );
    }

    #[test]
    fn cmark_text_file_ref_from_body() {
        let body = "text with guide.md ref\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            result.file_refs.iter().any(|(r, _)| r == "guide.md"),
            "should extract guide.md from text, got: {:?}",
            result.file_refs
        );
        assert!(
            refs.iter()
                .any(|r| r.raw == "guide.md" && r.hint == RefHint::FilePath),
            "should have DiscoveredRef for guide.md"
        );
    }

    #[test]
    fn cmark_discovered_ref_has_span() {
        let body = "first line\nOQ-42 on second line\n";
        let (_, refs, _) = cmark_scan(body);
        assert!(!refs.is_empty(), "should find OQ-42");
        let oq_ref = refs.iter().find(|r| r.raw == "OQ-42").expect("OQ-42 ref");
        assert!(oq_ref.span.is_some(), "DiscoveredRef should have a span");
        let span = oq_ref.span.as_ref().expect("span present");
        assert_eq!(span.file, "test.md", "span file should be test.md");
        assert!(span.line >= 1, "span line should be >= 1");
    }

    #[test]
    fn cmark_table_cell_labels_extracted() {
        // Simple labels in table cells
        let body = "| Label | Desc |\n|---|---|\n| OQ-1 | question |\n| OQ-2 | another |\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            refs.iter().any(|r| r.raw == "OQ-1"),
            "should extract OQ-1 from table cell, got refs: {:?}",
            refs.iter().map(|r| &r.raw).collect::<Vec<_>>()
        );
        assert!(
            refs.iter().any(|r| r.raw == "OQ-2"),
            "should extract OQ-2 from table cell"
        );
        assert!(
            result
                .label_candidates
                .iter()
                .any(|c| c.prefix == "OQ" && c.number == 1),
            "should have label candidate OQ-1"
        );
    }

    #[test]
    fn cmark_table_cell_compound_labels_extracted() {
        // Compound prefix labels — regex now captures full compound prefix
        let body = "| Label | Desc |\n|---|---|\n| ST-OQ-1 | question |\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            refs.iter().any(|r| r.raw == "ST-OQ-1"),
            "should extract ST-OQ-1 from table cell, got refs: {:?}",
            refs.iter().map(|r| &r.raw).collect::<Vec<_>>()
        );
        assert!(
            result
                .label_candidates
                .iter()
                .any(|c| c.prefix == "ST-OQ" && c.number == 1),
            "should have label candidate with compound prefix ST-OQ"
        );
    }

    #[test]
    fn cmark_section_refs_extracted() {
        let body = "See §4.1 for details\n";
        let (result, _refs, _) = cmark_scan(body);
        assert!(
            result.section_refs.iter().any(|(r, _)| r == "4.1"),
            "should extract section ref 4.1, got: {:?}",
            result.section_refs
        );
    }

    #[test]
    fn cmark_url_rejection_in_text() {
        let body = "See https://example.com/rust-lang/guide.md for details\n";
        let (result, _, _) = cmark_scan(body);
        assert!(
            result.file_refs.is_empty(),
            "URL fragments should not be matched as file refs, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_version_dot_rejection() {
        let body = "See formal-model/murail-algebra-v1.2.md for details\n";
        let (result, _, _) = cmark_scan(body);
        assert!(
            !result.file_refs.iter().any(|(r, _)| r == "2.md"),
            "should not match fragment 2.md from v1.2.md, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_hyphen_prefix_rejection() {
        let body = "See RQ-01-program-format-encoding.md for details\n";
        let (result, _, _) = cmark_scan(body);
        assert!(
            !result.file_refs.iter().any(|(r, _)| r.starts_with('-')),
            "should not extract hyphen-prefixed fragments, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_mid_word_rejection() {
        let body = "[transcript](refs/2026-02-06-4eJrp9byBRk.md)";
        let (_, refs, _) = cmark_scan(body);
        // The link itself should be extracted from the Link event (as file ref)
        // but the text inside should NOT produce a "k.md" match
        assert!(
            !refs.iter().any(|r| r.raw == "k.md"),
            "should not extract k.md from mid-word, got: {:?}",
            refs.iter().map(|r| &r.raw).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cmark_ref_source_is_body() {
        let body = "OQ-42 in body text\n";
        let (_, refs, _) = cmark_scan(body);
        assert!(!refs.is_empty(), "should find OQ-42");
        for r in &refs {
            assert!(
                matches!(r.source, RefSource::Body),
                "body refs should have RefSource::Body"
            );
        }
    }

    #[test]
    fn cmark_link_with_fragment() {
        let body = "[see](foo.md#section)\n";
        let (result, _refs, _) = cmark_scan(body);
        assert!(
            result.file_refs.iter().any(|(r, _)| r == "foo.md"),
            "should extract foo.md stripping fragment, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_external_links_skipped() {
        let body = "[google](https://google.com)\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(
            result.file_refs.is_empty(),
            "should not extract external URLs as file refs"
        );
        assert!(
            refs.is_empty(),
            "should not produce DiscoveredRef for external links"
        );
    }

    #[test]
    fn cmark_edge_kind_inference() {
        let body = "This incorporates OQ-42 into the design\n";
        let (result, refs, _) = cmark_scan(body);
        assert!(!result.label_candidates.is_empty());
        // "incorporates" keyword should produce DependsOn edge
        assert_eq!(
            result.label_candidates[0].edge_kind,
            EdgeKind::DependsOn,
            "incorporates keyword should produce DependsOn edge"
        );
        assert!(
            refs.iter().any(|r| r.edge_kind == EdgeKind::DependsOn),
            "DiscoveredRef should also have DependsOn"
        );
    }

    #[test]
    fn cmark_indented_code_block_skipping() {
        // Indented code blocks (4 spaces) should also be skipped
        let body = "Normal text OQ-1\n\n    OQ-2 in indented code\n\nMore text OQ-3\n";
        let (result, _, _) = cmark_scan(body);
        let labels: Vec<u32> = result.label_candidates.iter().map(|c| c.number).collect();
        assert!(labels.contains(&1), "should extract OQ-1, got: {labels:?}");
        assert!(
            !labels.contains(&2),
            "should NOT extract OQ-2 from indented code, got: {labels:?}"
        );
        assert!(labels.contains(&3), "should extract OQ-3, got: {labels:?}");
    }

    // -----------------------------------------------------------------------
    // Corpus smoke tests — validate cmark scanner on real corpora
    // -----------------------------------------------------------------------

    /// Walk a corpus, run scan_file_cmark on each .md file, verify SourceSpan coverage.
    fn corpus_smoke_test(corpus_path: &str, corpus_name: &str) {
        let corpus = std::path::Path::new(corpus_path);
        if !corpus.exists() {
            println!("SKIPPED: {corpus_name} corpus not found at {corpus_path}");
            return;
        }

        let mut files_scanned: usize = 0;
        let mut total_refs: usize = 0;
        let mut total_body_refs: usize = 0;
        let mut body_refs_with_span: usize = 0;

        for entry in walkdir::WalkDir::new(corpus) {
            let Ok(entry) = entry else { continue };
            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };

            let relative_str = path.strip_prefix(corpus).unwrap_or(path).to_string_lossy();
            let relative = Utf8Path::new(&*relative_str);
            let (frontmatter_yaml, body) = split_frontmatter(&content);

            #[allow(clippy::cast_possible_truncation)]
            let fm_lines = frontmatter_yaml.map_or(0, |yaml| yaml.lines().count() as u32 + 2);
            let line_index = LineIndex::from_content(body, fm_lines);
            let mut graph = DiGraph::new();
            let file_node = graph.add_node(Handle {
                id: relative_str.to_string(),
                kind: HandleKind::File(Utf8PathBuf::from(&*relative_str)),
                status: None,
                file_path: Some(Utf8PathBuf::from(&*relative_str)),
                metadata: HandleMetadata::default(),
            });
            let (result, body_discovered) =
                scan_file_cmark(body, relative, file_node, &mut graph, &line_index);

            let refs =
                result.label_candidates.len() + result.section_refs.len() + result.file_refs.len();
            total_refs += refs;
            files_scanned += 1;

            for dr in &body_discovered {
                total_body_refs += 1;
                if dr.span.as_ref().is_some_and(|s| s.line > 0) {
                    body_refs_with_span += 1;
                }
            }
        }

        println!("=== Corpus Smoke Test: {corpus_name} ===");
        println!("Files: {files_scanned}, Refs: {total_refs}");
        println!("SourceSpan coverage: {body_refs_with_span}/{total_body_refs}");

        assert!(files_scanned > 0, "should scan at least one file");
        assert_eq!(
            body_refs_with_span, total_body_refs,
            "every body DiscoveredRef must have a SourceSpan with line > 0"
        );
    }

    #[test]
    #[ignore = "requires external corpus at ~/code/murail/.design/"]
    fn corpus_smoke_murail() {
        let home = std::env::var("HOME").expect("HOME must be set");
        corpus_smoke_test(&format!("{home}/code/murail/.design/"), "Murail");
    }

    #[test]
    #[ignore = "requires external corpus at ~/code/herald/.design/"]
    fn corpus_smoke_herald() {
        let home = std::env::var("HOME").expect("HOME must be set");
        corpus_smoke_test(&format!("{home}/code/herald/.design/"), "Herald");
    }

    // -------------------------------------------------------------------
    // split_frontmatter
    // -------------------------------------------------------------------

    #[test]
    fn split_frontmatter_standard() {
        let input = "---\nstatus: draft\ntitle: Hello\n---\nBody text here.\n";
        let (fm, body) = split_frontmatter(input);

        assert_eq!(fm, Some("status: draft\ntitle: Hello"));
        assert_eq!(body, "Body text here.\n");
    }

    #[test]
    fn split_frontmatter_crlf_line_endings() {
        let input = "---\r\nstatus: draft\r\n---\r\nBody text.\r\n";
        let (fm, body) = split_frontmatter(input);

        // The \r before the closing \n---\r\n is part of the yaml slice.
        assert_eq!(fm, Some("status: draft\r"));
        assert_eq!(body, "Body text.\r\n");
    }

    #[test]
    fn split_frontmatter_no_frontmatter() {
        let input = "# Just a heading\n\nSome body text.\n";
        let (fm, body) = split_frontmatter(input);

        assert_eq!(fm, None);
        assert_eq!(body, input);
    }

    #[test]
    fn split_frontmatter_empty_yaml() {
        // With no content between fences, the closing `---` lacks the
        // preceding `\n` the parser requires, so this is treated as no
        // valid frontmatter.
        let input = "---\n---\nBody after empty frontmatter.\n";
        let (fm, body) = split_frontmatter(input);

        assert_eq!(fm, None);
        assert_eq!(body, input);
    }

    #[test]
    fn split_frontmatter_minimal_yaml() {
        // A single newline between fences provides the `\n---\n` the
        // parser needs to detect the closing fence.
        let input = "---\n\n---\nBody after minimal frontmatter.\n";
        let (fm, body) = split_frontmatter(input);

        assert_eq!(fm, Some(""));
        assert_eq!(body, "Body after minimal frontmatter.\n");
    }

    #[test]
    fn split_frontmatter_eof_without_trailing_newline() {
        let input = "---\nstatus: final\n---";
        let (fm, body) = split_frontmatter(input);

        assert_eq!(fm, Some("status: final"));
        assert_eq!(body, "");
    }
}
