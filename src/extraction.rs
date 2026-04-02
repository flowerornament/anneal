use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::graph::EdgeKind;
use crate::handle::HandleMetadata;

// ---------------------------------------------------------------------------
// Reference classification types
// ---------------------------------------------------------------------------

/// Classification hint for a discovered reference.
///
/// Determined during extraction before resolution. Resolution (Phase 6)
/// uses the hint to select the appropriate resolution strategy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum RefHint {
    /// Label reference like "OQ-64" or "KB-D1".
    Label { prefix: String, number: u32 },
    /// File path like "foo.md" or "subdir/bar.md".
    FilePath,
    /// Section cross-reference like "section:4.1".
    SectionRef,
    /// External URL like "https://example.com".
    External,
    /// Rejected as implausible: absolute path, prose, wildcard, etc.
    Implausible { reason: String },
}

/// Where a reference was discovered within a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum RefSource {
    /// From a YAML frontmatter field.
    Frontmatter { field: String },
    /// From body text scanning.
    Body,
}

/// Source location of a discovered reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SourceSpan {
    /// File path (relative to corpus root).
    pub(crate) file: String,
    /// 1-based line number within the file.
    pub(crate) line: u32,
}

/// Index for converting byte offsets to 1-based line numbers in O(log n).
///
/// Accounts for frontmatter offset so that body byte 0 maps to the correct
/// file-relative line number.
pub(crate) struct LineIndex {
    /// Byte offsets where each newline occurs in the content.
    newline_offsets: Vec<usize>,
    /// 1-based line number of the first byte (accounts for frontmatter).
    base_line: u32,
}

impl LineIndex {
    /// Build a `LineIndex` from body content.
    ///
    /// `frontmatter_line_count` is the number of lines consumed by frontmatter
    /// (including the opening `---` but NOT the closing `---`). The closing
    /// `---` line is accounted for by the +1 in `base_line`.
    ///
    /// If `frontmatter_line_count == 0`, body starts at file line 1.
    pub(crate) fn from_content(content: &str, frontmatter_line_count: u32) -> Self {
        let newline_offsets: Vec<usize> = content
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| if b == b'\n' { Some(i) } else { None })
            .collect();
        let base_line = if frontmatter_line_count == 0 {
            1
        } else {
            // frontmatter lines + closing --- line + 1 for 1-based
            frontmatter_line_count + 1 + 1
        };
        Self {
            newline_offsets,
            base_line,
        }
    }

    /// Convert a byte offset within the body to a 1-based file line number.
    ///
    /// Uses binary search for O(log n) performance.
    pub(crate) fn offset_to_line(&self, byte_offset: usize) -> u32 {
        // partition_point returns the number of newlines strictly before byte_offset
        let lines_before = self.newline_offsets.partition_point(|&nl| nl < byte_offset);
        #[allow(clippy::cast_possible_truncation)] // line count will never exceed u32::MAX
        let lines = lines_before as u32;
        self.base_line + lines
    }
}

/// A reference discovered during file extraction, before resolution.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiscoveredRef {
    /// Original string as found in the source.
    pub(crate) raw: String,
    /// Classification hint.
    pub(crate) hint: RefHint,
    /// Where in the file this was found.
    pub(crate) source: RefSource,
    /// What edge type this reference implies.
    pub(crate) edge_kind: EdgeKind,
    /// If true, the actual graph edge is target -> source (inverse direction).
    pub(crate) inverse: bool,
    /// Source location (file + line). None until populated by the scanner.
    pub(crate) span: Option<SourceSpan>,
}

/// Uniform per-file extraction output.
///
/// Collects all information extracted from a single markdown file before
/// resolution. Runs alongside existing `ScanResult`/`PendingEdge` types
/// (not replacing them).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct FileExtraction {
    /// File path relative to the corpus root.
    pub(crate) file: String,
    /// Frontmatter `status` value.
    pub(crate) status: Option<String>,
    /// Extracted metadata from frontmatter.
    pub(crate) metadata: HandleMetadata,
    /// All discovered references.
    pub(crate) refs: Vec<DiscoveredRef>,
    /// All frontmatter keys observed.
    pub(crate) all_keys: Vec<String>,
}

// ---------------------------------------------------------------------------
// Snippet extraction
// ---------------------------------------------------------------------------

/// Extract the first paragraph snippet from a file body.
pub(crate) fn extract_file_snippet_from_body(body: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && !lines.is_empty() {
            break;
        }
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            lines.push(trimmed);
        }
    }

    if lines.is_empty() {
        return None;
    }

    Some(truncate_snippet(&lines.join(" "), 200))
}

/// Extract the first heading-qualified line mentioning `label_id`.
pub(crate) fn extract_label_snippet_from_content(content: &str, label_id: &str) -> Option<String> {
    let mut heading = String::new();
    for line in content.lines() {
        if line.starts_with('#') {
            heading = line.trim_start_matches('#').trim().to_string();
        }
        if line.contains(label_id) {
            let context = if heading.is_empty() {
                line.trim().to_string()
            } else {
                format!("{heading}: {}", line.trim())
            };
            return Some(truncate_snippet(&context, 200));
        }
    }

    None
}

fn truncate_snippet(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        return s.to_string();
    }

    let mut cut_indices = Vec::new();
    for (count, (idx, ch)) in s.char_indices().enumerate() {
        if count >= max_len {
            break;
        }
        if ch == ' ' {
            cut_indices.push(idx);
        }
    }

    let end = cut_indices.last().copied().unwrap_or_else(|| {
        s.char_indices()
            .nth(max_len)
            .map_or(s.len(), |(idx, _)| idx)
    });

    format!("{}...", s[..end].trim_end())
}

// ---------------------------------------------------------------------------
// Classification regexes (anchored, for exact matching)
// ---------------------------------------------------------------------------

/// Label pattern anchored for exact match (unlike parse.rs's unanchored scanner).
/// Supports compound prefixes like "KB-D1" (prefix="KB-D", number=1) as well as
/// simple "OQ-64" (prefix="OQ", number=64). The optional hyphen before digits
/// handles both formats.
static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Z][A-Z_]*(?:-[A-Z][A-Z_]*)*)-?(\d+)$").expect("label regex must compile")
});

/// Section reference pattern like "section:4.1".
static SECTION_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^section:(\d+(?:\.\d+)*)$").expect("section ref regex must compile")
});

// ---------------------------------------------------------------------------
// Frontmatter value classification
// ---------------------------------------------------------------------------

/// Classify a frontmatter value string into a `RefHint`.
///
/// Applied to each scalar value found in frontmatter reference fields
/// (depends-on, verifies, discharges, superseded-by). First match wins.
pub(crate) fn classify_frontmatter_value(value: &str) -> RefHint {
    let trimmed = value.trim();

    // 1. URL
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return RefHint::External;
    }

    // 2. Absolute path
    if trimmed.starts_with('/') || trimmed.starts_with("~/") {
        return RefHint::Implausible {
            reason: "absolute path".into(),
        };
    }

    // 3. Wildcard
    if trimmed.contains('*') || trimmed.contains('?') {
        return RefHint::Implausible {
            reason: "wildcard pattern".into(),
        };
    }

    // 4. Comma-separated list (check before prose since comma lists also have spaces)
    if trimmed.contains(", ") && trimmed.len() > 40 {
        return RefHint::Implausible {
            reason: "comma-separated list".into(),
        };
    }

    // 5. Freeform prose: has space, not a .md path, not a label
    if trimmed.contains(' ')
        && !std::path::Path::new(trimmed)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        && !LABEL_RE.is_match(trimmed)
    {
        return RefHint::Implausible {
            reason: "freeform prose".into(),
        };
    }

    // 6. Section ref
    if SECTION_REF_RE.is_match(trimmed) {
        return RefHint::SectionRef;
    }

    // 7. Label
    if let Some(caps) = LABEL_RE.captures(trimmed)
        && let Ok(number) = caps[2].parse::<u32>()
    {
        return RefHint::Label {
            prefix: caps[1].to_string(),
            number,
        };
    }

    // 8. File path (.md extension)
    if std::path::Path::new(trimmed)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return RefHint::FilePath;
    }

    // 9. Default: treat as potential handle identity
    RefHint::FilePath
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- classify_frontmatter_value tests --

    #[test]
    fn classify_https_url() {
        assert_eq!(
            classify_frontmatter_value("https://modal.com/pricing"),
            RefHint::External
        );
    }

    #[test]
    fn classify_http_url() {
        assert_eq!(
            classify_frontmatter_value("http://example.com"),
            RefHint::External
        );
    }

    #[test]
    fn classify_absolute_path() {
        assert_eq!(
            classify_frontmatter_value("/absolute/path.md"),
            RefHint::Implausible {
                reason: "absolute path".into()
            }
        );
    }

    #[test]
    fn classify_tilde_path() {
        assert_eq!(
            classify_frontmatter_value("~/home/path.md"),
            RefHint::Implausible {
                reason: "absolute path".into()
            }
        );
    }

    #[test]
    fn classify_wildcard_star() {
        assert_eq!(
            classify_frontmatter_value("*.md"),
            RefHint::Implausible {
                reason: "wildcard pattern".into()
            }
        );
    }

    #[test]
    fn classify_wildcard_globstar() {
        assert_eq!(
            classify_frontmatter_value("src/**/*.rs"),
            RefHint::Implausible {
                reason: "wildcard pattern".into()
            }
        );
    }

    #[test]
    fn classify_freeform_prose() {
        assert_eq!(
            classify_frontmatter_value("claude-desktop session"),
            RefHint::Implausible {
                reason: "freeform prose".into()
            }
        );
    }

    #[test]
    fn classify_freeform_prose_with_numbers() {
        assert_eq!(
            classify_frontmatter_value("20+ academic papers"),
            RefHint::Implausible {
                reason: "freeform prose".into()
            }
        );
    }

    #[test]
    fn classify_comma_separated_list() {
        assert_eq!(
            classify_frontmatter_value(
                "GitHub repos, community forums, HN threads, industry reports"
            ),
            RefHint::Implausible {
                reason: "comma-separated list".into()
            }
        );
    }

    #[test]
    fn classify_label_oq() {
        assert_eq!(
            classify_frontmatter_value("OQ-64"),
            RefHint::Label {
                prefix: "OQ".into(),
                number: 64
            }
        );
    }

    #[test]
    fn classify_label_compound_prefix() {
        assert_eq!(
            classify_frontmatter_value("KB-D1"),
            RefHint::Label {
                prefix: "KB-D".into(),
                number: 1
            }
        );
    }

    #[test]
    fn classify_file_path_md() {
        assert_eq!(classify_frontmatter_value("foo.md"), RefHint::FilePath);
    }

    #[test]
    fn classify_file_path_subdir() {
        assert_eq!(
            classify_frontmatter_value("subdir/bar.md"),
            RefHint::FilePath
        );
    }

    #[test]
    fn classify_section_ref() {
        assert_eq!(
            classify_frontmatter_value("section:4.1"),
            RefHint::SectionRef
        );
    }

    #[test]
    fn classify_bare_word_as_filepath() {
        // "claude-desktop" has no space, no .md -- default to FilePath
        assert_eq!(
            classify_frontmatter_value("claude-desktop"),
            RefHint::FilePath
        );
    }

    #[test]
    fn classify_trims_whitespace() {
        assert_eq!(
            classify_frontmatter_value("  https://example.com  "),
            RefHint::External
        );
    }

    // -- DiscoveredRef construction tests --

    #[test]
    fn discovered_ref_all_variants() {
        let hints = vec![
            RefHint::Label {
                prefix: "OQ".into(),
                number: 1,
            },
            RefHint::FilePath,
            RefHint::SectionRef,
            RefHint::External,
            RefHint::Implausible {
                reason: "test".into(),
            },
        ];

        for hint in hints {
            let r = DiscoveredRef {
                raw: "test".into(),
                hint,
                source: RefSource::Body,
                edge_kind: EdgeKind::Cites,
                inverse: false,
                span: None,
            };
            // Just verify it can be constructed and debug-printed
            let _ = format!("{r:?}");
        }
    }

    // -- FileExtraction construction test --

    #[test]
    fn file_extraction_empty() {
        let fe = FileExtraction {
            file: "doc.md".into(),
            status: None,
            metadata: HandleMetadata::default(),
            refs: vec![],
            all_keys: vec![],
        };
        assert!(fe.refs.is_empty());
        assert!(fe.status.is_none());
    }

    // -- SourceSpan tests --

    #[test]
    fn source_span_constructable_and_eq() {
        let span = SourceSpan {
            file: "foo.md".into(),
            line: 42,
        };
        assert_eq!(span.file, "foo.md");
        assert_eq!(span.line, 42);

        let span2 = SourceSpan {
            file: "foo.md".into(),
            line: 42,
        };
        assert_eq!(span, span2);
    }

    #[test]
    fn source_span_serializable() {
        let span = SourceSpan {
            file: "bar.md".into(),
            line: 7,
        };
        let json = serde_json::to_string(&span).expect("serialize SourceSpan");
        assert!(json.contains("\"file\":\"bar.md\""));
        assert!(json.contains("\"line\":7"));
    }

    // -- LineIndex tests --

    #[test]
    fn line_index_first_line() {
        let idx = LineIndex::from_content("line1\nline2\nline3", 0);
        assert_eq!(idx.offset_to_line(0), 1, "byte 0 = line 1");
    }

    #[test]
    fn line_index_second_line() {
        // "line1\n" = 6 bytes, so byte 6 is start of line 2
        let idx = LineIndex::from_content("line1\nline2\nline3", 0);
        assert_eq!(idx.offset_to_line(6), 2, "byte 6 = line 2");
    }

    #[test]
    fn line_index_third_line() {
        // "line1\nline2\n" = 12 bytes, so byte 12 is start of line 3
        let idx = LineIndex::from_content("line1\nline2\nline3", 0);
        assert_eq!(idx.offset_to_line(12), 3, "byte 12 = line 3");
    }

    #[test]
    fn line_index_with_frontmatter_offset() {
        // Frontmatter has 3 lines (e.g. "---\nstatus: active\n---\n"),
        // so frontmatter_line_count = 3 (opening --- + content + closing ---).
        // Actually, frontmatter_line_count does NOT include closing ---,
        // so if we have "---\nstatus: active\n", that's 2 lines.
        // base_line = 2 + 1 + 1 = 4 (closing --- is line 3, body starts line 4).
        let idx = LineIndex::from_content("body line 1\nbody line 2\n", 2);
        assert_eq!(idx.offset_to_line(0), 4, "body byte 0 = file line 4");
        assert_eq!(idx.offset_to_line(12), 5, "body byte 12 = file line 5");
    }

    #[test]
    fn line_index_offset_beyond_content() {
        let idx = LineIndex::from_content("short", 0);
        // offset beyond content should return last line (line 1 for single-line content)
        assert_eq!(idx.offset_to_line(100), 1, "beyond content = last line");
    }

    #[test]
    fn line_index_empty_content() {
        let idx = LineIndex::from_content("", 0);
        assert_eq!(idx.offset_to_line(0), 1, "empty content byte 0 = line 1");
    }

    #[test]
    fn line_index_empty_content_with_offset() {
        let idx = LineIndex::from_content("", 5);
        // base_line = 5 + 1 + 1 = 7
        assert_eq!(
            idx.offset_to_line(0),
            7,
            "empty content with frontmatter offset"
        );
    }

    #[test]
    fn discovered_ref_with_span() {
        let r = DiscoveredRef {
            raw: "OQ-1".into(),
            hint: RefHint::Label {
                prefix: "OQ".into(),
                number: 1,
            },
            source: RefSource::Body,
            edge_kind: EdgeKind::Cites,
            inverse: false,
            span: Some(SourceSpan {
                file: "test.md".into(),
                line: 10,
            }),
        };
        assert_eq!(r.span.as_ref().expect("span present").line, 10);
    }
}
