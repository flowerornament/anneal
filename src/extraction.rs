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
#[allow(dead_code)] // Variants used by Phase 4 Plan 02 wiring
pub(crate) enum RefSource {
    /// From a YAML frontmatter field.
    Frontmatter { field: String },
    /// From body text scanning.
    Body,
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
}

/// Uniform per-file extraction output.
///
/// Collects all information extracted from a single markdown file before
/// resolution. Runs alongside existing `ScanResult`/`PendingEdge` types
/// (not replacing them).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct FileExtraction {
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
            };
            // Just verify it can be constructed and debug-printed
            let _ = format!("{r:?}");
        }
    }

    // -- FileExtraction construction test --

    #[test]
    fn file_extraction_empty() {
        let fe = FileExtraction {
            status: None,
            metadata: HandleMetadata::default(),
            refs: vec![],
            all_keys: vec![],
        };
        assert!(fe.refs.is_empty());
        assert!(fe.status.is_none());
    }
}
