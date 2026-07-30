//! Render-boundary output contracts and human/NDJSON projections.

use std::io::Write;

use anneal_core::runtime::{Row, write_ndjson};
use anyhow::Result;

use crate::ContextOutput;

use super::command::OutputMode;

mod context;
mod guidance;
mod handle;
mod help;
mod init;
mod rows;
mod status;
mod value;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use context::{write_context_ndjson, write_context_text};
pub(super) use guidance::{
    eval_zero_result_hint, partition_check_diagnostics, search_zero_result_hint,
};
pub(super) use help::{render_dynamic_verb_help, render_dynamic_verb_help_with_collision};
pub(super) use init::run_init;
pub(super) use rows::{RankedAnchorEnrichment, RankedAnchorSignal};
use rows::{
    round_search_row_score, write_describe_text, write_ranked_anchor_ndjson, write_rows_text,
};
pub(super) use status::StatusOutput;
use status::write_status_text;
pub(super) use value::required_string;

/// Compact marker rendered when the selected relation has no rows.
pub(super) const EMPTY_ROWS_DIAGNOSTIC: &str = "(0 rows)";

/// A completed command result awaiting channel-aware rendering.
pub(super) enum CommandOutput {
    Rows {
        rows: Vec<Row>,
        view: RowView,
        gate_failed: bool,
        empty_binding_hint: Option<String>,
        zero_result_hint: Option<String>,
        warnings: Vec<String>,
        ranked_anchor: Option<RankedAnchorEnrichment>,
    },
    Status(StatusOutput),
    Context(ContextOutput),
    Text(String),
}

impl CommandOutput {
    /// Construct an ordinary row result.
    pub(super) const fn rows(rows: Vec<Row>, view: RowView) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint: None,
            zero_result_hint: None,
            warnings: Vec::new(),
            ranked_anchor: None,
        }
    }

    /// Construct rows with non-fatal query warnings.
    pub(super) fn rows_with_warnings(rows: Vec<Row>, view: RowView, warnings: Vec<String>) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint: None,
            zero_result_hint: None,
            warnings,
            ranked_anchor: None,
        }
    }

    #[cfg(test)]
    /// Construct rows with explicit empty-binding guidance.
    pub(super) fn rows_with_empty_binding_hint(
        rows: Vec<Row>,
        view: RowView,
        empty_binding_hint: Option<String>,
    ) -> Self {
        Self::rows_with_ranked_anchor_enrichment(rows, view, empty_binding_hint, Vec::new(), None)
    }

    /// Construct rows with warnings and optional anchor provenance.
    pub(super) fn rows_with_ranked_anchor_enrichment(
        rows: Vec<Row>,
        view: RowView,
        empty_binding_hint: Option<String>,
        warnings: Vec<String>,
        ranked_anchor: Option<RankedAnchorEnrichment>,
    ) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint,
            zero_result_hint: None,
            warnings,
            ranked_anchor,
        }
    }

    /// Attach renderer guidance for a zero-row result.
    pub(super) fn with_zero_result_hint(mut self, hint: Option<String>) -> Self {
        if let Self::Rows {
            zero_result_hint, ..
        } = &mut self
        {
            *zero_result_hint = hint;
        }
        self
    }

    /// Mark a rendered diagnostic result as a failed CI gate.
    pub(super) fn with_gate_failed(mut self, failed: bool) -> Self {
        if let Self::Rows { gate_failed, .. } = &mut self {
            *gate_failed = failed;
        }
        self
    }

    /// Return whether the result itself carries visible content.
    pub(super) fn has_displayable_content(&self) -> bool {
        match self {
            Self::Rows { rows, .. } => !rows.is_empty(),
            Self::Status(output) => !output.rows.is_empty(),
            Self::Context(output) => {
                !output.hits.is_empty()
                    || !output.spans.is_empty()
                    || !output.neighborhood.is_empty()
            }
            Self::Text(_) => false,
        }
    }

    /// Return whether completion must use a failing exit status.
    pub(super) const fn gate_failed(&self) -> bool {
        match self {
            Self::Rows { gate_failed, .. } => *gate_failed,
            Self::Status(_) | Self::Context(_) | Self::Text(_) => false,
        }
    }

    /// Name an empty machine result without changing stdout.
    pub(super) fn empty_rows_diagnostic(&self, mode: OutputMode) -> Option<&'static str> {
        match (mode, self) {
            (_, Self::Rows { rows, .. })
            | (
                OutputMode::Json | OutputMode::JsonExplicit,
                Self::Status(StatusOutput { rows, .. }),
            ) if !matches!(mode, OutputMode::Human) && rows.is_empty() => {
                Some(EMPTY_ROWS_DIAGNOSTIC)
            }
            (_, Self::Status(_) | Self::Rows { .. } | Self::Context(_) | Self::Text(_)) => None,
        }
    }

    /// Collect warnings and zero-result guidance for stderr.
    pub(super) fn stderr_diagnostic(&self, mode: OutputMode) -> Option<String> {
        let mut messages = Vec::new();
        if let Self::Rows { warnings, .. } = self {
            messages.extend(warnings.iter().cloned());
        }
        if !matches!(mode, OutputMode::Human)
            && let Self::Rows {
                rows,
                view,
                zero_result_hint,
                ..
            } = self
            && rows.is_empty()
        {
            if let Some(handle) = view.missing_handle() {
                messages.push(missing_handle_hint(handle));
            }
            if let Some(hint) = zero_result_hint {
                messages.push(hint.clone());
            }
        }
        if let Some(message) = self.empty_rows_diagnostic(mode) {
            messages.push(message.to_string());
        }
        match (mode, self) {
            (
                OutputMode::Json | OutputMode::JsonExplicit,
                Self::Rows {
                    rows,
                    empty_binding_hint: Some(example),
                    ..
                },
            ) if zero_binding_rows(rows) => {
                messages.push(empty_binding_hint_text(rows.len(), example));
            }
            _ => {}
        }
        (!messages.is_empty()).then(|| messages.join("\n"))
    }

    /// Render the result according to the resolved output mode.
    pub(super) fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match (mode, self) {
            (OutputMode::Human, Self::Status(output)) => {
                write_status_text(writer, &output.rows, output.flow_baseline_ready)?;
            }
            (OutputMode::Human, Self::Context(output)) => write_context_text(writer, &output)?,
            (
                OutputMode::Human,
                Self::Rows {
                    rows,
                    view,
                    empty_binding_hint,
                    zero_result_hint,
                    ..
                },
            ) => {
                write_rows_text(
                    writer,
                    &rows,
                    &view,
                    empty_binding_hint.as_deref(),
                    zero_result_hint.as_deref(),
                )?;
            }
            (
                OutputMode::Json,
                Self::Rows {
                    rows,
                    view: RowView::Describe,
                    ..
                },
            ) => write_describe_text(writer, &rows)?,
            (_, Self::Status(output)) => write_ndjson(writer, output.rows)?,
            (
                _,
                Self::Rows {
                    rows,
                    view: RowView::Search,
                    ..
                },
            ) => write_ndjson(writer, rows.into_iter().map(round_search_row_score))?,
            (
                _,
                Self::Rows {
                    rows,
                    view: RowView::RankedAnchor { handle_field },
                    ranked_anchor: Some(enrichment),
                    ..
                },
            ) => write_ranked_anchor_ndjson(writer, &rows, &handle_field, &enrichment)?,
            (_, Self::Rows { rows, .. }) => write_ndjson(writer, rows)?,
            (_, Self::Context(output)) => write_context_ndjson(writer, &output)?,
            (_, Self::Text(text)) => write_text(writer, &text)?,
        }
        Ok(())
    }
}

fn zero_binding_rows(rows: &[Row]) -> bool {
    !rows.is_empty() && rows.iter().all(|row| row.fields.is_empty())
}

/// Explain that rows matched even though no values were projected.
pub(super) fn empty_binding_hint_text(row_count: usize, example: &str) -> String {
    format!(
        "hint: matched {row_count} rows but no fields are bound for output.\n\
         Add a variable to extract values, e.g.:\n  {example}"
    )
}

fn missing_handle_hint(handle: &str) -> String {
    format!("hint: handle {handle:?} not found; try `anneal search {handle:?}` or `anneal status`")
}

/// Semantic row interpretation selected by the producing command.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum RowView {
    Search,
    Read {
        missing_handle: Option<String>,
    },
    RankedAnchor {
        handle_field: String,
    },
    Handle {
        handle: String,
        impact: bool,
        lineage: bool,
        missing: bool,
    },
    Broken,
    Describe,
    Schema,
    Eval,
    Verb {
        name: String,
    },
}

impl RowView {
    fn heading(&self, count: usize) -> Option<String> {
        let heading = match self {
            Self::Search => format!("Search ({count})"),
            Self::Read { .. } => format!("Read ({count})"),
            Self::Handle { handle, .. } => format!("Handle {handle} ({count} edges)"),
            Self::Broken => format!("Broken ({count})"),
            Self::Describe => return None,
            Self::Schema => format!("Schema ({count})"),
            Self::RankedAnchor { .. } | Self::Eval => format!("Results ({count})"),
            Self::Verb { name } => format!("{name} ({count})"),
        };
        Some(heading)
    }

    fn missing_handle(&self) -> Option<&str> {
        match self {
            Self::Read {
                missing_handle: Some(handle),
            }
            | Self::Handle {
                handle,
                missing: true,
                ..
            } => Some(handle),
            _ => None,
        }
    }
}

/// Write canonical text with one trailing newline.
pub(super) fn write_text<W: Write>(mut writer: W, text: &str) -> Result<()> {
    writer.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}
