//! Handle adjacency, impact, lineage, and code-reference rendering.

use std::collections::BTreeMap;
use std::io::Write;

use anneal_core::runtime::NumberValue;
use anneal_core::runtime::Row;
use anneal_core::runtime::datalog_string_literal;
use anyhow::Result;

use super::value::{
    display_number, display_string_value, number_to_i64, optional_string, required_bool,
    required_number, required_string,
};
use super::{EMPTY_ROWS_DIAGNOSTIC, missing_handle_hint};

#[cfg(test)]
mod tests;

/// Render one handle's identity, adjacency, code references, impact, and lineage.
pub(in crate::app) fn write_handle_text<W: Write>(
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
