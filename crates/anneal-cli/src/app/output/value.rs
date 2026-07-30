//! Shared runtime-row access and compact value formatting.

use std::io::Write;

use anneal_core::runtime::NumberValue;
use anneal_core::runtime::{Row, Value};
use anyhow::{Result, bail};

/// Render a bounded, indentation-preserving text block.
pub(super) fn write_text_block<W: Write>(
    writer: &mut W,
    text: &str,
    max_lines: usize,
) -> Result<()> {
    let mut lines = text.lines().skip_while(|line| line.trim().is_empty());
    for line in lines.by_ref().take(max_lines) {
        writeln!(writer, "  {}", line.trim_end())?;
    }
    if lines.next().is_some() {
        writeln!(writer, "  ...")?;
    }
    Ok(())
}

/// Read a required string field from a runtime row.
pub(in crate::app) fn required_string<'a>(row: &'a Row, field: &str) -> Result<&'a str> {
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
pub(in crate::app) fn required_number<'a>(row: &'a Row, field: &str) -> Result<&'a NumberValue> {
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

/// Compare runtime numbers without discarding their integer representation.
pub(super) fn number_gt(left: &NumberValue, right: &NumberValue) -> bool {
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

/// Return the exact integer representation, rejecting floats.
pub(super) fn number_to_i64(number: &NumberValue) -> Option<i64> {
    match number {
        NumberValue::Int(value) => Some(*value),
        NumberValue::Float(_) => None,
    }
}

/// Format a runtime number for compact human output.
pub(super) fn display_number(value: &NumberValue) -> String {
    match value {
        NumberValue::Int(value) => value.to_string(),
        NumberValue::Float(value) => format!("{value:.3}"),
    }
}

/// Format a runtime value recursively for one-line row output.
pub(super) fn display_value(value: &Value) -> String {
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

/// Collapse and quote a string for compact one-line output.
pub(super) fn display_string_value(value: &str) -> String {
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
