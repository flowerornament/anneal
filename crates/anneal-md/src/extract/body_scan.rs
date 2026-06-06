use std::collections::HashMap;
use std::sync::LazyLock;

use camino::{Utf8Path, Utf8PathBuf};
use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::extract::extraction::{
    DiscoveredRef, ImplausibleReason, LineIndex, RefHint, RefSource, SourceSpan,
};
use crate::extract::graph::EdgeKind;

/// Body-text keywords that imply a DependsOn edge (D-01).
/// "based on" was removed — too common in prose and causes false DependsOn edges.
static DEPENDS_ON_KEYWORDS: &[&str] = &["incorporates", "builds on", "extends"];

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

/// Capture regex for in-repo code path references.
static CODE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<path>[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)+)(?::(?P<start>\d+)(?:-(?P<end>\d+))?(?::\d+)?)?",
    )
    .expect("code path regex must compile")
});

const DEFAULT_CODE_PATH_ROOTS: &[&str] = &["crates", "lib", "src", "app", "test", "priv", "native"];
const EXCLUDED_CODE_PATH_ROOTS: &[&str] = &["_build", "target", "node_modules"];

/// A label match found during content scanning, not yet resolved to a namespace.
pub(crate) struct LabelCandidate {
    pub(crate) prefix: String,
    pub(crate) number: u32,
    pub(crate) file_path: Utf8PathBuf,
    pub(crate) edge_kind: EdgeKind,
    /// Whether this label was found inside a section heading (definition site).
    pub(crate) is_heading: bool,
}

/// Result of scanning a single file's body content.
pub(crate) struct ScanResult {
    pub(crate) label_candidates: Vec<LabelCandidate>,
    /// Heading spans discovered in document order.
    pub(crate) heading_spans: Vec<HeadingSpan>,
    /// Section references with their 1-based line numbers.
    pub(crate) section_refs: Vec<(String, u32)>,
    /// File path references with their 1-based line numbers.
    pub(crate) file_refs: Vec<(String, u32)>,
    /// In-repo code path references discovered in body text.
    pub(crate) code_refs: Vec<CodePathRef>,
}

/// A heading-scoped content span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadingSpan {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) path: String,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
}

/// An in-repo code path reference discovered from markdown body text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodePathRef {
    pub(crate) file: String,
    pub(crate) target: String,
    pub(crate) path: String,
    pub(crate) start_line: Option<u32>,
    pub(crate) end_line: Option<u32>,
    pub(crate) source_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HeadingDraft {
    level: u32,
    title: String,
    start_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveHeading {
    text: String,
    start_offset: usize,
    level: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextRefKind {
    Label,
    Section,
    FilePath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextRefMatch {
    kind: TextRefKind,
    start: usize,
}

// ---------------------------------------------------------------------------
// Edge kind inference
// ---------------------------------------------------------------------------

/// Extract the line containing the byte offset `pos` from `text`.
fn containing_line(text: &str, pos: usize) -> &str {
    let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    &text[start..end]
}

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

fn infer_edge_kind_from_text_ref(text: &str, matched: TextRefMatch) -> EdgeKind {
    match matched.kind {
        TextRefKind::Label | TextRefKind::Section | TextRefKind::FilePath => {
            infer_edge_kind_from_line(containing_line(text, matched.start))
        }
    }
}

struct BodyRefRecorder<'a> {
    file_path: &'a Utf8Path,
    file_path_str: &'a str,
    line: u32,
    is_heading: bool,
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
                    is_heading: self.is_heading,
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
    line_index: &LineIndex,
    code_path_roots: &[String],
) -> (ScanResult, Vec<DiscoveredRef>) {
    let mut result = ScanResult {
        label_candidates: Vec::new(),
        heading_spans: Vec::new(),
        section_refs: Vec::new(),
        file_refs: Vec::new(),
        code_refs: Vec::new(),
    };
    let mut discovered_refs: Vec<DiscoveredRef> = Vec::new();
    let mut heading_drafts = Vec::new();

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts.insert(Options::ENABLE_WIKILINKS);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(body, opts);

    // State tracking
    let mut in_code_block = false;
    let mut active_heading: Option<ActiveHeading> = None;
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

            // Inline code spans are scanned only for code-path references.
            Event::Code(code) => {
                if !in_code_block {
                    scan_code_path_refs(
                        code.as_ref(),
                        range.start,
                        file_path,
                        file_path_str,
                        line_index,
                        code_path_roots,
                        &mut result,
                        &mut discovered_refs,
                    );
                }
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {}

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
                        false,
                        &mut result,
                        &mut discovered_refs,
                        code_path_roots,
                    );
                    html_accumulator.clear();
                }
            }

            // -- Headings --
            Event::Start(Tag::Heading { level, .. }) => {
                active_heading = Some(ActiveHeading {
                    text: String::new(),
                    start_offset: range.start,
                    level: heading_level_number(level),
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(active) = active_heading.take() {
                    let heading = active.text.trim().to_string();
                    if !heading.is_empty() {
                        let start_line = line_index.offset_to_line(active.start_offset);
                        // Scan heading text for label definitions (is_heading = true)
                        scan_text_for_refs(
                            &heading,
                            active.start_offset,
                            file_path,
                            file_path_str,
                            line_index,
                            true,
                            &mut result,
                            &mut discovered_refs,
                            code_path_roots,
                        );
                        heading_drafts.push(HeadingDraft {
                            level: active.level,
                            title: heading,
                            start_line,
                        });
                    }
                }
                // Clear text_accumulator so heading labels aren't double-counted
                // when the next block element flushes
                text_accumulator.clear();
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
                                is_heading: false,
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
                                if record_code_path_ref(
                                    clean_dest,
                                    offset,
                                    file_path,
                                    file_path_str,
                                    line_index,
                                    code_path_roots,
                                    &mut result,
                                    &mut discovered_refs,
                                ) {
                                    continue;
                                }
                                BodyRefRecorder {
                                    file_path,
                                    file_path_str,
                                    line,
                                    is_heading: false,
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
                        false,
                        &mut result,
                        &mut discovered_refs,
                        code_path_roots,
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
                if let Some(ref mut heading) = active_heading {
                    heading.text.push_str(text.as_ref());
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
                        false,
                        &mut result,
                        &mut discovered_refs,
                        code_path_roots,
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
                        false,
                        &mut result,
                        &mut discovered_refs,
                        code_path_roots,
                    );
                }
            }

            Event::SoftBreak | Event::HardBreak => {
                if let Some(ref mut heading) = active_heading {
                    heading.text.push(' ');
                }
                // Preserve newlines so per-line edge kind inference works correctly.
                text_accumulator.push('\n');
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
            false,
            &mut result,
            &mut discovered_refs,
            code_path_roots,
        );
    }

    result.heading_spans = finalize_heading_spans(
        file_path_str,
        &heading_drafts,
        body_end_line(body, line_index),
    );

    (result, discovered_refs)
}

const fn heading_level_number(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn body_end_line(body: &str, line_index: &LineIndex) -> u32 {
    let start_line = line_index.offset_to_line(0);
    let body_lines = u32::try_from(body.lines().count()).unwrap_or(u32::MAX);
    start_line.saturating_add(body_lines.saturating_sub(1))
}

fn finalize_heading_spans(
    file_path: &str,
    headings: &[HeadingDraft],
    body_end_line: u32,
) -> Vec<HeadingSpan> {
    let mut spans = Vec::with_capacity(headings.len());
    let mut slug_stack = Vec::<(u32, String)>::new();
    let mut title_stack = Vec::<(u32, String)>::new();
    let mut path_counts = HashMap::<String, usize>::new();

    for (index, heading) in headings.iter().enumerate() {
        while slug_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            slug_stack.pop();
        }
        while title_stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            title_stack.pop();
        }
        let base_slug = slugify_heading(&heading.title);
        let path_base = slug_path(slug_stack.iter().map(|(_, slug)| slug.as_str()), &base_slug);
        let occurrence = path_counts.entry(path_base).or_insert(0);
        *occurrence += 1;
        let slug = if *occurrence == 1 {
            base_slug
        } else {
            format!("{base_slug}~{occurrence}")
        };
        let full_slug_path = slug_path(slug_stack.iter().map(|(_, slug)| slug.as_str()), &slug);
        let next_boundary = headings
            .iter()
            .skip(index + 1)
            .find(|next| next.level <= heading.level)
            .map_or(body_end_line, |next| next.start_line.saturating_sub(1));
        let end_line = next_boundary.max(heading.start_line);
        spans.push(HeadingSpan {
            id: format!("{file_path}#h/{full_slug_path}"),
            title: heading.title.clone(),
            path: heading_title_path(
                title_stack.iter().map(|(_, title)| title.as_str()),
                &heading.title,
            ),
            start_line: heading.start_line,
            end_line,
        });
        slug_stack.push((heading.level, slug));
        title_stack.push((heading.level, heading.title.clone()));
    }

    spans
}

fn slug_path<'a>(parents: impl Iterator<Item = &'a str>, slug: &'a str) -> String {
    parents
        .chain(std::iter::once(slug))
        .collect::<Vec<_>>()
        .join("/")
}

fn heading_title_path<'a>(parents: impl Iterator<Item = &'a str>, title: &'a str) -> String {
    parents
        .chain(std::iter::once(title))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn slugify_heading(heading: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in heading.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "heading".to_string()
    } else {
        out
    }
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

    // Reject implausible link destinations: single characters, bare type
    // variables (e.g. `T` from `Stream[r](T)` parsed as a markdown link),
    // and other short tokens that can't be file paths.
    if value.len() <= 2
        || (value.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            && !value.contains('/')
            && !value.contains('.'))
    {
        return RefHint::Implausible {
            reason: ImplausibleReason::FreeformProse,
        };
    }

    // Default: FilePath (handle identity)
    RefHint::FilePath
}

#[allow(clippy::too_many_arguments)]
/// Scan accumulated text for labels, section refs, file paths using the same
/// regex patterns as the old scanner. Creates both `LabelCandidate`/ScanResult
/// entries and `DiscoveredRef` entries.
fn scan_text_for_refs(
    text: &str,
    block_start_offset: usize,
    file_path: &Utf8Path,
    file_path_str: &str,
    line_index: &LineIndex,
    is_heading: bool,
    result: &mut ScanResult,
    discovered_refs: &mut Vec<DiscoveredRef>,
    code_path_roots: &[String],
) {
    let line = line_index.offset_to_line(block_start_offset);
    {
        let mut recorder = BodyRefRecorder {
            file_path,
            file_path_str,
            line,
            is_heading,
            result,
            discovered_refs,
        };

        // Labels — infer edge kind per-line, not per-block
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
                let start = caps.get(0).expect("label match always present").start();
                let edge_kind = infer_edge_kind_from_text_ref(
                    text,
                    TextRefMatch {
                        kind: TextRefKind::Label,
                        start,
                    },
                );
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
                let start = caps
                    .get(0)
                    .expect("section ref match always present")
                    .start();
                let edge_kind = infer_edge_kind_from_text_ref(
                    text,
                    TextRefMatch {
                        kind: TextRefKind::Section,
                        start,
                    },
                );
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
            let edge_kind = infer_edge_kind_from_text_ref(
                text,
                TextRefMatch {
                    kind: TextRefKind::FilePath,
                    start: m.start(),
                },
            );
            recorder.record(path, RefHint::FilePath, &edge_kind);
        }
    }

    scan_code_path_refs(
        text,
        block_start_offset,
        file_path,
        file_path_str,
        line_index,
        code_path_roots,
        result,
        discovered_refs,
    );
}

#[allow(clippy::too_many_arguments)]
fn scan_code_path_refs(
    text: &str,
    block_start_offset: usize,
    file_path: &Utf8Path,
    file_path_str: &str,
    line_index: &LineIndex,
    code_path_roots: &[String],
    result: &mut ScanResult,
    discovered_refs: &mut Vec<DiscoveredRef>,
) {
    for captures in CODE_PATH_RE.captures_iter(text) {
        let Some(path_match) = captures.name("path") else {
            continue;
        };
        if !has_code_path_left_boundary(text, path_match.start()) {
            continue;
        }
        let path = trim_code_path_punctuation(path_match.as_str());
        if path.is_empty() || !is_recognized_code_path(path, code_path_roots) {
            continue;
        }
        let start_line = captures
            .name("start")
            .and_then(|m| m.as_str().parse::<u32>().ok());
        let end_line = captures
            .name("end")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .or(start_line);
        let source_line = line_index.offset_to_line(block_start_offset + path_match.start());
        record_normalized_code_path_ref(
            path,
            start_line,
            end_line,
            source_line,
            file_path,
            file_path_str,
            result,
            discovered_refs,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn record_code_path_ref(
    raw: &str,
    block_start_offset: usize,
    file_path: &Utf8Path,
    file_path_str: &str,
    line_index: &LineIndex,
    code_path_roots: &[String],
    result: &mut ScanResult,
    discovered_refs: &mut Vec<DiscoveredRef>,
) -> bool {
    let Some(captures) = CODE_PATH_RE.captures(raw) else {
        return false;
    };
    let Some(path_match) = captures.name("path") else {
        return false;
    };
    if path_match.start() != 0 {
        return false;
    }
    let matched = captures
        .get(0)
        .map_or("", |m| trim_code_path_punctuation(m.as_str()));
    if matched.len() != trim_code_path_punctuation(raw).len() {
        return false;
    }
    let path = trim_code_path_punctuation(path_match.as_str());
    if path.is_empty() || !is_recognized_code_path(path, code_path_roots) {
        return false;
    }
    let start_line = captures
        .name("start")
        .and_then(|m| m.as_str().parse::<u32>().ok());
    let end_line = captures
        .name("end")
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .or(start_line);
    let source_line = line_index.offset_to_line(block_start_offset);
    record_normalized_code_path_ref(
        path,
        start_line,
        end_line,
        source_line,
        file_path,
        file_path_str,
        result,
        discovered_refs,
    );
    true
}

#[allow(clippy::too_many_arguments)]
fn record_normalized_code_path_ref(
    path: &str,
    start_line: Option<u32>,
    end_line: Option<u32>,
    source_line: u32,
    file_path: &Utf8Path,
    file_path_str: &str,
    result: &mut ScanResult,
    discovered_refs: &mut Vec<DiscoveredRef>,
) {
    let end_line = match (start_line, end_line) {
        (Some(start), Some(end)) if end < start => Some(start),
        (_, end) => end,
    };
    let target = code_ref_target(path, start_line, end_line);
    result.code_refs.push(CodePathRef {
        file: file_path_str.to_string(),
        target: target.clone(),
        path: path.to_string(),
        start_line,
        end_line,
        source_line,
    });
    discovered_refs.push(DiscoveredRef {
        raw: target,
        hint: RefHint::External,
        source: RefSource::Body,
        edge_kind: EdgeKind::Cites,
        inverse: false,
        span: Some(SourceSpan {
            file: file_path.to_string(),
            line: source_line,
        }),
    });
}

fn code_ref_target(path: &str, start_line: Option<u32>, end_line: Option<u32>) -> String {
    match (start_line, end_line) {
        (Some(start), Some(end)) if end != start => format!("{path}:{start}-{end}"),
        (Some(start), _) => format!("{path}:{start}"),
        _ => path.to_string(),
    }
}

fn trim_code_path_punctuation(path: &str) -> &str {
    path.trim_end_matches(['.', ',', ';', ':', '!', '?'])
}

fn has_code_path_left_boundary(text: &str, start: usize) -> bool {
    if start == 0 {
        return true;
    }
    let Some(previous) = text[..start].chars().next_back() else {
        return true;
    };
    !matches!(
        previous,
        '/' | ':' | '.' | '-' | '_' | '#' | '@' | 'A'..='Z' | 'a'..='z' | '0'..='9'
    )
}

fn is_recognized_code_path(path: &str, code_path_roots: &[String]) -> bool {
    let Some(root) = path.split('/').next() else {
        return false;
    };
    if EXCLUDED_CODE_PATH_ROOTS.contains(&root)
        || std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        || path.contains("...")
    {
        return false;
    }
    let Some(last) = path.rsplit('/').next() else {
        return false;
    };
    if !last.contains('.') {
        return false;
    }
    DEFAULT_CODE_PATH_ROOTS.contains(&root)
        || code_path_roots
            .iter()
            .map(|root| root.trim_matches('/'))
            .any(|configured| configured == root)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // scan_file_cmark tests
    // -----------------------------------------------------------------------

    fn cmark_scan(body: &str) -> (ScanResult, Vec<DiscoveredRef>) {
        let line_index = LineIndex::from_content(body, 0);
        scan_file_cmark(body, Utf8Path::new("test.md"), &line_index, &[])
    }

    #[test]
    fn cmark_code_block_skipping() {
        let body = "## Heading\nSome OQ-64 ref\n```\nOQ-99 in code\n```\n";
        let (result, _refs) = cmark_scan(body);

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

        // Headings are spans, not section handles.
        assert_eq!(result.heading_spans.len(), 1);
        assert_eq!(result.heading_spans[0].id, "test.md#h/heading");
        assert_eq!(result.heading_spans[0].title, "Heading");
        assert_eq!(result.heading_spans[0].path, "Heading");
    }

    #[test]
    fn cmark_extracts_code_path_refs_from_text_and_inline_code() {
        let body = "See `lib/example/admission.rs:142-167` and crates/anneal-core/src/lib.rs:12.\n";
        let (result, refs) = cmark_scan(body);

        let targets: Vec<_> = result
            .code_refs
            .iter()
            .map(|reference| reference.target.as_str())
            .collect();
        assert_eq!(
            targets,
            vec![
                "lib/example/admission.rs:142-167",
                "crates/anneal-core/src/lib.rs:12",
            ]
        );
        assert!(
            refs.iter()
                .filter(|reference| reference.hint == RefHint::External)
                .count()
                >= 2,
            "code refs should be recorded as external refs: {refs:?}"
        );
        assert!(
            result.file_refs.is_empty(),
            "code refs should not become markdown file refs"
        );
    }

    #[test]
    fn cmark_extracts_project_configured_code_roots() {
        let body = "The handler lives at `web/controllers/page_controller.ex:9`.\n";
        let line_index = LineIndex::from_content(body, 0);
        let roots = vec!["web".to_string()];
        let (result, _refs) = scan_file_cmark(body, Utf8Path::new("test.md"), &line_index, &roots);

        assert_eq!(result.code_refs.len(), 1);
        assert_eq!(
            result.code_refs[0].target,
            "web/controllers/page_controller.ex:9"
        );
    }

    #[test]
    fn cmark_skips_code_refs_inside_fenced_code_blocks() {
        let body = "```\nlib/example/admission.rs:142-167\n```\n";
        let (result, _refs) = cmark_scan(body);

        assert!(result.code_refs.is_empty());
    }

    #[test]
    fn cmark_skips_ellipsized_code_path_placeholders() {
        let body = "Example placeholder: `lib/example/...file.ex`.\n";
        let (result, _refs) = cmark_scan(body);

        assert!(result.code_refs.is_empty());
    }

    #[test]
    fn cmark_skips_code_path_substrings_inside_urls() {
        let body = "External URL: https://example.com/src/main.rs for background.\n";
        let (result, _refs) = cmark_scan(body);

        assert!(result.code_refs.is_empty());
    }

    #[test]
    fn cmark_heading_spans_use_structural_ids_and_scopes() {
        let body = "# Architecture\nIntro\n\n## Lease Protocol\nBody\n\n### Renewal\nNested\n\n## Lease Protocol\nSecond\n";
        let (result, _refs) = cmark_scan(body);

        let spans = result
            .heading_spans
            .iter()
            .map(|span| {
                (
                    span.id.as_str(),
                    span.title.as_str(),
                    span.path.as_str(),
                    span.start_line,
                    span.end_line,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            spans,
            vec![
                (
                    "test.md#h/architecture",
                    "Architecture",
                    "Architecture",
                    1,
                    11,
                ),
                (
                    "test.md#h/architecture/lease-protocol",
                    "Lease Protocol",
                    "Architecture / Lease Protocol",
                    4,
                    9,
                ),
                (
                    "test.md#h/architecture/lease-protocol/renewal",
                    "Renewal",
                    "Architecture / Lease Protocol / Renewal",
                    7,
                    9,
                ),
                (
                    "test.md#h/architecture/lease-protocol~2",
                    "Lease Protocol",
                    "Architecture / Lease Protocol",
                    10,
                    11,
                ),
            ]
        );
    }

    #[test]
    fn cmark_inline_code_skipping() {
        let body = "See `OQ-64` inline\n";
        let (result, refs) = cmark_scan(body);
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
        let (result, refs) = cmark_scan(body);
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
        let (_result, refs) = cmark_scan(body);
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
        let (result, _refs) = cmark_scan(body);
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
        let (result, refs) = cmark_scan(body);
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
        let (_, refs) = cmark_scan(body);
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
        let (result, refs) = cmark_scan(body);
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
        let (result, refs) = cmark_scan(body);
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
        let (result, _refs) = cmark_scan(body);
        assert!(
            result.section_refs.iter().any(|(r, _)| r == "4.1"),
            "should extract section ref 4.1, got: {:?}",
            result.section_refs
        );
    }

    #[test]
    fn cmark_url_rejection_in_text() {
        let body = "See https://example.com/rust-lang/guide.md for details\n";
        let (result, _) = cmark_scan(body);
        assert!(
            result.file_refs.is_empty(),
            "URL fragments should not be matched as file refs, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_version_dot_rejection() {
        let body = "See formal-model/sample-algebra-v1.2.md for details\n";
        let (result, _) = cmark_scan(body);
        assert!(
            !result.file_refs.iter().any(|(r, _)| r == "2.md"),
            "should not match fragment 2.md from v1.2.md, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_hyphen_prefix_rejection() {
        let body = "See RQ-01-program-format-encoding.md for details\n";
        let (result, _) = cmark_scan(body);
        assert!(
            !result.file_refs.iter().any(|(r, _)| r.starts_with('-')),
            "should not extract hyphen-prefixed fragments, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_mid_word_rejection() {
        let body = "[transcript](refs/2026-02-06-4eJrp9byBRk.md)";
        let (_, refs) = cmark_scan(body);
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
        let (_, refs) = cmark_scan(body);
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
        let (result, _refs) = cmark_scan(body);
        assert!(
            result.file_refs.iter().any(|(r, _)| r == "foo.md"),
            "should extract foo.md stripping fragment, got: {:?}",
            result.file_refs
        );
    }

    #[test]
    fn cmark_external_links_skipped() {
        let body = "[google](https://google.com)\n";
        let (result, refs) = cmark_scan(body);
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
        let (result, refs) = cmark_scan(body);
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
        let (result, _) = cmark_scan(body);
        let labels: Vec<u32> = result.label_candidates.iter().map(|c| c.number).collect();
        assert!(labels.contains(&1), "should extract OQ-1, got: {labels:?}");
        assert!(
            !labels.contains(&2),
            "should NOT extract OQ-2 from indented code, got: {labels:?}"
        );
        assert!(labels.contains(&3), "should extract OQ-3, got: {labels:?}");
    }

    // -----------------------------------------------------------------------
    // Edge kind: per-line inference, not per-block
    // -----------------------------------------------------------------------

    #[test]
    fn edge_kind_inference_is_per_line_not_per_block() {
        // "incorporates" on line 1 should NOT promote OQ-99 on line 2 within the same paragraph
        let body = "This incorporates OQ-42 into the design\nSee also OQ-99 for context\n";
        let (result, _) = cmark_scan(body);
        let oq42 = result
            .label_candidates
            .iter()
            .find(|c| c.number == 42)
            .expect("OQ-42 should be found");
        let oq99 = result
            .label_candidates
            .iter()
            .find(|c| c.number == 99)
            .expect("OQ-99 should be found");
        assert_eq!(
            oq42.edge_kind,
            EdgeKind::DependsOn,
            "OQ-42 is on the 'incorporates' line"
        );
        assert_eq!(
            oq99.edge_kind,
            EdgeKind::Cites,
            "OQ-99 is on a different line — should be Cites"
        );
    }

    #[test]
    fn based_on_no_longer_promotes_to_depends_on() {
        let body = "This is based on OQ-10\n";
        let (result, _) = cmark_scan(body);
        assert!(!result.label_candidates.is_empty());
        assert_eq!(
            result.label_candidates[0].edge_kind,
            EdgeKind::Cites,
            "'based on' should no longer promote to DependsOn"
        );
    }

    // -----------------------------------------------------------------------
    // Body ref: implausible link destinations rejected
    // -----------------------------------------------------------------------

    #[test]
    fn classify_body_ref_rejects_single_char_type_var() {
        let hint = classify_body_ref("T");
        assert!(
            matches!(hint, RefHint::Implausible { .. }),
            "single char 'T' should be implausible, got: {hint:?}"
        );
    }

    #[test]
    fn classify_body_ref_rejects_short_uppercase_token() {
        let hint = classify_body_ref("FOO");
        assert!(
            matches!(hint, RefHint::Implausible { .. }),
            "bare uppercase 'FOO' should be implausible, got: {hint:?}"
        );
    }

    #[test]
    fn classify_body_ref_accepts_md_file_path() {
        let hint = classify_body_ref("design.md");
        assert!(
            matches!(hint, RefHint::FilePath),
            "design.md should be FilePath, got: {hint:?}"
        );
    }

    #[test]
    fn classify_body_ref_accepts_label() {
        let hint = classify_body_ref("OQ-42");
        assert!(
            matches!(hint, RefHint::Label { .. }),
            "OQ-42 should be Label, got: {hint:?}"
        );
    }

    // -----------------------------------------------------------------------
    // containing_line helper
    // -----------------------------------------------------------------------

    #[test]
    fn containing_line_extracts_correct_line() {
        let text = "first line\nsecond line\nthird line";
        assert_eq!(containing_line(text, 0), "first line");
        assert_eq!(containing_line(text, 11), "second line");
        assert_eq!(containing_line(text, 23), "third line");
    }

    // -----------------------------------------------------------------------
    // Corpus smoke test — validate cmark scanner on a local real-world corpus
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
            let line_index = LineIndex::from_content(&content, 0);
            let (result, body_discovered) = scan_file_cmark(&content, relative, &line_index, &[]);

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
    #[ignore = "requires ANNEAL_SMOKE_CORPUS_ROOT"]
    fn corpus_smoke_external() {
        let root = std::env::var("ANNEAL_SMOKE_CORPUS_ROOT")
            .expect("ANNEAL_SMOKE_CORPUS_ROOT must point at a markdown corpus");
        corpus_smoke_test(&root, "external");
    }
}
