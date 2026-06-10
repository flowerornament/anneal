//! Goal-oriented context selection for the CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use anneal_core::ranking::{context_neighbor_sort_score, context_sort_score};
use anneal_core::runtime::eval::NumberValue;
use anneal_core::runtime::prelude::{ContextQueryArgs, render_context_query};
use anneal_core::runtime::{Row, Value};
use serde::Serialize;

pub const DEFAULT_CONTEXT_BUDGET: i64 = 4_000;
pub const DEFAULT_CONTEXT_HITS: usize = 3;
pub const DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH: i64 = 1;
const CONTEXT_CANDIDATE_MULTIPLIER: usize = 8;
const MIN_CONTEXT_CANDIDATES: usize = 20;
const MAX_CONTEXT_CANDIDATES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextCommand {
    goal: String,
    budget: i64,
    neighborhood_depth: i64,
    hits: usize,
    include_low_confidence: bool,
    read_spans: bool,
}

impl ContextCommand {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            budget: DEFAULT_CONTEXT_BUDGET,
            neighborhood_depth: DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH,
            hits: DEFAULT_CONTEXT_HITS,
            include_low_confidence: false,
            read_spans: false,
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

    pub fn read_spans(mut self, read: bool) -> Self {
        self.read_spans = read;
        self
    }

    pub fn datalog(&self) -> String {
        render_context_query(&ContextQueryArgs {
            goal: &self.goal,
            hits: self.candidate_hits(),
            per_hit_read_budget: self.per_hit_read_budget(),
            neighborhood_depth: self.neighborhood_depth,
            include_low_confidence: self.include_low_confidence,
        })
    }

    pub fn group_rows(&self, rows: &[Row]) -> Result<ContextOutput, ContextGroupError> {
        ContextOutput::from_rows_with_limit(self.goal.clone(), rows, self.hits, self.read_spans)
    }

    fn per_hit_read_budget(&self) -> i64 {
        let span_budget = self.budget.saturating_mul(60).saturating_div(100);
        if span_budget == 0 {
            return 0;
        }
        span_budget.max(1)
    }

    fn candidate_hits(&self) -> usize {
        let candidates = self
            .hits
            .saturating_mul(CONTEXT_CANDIDATE_MULTIPLIER)
            .max(self.hits);
        let candidates = if candidates < MIN_CONTEXT_CANDIDATES {
            MIN_CONTEXT_CANDIDATES
        } else {
            candidates
        };
        candidates.min(MAX_CONTEXT_CANDIDATES).max(self.hits)
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
        Self::from_rows_with_limit(goal, rows, usize::MAX, true)
    }

    fn from_rows_with_limit(
        goal: impl Into<String>,
        rows: &[Row],
        hit_limit: usize,
        read_spans: bool,
    ) -> Result<Self, ContextGroupError> {
        let mut hits = Vec::new();
        let mut spans = Vec::new();
        let mut span_keys = BTreeSet::new();
        let mut neighborhood = Vec::new();
        let mut neighborhood_keys = BTreeSet::new();

        for row in rows {
            match ContextSection::parse(&string_field(row, "section")?)? {
                ContextSection::Hit => {
                    let hit = RankedContextHit {
                        sort_score: context_hit_sort_score(row)?,
                        hit: ContextHit {
                            handle: string_field(row, "h")?,
                            span_id: optional_string_field(row, "hit_span_id")?,
                            score: number_field(row, "score")?,
                            reason: string_field(row, "reason")?,
                            field: string_field(row, "field")?,
                            summary: optional_string_field(row, "summary")?,
                            status: optional_string_field(row, "status")?,
                            disposition: string_field(row, "disposition")?,
                            age_days: optional_int_field(row, "age_days")?,
                            topic_signal: string_field(row, "topic_signal")?,
                            newer_topic_sibling_count: int_field(row, "newer_topic_sibling_count")?,
                            top_newer_topic_sibling: optional_string_field(
                                row,
                                "top_newer_topic_sibling",
                            )?,
                        },
                    };
                    hits.push(hit);
                }
                ContextSection::Span => {
                    let span = ContextSpan {
                        handle: string_field(row, "h")?,
                        span_id: string_field(row, "span_id")?,
                        start_line: int_field(row, "start_line")?,
                        end_line: int_field(row, "end_line")?,
                        tokens: int_field(row, "tokens")?,
                        text: read_spans.then(|| string_field(row, "text")).transpose()?,
                    };
                    if span_keys.insert((span.handle.clone(), span.span_id.clone())) {
                        spans.push(span);
                    }
                }
                ContextSection::Neighbor => {
                    let neighbor = ContextNeighbor {
                        handle: string_field(row, "h")?,
                        neighbor: string_field(row, "neighbor")?,
                        status: optional_string_field(row, "neighbor_status")?,
                        disposition: string_field(row, "neighbor_disposition")?,
                        age_days: optional_int_field(row, "neighbor_age_days")?,
                        degree: int_field(row, "neighbor_degree")?,
                        group: string_field(row, "neighbor_group")?,
                    };
                    if neighborhood_keys
                        .insert((neighbor.handle.clone(), neighbor.neighbor.clone()))
                    {
                        neighborhood.push(neighbor);
                    }
                }
            }
        }

        hits.sort_by(|left, right| {
            right
                .sort_score
                .total_cmp(&left.sort_score)
                .then_with(|| right.hit.score.total_cmp(&left.hit.score))
                .then_with(|| left.hit.handle.cmp(&right.hit.handle))
                .then_with(|| left.hit.span_id.cmp(&right.hit.span_id))
        });
        let mut hit_keys = BTreeSet::new();
        hits.retain(|hit| hit_keys.insert((hit.hit.handle.clone(), hit.hit.span_id.clone())));
        prefer_matched_span_hits(&mut hits, &mut spans);
        hits.truncate(hit_limit);
        filter_context_to_hits(&hits, &mut spans, &mut neighborhood);
        let handle_ranks = handle_rank_map(&hits);

        spans.sort_by(|left, right| {
            handle_rank(&handle_ranks, &left.handle)
                .cmp(&handle_rank(&handle_ranks, &right.handle))
                .then_with(|| left.handle.cmp(&right.handle))
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.span_id.cmp(&right.span_id))
        });
        neighborhood.sort_by(|left, right| {
            handle_rank(&handle_ranks, &left.handle)
                .cmp(&handle_rank(&handle_ranks, &right.handle))
                .then_with(|| right.sort_score().total_cmp(&left.sort_score()))
                .then_with(|| left.handle.cmp(&right.handle))
                .then_with(|| left.neighbor.cmp(&right.neighbor))
        });

        Ok(Self {
            goal: goal.into(),
            hits: hits.into_iter().map(|ranked| ranked.hit).collect(),
            spans,
            neighborhood,
        })
    }
}

fn prefer_matched_span_hits(hits: &mut Vec<RankedContextHit>, spans: &mut Vec<ContextSpan>) {
    let retained_span_ids = hit_span_ids_by_handle(hits);

    if retained_span_ids.is_empty() {
        return;
    }

    hits.retain(|hit| {
        hit.hit.span_id.is_some()
            || hit.hit.reason == anneal_core::REASON_PARENT_CLUSTER
            || !retained_span_ids.contains_key(hit.hit.handle.as_str())
    });
    spans.retain(|span| {
        retained_span_ids
            .get(span.handle.as_str())
            .is_none_or(|span_ids| span_ids.contains(span.span_id.as_str()))
    });
}

fn filter_context_to_hits(
    hits: &[RankedContextHit],
    spans: &mut Vec<ContextSpan>,
    neighborhood: &mut Vec<ContextNeighbor>,
) {
    let retained_handles = hits
        .iter()
        .map(|hit| hit.hit.handle.as_str())
        .collect::<BTreeSet<_>>();
    let retained_span_ids = hit_span_ids_by_handle(hits);

    spans.retain(|span| {
        if !retained_handles.contains(span.handle.as_str()) {
            return false;
        }
        retained_span_ids
            .get(span.handle.as_str())
            .is_none_or(|span_ids| span_ids.contains(span.span_id.as_str()))
    });
    neighborhood.retain(|neighbor| retained_handles.contains(neighbor.handle.as_str()));
}

fn hit_span_ids_by_handle(hits: &[RankedContextHit]) -> BTreeMap<String, BTreeSet<String>> {
    let mut span_ids_by_handle = BTreeMap::<String, BTreeSet<String>>::new();
    for hit in hits {
        if let Some(span_id) = &hit.hit.span_id {
            span_ids_by_handle
                .entry(hit.hit.handle.clone())
                .or_default()
                .insert(span_id.clone());
        }
    }
    span_ids_by_handle
}

fn handle_rank_map(hits: &[RankedContextHit]) -> BTreeMap<&str, usize> {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| (hit.hit.handle.as_str(), index))
        .collect()
}

fn handle_rank(ranks: &BTreeMap<&str, usize>, handle: &str) -> usize {
    ranks.get(handle).copied().unwrap_or(usize::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextSection {
    Hit,
    Span,
    Neighbor,
}

impl ContextSection {
    fn parse(value: &str) -> Result<Self, ContextGroupError> {
        match value {
            "hit" => Ok(Self::Hit),
            "span" => Ok(Self::Span),
            "neighbor" => Ok(Self::Neighbor),
            section => Err(ContextGroupError::UnknownSection {
                section: section.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct RankedContextHit {
    hit: ContextHit,
    sort_score: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContextHit {
    pub handle: String,
    pub span_id: Option<String>,
    pub score: f64,
    pub reason: String,
    pub field: String,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub disposition: String,
    pub age_days: Option<i64>,
    pub topic_signal: String,
    pub newer_topic_sibling_count: i64,
    pub top_newer_topic_sibling: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextSpan {
    pub handle: String,
    pub span_id: String,
    pub start_line: i64,
    pub end_line: i64,
    pub tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextNeighbor {
    pub handle: String,
    pub neighbor: String,
    pub status: Option<String>,
    pub disposition: String,
    pub age_days: Option<i64>,
    pub degree: i64,
    pub group: String,
}

impl ContextNeighbor {
    fn sort_score(&self) -> f64 {
        context_neighbor_sort_score(
            self.group.as_str(),
            self.disposition.as_str(),
            self.degree,
            self.handle == self.neighbor,
        )
    }
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
    UnknownSection {
        section: String,
    },
}

impl fmt::Display for ContextGroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "context row missing field {field:?}"),
            Self::WrongFieldType { field, expected } => {
                write!(f, "context row field {field:?} is not a {expected}")
            }
            Self::UnknownSection { section } => {
                write!(f, "context row has unknown section {section:?}")
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

fn optional_int_field(row: &Row, field: &'static str) -> Result<Option<i64>, ContextGroupError> {
    match row.fields.get(field) {
        Some(Value::Number(NumberValue::Int(value))) => Ok(Some(*value)),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(ContextGroupError::WrongFieldType {
            field,
            expected: "integer or null",
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

fn context_hit_sort_score(row: &Row) -> Result<f64, ContextGroupError> {
    let score = number_field(row, "score")?;
    let field = string_field(row, "field")?;
    let reason = string_field(row, "reason")?;
    Ok(context_sort_score(score, reason.as_str(), field.as_str()))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use anneal_core::runtime::prelude::{CONTEXT_OUTPUT_SCHEMA, standard_prelude_program};
    use anneal_core::runtime::{Database, EvalOptions, Evaluator, analyze, parse_program};
    use anneal_core::{
        ActorContext, CancellationToken, ConfigFacts, ContentFact, EdgeFact, FactBatch,
        FactBatchMode, FactIdentity, FactStore, Generation, HandleFact, NativeId,
        OneShotSourceDriver, OriginUri, Revision, SourceDriver, SourceName, SourceRefreshRequest,
        SpanFact, refresh_source,
    };
    use camino::Utf8PathBuf;

    use super::*;

    #[test]
    fn context_template_is_executable_datalog() {
        let query = ContextCommand::new("v17 conformance audit")
            .with_hits(3)
            .with_budget(4_000)
            .datalog();

        assert!(query.contains("verb_arg(\"budget\", 2400)"));
        assert!(query.contains("*content{handle: h, tokens}"));
        assert!(query.contains("TopK{ k: hits"));
        assert!(query.contains("TakeUntil{"));
        assert!(query.contains("low_confidence = false"));
        assert!(query.contains("context_hit_handle(h) := context_hit"));
        assert!(query.contains("context_neighbor_seed(h, h) := context_hit_handle"));
        assert!(query.contains("TopK{ k: 16, key: group_score + disposition_score"));
        assert!(query.contains(r#"context_output("hit""#));
        analyze_context_query(&query);
    }

    #[test]
    fn context_output_schema_matches_grouped_public_fields() {
        let schema: serde_json::Value =
            serde_json::from_str(CONTEXT_OUTPUT_SCHEMA).expect("context schema parses");

        assert_eq!(schema["goal"], "String");
        assert_eq!(schema["hits"][0]["handle"], "HandleId");
        assert_eq!(schema["hits"][0]["span_id"], "String|null");
        assert_eq!(schema["hits"][0]["summary"], "String|null");
        assert_eq!(schema["hits"][0]["status"], "String|null");
        assert_eq!(schema["hits"][0]["disposition"], "String");
        assert_eq!(schema["hits"][0]["age_days"], "Number|null");
        assert_eq!(schema["hits"][0]["topic_signal"], "String");
        assert_eq!(schema["hits"][0]["newer_topic_sibling_count"], "Number");
        assert_eq!(
            schema["hits"][0]["top_newer_topic_sibling"],
            "HandleId|null"
        );
        assert_eq!(schema["spans"][0]["handle"], "HandleId");
        assert_eq!(
            schema["spans"][0]["text"],
            "String|null; present with --read-spans"
        );
        assert_eq!(schema["neighborhood"][0]["handle"], "HandleId");
        assert_eq!(schema["neighborhood"][0]["status"], "String|null");
        assert_eq!(schema["neighborhood"][0]["disposition"], "String");
        assert_eq!(schema["neighborhood"][0]["age_days"], "Number|null");
        assert_eq!(schema["neighborhood"][0]["degree"], "Number");
        assert_eq!(schema["neighborhood"][0]["group"], "String");
        assert!(schema["hits"][0].get("h").is_none());
        assert!(schema["hits"][0].get("hit_span_id").is_none());
    }

    #[test]
    fn context_template_can_include_low_confidence() {
        let query = ContextCommand::new("v17 conformance audit")
            .include_low_confidence(true)
            .datalog();

        assert!(!query.contains("low_confidence = false"));
        analyze_context_query(&query);
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
            &ContextCommand::new("urgent blocker")
                .with_hits(1)
                .with_budget(10)
                .include_low_confidence(true),
            context_database(),
            EvalOptions::default().with_low_confidence_threshold(0.0),
        );

        assert_eq!(output.goal, "urgent blocker");
        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].handle, "audit/v17.md");
        assert_eq!(output.hits[0].summary.as_deref(), Some("Intro"));
        assert_eq!(
            output.spans,
            vec![ContextSpan {
                handle: "audit/v17.md".to_string(),
                span_id: "intro".to_string(),
                start_line: 1,
                end_line: 3,
                tokens: 4,
                text: None,
            }]
        );
        assert_eq!(
            output.neighborhood,
            vec![ContextNeighbor {
                handle: "audit/v17.md".to_string(),
                neighbor: "audit/v17.md".to_string(),
                status: Some("current".to_string()),
                disposition: "current".to_string(),
                age_days: None,
                degree: 0,
                group: "current".to_string(),
            }]
        );
    }

    #[test]
    fn context_read_spans_controls_body_expansion() {
        let compact = evaluate_context(
            &ContextCommand::new("urgent blocker")
                .with_hits(1)
                .with_budget(10)
                .include_low_confidence(true),
            context_database(),
            EvalOptions::default().with_low_confidence_threshold(0.0),
        );
        assert_eq!(compact.spans.len(), 1);
        assert_eq!(compact.spans[0].text, None);

        let expanded = evaluate_context(
            &ContextCommand::new("urgent blocker")
                .with_hits(1)
                .with_budget(10)
                .include_low_confidence(true)
                .read_spans(true),
            context_database(),
            EvalOptions::default().with_low_confidence_threshold(0.0),
        );
        assert_eq!(
            expanded.spans[0].text.as_deref(),
            Some("v17 conformance audit urgent blocker")
        );
    }

    #[test]
    fn context_top_k_filters_low_confidence_before_selection() {
        let strict_options = EvalOptions::default().with_low_confidence_threshold(0.99);

        let filtered = evaluate_context(
            &ContextCommand::new("urgent").with_hits(1).with_budget(10),
            context_database(),
            strict_options.clone(),
        );
        assert!(filtered.hits.is_empty());

        let included = evaluate_context(
            &ContextCommand::new("urgent")
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
            context_hit_row("audit/v17.md", "intro", 0.9),
            context_hit_row("audit/v17.md", "details", 0.8),
            context_span_row("audit/v17.md", "intro"),
            context_neighbor_row("audit/v17.md", "audit/v17.md"),
            context_neighbor_row("audit/v17.md", "formal-model/v17.md"),
        ];

        let output = ContextOutput::from_rows("v17 conformance audit", &rows).expect("rows group");

        assert_eq!(output.goal, "v17 conformance audit");
        assert_eq!(output.hits.len(), 2);
        assert_eq!(output.hits[0].status.as_deref(), Some("current"));
        assert_eq!(output.hits[0].disposition, "unknown");
        assert_eq!(output.hits[0].age_days, None);
        assert_eq!(output.hits[0].topic_signal, "none");
        assert_eq!(output.hits[0].newer_topic_sibling_count, 0);
        assert_eq!(output.hits[0].top_newer_topic_sibling, None);
        assert_eq!(output.spans.len(), 1);
        assert_eq!(
            output.neighborhood,
            vec![
                ContextNeighbor {
                    handle: "audit/v17.md".to_string(),
                    neighbor: "audit/v17.md".to_string(),
                    status: Some("current".to_string()),
                    disposition: "current".to_string(),
                    age_days: None,
                    degree: 0,
                    group: "current".to_string(),
                },
                ContextNeighbor {
                    handle: "audit/v17.md".to_string(),
                    neighbor: "formal-model/v17.md".to_string(),
                    status: Some("current".to_string()),
                    disposition: "current".to_string(),
                    age_days: None,
                    degree: 0,
                    group: "current".to_string(),
                },
            ]
        );
    }

    #[test]
    fn context_output_dedupes_hits_and_orders_sections_by_hit_rank() {
        let rows = vec![
            context_hit_row("second.md", "body", 0.7),
            context_hit_row("first.md", "body", 0.9),
            context_hit_row("first.md", "details", 0.8),
            context_span_row("second.md", "body"),
            context_span_row("first.md", "details"),
            context_neighbor_row("second.md", "late.md"),
            context_neighbor_row("first.md", "early.md"),
        ];

        let output = ContextOutput::from_rows("ranked context", &rows).expect("rows group");

        assert_eq!(
            output
                .hits
                .iter()
                .map(|hit| (hit.handle.as_str(), hit.span_id.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("first.md", Some("body")),
                ("first.md", Some("details")),
                ("second.md", Some("body")),
            ]
        );
        assert_eq!(
            output
                .spans
                .iter()
                .map(|span| span.handle.as_str())
                .collect::<Vec<_>>(),
            vec!["first.md", "second.md"]
        );
        assert_eq!(
            output
                .neighborhood
                .iter()
                .map(|neighbor| neighbor.handle.as_str())
                .collect::<Vec<_>>(),
            vec!["first.md", "second.md"]
        );
    }

    #[test]
    fn context_output_prefers_matched_span_reads_over_same_handle_file_hits() {
        let mut file_hit = context_hit_row("guide.md", "guide.md#h/target", 0.8);
        file_hit
            .fields
            .insert("hit_span_id".to_string(), Value::Null);
        file_hit.fields.insert("field".to_string(), s("identifier"));
        file_hit
            .fields
            .insert("reason".to_string(), s("identifier-substring"));
        file_hit.fields.insert("summary".to_string(), Value::Null);

        let rows = vec![
            file_hit,
            context_hit_row("guide.md", "guide.md#h/target", 0.7),
            context_span_row("guide.md", "guide.md#full"),
            context_span_row("guide.md", "guide.md#h/target"),
        ];

        let output = ContextOutput::from_rows("target", &rows).expect("rows group");

        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].span_id.as_deref(), Some("guide.md#h/target"));
        assert_eq!(
            output
                .spans
                .iter()
                .map(|span| span.span_id.as_str())
                .collect::<Vec<_>>(),
            vec!["guide.md#h/target"]
        );
    }

    #[test]
    fn context_output_rejects_unknown_group_section() {
        let row = Row {
            fields: BTreeMap::from([("section".to_string(), s("mystery"))]),
            derivation: None,
        };
        let err =
            ContextOutput::from_rows("v17 conformance audit", &[row]).expect_err("bad section");

        assert_eq!(
            err,
            ContextGroupError::UnknownSection {
                section: "mystery".to_string(),
            }
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
    fn context_sample_v17_fixture_gate() {
        const AUDIT_HANDLE: &str = "reviews/2026-04-28-formal-model-v17-conformance-audit.md";

        let mut tool_calls = 0;
        let output = {
            tool_calls += 1;
            evaluate_context(
                &ContextCommand::new("v17 conformance audit")
                    .with_hits(3)
                    .with_budget(4_000),
                frozen_sample_database(),
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
            output.spans.iter().any(|span| span.handle == AUDIT_HANDLE),
            "context should include audit span metadata without forcing body reads: {:?}",
            output.spans
        );
        assert!(
            output.spans.iter().all(|span| span.text.is_none()),
            "default context should not inline span bodies: {:?}",
            output.spans
        );
    }

    #[test]
    fn context_neighbors_group_inventory_history_and_high_degree_current() {
        let output = evaluate_context(
            &ContextCommand::new("parametric perf")
                .with_hits(1)
                .with_budget(40),
            ranked_neighbor_database(),
            EvalOptions::default(),
        );

        let grouped = output
            .neighborhood
            .iter()
            .map(|neighbor| {
                (
                    neighbor.neighbor.as_str(),
                    neighbor.disposition.as_str(),
                    neighbor.group.as_str(),
                    neighbor.degree,
                )
            })
            .collect::<Vec<_>>();

        assert!(
            grouped.contains(&("docs/current.md", "current_head", "current", 3)),
            "self current head should stay visible: {grouped:?}"
        );
        assert!(
            grouped.contains(&("docs/v17.md", "current_head", "current", 32)),
            "high-degree current anchor should stay visible: {grouped:?}"
        );
        assert!(
            grouped.contains(&("LABELS.md", "current", "hidden", 32)),
            "inventory hub should be hidden/collapsed, not promoted: {grouped:?}"
        );
        assert!(
            grouped.contains(&("docs/old.md", "superseded", "superseded", 2)),
            "superseded neighbor should remain reachable in history group: {grouped:?}"
        );
    }

    #[test]
    fn context_prefers_canonical_parent_when_child_hits_cluster() {
        let output = evaluate_context(
            &ContextCommand::new("milestone chain")
                .with_hits(1)
                .with_budget(20)
                .include_low_confidence(true),
            clustered_label_database(),
            EvalOptions::default().with_low_confidence_threshold(0.0),
        );

        assert_eq!(output.hits.len(), 1);
        assert_eq!(
            output.hits[0].handle, "docs/canonical.md",
            "hits: {:?}",
            output.hits
        );
        assert_eq!(output.hits[0].reason, anneal_core::REASON_PARENT_CLUSTER);
        assert_eq!(output.spans[0].handle, "docs/canonical.md");
    }

    #[test]
    fn context_prefers_heading_span_over_short_label_title_match() {
        let output = evaluate_context(
            &ContextCommand::new("load shedding")
                .with_hits(1)
                .with_budget(40),
            heading_vs_label_database(),
            EvalOptions::default(),
        );

        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].handle, "docs/canonical.md");
        assert_eq!(
            output.hits[0].span_id.as_deref(),
            Some("docs/canonical.md#h/error-model-and-load-shedding")
        );
        assert_eq!(output.hits[0].field, "heading");
        assert!(
            output.hits[0].score > 0.95,
            "heading score should stay near the saturated title hits: {:?}",
            output.hits
        );
    }

    #[test]
    fn context_weights_rare_specific_terms_in_verbose_goals() {
        let output = evaluate_context(
            &ContextCommand::new("graceful overrun load shedding audio degradation")
                .with_hits(1)
                .with_budget(40),
            verbose_goal_specificity_database(),
            EvalOptions::default(),
        );

        assert_eq!(output.hits.len(), 1);
        assert_eq!(output.hits[0].handle, "docs/canonical.md");
        assert_eq!(
            output.hits[0].span_id.as_deref(),
            Some("docs/canonical.md#h/error-model-and-load-shedding")
        );
        assert!(
            output.hits[0].score > 0.5,
            "specific canonical heading should remain above low-confidence cutoff: {:?}",
            output.hits
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
        let mut program = standard_prelude_program().expect("prelude parses");
        program.statements.extend(
            parse_program(source_name, input)
                .expect("program parses")
                .statements,
        );
        let analyzed = analyze(program).expect("context analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect("query evaluates").rows
    }

    fn analyze_context_query(query: &str) {
        let mut program = standard_prelude_program().expect("prelude parses");
        program.statements.extend(
            parse_program("context", query)
                .expect("query parses")
                .statements,
        );
        analyze(program).expect("query analyzes");
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

    fn clustered_label_database() -> Database {
        let mut batch = FactBatch::new(
            "test".into(),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("docs/canonical.md", "Canonical source"),
            handle_in_file("MCD-1", "docs/canonical.md", "milestone chain alpha"),
            handle_in_file("MCD-2", "docs/canonical.md", "milestone chain beta"),
        ];
        batch.content = vec![
            content(
                "docs/canonical.md",
                "body",
                "Authoritative milestone source with enough surrounding text to exceed the tiny context budget.",
                100,
            ),
            content("MCD-1", "body", "milestone chain alpha", 3),
            content("MCD-2", "body", "milestone chain beta", 3),
        ];
        batch.spans = vec![
            span("docs/canonical.md", "body", 1, 3),
            span("MCD-1", "body", 4, 4),
            span("MCD-2", "body", 5, 5),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("merge clustered label fixture");
        Database::from_store(&store)
    }

    fn heading_vs_label_database() -> Database {
        let mut batch = FactBatch::new(
            "test".into(),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        let span_id = "docs/canonical.md#h/error-model-and-load-shedding";
        batch.handles = vec![
            handle("docs/canonical.md", "Canonical source"),
            handle_in_file("C-12", "docs/canonical.md", "Load shedding"),
        ];
        batch.content = vec![
            content(
                "docs/canonical.md",
                span_id,
                "Load shedding policy and error model details.",
                8,
            ),
            content("C-12", "body", "Load shedding", 2),
        ];
        batch.spans = vec![
            span_with_summary(
                "docs/canonical.md",
                span_id,
                31,
                38,
                "Error Model and Load Shedding",
            ),
            span("C-12", "body", 40, 40),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("merge heading vs label fixture");
        Database::from_store(&store)
    }

    fn verbose_goal_specificity_database() -> Database {
        let mut batch = FactBatch::new(
            "test".into(),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        let span_id = "docs/canonical.md#h/error-model-and-load-shedding";
        batch.handles = vec![
            handle("docs/canonical.md", "Canonical source"),
            handle("docs/protocol.md", "Protocol"),
            handle("docs/strategy.md", "Strategy"),
            handle_in_file("C-12", "docs/canonical.md", "Load shedding"),
        ];
        batch.content = vec![
            content(
                "docs/canonical.md",
                span_id,
                "Graceful overrun load shedding keeps audio stable.",
                8,
            ),
            content(
                "docs/protocol.md",
                "body",
                "Graceful overrun audio degradation protocol details.",
                8,
            ),
            content(
                "docs/strategy.md",
                "body",
                "Graceful overrun audio degradation strategy details.",
                8,
            ),
            content("C-12", "body", "Load shedding", 2),
        ];
        batch.spans = vec![
            span_with_summary(
                "docs/canonical.md",
                span_id,
                31,
                38,
                "Error Model and Load Shedding",
            ),
            span("docs/protocol.md", "body", 1, 4),
            span("docs/strategy.md", "body", 5, 8),
            span("C-12", "body", 40, 40),
        ];
        let mut store = FactStore::default();
        store
            .merge(batch)
            .expect("merge verbose specificity fixture");
        Database::from_store(&store)
    }

    fn ranked_neighbor_database() -> Database {
        let mut batch = FactBatch::new(
            "test".into(),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("docs/current.md", "parametric perf current"),
            handle("docs/v17.md", "V17 current anchor"),
            handle("docs/old.md", "Old superseded spec"),
            handle("LABELS.md", "Label inventory"),
        ];
        batch.content = vec![content(
            "docs/current.md",
            "body",
            "parametric perf current anchor",
            4,
        )];
        batch.spans = vec![span("docs/current.md", "body", 1, 1)];
        batch.edges = vec![
            edge("docs/old.md", "docs/current.md", "Supersedes"),
            edge("docs/old.md", "docs/v17.md", "Supersedes"),
            edge("docs/current.md", "docs/v17.md", "Cites"),
            edge("docs/current.md", "LABELS.md", "Cites"),
            edge("docs/current.md", "docs/old.md", "Cites"),
        ];
        for index in 0..32 {
            let leaf = format!("docs/leaf-{index}.md");
            batch.handles.push(handle(&leaf, "Leaf"));
            batch.edges.push(edge("docs/v17.md", &leaf, "Cites"));
            batch.edges.push(edge("LABELS.md", &leaf, "Cites"));
        }
        let mut store = FactStore::default();
        store.merge(batch).expect("merge ranked neighbor fixture");
        Database::from_store(&store)
    }

    fn frozen_sample_database() -> Database {
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
        let source = anneal_md::MarkdownSource::default();
        let request = SourceRefreshRequest::new("sample", &roots, &config)
            .with_actor(actor)
            .with_cancellation(CancellationToken::new());
        let driver = OneShotSourceDriver::new(source);
        let mut store = FactStore::default();
        refresh_source(&driver, &request, &mut store).expect("refresh frozen sample corpus");
        Database::from_store(&store).with_sources([driver.describe()])
    }

    fn context_hit_row(handle: &str, span_id: &str, score: f64) -> Row {
        Row {
            fields: BTreeMap::from([
                ("section".to_string(), s("hit")),
                ("h".to_string(), s(handle)),
                ("hit_span_id".to_string(), s(span_id)),
                ("score".to_string(), f(score)),
                ("reason".to_string(), s("body-substring")),
                ("field".to_string(), s("body")),
                ("summary".to_string(), s("Intro")),
                ("status".to_string(), s("current")),
                ("disposition".to_string(), s("unknown")),
                ("age_days".to_string(), Value::Null),
                ("topic_signal".to_string(), s("none")),
                ("newer_topic_sibling_count".to_string(), n(0)),
                ("top_newer_topic_sibling".to_string(), Value::Null),
            ]),
            derivation: None,
        }
    }

    fn context_span_row(handle: &str, span_id: &str) -> Row {
        Row {
            fields: BTreeMap::from([
                ("section".to_string(), s("span")),
                ("h".to_string(), s(handle)),
                ("span_id".to_string(), s(span_id)),
                ("text".to_string(), s("body text")),
                ("start_line".to_string(), n(1)),
                ("end_line".to_string(), n(2)),
                ("tokens".to_string(), n(4)),
            ]),
            derivation: None,
        }
    }

    fn context_neighbor_row(handle: &str, neighbor: &str) -> Row {
        Row {
            fields: BTreeMap::from([
                ("section".to_string(), s("neighbor")),
                ("h".to_string(), s(handle)),
                ("neighbor".to_string(), s(neighbor)),
                ("neighbor_status".to_string(), s("current")),
                ("neighbor_disposition".to_string(), s("current")),
                ("neighbor_age_days".to_string(), Value::Null),
                ("neighbor_degree".to_string(), n(0)),
                ("neighbor_group".to_string(), s("current")),
            ]),
            derivation: None,
        }
    }

    fn handle(id: &str, summary: &str) -> HandleFact {
        handle_in_file(id, id, summary)
    }

    fn handle_in_file(id: &str, file: &str, summary: &str) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: if id == file { "file" } else { "label" }.to_string(),
            status: Some("current".to_string()),
            namespace: String::new(),
            file: file.to_string(),
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
        span_with_summary(handle, span_id, start_line, end_line, "Intro")
    }

    fn span_with_summary(
        handle: &str,
        span_id: &str,
        start_line: u32,
        end_line: u32,
        summary: &str,
    ) -> SpanFact {
        SpanFact {
            identity: identity(&format!("{handle}#{span_id}")),
            id: span_id.to_string(),
            handle: handle.to_string(),
            start_line,
            end_line,
            summary: summary.to_string(),
        }
    }

    fn edge(from: &str, to: &str, kind: &str) -> EdgeFact {
        EdgeFact {
            identity: identity(&format!("{from}->{to}:{kind}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: "fixture.md".to_string(),
            line: 1,
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
