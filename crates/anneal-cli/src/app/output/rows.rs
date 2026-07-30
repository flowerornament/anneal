//! Generic row, read, describe, search, and ranked-anchor rendering.

use std::collections::BTreeMap;
use std::io::Write;

use anneal_core::runtime::eval::NumberValue;
use anneal_core::runtime::{Row, Value, write_ndjson};
use anyhow::Result;
use serde::Serialize;
use serde::ser::SerializeMap;

use super::handle::write_handle_text;
use super::value::{
    display_number, display_value, number_gt, optional_number, optional_string, required_number,
    required_string, write_text_block,
};
use super::{
    EMPTY_ROWS_DIAGNOSTIC, RowView, empty_binding_hint_text, missing_handle_hint, zero_binding_rows,
};

#[cfg(test)]
mod tests;

/// Per-handle signal sets attached only to ranked-anchor JSON rows.
pub(in crate::app) struct RankedAnchorEnrichment {
    pub(in crate::app) handle_field: String,
    pub(in crate::app) signals_by_handle: BTreeMap<String, Vec<RankedAnchorSignal>>,
}

/// One scored provenance signal contributing to an anchor rank.
#[derive(Clone, Debug, Serialize)]
pub(in crate::app) struct RankedAnchorSignal {
    pub(in crate::app) why: String,
    pub(in crate::app) score: NumberValue,
    #[serde(skip)]
    pub(in crate::app) priority: i64,
}

/// Round a search score at the JSON authority boundary.
pub(super) fn round_search_row_score(mut row: Row) -> Row {
    if let Some(Value::Number(NumberValue::Float(score))) = row.fields.get_mut("score") {
        *score = crate::round_json_score(*score);
    }
    row
}

/// Stream ranked anchors with their ordered contributing signal sets.
pub(super) fn write_ranked_anchor_ndjson<W: Write>(
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

/// Dispatch a row view to its human renderer.
pub(super) fn write_rows_text<W: Write>(
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

/// Render describe cards as prose before any residual structured rows.
pub(super) fn write_describe_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
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
