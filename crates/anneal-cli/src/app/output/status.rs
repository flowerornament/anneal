//! Status dashboard rendering.

use std::collections::BTreeMap;
use std::io::Write;

use anneal_core::runtime::Row;
use anneal_core::runtime::eval::NumberValue;
use anyhow::Result;

use super::EMPTY_ROWS_DIAGNOSTIC;
use super::value::{display_number, number_to_i64, required_number, required_string};

#[cfg(test)]
mod tests;

/// Status relation rows plus the snapshot state needed by the dashboard.
pub(in crate::app) struct StatusOutput {
    pub(in crate::app) rows: Vec<Row>,
    pub(in crate::app) flow_baseline_ready: bool,
}

/// Render the complete human status dashboard from one evaluated row set.
pub(super) fn write_status_text<W: Write>(
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
        "Diagnostics  {} total, {} error, {} warning, {} suggestion, {} info",
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
