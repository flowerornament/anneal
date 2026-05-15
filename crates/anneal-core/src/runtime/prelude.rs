//! Embedded prelude declarations that the runtime exposes to surfaces.

pub const CONTEXT_VERB_NAME: &str = "context";
pub const CONTEXT_VERB_DOC: &str = "Find the most relevant handles for a goal, read bounded spans, and include a small neighborhood so a cold agent can localize work in one call.";
pub const CONTEXT_OUTPUT_SCHEMA: &str = r#"{"goal":"String","hits":[{"handle":"HandleId","span_id":"String|null","score":"Number","reason":"String","field":"String"}],"spans":[{"handle":"HandleId","span_id":"String","start_line":"Number","end_line":"Number","tokens":"Number","text":"String"}],"neighborhood":[{"handle":"HandleId","neighbor":"HandleId"}]}"#;
pub const CONTEXT_DEFAULT_ARGS: &[&str] = &["goal", "budget", "neighborhood_depth", "hits"];
pub const CONTEXT_CAPABILITIES: &[&str] = &["read"];
pub const VIEWS_PRELUDE: &str = include_str!("prelude/views.dl");

pub struct ContextQueryArgs<'a> {
    pub goal: &'a str,
    pub hits: usize,
    pub per_hit_read_budget: i64,
    pub neighborhood_depth: i64,
    pub include_low_confidence: bool,
}

pub fn render_context_query(args: &ContextQueryArgs<'_>) -> String {
    render_context_query_terms(
        &datalog_string_literal(args.goal),
        &args.hits.to_string(),
        &args.per_hit_read_budget.to_string(),
        &args.neighborhood_depth.to_string(),
        args.include_low_confidence,
    )
}

pub fn context_query_template() -> String {
    render_context_query_terms("goal", "hits", "per_hit_budget", "depth", false)
}

pub fn context_verb_source() -> String {
    format!(
        "\
# Starter verb declarations for the v2 surface.
#
# Phase 6 will add the full standard library. This file starts with the
# executable context contract because cold-agent localization depends on
# its exact query shape.

@verb(
  name: {name},
  query: {query},
  doc: {doc},
  output_schema: {output_schema},
  default_args: {default_args},
  capabilities: {capabilities}
).
",
        name = datalog_string_literal(CONTEXT_VERB_NAME),
        query = datalog_string_literal(&context_query_template()),
        doc = datalog_string_literal(CONTEXT_VERB_DOC),
        output_schema = datalog_string_literal(CONTEXT_OUTPUT_SCHEMA),
        default_args = datalog_string_list(CONTEXT_DEFAULT_ARGS),
        capabilities = datalog_string_list(CONTEXT_CAPABILITIES),
    )
}

pub fn low_confidence_filter(include_low_confidence: bool) -> &'static str {
    if include_low_confidence {
        ""
    } else {
        ",\n        low_confidence = false"
    }
}

pub fn datalog_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn render_context_query_terms(
    goal_term: &str,
    hits_term: &str,
    read_budget_term: &str,
    neighborhood_depth_term: &str,
    include_low_confidence: bool,
) -> String {
    let confidence_filter = low_confidence_filter(include_low_confidence);
    format!(
        "\
context_goal({goal_term}).
context_hits({hits_term}).
context_read_budget({read_budget_term}).
context_neighborhood_depth({neighborhood_depth_term}).

context_readable(h) :=
  *handle{{id: h}},
  context_read_budget(per_hit_budget),
  *content{{handle: h, tokens}},
  tokens <= per_hit_budget.

context_hit(h, hit_span_id, score, reason, field) :=
  context_goal(goal),
  context_hits(hits),
  (h, hit_span_id, score, reason, field) = TopK{{ k: hits, key: score :
    (h, hit_span_id, score, reason, field) :
      search(goal, h, hit_span_id, score, reason, field, low_confidence){confidence_filter},
      context_readable(h)
  }}.

context_neighbor(h, h) := context_hit(h, hit_span_id, score, reason, field).
context_neighbor(h, neighbor) :=
  context_hit(h, hit_span_id, score, reason, field),
  context_neighborhood_depth(depth),
  neighborhood(h, depth, neighbor).

?
  context_hit(h, hit_span_id, score, reason, field),
  context_read_budget(per_hit_budget),
  (span_id, text, start_line, end_line, tokens) = TakeUntil{{
    budget: per_hit_budget, sum: tokens, key: start_line :
    (span_id, text, start_line, end_line, tokens) :
      read(h, per_hit_budget, span_id, text, start_line, end_line, tokens)
  }},
  context_neighbor(h, neighbor).",
    )
}

fn datalog_string_list(values: &[&str]) -> String {
    let values = values
        .iter()
        .map(|value| datalog_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_verb_source_matches_checked_in_views_prelude() {
        assert_eq!(VIEWS_PRELUDE, context_verb_source());
    }
}
