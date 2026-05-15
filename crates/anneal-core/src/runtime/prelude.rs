//! Embedded prelude declarations that the runtime exposes to surfaces.

use std::sync::LazyLock;

use super::ast::{Program, RuleLayer};
use super::parser::{ParseError, parse_program};

pub const CONTEXT_VERB_NAME: &str = "context";
pub const CONTEXT_VERB_DOC: &str = "Find the most relevant handles for a goal, read bounded spans, and include a small neighborhood so a cold agent can localize work in one call.";
pub const CONTEXT_OUTPUT_SCHEMA: &str = r#"{"goal":"String","hits":[{"handle":"HandleId","span_id":"String|null","score":"Number","reason":"String","field":"String"}],"spans":[{"handle":"HandleId","span_id":"String","start_line":"Number","end_line":"Number","tokens":"Number","text":"String"}],"neighborhood":[{"handle":"HandleId","neighbor":"HandleId"}]}"#;
pub const CONTEXT_DEFAULT_ARGS: &[&str] = &["goal", "budget", "neighborhood_depth", "hits"];
pub const CONTEXT_CAPABILITIES: &[&str] = &["read"];
pub const VIEWS_PRELUDE_DOC: &str = "Saved verb declarations and lifecycle profile examples for the v2 surface. Verbs are project-extensible templates over the same Datalog runtime as the prelude.";
pub const GRAPH_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/graph.dl";
pub const CONVERGENCE_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/convergence.dl";
pub const CHECKS_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/checks.dl";
pub const RANKING_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/ranking.dl";
pub const VIEWS_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/views.dl";
pub const GRAPH_PRELUDE: &str = include_str!("../prelude/graph.dl");
pub const CONVERGENCE_PRELUDE: &str = include_str!("../prelude/convergence.dl");
pub const CHECKS_PRELUDE: &str = include_str!("../prelude/checks.dl");
pub const RANKING_PRELUDE: &str = include_str!("../prelude/ranking.dl");
pub const VIEWS_PRELUDE: &str = include_str!("../prelude/views.dl");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedPreludeFile {
    pub source_name: &'static str,
    pub contents: &'static str,
}

pub const STANDARD_PRELUDE_FILES: &[EmbeddedPreludeFile] = &[
    EmbeddedPreludeFile {
        source_name: GRAPH_PRELUDE_SOURCE,
        contents: GRAPH_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: CONVERGENCE_PRELUDE_SOURCE,
        contents: CONVERGENCE_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: CHECKS_PRELUDE_SOURCE,
        contents: CHECKS_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: RANKING_PRELUDE_SOURCE,
        contents: RANKING_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: VIEWS_PRELUDE_SOURCE,
        contents: VIEWS_PRELUDE,
    },
];

static STANDARD_PRELUDE_PROGRAM: LazyLock<Result<Program, ParseError>> =
    LazyLock::new(parse_standard_prelude_program);

pub fn standard_prelude_program() -> Result<Program, ParseError> {
    match &*STANDARD_PRELUDE_PROGRAM {
        Ok(program) => Ok(program.clone()),
        Err(err) => Err(err.clone()),
    }
}

fn parse_standard_prelude_program() -> Result<Program, ParseError> {
    let mut statements = Vec::new();
    for file in STANDARD_PRELUDE_FILES {
        let mut program = parse_program(file.source_name, file.contents)?;
        program.assign_rule_layer(RuleLayer::Prelude);
        statements.extend(program.statements);
    }
    Ok(Program { statements })
}

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

@doc(
  name: \"views\",
  doc: {views_doc}
).

@verb(
  name: {name},
  query: {query},
  doc: {doc},
  output_schema: {output_schema},
  default_args: {default_args},
  capabilities: {capabilities}
).
",
        views_doc = datalog_string_literal(VIEWS_PRELUDE_DOC),
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
    use std::fmt::Write as _;

    use crate::runtime::ast::Statement;
    use crate::runtime::{Database, Evaluator, Value, analyze, parse_program};

    #[test]
    fn context_verb_source_matches_checked_in_views_prelude() {
        assert_eq!(VIEWS_PRELUDE, context_verb_source());
    }

    #[test]
    fn standard_prelude_file_set_matches_spec_layout() {
        let source_names = STANDARD_PRELUDE_FILES
            .iter()
            .map(|file| file.source_name)
            .collect::<Vec<_>>();

        assert_eq!(
            source_names,
            vec![
                GRAPH_PRELUDE_SOURCE,
                CONVERGENCE_PRELUDE_SOURCE,
                CHECKS_PRELUDE_SOURCE,
                RANKING_PRELUDE_SOURCE,
                VIEWS_PRELUDE_SOURCE,
            ]
        );
        assert!(STANDARD_PRELUDE_FILES.iter().all(|file| {
            parse_program(file.source_name, file.contents).is_ok()
                && !file.contents.trim().is_empty()
        }));
    }

    #[test]
    fn standard_prelude_exposes_source_backed_convergence_doc() {
        let mut program = standard_prelude_program().expect("prelude parses");
        let query = parse_program(
            "describe",
            r#"
            ? describe("convergence", doc).
            ? source_of("convergence", file, lines).
            "#,
        )
        .expect("describe query parses");
        program.statements.extend(query.statements);

        let analyzed = analyze(program).expect("prelude with describe query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        assert!(matches!(
            outputs[0].rows[0].fields.get("doc"),
            Some(Value::String(doc)) if doc.contains("potential") && doc.contains("blocked")
        ));
        assert_eq!(
            outputs[1].rows[0].fields.get("file"),
            Some(&Value::String(CONVERGENCE_PRELUDE_SOURCE.to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("lines"),
            Some(&Value::String("3".to_string()))
        );
    }

    #[test]
    fn standard_prelude_exposes_source_backed_topic_docs() {
        let topic_sources = [
            ("graph", GRAPH_PRELUDE_SOURCE),
            ("convergence", CONVERGENCE_PRELUDE_SOURCE),
            ("checks", CHECKS_PRELUDE_SOURCE),
            ("ranking", RANKING_PRELUDE_SOURCE),
            ("views", VIEWS_PRELUDE_SOURCE),
        ];
        let mut program = standard_prelude_program().expect("prelude parses");
        let mut query_source = String::new();
        for (topic, _) in &topic_sources {
            writeln!(
                query_source,
                "? describe({}, doc).\n? source_of({}, file, lines).",
                datalog_string_literal(topic),
                datalog_string_literal(topic),
            )
            .expect("write query");
        }
        let query = parse_program("describe-topics", &query_source).expect("topic query parses");
        program.statements.extend(query.statements);

        let analyzed = analyze(program).expect("prelude topic query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        for (idx, (topic, source_name)) in topic_sources.iter().enumerate() {
            let describe = &outputs[idx * 2];
            assert_eq!(describe.rows.len(), 1, "describe({topic})");
            assert!(
                matches!(describe.rows[0].fields.get("doc"), Some(Value::String(doc)) if !doc.is_empty()),
                "describe({topic}) should have doc text"
            );

            let source = &outputs[idx * 2 + 1];
            assert_eq!(
                source.rows[0].fields.get("file"),
                Some(&Value::String((*source_name).to_string())),
                "source_of({topic})"
            );
            assert_ne!(
                source.rows[0].fields.get("lines"),
                Some(&Value::String("unknown".to_string())),
                "source_of({topic}) should have concrete lines"
            );
        }
    }

    #[test]
    fn doc_declarations_replace_and_document_predicates() {
        let program = parse_program(
            "docs.dl",
            r#"@doc(name: "topic", doc: "first").
@doc(name: "topic", doc: "second").
fact("a").
topic(x) := fact(x).
? describe("topic", doc).
? predicates("topic", doc, file, lines).
? source_of("topic", file, lines).
"#,
        )
        .expect("doc program parses");
        let analyzed = analyze(program).expect("doc program analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("doc program fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        assert_eq!(outputs[0].rows.len(), 1);
        assert_eq!(
            outputs[0].rows[0].fields.get("doc"),
            Some(&Value::String("second".to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("doc"),
            Some(&Value::String("second".to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("lines"),
            Some(&Value::String("4".to_string()))
        );

        let source_lines = outputs[2]
            .rows
            .iter()
            .map(|row| row.fields.get("lines").cloned())
            .collect::<Vec<_>>();
        assert_eq!(
            source_lines,
            vec![
                Some(Value::String("2".to_string())),
                Some(Value::String("4".to_string())),
            ]
        );
    }

    #[test]
    fn assign_rule_layer_marks_nested_rules_as_prelude() {
        let program = parse_program(
            "layers.dl",
            r#"root(x) := fact(x).
? where local(x) := root(x). local(x).
at("snapshot:last") { historical(h) := *handle{id: h}. }
"#,
        );
        let mut program = program.expect("layer fixture parses");
        program.assign_rule_layer(RuleLayer::Prelude);
        assert_prelude_layers(&program.statements);
    }

    fn assert_prelude_layers(statements: &[Statement]) {
        for statement in statements {
            match statement {
                Statement::Rule(rule) => {
                    assert_eq!(rule.origin.layer, RuleLayer::Prelude);
                }
                Statement::Query(query) => {
                    assert!(
                        query
                            .local_rules
                            .iter()
                            .all(|rule| rule.origin.layer == RuleLayer::Inline)
                    );
                }
                Statement::AtBlock { statements, .. } => assert_prelude_layers(statements),
                Statement::Fact(_)
                | Statement::Include(_)
                | Statement::Import(_)
                | Statement::Verb(_)
                | Statement::Doc(_) => {}
            }
        }
    }
}
