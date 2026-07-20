//! Markdown parser that turns source files into extraction artifacts.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::extract::body_scan::{
    CodePathRef, HeadingSpan, LabelCandidate, ScanResult, scan_file_cmark,
};
use crate::extract::config::{AnnealConfig, Direction, FrontmatterConfig};
use crate::extract::extraction::{
    DiscoveredRef, FileExtraction, ImplausibleReason, LineIndex, RefHint, RefSource,
    UnresolvedRefDisposition, classify_frontmatter_value, extract_file_snippet_from_body,
    extract_label_snippet_from_content,
};
use crate::extract::graph::{DiGraph, EdgeKind};
use crate::extract::handle::{Handle, HandleMetadata, NodeId};

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
    /// Explicit `date:` frontmatter field (lower priority than `updated:`).
    pub(crate) frontmatter_date: Option<chrono::NaiveDate>,
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

    // Special fields: status, updated, date, purpose, note (not edge-producing)
    let status = get("status").and_then(yaml_value_to_string);
    let updated = get("updated")
        .and_then(yaml_value_to_string)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());
    let frontmatter_date = get("date")
        .and_then(yaml_value_to_string)
        .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());
    let purpose = get("purpose").and_then(yaml_value_to_string);
    let note = get("note").and_then(yaml_value_to_string);

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
        if matches!(key_str, "status" | "updated" | "date" | "purpose" | "note") {
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
        purpose,
        note,
    };

    FrontmatterParseResult {
        status,
        metadata,
        field_edges,
        all_keys,
        yaml_failed: false,
        frontmatter_date,
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

fn yaml_scalar_values(value: &serde_yaml_ng::Value) -> Vec<String> {
    match value {
        serde_yaml_ng::Value::String(value) => vec![strip_trailing_parenthetical(value)],
        serde_yaml_ng::Value::Number(value) => vec![value.to_string()],
        serde_yaml_ng::Value::Bool(value) => vec![value.to_string()],
        serde_yaml_ng::Value::Sequence(values) => {
            values.iter().flat_map(yaml_scalar_values).collect()
        }
        _ => Vec::new(),
    }
}

fn frontmatter_scalars(yaml: Option<&str>) -> Vec<(String, String)> {
    let Some(yaml) = yaml else {
        return Vec::new();
    };
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) else {
        return Vec::new();
    };
    let Some(mapping) = value.as_mapping() else {
        return Vec::new();
    };
    let mut scalars = Vec::new();
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        for scalar in yaml_scalar_values(value) {
            scalars.push((key.to_string(), scalar));
        }
    }
    scalars
}

// ---------------------------------------------------------------------------
// Scan result types
// ---------------------------------------------------------------------------

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
    pub(crate) unresolved_disposition: UnresolvedRefDisposition,
}

/// An edge descriptor parsed from a frontmatter field via the extensible mapping.
pub(crate) struct FrontmatterEdge {
    pub(crate) targets: Vec<String>,
    pub(crate) edge_kind: EdgeKind,
    pub(crate) inverse: bool,
}

// ---------------------------------------------------------------------------
// Root inference
// ---------------------------------------------------------------------------

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

/// Extract a `YYYY-MM-DD` date prefix from a filename.
///
/// Matches filenames like `2026-03-29-architecture-spike-findings.md`.
/// Returns `None` if the filename doesn't start with a valid date.
fn date_from_filename(filename: &str) -> Option<chrono::NaiveDate> {
    if filename.len() >= 10 {
        chrono::NaiveDate::parse_from_str(&filename[..10], "%Y-%m-%d").ok()
    } else {
        None
    }
}

/// Resolve the best available date for a file handle.
///
/// Priority: `updated:` frontmatter > `date:` frontmatter > filename prefix.
fn resolve_file_date(
    metadata: &HandleMetadata,
    frontmatter_date: Option<chrono::NaiveDate>,
    filename: &str,
) -> Option<chrono::NaiveDate> {
    metadata
        .updated
        .or(frontmatter_date)
        .or_else(|| date_from_filename(filename))
}

/// Result of `build_graph`: the populated graph, label candidates for namespace
/// inference, pending edges for resolution, and observed status values for
/// lattice inference.
pub(crate) struct BuildResult {
    pub(crate) graph: DiGraph,
    pub(crate) label_candidates: Vec<LabelCandidate>,
    pub(crate) pending_edges: Vec<PendingEdge>,
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
    /// Per-file markdown payload read during graph construction.
    pub(crate) file_payloads: HashMap<String, ParsedMarkdownFile>,
    /// Physical source paths keyed by logical file handle.
    pub(crate) file_origins: Arc<HashMap<String, Utf8PathBuf>>,
    /// Precomputed snippets for file handles, keyed by relative file path.
    pub(crate) file_snippets: HashMap<String, String>,
    /// Precomputed snippets for label handles, keyed by label identity.
    pub(crate) label_snippets: HashMap<String, String>,
    /// Heading spans keyed by relative file path.
    pub(crate) heading_spans: HashMap<String, Vec<HeadingSpan>>,
    /// In-repo code path references discovered in markdown body text.
    pub(crate) code_refs: Vec<CodePathRef>,
    /// Files whose YAML frontmatter failed to deserialize (§7.2 silent-failure tracking).
    #[allow(dead_code)] // Consumed by status/check reporting once surfaced
    pub(crate) malformed_frontmatter: Vec<String>,
    /// Count of directory entries skipped because their filename was not valid UTF-8 (§7.3).
    #[allow(dead_code)] // Consumed by status/check reporting once surfaced
    pub(crate) skipped_non_utf8: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ExternalScanRoot {
    pub(crate) physical_root: Utf8PathBuf,
    pub(crate) handle_prefix: Utf8PathBuf,
    pub(crate) containment_root: Utf8PathBuf,
}

struct MarkdownFileCandidate {
    physical_path: Utf8PathBuf,
    logical_path: Utf8PathBuf,
}

#[derive(Clone, Copy)]
struct MarkdownWalk<'a> {
    physical_root: &'a Utf8Path,
    handle_prefix: &'a Utf8Path,
    scan_roots: Option<&'a [Utf8PathBuf]>,
    containment_root: Option<&'a Utf8Path>,
}

/// Parsed markdown file content retained for fact emission.
#[derive(Clone, Debug)]
pub(crate) struct ParsedMarkdownFile {
    pub(crate) body: String,
    pub(crate) body_start_line: u32,
    pub(crate) frontmatter_scalars: Vec<(String, String)>,
    pub(crate) revision: String,
}

struct ParsedMarkdownFileScan {
    relative: Utf8PathBuf,
    content: String,
    body: String,
    body_start_line: u32,
    frontmatter_scalars: Vec<(String, String)>,
    revision: String,
    frontmatter: FrontmatterParseResult,
    file_date: Option<chrono::NaiveDate>,
    file_snippet: Option<String>,
    scan_result: ScanResult,
    body_refs: Vec<DiscoveredRef>,
}

/// Split `exclude` entries into plain directory names and glob patterns.
///
/// An entry is treated as a glob pattern if it contains `*`, `?`, `[`, or `/`.
/// Plain entries continue to work as directory-name exclusions (backward compatible).
/// Glob patterns are compiled into a `GlobSet` matched against relative paths,
/// allowing file-level exclusions like `**/README.md`.
///
/// Used by the graph-building walker (parse.rs) and by the orient command's
/// reading-list pruner so both honor the same `exclude` grammar.
pub(crate) fn build_exclude_sets(exclude: &[String]) -> (Vec<&str>, Option<GlobSet>) {
    let mut dir_names = Vec::new();
    let mut builder = GlobSetBuilder::new();
    let mut has_globs = false;

    for entry in exclude {
        if entry.contains('*') || entry.contains('?') || entry.contains('[') || entry.contains('/')
        {
            if let Ok(glob) = GlobBuilder::new(entry).literal_separator(false).build() {
                builder.add(glob);
                has_globs = true;
            }
        } else {
            dir_names.push(entry.as_str());
        }
    }

    let glob_set = if has_globs {
        builder.build().ok()
    } else {
        None
    };

    (dir_names, glob_set)
}

/// Build the knowledge graph from a directory of markdown files.
///
/// Walks the directory tree, creates File handles, scans content with
/// pulldown-cmark, and collects label candidates and pending edges for
/// later resolution.
pub(crate) fn build_graph(root: &Utf8Path, config: &AnnealConfig) -> Result<BuildResult> {
    build_graph_scoped(root, config, &[Utf8PathBuf::from(".")])
}

fn parse_markdown_file(
    utf8_path: &Utf8Path,
    relative: Utf8PathBuf,
    frontmatter_config: &FrontmatterConfig,
    code_path_roots: &[String],
) -> Result<ParsedMarkdownFileScan> {
    let content = std::fs::read_to_string(utf8_path)
        .with_context(|| format!("failed to read {utf8_path}"))?;

    let (frontmatter_yaml, body) = split_frontmatter(&content);
    let file_snippet = extract_file_snippet_from_body(body);

    // Compute frontmatter line count for LineIndex offset calculation.
    // LineIndex::from_content expects the opening --- plus yaml content lines;
    // it adds the closing --- itself via +1 in base_line.
    #[allow(clippy::cast_possible_truncation)] // frontmatter line count won't exceed u32::MAX
    let frontmatter_line_count = frontmatter_yaml.map_or(0, |yaml| {
        // +1 for the opening --- line only (closing --- handled by LineIndex)
        yaml.lines().count() as u32 + 1
    });
    let body_start_line = if frontmatter_line_count == 0 {
        1
    } else {
        frontmatter_line_count.saturating_add(2)
    };
    let frontmatter_scalars = frontmatter_scalars(frontmatter_yaml);
    let revision = format!("{:016x}", anneal_core::fnv1a_64(content.as_bytes()));

    // D-05: table-driven frontmatter parsing with extensible field mapping
    // D-07: all_keys returned here to avoid double-parsing YAML
    let frontmatter = frontmatter_yaml
        .map(|yaml| parse_frontmatter(yaml, frontmatter_config))
        .unwrap_or_default();

    let filename = relative.file_name().unwrap_or(relative.as_str());
    let file_date = resolve_file_date(
        &frontmatter.metadata,
        frontmatter.frontmatter_date,
        filename,
    );

    // Use pulldown-cmark scanner for production body scanning
    let line_index = LineIndex::from_content(body, frontmatter_line_count);
    let (scan_result, body_refs) = scan_file_cmark(body, &relative, &line_index, code_path_roots);

    let body = body.to_string();

    Ok(ParsedMarkdownFileScan {
        relative,
        content,
        body,
        body_start_line,
        frontmatter_scalars,
        revision,
        frontmatter,
        file_date,
        file_snippet,
        scan_result,
        body_refs,
    })
}

pub(crate) fn build_graph_scoped(
    root: &Utf8Path,
    config: &AnnealConfig,
    scan_roots: &[Utf8PathBuf],
) -> Result<BuildResult> {
    build_graph_with_external_roots(root, config, scan_roots, &[])
}

pub(crate) fn build_graph_with_external_roots(
    root: &Utf8Path,
    config: &AnnealConfig,
    scan_roots: &[Utf8PathBuf],
    external_roots: &[ExternalScanRoot],
) -> Result<BuildResult> {
    let mut graph = DiGraph::new();
    let mut all_label_candidates = Vec::new();
    let mut pending_edges = Vec::new();
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
    let mut file_payloads: HashMap<String, ParsedMarkdownFile> = HashMap::new();
    let mut file_origins: HashMap<String, Utf8PathBuf> = HashMap::new();
    let mut file_snippets: HashMap<String, String> = HashMap::new();
    let mut label_snippets: HashMap<String, String> = HashMap::new();
    let mut heading_spans: HashMap<String, Vec<HeadingSpan>> = HashMap::new();
    let mut code_refs: Vec<CodePathRef> = Vec::new();
    let mut malformed_frontmatter: Vec<String> = Vec::new();

    // §7.3: Track non-UTF-8 filenames silently skipped by the walker filter.
    // Uses Cell so the FnMut closure can increment without &mut self conflicts.
    let skipped_non_utf8: Cell<usize> = Cell::new(0);

    let (dir_exclusions, file_glob_set) = build_exclude_sets(&config.exclude);

    let mut candidates = collect_markdown_files(
        MarkdownWalk {
            physical_root: root,
            handle_prefix: Utf8Path::new(""),
            scan_roots: Some(scan_roots),
            containment_root: None,
        },
        &dir_exclusions,
        file_glob_set.as_ref(),
        &skipped_non_utf8,
    )?;
    for external_root in external_roots {
        candidates.extend(collect_markdown_files(
            MarkdownWalk {
                physical_root: &external_root.physical_root,
                handle_prefix: &external_root.handle_prefix,
                scan_roots: None,
                containment_root: Some(&external_root.containment_root),
            },
            &dir_exclusions,
            file_glob_set.as_ref(),
            &skipped_non_utf8,
        )?);
    }

    for candidate in candidates {
        let utf8_path = candidate.physical_path;
        let relative = candidate.logical_path;
        if let Some(existing) = file_origins.get(relative.as_str()) {
            anyhow::bail!(
                "markdown handle collision for {:?}: both {} and {} map to the same logical handle",
                relative.as_str(),
                existing,
                utf8_path
            );
        }
        file_origins.insert(relative.to_string(), utf8_path.clone());

        if let Some(filename) = relative.file_name() {
            filename_index
                .entry(filename.to_string())
                .or_default()
                .push(relative.clone());
        }

        let ParsedMarkdownFileScan {
            relative,
            content,
            body,
            body_start_line,
            frontmatter_scalars,
            revision,
            frontmatter,
            file_date,
            file_snippet,
            mut scan_result,
            body_refs,
        } = parse_markdown_file(
            &utf8_path,
            relative,
            &config.frontmatter,
            &config.code_path_root.root,
        )?;
        if let Some(snippet) = file_snippet {
            file_snippets.insert(relative.to_string(), snippet);
        }

        let FrontmatterParseResult {
            status,
            metadata,
            field_edges,
            all_keys,
            yaml_failed,
            frontmatter_date: _,
        } = frontmatter;

        // §7.2: Track files whose YAML frontmatter was present but malformed.
        if yaml_failed {
            malformed_frontmatter.push(relative.to_string());
        }

        for key in &all_keys {
            *observed_frontmatter_keys.entry(key.clone()).or_insert(0) += 1;
        }

        if let Some(ref s) = status {
            // D-04: Track directory convention for terminal status classification
            let in_terminal = crate::extract::path_conventions::has_terminal_directory(&relative);
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
                    RefHint::CodePath => {
                        // Mint an external:code handle via the body code-ref
                        // pipeline (parse.rs minting block below) so the
                        // filesystem probe decides existence. A present target
                        // resolves cleanly; a missing one becomes W006
                        // spec_code_drift, never a false E001 broken_reference.
                        scan_result
                            .code_refs
                            .push(CodePathRef::from_frontmatter_field(
                                relative.as_str(),
                                target,
                            ));
                    }
                    RefHint::Label { .. } | RefHint::FilePath | RefHint::SectionRef => {
                        pending_edges.push(PendingEdge {
                            source: file_node_placeholder,
                            target_identity: target.clone(),
                            kind: fe.edge_kind.clone(),
                            inverse: fe.inverse,
                            line: Some(1),
                            unresolved_disposition: UnresolvedRefDisposition::CorpusGate,
                        });
                    }
                }
            }
        }

        let file_node = graph.add_node(Handle::file(
            relative.clone(),
            status.clone(),
            file_date,
            Some(u32::try_from(content.len()).unwrap_or(u32::MAX)),
            metadata.clone(),
        ));
        assert_eq!(
            file_node, file_node_placeholder,
            "node insertion order changed between placeholder computation and add_node"
        );

        for target in file_external_targets {
            let external_node = if let Some(existing) = external_nodes.get(&target).copied() {
                existing
            } else {
                let node_id =
                    graph.add_node(Handle::external(target.clone(), Some(relative.clone())));
                external_nodes.insert(target.clone(), node_id);
                node_id
            };

            graph.add_edge(file_node, external_node, EdgeKind::Cites);
        }

        if !scan_result.heading_spans.is_empty() {
            heading_spans.insert(
                relative.to_string(),
                std::mem::take(&mut scan_result.heading_spans),
            );
        }

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
        for code_ref in std::mem::take(&mut scan_result.code_refs) {
            let handle_id = code_ref.handle_id.clone();
            if !external_nodes.contains_key(&handle_id) {
                let node_id =
                    graph.add_node(Handle::external(handle_id.clone(), Some(relative.clone())));
                external_nodes.insert(handle_id.clone(), node_id);
            }
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: handle_id,
                kind: EdgeKind::Cites,
                inverse: false,
                line: Some(code_ref.source_line),
                unresolved_disposition: UnresolvedRefDisposition::AmbiguousExternalOk,
            });
            code_refs.push(code_ref);
        }

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
        file_payloads.insert(
            relative.to_string(),
            ParsedMarkdownFile {
                body,
                body_start_line,
                frontmatter_scalars,
                revision,
            },
        );

        for file_ref in &scan_result.file_refs {
            let (target_identity, unresolved_disposition) =
                normalize_body_file_ref(root, &file_ref.target, file_ref.unresolved_disposition);
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity,
                kind: EdgeKind::Cites,
                inverse: false,
                line: Some(file_ref.line),
                unresolved_disposition,
            });
        }
        for (section_ref, line) in &scan_result.section_refs {
            pending_edges.push(PendingEdge {
                source: file_node,
                target_identity: format!("section:{section_ref}"),
                kind: EdgeKind::Cites,
                inverse: false,
                line: Some(*line),
                unresolved_disposition: UnresolvedRefDisposition::CorpusGate,
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
        terminal_by_directory,
        observed_frontmatter_keys,
        filename_index,
        implausible_refs,
        external_refs,
        extractions,
        file_payloads,
        file_origins: Arc::new(file_origins),
        file_snippets,
        label_snippets,
        heading_spans,
        code_refs,
        malformed_frontmatter,
        skipped_non_utf8: skipped_non_utf8.get(),
    })
}

fn collect_markdown_files(
    scan: MarkdownWalk<'_>,
    dir_exclusions: &[&str],
    file_glob_set: Option<&GlobSet>,
    skipped_non_utf8: &Cell<usize>,
) -> Result<Vec<MarkdownFileCandidate>> {
    let walker = WalkDir::new(scan.physical_root.as_std_path())
        .into_iter()
        .filter_entry(|entry| {
            let Some(name) = entry.file_name().to_str() else {
                skipped_non_utf8.set(skipped_non_utf8.get() + 1);
                return false;
            };
            if entry.file_type().is_dir()
                && (DEFAULT_EXCLUSIONS.contains(&name)
                    || (name.starts_with('.') && name != ".design")
                    || dir_exclusions.contains(&name))
            {
                return false;
            }
            if let Some(globs) = file_glob_set
                && let Ok(relative) = entry.path().strip_prefix(scan.physical_root.as_std_path())
                && globs.is_match(scan.handle_prefix.as_std_path().join(relative))
            {
                return false;
            }
            true
        });

    let mut files = Vec::new();
    for entry in walker {
        let entry = entry.context("failed to read directory entry")?;
        if entry.file_type().is_dir()
            || entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("md")
        {
            continue;
        }
        let path = Utf8PathBuf::try_from(entry.path().to_path_buf())
            .with_context(|| format!("non-UTF-8 path: {}", entry.path().display()))?;
        let local = path.strip_prefix(scan.physical_root).with_context(|| {
            format!(
                "failed to key markdown path {path} under {}",
                scan.physical_root
            )
        })?;
        let logical = scan.handle_prefix.join(local);
        if scan
            .scan_roots
            .is_some_and(|roots| !is_inside_scan_roots(&logical, roots))
        {
            continue;
        }
        let physical_path = if let Some(boundary) = scan.containment_root {
            let canonical = path
                .canonicalize_utf8()
                .with_context(|| format!("failed to resolve external markdown file {path}"))?;
            if !canonical.starts_with(boundary) {
                anyhow::bail!(
                    "external markdown file {path} resolves outside the provenance git root {boundary}"
                );
            }
            canonical
        } else {
            path
        };
        files.push(MarkdownFileCandidate {
            physical_path,
            logical_path: logical,
        });
    }
    Ok(files)
}

fn normalize_body_file_ref(
    root: &Utf8Path,
    target: &str,
    disposition: UnresolvedRefDisposition,
) -> (String, UnresolvedRefDisposition) {
    let target_path = Utf8Path::new(target);
    if !target_path.is_absolute() {
        return (target.to_string(), disposition);
    }
    if let Ok(relative) = target_path.strip_prefix(root) {
        return (relative.to_string(), disposition);
    }
    if target_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return (
            target.to_string(),
            UnresolvedRefDisposition::AmbiguousExternalOk,
        );
    }
    (target.to_string(), disposition)
}

fn is_inside_scan_roots(relative: &Utf8Path, scan_roots: &[Utf8PathBuf]) -> bool {
    scan_roots
        .iter()
        .any(|scan_root| scan_root == "." || relative.starts_with(scan_root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::handle::{HandleKind, HandleMetadata};

    // -----------------------------------------------------------------------
    // Plausibility filter integration tests (build_graph)
    // -----------------------------------------------------------------------

    fn write_md_file(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("write test file");
    }

    #[test]
    fn parse_markdown_file_boundary_matches_build_graph_artifacts() {
        let tmp = std::env::temp_dir().join("anneal_test_parse_file_boundary");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "source.md",
            "---\nstatus: draft\nupdated: 2026-01-02\npurpose: Boundary test\ndepends-on: target.md\n---\n# Source\n\nThis incorporates OQ-42.\n\n## Details\nSee guide.md and §4.1.\n",
        );
        write_md_file(&tmp, "target.md", "# Target\n");
        write_md_file(&tmp, "guide.md", "# Guide\n");

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let parsed = parse_markdown_file(
            &root.join("source.md"),
            Utf8PathBuf::from("source.md"),
            &config.frontmatter,
            &config.code_path_root.root,
        )
        .expect("parse file");
        let result = build_graph(root, &config).expect("build graph");

        let payload = result
            .file_payloads
            .get("source.md")
            .expect("source payload");
        assert_eq!(payload.body, parsed.body);
        assert_eq!(payload.body_start_line, parsed.body_start_line);
        assert_eq!(payload.frontmatter_scalars, parsed.frontmatter_scalars);
        assert_eq!(payload.revision, parsed.revision);
        assert_eq!(
            result.file_snippets.get("source.md"),
            parsed.file_snippet.as_ref()
        );
        assert_eq!(
            result
                .heading_spans
                .get("source.md")
                .map_or(0, std::vec::Vec::len),
            parsed.scan_result.heading_spans.len()
        );

        let source_extraction = result
            .extractions
            .iter()
            .find(|extraction| extraction.file == "source.md")
            .expect("source extraction");
        assert_eq!(source_extraction.status, parsed.frontmatter.status);
        assert_eq!(
            source_extraction.metadata.updated,
            parsed.frontmatter.metadata.updated
        );
        assert_eq!(
            source_extraction.metadata.purpose,
            parsed.frontmatter.metadata.purpose
        );
        assert_eq!(
            source_extraction.metadata.depends_on,
            parsed.frontmatter.metadata.depends_on
        );
        assert!(
            source_extraction
                .refs
                .iter()
                .any(|reference| reference.raw == "target.md"
                    && matches!(reference.source, RefSource::Frontmatter { .. }))
        );
        assert!(source_extraction.refs.iter().any(
            |reference| reference.raw == "OQ-42" && matches!(reference.source, RefSource::Body)
        ));

        let _ = std::fs::remove_dir_all(&tmp);
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

    #[test]
    fn build_graph_emits_heading_spans_without_section_handles() {
        let tmp = std::env::temp_dir().join("anneal_test_heading_spans");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_md_file(
            &tmp,
            "guide.md",
            "---\nstatus: draft\n---\n# Overview\nIntro.\n\n## Details\nBody.\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert_eq!(
            result.graph.node_count(),
            1,
            "section headings should not emit handles"
        );
        let spans = result
            .heading_spans
            .get("guide.md")
            .expect("guide heading spans");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].id, "guide.md#h/overview");
        assert_eq!(spans[1].id, "guide.md#h/overview/details");

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

    #[test]
    fn unresolved_wikilink_pending_edge_is_ambiguous_not_gate_level() {
        let tmp = std::env::temp_dir().join("anneal_test_unresolved_wikilink");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "doc.md", "# Doc\n\nSee [[claim]].\n");

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert!(result.pending_edges.iter().any(|edge| {
            edge.target_identity == "claim"
                && edge.unresolved_disposition == UnresolvedRefDisposition::AmbiguousExternalOk
        }));
        let doc_ext = result
            .extractions
            .iter()
            .find(|extraction| extraction.file == "doc.md")
            .expect("doc extraction");
        assert!(
            doc_ext.refs.iter().any(|reference| reference.raw == "claim"
                && reference.hint == RefHint::FilePath
                && matches!(reference.source, RefSource::Body)),
            "ambiguous wikilink remains visible in extraction refs: {:?}",
            doc_ext.refs
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolved_wikilink_builds_normal_corpus_edge() {
        let tmp = std::env::temp_dir().join("anneal_test_resolved_wikilink");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "doc.md", "# Doc\n\nSee [[target.md]].\n");
        write_md_file(&tmp, "target.md", "# Target\n");

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let mut result = build_graph(root, &config).unwrap();
        crate::extract::resolve::resolve_all(
            &mut result.graph,
            &result.label_candidates,
            &result.pending_edges,
            &config,
            root,
            &result.filename_index,
        );
        let doc_node = result
            .graph
            .nodes()
            .find_map(|(node_id, handle)| (handle.id == "doc.md").then_some(node_id))
            .expect("doc node");

        assert!(
            result.graph.outgoing(doc_node).iter().any(|edge| {
                result.graph.node(edge.target).id == "target.md" && edge.kind == EdgeKind::Cites
            }),
            "resolved wikilink should produce a normal Cites edge"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unresolved_markdown_links_remain_gate_level_pending_edges() {
        let tmp = std::env::temp_dir().join("anneal_test_unresolved_md_link");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "doc.md", "# Doc\n\nSee [target](missing.md).\n");

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert!(result.pending_edges.iter().any(|edge| {
            edge.target_identity == "missing.md"
                && edge.unresolved_disposition == UnresolvedRefDisposition::CorpusGate
        }));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn absolute_markdown_links_inside_corpus_normalize_to_relative_refs() {
        let tmp = std::env::temp_dir().join("anneal_test_absolute_md_link");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "target.md", "# Target\n");
        let absolute_target = tmp.join("target.md");
        write_md_file(
            &tmp,
            "doc.md",
            &format!("# Doc\n\nSee [target]({}).\n", absolute_target.display()),
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let mut result = build_graph(root, &config).unwrap();
        assert!(result.pending_edges.iter().any(|edge| {
            edge.target_identity == "target.md"
                && edge.unresolved_disposition == UnresolvedRefDisposition::CorpusGate
        }));
        crate::extract::resolve::resolve_all(
            &mut result.graph,
            &result.label_candidates,
            &result.pending_edges,
            &config,
            root,
            &result.filename_index,
        );
        let doc_node = result
            .graph
            .nodes()
            .find_map(|(node_id, handle)| (handle.id == "doc.md").then_some(node_id))
            .expect("doc node");
        assert!(
            result.graph.outgoing(doc_node).iter().any(|edge| {
                result.graph.node(edge.target).id == "target.md" && edge.kind == EdgeKind::Cites
            }),
            "absolute in-corpus markdown link should resolve to target.md"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn source_code_links_do_not_create_gate_level_pending_edges() {
        let tmp = std::env::temp_dir().join("anneal_test_source_code_link");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "doc.md",
            "# Doc\n\nSee [analysis](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/analysis.rs:1078) and [missing](missing.md).\n",
        );

        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        assert!(
            !result
                .pending_edges
                .iter()
                .any(|edge| edge.target_identity.contains("analysis.rs")
                    && edge.unresolved_disposition == UnresolvedRefDisposition::CorpusGate),
            "source-code citations should not enter gate-level pending edges: {:?}",
            result
                .pending_edges
                .iter()
                .map(|edge| (edge.target_identity.as_str(), edge.unresolved_disposition))
                .collect::<Vec<_>>()
        );
        assert!(
            result.code_refs.iter().any(|reference| reference.path
                == "/Users/morgan/code/anneal/crates/anneal-core/src/runtime/analysis.rs"
                && reference.start_line == Some(1078)),
            "source-code citation should be preserved as a code ref: {:?}",
            result.code_refs
        );
        assert!(result.pending_edges.iter().any(|edge| {
            edge.target_identity == "missing.md"
                && edge.unresolved_disposition == UnresolvedRefDisposition::CorpusGate
        }));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // Temporal awareness: date extraction
    // -----------------------------------------------------------------------

    #[test]
    fn date_from_filename_valid_prefix() {
        let d = date_from_filename("2026-03-29-architecture-spike-findings.md");
        assert_eq!(
            d,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 29).unwrap())
        );
    }

    #[test]
    fn date_from_filename_no_date() {
        assert_eq!(date_from_filename("README.md"), None);
        assert_eq!(date_from_filename("LABELS.md"), None);
    }

    #[test]
    fn date_from_filename_short_name() {
        assert_eq!(date_from_filename("foo.md"), None);
    }

    #[test]
    fn resolve_file_date_prefers_updated() {
        let meta = HandleMetadata {
            updated: Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()),
            ..HandleMetadata::default()
        };
        let fm_date = Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
        let result = resolve_file_date(&meta, fm_date, "2026-02-01-old.md");
        assert_eq!(
            result, meta.updated,
            "updated: should win over date: and filename"
        );
    }

    #[test]
    fn resolve_file_date_falls_back_to_frontmatter_date() {
        let meta = HandleMetadata::default();
        let fm_date = Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
        let result = resolve_file_date(&meta, fm_date, "2026-02-01-old.md");
        assert_eq!(
            result, fm_date,
            "date: should win over filename when no updated:"
        );
    }

    #[test]
    fn resolve_file_date_falls_back_to_filename() {
        let meta = HandleMetadata::default();
        let result = resolve_file_date(&meta, None, "2026-04-09-graph-coalescing.md");
        assert_eq!(
            result,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 9).unwrap()),
            "should extract date from filename when no frontmatter dates"
        );
    }

    #[test]
    fn resolve_file_date_none_when_no_signal() {
        let meta = HandleMetadata::default();
        assert_eq!(resolve_file_date(&meta, None, "README.md"), None);
    }

    #[test]
    fn build_graph_populates_file_date_from_filename() {
        let tmp = std::env::temp_dir().join("anneal_test_file_date");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(
            &tmp,
            "2026-03-29-design.md",
            "---\nstatus: draft\n---\nBody\n",
        );
        let config = AnnealConfig::default();
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        let file_handle = result
            .graph
            .nodes()
            .find(|(_, h)| h.id.contains("2026-03-29"))
            .map(|(_, h)| h)
            .expect("should find dated file");
        assert_eq!(
            file_handle.date,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 29).unwrap()),
            "file date should be extracted from filename"
        );
        let _ = std::fs::remove_dir_all(&tmp);
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

    // -----------------------------------------------------------------------
    // build_exclude_sets
    // -----------------------------------------------------------------------

    #[test]
    fn exclude_plain_entries_become_dir_names() {
        let entries = vec!["vendor".to_string(), "dist".to_string()];
        let (dirs, globs) = build_exclude_sets(&entries);
        assert_eq!(dirs, vec!["vendor", "dist"]);
        assert!(globs.is_none());
    }

    #[test]
    fn exclude_glob_patterns_build_glob_set() {
        let entries = vec!["**/README.md".to_string(), "docs/*.txt".to_string()];
        let (dirs, globs) = build_exclude_sets(&entries);
        assert!(dirs.is_empty());
        let gs = globs.expect("should produce a GlobSet");
        assert!(gs.is_match("README.md"));
        assert!(gs.is_match("sub/README.md"));
        assert!(gs.is_match("docs/notes.txt"));
        assert!(!gs.is_match("docs/notes.md"));
    }

    #[test]
    fn exclude_mixed_entries_split_correctly() {
        let entries = vec!["node_modules".to_string(), "**/README.md".to_string()];
        let (dirs, globs) = build_exclude_sets(&entries);
        assert_eq!(dirs, vec!["node_modules"]);
        assert!(globs.is_some());
    }

    #[test]
    fn exclude_glob_filters_files_from_graph() {
        let tmp = std::env::temp_dir().join("anneal_test_exclude_glob");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_md_file(&tmp, "README.md", "---\nstatus: current\n---\nIndex file\n");
        write_md_file(&tmp, "design.md", "---\nstatus: draft\n---\nReal doc\n");

        let config = AnnealConfig {
            exclude: vec!["**/README.md".to_string()],
            ..AnnealConfig::default()
        };
        let root = Utf8Path::from_path(&tmp).unwrap();
        let result = build_graph(root, &config).unwrap();

        let file_ids: Vec<&str> = result
            .graph
            .nodes()
            .filter_map(|(_, h)| match &h.kind {
                HandleKind::File(_) => Some(h.id.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            !file_ids.iter().any(|id| id.contains("README")),
            "README.md should be excluded from graph, got: {file_ids:?}"
        );
        assert!(
            file_ids.iter().any(|id| id.contains("design")),
            "design.md should be in graph, got: {file_ids:?}"
        );
    }
}
