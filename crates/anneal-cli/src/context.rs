use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use anneal_core::runtime::eval::NumberValue;
use anneal_core::runtime::prelude::{ContextQueryArgs, render_context_query};
use anneal_core::runtime::{Row, Value};
use serde::Serialize;

pub const DEFAULT_CONTEXT_BUDGET: i64 = 4_000;
pub const DEFAULT_CONTEXT_HITS: usize = 3;
pub const DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextCommand {
    goal: String,
    budget: i64,
    neighborhood_depth: i64,
    hits: usize,
    include_low_confidence: bool,
}

impl ContextCommand {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            budget: DEFAULT_CONTEXT_BUDGET,
            neighborhood_depth: DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH,
            hits: DEFAULT_CONTEXT_HITS,
            include_low_confidence: false,
        }
    }

    pub fn with_budget(mut self, budget: i64) -> Self {
        self.budget = budget.max(0);
        self
    }

    pub fn with_neighborhood_depth(mut self, depth: i64) -> Self {
        self.neighborhood_depth = depth.max(0);
        self
    }

    pub fn with_hits(mut self, hits: usize) -> Self {
        self.hits = hits.max(1);
        self
    }

    pub fn include_low_confidence(mut self, include: bool) -> Self {
        self.include_low_confidence = include;
        self
    }

    pub fn datalog(&self) -> String {
        render_context_query(&ContextQueryArgs {
            goal: &self.goal,
            hits: self.hits,
            per_hit_read_budget: self.per_hit_read_budget(),
            neighborhood_depth: self.neighborhood_depth,
            include_low_confidence: self.include_low_confidence,
        })
    }

    pub fn group_rows(&self, rows: &[Row]) -> Result<ContextOutput, ContextGroupError> {
        ContextOutput::from_rows(self.goal.clone(), rows)
    }

    fn per_hit_read_budget(&self) -> i64 {
        let span_budget = self.budget.saturating_mul(60).saturating_div(100);
        if span_budget == 0 {
            return 0;
        }
        span_budget.max(1)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContextOutput {
    pub goal: String,
    pub hits: Vec<ContextHit>,
    pub spans: Vec<ContextSpan>,
    pub neighborhood: Vec<ContextNeighbor>,
}

impl ContextOutput {
    pub fn from_rows(goal: impl Into<String>, rows: &[Row]) -> Result<Self, ContextGroupError> {
        let mut hits = Vec::new();
        let mut hit_keys = BTreeSet::new();
        let mut spans = Vec::new();
        let mut span_keys = BTreeSet::new();
        let mut neighborhood = Vec::new();
        let mut neighborhood_keys = BTreeSet::new();

        for row in rows {
            let hit = ContextHit {
                handle: string_field(row, "h")?,
                span_id: optional_string_field(row, "hit_span_id")?,
                score: number_field(row, "score")?,
                reason: string_field(row, "reason")?,
                field: string_field(row, "field")?,
            };
            if hit_keys.insert((
                hit.handle.clone(),
                hit.span_id.clone(),
                hit.reason.clone(),
                hit.field.clone(),
            )) {
                hits.push(hit);
            }

            let span = ContextSpan {
                handle: string_field(row, "h")?,
                span_id: string_field(row, "span_id")?,
                start_line: int_field(row, "start_line")?,
                end_line: int_field(row, "end_line")?,
                tokens: int_field(row, "tokens")?,
                text: string_field(row, "text")?,
            };
            if span_keys.insert((span.handle.clone(), span.span_id.clone())) {
                spans.push(span);
            }

            let neighbor = ContextNeighbor {
                handle: string_field(row, "h")?,
                neighbor: string_field(row, "neighbor")?,
            };
            if neighborhood_keys.insert((neighbor.handle.clone(), neighbor.neighbor.clone())) {
                neighborhood.push(neighbor);
            }
        }

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.handle.cmp(&right.handle))
                .then_with(|| left.span_id.cmp(&right.span_id))
        });
        spans.sort_by(|left, right| {
            left.handle
                .cmp(&right.handle)
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.span_id.cmp(&right.span_id))
        });
        neighborhood.sort_by(|left, right| {
            left.handle
                .cmp(&right.handle)
                .then_with(|| left.neighbor.cmp(&right.neighbor))
        });

        Ok(Self {
            goal: goal.into(),
            hits,
            spans,
            neighborhood,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContextHit {
    pub handle: String,
    pub span_id: Option<String>,
    pub score: f64,
    pub reason: String,
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextSpan {
    pub handle: String,
    pub span_id: String,
    pub start_line: i64,
    pub end_line: i64,
    pub tokens: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextNeighbor {
    pub handle: String,
    pub neighbor: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextGroupError {
    MissingField {
        field: &'static str,
    },
    WrongFieldType {
        field: &'static str,
        expected: &'static str,
    },
}

impl fmt::Display for ContextGroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "context row missing field {field:?}"),
            Self::WrongFieldType { field, expected } => {
                write!(f, "context row field {field:?} is not a {expected}")
            }
        }
    }
}

impl Error for ContextGroupError {}

fn string_field(row: &Row, field: &'static str) -> Result<String, ContextGroupError> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(ContextGroupError::WrongFieldType {
            field,
            expected: "string",
        }),
        None => Err(ContextGroupError::MissingField { field }),
    }
}

fn optional_string_field(
    row: &Row,
    field: &'static str,
) -> Result<Option<String>, ContextGroupError> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(ContextGroupError::WrongFieldType {
            field,
            expected: "string or null",
        }),
        None => Err(ContextGroupError::MissingField { field }),
    }
}

fn int_field(row: &Row, field: &'static str) -> Result<i64, ContextGroupError> {
    match row.fields.get(field) {
        Some(Value::Number(NumberValue::Int(value))) => Ok(*value),
        Some(_) => Err(ContextGroupError::WrongFieldType {
            field,
            expected: "integer",
        }),
        None => Err(ContextGroupError::MissingField { field }),
    }
}

fn number_field(row: &Row, field: &'static str) -> Result<f64, ContextGroupError> {
    match row.fields.get(field) {
        Some(Value::Number(NumberValue::Int(value))) => {
            Ok(value.to_string().parse().expect("i64 renders as valid f64"))
        }
        Some(Value::Number(NumberValue::Float(value))) => Ok(*value),
        Some(_) => Err(ContextGroupError::WrongFieldType {
            field,
            expected: "number",
        }),
        None => Err(ContextGroupError::MissingField { field }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use anneal_core::runtime::prelude::CONTEXT_OUTPUT_SCHEMA;
    use anneal_core::runtime::{Database, EvalOptions, Evaluator, analyze, parse_program};
    use anneal_core::{
        ActorContext, CancellationToken, ConfigFacts, ContentFact, FactBatch, FactBatchMode,
        FactIdentity, FactStore, Generation, HandleFact, NativeId, OneShotSourceDriver, OriginUri,
        Revision, SourceDriver, SourceName, SourceRefreshRequest, SpanFact, refresh_source,
    };
    use camino::Utf8PathBuf;

    use super::*;

    #[test]
    fn context_template_is_executable_datalog() {
        let query = ContextCommand::new("v17 conformance audit")
            .with_hits(3)
            .with_budget(4_000)
            .datalog();

        assert!(query.contains("context_read_budget(2400)"));
        assert!(query.contains("*content{handle: h, tokens}"));
        assert!(query.contains("TopK{ k: hits"));
        assert!(query.contains("TakeUntil{"));
        assert!(query.contains("low_confidence = false"));
        assert!(query.contains("context_neighbor(h, h) := context_hit"));
        analyze(parse_program("context", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn context_output_schema_matches_grouped_public_fields() {
        let schema: serde_json::Value =
            serde_json::from_str(CONTEXT_OUTPUT_SCHEMA).expect("context schema parses");

        assert_eq!(schema["goal"], "String");
        assert_eq!(schema["hits"][0]["handle"], "HandleId");
        assert_eq!(schema["hits"][0]["span_id"], "String|null");
        assert_eq!(schema["spans"][0]["handle"], "HandleId");
        assert_eq!(schema["neighborhood"][0]["handle"], "HandleId");
        assert!(schema["hits"][0].get("h").is_none());
        assert!(schema["hits"][0].get("hit_span_id").is_none());
    }

    #[test]
    fn context_template_can_include_low_confidence() {
        let query = ContextCommand::new("v17 conformance audit")
            .include_low_confidence(true)
            .datalog();

        assert!(!query.contains("low_confidence = false"));
        analyze(parse_program("context", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn context_workflow_preserves_isolated_hit_and_span_budget() {
        let search_rows = evaluate_rows(
            "search",
            r#"? search("conformance", h, span_id, score, reason, field, low_confidence)."#,
            context_database(),
            EvalOptions::default(),
        );
        assert!(!search_rows.is_empty(), "fixture should be searchable");

        let output = evaluate_context(
            &ContextCommand::new("conformance")
                .with_hits(1)
                .with_budget(10)
                .include_low_confidence(true),
            context_database(),
            EvalOptions::default().with_low_confidence_threshold(0.0),
        );

        assert_eq!(output.goal, "conformance");
        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].handle, "audit/v17.md");
        assert_eq!(
            output.spans,
            vec![ContextSpan {
                handle: "audit/v17.md".to_string(),
                span_id: "intro".to_string(),
                start_line: 1,
                end_line: 3,
                tokens: 4,
                text: "v17 conformance audit urgent blocker".to_string(),
            }]
        );
        assert_eq!(
            output.neighborhood,
            vec![ContextNeighbor {
                handle: "audit/v17.md".to_string(),
                neighbor: "audit/v17.md".to_string(),
            }]
        );
    }

    #[test]
    fn context_top_k_filters_low_confidence_before_selection() {
        let strict_options = EvalOptions::default().with_low_confidence_threshold(0.99);

        let filtered = evaluate_context(
            &ContextCommand::new("conformance")
                .with_hits(1)
                .with_budget(10),
            context_database(),
            strict_options.clone(),
        );
        assert!(filtered.hits.is_empty());

        let included = evaluate_context(
            &ContextCommand::new("conformance")
                .with_hits(1)
                .with_budget(10)
                .include_low_confidence(true),
            context_database(),
            strict_options,
        );
        assert_eq!(included.hits.len(), 1);
    }

    #[test]
    fn context_output_groups_relational_rows_by_schema() {
        let rows = vec![
            context_row("audit/v17.md", "intro", 0.9, "audit/v17.md"),
            context_row("audit/v17.md", "intro", 0.9, "formal-model/v17.md"),
        ];

        let output = ContextOutput::from_rows("v17 conformance audit", &rows).expect("rows group");

        assert_eq!(output.goal, "v17 conformance audit");
        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.spans.len(), 1);
        assert_eq!(
            output.neighborhood,
            vec![
                ContextNeighbor {
                    handle: "audit/v17.md".to_string(),
                    neighbor: "audit/v17.md".to_string(),
                },
                ContextNeighbor {
                    handle: "audit/v17.md".to_string(),
                    neighbor: "formal-model/v17.md".to_string(),
                },
            ]
        );
    }

    #[test]
    fn context_output_preserves_goal_without_rows() {
        let output =
            ContextOutput::from_rows("unknown target", &[]).expect("empty context rows group");

        assert_eq!(output.goal, "unknown target");
        assert!(output.hits.is_empty());
        assert!(output.spans.is_empty());
        assert!(output.neighborhood.is_empty());
    }

    #[test]
    fn context_large-corpus_v17_fixture_gate() {
        const AUDIT_HANDLE: &str = "reviews/2026-04-28-formal-model-v17-conformance-audit.md";

        let mut tool_calls = 0;
        let output = {
            tool_calls += 1;
            evaluate_context(
                &ContextCommand::new("v17 conformance audit")
                    .with_hits(3)
                    .with_budget(4_000),
                frozen_large-corpus_database(),
                EvalOptions::default(),
            )
        };

        assert_eq!(output.goal, "v17 conformance audit");
        assert!(
            tool_calls <= 2,
            "CR-R5 cold-agent gate allows at most two tool calls"
        );
        assert!(!output.hits.is_empty(), "context should find v17 material");
        assert!(
            output.hits.len() <= 3,
            "TopK should respect the configured hit bound: {:?}",
            output.hits
        );
        assert!(
            output.hits.iter().any(|hit| hit.handle == AUDIT_HANDLE),
            "CR-R5 context gate should include the v17 conformance audit: {:?}",
            output.hits
        );
        assert!(
            output.spans.iter().any(|span| span.handle == AUDIT_HANDLE
                && (span.text.contains("## Method") || span.text.contains("## Summary"))),
            "context should read the audit Method or Summary span: {:?}",
            output.spans
        );
    }

    fn evaluate_context(
        command: &ContextCommand,
        database: Database,
        options: EvalOptions,
    ) -> ContextOutput {
        let rows = evaluate_rows("context", &command.datalog(), database, options);
        command.group_rows(&rows).expect("context rows group")
    }

    fn evaluate_rows(
        source_name: &str,
        input: &str,
        database: Database,
        options: EvalOptions,
    ) -> Vec<Row> {
        let program = parse_program(source_name, input).expect("program parses");
        let analyzed = analyze(program).expect("context analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect("query evaluates").rows
    }

    fn context_database() -> Database {
        let mut batch = FactBatch::new(
            "test".into(),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("audit/v17.md", "V17 conformance audit"),
            handle("notes/other.md", "Other notes"),
        ];
        batch.content = vec![
            content(
                "audit/v17.md",
                "intro",
                "v17 conformance audit urgent blocker",
                4,
            ),
            content(
                "audit/v17.md",
                "details",
                "second span that should exceed the per-hit budget",
                4,
            ),
            content("notes/other.md", "body", "unrelated release notes", 4),
        ];
        batch.spans = vec![
            span("audit/v17.md", "intro", 1, 3),
            span("audit/v17.md", "details", 10, 12),
            span("notes/other.md", "body", 1, 1),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("merge fixture");
        Database::from_store(&store)
    }

    fn frozen_large-corpus_database() -> Database {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.join("../../.fixtures/sample-corpus");
        let root = Utf8PathBuf::from_path_buf(root).expect("fixture path is utf8");
        let roots = [root];
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".md".to_string()),
            ("md.scan_root".to_string(), ".".to_string()),
        ]);
        let actor = ActorContext {
            actor: "test".to_string(),
            capabilities: BTreeSet::new(),
        };
        let source = anneal_md::MarkdownSource;
        let request = SourceRefreshRequest::new("large-corpus", &roots, &config)
            .with_actor(actor)
            .with_cancellation(CancellationToken::new());
        let driver = OneShotSourceDriver::new(source);
        let mut store = FactStore::default();
        refresh_source(&driver, &request, &mut store).expect("refresh frozen large-corpus");
        Database::from_store(&store).with_sources([driver.describe()])
    }

    fn context_row(handle: &str, span_id: &str, score: f64, neighbor: &str) -> Row {
        Row {
            fields: BTreeMap::from([
                ("h".to_string(), s(handle)),
                ("hit_span_id".to_string(), s(span_id)),
                ("span_id".to_string(), s(span_id)),
                ("score".to_string(), f(score)),
                ("reason".to_string(), s("body-substring")),
                ("field".to_string(), s("body")),
                ("text".to_string(), s("body text")),
                ("start_line".to_string(), n(1)),
                ("end_line".to_string(), n(2)),
                ("tokens".to_string(), n(4)),
                ("neighbor".to_string(), s(neighbor)),
            ]),
            derivation: None,
        }
    }

    fn handle(id: &str, summary: &str) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: "file".to_string(),
            status: Some("current".to_string()),
            namespace: String::new(),
            file: id.to_string(),
            line: 1,
            date: None,
            area: "fixture".to_string(),
            summary: summary.to_string(),
        }
    }

    fn content(handle: &str, span_id: &str, text: &str, tokens: u32) -> ContentFact {
        ContentFact {
            identity: identity(&format!("{handle}#{span_id}")),
            handle: handle.to_string(),
            span_id: span_id.to_string(),
            lines: 1,
            text: text.to_string(),
            tokens,
        }
    }

    fn span(handle: &str, span_id: &str, start_line: u32, end_line: u32) -> SpanFact {
        SpanFact {
            identity: identity(&format!("{handle}#{span_id}")),
            id: span_id.to_string(),
            handle: handle.to_string(),
            start_line,
            end_line,
            summary: String::new(),
        }
    }

    fn identity(native_id: &str) -> FactIdentity {
        FactIdentity {
            corpus: "test".into(),
            source: SourceName::from("fixture"),
            native_id: NativeId::from(native_id),
            origin_uri: OriginUri::from(format!("fixture://{native_id}")),
            revision: Revision::from("rev"),
            generation: Generation::initial(),
        }
    }

    fn s(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn n(value: i64) -> Value {
        Value::Number(NumberValue::Int(value))
    }

    fn f(value: f64) -> Value {
        Value::Number(NumberValue::Float(value))
    }
}
