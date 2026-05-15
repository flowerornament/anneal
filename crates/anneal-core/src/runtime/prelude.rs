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
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use crate::facts::{
        ConfigFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity, HandleFact, MetaFact,
        SnapshotFact,
    };
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::runtime::QueryOutput;
    use crate::runtime::ast::Statement;
    use crate::runtime::eval::NumberValue;
    use crate::runtime::{Database, Evaluator, Value, analyze, parse_program};
    use crate::store::FactStore;

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
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&FactStore::default()));
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
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&FactStore::default()));
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
    fn standard_prelude_derives_graph_convergence_and_ranking_rules() {
        let outputs = evaluate_standard_prelude_cases(
            &[
                ("area_of", r#"? area_of("ticket-1", area)."#),
                ("status_of", r#"? status_of("ticket-1", status)."#),
                (
                    "incoming_edge",
                    r#"? incoming_edge("closed-issue", from, kind)."#,
                ),
                ("outgoing_edge", r#"? outgoing_edge("ticket-1", to, kind)."#),
                ("orphan", r#"? orphan("REQ-1")."#),
                ("stub", r#"? stub("stub.md")."#),
                (
                    "diagnostic",
                    r#"? diagnostic("E001", severity, "ticket-1", file, line, evidence)."#,
                ),
                ("entropy", r#"? entropy("ticket-1", source)."#),
                ("potential", r#"? potential("ticket-1", energy)."#),
                ("blocked", r#"? blocked("ticket-1")."#),
                ("advancing", r#"? advancing("ticket-2")."#),
                ("ranked_work", r"? ranked_work(h, energy, rank)."),
                ("top_work", r"? top_work(h, energy)."),
                ("describe", r#"? describe("potential", doc)."#),
                ("source_of", r#"? source_of("ranked_work", file, lines)."#),
            ],
            standard_library_database(),
        );

        assert!(has_row(
            output(&outputs, "area_of"),
            &[("area", string("host"))]
        ));
        assert!(has_row(
            output(&outputs, "status_of"),
            &[("status", string("open"))]
        ));
        assert!(has_row(
            output(&outputs, "incoming_edge"),
            &[("from", string("ticket-1")), ("kind", string("DependsOn"))]
        ));
        assert!(has_row(
            output(&outputs, "outgoing_edge"),
            &[
                ("to", string("closed-issue")),
                ("kind", string("DependsOn"))
            ]
        ));
        assert_eq!(
            output(&outputs, "orphan").rows.len(),
            1,
            "REQ-1 is orphaned"
        );
        assert_eq!(
            output(&outputs, "stub").rows.len(),
            1,
            "stub.md is a content stub"
        );
        assert!(has_row(
            output(&outputs, "diagnostic"),
            &[
                ("severity", string("error")),
                ("file", string("ticket-1.md")),
                ("line", int(7)),
                (
                    "evidence",
                    list(vec![string("broken_ref"), string("ghost")])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "entropy"),
            &[("source", string("broken_ref"))]
        ));
        assert!(has_row(
            output(&outputs, "entropy"),
            &[("source", string("stale_dep"))]
        ));
        assert!(has_row(
            output(&outputs, "potential"),
            &[("energy", int(7))]
        ));
        assert_eq!(
            output(&outputs, "blocked").rows.len(),
            1,
            "ticket-1 is blocked"
        );
        assert_eq!(
            output(&outputs, "advancing").rows.len(),
            1,
            "ticket-2 advanced"
        );
        assert!(
            has_row(
                output(&outputs, "ranked_work"),
                &[
                    ("h", string("ticket-1")),
                    ("energy", int(7)),
                    ("rank", int(1))
                ]
            ),
            "ranked_work rows: {:?}",
            output(&outputs, "ranked_work").rows
        );
        assert!(has_row(
            output(&outputs, "top_work"),
            &[("h", string("REQ-1")), ("energy", int(6))]
        ));
        assert!(matches!(
            output(&outputs, "describe").rows[0].fields.get("doc"),
            Some(Value::String(doc)) if doc.contains("convergence energy")
        ));
        assert_eq!(
            output(&outputs, "source_of").rows[0].fields.get("file"),
            Some(&Value::String(RANKING_PRELUDE_SOURCE.to_string()))
        );
    }

    #[test]
    fn standard_prelude_derives_v1_diagnostic_catalog_relations() {
        let outputs = evaluate_standard_prelude_cases(
            &[
                (
                    "E001",
                    r#"? diagnostic("E001", severity, "broken.md", file, line, evidence)."#,
                ),
                (
                    "I001",
                    r#"? diagnostic("I001", severity, subject, file, line, evidence)."#,
                ),
                (
                    "W004",
                    r#"? diagnostic("W004", severity, "implausible.md", file, line, evidence)."#,
                ),
                (
                    "W001",
                    r#"? diagnostic("W001", severity, "stale-src.md", file, line, evidence)."#,
                ),
                (
                    "W002",
                    r#"? diagnostic("W002", severity, "stable-src.md", file, line, evidence)."#,
                ),
                (
                    "E002",
                    r#"? diagnostic("E002", severity, "OQ-1", file, line, evidence)."#,
                ),
                (
                    "I002",
                    r#"? diagnostic("I002", severity, "OQ-2", file, line, evidence)."#,
                ),
                (
                    "W003",
                    r#"? diagnostic("W003", severity, "team/missing.md", file, line, evidence)."#,
                ),
                (
                    "S001",
                    r#"? diagnostic("S001", severity, "ORPH-1", file, line, evidence)."#,
                ),
                (
                    "S002",
                    r#"? diagnostic("S002", severity, "NEW", file, line, evidence)."#,
                ),
                (
                    "S003",
                    r#"? diagnostic("S003", severity, "draft", file, line, evidence)."#,
                ),
                (
                    "S004",
                    r#"? diagnostic("S004", severity, "OLD", file, line, evidence)."#,
                ),
                (
                    "S005",
                    r#"? diagnostic("S005", severity, "AA", file, line, evidence)."#,
                ),
            ],
            diagnostic_catalog_database(),
        );

        assert!(
            has_row(
                output(&outputs, "E001"),
                &[
                    ("severity", string("error")),
                    ("file", string("broken.md")),
                    ("line", int(3)),
                    (
                        "evidence",
                        list(vec![string("broken_ref"), string("missing.md")])
                    )
                ]
            ),
            "E001 rows: {:?}",
            output(&outputs, "E001").rows
        );
        assert!(has_row(
            output(&outputs, "I001"),
            &[
                ("severity", string("info")),
                ("subject", string("corpus")),
                ("file", Value::Null),
                ("evidence", list(vec![string("section_refs"), int(1)]))
            ]
        ));
        assert!(has_row(
            output(&outputs, "W004"),
            &[
                ("severity", string("warning")),
                ("file", string("implausible.md")),
                (
                    "evidence",
                    list(vec![
                        string("implausible_ref"),
                        string(r#"{"value":"/tmp/foo","reason":"absolute path","line":4}"#)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W001"),
            &[
                ("severity", string("warning")),
                ("file", string("stale-src.md")),
                (
                    "evidence",
                    list(vec![
                        string("stale_ref"),
                        string("draft"),
                        string("archived")
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W002"),
            &[
                ("severity", string("warning")),
                ("file", string("stable-src.md")),
                (
                    "evidence",
                    list(vec![
                        string("confidence_gap"),
                        string("stable"),
                        int(1),
                        string("draft"),
                        int(0)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "E002"),
            &[
                ("severity", string("error")),
                ("file", string("OQ-1.md")),
                ("evidence", string("undischarged"))
            ]
        ));
        assert!(has_row(
            output(&outputs, "I002"),
            &[
                ("severity", string("info")),
                ("file", string("OQ-2.md")),
                (
                    "evidence",
                    list(vec![string("multiple_discharges"), int(2)])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W003"),
            &[
                ("severity", string("warning")),
                ("file", string("team/missing.md")),
                ("evidence", Value::Null)
            ]
        ));
        assert!(has_row(
            output(&outputs, "S001"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![string("orphaned_handle"), string("ORPH-1")])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S002"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![string("candidate_namespace"), string("NEW"), int(3)])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S003"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("pipeline_stall"),
                        string("draft"),
                        int(20),
                        string("stable"),
                        Value::Bool(false)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S004"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("abandoned_namespace"),
                        string("OLD"),
                        int(2),
                        int(2),
                        int(0)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S005"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("concern_group_candidate"),
                        string("AA"),
                        string("BB"),
                        int(3)
                    ])
                )
            ]
        ));
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

    fn evaluate_standard_prelude_queries(source: &str, database: Database) -> Vec<QueryOutput> {
        let mut program = standard_prelude_program().expect("prelude parses");
        let query = parse_program("stdlib-test", source).expect("query parses");
        program.statements.extend(query.statements);
        let analyzed = analyze(program).expect("prelude query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect()
    }

    fn evaluate_standard_prelude_cases(
        cases: &[(&'static str, &str)],
        database: Database,
    ) -> BTreeMap<&'static str, QueryOutput> {
        let mut source = String::new();
        for (_, query) in cases {
            writeln!(&mut source, "{query}").expect("write query case");
        }

        let outputs = evaluate_standard_prelude_queries(&source, database);
        assert_eq!(outputs.len(), cases.len(), "one output per query case");
        cases.iter().map(|(name, _)| *name).zip(outputs).collect()
    }

    fn output<'a>(outputs: &'a BTreeMap<&'static str, QueryOutput>, name: &str) -> &'a QueryOutput {
        outputs.get(name).expect("query case exists")
    }

    fn standard_library_database() -> Database {
        let corpus = CorpusId::from("test");
        let source = SourceName::from("host");
        let generation = Generation::initial();
        let scope = FixtureScope {
            corpus: &corpus,
            source: &source,
            generation,
        };
        let mut batch = FactBatch::new(
            corpus.clone(),
            source.clone(),
            FactBatchMode::FullSnapshot,
            generation,
        );
        batch.handles = vec![
            handle(&scope, "ticket-1", "issue", Some("open"), "", "host"),
            handle(&scope, "closed-issue", "issue", Some("closed"), "", "host"),
            handle(&scope, "ticket-2", "issue", Some("review"), "", "host"),
            handle(&scope, "REQ-1", "label", Some("open"), "REQ", "host"),
            handle(&scope, "stub.md", "file", Some("open"), "", "host"),
        ];
        batch.edges = vec![
            edge(&scope, "ticket-1", "closed-issue", "DependsOn", 4),
            edge(&scope, "ticket-1", "ghost", "Cites", 7),
        ];

        let mut store = FactStore::default();
        store.merge(batch).expect("merge stdlib fixture");
        store
            .replace_configs(
                &corpus,
                vec![
                    config(&corpus, "convergence.active", "open", None),
                    config(&corpus, "convergence.active", "review", None),
                    config(&corpus, "convergence.terminal", "closed", None),
                    config(&corpus, "convergence.ordering", "open", Some(0)),
                    config(&corpus, "convergence.ordering", "review", Some(1)),
                    config(&corpus, "convergence.ordering", "closed", Some(2)),
                    config(&corpus, "handles.linear", "REQ", None),
                ],
            )
            .expect("replace stdlib fixture config");
        store
            .replace_snapshots(
                &corpus,
                vec![SnapshotFact {
                    corpus: corpus.clone(),
                    snapshot: "s1".to_string(),
                    at: "2026-05-01".to_string(),
                    id: "ticket-2".to_string(),
                    key: "status".to_string(),
                    value: "open".to_string(),
                }],
            )
            .expect("replace stdlib fixture snapshots");
        Database::from_store(&store)
    }

    fn diagnostic_catalog_database() -> Database {
        let corpus = CorpusId::from("diagnostics");
        let source = SourceName::from("host");
        let generation = Generation::initial();
        let scope = FixtureScope {
            corpus: &corpus,
            source: &source,
            generation,
        };
        let mut batch = FactBatch::new(
            corpus.clone(),
            source.clone(),
            FactBatchMode::FullSnapshot,
            generation,
        );
        batch.handles = vec![
            handle(&scope, "broken.md", "file", Some("draft"), "", ""),
            handle(&scope, "section.md", "file", Some("draft"), "", ""),
            handle(&scope, "implausible.md", "file", Some("draft"), "", ""),
            handle(&scope, "stale-src.md", "file", Some("draft"), "", ""),
            handle(&scope, "terminal.md", "file", Some("archived"), "", ""),
            handle(&scope, "stable-src.md", "file", Some("stable"), "", ""),
            handle(&scope, "draft-target.md", "file", Some("draft"), "", ""),
            handle(&scope, "team/with-a.md", "file", Some("draft"), "", "team"),
            handle(&scope, "team/with-b.md", "file", Some("draft"), "", "team"),
            handle(&scope, "team/missing.md", "file", None, "", "team"),
            handle(&scope, "OQ-1", "label", Some("draft"), "OQ", ""),
            handle(&scope, "OQ-2", "label", Some("draft"), "OQ", ""),
            handle(&scope, "impl-1.md", "file", Some("draft"), "", ""),
            handle(&scope, "impl-2.md", "file", Some("draft"), "", ""),
            handle(&scope, "ORPH-1", "label", Some("draft"), "ORPH", ""),
            handle(&scope, "NEW-1", "label", Some("draft"), "NEW", ""),
            handle(&scope, "NEW-2", "label", Some("draft"), "NEW", ""),
            handle(&scope, "NEW-3", "label", Some("draft"), "NEW", ""),
            handle(&scope, "OLD-1", "label", Some("archived"), "OLD", ""),
            handle(&scope, "OLD-2", "label", Some("archived"), "OLD", ""),
            handle(&scope, "AA-1", "label", Some("draft"), "AA", ""),
            handle(&scope, "BB-1", "label", Some("draft"), "BB", ""),
            handle(&scope, "co1.md", "file", Some("draft"), "", ""),
            handle(&scope, "co2.md", "file", Some("draft"), "", ""),
            handle(&scope, "co3.md", "file", Some("draft"), "", ""),
        ];
        batch.edges = vec![
            edge(&scope, "broken.md", "missing.md", "Cites", 3),
            edge(&scope, "section.md", "section:intro", "Cites", 9),
            edge(&scope, "stale-src.md", "terminal.md", "DependsOn", 1),
            edge(&scope, "stable-src.md", "draft-target.md", "DependsOn", 2),
            edge(&scope, "impl-1.md", "OQ-2", "Discharges", 1),
            edge(&scope, "impl-2.md", "OQ-2", "Discharges", 1),
            edge(&scope, "co1.md", "AA-1", "Cites", 1),
            edge(&scope, "co1.md", "BB-1", "Cites", 2),
            edge(&scope, "co2.md", "AA-1", "Cites", 1),
            edge(&scope, "co2.md", "BB-1", "Cites", 2),
            edge(&scope, "co3.md", "AA-1", "Cites", 1),
            edge(&scope, "co3.md", "BB-1", "Cites", 2),
        ];
        batch.meta = vec![
            meta(
                &scope,
                "implausible.md",
                "md.implausible_ref",
                r#"{"value":"/tmp/foo","reason":"absolute path","line":4}"#,
            ),
            meta(&scope, "team/with-a.md", "md.parent_dir", "team"),
            meta(&scope, "team/with-b.md", "md.parent_dir", "team"),
            meta(&scope, "team/missing.md", "md.parent_dir", "team"),
        ];

        let mut store = FactStore::default();
        store.merge(batch).expect("merge diagnostic fixture");
        store
            .replace_configs(
                &corpus,
                vec![
                    config(&corpus, "convergence.active", "draft", None),
                    config(&corpus, "convergence.active", "stable", None),
                    config(&corpus, "convergence.terminal", "archived", None),
                    config(&corpus, "convergence.ordering", "draft", Some(0)),
                    config(&corpus, "convergence.ordering", "stable", Some(1)),
                    config(&corpus, "convergence.ordering", "archived", Some(2)),
                    config(&corpus, "handles.linear", "OQ", None),
                    config(&corpus, "handles.confirmed", "OLD", None),
                    config(&corpus, "handles.confirmed", "AA", None),
                    config(&corpus, "handles.confirmed", "BB", None),
                ],
            )
            .expect("replace diagnostic fixture config");
        Database::from_store(&store)
    }

    struct FixtureScope<'a> {
        corpus: &'a CorpusId,
        source: &'a SourceName,
        generation: Generation,
    }

    fn handle(
        scope: &FixtureScope<'_>,
        id: &str,
        kind: &str,
        status: Option<&str>,
        namespace: &str,
        area: &str,
    ) -> HandleFact {
        let file = fixture_file_for(id);
        HandleFact {
            identity: identity(scope, id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(str::to_string),
            namespace: namespace.to_string(),
            file,
            line: 1,
            date: None,
            area: area.to_string(),
            summary: String::new(),
        }
    }

    fn edge(scope: &FixtureScope<'_>, from: &str, to: &str, kind: &str, line: u32) -> EdgeFact {
        let file = fixture_file_for(from);
        EdgeFact {
            identity: identity(scope, &format!("{from}->{to}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file,
            line,
        }
    }

    fn meta(scope: &FixtureScope<'_>, handle: &str, key: &str, value: &str) -> MetaFact {
        MetaFact {
            identity: identity(scope, &format!("{handle}:{key}:{value}")),
            handle: handle.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn fixture_file_for(id: &str) -> String {
        if std::path::Path::new(id)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            id.to_string()
        } else {
            format!("{id}.md")
        }
    }

    fn identity(scope: &FixtureScope<'_>, native_id: &str) -> FactIdentity {
        FactIdentity::new(
            scope.corpus.clone(),
            scope.source.clone(),
            NativeId::from(native_id),
            OriginUri::from(format!("fixture://{native_id}")),
            Revision::from("test"),
            scope.generation,
        )
    }

    fn config(corpus: &CorpusId, key: &str, value: &str, ordinal: Option<u32>) -> ConfigFact {
        ConfigFact {
            corpus: corpus.clone(),
            key: key.to_string(),
            value: value.to_string(),
            ordinal,
        }
    }

    fn has_row(output: &QueryOutput, expected: &[(&str, Value)]) -> bool {
        output.rows.iter().any(|row| {
            expected
                .iter()
                .all(|(field, value)| row.fields.get(*field) == Some(value))
        })
    }

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn int(value: i64) -> Value {
        Value::Number(NumberValue::Int(value))
    }

    fn list(values: Vec<Value>) -> Value {
        Value::List(values)
    }
}
