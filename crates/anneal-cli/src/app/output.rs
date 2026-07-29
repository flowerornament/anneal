//! Render-boundary output contracts and human/NDJSON projections.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use anneal_core::ranking::{
    CONTEXT_NEIGHBOR_GROUP_CURRENT, CONTEXT_NEIGHBOR_GROUP_HIDDEN,
    CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT, CONTEXT_NEIGHBOR_GROUP_SUPERSEDED,
};
use anneal_core::runtime::eval::NumberValue;
use anneal_core::runtime::prelude::datalog_string_literal;
use anneal_core::runtime::{Atom, Body, Expr, NegatedAtom, Query, Row, Value, write_ndjson};
use anneal_core::{VerbCapability, VerbEntry};
use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use serde::Serialize;
use serde::ser::SerializeMap;

use crate::ContextOutput;

use super::command::OutputMode;

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

/// Markdown project initialization result.
pub(super) struct InitCommandOutput {
    pub(super) inner: anneal_md::InitOutput,
}

impl InitCommandOutput {
    /// Render initialization as readable text or a JSON object.
    pub(super) fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match mode {
            OutputMode::Human => write_init_text(writer, &self.inner),
            OutputMode::Json | OutputMode::JsonExplicit => write_json_object(writer, &self.inner),
        }
    }
}

/// Status rows plus snapshot context needed by the human dashboard.
pub(super) struct StatusOutput {
    pub(super) rows: Vec<Row>,
    pub(super) flow_baseline_ready: bool,
}

/// Per-handle signal sets attached only to ranked-anchor JSON rows.
#[derive(Clone, Debug)]
pub(super) struct RankedAnchorEnrichment {
    pub(super) handle_field: String,
    pub(super) signals_by_handle: BTreeMap<String, Vec<RankedAnchorSignal>>,
}

/// One scored provenance signal contributing to an anchor rank.
#[derive(Clone, Debug, Serialize)]
pub(super) struct RankedAnchorSignal {
    pub(super) why: String,
    pub(super) score: NumberValue,
    #[serde(skip)]
    pub(super) priority: i64,
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

/// Partition one snapshot into rendered errors and adjacent non-errors.
pub(super) fn partition_check_diagnostics(rows: Vec<Row>) -> Result<(Vec<Row>, usize)> {
    let mut error_rows = Vec::new();
    let mut non_error_count = 0;
    for mut row in rows {
        if required_string(&row, "severity")? == "error" {
            row.fields.remove("severity");
            error_rows.push(row);
        } else {
            non_error_count += 1;
        }
    }
    Ok((error_rows, non_error_count))
}

/// Teach the adjacent search move selected by confidence policy.
pub(super) fn search_zero_result_hint(include_low_confidence: bool) -> String {
    if include_low_confidence {
        "hint: search returned 0 rows including low-confidence matches; retry with broader terms."
            .to_string()
    } else {
        "hint: search returned 0 rows after excluding low-confidence matches; retry with --include-low-confidence or broader terms."
            .to_string()
    }
}

/// Explain a zero-row query without running a second evaluation.
pub(super) fn eval_zero_result_hint(query: &Query) -> String {
    if let Some(predicate) = bare_relation_name(query) {
        return format!(
            "hint: {predicate} currently has no rows; run `anneal describe {predicate}` for requirements and common joins."
        );
    }
    let recovery = first_relation_name(&query.body).map_or_else(
        || "Relax one constraint at a time.".to_string(),
        |predicate| format!("Relax one constraint at a time or run `anneal describe {predicate}`."),
    );
    format!(
        "hint: this filtered or joined query returned 0 rows; that does not establish its predicates are empty. {recovery}"
    )
}

fn bare_relation_name(query: &Query) -> Option<String> {
    if !query.local_rules.is_empty() || query.body.atoms.len() != 1 {
        return None;
    }
    match query.body.atoms.first()? {
        Atom::Stored(stored)
            if stored.fields.iter().all(|field| {
                field
                    .term
                    .expr()
                    .is_none_or(|expr| matches!(expr, Expr::Var(_)))
            }) =>
        {
            Some(stored.relation.to_string())
        }
        Atom::Derived(derived)
            if derived
                .args
                .iter()
                .all(|arg| arg.expr().is_none_or(|expr| matches!(expr, Expr::Var(_)))) =>
        {
            Some(derived.predicate.display_name())
        }
        Atom::Stored(_)
        | Atom::Derived(_)
        | Atom::Comparison(_)
        | Atom::Aggregation(_)
        | Atom::Negation(_)
        | Atom::TimeBlock(_) => None,
    }
}

fn first_relation_name(body: &Body) -> Option<String> {
    body.atoms.iter().find_map(|atom| match atom {
        Atom::Stored(stored) => Some(stored.relation.to_string()),
        Atom::Derived(derived) => Some(derived.predicate.display_name()),
        Atom::Aggregation(aggregate) => first_relation_name(&aggregate.body),
        Atom::TimeBlock(time_block) => first_relation_name(&time_block.body),
        Atom::Negation(negation) => match &negation.atom {
            NegatedAtom::Stored(stored) => Some(stored.relation.to_string()),
            NegatedAtom::Derived(derived) => Some(derived.predicate.display_name()),
        },
        Atom::Comparison(_) => None,
    })
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

/// Render help from a resolved project-verb registry entry.
pub(super) fn render_dynamic_verb_help(entry: &VerbEntry) -> String {
    let name = entry.name();
    let usage_args = entry
        .args()
        .iter()
        .filter(|arg| arg.default().is_none())
        .fold(String::new(), |mut out, arg| {
            let _ = write!(out, " <{}>", arg.name().to_ascii_uppercase());
            out
        });
    let schema = entry.output_schema().to_string();
    let args = if entry.args().is_empty() {
        "  none".to_string()
    } else {
        entry
            .args()
            .iter()
            .map(|arg| match arg.default() {
                Some(default) => {
                    format!("  {}: {} = {default}", arg.name(), arg.kind())
                }
                None => format!("  {}: {}", arg.name(), arg.kind()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let capabilities = if entry.capabilities().is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            entry
                .capabilities()
                .iter()
                .map(VerbCapability::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        "\
Usage: anneal [OPTIONS] {name} [OPTIONS]{usage_args}

{doc}

This is a saved @verb projected from the resolved VerbRegistry. Use it like a
standard verb, or inspect/modify the underlying query with `anneal describe {name}`,
`anneal schema`, and `anneal -e`.

Options:
      --rows <N>                 Cap returned rows after evaluation
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Arguments:
{args}

Output schema:
  {schema}

Capabilities: {capabilities}
Source: {source}:{line}

Query:
  {query}

Global options:
      --root <PATH>              Corpus root (default: nearest .design, docs, or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json|ndjson> Force readable text or JSON/NDJSON output
",
        doc = entry.doc(),
        source = entry.source().location().source_name,
        line = entry.source().location().line,
        query = entry.query_source(),
    )
}

/// Render project-verb help and disclose a same-named runtime topic.
pub(super) fn render_dynamic_verb_help_with_collision(
    entry: &VerbEntry,
    collision: bool,
) -> String {
    let mut help = render_dynamic_verb_help(entry);
    if collision {
        let _ = writeln!(
            help,
            "Also: `anneal describe {}` teaches the additional runtime vocabulary sharing this name.",
            entry.name()
        );
    }
    help
}

/// Write canonical text with one trailing newline.
pub(super) fn write_text<W: Write>(mut writer: W, text: &str) -> Result<()> {
    writer.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn write_json_object<W: Write, T: Serialize>(mut writer: W, value: &T) -> Result<()> {
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Run markdown initialization and retain its typed render result.
pub(super) fn run_init(root: &Utf8Path, dry_run: bool, force: bool) -> Result<InitCommandOutput> {
    let mode = anneal_md::InitMode::from_flags(dry_run, force);
    let inner =
        anneal_md::render_or_write_init(root, mode).context("failed to initialize anneal.dl")?;
    Ok(InitCommandOutput { inner })
}

fn write_init_text<W: Write>(mut writer: W, output: &anneal_md::InitOutput) -> Result<()> {
    if output.written {
        writeln!(writer, "Wrote {}", output.path)?;
        if let Some(path) = &output.backup_path {
            writeln!(writer, "Moved existing anneal.toml to {path}")?;
        }
    } else {
        writeln!(writer, "anneal.dl")?;
        writeln!(writer, "dry run — not written")?;
    }
    writeln!(writer)?;
    write_text(writer, &output.body)
}

fn write_status_text<W: Write>(
    mut writer: W,
    rows: &[Row],
    flow_baseline_ready: bool,
) -> Result<()> {
    writeln!(writer, "Status")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        writeln!(
            writer,
            "Note: no corpus facts found; root may be empty or unresolved."
        )?;
        return Ok(());
    }

    let mut metrics = BTreeMap::<(&str, &str), StatusMetric<'_>>::new();
    let mut pipeline = Vec::new();
    for row in rows {
        let metric = StatusMetric::from_row(row)?;
        if metric.category == "pipeline" {
            pipeline.push(metric);
        }
        metrics.insert((metric.category, metric.name), metric);
    }

    let total_handles = metric_count(&metrics, "scale", "handles");
    let file_handles = metric_count(&metrics, "scale", "file_handles");
    let files_with_status = metric_count(&metrics, "scale", "file_handles_with_status");
    let statusless_files = metric_count(&metrics, "scale", "statusless_file_handles");
    let coverage = percentage(files_with_status, file_handles);

    writeln!(
        writer,
        "Scale        {total_handles} handles · {file_handles} files · {coverage}% lifecycle coverage ({statusless_files} statusless files)"
    )?;
    if total_handles == 0 {
        writeln!(
            writer,
            "Note: no corpus facts found; root may be empty or unresolved."
        )?;
    }
    writeln!(
        writer,
        "Coverage     {coverage}% of file handles carry lifecycle status; orientation is graph+recency-led"
    )?;

    if !pipeline.is_empty() {
        pipeline.sort_by(|left, right| left.name.cmp(right.name));
        let parts = pipeline
            .iter()
            .map(|metric| format!("{} {}", metric.name, display_number(metric.count)))
            .collect::<Vec<_>>()
            .join(" · ");
        writeln!(writer, "Pipeline     {parts}")?;
    }

    if flow_baseline_ready {
        writeln!(
            writer,
            "Convergence  broken={}  blocked={}  open={}  advancing={}  holding={}  drifting={}",
            metric_count(&metrics, "convergence", "broken"),
            metric_count(&metrics, "convergence", "blocked"),
            metric_count(&metrics, "convergence", "open"),
            metric_count(&metrics, "convergence", "advancing"),
            metric_count(&metrics, "convergence", "holding"),
            metric_count(&metrics, "convergence", "drifting")
        )?;
    } else {
        writeln!(
            writer,
            "Convergence  broken={}  blocked={}  open={}  advancing=-  holding=-  drifting=-",
            metric_count(&metrics, "convergence", "broken"),
            metric_count(&metrics, "convergence", "blocked"),
            metric_count(&metrics, "convergence", "open")
        )?;
        writeln!(
            writer,
            "Note: flow signals empty until snapshot baseline accumulates."
        )?;
        writeln!(writer, "      Run `anneal status` again to populate.")?;
    }

    writeln!(
        writer,
        "Health       errors={}  blockers={}  spec_code_drift={}",
        metric_count(&metrics, "health", "errors"),
        metric_count(&metrics, "health", "blockers"),
        metric_count(&metrics, "health", "spec_code_drift")
    )?;
    writeln!(
        writer,
        "Diagnostics  {} total · {} error · {} warning · {} suggestion · {} info",
        metric_count(&metrics, "diagnostics", "total"),
        metric_count(&metrics, "diagnostics", "error"),
        metric_count(&metrics, "diagnostics", "warning"),
        metric_count(&metrics, "diagnostics", "suggestion"),
        metric_count(&metrics, "diagnostics", "info")
    )?;
    let drift_cold = metric_count(&metrics, "drift", "cold");
    if has_metric_category(&metrics, "drift") {
        let warm = metric_count(&metrics, "drift", "intact")
            + metric_count(&metrics, "drift", "drifted")
            + metric_count(&metrics, "drift", "moved")
            + metric_count(&metrics, "drift", "moved_ambiguous")
            + metric_count(&metrics, "drift", "gone")
            + metric_count(&metrics, "drift", "unknown")
            + metric_count(&metrics, "drift", "dirty");
        if warm > 0 {
            write!(
                writer,
                "Code refs    {} intact · {} drifted · {} moved · {} moved? · {} gone · {} unknown · {} dirty",
                metric_count(&metrics, "drift", "intact"),
                metric_count(&metrics, "drift", "drifted"),
                metric_count(&metrics, "drift", "moved"),
                metric_count(&metrics, "drift", "moved_ambiguous"),
                metric_count(&metrics, "drift", "gone"),
                metric_count(&metrics, "drift", "unknown"),
                metric_count(&metrics, "drift", "dirty")
            )?;
            if drift_cold > 0 {
                write!(
                    writer,
                    " · {drift_cold} cold (run `anneal check --refresh-drift`)"
                )?;
            }
            writeln!(writer)?;
        } else if drift_cold > 0 {
            writeln!(
                writer,
                "Code refs    drift evidence not built for {drift_cold} refs; run `anneal check --refresh-drift`"
            )?;
        }
    }
    writeln!(writer)?;
    writeln!(writer, "Read first")?;
    writeln!(
        writer,
        "  anneal -e '? recent_frontier(h, rank, recency), *handle{{id: h, file: file}} order by rank asc.' --limit 12"
    )?;
    writeln!(
        writer,
        "  anneal -e '? ranked_anchor(h, rank, score, why), *handle{{id: h, file: file}} order by rank asc.' --limit 12"
    )?;
    writeln!(
        writer,
        "  follow-up: anneal -e '? anchor_signal(h, s, prio, why).'"
    )?;
    writeln!(writer, "Work")?;
    writeln!(
        writer,
        "  anneal -e '? diagnostic{{code: code, severity: severity, subject: h, file: file, line: line}}.' --limit 12"
    )?;
    writeln!(
        writer,
        "  anneal -e '? blocker(h, energy, source), *handle{{id: h, file: file, status: status}}.' --limit 12"
    )?;
    Ok(())
}

fn metric_count(
    metrics: &BTreeMap<(&str, &str), StatusMetric<'_>>,
    category: &str,
    name: &str,
) -> i64 {
    metrics
        .get(&(category, name))
        .and_then(|metric| number_to_i64(metric.count))
        .unwrap_or(0)
}

fn has_metric_category(metrics: &BTreeMap<(&str, &str), StatusMetric<'_>>, category: &str) -> bool {
    metrics
        .keys()
        .any(|(metric_category, _)| *metric_category == category)
}

fn percentage(numerator: i64, denominator: i64) -> i64 {
    if denominator <= 0 {
        0
    } else {
        numerator.saturating_mul(100) / denominator
    }
}

fn number_to_i64(number: &NumberValue) -> Option<i64> {
    match number {
        NumberValue::Int(value) => Some(*value),
        NumberValue::Float(_) => None,
    }
}

fn write_context_text<W: Write>(mut writer: W, output: &ContextOutput) -> Result<()> {
    const MAX_TEXT_LINES_PER_SPAN: usize = 8;
    const MAX_NEIGHBORS_PER_HANDLE: usize = 8;

    writeln!(writer, "Context")?;
    writeln!(writer, "Goal: {}", output.goal)?;

    if output.hits.is_empty() {
        writeln!(writer, "(0 hits)")?;
        return Ok(());
    }

    writeln!(writer)?;
    writeln!(writer, "Hits")?;
    for (index, hit) in output.hits.iter().enumerate() {
        let span = hit
            .span_id
            .as_deref()
            .map_or(String::new(), |span| format!(" span={span}"));
        let summary = hit
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
            .map_or(String::new(), |summary| {
                format!(" summary={}", display_string_value(summary))
            });
        let status = hit
            .status
            .as_deref()
            .map_or(String::new(), |status| format!(" status={status}"));
        let age = hit
            .age_days
            .map_or(String::new(), |days| format!(" age_days={days}"));
        let topic = if hit.newer_topic_sibling_count > 0 {
            hit.top_newer_topic_sibling
                .as_deref()
                .map_or_else(String::new, |top| {
                    let handle = datalog_string_literal(&hit.handle);
                    format!(
                        " topic=\"{} unmarked newer topical siblings (top: {}; follow-up: anneal -e '? currency_suspect({}, newer).')\"",
                        hit.newer_topic_sibling_count, top, handle
                    )
                })
        } else {
            String::new()
        };
        writeln!(
            writer,
            "{:>2}. {}  score={:.3}  field={}  reason={} disposition={}{}{}{}{}{}",
            index + 1,
            hit.handle,
            hit.score,
            hit.field,
            hit.reason,
            hit.disposition,
            status,
            age,
            topic,
            span,
            summary
        )?;
    }

    if !output.spans.is_empty() {
        writeln!(writer)?;
        let has_bodies = output.spans.iter().any(|span| span.text.is_some());
        writeln!(writer, "{}", if has_bodies { "Read" } else { "Spans" })?;
        for span in &output.spans {
            writeln!(
                writer,
                "{} span={} lines={}-{} tokens={}",
                span.handle, span.span_id, span.start_line, span.end_line, span.tokens
            )?;
            if let Some(text) = &span.text {
                write_text_block(&mut writer, text, MAX_TEXT_LINES_PER_SPAN)?;
            }
        }
    }

    if !output.neighborhood.is_empty() {
        let mut by_handle: BTreeMap<&str, Vec<&crate::ContextNeighbor>> = BTreeMap::new();
        for neighbor in &output.neighborhood {
            by_handle
                .entry(&neighbor.handle)
                .or_default()
                .push(neighbor);
        }

        writeln!(writer)?;
        writeln!(writer, "Neighborhood")?;
        for (handle, neighbors) in by_handle {
            writeln!(writer, "{handle}:")?;
            let groups = [
                (CONTEXT_NEIGHBOR_GROUP_CURRENT, "current"),
                (CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT, "in-flight"),
                (CONTEXT_NEIGHBOR_GROUP_SUPERSEDED, "superseded"),
                (CONTEXT_NEIGHBOR_GROUP_HIDDEN, "hidden"),
            ];
            for (group, label) in groups {
                let group_neighbors = neighbors
                    .iter()
                    .copied()
                    .filter(|neighbor| neighbor.group == group)
                    .collect::<Vec<_>>();
                if group_neighbors.is_empty() {
                    continue;
                }
                let limit = if group == CONTEXT_NEIGHBOR_GROUP_HIDDEN {
                    1
                } else {
                    MAX_NEIGHBORS_PER_HANDLE
                };
                let omitted = group_neighbors.len().saturating_sub(limit);
                write!(writer, "  {label}: ")?;
                for (index, neighbor) in group_neighbors.iter().take(limit).enumerate() {
                    if index > 0 {
                        write!(writer, ", ")?;
                    }
                    write!(writer, "{}", neighbor.neighbor)?;
                    write!(writer, " disposition={}", neighbor.disposition)?;
                    if let Some(status) = &neighbor.status {
                        write!(writer, " status={status}")?;
                    }
                    if let Some(age_days) = neighbor.age_days {
                        write!(writer, " age_days={age_days}")?;
                    }
                    write!(writer, " degree={}", neighbor.degree)?;
                }
                if omitted == 0 {
                    writeln!(writer)?;
                } else if group == CONTEXT_NEIGHBOR_GROUP_HIDDEN {
                    writeln!(writer, ", ... {omitted} hidden inventory handles")?;
                } else {
                    writeln!(writer, ", ... {omitted} more")?;
                }
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "section")]
enum ContextEvent<'a> {
    #[serde(rename = "goal")]
    Goal { goal: &'a str },
    #[serde(rename = "hit")]
    Hit {
        handle: &'a str,
        span_id: Option<&'a str>,
        #[serde(serialize_with = "crate::serialize_json_score")]
        score: f64,
        reason: &'a str,
        field: &'a str,
        summary: Option<&'a str>,
        status: Option<&'a str>,
        disposition: &'a str,
        age_days: Option<i64>,
        topic_signal: &'a str,
        newer_topic_sibling_count: i64,
        top_newer_topic_sibling: Option<&'a str>,
    },
    #[serde(rename = "span")]
    Span {
        handle: &'a str,
        span_id: &'a str,
        start_line: i64,
        end_line: i64,
        tokens: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<&'a str>,
    },
    #[serde(rename = "neighbor")]
    Neighbor {
        handle: &'a str,
        neighbor: &'a str,
        status: Option<&'a str>,
        disposition: &'a str,
        age_days: Option<i64>,
        degree: i64,
        group: &'a str,
    },
}

fn write_context_ndjson<W: Write>(writer: W, output: &ContextOutput) -> Result<()> {
    let events = std::iter::once(ContextEvent::Goal {
        goal: output.goal.as_str(),
    })
    .chain(output.hits.iter().map(|hit| ContextEvent::Hit {
        handle: hit.handle.as_str(),
        span_id: hit.span_id.as_deref(),
        score: hit.score,
        reason: hit.reason.as_str(),
        field: hit.field.as_str(),
        summary: hit.summary.as_deref(),
        status: hit.status.as_deref(),
        disposition: hit.disposition.as_str(),
        age_days: hit.age_days,
        topic_signal: hit.topic_signal.as_str(),
        newer_topic_sibling_count: hit.newer_topic_sibling_count,
        top_newer_topic_sibling: hit.top_newer_topic_sibling.as_deref(),
    }))
    .chain(output.spans.iter().map(|span| ContextEvent::Span {
        handle: span.handle.as_str(),
        span_id: span.span_id.as_str(),
        start_line: span.start_line,
        end_line: span.end_line,
        tokens: span.tokens,
        text: span.text.as_deref(),
    }))
    .chain(
        output
            .neighborhood
            .iter()
            .map(|neighbor| ContextEvent::Neighbor {
                handle: neighbor.handle.as_str(),
                neighbor: neighbor.neighbor.as_str(),
                status: neighbor.status.as_deref(),
                disposition: neighbor.disposition.as_str(),
                age_days: neighbor.age_days,
                degree: neighbor.degree,
                group: neighbor.group.as_str(),
            }),
    );
    write_ndjson(writer, events)?;
    Ok(())
}

fn round_search_row_score(mut row: Row) -> Row {
    if let Some(Value::Number(NumberValue::Float(score))) = row.fields.get_mut("score") {
        *score = crate::round_json_score(*score);
    }
    row
}

fn write_ranked_anchor_ndjson<W: Write>(
    writer: W,
    rows: &[Row],
    handle_field: &str,
    enrichment: &RankedAnchorEnrichment,
) -> Result<()> {
    let rows = rows.iter().map(|row| RankedAnchorJsonRow {
        row,
        handle_field,
        signals_by_handle: &enrichment.signals_by_handle,
    });
    write_ndjson(writer, rows)?;
    Ok(())
}

struct RankedAnchorJsonRow<'a> {
    row: &'a Row,
    handle_field: &'a str,
    signals_by_handle: &'a BTreeMap<String, Vec<RankedAnchorSignal>>,
}

impl Serialize for RankedAnchorJsonRow<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let derivation_replaces_field =
            self.row.derivation.is_some() && self.row.fields.contains_key("_derivation");
        let signals_replaces_field = self.row.fields.contains_key("signals");
        let len = self.row.fields.len() + 1 + usize::from(self.row.derivation.is_some())
            - usize::from(derivation_replaces_field)
            - usize::from(signals_replaces_field);
        let mut map = serializer.serialize_map(Some(len))?;
        for (key, value) in &self.row.fields {
            if (derivation_replaces_field && key == "_derivation") || key == "signals" {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        if let Some(derivation) = &self.row.derivation {
            map.serialize_entry("_derivation", derivation)?;
        }
        let signals = match self.row.fields.get(self.handle_field) {
            Some(Value::String(handle)) => self
                .signals_by_handle
                .get(handle)
                .map_or(&[] as &[RankedAnchorSignal], Vec::as_slice),
            _ => &[],
        };
        map.serialize_entry("signals", signals)?;
        map.end()
    }
}

fn write_rows_text<W: Write>(
    mut writer: W,
    rows: &[Row],
    view: &RowView,
    empty_binding_hint: Option<&str>,
    zero_result_hint: Option<&str>,
) -> Result<()> {
    if let RowView::Handle {
        handle,
        impact,
        lineage,
        missing,
    } = view
    {
        return write_handle_text(writer, handle, *impact, *lineage, *missing, rows);
    }

    if *view == RowView::Describe {
        return write_describe_text(writer, rows);
    }

    if let RowView::Read { missing_handle } = view {
        return write_read_text(writer, rows, missing_handle.as_deref());
    }

    if let Some(heading) = view.heading(rows.len()) {
        writeln!(writer, "{heading}")?;
    }
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        if let Some(hint) = zero_result_hint {
            writeln!(writer, "{hint}")?;
        }
        return Ok(());
    }

    for (index, row) in rows.iter().enumerate() {
        write!(writer, "{:>2}.", index + 1)?;
        for (field, value) in &row.fields {
            write!(writer, " {field}={}", display_value(value))?;
        }
        writeln!(writer)?;
    }
    if zero_binding_rows(rows)
        && let Some(example) = empty_binding_hint
    {
        writeln!(writer)?;
        writeln!(writer, "{}", empty_binding_hint_text(rows.len(), example))?;
    }
    if matches!(view, RowView::RankedAnchor { .. }) {
        writeln!(writer)?;
        writeln!(
            writer,
            "Follow-up: anneal -e '? anchor_signal(h, s, prio, why).'"
        )?;
    }
    Ok(())
}

fn write_read_text<W: Write>(
    mut writer: W,
    rows: &[Row],
    missing_handle: Option<&str>,
) -> Result<()> {
    const MAX_TEXT_LINES_PER_SPAN: usize = 80;

    writeln!(writer, "Read ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        if let Some(handle) = missing_handle {
            writeln!(writer, "{}", missing_handle_hint(handle))?;
        }
        return Ok(());
    }

    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            writeln!(writer)?;
        }

        let span_id = required_string(row, "span_id")?;
        let start_line = required_number(row, "start_line")?;
        let end_line = required_number(row, "end_line")?;
        let tokens = required_number(row, "tokens")?;
        let total_tokens = optional_number(row, "total_tokens")?;
        let text = required_string(row, "text")?;

        writeln!(
            writer,
            "{:>2}. {}  lines={}-{}  tokens={}",
            index + 1,
            span_id,
            display_number(start_line),
            display_number(end_line),
            display_number(tokens)
        )?;

        write_text_block(&mut writer, text, MAX_TEXT_LINES_PER_SPAN)?;
        if let Some(total_tokens) = total_tokens
            && number_gt(total_tokens, tokens)
        {
            writeln!(
                writer,
                "    read: showing first {} tokens of span ({} total); use --budget {} to read the full span",
                display_number(tokens),
                display_number(total_tokens),
                display_number(total_tokens)
            )?;
        }
    }
    Ok(())
}

fn write_text_block<W: Write>(writer: &mut W, text: &str, max_lines: usize) -> Result<()> {
    let mut lines = text.lines().skip_while(|line| line.trim().is_empty());
    for line in lines.by_ref().take(max_lines) {
        writeln!(writer, "  {}", line.trim_end())?;
    }
    if lines.next().is_some() {
        writeln!(writer, "  ...")?;
    }
    Ok(())
}

fn write_describe_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    let mut wrote_any = false;
    let mut doc_rows = Vec::new();
    let mut other_rows = Vec::new();
    for row in rows {
        if let Some(doc) = optional_string(row, "doc")? {
            doc_rows.push(doc.to_string());
        } else {
            other_rows.push(row);
        }
    }

    for doc in doc_rows {
        if wrote_any {
            writeln!(writer)?;
        }
        writeln!(writer, "{doc}")?;
        wrote_any = true;
    }

    for (index, row) in other_rows.iter().enumerate() {
        if wrote_any {
            writeln!(writer)?;
        }
        write!(writer, "{:>2}.", index + 1)?;
        for (field, value) in &row.fields {
            write!(writer, " {field}={}", display_value(value))?;
        }
        writeln!(writer)?;
        wrote_any = true;
    }
    Ok(())
}

/// Render the complete human handle view.
pub(super) fn write_handle_text<W: Write>(
    mut writer: W,
    handle: &str,
    include_impact: bool,
    include_lineage: bool,
    missing: bool,
    rows: &[Row],
) -> Result<()> {
    let edge_count = rows
        .iter()
        .filter(|row| {
            matches!(
                required_string(row, "relation"),
                Ok("in" | "out" | "code_ref")
            )
        })
        .count();

    writeln!(writer, "Handle {handle} ({edge_count} edges)")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        if missing {
            writeln!(writer, "{}", missing_handle_hint(handle))?;
        }
        return Ok(());
    }

    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut code_refs = Vec::new();
    let mut direct_impact = Vec::new();
    let mut indirect_impact = Vec::new();
    let mut lineage = Vec::new();
    let mut wrote_self = false;
    for row in rows {
        let relation = required_string(row, "relation")?;
        match relation {
            "self" => {
                wrote_self = true;
                let kind = required_string(row, "kind")?;
                let status = optional_string(row, "status")?.unwrap_or("unknown");
                let file = required_string(row, "file")?;
                let line = required_number(row, "line")?;
                writeln!(
                    writer,
                    "kind={kind}  status={status}  at={file}:{}",
                    display_number(line)
                )?;
                if let Some(summary) = optional_string(row, "summary")?
                    && !summary.trim().is_empty()
                {
                    writeln!(writer, "summary={}", display_string_value(summary))?;
                }
            }
            "in" => incoming.push(row),
            "out" => outgoing.push(row),
            "code_ref" => code_refs.push(row),
            "impact" => {
                let depth = required_number(row, "depth")?;
                if matches!(depth, NumberValue::Int(1)) {
                    direct_impact.push(row);
                } else {
                    indirect_impact.push(row);
                }
            }
            "lineage" => lineage.push(row),
            _ => {}
        }
    }

    if !wrote_self {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
    }
    write_handle_edges(&mut writer, "Outgoing", "->", &outgoing)?;
    write_handle_code_refs(&mut writer, handle, &code_refs)?;
    write_handle_edges(&mut writer, "Incoming", "<-", &incoming)?;
    if include_impact {
        write_handle_impact(&mut writer, &direct_impact, &indirect_impact)?;
    }
    if include_lineage {
        write_handle_lineage(&mut writer, handle, &lineage)?;
    }
    Ok(())
}

fn write_handle_edges<W: Write>(
    writer: &mut W,
    heading: &str,
    arrow: &str,
    rows: &[&Row],
) -> Result<()> {
    const MAX_HANDLE_EDGES_PER_SECTION: usize = 24;

    if rows.is_empty() {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "{heading}")?;
    let mut by_kind = BTreeMap::<&str, Vec<&Row>>::new();
    for row in rows {
        by_kind
            .entry(required_string(row, "kind")?)
            .or_default()
            .push(row);
    }
    for (kind, group) in by_kind {
        writeln!(writer, "{kind} ({})", group.len())?;
        for (index, row) in group.iter().take(MAX_HANDLE_EDGES_PER_SECTION).enumerate() {
            let other = required_string(row, "other")?;
            let file = required_string(row, "file")?;
            let line = required_number(row, "line")?;
            writeln!(
                writer,
                "{:>2}. {arrow} {other}  at={file}:{}",
                index + 1,
                display_number(line)
            )?;
        }
        let omitted = group.len().saturating_sub(MAX_HANDLE_EDGES_PER_SECTION);
        if omitted > 0 {
            writeln!(writer, "    ... {omitted} more")?;
        }
    }
    Ok(())
}

fn write_handle_code_refs<W: Write>(writer: &mut W, handle: &str, rows: &[&Row]) -> Result<()> {
    const MAX_CODE_REFERENCES: usize = 24;

    if rows.is_empty() {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "Code references ({})", rows.len())?;
    for (index, row) in rows.iter().take(MAX_CODE_REFERENCES).enumerate() {
        let target = optional_string(row, "summary")?
            .filter(|summary| !summary.is_empty())
            .unwrap_or(required_string(row, "other")?);
        let annotation = code_ref_annotation(row)?;
        let file = required_string(row, "file")?;
        let line = required_number(row, "line")?;
        writeln!(
            writer,
            "{:>2}. {target}{annotation}  at={file}:{}",
            index + 1,
            display_number(line)
        )?;
    }
    if rows
        .iter()
        .any(|row| matches!(optional_string(row, "disposition"), Ok(None)))
    {
        writeln!(
            writer,
            "    drift evidence not built; run `anneal check --refresh-drift`"
        )?;
    }
    if !rows.is_empty() {
        let handle_literal = datalog_string_literal(handle);
        writeln!(
            writer,
            "    follow-up: anneal -e '? assertion_drift({handle_literal}, target, commits).'"
        )?;
    }
    let omitted = rows.len().saturating_sub(MAX_CODE_REFERENCES);
    if omitted > 0 {
        writeln!(writer, "    ... {omitted} more")?;
    }
    Ok(())
}

fn code_ref_annotation(row: &Row) -> Result<String> {
    let Some(disposition) = optional_string(row, "disposition")? else {
        return Ok(String::new());
    };
    let mut parts = vec![disposition.to_string()];
    if let Some(commits) = optional_string(row, "candidate_count")?
        && disposition == "referent-moved-ambiguous"
    {
        parts.push(format!("{commits} candidates"));
    }
    if let Some(target) = optional_string(row, "moved_to")?
        && disposition == "referent-moved"
    {
        parts.push(format!("moved to {target}"));
    }
    Ok(format!("  [{}]", parts.join(" · ")))
}

fn write_handle_impact<W: Write>(writer: &mut W, direct: &[&Row], indirect: &[&Row]) -> Result<()> {
    writeln!(writer)?;
    writeln!(writer, "Impact (configured reverse traversal)")?;
    write_handle_impact_group(writer, "Direct", direct)?;
    write_handle_impact_group(writer, "Indirect", indirect)?;
    Ok(())
}

fn write_handle_impact_group<W: Write>(writer: &mut W, heading: &str, rows: &[&Row]) -> Result<()> {
    writeln!(writer, "{heading} ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "    (none)")?;
        return Ok(());
    }
    for (index, row) in rows.iter().enumerate() {
        let other = required_string(row, "other")?;
        writeln!(writer, "{:>2}. {other}", index + 1)?;
    }
    Ok(())
}

fn write_handle_lineage<W: Write>(writer: &mut W, handle: &str, rows: &[&Row]) -> Result<()> {
    writeln!(writer)?;
    writeln!(writer, "Lineage (file supersession)")?;
    if rows.is_empty() {
        writeln!(
            writer,
            "    (none; no file-handle Supersedes lineage found)"
        )?;
        return Ok(());
    }

    let normalized_root = rows
        .first()
        .map(|row| required_string(row, "normalized_root"))
        .transpose()?
        .unwrap_or(handle);
    if normalized_root != handle {
        writeln!(writer, "normalized_root={normalized_root}")?;
    }

    let root = rows
        .iter()
        .find(|row| required_string(row, "role").is_ok_and(|role| role == "root"));
    if let Some(root) = root {
        writeln!(writer, "root: {}", lineage_row_summary(root)?)?;
    }

    let mut heads = lineage_rows_with_bool(rows, "head", true)?;
    let mut successors = lineage_rows_with_role(rows, "successor")?;
    let mut predecessors = lineage_rows_with_role(rows, "predecessor")?;
    sort_lineage_rows(&mut heads, false);
    sort_lineage_rows(&mut successors, false);
    sort_lineage_rows(&mut predecessors, true);

    write_handle_lineage_group(writer, "Current head(s)", &heads)?;
    write_handle_lineage_group(writer, "Newer", &successors)?;
    write_handle_lineage_group(writer, "Older", &predecessors)?;
    Ok(())
}

fn lineage_rows_with_bool<'a>(rows: &[&'a Row], field: &str, value: bool) -> Result<Vec<&'a Row>> {
    rows.iter()
        .copied()
        .filter(|row| required_bool(row, field).is_ok_and(|actual| actual == value))
        .map(Ok)
        .collect()
}

fn lineage_rows_with_role<'a>(rows: &[&'a Row], role: &str) -> Result<Vec<&'a Row>> {
    rows.iter()
        .copied()
        .filter(|row| required_string(row, "role").is_ok_and(|actual| actual == role))
        .map(Ok)
        .collect()
}

fn sort_lineage_rows(rows: &mut [&Row], reverse_depth: bool) {
    rows.sort_by(|left, right| {
        let left_depth = lineage_row_depth(left);
        let right_depth = lineage_row_depth(right);
        let depth_order = if reverse_depth {
            right_depth.cmp(&left_depth)
        } else {
            left_depth.cmp(&right_depth)
        };
        depth_order.then_with(|| lineage_row_handle(left).cmp(lineage_row_handle(right)))
    });
}

fn lineage_row_depth(row: &Row) -> i64 {
    required_number(row, "depth")
        .ok()
        .and_then(number_to_i64)
        .unwrap_or(i64::MAX)
}

fn lineage_row_handle(row: &Row) -> &str {
    required_string(row, "other").unwrap_or("")
}

fn write_handle_lineage_group<W: Write>(
    writer: &mut W,
    heading: &str,
    rows: &[&Row],
) -> Result<()> {
    writeln!(writer, "{heading} ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "    (none)")?;
        return Ok(());
    }
    for (index, row) in rows.iter().enumerate() {
        writeln!(writer, "{:>2}. {}", index + 1, lineage_row_summary(row)?)?;
    }
    Ok(())
}

fn lineage_row_summary(row: &Row) -> Result<String> {
    let handle = required_string(row, "other")?;
    let disposition = required_string(row, "disposition")?;
    let status = optional_string(row, "status")?.unwrap_or("unknown");
    let depth = required_number(row, "depth")?;
    let file = required_string(row, "file")?;
    Ok(format!(
        "{handle}  disposition={disposition}  status={status}  depth={}  read=`anneal read {file}`",
        display_number(depth),
    ))
}

#[derive(Clone, Copy)]
struct StatusMetric<'a> {
    category: &'a str,
    name: &'a str,
    count: &'a NumberValue,
}

impl<'a> StatusMetric<'a> {
    fn from_row(row: &'a Row) -> Result<Self> {
        Ok(Self {
            category: required_string(row, "category")?,
            name: required_string(row, "name")?,
            count: required_number(row, "count")?,
        })
    }
}

/// Read a required string field from a runtime row.
pub(super) fn required_string<'a>(row: &'a Row, field: &str) -> Result<&'a str> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(value),
        Some(_) => bail!("status row field {field:?} must be a string"),
        None => bail!("status row missing field {field:?}"),
    }
}

/// Read a nullable string field from a runtime row.
pub(super) fn optional_string<'a>(row: &'a Row, field: &str) -> Result<Option<&'a str>> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("row field {field:?} must be a string"),
    }
}

/// Read a required number field from a runtime row.
pub(super) fn required_number<'a>(row: &'a Row, field: &str) -> Result<&'a NumberValue> {
    match row.fields.get(field) {
        Some(Value::Number(value)) => Ok(value),
        Some(_) => bail!("status row field {field:?} must be a number"),
        None => bail!("status row missing field {field:?}"),
    }
}

/// Read a nullable number field from a runtime row.
pub(super) fn optional_number<'a>(row: &'a Row, field: &str) -> Result<Option<&'a NumberValue>> {
    match row.fields.get(field) {
        Some(Value::Number(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("row field {field:?} must be a number"),
    }
}

/// Read a required boolean field from a runtime row.
pub(super) fn required_bool(row: &Row, field: &str) -> Result<bool> {
    match row.fields.get(field) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("row field {field:?} must be a bool"),
        None => bail!("row missing field {field:?}"),
    }
}

fn number_gt(left: &NumberValue, right: &NumberValue) -> bool {
    match (left, right) {
        (NumberValue::Int(left), NumberValue::Int(right)) => left > right,
        (NumberValue::Float(left), NumberValue::Float(right)) => left > right,
        (NumberValue::Int(left), NumberValue::Float(right)) => left
            .to_string()
            .parse::<f64>()
            .is_ok_and(|left| left > *right),
        (NumberValue::Float(left), NumberValue::Int(right)) => right
            .to_string()
            .parse::<f64>()
            .map_or(true, |right| *left > right),
    }
}

fn display_number(value: &NumberValue) -> String {
    match value {
        NumberValue::Int(value) => value.to_string(),
        NumberValue::Float(value) => format!("{value:.3}"),
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => display_string_value(value),
        Value::Number(value) => display_number(value),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::List(values) => {
            let values = values
                .iter()
                .map(display_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({values})")
        }
    }
}

fn display_string_value(value: &str) -> String {
    const MAX_INLINE_CHARS: usize = 96;

    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rendered = String::new();
    for (index, ch) in collapsed.chars().enumerate() {
        if index == MAX_INLINE_CHARS {
            rendered.push_str("...");
            break;
        }
        rendered.push(ch);
    }
    if rendered.is_empty() {
        r#""""#.to_string()
    } else if rendered
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '/' | '_' | '-' | ':' | '#'))
    {
        rendered
    } else {
        format!("{rendered:?}")
    }
}
