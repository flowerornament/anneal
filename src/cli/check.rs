use std::collections::{HashMap, HashSet};
use std::io::Write;

use serde::Serialize;

use crate::checks::{self, Diagnostic, DiagnosticCode, Severity};
use crate::output::{Line, Location, Printer, Render};

use super::{DetailLevel, OutputMeta};

// ---------------------------------------------------------------------------
// Check command (CLI-01)
// ---------------------------------------------------------------------------

/// Output of `anneal check`: diagnostics with summary counts.
#[derive(Serialize)]
pub(crate) struct CheckOutput {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) info: usize,
    pub(crate) suggestions: usize,
    /// Errors sourced from terminal (settled) files — informational, not actionable.
    pub(crate) terminal_errors: usize,
}

#[derive(Serialize)]
pub(crate) struct CodeCount {
    pub(crate) code: String,
    pub(crate) count: usize,
}

#[derive(Serialize)]
pub(crate) struct CheckSummary {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) info: usize,
    pub(crate) suggestions: usize,
    pub(crate) terminal_errors: usize,
    pub(crate) total_diagnostics: usize,
}

#[derive(Serialize)]
pub(crate) struct ExtractionSummary {
    pub(crate) files: usize,
    pub(crate) refs: usize,
    pub(crate) frontmatter_refs: usize,
    pub(crate) body_refs: usize,
    pub(crate) by_hint: Vec<HintCount>,
}

#[derive(Serialize)]
pub(crate) struct HintCount {
    pub(crate) hint: &'static str,
    pub(crate) count: usize,
}

#[derive(Serialize)]
pub(crate) struct CheckJsonOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) summary: CheckSummary,
    pub(crate) by_code: Vec<CodeCount>,
    pub(crate) sample_diagnostics: Vec<checks::DiagnosticRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<Vec<checks::DiagnosticRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extractions_summary: Option<ExtractionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extractions: Option<Vec<crate::extraction::FileExtraction>>,
}

pub(crate) struct CheckJsonOptions {
    pub(crate) include_diagnostics: bool,
    pub(crate) diagnostics_limit: usize,
    pub(crate) include_extractions_summary: bool,
    pub(crate) include_full_extractions: bool,
    pub(crate) full: bool,
}

impl Render for CheckOutput {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        for diag in &self.diagnostics {
            render_diagnostic(p, diag)?;
        }
        if !self.diagnostics.is_empty() {
            p.blank()?;
        }
        // Summary tally. The first entry is "errors", possibly with a
        // `(N in terminal)` annotation when terminal errors split the total.
        let mut tally_parts: Vec<(usize, &str)> = vec![
            (self.errors, "errors"),
            (self.warnings, "warnings"),
            (self.info, "info"),
            (self.suggestions, "suggestions"),
        ];
        if self.errors == 1 {
            tally_parts[0] = (self.errors, "error");
        }
        if self.warnings == 1 {
            tally_parts[1] = (self.warnings, "warning");
        }
        if self.suggestions == 1 {
            tally_parts[3] = (self.suggestions, "suggestion");
        }
        p.tally(&tally_parts)?;
        if self.terminal_errors > 0 {
            let active = self.errors.saturating_sub(self.terminal_errors);
            p.line_at(
                4,
                &Line::new().dim(format!(
                    "{active} in active files, {} in terminal",
                    self.terminal_errors
                )),
            )?;
        }

        let has_e002 = self
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E002);
        if self.errors > 0 || self.suggestions > 0 {
            p.blank()?;
            let mut hints: Vec<(&str, &str)> = Vec::new();
            if has_e002 {
                hints.push((
                    "anneal explain obligation <handle>",
                    "inspect a specific obligation",
                ));
            }
            hints.push(("anneal garden", "ranked maintenance tasks"));
            p.hints(&hints)?;
        }
        Ok(())
    }
}

/// Render a single diagnostic through the shared Printer.
pub(crate) fn render_diagnostic<W: Write>(
    p: &mut Printer<W>,
    diag: &Diagnostic,
) -> std::io::Result<()> {
    let at = diag
        .file
        .as_deref()
        .map(|path| Location::new(path, diag.line));
    let code = diag.code.to_string();
    p.diagnostic(diag.severity.to_output(), &code, &diag.message, at)
}

/// Filter flags for the check command (D-19).
///
/// Combined with OR logic when multiple are set. If all are false, all
/// diagnostics are shown (default behavior).
#[derive(Default)]
pub(crate) struct CheckFilters {
    pub(crate) errors_only: bool,
    pub(crate) suggest: bool,
    pub(crate) stale: bool,
    pub(crate) obligations: bool,
    pub(crate) active_only: bool,
}

impl CheckFilters {
    fn any_severity_filter(&self) -> bool {
        self.errors_only || self.suggest || self.stale || self.obligations
    }
}

pub(crate) fn apply_check_filters(
    mut diagnostics: Vec<checks::Diagnostic>,
    filters: &CheckFilters,
    terminal_files: &HashSet<String>,
) -> Vec<checks::Diagnostic> {
    if filters.active_only {
        diagnostics.retain(|d| d.file.as_ref().is_none_or(|f| !terminal_files.contains(f)));
    }
    if filters.any_severity_filter() {
        diagnostics.retain(|d| {
            (filters.errors_only && d.severity == Severity::Error)
                || (filters.suggest && d.severity == Severity::Suggestion)
                || (filters.stale && checks::is_stale_code(d.code))
                || (filters.obligations && checks::is_obligation_code(d.code))
        });
    }
    diagnostics
}

/// Produce check output from pre-computed diagnostics with optional filter flags (D-19).
///
/// `terminal_files` is the set of file paths with terminal status — used to split
/// the error count into active vs terminal, and to filter with `--active-only`.
pub(crate) fn cmd_check(
    diagnostics: Vec<checks::Diagnostic>,
    filters: &CheckFilters,
    terminal_files: &HashSet<String>,
) -> CheckOutput {
    let mut diagnostics = apply_check_filters(diagnostics, filters, terminal_files);
    diagnostics.sort_by_key(|d| d.severity);

    let (mut errors, mut warnings, mut info, mut suggestions, mut terminal_errors) =
        (0, 0, 0, 0, 0);
    for d in &diagnostics {
        match d.severity {
            Severity::Error => {
                errors += 1;
                if d.file.as_ref().is_some_and(|f| terminal_files.contains(f)) {
                    terminal_errors += 1;
                }
            }
            Severity::Warning => warnings += 1,
            Severity::Info => info += 1,
            Severity::Suggestion => suggestions += 1,
        }
    }

    CheckOutput {
        diagnostics,
        errors,
        warnings,
        info,
        suggestions,
        terminal_errors,
    }
}

fn summarize_diagnostic_codes(diagnostics: &[Diagnostic]) -> Vec<CodeCount> {
    let mut by_code: HashMap<DiagnosticCode, usize> = HashMap::new();
    for diagnostic in diagnostics {
        *by_code.entry(diagnostic.code).or_insert(0) += 1;
    }

    let mut counts: Vec<CodeCount> = by_code
        .into_iter()
        .map(|(code, count)| CodeCount {
            code: code.to_string(),
            count,
        })
        .collect();
    counts.sort_by(|a, b| a.code.cmp(&b.code));
    counts
}

fn summarize_extractions(extractions: &[crate::extraction::FileExtraction]) -> ExtractionSummary {
    let mut by_hint: HashMap<&'static str, usize> = HashMap::new();
    let mut frontmatter_refs = 0usize;
    let mut body_refs = 0usize;

    for extraction in extractions {
        for discovered in &extraction.refs {
            *by_hint
                .entry(match &discovered.hint {
                    crate::extraction::RefHint::Label { .. } => "label",
                    crate::extraction::RefHint::FilePath => "file_path",
                    crate::extraction::RefHint::SectionRef => "section_ref",
                    crate::extraction::RefHint::External => "external",
                    crate::extraction::RefHint::Implausible { .. } => "implausible",
                })
                .or_insert(0) += 1;

            match discovered.source {
                crate::extraction::RefSource::Frontmatter { .. } => frontmatter_refs += 1,
                crate::extraction::RefSource::Body => body_refs += 1,
            }
        }
    }

    let mut by_hint: Vec<HintCount> = by_hint
        .into_iter()
        .map(|(hint, count)| HintCount { hint, count })
        .collect();
    by_hint.sort_by(|a, b| a.hint.cmp(b.hint));

    ExtractionSummary {
        files: extractions.len(),
        refs: frontmatter_refs + body_refs,
        frontmatter_refs,
        body_refs,
        by_hint,
    }
}

pub(crate) fn build_check_json_output(
    output: &CheckOutput,
    extractions: &[crate::extraction::FileExtraction],
    options: &CheckJsonOptions,
) -> CheckJsonOutput {
    let sample_limit = output.diagnostics.len().min(5);
    let sample_diagnostics = output
        .diagnostics
        .iter()
        .take(sample_limit)
        .map(checks::DiagnosticRecord::from_diagnostic)
        .collect();

    let diagnostics: Option<Vec<checks::DiagnosticRecord>> = if options.full {
        Some(
            output
                .diagnostics
                .iter()
                .map(checks::DiagnosticRecord::from_diagnostic)
                .collect(),
        )
    } else if options.include_diagnostics {
        Some(
            output
                .diagnostics
                .iter()
                .take(options.diagnostics_limit)
                .map(checks::DiagnosticRecord::from_diagnostic)
                .collect(),
        )
    } else {
        None
    };

    let diagnostics_returned = diagnostics.as_ref().map(Vec::len);
    let diagnostics_total = output.diagnostics.len();
    let diagnostics_truncated =
        diagnostics_returned.is_some_and(|returned| returned < diagnostics_total);

    let include_extractions_summary = options.include_extractions_summary || options.full;
    let extractions_summary =
        include_extractions_summary.then(|| summarize_extractions(extractions));
    let full_extractions =
        (options.include_full_extractions || options.full).then(|| extractions.to_vec());

    let expand = if options.full {
        Vec::new()
    } else {
        vec![
            "--diagnostics".to_string(),
            "--extractions-summary".to_string(),
            "--full".to_string(),
        ]
    };

    CheckJsonOutput {
        meta: OutputMeta::new(
            if options.full {
                DetailLevel::Full
            } else if options.include_diagnostics {
                DetailLevel::Sample
            } else {
                DetailLevel::Summary
            },
            diagnostics_truncated || (!options.full && diagnostics_total > sample_limit),
            diagnostics_returned,
            diagnostics_returned.map(|_| diagnostics_total),
            expand,
        ),
        summary: CheckSummary {
            errors: output.errors,
            warnings: output.warnings,
            info: output.info,
            suggestions: output.suggestions,
            terminal_errors: output.terminal_errors,
            total_diagnostics: diagnostics_total,
        },
        by_code: summarize_diagnostic_codes(&output.diagnostics),
        sample_diagnostics,
        diagnostics,
        extractions_summary,
        extractions: full_extractions,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::checks::DiagnosticCode;
    use crate::cli::test_helpers::*;

    use super::*;

    #[test]
    fn cmd_check_filters_terminal_files_when_active_only() {
        let diagnostics = vec![
            test_diag(DiagnosticCode::E001, "active.md"),
            test_diag(DiagnosticCode::E001, "done.md"),
        ];
        let terminal_files = HashSet::from([String::from("done.md")]);

        let output = cmd_check(
            diagnostics,
            &CheckFilters {
                active_only: true,
                ..CheckFilters::default()
            },
            &terminal_files,
        );

        assert_eq!(output.errors, 1);
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].file.as_deref(), Some("active.md"));
    }

    #[test]
    fn cmd_check_keeps_terminal_files_when_not_active_only() {
        let diagnostics = vec![
            test_diag(DiagnosticCode::E001, "active.md"),
            test_diag(DiagnosticCode::E001, "done.md"),
        ];
        let terminal_files = HashSet::from([String::from("done.md")]);

        let output = cmd_check(diagnostics, &CheckFilters::default(), &terminal_files);

        assert_eq!(output.errors, 2);
        assert_eq!(output.terminal_errors, 1);
    }

    #[test]
    fn check_json_default_is_summary_first() {
        let diagnostics = vec![
            test_diag(DiagnosticCode::E001, "active.md"),
            test_diag(DiagnosticCode::E002, "active.md"),
            test_diag(DiagnosticCode::E002, "active.md"),
        ];
        let terminal_files = HashSet::new();
        let output = cmd_check(diagnostics, &CheckFilters::default(), &terminal_files);
        let extraction = crate::extraction::FileExtraction {
            file: "active.md".into(),
            status: Some("draft".into()),
            metadata: crate::handle::HandleMetadata::default(),
            refs: vec![],
            all_keys: vec!["status".into()],
        };

        let json = build_check_json_output(
            &output,
            &[extraction],
            &CheckJsonOptions {
                include_diagnostics: false,
                diagnostics_limit: 50,
                include_extractions_summary: false,
                include_full_extractions: false,
                full: false,
            },
        );

        assert!(matches!(json.meta.detail, DetailLevel::Summary));
        assert!(json.diagnostics.is_none());
        assert!(json.extractions.is_none());
        assert_eq!(json.summary.total_diagnostics, 3);
        assert_eq!(json.by_code.len(), 2);
        assert!(
            json.sample_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.diagnostic_id.starts_with("diag_"))
        );
    }
}
