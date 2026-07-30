//! Context result rendering.

use std::collections::BTreeMap;
use std::io::Write;

use anneal_core::ranking::{
    CONTEXT_NEIGHBOR_GROUP_CURRENT, CONTEXT_NEIGHBOR_GROUP_HIDDEN,
    CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT, CONTEXT_NEIGHBOR_GROUP_SUPERSEDED,
};
use anneal_core::runtime::prelude::datalog_string_literal;
use anneal_core::runtime::write_ndjson;
use anyhow::Result;
use serde::Serialize;

use crate::ContextOutput;

use super::value::{display_string_value, write_text_block};

/// Render ranked context hits, spans, and neighborhoods for a human reader.
pub(super) fn write_context_text<W: Write>(mut writer: W, output: &ContextOutput) -> Result<()> {
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

/// Stream context sections as typed NDJSON events.
pub(super) fn write_context_ndjson<W: Write>(writer: W, output: &ContextOutput) -> Result<()> {
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
