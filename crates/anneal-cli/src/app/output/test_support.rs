use std::collections::BTreeMap;

use anneal_core::runtime::NumberValue;
use anneal_core::runtime::{Row, Value};

use super::value::{required_number, required_string};
use super::{CommandOutput, RepositoryDisclosure, StatusOutput};

pub(super) fn row(fields: &[(&str, Value)]) -> Row {
    Row {
        fields: fields
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect(),
        derivation: None,
    }
}

pub(super) fn status_output(rows: Vec<Row>) -> CommandOutput {
    status_output_with_baseline(rows, true)
}

pub(super) fn status_output_with_baseline(
    rows: Vec<Row>,
    flow_baseline_ready: bool,
) -> CommandOutput {
    CommandOutput::Status(StatusOutput {
        rows,
        flow_baseline_ready,
        repository: RepositoryDisclosure::direct_git(),
    })
}

pub(super) fn status_metric(category: &str, name: &str, count: i64) -> Row {
    row(&[
        ("category", Value::String(category.to_string())),
        ("name", Value::String(name.to_string())),
        ("count", Value::Number(NumberValue::Int(count))),
        ("detail", Value::Null),
    ])
}

pub(super) fn status_metric_counts(rows: &[Row], expected_category: &str) -> BTreeMap<String, i64> {
    rows.iter()
        .filter_map(|row| {
            let category = required_string(row, "category").ok()?;
            if category != expected_category {
                return None;
            }
            let name = required_string(row, "name").ok()?.to_string();
            let count = required_number(row, "count").ok()?;
            let NumberValue::Int(count) = *count else {
                return None;
            };
            Some((name, count))
        })
        .collect()
}
