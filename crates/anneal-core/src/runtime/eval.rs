use std::cmp::Ordering;
use std::collections::btree_set;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io;
use std::slice;
use std::sync::Arc;

use regex::Regex;
use serde::Serialize;

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactIdentity, HandleFact, MetaFact,
    SnapshotFact, SpanFact,
};
use crate::ids::Generation;
use crate::ranking::{DefaultRanker, Ranker, RankingContext, SearchHit};
use crate::runtime::analysis::{AnalyzedProgram, AnalyzedQuery};
use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, ComparisonOp, Expr,
    FieldPattern, Head, Ident, Literal, NegatedAtom, NumberLiteral, PredicateRef, Rule, StoredAtom,
    Term,
};
use crate::runtime::primitives::PrimitivePredicate;
use crate::source::{ActorContext, RuntimeCapability};
use crate::store::FactStore;
use crate::time::{
    current_days_since_epoch, iso_days_since_epoch, relative_days_reference,
    snapshot_days_since_epoch,
};

pub type Binding = BTreeMap<Ident, Value>;
type DeltaMap = BTreeMap<PredicateRef, DerivedRelation>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Tuple(pub Vec<Value>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueryOutput {
    pub rows: Vec<Row>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<QueryWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueryWarning {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
}

pub const READ_FULL_CAPABILITY: RuntimeCapability = RuntimeCapability::ReadFull;
const DEFAULT_READ_FULL_TOKEN_LIMIT: i64 = 8_000;
const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f32 = 0.5;

#[derive(Clone)]
pub struct EvalOptions {
    actor: ActorContext,
    read_full_token_limit: i64,
    low_confidence_threshold: f32,
    ranker: Arc<dyn Ranker>,
}

impl EvalOptions {
    pub fn with_actor(mut self, actor: ActorContext) -> Self {
        self.actor = actor;
        self
    }

    pub fn with_capability(mut self, capability: RuntimeCapability) -> Self {
        self.actor = self.actor.with_runtime_capability(capability);
        self
    }

    pub fn with_read_full_token_limit(mut self, limit: i64) -> Self {
        self.read_full_token_limit = limit.max(0);
        self
    }

    pub fn with_low_confidence_threshold(mut self, threshold: f32) -> Self {
        self.low_confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn with_ranker(mut self, ranker: impl Ranker + 'static) -> Self {
        self.ranker = Arc::new(ranker);
        self
    }

    fn has_capability(&self, capability: RuntimeCapability) -> bool {
        self.actor.has_runtime_capability(capability)
    }

    fn ranking_context(&self, query: &str) -> RankingContext {
        RankingContext::new(query, self.low_confidence_threshold)
    }

    fn ranker(&self) -> &dyn Ranker {
        self.ranker.as_ref()
    }
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            actor: ActorContext::anonymous_cli(),
            read_full_token_limit: DEFAULT_READ_FULL_TOKEN_LIMIT,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            ranker: Arc::new(DefaultRanker),
        }
    }
}

impl fmt::Debug for EvalOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvalOptions")
            .field("actor", &self.actor)
            .field("read_full_token_limit", &self.read_full_token_limit)
            .field("low_confidence_threshold", &self.low_confidence_threshold)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum Value {
    String(String),
    Number(NumberValue),
    Bool(bool),
    Null,
    List(Vec<Value>),
}

impl Value {
    fn kind_rank(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Number(_) => 2,
            Self::String(_) => 3,
            Self::List(_) => 4,
        }
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::List(a), Self::List(b)) => a.cmp(b),
            _ => self.kind_rank().cmp(&other.kind_rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NumberValue {
    Int(i64),
    Float(f64),
}

impl Eq for NumberValue {}

impl Ord for NumberValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.total_cmp(b),
            (Self::Int(_), Self::Float(_)) => Ordering::Less,
            (Self::Float(_), Self::Int(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NumberValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for NumberValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Int(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Self::Float(value) => {
                1_u8.hash(state);
                value.to_bits().hash(state);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Database {
    stored: BTreeMap<Ident, StoredRelation>,
    derived: BTreeMap<PredicateRef, DerivedRelation>,
    graph: Arc<GraphIndex>,
    content: Arc<ContentIndex>,
    search: Arc<SearchIndex>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database")
            .field(
                "stored",
                &self
                    .stored
                    .iter()
                    .map(|(relation, rows)| (relation.to_string(), rows.len()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .field(
                "derived",
                &self
                    .derived
                    .iter()
                    .map(|(predicate, tuples)| (predicate.display_name(), tuples.len()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .field("content_spans", &self.content.len())
            .field("search_documents", &self.search.len())
            .finish_non_exhaustive()
    }
}

impl Database {
    pub fn from_store(store: &FactStore) -> Self {
        let mut db = Self::default();
        db.insert_named_rows("handle", store.handles().iter().map(handle_row));
        db.insert_named_rows("edge", store.edges().iter().map(edge_row));
        db.insert_named_rows("meta", store.meta().iter().map(meta_row));
        db.insert_named_rows("content", store.content().iter().map(content_row));
        db.insert_named_rows("span", store.spans().iter().map(span_row));
        db.insert_named_rows("concern", store.concerns().iter().map(concern_row));
        db.insert_named_rows("config", store.configs().iter().map(config_row));
        db.insert_named_rows("snapshot", store.snapshots().iter().map(snapshot_row));
        db.insert_named_rows(
            "generation",
            store.generations().iter().map(|row| {
                named_row([
                    ("corpus", Value::String(row.corpus.to_string())),
                    ("source", Value::String(row.source.to_string())),
                    ("current", generation_value(row.current)),
                ])
            }),
        );
        db
    }

    pub fn insert_stored_rows(
        &mut self,
        relation: impl Into<String>,
        rows: impl IntoIterator<Item = NamedRow>,
    ) {
        self.insert_named_rows(&relation.into(), rows);
    }

    pub fn derived(&self, predicate: &PredicateRef) -> Option<&BTreeSet<Tuple>> {
        self.derived.get(predicate).map(DerivedRelation::tuples)
    }

    fn search_tuples(&self, constraints: &[(usize, Value)], options: &EvalOptions) -> Vec<Tuple> {
        let ArgConstraint::Exact(query_text) = string_constraint(constraints, 0) else {
            return Vec::new();
        };
        let Some(query) = SearchQuery::parse(query_text) else {
            return Vec::new();
        };
        let handle = string_constraint(constraints, 1);
        let ctx = options.ranking_context(query_text);
        let ranker = options.ranker();
        let mut ranked = self
            .search
            .search_hits(&query, handle)
            .into_iter()
            .chain(self.content.search_hits(&query, handle))
            .map(|hit| rank_search_hit(hit, &ctx, ranker))
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| compare_ranked_search_hits(left, right, ranker));

        let mut seen = BTreeSet::new();
        ranked
            .into_iter()
            .map(|hit| hit.tuple(query_text))
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .filter(|tuple| seen.insert(tuple.clone()))
            .collect()
    }

    fn ensure_derived(&mut self, predicates: impl IntoIterator<Item = PredicateRef>) {
        for predicate in predicates {
            self.derived.entry(predicate).or_default();
        }
    }

    fn insert_named_rows(&mut self, relation: &str, rows: impl IntoIterator<Item = NamedRow>) {
        let relation = Ident::new_unchecked(relation);
        let stored = self
            .stored
            .entry(relation.clone())
            .or_insert_with(|| StoredRelation::new(relation.clone()));
        for row in rows {
            Arc::make_mut(&mut self.graph).insert_row(&relation, &row);
            Arc::make_mut(&mut self.content).insert_row(&relation, &row);
            Arc::make_mut(&mut self.search).insert_row(&relation, &row);
            stored.push(row);
        }
    }

    fn set_stored_relation_rows(
        &mut self,
        relation: &str,
        rows: impl IntoIterator<Item = NamedRow>,
    ) {
        let relation = Ident::new_unchecked(relation);
        let mut stored = StoredRelation::new(relation.clone());
        for row in rows {
            stored.push(row);
        }
        self.stored.insert(relation, stored);
    }

    fn rebuild_graph(&mut self) {
        let mut graph = GraphIndex::default();
        for (relation, stored) in &self.stored {
            for row in &stored.rows {
                graph.insert_row(relation, row);
            }
        }
        self.graph = Arc::new(graph);
    }

    fn scoped_to_time_ref(&self, reference: &str) -> Result<(Self, Vec<QueryWarning>), EvalError> {
        let Some(selection) = self.resolve_snapshot_selection(reference) else {
            return Err(EvalError::UnsupportedTimeRef {
                reference: reference.to_string(),
            });
        };

        let mut scoped = self.clone_for_time_scope();
        scoped.set_stored_relation_rows(SNAPSHOT_RELATION, selection.rows.clone());
        scoped.apply_handle_snapshot(&selection.rows);
        scoped.rebuild_graph();

        Ok((
            scoped,
            self.snapshot_partial_history_warnings(reference, &selection.snapshot),
        ))
    }

    fn resolve_snapshot_selection(&self, reference: &str) -> Option<SnapshotSelection> {
        let relation = self.stored.get(&Ident::new_unchecked(SNAPSHOT_RELATION))?;
        let candidates = snapshot_candidates(&relation.rows);
        match snapshot_reference(reference)? {
            SnapshotReference::Last => latest_snapshot_candidate(candidates.into_values()),
            SnapshotReference::Snapshot(id) => candidates.get(&id).cloned().map(Into::into),
            SnapshotReference::Day(target_day) => {
                nearest_snapshot_candidate(candidates.into_values(), target_day)
            }
        }
    }

    fn clone_for_time_scope(&self) -> Self {
        Self {
            stored: self.stored.clone(),
            derived: BTreeMap::new(),
            graph: Arc::clone(&self.graph),
            content: Arc::clone(&self.content),
            search: Arc::clone(&self.search),
        }
    }

    fn apply_handle_snapshot(&mut self, snapshot_rows: &[NamedRow]) {
        let Some(handles) = self.stored.get(&Ident::new_unchecked(HANDLE_RELATION)) else {
            return;
        };
        let patches = handle_snapshot_patches(snapshot_rows);
        if patches.is_empty() {
            return;
        }
        let rows = handles
            .rows
            .iter()
            .map(|row| apply_handle_snapshot_patch(row, &patches))
            .collect::<Vec<_>>();
        self.set_stored_relation_rows(HANDLE_RELATION, rows);
    }

    fn snapshot_partial_history_warnings(
        &self,
        reference: &str,
        snapshot: &str,
    ) -> Vec<QueryWarning> {
        let sources = self
            .stored
            .get(&Ident::new_unchecked(HANDLE_RELATION))
            .into_iter()
            .flat_map(|relation| relation.rows.iter())
            .filter_map(|row| row_string(row, SOURCE_FIELD).map(str::to_string))
            .collect::<BTreeSet<_>>();
        if sources.is_empty() {
            return vec![snapshot_partial_history_warning(reference, snapshot, None)];
        }
        sources
            .into_iter()
            .map(|source| snapshot_partial_history_warning(reference, snapshot, Some(source)))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotSelection {
    snapshot: String,
    day: i64,
    rows: Vec<NamedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotCandidate {
    snapshot: String,
    day: i64,
    sort_at: String,
    rows: Vec<NamedRow>,
}

enum SnapshotReference {
    Last,
    Snapshot(String),
    Day(i64),
}

fn snapshot_reference(reference: &str) -> Option<SnapshotReference> {
    if reference == "snapshot:last" {
        return Some(SnapshotReference::Last);
    }
    if let Some(snapshot) = reference.strip_prefix("snapshot:") {
        return (!snapshot.is_empty()).then(|| SnapshotReference::Snapshot(snapshot.to_string()));
    }
    if let Some(day) = snapshot_days_since_epoch(reference) {
        return Some(SnapshotReference::Day(day));
    }
    relative_days_reference(reference).map(SnapshotReference::Day)
}

fn snapshot_candidates(rows: &[NamedRow]) -> BTreeMap<String, SnapshotCandidate> {
    let mut candidates = BTreeMap::<String, SnapshotCandidate>::new();
    for row in rows {
        let Some(at) = row_string(row, AT_FIELD) else {
            continue;
        };
        let Some(day) = snapshot_days_since_epoch(at) else {
            continue;
        };
        let snapshot = row_string(row, SNAPSHOT_FIELD).unwrap_or(at).to_string();
        candidates
            .entry(snapshot.clone())
            .or_insert_with(|| SnapshotCandidate {
                snapshot,
                day,
                sort_at: at.to_string(),
                rows: Vec::new(),
            })
            .rows
            .push(row.clone());
    }
    candidates
}

fn latest_snapshot_candidate(
    candidates: impl Iterator<Item = SnapshotCandidate>,
) -> Option<SnapshotSelection> {
    candidates
        .max_by(|left, right| {
            left.day
                .cmp(&right.day)
                .then_with(|| left.sort_at.cmp(&right.sort_at))
                .then_with(|| left.snapshot.cmp(&right.snapshot))
        })
        .map(SnapshotSelection::from)
}

fn nearest_snapshot_candidate(
    candidates: impl Iterator<Item = SnapshotCandidate>,
    target_day: i64,
) -> Option<SnapshotSelection> {
    candidates
        .min_by(|left, right| {
            let left_distance = left.day.abs_diff(target_day);
            let right_distance = right.day.abs_diff(target_day);
            left_distance
                .cmp(&right_distance)
                .then_with(|| right.day.cmp(&left.day))
                .then_with(|| right.sort_at.cmp(&left.sort_at))
                .then_with(|| right.snapshot.cmp(&left.snapshot))
        })
        .map(SnapshotSelection::from)
}

impl From<SnapshotCandidate> for SnapshotSelection {
    fn from(candidate: SnapshotCandidate) -> Self {
        Self {
            snapshot: candidate.snapshot,
            day: candidate.day,
            rows: candidate.rows,
        }
    }
}

fn handle_snapshot_patches(
    snapshot_rows: &[NamedRow],
) -> BTreeMap<(String, String), Vec<(String, String)>> {
    let mut patches = BTreeMap::<(String, String), Vec<(String, String)>>::new();
    for row in snapshot_rows {
        let (Some(corpus), Some(id), Some(key), Some(value)) = (
            row_string(row, CORPUS_FIELD),
            row_string(row, ID_FIELD),
            row_string(row, KEY_FIELD),
            row_string(row, VALUE_FIELD),
        ) else {
            continue;
        };
        patches
            .entry((corpus.to_string(), id.to_string()))
            .or_default()
            .push((key.to_string(), value.to_string()));
    }
    patches
}

fn apply_handle_snapshot_patch(
    row: &NamedRow,
    patches: &BTreeMap<(String, String), Vec<(String, String)>>,
) -> NamedRow {
    let Some(corpus) = row_string(row, CORPUS_FIELD) else {
        return row.clone();
    };
    let Some(id) = row_string(row, ID_FIELD) else {
        return row.clone();
    };
    let Some(values) = patches.get(&(corpus.to_string(), id.to_string())) else {
        return row.clone();
    };

    let mut row = row.clone();
    for (key, value) in values {
        if let Ok(field) = Ident::new(key.clone()) {
            row.insert(field, Value::String(value.clone()));
        }
    }
    row
}

fn push_warnings(out: &mut Vec<QueryWarning>, warnings: Vec<QueryWarning>) {
    for warning in warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
}

fn snapshot_partial_history_warning(
    reference: &str,
    snapshot: &str,
    source: Option<String>,
) -> QueryWarning {
    let source_clause = source
        .as_deref()
        .map_or_else(String::new, |source| format!(" for source {source}"));
    QueryWarning {
        code: "partial_history".to_string(),
        message: format!(
            "at({reference:?}) used snapshot fallback {snapshot}{source_clause}; only snapshot-backed handle fields are historical"
        ),
        reference: Some(reference.to_string()),
        source,
        relation: Some(HANDLE_RELATION.to_string()),
    }
}

pub type NamedRow = BTreeMap<Ident, Value>;

#[derive(Clone, Debug)]
struct StoredRelation {
    relation: Ident,
    rows: Vec<NamedRow>,
    indexes: BTreeMap<Ident, BTreeMap<Value, Vec<usize>>>,
}

impl StoredRelation {
    fn new(relation: Ident) -> Self {
        Self {
            relation,
            rows: Vec::new(),
            indexes: BTreeMap::new(),
        }
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    fn push(&mut self, row: NamedRow) {
        let idx = self.rows.len();
        for (field, value) in &row {
            if !should_index_stored_field(&self.relation, field) {
                continue;
            }
            self.indexes
                .entry(field.clone())
                .or_default()
                .entry(value.clone())
                .or_default()
                .push(idx);
        }
        self.rows.push(row);
    }

    fn candidate_rows(&self, constraints: &[(Ident, Value)]) -> RowCandidates<'_> {
        let mut best = None;
        for (field, value) in constraints {
            if !should_index_stored_field(&self.relation, field) {
                continue;
            }
            let Some(values) = self.indexes.get(field) else {
                return RowCandidates::Empty;
            };
            let Some(indices) = values.get(value) else {
                return RowCandidates::Empty;
            };
            if best.is_none_or(|current: &Vec<usize>| indices.len() < current.len()) {
                best = Some(indices);
            }
        }

        best.map_or_else(
            || RowCandidates::All(self.rows.iter()),
            |indices| RowCandidates::Indexed {
                rows: &self.rows,
                indices: indices.iter(),
            },
        )
    }
}

enum RowCandidates<'a> {
    All(slice::Iter<'a, NamedRow>),
    Indexed {
        rows: &'a [NamedRow],
        indices: slice::Iter<'a, usize>,
    },
    Empty,
}

impl<'a> Iterator for RowCandidates<'a> {
    type Item = &'a NamedRow;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(rows) => rows.next(),
            Self::Indexed { rows, indices } => indices.next().map(|idx| &rows[*idx]),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ContentIndex {
    content: BTreeMap<ContentKey, ContentPayload>,
    spans: BTreeMap<ContentKey, SpanPayload>,
    span_order_by_handle: BTreeMap<String, BTreeSet<OrderedSpanKey>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContentKey {
    handle: String,
    span_id: String,
}

impl ContentKey {
    fn new(handle: &str, span_id: &str) -> Self {
        Self {
            handle: handle.to_owned(),
            span_id: span_id.to_owned(),
        }
    }
}

impl Ord for ContentKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.handle
            .cmp(&other.handle)
            .then_with(|| self.span_id.cmp(&other.span_id))
    }
}

impl PartialOrd for ContentKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContentPayload {
    source: String,
    text: String,
    tokens: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpanPayload {
    start_line: i64,
    end_line: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OrderedSpanKey {
    start_line: i64,
    span_id: String,
}

impl OrderedSpanKey {
    fn new(span_id: &str, start_line: i64) -> Self {
        Self {
            start_line,
            span_id: span_id.to_owned(),
        }
    }
}

impl Ord for OrderedSpanKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.start_line
            .cmp(&other.start_line)
            .then_with(|| self.span_id.cmp(&other.span_id))
    }
}

impl PartialOrd for OrderedSpanKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug)]
struct ContentSpan<'a> {
    key: &'a ContentKey,
    content: &'a ContentPayload,
    span: &'a SpanPayload,
}

impl ContentIndex {
    fn len(&self) -> usize {
        self.content.len()
    }

    fn insert_row(&mut self, relation: &Ident, row: &NamedRow) {
        match relation.as_str() {
            CONTENT_RELATION => self.insert_content(row),
            SPAN_RELATION => self.insert_span(row),
            _ => {}
        }
    }

    fn insert_content(&mut self, row: &NamedRow) {
        let (Some(handle), Some(span_id), Some(text), Some(tokens)) = (
            row_string(row, HANDLE_FIELD),
            row_string(row, SPAN_ID_FIELD),
            row_string(row, TEXT_FIELD),
            row_i64(row, TOKENS_FIELD),
        ) else {
            return;
        };
        let key = ContentKey::new(handle, span_id);
        let payload = ContentPayload {
            source: row_string(row, SOURCE_FIELD).unwrap_or_default().to_owned(),
            text: text.to_owned(),
            tokens,
        };
        self.content.insert(key, payload);
    }

    fn insert_span(&mut self, row: &NamedRow) {
        let (Some(handle), Some(span_id), Some(start_line), Some(end_line)) = (
            row_string(row, HANDLE_FIELD),
            row_string(row, ID_FIELD),
            row_i64(row, START_LINE_FIELD),
            row_i64(row, END_LINE_FIELD),
        ) else {
            return;
        };
        let key = ContentKey::new(handle, span_id);
        let payload = SpanPayload {
            start_line,
            end_line,
        };
        self.span_order_by_handle
            .entry(handle.to_owned())
            .or_default()
            .insert(OrderedSpanKey::new(span_id, start_line));
        self.spans.insert(key, payload);
    }

    fn read_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 0) else {
            return Vec::new();
        };
        let ArgConstraint::Exact(budget) = i64_constraint(constraints, 1) else {
            return Vec::new();
        };
        if budget < 0 {
            return Vec::new();
        }
        let span_id = string_constraint(constraints, 2);
        if let ArgConstraint::Exact(span_id) = span_id {
            return self
                .content_span(&ContentKey::new(handle, span_id))
                .filter(|span| span.content.tokens <= budget)
                .map(|span| read_tuple(span, budget))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .into_iter()
                .collect();
        }
        let mut used = 0_i64;
        let mut out = Vec::new();
        for span in self.content_spans_for_handle(handle) {
            let next = used.saturating_add(span.content.tokens);
            if next > budget {
                break;
            }
            used = next;
            let tuple = read_tuple(span, budget);
            if tuple_matches_constraints(&tuple, constraints) {
                out.push(tuple);
            }
        }
        out
    }

    fn read_full_tuples(
        &self,
        constraints: &[(usize, Value)],
        token_limit: i64,
    ) -> Result<Vec<Tuple>, EvalError> {
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 0) else {
            return Ok(Vec::new());
        };
        let Some(content) = self.full_content_under_limit(handle, token_limit)? else {
            return Ok(Vec::new());
        };
        let tuple = Tuple(vec![string_value(handle), Value::String(content)]);
        Ok(tuple_matches_constraints(&tuple, constraints)
            .then_some(tuple)
            .into_iter()
            .collect())
    }

    fn match_tuples(&self, constraints: &[(usize, Value)], regex: &Regex) -> Vec<Tuple> {
        let ArgConstraint::Exact(pattern) = string_constraint(constraints, 0) else {
            return Vec::new();
        };
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 1) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for span in self.content_spans_for_handle(handle) {
            for (line_offset, line) in span.content.text.lines().enumerate() {
                if !regex.is_match(line) {
                    continue;
                }
                let line_offset = i64::try_from(line_offset).unwrap_or(i64::MAX);
                let tuple = Tuple(vec![
                    string_value(pattern),
                    string_value(&span.key.handle),
                    int_value(span.span.start_line.saturating_add(line_offset)),
                    Value::String(line.to_owned()),
                ]);
                if tuple_matches_constraints(&tuple, constraints) {
                    out.push(tuple);
                }
            }
        }
        out
    }

    fn search_hits(&self, query: &SearchQuery, handle: ArgConstraint<&str>) -> Vec<SearchHit> {
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(handle) => self
                .content_spans_for_handle(handle)
                .filter_map(|span| body_search_hit(query, span))
                .collect(),
            ArgConstraint::Any => self
                .span_order_by_handle
                .keys()
                .flat_map(|handle| self.content_spans_for_handle(handle))
                .filter_map(|span| body_search_hit(query, span))
                .collect(),
        }
    }

    fn content_span(&self, key: &ContentKey) -> Option<ContentSpan<'_>> {
        let (key, content) = self.content.get_key_value(key)?;
        let span = self.spans.get(key)?;
        Some(ContentSpan { key, content, span })
    }

    fn content_spans_for_handle(&self, handle: &str) -> impl Iterator<Item = ContentSpan<'_>> {
        self.span_order_by_handle
            .get(handle)
            .into_iter()
            .flat_map(move |ordered_keys| {
                ordered_keys.iter().filter_map(move |ordered_key| {
                    self.content_span(&ContentKey::new(handle, &ordered_key.span_id))
                })
            })
    }

    fn full_content_under_limit(
        &self,
        handle: &str,
        token_limit: i64,
    ) -> Result<Option<String>, EvalError> {
        let mut tokens = 0_i64;
        let mut has_content = false;
        for span in self.content_spans_for_handle(handle) {
            has_content = true;
            tokens = tokens.saturating_add(span.content.tokens);
            if tokens > token_limit {
                return Err(EvalError::ReadFullBudgetExceeded {
                    handle: handle.to_owned(),
                    tokens,
                    limit: token_limit,
                });
            }
        }
        if !has_content {
            return Ok(None);
        }
        let mut content = String::new();
        for span in self.content_spans_for_handle(handle) {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&span.content.text);
        }
        Ok(Some(content))
    }
}

fn read_tuple(span: ContentSpan<'_>, budget: i64) -> Tuple {
    Tuple(vec![
        string_value(&span.key.handle),
        int_value(budget),
        string_value(&span.key.span_id),
        Value::String(span.content.text.clone()),
        int_value(span.span.start_line),
        int_value(span.span.end_line),
        int_value(span.content.tokens),
    ])
}

fn body_search_hit(query: &SearchQuery, span: ContentSpan<'_>) -> Option<SearchHit> {
    query.score(&span.content.text).map(|score| {
        SearchHit::new(
            span.content.source.as_str(),
            span.key.handle.as_str(),
            Some(span.key.span_id.clone()),
            score,
            SearchQuery::reason("body"),
            "body",
        )
    })
}

#[derive(Clone, Debug, Default)]
struct SearchIndex {
    documents: BTreeMap<String, SearchDocument>,
}

#[derive(Clone, Debug, Default)]
struct SearchDocument {
    handle: String,
    source: String,
    fields: Vec<SearchField>,
}

#[derive(Clone, Debug, PartialEq)]
struct SearchField {
    field: String,
    reason: String,
    text: String,
}

impl SearchField {
    fn new(field: impl Into<String>, reason: impl Into<String>, text: &str) -> Option<Self> {
        (!text.trim().is_empty()).then(|| Self {
            field: field.into(),
            reason: reason.into(),
            text: text.to_owned(),
        })
    }
}

impl SearchIndex {
    fn len(&self) -> usize {
        self.documents.len()
    }

    fn insert_row(&mut self, relation: &Ident, row: &NamedRow) {
        match relation.as_str() {
            HANDLE_RELATION => self.insert_handle(row),
            META_RELATION => self.insert_meta(row),
            _ => {}
        }
    }

    fn insert_handle(&mut self, row: &NamedRow) {
        let Some(handle) = row_string(row, ID_FIELD) else {
            return;
        };
        let source = row_string(row, SOURCE_FIELD).unwrap_or_default();
        let document = self.document_mut(handle, source);
        push_search_field(
            &mut document.fields,
            "identifier",
            "identifier-substring",
            handle,
        );
        if let Some(summary) = row_string(row, SUMMARY_FIELD) {
            push_search_field(&mut document.fields, "title", "title-substring", summary);
        }
        for (field, source_field) in [
            ("frontmatter:status", STATUS_FIELD),
            ("frontmatter:namespace", NAMESPACE_FIELD),
            ("frontmatter:area", AREA_FIELD),
            ("frontmatter:kind", KIND_FIELD),
        ] {
            if let Some(value) = row_string(row, source_field) {
                push_search_field(
                    &mut document.fields,
                    field,
                    "frontmatter-value-match",
                    value,
                );
            }
        }
    }

    fn insert_meta(&mut self, row: &NamedRow) {
        let (Some(handle), Some(key), Some(value)) = (
            row_string(row, HANDLE_FIELD),
            row_string(row, KEY_FIELD),
            row_string(row, VALUE_FIELD),
        ) else {
            return;
        };
        let source = row_string(row, SOURCE_FIELD).unwrap_or_default();
        let field = format!("frontmatter:{key}");
        let document = self.document_mut(handle, source);
        push_search_field(&mut document.fields, field, "frontmatter-key-match", key);
        push_search_field(
            &mut document.fields,
            format!("frontmatter:{key}"),
            "frontmatter-value-match",
            value,
        );
    }

    fn document_mut(&mut self, handle: &str, source: &str) -> &mut SearchDocument {
        let document = self
            .documents
            .entry(handle.to_owned())
            .or_insert_with(|| SearchDocument {
                handle: handle.to_owned(),
                source: source.to_owned(),
                fields: Vec::new(),
            });
        if document.source.is_empty() && !source.is_empty() {
            source.clone_into(&mut document.source);
        }
        document
    }

    fn search_hits(&self, query: &SearchQuery, handle: ArgConstraint<&str>) -> Vec<SearchHit> {
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(handle) => self
                .documents
                .get(handle)
                .map_or_else(Vec::new, |document| document.search_hits(query).collect()),
            ArgConstraint::Any => self
                .documents
                .values()
                .flat_map(|document| document.search_hits(query))
                .collect(),
        }
    }
}

impl SearchDocument {
    fn search_hits<'a>(&'a self, query: &'a SearchQuery) -> impl Iterator<Item = SearchHit> + 'a {
        self.fields.iter().filter_map(move |field| {
            query.score(&field.text).map(|score| {
                SearchHit::new(
                    self.source.as_str(),
                    self.handle.as_str(),
                    None,
                    score,
                    field.reason.clone(),
                    field.field.clone(),
                )
            })
        })
    }
}

fn push_search_field(
    fields: &mut Vec<SearchField>,
    field: impl Into<String>,
    reason: impl Into<String>,
    text: &str,
) {
    if let Some(field) = SearchField::new(field, reason, text) {
        fields.push(field);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchQuery {
    normalized: String,
    terms: Vec<String>,
}

impl SearchQuery {
    fn parse(query: &str) -> Option<Self> {
        let normalized = normalize_search_text(query);
        if normalized.is_empty() {
            return None;
        }
        let mut terms = normalized
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        terms.sort();
        terms.dedup();
        Some(Self { normalized, terms })
    }

    fn score(&self, text: &str) -> Option<f32> {
        let normalized = normalize_search_text(text);
        if normalized.is_empty() {
            return None;
        }
        if normalized.contains(&self.normalized) {
            return Some(1.0);
        }
        let matched = self
            .terms
            .iter()
            .filter(|term| normalized.contains(term.as_str()))
            .count();
        (matched > 0).then(|| {
            let matched = u16::try_from(matched).unwrap_or(u16::MAX);
            let total = u16::try_from(self.terms.len().max(1)).unwrap_or(u16::MAX);
            0.35 + (0.55 * (f32::from(matched) / f32::from(total)))
        })
    }

    fn reason(field: &str) -> &'static str {
        if field == "body" {
            "body-substring"
        } else {
            "substring"
        }
    }
}

fn normalize_search_text(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq)]
struct RankedSearchHit {
    hit: SearchHit,
    score: f32,
    low_confidence: bool,
}

impl RankedSearchHit {
    fn tuple(&self, query: &str) -> Tuple {
        Tuple(vec![
            string_value(query),
            string_value(&self.hit.handle),
            self.hit
                .span_id
                .as_deref()
                .map_or(Value::Null, string_value),
            float_value(f64::from(self.score)),
            string_value(&self.hit.reason),
            string_value(&self.hit.field),
            Value::Bool(self.low_confidence),
        ])
    }
}

fn rank_search_hit(hit: SearchHit, ctx: &RankingContext, ranker: &dyn Ranker) -> RankedSearchHit {
    let score = ranker.calibrate(&hit, ctx);
    let score = if score.is_finite() {
        score.clamp(0.0, 1.0)
    } else {
        0.0
    };
    RankedSearchHit {
        low_confidence: score < ctx.low_confidence_threshold,
        hit,
        score,
    }
}

fn compare_ranked_search_hits(
    left: &RankedSearchHit,
    right: &RankedSearchHit,
    ranker: &dyn Ranker,
) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| ranker.tie_break(&left.hit, &right.hit))
        .then_with(|| left.tuple("").cmp(&right.tuple("")))
}

#[derive(Clone, Debug, Default)]
struct GraphIndex {
    nodes: BTreeSet<String>,
    handles: BTreeMap<String, HandleState>,
    outgoing: BTreeMap<String, BTreeSet<String>>,
    incoming: BTreeMap<String, BTreeSet<String>>,
    out_edge_count: BTreeMap<String, usize>,
    in_edge_count: BTreeMap<String, usize>,
    cite_count: BTreeMap<String, usize>,
    discharge_count: BTreeMap<String, usize>,
    content_tokens: BTreeMap<String, usize>,
    active_statuses: BTreeSet<String>,
    terminal_statuses: BTreeSet<String>,
    settled_statuses: BTreeSet<String>,
    pipeline_positions: BTreeMap<String, i64>,
    linear_namespaces: BTreeSet<String>,
    status_snapshots: BTreeMap<String, Vec<SnapshotStatus>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HandleState {
    kind: String,
    status: Option<String>,
    namespace: String,
    date: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotStatus {
    day: i64,
    sort_at: String,
    status: String,
}

impl GraphIndex {
    fn insert_row(&mut self, relation: &Ident, row: &NamedRow) {
        match relation.as_str() {
            HANDLE_RELATION => {
                if let Some(id) = row_string(row, ID_FIELD) {
                    self.nodes.insert(id.to_owned());
                    self.handles.insert(
                        id.to_owned(),
                        HandleState {
                            kind: row_string(row, KIND_FIELD).unwrap_or_default().to_owned(),
                            status: row_string(row, STATUS_FIELD).map(str::to_owned),
                            namespace: row_string(row, NAMESPACE_FIELD)
                                .unwrap_or_default()
                                .to_owned(),
                            date: row_string(row, DATE_FIELD).and_then(iso_days_since_epoch),
                        },
                    );
                }
            }
            EDGE_RELATION => {
                let (Some(from), Some(to)) =
                    (row_string(row, FROM_FIELD), row_string(row, TO_FIELD))
                else {
                    return;
                };
                let from = from.to_owned();
                let to = to.to_owned();
                self.nodes.insert(from.clone());
                self.nodes.insert(to.clone());
                self.outgoing
                    .entry(from.clone())
                    .or_default()
                    .insert(to.clone());
                self.incoming
                    .entry(to.clone())
                    .or_default()
                    .insert(from.clone());
                *self.out_edge_count.entry(from).or_default() += 1;
                *self.in_edge_count.entry(to.clone()).or_default() += 1;
                if row_string(row, KIND_FIELD) == Some(CITES_EDGE_KIND) {
                    *self.cite_count.entry(to).or_default() += 1;
                } else if row_string(row, KIND_FIELD) == Some(DISCHARGES_EDGE_KIND) {
                    *self.discharge_count.entry(to).or_default() += 1;
                }
            }
            CONFIG_RELATION => self.insert_config(row),
            CONTENT_RELATION => {
                let (Some(handle), Some(tokens)) =
                    (row_string(row, HANDLE_FIELD), row_i64(row, TOKENS_FIELD))
                else {
                    return;
                };
                let tokens = usize::try_from(tokens).unwrap_or(0);
                *self.content_tokens.entry(handle.to_owned()).or_default() += tokens;
            }
            SNAPSHOT_RELATION => {
                let (Some(id), Some(key), Some(status), Some(at)) = (
                    row_string(row, ID_FIELD),
                    row_string(row, KEY_FIELD),
                    row_string(row, VALUE_FIELD),
                    row_string(row, AT_FIELD),
                ) else {
                    return;
                };
                let Some(day) = snapshot_days_since_epoch(at) else {
                    return;
                };
                if key == STATUS_FIELD {
                    self.insert_status_snapshot(
                        id,
                        SnapshotStatus {
                            day,
                            sort_at: at.to_owned(),
                            status: status.to_owned(),
                        },
                    );
                }
            }
            LINEAR_NAMESPACE_RELATION => {
                if let Some(namespace) = row_string(row, NAMESPACE_FIELD) {
                    self.linear_namespaces.insert(namespace.to_owned());
                }
            }
            _ => {}
        }
    }

    fn insert_status_snapshot(&mut self, handle: &str, snapshot: SnapshotStatus) {
        let snapshots = self.status_snapshots.entry(handle.to_owned()).or_default();
        let idx = snapshots
            .binary_search_by(|probe| {
                probe
                    .day
                    .cmp(&snapshot.day)
                    .then_with(|| probe.sort_at.cmp(&snapshot.sort_at))
                    .then_with(|| probe.status.cmp(&snapshot.status))
            })
            .unwrap_or_else(|idx| idx);
        snapshots.insert(idx, snapshot);
    }

    fn insert_config(&mut self, row: &NamedRow) {
        let (Some(key), Some(value)) = (row_string(row, KEY_FIELD), row_string(row, VALUE_FIELD))
        else {
            return;
        };
        match key {
            CONFIG_ACTIVE_STATUS => {
                self.active_statuses.insert(value.to_owned());
            }
            CONFIG_TERMINAL_STATUS => {
                self.terminal_statuses.insert(value.to_owned());
            }
            CONFIG_SETTLED_STATUS => {
                self.settled_statuses.insert(value.to_owned());
            }
            CONFIG_PIPELINE_ORDERING => {
                let position = row_i64(row, ORDINAL_FIELD).unwrap_or_else(|| {
                    i64::try_from(self.pipeline_positions.len()).unwrap_or(i64::MAX)
                });
                self.pipeline_positions
                    .entry(value.to_owned())
                    .and_modify(|existing| *existing = (*existing).min(position))
                    .or_insert(position);
            }
            CONFIG_LINEAR_NAMESPACE => {
                self.linear_namespaces.insert(value.to_owned());
            }
            _ => {}
        }
    }

    fn tuples(&self, primitive: PrimitivePredicate, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        match primitive {
            PrimitivePredicate::Upstream => {
                self.directional_pairs(constraints, Direction::Outgoing, Direction::Incoming)
            }
            PrimitivePredicate::Downstream => {
                self.directional_pairs(constraints, Direction::Incoming, Direction::Outgoing)
            }
            PrimitivePredicate::Impact => self.impact_tuples(constraints),
            PrimitivePredicate::Neighborhood => self.neighborhood_tuples(constraints),
            PrimitivePredicate::Terminal => self.lifecycle_tuples(constraints, Self::is_terminal),
            PrimitivePredicate::Active => self.lifecycle_tuples(constraints, Self::is_active),
            PrimitivePredicate::Settled => self.lifecycle_tuples(constraints, Self::is_settled),
            PrimitivePredicate::PipelinePosition => self.pipeline_position_tuples(constraints),
            PrimitivePredicate::PipelinePositionFor => {
                self.pipeline_position_for_tuples(constraints)
            }
            PrimitivePredicate::Obligation => {
                self.lifecycle_tuples(constraints, Self::is_obligation)
            }
            PrimitivePredicate::Discharged => {
                self.lifecycle_tuples(constraints, Self::is_discharged)
            }
            PrimitivePredicate::Undischarged => {
                self.lifecycle_tuples(constraints, Self::is_undischarged)
            }
            PrimitivePredicate::CiteCount => self.count_tuples(constraints, &self.cite_count),
            PrimitivePredicate::InDegree => self.count_tuples(constraints, &self.in_edge_count),
            PrimitivePredicate::OutDegree => self.count_tuples(constraints, &self.out_edge_count),
            PrimitivePredicate::DischargeCount => {
                self.handle_count_tuples(constraints, &self.discharge_count)
            }
            PrimitivePredicate::Freshness => self.freshness_tuples(constraints),
            PrimitivePredicate::Flux => self.flux_tuples(constraints),
            PrimitivePredicate::TokenEstimate => {
                self.handle_count_tuples(constraints, &self.content_tokens)
            }
            PrimitivePredicate::Search
            | PrimitivePredicate::Read
            | PrimitivePredicate::ReadFull
            | PrimitivePredicate::Match => Vec::new(),
        }
    }

    fn directional_pairs(
        &self,
        constraints: &[(usize, Value)],
        from_direction: Direction,
        to_direction: Direction,
    ) -> Vec<Tuple> {
        let left = string_constraint(constraints, 0);
        let right = string_constraint(constraints, 1);
        match (left, right) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .reachable_from(start, from_direction, None)
                .into_iter()
                .map(|step| Tuple(vec![string_value(start), string_value(&step.node)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .reachable_from(end, to_direction, None)
                .into_iter()
                .map(|step| Tuple(vec![string_value(&step.node), string_value(end)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.reachable_from(start, from_direction, None)
                        .into_iter()
                        .map(|step| Tuple(vec![string_value(start), string_value(&step.node)]))
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn impact_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let root = string_constraint(constraints, 0);
        let impacted = string_constraint(constraints, 1);
        let depth = i64_constraint(constraints, 2);
        let max_depth = match depth_limit(depth) {
            DepthLimit::Unbounded => None,
            DepthLimit::Max(value) => Some(value),
            DepthLimit::Impossible => return Vec::new(),
        };
        match (root, impacted) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .reachable_from(start, Direction::Incoming, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(start),
                        string_value(&step.node),
                        int_value(step.depth),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .reachable_from(end, Direction::Outgoing, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(&step.node),
                        string_value(end),
                        int_value(step.depth),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.reachable_from(start, Direction::Incoming, max_depth)
                        .into_iter()
                        .map(|step| {
                            Tuple(vec![
                                string_value(start),
                                string_value(&step.node),
                                int_value(step.depth),
                            ])
                        })
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn neighborhood_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let root = string_constraint(constraints, 0);
        let depth = i64_constraint(constraints, 1);
        let member = string_constraint(constraints, 2);
        let max_depth = match depth_limit(depth) {
            DepthLimit::Unbounded => None,
            DepthLimit::Max(value) => Some(value),
            DepthLimit::Impossible => return Vec::new(),
        };
        match (root, member) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .neighborhood_from(start, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(start),
                        int_value(step.depth),
                        string_value(&step.node),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .neighborhood_from(end, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(&step.node),
                        int_value(step.depth),
                        string_value(end),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.neighborhood_from(start, max_depth)
                        .into_iter()
                        .map(|step| {
                            Tuple(vec![
                                string_value(start),
                                int_value(step.depth),
                                string_value(&step.node),
                            ])
                        })
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn lifecycle_tuples(
        &self,
        constraints: &[(usize, Value)],
        predicate: fn(&Self, &str, &HandleState) -> bool,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(id) => self
                .handles
                .get(id)
                .filter(|state| predicate(self, id, state))
                .map(|_| vec![Tuple(vec![string_value(id)])])
                .unwrap_or_default(),
            ArgConstraint::Any => self
                .handles
                .iter()
                .filter(|(id, state)| predicate(self, id, state))
                .map(|(id, _)| Tuple(vec![string_value(id)]))
                .collect(),
        }
    }

    fn pipeline_position_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let position = i64_constraint(constraints, 1);
        match (handle, position) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(id), _) => self
                .handles
                .get(id)
                .and_then(|state| state.status.as_deref())
                .and_then(|status| self.pipeline_position(status))
                .map(|position| Tuple(vec![string_value(id), int_value(position)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .filter_map(|(id, state)| {
                    state
                        .status
                        .as_deref()
                        .and_then(|status| self.pipeline_position(status))
                        .map(|position| Tuple(vec![string_value(id), int_value(position)]))
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn pipeline_position_for_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let status = string_constraint(constraints, 0);
        let position = i64_constraint(constraints, 1);
        match (status, position) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(status), _) => self
                .pipeline_position(status)
                .map(|position| Tuple(vec![string_value(status), int_value(position)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .pipeline_ordering()
                .into_iter()
                .map(|(status, position)| Tuple(vec![string_value(status), int_value(position)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn handle_count_tuples(
        &self,
        constraints: &[(usize, Value)],
        counts: &BTreeMap<String, usize>,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let count = i64_constraint(constraints, 1);
        match (handle, count) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) if self.handles.contains_key(handle) => {
                let count = i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX);
                vec![Tuple(vec![string_value(handle), int_value(count)])]
                    .into_iter()
                    .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                    .collect()
            }
            (ArgConstraint::Exact(_), _) => Vec::new(),
            (ArgConstraint::Any, _) => self
                .handles
                .keys()
                .map(|handle| {
                    let count =
                        i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX);
                    Tuple(vec![string_value(handle), int_value(count)])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn freshness_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = i64_constraint(constraints, 1);
        let today = current_days_since_epoch();
        match (handle, days) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) => self
                .handles
                .get(handle)
                .map(|state| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(freshness_days(state, today)),
                    ])
                })
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .map(|(handle, state)| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(freshness_days(state, today)),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn flux_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = match i64_constraint(constraints, 1) {
            ArgConstraint::Exact(days) if days >= 0 => days,
            ArgConstraint::Any | ArgConstraint::Exact(_) | ArgConstraint::Impossible => {
                return Vec::new();
            }
        };
        let delta = i64_constraint(constraints, 2);
        let today = current_days_since_epoch();
        match (handle, delta) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) => self
                .handles
                .get(handle)
                .map(|state| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(days),
                        int_value(self.flux_delta(handle, state, days, today)),
                    ])
                })
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .map(|(handle, state)| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(days),
                        int_value(self.flux_delta(handle, state, days, today)),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn count_tuples(
        &self,
        constraints: &[(usize, Value)],
        counts: &BTreeMap<String, usize>,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let count = i64_constraint(constraints, 1);
        match (handle, count) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) if self.nodes.contains(handle) => vec![Tuple(vec![
                string_value(handle),
                int_value(i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX)),
            ])]
            .into_iter()
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .collect(),
            (ArgConstraint::Exact(_), _) => Vec::new(),
            (ArgConstraint::Any, _) => self
                .nodes
                .iter()
                .map(|handle| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(
                            i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX),
                        ),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn is_terminal(&self, _handle: &str, state: &HandleState) -> bool {
        let Some(status) = state.status.as_deref() else {
            return false;
        };
        if self.terminal_statuses.contains(status) {
            return true;
        }
        if self.active_statuses.contains(status) {
            return false;
        }
        is_terminal_status(status)
    }

    fn is_active(&self, handle: &str, state: &HandleState) -> bool {
        !self.is_terminal(handle, state)
    }

    fn is_settled(&self, _handle: &str, state: &HandleState) -> bool {
        let Some(status) = state.status.as_deref() else {
            return false;
        };
        self.settled_statuses.contains(status) || is_canonical_settled_status(status)
    }

    fn is_obligation(&self, _handle: &str, state: &HandleState) -> bool {
        state.kind == LABEL_KIND && self.linear_namespaces.contains(&state.namespace)
    }

    fn is_discharged(&self, handle: &str, _state: &HandleState) -> bool {
        self.discharge_count
            .get(handle)
            .copied()
            .unwrap_or_default()
            > 0
    }

    fn is_undischarged(&self, handle: &str, state: &HandleState) -> bool {
        self.is_obligation(handle, state)
            && !self.is_discharged(handle, state)
            && !self.is_terminal(handle, state)
    }

    fn pipeline_position(&self, status: &str) -> Option<i64> {
        self.pipeline_positions.get(status).copied().or_else(|| {
            self.pipeline_positions
                .is_empty()
                .then(|| canonical_pipeline_position(status))
                .flatten()
        })
    }

    fn pipeline_ordering(&self) -> Vec<(&str, i64)> {
        if self.pipeline_positions.is_empty() {
            return CANONICAL_PIPELINE_ORDERING
                .iter()
                .enumerate()
                .map(|(idx, status)| (*status, i64::try_from(idx).unwrap_or(i64::MAX)))
                .collect();
        }
        let mut ordering = self
            .pipeline_positions
            .iter()
            .map(|(status, position)| (status.as_str(), *position))
            .collect::<Vec<_>>();
        ordering.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(right.0)));
        ordering
    }

    fn flux_delta(&self, handle: &str, state: &HandleState, days: i64, today: Option<i64>) -> i64 {
        let Some(today) = today else {
            return 0;
        };
        let start = today.saturating_sub(days);
        let mut statuses = self
            .status_snapshots
            .get(handle)
            .into_iter()
            .flat_map(|snapshots| snapshots.iter())
            .filter(|snapshot| snapshot.day >= start && snapshot.day <= today)
            .map(|snapshot| (snapshot.day, snapshot.status.as_str()))
            .collect::<Vec<_>>();
        if let Some(status) = state.status.as_deref() {
            statuses.push((today, status));
        }
        i64::try_from(
            statuses
                .windows(2)
                .filter(|pair| pair[0].1 != pair[1].1)
                .count(),
        )
        .unwrap_or(i64::MAX)
    }

    fn reachable_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        self.walk_from(start, direction, false, max_depth)
    }

    fn neighborhood_from(&self, start: &str, max_depth: Option<i64>) -> Vec<GraphStep> {
        if !self.nodes.contains(start) {
            return Vec::new();
        }
        self.walk_from(start, Direction::Undirected, true, max_depth)
    }

    fn walk_from(
        &self,
        start: &str,
        direction: Direction,
        include_start: bool,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        let mut out = Vec::new();
        if include_start {
            out.push(GraphStep {
                node: start.to_owned(),
                depth: 0,
            });
        }
        let mut seen = BTreeSet::from([start.to_owned()]);
        let mut queue = VecDeque::from([(start.to_owned(), 0_i64)]);
        while let Some((node, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            self.visit_neighbors(&node, direction, |next| {
                if !seen.insert(next.clone()) {
                    return;
                }
                let next_depth = depth + 1;
                out.push(GraphStep {
                    node: next.clone(),
                    depth: next_depth,
                });
                queue.push_back((next.clone(), next_depth));
            });
        }
        out
    }

    fn visit_neighbors(&self, node: &str, direction: Direction, mut visit: impl FnMut(&String)) {
        match direction {
            Direction::Outgoing => {
                if let Some(outgoing) = self.outgoing.get(node) {
                    for next in outgoing {
                        visit(next);
                    }
                }
            }
            Direction::Incoming => {
                if let Some(incoming) = self.incoming.get(node) {
                    for next in incoming {
                        visit(next);
                    }
                }
            }
            Direction::Undirected => {
                if let Some(incoming) = self.incoming.get(node) {
                    for next in incoming {
                        visit(next);
                    }
                }
                if let Some(outgoing) = self.outgoing.get(node) {
                    for next in outgoing {
                        visit(next);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GraphStep {
    node: String,
    depth: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgConstraint<T> {
    Any,
    Exact(T),
    Impossible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DepthLimit {
    Unbounded,
    Max(i64),
    Impossible,
}

fn row_string<'a>(row: &'a NamedRow, field: &str) -> Option<&'a str> {
    let field = Ident::new_unchecked(field);
    let Some(Value::String(value)) = row.get(&field) else {
        return None;
    };
    Some(value)
}

fn row_i64(row: &NamedRow, field: &str) -> Option<i64> {
    let field = Ident::new_unchecked(field);
    let Some(Value::Number(NumberValue::Int(value))) = row.get(&field) else {
        return None;
    };
    Some(*value)
}

const HANDLE_RELATION: &str = "handle";
const EDGE_RELATION: &str = "edge";
const META_RELATION: &str = "meta";
const CONFIG_RELATION: &str = "config";
const CONTENT_RELATION: &str = "content";
const SPAN_RELATION: &str = "span";
const SNAPSHOT_RELATION: &str = "snapshot";
const LINEAR_NAMESPACE_RELATION: &str = "linear_namespace";
const CORPUS_FIELD: &str = "corpus";
const SOURCE_FIELD: &str = "source";
const SNAPSHOT_FIELD: &str = "snapshot";
const ID_FIELD: &str = "id";
const FROM_FIELD: &str = "from";
const TO_FIELD: &str = "to";
const KIND_FIELD: &str = "kind";
const STATUS_FIELD: &str = "status";
const NAMESPACE_FIELD: &str = "namespace";
const DATE_FIELD: &str = "date";
const AREA_FIELD: &str = "area";
const SUMMARY_FIELD: &str = "summary";
const HANDLE_FIELD: &str = "handle";
const SPAN_ID_FIELD: &str = "span_id";
const TEXT_FIELD: &str = "text";
const START_LINE_FIELD: &str = "start_line";
const END_LINE_FIELD: &str = "end_line";
const TOKENS_FIELD: &str = "tokens";
const KEY_FIELD: &str = "key";
const VALUE_FIELD: &str = "value";
const ORDINAL_FIELD: &str = "ordinal";
const AT_FIELD: &str = "at";
const LABEL_KIND: &str = "label";
const CITES_EDGE_KIND: &str = "Cites";
const DISCHARGES_EDGE_KIND: &str = "Discharges";
const CONFIG_ACTIVE_STATUS: &str = "convergence.active";
const CONFIG_TERMINAL_STATUS: &str = "convergence.terminal";
const CONFIG_SETTLED_STATUS: &str = "convergence.settled";
const CONFIG_PIPELINE_ORDERING: &str = "convergence.ordering";
const CONFIG_LINEAR_NAMESPACE: &str = "handles.linear";
const CANONICAL_PIPELINE_ORDERING: &[&str] = &[
    "raw",
    "draft",
    "research",
    "plan",
    "current",
    "active",
    "stable",
    "authoritative",
];
const TERMINAL_STATUS_HEURISTICS: &[&str] = &[
    "superseded",
    "archived",
    "historical",
    "prior",
    "retired",
    "deprecated",
    "obsolete",
    "withdrawn",
    "cancelled",
    "canceled",
    "closed",
    "resolved",
    "done",
    "completed",
    "incorporated",
    "digested",
];
const CANONICAL_SETTLED_STATUSES: &[&str] =
    &["authoritative", "current", "active", "stable", "living"];

fn is_terminal_status(status: &str) -> bool {
    let lower = status.to_lowercase();
    TERMINAL_STATUS_HEURISTICS
        .iter()
        .any(|heuristic| lower.contains(heuristic))
}

fn is_canonical_settled_status(status: &str) -> bool {
    CANONICAL_SETTLED_STATUSES.contains(&status)
}

fn canonical_pipeline_position(status: &str) -> Option<i64> {
    CANONICAL_PIPELINE_ORDERING
        .iter()
        .position(|candidate| candidate == &status)
        .map(|idx| i64::try_from(idx).unwrap_or(i64::MAX))
}

fn freshness_days(state: &HandleState, today: Option<i64>) -> i64 {
    let (Some(date), Some(today)) = (state.date, today) else {
        return 0;
    };
    today.saturating_sub(date).max(0)
}

fn string_constraint(constraints: &[(usize, Value)], position: usize) -> ArgConstraint<&str> {
    value_constraint(constraints, position, |value| match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    })
}

fn i64_constraint(constraints: &[(usize, Value)], position: usize) -> ArgConstraint<i64> {
    value_constraint(constraints, position, |value| match value {
        Value::Number(NumberValue::Int(value)) => Some(*value),
        _ => None,
    })
}

fn depth_limit(depth: ArgConstraint<i64>) -> DepthLimit {
    match depth {
        ArgConstraint::Any => DepthLimit::Unbounded,
        ArgConstraint::Exact(value) if value >= 0 => DepthLimit::Max(value),
        ArgConstraint::Exact(_) | ArgConstraint::Impossible => DepthLimit::Impossible,
    }
}

fn value_constraint<'a, T>(
    constraints: &'a [(usize, Value)],
    position: usize,
    get: impl Fn(&'a Value) -> Option<T>,
) -> ArgConstraint<T> {
    let Some((_, value)) = constraints.iter().find(|(idx, _)| *idx == position) else {
        return ArgConstraint::Any;
    };
    get(value).map_or(ArgConstraint::Impossible, ArgConstraint::Exact)
}

fn tuple_matches_constraints(tuple: &Tuple, constraints: &[(usize, Value)]) -> bool {
    constraints
        .iter()
        .all(|(idx, value)| tuple.0.get(*idx) == Some(value))
}

fn string_value(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn int_value(value: i64) -> Value {
    Value::Number(NumberValue::Int(value))
}

fn float_value(value: f64) -> Value {
    Value::Number(NumberValue::Float(value))
}

fn should_index_stored_field(relation: &Ident, field: &Ident) -> bool {
    !matches!(
        (relation.as_str(), field.as_str()),
        ("content", "text")
            | ("span" | "handle", "summary")
            | ("meta" | "config" | "snapshot", "value")
    )
}

#[derive(Clone, Debug, Default)]
struct DerivedRelation {
    tuples: BTreeSet<Tuple>,
    indexes: Vec<BTreeMap<Value, Vec<Tuple>>>,
}

impl DerivedRelation {
    fn len(&self) -> usize {
        self.tuples.len()
    }

    fn tuples(&self) -> &BTreeSet<Tuple> {
        &self.tuples
    }

    fn insert(&mut self, tuple: &Tuple) -> bool {
        if !self.tuples.insert(tuple.clone()) {
            return false;
        }
        if self.indexes.len() < tuple.0.len() {
            self.indexes.resize_with(tuple.0.len(), BTreeMap::new);
        }
        for (idx, value) in tuple.0.iter().enumerate() {
            self.indexes[idx]
                .entry(value.clone())
                .or_default()
                .push(tuple.clone());
        }
        true
    }

    fn candidate_tuples(&self, constraints: &[(usize, Value)]) -> TupleCandidates<'_> {
        let mut best = None;
        for (idx, value) in constraints {
            let Some(values) = self.indexes.get(*idx) else {
                return TupleCandidates::Empty;
            };
            let Some(tuples) = values.get(value) else {
                return TupleCandidates::Empty;
            };
            if best.is_none_or(|current: &Vec<Tuple>| tuples.len() < current.len()) {
                best = Some(tuples);
            }
        }

        best.map_or_else(
            || TupleCandidates::All(self.tuples.iter()),
            |tuples| TupleCandidates::Indexed(tuples.iter()),
        )
    }
}

enum TupleCandidates<'a> {
    All(btree_set::Iter<'a, Tuple>),
    Indexed(slice::Iter<'a, Tuple>),
    Empty,
}

impl<'a> Iterator for TupleCandidates<'a> {
    type Item = &'a Tuple;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(tuples) => tuples.next(),
            Self::Indexed(tuples) => tuples.next(),
            Self::Empty => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("unknown stored relation '*{relation}'")]
    UnknownStoredRelation { relation: Ident },
    #[error("unknown derived predicate '{predicate}'")]
    UnknownDerivedPredicate { predicate: PredicateRef },
    #[error("unbound variable '{variable}'")]
    UnboundVariable { variable: Ident },
    #[error("unsupported aggregate '{function:?}'")]
    UnsupportedAggregate { function: AggregateFunction },
    #[error("aggregate '{function:?}' requires argument '{argument}'")]
    MissingAggregateArg {
        function: AggregateFunction,
        argument: &'static str,
    },
    #[error("aggregate '{function:?}' argument '{argument}' is invalid")]
    InvalidAggregateArg {
        function: AggregateFunction,
        argument: &'static str,
    },
    #[error("unsupported time reference '{reference}'")]
    UnsupportedTimeRef { reference: String },
    #[error(
        "time reference '{reference}' cannot evaluate derived predicate '{predicate}' with snapshot fallback"
    )]
    UnsupportedTimeScopedDerivedPredicate {
        reference: String,
        predicate: PredicateRef,
    },
    #[error(
        "time reference '{reference}' cannot evaluate stored relation '*{relation}' with snapshot fallback"
    )]
    UnsupportedTimeScopedStoredRelation { reference: String, relation: Ident },
    #[error(
        "time reference '{reference}' cannot evaluate primitive '{predicate}' with snapshot fallback"
    )]
    UnsupportedTimeScopedPrimitive {
        reference: String,
        predicate: PredicateRef,
    },
    #[error("primitive '{primitive}' requires capability '{capability}'")]
    CapabilityRequired {
        primitive: &'static str,
        capability: RuntimeCapability,
    },
    #[error("read_full({handle:?}) would return {tokens} tokens, exceeding the hard limit {limit}")]
    ReadFullBudgetExceeded {
        handle: String,
        tokens: i64,
        limit: i64,
    },
    #[error("invalid regex pattern {pattern:?}: {source}")]
    InvalidRegex {
        pattern: String,
        source: regex::Error,
    },
    #[error("unsupported expression")]
    UnsupportedExpression,
    #[error("division by zero")]
    DivisionByZero,
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct Evaluator {
    program: AnalyzedProgram,
    database: Database,
    facts_seeded: bool,
    warnings: Vec<QueryWarning>,
    options: EvalOptions,
}

impl Evaluator {
    pub fn new(program: AnalyzedProgram, database: Database) -> Self {
        Self::with_options(program, database, EvalOptions::default())
    }

    pub fn with_options(
        program: AnalyzedProgram,
        mut database: Database,
        options: EvalOptions,
    ) -> Self {
        database.ensure_derived(program.predicates().cloned());
        Self {
            program,
            database,
            facts_seeded: false,
            warnings: Vec::new(),
            options,
        }
    }

    pub fn run_fixpoint(&mut self) -> Result<(), EvalError> {
        self.seed_facts()?;
        let strata = self.program.strata().to_vec();
        for stratum in strata {
            let rules = self
                .program
                .rules()
                .filter(|rule| stratum.predicates.contains(&rule.head.predicate))
                .cloned()
                .collect::<Vec<_>>();
            run_rule_group(
                &mut self.database,
                &rules,
                &mut self.warnings,
                &self.options,
            )?;
        }
        Ok(())
    }

    pub fn eval_query(&self, query: &AnalyzedQuery) -> Result<QueryOutput, EvalError> {
        let query_ast = query.query();
        let mut warnings = self.warnings.clone();
        if query_ast.local_rules.is_empty() {
            let bindings = eval_body(
                &query_ast.body,
                vec![Binding::new()],
                &self.database,
                &mut warnings,
                &self.options,
            )?;
            return Ok(QueryOutput {
                rows: bindings.into_iter().map(binding_to_row).collect(),
                warnings,
            });
        }

        let mut database = self.database.clone();
        database.ensure_derived(query.local_predicates().cloned());
        for stratum in query.local_strata() {
            let rules = query_ast
                .local_rules
                .iter()
                .filter(|rule| stratum.predicates.contains(&rule.head.predicate))
                .cloned()
                .collect::<Vec<_>>();
            run_rule_group(&mut database, &rules, &mut warnings, &self.options)?;
        }
        let bindings = eval_body(
            &query_ast.body,
            vec![Binding::new()],
            &database,
            &mut warnings,
            &self.options,
        )?;
        let rows = bindings.into_iter().map(binding_to_row).collect();
        Ok(QueryOutput { rows, warnings })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    fn seed_facts(&mut self) -> Result<(), EvalError> {
        if self.facts_seeded {
            return Ok(());
        }
        for fact in self.program.facts() {
            let tuple = project_fact_head(fact)?;
            self.database
                .derived
                .entry(fact.predicate.clone())
                .or_default()
                .insert(&tuple);
        }
        self.facts_seeded = true;
        Ok(())
    }
}

fn run_rule_group(
    database: &mut Database,
    rules: &[Rule],
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<(), EvalError> {
    let stratum_predicates = rules
        .iter()
        .map(|rule| rule.head.predicate.clone())
        .collect::<BTreeSet<_>>();
    database.ensure_derived(stratum_predicates.iter().cloned());

    let mut delta = DeltaMap::new();
    for rule in rules {
        let tuples = eval_rule(rule, database, warnings, options)?;
        insert_new_tuples(database, &rule.head.predicate, tuples, &mut delta);
    }

    while !delta.is_empty() {
        let previous_delta = delta;
        delta = DeltaMap::new();
        for rule in rules {
            for atom_index in recursive_atom_indexes(&rule.body, &stratum_predicates) {
                let tuples = eval_rule_with_delta(
                    rule,
                    database,
                    &previous_delta,
                    atom_index,
                    warnings,
                    options,
                )?;
                insert_new_tuples(database, &rule.head.predicate, tuples, &mut delta);
            }
        }
    }
    Ok(())
}

fn eval_rule(
    rule: &Rule,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Tuple>, EvalError> {
    let bindings = eval_body(
        &rule.body,
        vec![Binding::new()],
        database,
        warnings,
        options,
    )?;
    bindings
        .into_iter()
        .map(|binding| project_head(&rule.head, &binding))
        .collect()
}

fn eval_rule_with_delta(
    rule: &Rule,
    database: &Database,
    delta: &DeltaMap,
    atom_index: usize,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Tuple>, EvalError> {
    let bindings = eval_body_with_delta(
        &rule.body,
        vec![Binding::new()],
        database,
        Some(DeltaView { delta, atom_index }),
        warnings,
        options,
    )?;
    bindings
        .into_iter()
        .map(|binding| project_head(&rule.head, &binding))
        .collect()
}

fn insert_new_tuples(
    database: &mut Database,
    predicate: &PredicateRef,
    tuples: Vec<Tuple>,
    delta: &mut DeltaMap,
) -> bool {
    let relation = database.derived.entry(predicate.clone()).or_default();
    let mut changed = false;
    for tuple in tuples {
        if relation.insert(&tuple) {
            delta.entry(predicate.clone()).or_default().insert(&tuple);
            changed = true;
        }
    }
    changed
}

fn recursive_atom_indexes(body: &Body, stratum_predicates: &BTreeSet<PredicateRef>) -> Vec<usize> {
    body.atoms
        .iter()
        .enumerate()
        .filter_map(|(idx, atom)| match atom {
            Atom::Derived(derived) if stratum_predicates.contains(&derived.predicate) => Some(idx),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy)]
struct DeltaView<'a> {
    delta: &'a DeltaMap,
    atom_index: usize,
}

fn eval_body(
    body: &Body,
    bindings: Vec<Binding>,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    eval_body_with_delta(body, bindings, database, None, warnings, options)
}

fn eval_body_with_delta(
    body: &Body,
    mut bindings: Vec<Binding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    let mut remaining = body.atoms.iter().enumerate().collect::<Vec<_>>();
    while !remaining.is_empty() {
        if bindings.is_empty() {
            break;
        }
        let bound = common_bound_variables(&bindings);
        let next_index = remaining
            .iter()
            .position(|(atom_index, atom)| atom_ready(body, *atom_index, atom, &bound))
            .unwrap_or(0);
        let (atom_index, atom) = remaining.remove(next_index);
        let atom_delta = delta.filter(|view| view.atom_index == atom_index);
        bindings = eval_atom(atom, bindings, database, atom_delta, warnings, options)?;
    }
    Ok(bindings)
}

fn common_bound_variables(bindings: &[Binding]) -> BTreeSet<Ident> {
    let Some((first, rest)) = bindings.split_first() else {
        return BTreeSet::new();
    };
    let mut common = first.keys().cloned().collect::<BTreeSet<_>>();
    for binding in rest {
        common.retain(|var| binding.contains_key(var));
    }
    common
}

fn atom_ready(body: &Body, atom_index: usize, atom: &Atom, bound: &BTreeSet<Ident>) -> bool {
    match atom {
        Atom::Stored(_) | Atom::TimeBlock(_) => true,
        Atom::Derived(derived) => derived_atom_ready(derived, bound),
        Atom::Comparison(comparison) => {
            expr_variables_are_bound(&comparison.left, bound)
                && expr_variables_are_bound(&comparison.right, bound)
        }
        Atom::Aggregation(aggregate) => aggregate_atom_ready(body, atom_index, aggregate, bound),
        Atom::Negation(negation) => negated_atom_variables_are_bound(&negation.atom, bound),
    }
}

fn derived_atom_ready(atom: &crate::runtime::ast::DerivedAtom, bound: &BTreeSet<Ident>) -> bool {
    let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) else {
        return true;
    };
    let graph_ready = primitive.graph_anchor_positions().is_none_or(|positions| {
        positions.iter().any(|idx| {
            atom.args
                .get(*idx)
                .is_some_and(|arg| expr_variables_are_bound(arg.expr(), bound))
        })
    });
    graph_ready && content_primitive_inputs_ready(atom, primitive, bound)
}

fn content_primitive_inputs_ready(
    atom: &crate::runtime::ast::DerivedAtom,
    primitive: PrimitivePredicate,
    bound: &BTreeSet<Ident>,
) -> bool {
    primitive.required_bound_inputs().iter().all(|input| {
        atom.args
            .get(input.position)
            .is_some_and(|arg| expr_variables_are_bound(arg.expr(), bound))
    })
}

fn aggregate_atom_ready(
    body: &Body,
    atom_index: usize,
    aggregate: &Aggregate,
    bound: &BTreeSet<Ident>,
) -> bool {
    let mut outside = BTreeSet::new();
    for (other_index, atom) in body.atoms.iter().enumerate() {
        if other_index != atom_index {
            collect_non_aggregate_positive_atom_variables(atom, &mut outside);
        }
    }

    let mut inner = BTreeSet::new();
    collect_positive_body_variables(&aggregate.body, &mut inner);

    let mut required = inner
        .intersection(&outside)
        .cloned()
        .collect::<BTreeSet<_>>();
    collect_ground_aggregate_arg_variables(aggregate, &mut required);
    required.iter().all(|var| bound.contains(var))
}

fn collect_ground_aggregate_arg_variables(aggregate: &Aggregate, out: &mut BTreeSet<Ident>) {
    for arg in &aggregate.args {
        if matches!(
            (aggregate.function, arg.name.as_str()),
            (AggregateFunction::TopK, "k") | (AggregateFunction::TakeUntil, "budget")
        ) {
            arg.expr.variables(out);
        }
    }
}

fn negated_atom_variables_are_bound(atom: &NegatedAtom, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    collect_negated_atom_variables(atom, &mut vars);
    vars.iter().all(|var| bound.contains(var))
}

fn expr_variables_are_bound(expr: &Expr, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    vars.iter().all(|var| bound.contains(var))
}

fn collect_non_aggregate_positive_atom_variables(atom: &Atom, out: &mut BTreeSet<Ident>) {
    match atom {
        Atom::Stored(stored) => collect_stored_atom_variables(stored, out),
        Atom::Derived(derived) => collect_derived_atom_variables(derived, out),
        Atom::TimeBlock(time_block) => collect_positive_body_variables(&time_block.body, out),
        Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
    }
}

fn collect_positive_body_variables(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        match atom {
            Atom::Aggregation(aggregate) => aggregate.result.variables(out),
            _ => collect_non_aggregate_positive_atom_variables(atom, out),
        }
    }
}

fn collect_negated_atom_variables(atom: &NegatedAtom, out: &mut BTreeSet<Ident>) {
    match atom {
        NegatedAtom::Stored(stored) => collect_stored_atom_variables(stored, out),
        NegatedAtom::Derived(derived) => collect_derived_atom_variables(derived, out),
    }
}

fn collect_stored_atom_variables(atom: &StoredAtom, out: &mut BTreeSet<Ident>) {
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.variables(out);
        }
    }
}

fn collect_derived_atom_variables(
    atom: &crate::runtime::ast::DerivedAtom,
    out: &mut BTreeSet<Ident>,
) {
    for arg in &atom.args {
        arg.expr().variables(out);
    }
}

fn eval_atom(
    atom: &Atom,
    bindings: Vec<Binding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    match atom {
        Atom::Stored(stored) => eval_stored(stored, bindings, database),
        Atom::Derived(derived) => {
            if let Some(view) = delta {
                eval_derived_from_delta(derived, bindings, view.delta)
            } else {
                eval_derived(derived, bindings, database, options)
            }
        }
        Atom::Comparison(comparison) => eval_comparison(comparison, bindings),
        Atom::Aggregation(aggregate) => {
            eval_aggregate(aggregate, bindings, database, warnings, options)
        }
        Atom::Negation(negation) => {
            eval_negation(&negation.atom, bindings, database, warnings, options)
        }
        Atom::TimeBlock(time_block) => {
            ensure_snapshot_time_body_supported(&time_block.reference, &time_block.body, database)?;
            let (scoped, scoped_warnings) = database.scoped_to_time_ref(&time_block.reference)?;
            push_warnings(warnings, scoped_warnings);
            eval_body(&time_block.body, bindings, &scoped, warnings, options)
        }
    }
}

fn ensure_snapshot_time_body_supported(
    reference: &str,
    body: &Body,
    database: &Database,
) -> Result<(), EvalError> {
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => {
                if !time_scoped_stored_relation_supported(&stored.relation) {
                    return Err(EvalError::UnsupportedTimeScopedStoredRelation {
                        reference: reference.to_string(),
                        relation: stored.relation.clone(),
                    });
                }
            }
            Atom::Comparison(_) => {}
            Atom::Derived(derived) => {
                ensure_snapshot_time_derived_supported(reference, &derived.predicate, database)?;
            }
            Atom::Negation(negation) => match &negation.atom {
                NegatedAtom::Stored(stored) => {
                    if !time_scoped_stored_relation_supported(&stored.relation) {
                        return Err(EvalError::UnsupportedTimeScopedStoredRelation {
                            reference: reference.to_string(),
                            relation: stored.relation.clone(),
                        });
                    }
                }
                NegatedAtom::Derived(derived) => {
                    ensure_snapshot_time_derived_supported(
                        reference,
                        &derived.predicate,
                        database,
                    )?;
                }
            },
            Atom::Aggregation(aggregate) => {
                ensure_snapshot_time_body_supported(reference, &aggregate.body, database)?;
            }
            Atom::TimeBlock(time_block) => {
                ensure_snapshot_time_body_supported(
                    &time_block.reference,
                    &time_block.body,
                    database,
                )?;
            }
        }
    }
    Ok(())
}

fn time_scoped_stored_relation_supported(relation: &Ident) -> bool {
    matches!(relation.as_str(), HANDLE_RELATION | SNAPSHOT_RELATION)
}

fn ensure_snapshot_time_derived_supported(
    reference: &str,
    predicate: &PredicateRef,
    database: &Database,
) -> Result<(), EvalError> {
    let Some(primitive) = PrimitivePredicate::from_predicate(predicate) else {
        return Err(EvalError::UnsupportedTimeScopedDerivedPredicate {
            reference: reference.to_string(),
            predicate: predicate.clone(),
        });
    };
    if database.derived.contains_key(predicate) {
        return Err(EvalError::UnsupportedTimeScopedDerivedPredicate {
            reference: reference.to_string(),
            predicate: predicate.clone(),
        });
    }
    if time_scoped_primitive_supported(primitive) {
        Ok(())
    } else {
        Err(EvalError::UnsupportedTimeScopedPrimitive {
            reference: reference.to_string(),
            predicate: predicate.clone(),
        })
    }
}

fn time_scoped_primitive_supported(primitive: PrimitivePredicate) -> bool {
    matches!(
        primitive,
        PrimitivePredicate::Terminal
            | PrimitivePredicate::Active
            | PrimitivePredicate::Settled
            | PrimitivePredicate::PipelinePosition
            | PrimitivePredicate::PipelinePositionFor
            | PrimitivePredicate::Obligation
            | PrimitivePredicate::Freshness
            | PrimitivePredicate::Flux
    )
}

fn eval_stored(
    atom: &StoredAtom,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    let relation =
        database
            .stored
            .get(&atom.relation)
            .ok_or_else(|| EvalError::UnknownStoredRelation {
                relation: atom.relation.clone(),
            })?;
    let mut out = Vec::new();
    for binding in bindings {
        let constraints = stored_constraints(&atom.fields, &binding)?;
        for row in relation.candidate_rows(&constraints) {
            if let Some(next) = unify_stored_fields(&atom.fields, row, &binding)? {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn eval_derived(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    if let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) {
        if primitive.is_soft() && database.derived.contains_key(&atom.predicate) {
            let relation = database.derived.get(&atom.predicate).ok_or_else(|| {
                EvalError::UnknownDerivedPredicate {
                    predicate: atom.predicate.clone(),
                }
            })?;
            return eval_derived_from_relation(atom, bindings, relation);
        }
        return eval_primitive(primitive, &atom.args, bindings, database, options);
    }
    let relation = database.derived.get(&atom.predicate).ok_or_else(|| {
        EvalError::UnknownDerivedPredicate {
            predicate: atom.predicate.clone(),
        }
    })?;
    eval_derived_from_relation(atom, bindings, relation)
}

fn eval_derived_from_delta(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    delta: &DeltaMap,
) -> Result<Vec<Binding>, EvalError> {
    let Some(relation) = delta.get(&atom.predicate) else {
        return Ok(Vec::new());
    };
    eval_derived_from_relation(atom, bindings, relation)
}

fn eval_derived_from_relation(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    relation: &DerivedRelation,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let constraints = call_constraints(&atom.args, &binding)?;
        for tuple in relation.candidate_tuples(&constraints) {
            if tuple.0.len() != atom.args.len() {
                continue;
            }
            if let Some(next) = unify_call_args(&atom.args, tuple, &binding)? {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn eval_primitive(
    primitive: PrimitivePredicate,
    args: &[CallArg],
    bindings: Vec<Binding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    let mut regex_cache = BTreeMap::<String, Regex>::new();
    for binding in bindings {
        let constraints = call_constraints(args, &binding)?;
        let tuples =
            primitive_tuples(primitive, &constraints, database, options, &mut regex_cache)?;
        for tuple in tuples {
            if let Some(next) = unify_call_args(args, &tuple, &binding)? {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn primitive_tuples(
    primitive: PrimitivePredicate,
    constraints: &[(usize, Value)],
    database: &Database,
    options: &EvalOptions,
    regex_cache: &mut BTreeMap<String, Regex>,
) -> Result<Vec<Tuple>, EvalError> {
    match primitive {
        PrimitivePredicate::Search => Ok(database.search_tuples(constraints, options)),
        PrimitivePredicate::Read => Ok(database.content.read_tuples(constraints)),
        PrimitivePredicate::ReadFull => {
            if !options.has_capability(READ_FULL_CAPABILITY) {
                return Err(EvalError::CapabilityRequired {
                    primitive: "read_full",
                    capability: READ_FULL_CAPABILITY,
                });
            }
            database
                .content
                .read_full_tuples(constraints, options.read_full_token_limit)
        }
        PrimitivePredicate::Match => {
            let ArgConstraint::Exact(pattern) = string_constraint(constraints, 0) else {
                return Ok(Vec::new());
            };
            if !regex_cache.contains_key(pattern) {
                let regex = Regex::new(pattern).map_err(|source| EvalError::InvalidRegex {
                    pattern: pattern.to_owned(),
                    source,
                })?;
                regex_cache.insert(pattern.to_owned(), regex);
            }
            let regex = regex_cache
                .get(pattern)
                .expect("regex was inserted before lookup");
            Ok(database.content.match_tuples(constraints, regex))
        }
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::TokenEstimate => Ok(database.graph.tuples(primitive, constraints)),
    }
}

fn eval_comparison(
    comparison: &Comparison,
    bindings: Vec<Binding>,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let left = eval_expr(&comparison.left, &binding)?;
        let right = eval_expr(&comparison.right, &binding)?;
        if compare(&left, comparison.op, &right)? {
            out.push(binding);
        }
    }
    Ok(out)
}

fn eval_negation(
    negated: &NegatedAtom,
    bindings: Vec<Binding>,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let atom = match negated {
            NegatedAtom::Stored(stored) => Atom::Stored(stored.clone()),
            NegatedAtom::Derived(derived) => Atom::Derived(derived.clone()),
        };
        let matches = eval_atom(
            &atom,
            vec![binding.clone()],
            database,
            None,
            warnings,
            options,
        )?;
        if matches.is_empty() {
            out.push(binding);
        }
    }
    Ok(out)
}

fn eval_aggregate(
    aggregate: &Aggregate,
    bindings: Vec<Binding>,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    validate_aggregate_args(aggregate)?;

    let mut out = Vec::new();
    for binding in bindings {
        let inner = eval_body(
            &aggregate.body,
            vec![binding.clone()],
            database,
            warnings,
            options,
        )?;
        if inner.is_empty() {
            if aggregate.function == AggregateFunction::Count
                && let Some(group) = bind_aggregate_result(
                    &aggregate.result,
                    &binding,
                    &Value::Number(NumberValue::Int(0)),
                )?
            {
                out.push(group);
            }
            continue;
        }
        out.extend(eval_aggregate_group(aggregate, &binding, &inner)?);
    }
    Ok(out)
}

fn eval_aggregate_group(
    aggregate: &Aggregate,
    base: &Binding,
    rows: &[Binding],
) -> Result<Vec<Binding>, EvalError> {
    match aggregate.function {
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::Avg
        | AggregateFunction::List
        | AggregateFunction::Set => {
            let values = distinct_aggregate_values(aggregate, rows)?;
            let Some(value) = scalar_aggregate_value(aggregate.function, &values)? else {
                return Ok(Vec::new());
            };
            Ok(bind_aggregate_result(&aggregate.result, base, &value)?
                .into_iter()
                .collect())
        }
        AggregateFunction::TopK => eval_top_k_aggregate(aggregate, base, rows),
        AggregateFunction::Rank => eval_rank_aggregate(aggregate, base, rows),
        AggregateFunction::TakeUntil => eval_take_until_aggregate(aggregate, base, rows),
    }
}

fn distinct_aggregate_values(
    aggregate: &Aggregate,
    rows: &[Binding],
) -> Result<BTreeSet<Value>, EvalError> {
    rows.iter()
        .map(|row| eval_expr(&aggregate.value, row))
        .collect()
}

fn scalar_aggregate_value(
    function: AggregateFunction,
    values: &BTreeSet<Value>,
) -> Result<Option<Value>, EvalError> {
    if values.is_empty() && function != AggregateFunction::Count {
        return Ok(None);
    }
    match function {
        AggregateFunction::Count => Ok(Some(Value::Number(NumberValue::Int(
            i64::try_from(values.len()).unwrap_or(i64::MAX),
        )))),
        AggregateFunction::Sum => numeric_sum(values).map(Some),
        AggregateFunction::Min => Ok(values.first().cloned()),
        AggregateFunction::Max => Ok(values.last().cloned()),
        AggregateFunction::Avg => numeric_avg(values).map(Some),
        AggregateFunction::List | AggregateFunction::Set => {
            Ok(Some(Value::List(values.iter().cloned().collect())))
        }
        AggregateFunction::TopK | AggregateFunction::Rank | AggregateFunction::TakeUntil => {
            Err(EvalError::UnsupportedAggregate { function })
        }
    }
}

fn numeric_sum(values: &BTreeSet<Value>) -> Result<Value, EvalError> {
    let mut int_sum = 0_i64;
    let mut float_sum = 0.0_f64;
    let mut has_float = false;
    for value in values {
        match numeric_value(value)? {
            NumberValue::Int(value) if !has_float => {
                int_sum = int_sum.saturating_add(value);
            }
            NumberValue::Int(value) => {
                float_sum += i64_to_f64(value);
            }
            NumberValue::Float(value) => {
                if !has_float {
                    float_sum = i64_to_f64(int_sum);
                    has_float = true;
                }
                float_sum += value;
            }
        }
    }
    if has_float {
        Ok(Value::Number(NumberValue::Float(float_sum)))
    } else {
        Ok(Value::Number(NumberValue::Int(int_sum)))
    }
}

fn numeric_avg(values: &BTreeSet<Value>) -> Result<Value, EvalError> {
    let mut total = 0.0_f64;
    for value in values {
        match numeric_value(value)? {
            NumberValue::Int(value) => total += i64_to_f64(value),
            NumberValue::Float(value) => total += value,
        }
    }
    Ok(Value::Number(NumberValue::Float(
        total / usize_to_f64(values.len()),
    )))
}

fn numeric_value(value: &Value) -> Result<NumberValue, EvalError> {
    let Value::Number(value) = value else {
        return Err(EvalError::UnsupportedExpression);
    };
    Ok(*value)
}

fn i64_to_f64(value: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

fn usize_to_f64(value: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

#[derive(Clone, Debug)]
struct OrderedAggregateCandidate {
    value: Value,
    key: Value,
}

#[derive(Clone, Debug)]
struct RankAggregateCandidate {
    key: Value,
    row: Binding,
}

fn compare_ordered_candidates(
    left: &OrderedAggregateCandidate,
    right: &OrderedAggregateCandidate,
) -> Ordering {
    right
        .key
        .cmp(&left.key)
        .then_with(|| left.value.cmp(&right.value))
}

fn top_k_candidates(
    aggregate: &Aggregate,
    rows: &[Binding],
    limit: usize,
) -> Result<Vec<OrderedAggregateCandidate>, EvalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let key = required_aggregate_arg(aggregate, "key")?;
    let mut candidates = Vec::new();
    for row in rows {
        let candidate = OrderedAggregateCandidate {
            value: eval_expr(&aggregate.value, row)?,
            key: eval_expr(&key.expr, row)?,
        };
        let insert_at = candidates
            .binary_search_by(|existing| compare_ordered_candidates(existing, &candidate))
            .unwrap_or_else(|idx| idx);
        if insert_at < limit {
            candidates.insert(insert_at, candidate);
            if candidates.len() > limit {
                candidates.pop();
            }
        }
    }
    Ok(candidates)
}

fn rank_candidates(
    aggregate: &Aggregate,
    rows: &[Binding],
) -> Result<Vec<RankAggregateCandidate>, EvalError> {
    let key = required_aggregate_arg(aggregate, "key")?;
    let mut candidates = rows
        .iter()
        .map(|row| {
            Ok(RankAggregateCandidate {
                key: eval_expr(&key.expr, row)?,
                row: row.clone(),
            })
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    candidates.sort_by(|left, right| {
        right
            .key
            .cmp(&left.key)
            .then_with(|| left.row.cmp(&right.row))
    });
    Ok(candidates)
}

fn eval_top_k_aggregate(
    aggregate: &Aggregate,
    base: &Binding,
    rows: &[Binding],
) -> Result<Vec<Binding>, EvalError> {
    let k = required_non_negative_int_arg(aggregate, "k", base)?;
    let candidates = top_k_candidates(aggregate, rows, usize::try_from(k).unwrap_or(usize::MAX))?;
    candidates
        .into_iter()
        .filter_map(|candidate| {
            bind_aggregate_result(&aggregate.result, base, &candidate.value).transpose()
        })
        .collect()
}

fn eval_rank_aggregate(
    aggregate: &Aggregate,
    base: &Binding,
    rows: &[Binding],
) -> Result<Vec<Binding>, EvalError> {
    let rank_var = required_rank_var_arg(aggregate)?;
    let candidates = rank_candidates(aggregate, rows)?;
    let mut out = Vec::new();
    let mut current_rank = 0_i64;
    let mut previous_key = None;
    for candidate in candidates {
        if previous_key.as_ref() != Some(&candidate.key) {
            current_rank += 1;
            previous_key = Some(candidate.key.clone());
        }
        let mut row = candidate.row;
        row.insert(
            rank_var.clone(),
            Value::Number(NumberValue::Int(current_rank)),
        );
        let value = eval_expr(&aggregate.value, &row)?;
        if let Some(binding) = bind_aggregate_result(&aggregate.result, base, &value)? {
            out.push(binding);
        }
    }
    Ok(out)
}

fn eval_take_until_aggregate(
    aggregate: &Aggregate,
    base: &Binding,
    rows: &[Binding],
) -> Result<Vec<Binding>, EvalError> {
    let budget = required_non_negative_int_arg(aggregate, "budget", base)?;
    let sum = required_aggregate_arg(aggregate, "sum")?;
    let key = required_aggregate_arg(aggregate, "key")?;
    let mut candidates = rows
        .iter()
        .map(|row| {
            let cost = eval_expr(&sum.expr, row)?;
            let NumberValue::Int(cost) = numeric_value(&cost)? else {
                return Err(EvalError::InvalidAggregateArg {
                    function: aggregate.function,
                    argument: "sum",
                });
            };
            if cost < 0 {
                return Err(EvalError::InvalidAggregateArg {
                    function: aggregate.function,
                    argument: "sum",
                });
            }
            Ok((
                eval_expr(&key.expr, row)?,
                eval_expr(&aggregate.value, row)?,
                cost,
            ))
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut out = Vec::new();
    let mut used = 0_i64;
    for (_, value, cost) in candidates {
        let next = used.saturating_add(cost);
        if next > budget {
            break;
        }
        used = next;
        if let Some(binding) = bind_aggregate_result(&aggregate.result, base, &value)? {
            out.push(binding);
        }
    }
    Ok(out)
}

fn bind_aggregate_result(
    result: &Expr,
    base: &Binding,
    value: &Value,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    if !unify_expr(result, value, base, &mut next)? {
        return Ok(None);
    }
    Ok(Some(next.unwrap_or_else(|| base.clone())))
}

fn validate_aggregate_args(aggregate: &Aggregate) -> Result<(), EvalError> {
    let allowed = match aggregate.function {
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::Avg
        | AggregateFunction::List
        | AggregateFunction::Set => &[][..],
        AggregateFunction::TopK => &["k", "key"][..],
        AggregateFunction::Rank => &["key", "rank"][..],
        AggregateFunction::TakeUntil => &["budget", "sum", "key"][..],
    };
    let mut seen = BTreeSet::new();
    for arg in &aggregate.args {
        if !allowed.contains(&arg.name.as_str()) {
            return Err(EvalError::InvalidAggregateArg {
                function: aggregate.function,
                argument: "unknown",
            });
        }
        if !seen.insert(arg.name.as_str()) {
            return Err(EvalError::InvalidAggregateArg {
                function: aggregate.function,
                argument: "duplicate",
            });
        }
    }
    Ok(())
}

fn required_aggregate_arg<'a>(
    aggregate: &'a Aggregate,
    name: &'static str,
) -> Result<&'a crate::runtime::ast::NamedArg, EvalError> {
    aggregate
        .args
        .iter()
        .find(|arg| arg.name.as_str() == name)
        .ok_or(EvalError::MissingAggregateArg {
            function: aggregate.function,
            argument: name,
        })
}

fn required_non_negative_int_arg(
    aggregate: &Aggregate,
    name: &'static str,
    binding: &Binding,
) -> Result<i64, EvalError> {
    let value = eval_expr(&required_aggregate_arg(aggregate, name)?.expr, binding)?;
    let Value::Number(NumberValue::Int(value)) = value else {
        return Err(EvalError::InvalidAggregateArg {
            function: aggregate.function,
            argument: name,
        });
    };
    if value < 0 {
        return Err(EvalError::InvalidAggregateArg {
            function: aggregate.function,
            argument: name,
        });
    }
    Ok(value)
}

fn required_rank_var_arg(aggregate: &Aggregate) -> Result<Ident, EvalError> {
    let Expr::Var(var) = &required_aggregate_arg(aggregate, "rank")?.expr else {
        return Err(EvalError::InvalidAggregateArg {
            function: aggregate.function,
            argument: "rank",
        });
    };
    Ok(var.clone())
}

fn stored_constraints(
    fields: &[FieldPattern],
    binding: &Binding,
) -> Result<Vec<(Ident, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for field in fields {
        if let Some(value) = bound_value_for_term(&field.term, binding)? {
            constraints.push((field.field.clone(), value));
        }
    }
    Ok(constraints)
}

fn call_constraints(args: &[CallArg], binding: &Binding) -> Result<Vec<(usize, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for (idx, arg) in args.iter().enumerate() {
        if let Some(value) = bound_value_for_expr(arg.expr(), binding)? {
            constraints.push((idx, value));
        }
    }
    Ok(constraints)
}

fn bound_value_for_term(term: &Term, binding: &Binding) -> Result<Option<Value>, EvalError> {
    match term {
        Term::Wildcard => Ok(None),
        Term::Expr(expr) => bound_value_for_expr(expr, binding),
    }
}

fn bound_value_for_expr(expr: &Expr, binding: &Binding) -> Result<Option<Value>, EvalError> {
    match expr {
        Expr::Var(var) => Ok(binding.get(var).cloned()),
        _ if expr_is_bound(expr, binding) => eval_expr(expr, binding).map(Some),
        _ => Ok(None),
    }
}

fn expr_is_bound(expr: &Expr, binding: &Binding) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    vars.iter().all(|var| binding.contains_key(var))
}

fn unify_stored_fields(
    fields: &[FieldPattern],
    row: &NamedRow,
    binding: &Binding,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    for field in fields {
        let Some(value) = row.get(&field.field) else {
            return Ok(None);
        };
        if !unify_term(&field.term, value, binding, &mut next)? {
            return Ok(None);
        }
    }
    Ok(Some(next.unwrap_or_else(|| binding.clone())))
}

fn unify_call_args(
    args: &[CallArg],
    tuple: &Tuple,
    binding: &Binding,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    for (arg, value) in args.iter().zip(&tuple.0) {
        if !unify_expr(arg.expr(), value, binding, &mut next)? {
            return Ok(None);
        }
    }
    Ok(Some(next.unwrap_or_else(|| binding.clone())))
}

fn unify_term(
    term: &Term,
    value: &Value,
    binding: &Binding,
    next: &mut Option<Binding>,
) -> Result<bool, EvalError> {
    match term {
        Term::Wildcard => Ok(true),
        Term::Expr(expr) => unify_expr(expr, value, binding, next),
    }
}

fn unify_expr(
    expr: &Expr,
    value: &Value,
    binding: &Binding,
    next: &mut Option<Binding>,
) -> Result<bool, EvalError> {
    match expr {
        Expr::Var(var) => {
            if let Some(existing) = active_binding(binding, next.as_ref()).get(var) {
                Ok(existing == value)
            } else {
                writable_binding(binding, next).insert(var.clone(), value.clone());
                Ok(true)
            }
        }
        Expr::Tuple(items) => {
            let Value::List(values) = value else {
                return Ok(false);
            };
            if items.len() != values.len() {
                return Ok(false);
            }
            for (item, value) in items.iter().zip(values) {
                if !unify_expr(item, value, binding, next)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(eval_expr(expr, active_binding(binding, next.as_ref()))? == *value),
    }
}

fn active_binding<'a>(binding: &'a Binding, next: Option<&'a Binding>) -> &'a Binding {
    next.map_or(binding, |next| next)
}

fn writable_binding<'a>(binding: &Binding, next: &'a mut Option<Binding>) -> &'a mut Binding {
    next.get_or_insert_with(|| binding.clone())
}

fn project_head(head: &Head, binding: &Binding) -> Result<Tuple, EvalError> {
    let mut values = Vec::with_capacity(head.terms.len());
    for term in &head.terms {
        match term {
            Term::Wildcard => values.push(Value::Null),
            Term::Expr(expr) => values.push(eval_expr(expr, binding)?),
        }
    }
    Ok(Tuple(values))
}

fn project_fact_head(head: &Head) -> Result<Tuple, EvalError> {
    project_head(head, &Binding::new())
}

fn eval_expr(expr: &Expr, binding: &Binding) -> Result<Value, EvalError> {
    match expr {
        Expr::Var(var) => binding
            .get(var)
            .cloned()
            .ok_or_else(|| EvalError::UnboundVariable {
                variable: var.clone(),
            }),
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::Binary { left, op, right } => eval_binary(left, *op, right, binding),
        Expr::Tuple(items) => items
            .iter()
            .map(|item| eval_expr(item, binding))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Expr::FunctionCall { .. } => Err(EvalError::UnsupportedExpression),
    }
}

fn eval_binary(
    left: &Expr,
    op: crate::runtime::ast::ArithmeticOp,
    right: &Expr,
    binding: &Binding,
) -> Result<Value, EvalError> {
    let left = eval_expr(left, binding)?;
    let right = eval_expr(right, binding)?;
    let (Value::Number(left), Value::Number(right)) = (left, right) else {
        return Err(EvalError::UnsupportedExpression);
    };
    let (NumberValue::Int(left), NumberValue::Int(right)) = (left, right) else {
        return Err(EvalError::UnsupportedExpression);
    };
    let value = match op {
        crate::runtime::ast::ArithmeticOp::Add => left + right,
        crate::runtime::ast::ArithmeticOp::Sub => left - right,
        crate::runtime::ast::ArithmeticOp::Mul => left * right,
        crate::runtime::ast::ArithmeticOp::Div => {
            if right == 0 {
                return Err(EvalError::DivisionByZero);
            }
            left / right
        }
        crate::runtime::ast::ArithmeticOp::Rem => {
            if right == 0 {
                return Err(EvalError::DivisionByZero);
            }
            left % right
        }
    };
    Ok(Value::Number(NumberValue::Int(value)))
}

fn value_from_literal(literal: &Literal) -> Value {
    match literal {
        Literal::String(value) => Value::String(value.clone()),
        Literal::Number(NumberLiteral::Int(value)) => Value::Number(NumberValue::Int(*value)),
        Literal::Number(NumberLiteral::Float(value)) => Value::Number(NumberValue::Float(*value)),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Null => Value::Null,
        Literal::List(items) => Value::List(items.iter().map(value_from_literal).collect()),
    }
}

fn compare(left: &Value, op: ComparisonOp, right: &Value) -> Result<bool, EvalError> {
    let result = match op {
        ComparisonOp::Eq => left == right,
        ComparisonOp::Ne => left != right,
        ComparisonOp::Lt => left < right,
        ComparisonOp::Gt => left > right,
        ComparisonOp::Le => left <= right,
        ComparisonOp::Ge => left >= right,
        ComparisonOp::In => match right {
            Value::List(items) => items.contains(left),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::Contains => match (left, right) {
            (Value::String(haystack), Value::String(needle)) => haystack.contains(needle),
            (Value::List(items), needle) => items.contains(needle),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::StartsWith => match (left, right) {
            (Value::String(value), Value::String(prefix)) => value.starts_with(prefix),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::EndsWith => match (left, right) {
            (Value::String(value), Value::String(suffix)) => value.ends_with(suffix),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::Matches => return Err(EvalError::UnsupportedExpression),
    };
    Ok(result)
}

fn binding_to_row(binding: Binding) -> Row {
    Row {
        fields: binding
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    }
}

fn named_row(entries: impl IntoIterator<Item = (&'static str, Value)>) -> NamedRow {
    entries
        .into_iter()
        .map(|(key, value)| (Ident::new_unchecked(key), value))
        .collect()
}

fn source_fact_row(
    identity: &FactIdentity,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) -> NamedRow {
    let mut row = identity_row(identity);
    row.extend(named_row(entries));
    row
}

fn identity_row(identity: &FactIdentity) -> NamedRow {
    named_row([
        ("corpus", Value::String(identity.corpus.to_string())),
        ("source", Value::String(identity.source.to_string())),
        ("native_id", Value::String(identity.native_id.to_string())),
        ("origin_uri", Value::String(identity.origin_uri.to_string())),
        ("revision", Value::String(identity.revision.to_string())),
        ("generation", generation_value(identity.generation)),
    ])
}

fn opt_string(value: Option<&String>) -> Value {
    value.cloned().map_or(Value::Null, Value::String)
}

fn handle_row(fact: &HandleFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("id", Value::String(fact.id.clone())),
            ("kind", Value::String(fact.kind.clone())),
            ("status", opt_string(fact.status.as_ref())),
            ("namespace", Value::String(fact.namespace.clone())),
            ("file", Value::String(fact.file.clone())),
            (
                "line",
                Value::Number(NumberValue::Int(i64::from(fact.line))),
            ),
            ("date", opt_string(fact.date.as_ref())),
            ("area", Value::String(fact.area.clone())),
            ("summary", Value::String(fact.summary.clone())),
        ],
    )
}

fn edge_row(fact: &EdgeFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("from", Value::String(fact.from.clone())),
            ("to", Value::String(fact.to.clone())),
            ("kind", Value::String(fact.kind.clone())),
            ("file", Value::String(fact.file.clone())),
            (
                "line",
                Value::Number(NumberValue::Int(i64::from(fact.line))),
            ),
        ],
    )
}

fn meta_row(fact: &MetaFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("handle", Value::String(fact.handle.clone())),
            ("key", Value::String(fact.key.clone())),
            ("value", Value::String(fact.value.clone())),
        ],
    )
}

fn content_row(fact: &ContentFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("handle", Value::String(fact.handle.clone())),
            ("span_id", Value::String(fact.span_id.clone())),
            (
                "lines",
                Value::Number(NumberValue::Int(i64::from(fact.lines))),
            ),
            ("text", Value::String(fact.text.clone())),
            (
                "tokens",
                Value::Number(NumberValue::Int(i64::from(fact.tokens))),
            ),
        ],
    )
}

fn span_row(fact: &SpanFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("id", Value::String(fact.id.clone())),
            ("handle", Value::String(fact.handle.clone())),
            (
                "start_line",
                Value::Number(NumberValue::Int(i64::from(fact.start_line))),
            ),
            (
                "end_line",
                Value::Number(NumberValue::Int(i64::from(fact.end_line))),
            ),
            ("summary", Value::String(fact.summary.clone())),
        ],
    )
}

fn concern_row(fact: &ConcernFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("name", Value::String(fact.name.clone())),
            ("member", Value::String(fact.member.clone())),
        ],
    )
}

fn config_row(fact: &ConfigFact) -> NamedRow {
    named_row([
        ("corpus", Value::String(fact.corpus.to_string())),
        ("key", Value::String(fact.key.clone())),
        ("value", Value::String(fact.value.clone())),
        (
            "ordinal",
            fact.ordinal.map_or(Value::Null, |ordinal| {
                Value::Number(NumberValue::Int(i64::from(ordinal)))
            }),
        ),
    ])
}

fn snapshot_row(fact: &SnapshotFact) -> NamedRow {
    named_row([
        ("corpus", Value::String(fact.corpus.to_string())),
        ("snapshot", Value::String(fact.snapshot.clone())),
        ("at", Value::String(fact.at.clone())),
        ("id", Value::String(fact.id.clone())),
        ("key", Value::String(fact.key.clone())),
        ("value", Value::String(fact.value.clone())),
    ])
}

fn generation_value(generation: Generation) -> Value {
    Value::Number(NumberValue::Int(
        i64::try_from(generation.get()).unwrap_or(i64::MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::sync::OnceLock;

    use crate::facts::{FactBatch, FactBatchMode, FactIdentity, SnapshotFact};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::runtime::ast::{RuleLayer, Statement};
    use crate::runtime::{StaticError, analyze, parse_program};

    fn identity(native_id: &str) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            NativeId::from(native_id),
            OriginUri::from(format!("fixture://{native_id}")),
            Revision::from("rev"),
            Generation::initial(),
        )
    }

    fn handle(id: &str, kind: &str, status: &str, namespace: &str, area: &str) -> HandleFact {
        handle_with_options(id, kind, Some(status), namespace, area, None)
    }

    fn handle_with_options(
        id: &str,
        kind: &str,
        status: Option<&str>,
        namespace: &str,
        area: &str,
        date: Option<&str>,
    ) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(str::to_string),
            namespace: namespace.to_string(),
            file: format!("{area}/{id}.md"),
            line: 1,
            date: date.map(str::to_string),
            area: area.to_string(),
            summary: String::new(),
        }
    }

    fn handle_with_summary(
        id: &str,
        kind: &str,
        status: &str,
        namespace: &str,
        area: &str,
        summary: &str,
    ) -> HandleFact {
        let mut handle = handle(id, kind, status, namespace, area);
        handle.summary = summary.to_string();
        handle
    }

    fn edge(from: &str, to: &str, kind: &str) -> EdgeFact {
        EdgeFact {
            identity: identity(&format!("{from}->{to}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: "fixture.md".to_string(),
            line: 1,
        }
    }

    fn meta(handle: &str, key: &str, value: &str) -> MetaFact {
        MetaFact {
            identity: identity(&format!("{handle}:meta:{key}")),
            handle: handle.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn content(handle: &str, span_id: &str, tokens: u32) -> ContentFact {
        content_with_text(handle, span_id, "", tokens)
    }

    fn content_with_text(handle: &str, span_id: &str, text: &str, tokens: u32) -> ContentFact {
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
            identity: identity(&format!("{handle}#{span_id}:span")),
            id: span_id.to_string(),
            handle: handle.to_string(),
            start_line,
            end_line,
            summary: String::new(),
        }
    }

    fn config(key: &str, value: &str) -> ConfigFact {
        ConfigFact {
            corpus: CorpusId::from("test"),
            key: key.to_string(),
            value: value.to_string(),
            ordinal: None,
        }
    }

    fn ordered_config(key: &str, value: &str, ordinal: u32) -> ConfigFact {
        ConfigFact {
            corpus: CorpusId::from("test"),
            key: key.to_string(),
            value: value.to_string(),
            ordinal: Some(ordinal),
        }
    }

    fn snapshot_fact(snapshot: &str, at: &str, id: &str, key: &str, value: &str) -> SnapshotFact {
        SnapshotFact {
            corpus: CorpusId::from("test"),
            snapshot: snapshot.to_string(),
            at: at.to_string(),
            id: id.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn chain_store(edge_count: usize) -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.edges = (0..edge_count)
            .map(|idx| edge(&format!("n{idx}"), &format!("n{}", idx + 1), "DependsOn"))
            .collect();
        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merge");
        store
    }

    fn fixture_store() -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("v17", "file", "current", "", "formal-model"),
            handle("v16", "file", "superseded", "", "formal-model"),
            handle("jit", "file", "draft", "", "compiler"),
            handle("OQ-22", "label", "open", "OQ", "formal-model"),
            handle("OQ-99", "label", "resolved", "OQ", "compiler"),
        ];
        batch.edges = vec![
            edge("v17", "v16", "Supersedes"),
            edge("jit", "OQ-22", "DependsOn"),
            edge("v17", "OQ-22", "DependsOn"),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merge");
        store
    }

    fn lifecycle_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle_with_options("raw.md", "file", Some("raw"), "", "core", None),
            handle_with_options(
                "draft.md",
                "file",
                Some("draft"),
                "",
                "core",
                Some("9999-01-01"),
            ),
            handle_with_options("done.md", "file", Some("done"), "", "core", None),
            handle_with_options("stable.md", "file", Some("stable"), "", "core", None),
            handle_with_options("nostatus.md", "file", None, "", "core", None),
            handle_with_options("OQ-1", "label", Some("open"), "OQ", "core", None),
            handle_with_options("OQ-2", "label", Some("open"), "OQ", "core", None),
        ];
        batch.edges = vec![edge("doc.md", "OQ-1", "Discharges")];
        batch.content = vec![
            content("draft.md", "draft-1", 10),
            content("draft.md", "draft-2", 15),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("lifecycle fixture merge");
        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![
                    config("convergence.active", "draft"),
                    config("convergence.terminal", "done"),
                    config("convergence.settled", "stable"),
                    ordered_config("convergence.ordering", "stable", 2),
                    ordered_config("convergence.ordering", "raw", 0),
                    ordered_config("convergence.ordering", "draft", 1),
                    config("handles.linear", "OQ"),
                ],
            )
            .expect("lifecycle config replace");

        let mut database = Database::from_store(&store);
        database.insert_stored_rows(
            "snapshot",
            [
                named_row([
                    ("id", s("draft.md")),
                    ("key", s("status")),
                    ("value", s("raw")),
                    ("at", s("1970-01-01")),
                    ("corpus", s("test")),
                ]),
                named_row([
                    ("id", s("draft.md")),
                    ("key", s("status")),
                    ("value", s("draft")),
                    ("at", s("1970-01-02")),
                    ("corpus", s("test")),
                ]),
            ],
        );
        database
    }

    fn content_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("alpha.md", "file", "current", "", "core"),
            handle("beta.md", "file", "current", "", "core"),
        ];
        batch.content = vec![
            content_with_text("alpha.md", "shared", "intro line", 4),
            content_with_text("alpha.md", "middle", "urgent middle\nplain tail", 5),
            content_with_text("alpha.md", "late", "final urgent", 8),
            content_with_text("beta.md", "shared", "beta urgent", 3),
        ];
        batch.spans = vec![
            span("alpha.md", "late", 30, 32),
            span("alpha.md", "shared", 1, 1),
            span("alpha.md", "middle", 10, 12),
            span("beta.md", "shared", 2, 2),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("content fixture merge");
        Database::from_store(&store)
    }

    fn search_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle_with_summary(
                "audit/v17.md",
                "file",
                "current",
                "",
                "audit",
                "V17 conformance audit",
            ),
            handle_with_summary(
                "notes/release.md",
                "file",
                "current",
                "",
                "notes",
                "Release readiness notes",
            ),
            handle_with_summary(
                "notes/tie-a.md",
                "file",
                "current",
                "",
                "notes",
                "Same topic",
            ),
            handle_with_summary(
                "notes/tie-b.md",
                "file",
                "current",
                "",
                "notes",
                "Same topic",
            ),
        ];
        batch.meta = vec![
            meta("audit/v17.md", "concern", "C-conformance"),
            meta("notes/release.md", "concern", "C-release"),
        ];
        batch.content = vec![
            content_with_text(
                "audit/v17.md",
                "body",
                "urgent blocker list for conformance gaps",
                6,
            ),
            content_with_text(
                "notes/release.md",
                "body",
                "packaging checklist and smoke test notes",
                6,
            ),
        ];
        batch.spans = vec![
            span("audit/v17.md", "body", 10, 11),
            span("notes/release.md", "body", 3, 4),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("search fixture merge");
        Database::from_store(&store)
    }

    fn time_travel_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("draft.md", "file", "current", "", "core"),
            handle("plan.md", "file", "plan", "", "core"),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("time travel fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "draft.md", "status", "draft"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "plan.md", "status", "plan"),
                ],
            )
            .expect("replace snapshots");
        Database::from_store(&store)
    }

    fn tie_time_travel_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![handle("draft.md", "file", "current", "", "core")];
        let mut store = FactStore::default();
        store.merge(batch).expect("tie fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "2026-05-09T00:00:00Z", "draft.md", "status", "draft"),
                ],
            )
            .expect("replace snapshots");
        Database::from_store(&store)
    }

    fn same_day_flux_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![handle("draft.md", "file", "current", "", "core")];
        let mut store = FactStore::default();
        store.merge(batch).expect("same-day fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "1970-01-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "1970-01-01T12:00:00Z", "draft.md", "status", "draft"),
                    snapshot_fact(
                        "s3",
                        "1970-01-01T18:00:00Z",
                        "draft.md",
                        "status",
                        "current",
                    ),
                ],
            )
            .expect("replace snapshots");
        Database::from_store(&store)
    }

    fn mvs_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            mvs_handle(
                "formal-model/v17.md",
                "file",
                "authoritative",
                "",
                "formal-model/v17.md",
                "formal-model",
                Some("2026-03-25"),
            ),
            mvs_handle(
                "formal-model/v16.md",
                "file",
                "superseded",
                "",
                "formal-model/v16.md",
                "formal-model",
                Some("2026-03-10"),
            ),
            mvs_handle(
                "formal-model/v15.md",
                "file",
                "superseded",
                "",
                "formal-model/v15.md",
                "formal-model",
                Some("2026-02-15"),
            ),
            mvs_handle(
                "formal-model/v14.md",
                "file",
                "superseded",
                "",
                "formal-model/v14.md",
                "formal-model",
                Some("2026-02-01"),
            ),
            mvs_handle(
                "compiler/jit-spec.md",
                "file",
                "draft",
                "",
                "compiler/jit-spec.md",
                "compiler",
                Some("2026-04-10"),
            ),
            mvs_handle(
                "compiler/jit-stale.md",
                "file",
                "superseded",
                "",
                "compiler/jit-stale.md",
                "compiler",
                Some("2026-02-20"),
            ),
            mvs_handle(
                "compiler/exec.md",
                "file",
                "current",
                "",
                "compiler/exec.md",
                "compiler",
                Some("2026-04-22"),
            ),
            mvs_handle(
                "research-log/2026-04-jit.md",
                "file",
                "research",
                "",
                "research-log/2026-04-jit.md",
                "research-log",
                Some("2026-04-29"),
            ),
            mvs_handle(
                "synthesis/2026-04-discharge.md",
                "file",
                "current",
                "",
                "synthesis/2026-04-discharge.md",
                "synthesis",
                Some("2026-04-15"),
            ),
            mvs_handle(
                "OQ-22",
                "label",
                "open",
                "OQ",
                "formal-model/v17.md",
                "formal-model",
                None,
            ),
            mvs_handle(
                "OQ-23",
                "label",
                "open",
                "OQ",
                "formal-model/v17.md",
                "formal-model",
                None,
            ),
            mvs_handle(
                "OQ-60",
                "label",
                "open",
                "OQ",
                "compiler/jit-spec.md",
                "compiler",
                None,
            ),
            mvs_handle(
                "OQ-77",
                "label",
                "open",
                "OQ",
                "research-log/2026-04-jit.md",
                "research-log",
                None,
            ),
            mvs_handle(
                "OQ-88",
                "label",
                "open",
                "OQ",
                "compiler/jit-spec.md",
                "compiler",
                None,
            ),
            mvs_handle(
                "OQ-99",
                "label",
                "resolved",
                "OQ",
                "formal-model/v16.md",
                "formal-model",
                None,
            ),
        ];
        batch.edges = vec![
            mvs_edge(
                "formal-model/v17.md",
                "OQ-22",
                "DependsOn",
                "formal-model/v17.md",
                14,
            ),
            mvs_edge(
                "formal-model/v17.md",
                "OQ-23",
                "DependsOn",
                "formal-model/v17.md",
                14,
            ),
            mvs_edge(
                "formal-model/v17.md",
                "OQ-60",
                "DependsOn",
                "formal-model/v17.md",
                18,
            ),
            mvs_edge(
                "formal-model/v17.md",
                "formal-model/v16.md",
                "Supersedes",
                "formal-model/v17.md",
                6,
            ),
            mvs_edge(
                "formal-model/v16.md",
                "formal-model/v15.md",
                "Supersedes",
                "formal-model/v16.md",
                6,
            ),
            mvs_edge(
                "formal-model/v15.md",
                "formal-model/v14.md",
                "Supersedes",
                "formal-model/v15.md",
                6,
            ),
            mvs_edge(
                "compiler/jit-spec.md",
                "OQ-22",
                "DependsOn",
                "compiler/jit-spec.md",
                22,
            ),
            mvs_edge(
                "compiler/jit-spec.md",
                "compiler/jit-stale.md",
                "DependsOn",
                "compiler/jit-spec.md",
                30,
            ),
            mvs_edge(
                "compiler/exec.md",
                "compiler/jit-spec.md",
                "DependsOn",
                "compiler/exec.md",
                8,
            ),
            mvs_edge(
                "research-log/2026-04-jit.md",
                "formal-model/v17.md",
                "Cites",
                "research-log/2026-04-jit.md",
                3,
            ),
            mvs_edge(
                "synthesis/2026-04-discharge.md",
                "OQ-77",
                "Discharges",
                "synthesis/2026-04-discharge.md",
                12,
            ),
            mvs_edge(
                "compiler/jit-spec.md",
                "OQ-22",
                "Verifies",
                "compiler/jit-spec.md",
                44,
            ),
        ];

        let mut store = FactStore::default();
        store.merge(batch).expect("mvs fixture merge");
        let mut database = Database::from_store(&store);
        database.insert_stored_rows(
            "pending_edge",
            [named_row([
                ("from", s("compiler/jit-spec.md")),
                ("target", s("OQ-9999")),
                ("kind", s("DependsOn")),
                ("file", s("compiler/jit-spec.md")),
                ("line", n(51)),
            ])],
        );
        database.insert_stored_rows("linear_namespace", [named_row([("namespace", s("OQ"))])]);
        database
    }

    fn mvs_handle(
        id: &str,
        kind: &str,
        status: &str,
        namespace: &str,
        file: &str,
        area: &str,
        date: Option<&str>,
    ) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: Some(status.to_string()),
            namespace: namespace.to_string(),
            file: file.to_string(),
            line: 1,
            date: date.map(str::to_string),
            area: area.to_string(),
            summary: String::new(),
        }
    }

    fn mvs_edge(from: &str, to: &str, kind: &str, file: &str, line: u32) -> EdgeFact {
        EdgeFact {
            identity: identity(&format!("{from}->{to}:{kind}:{line}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: file.to_string(),
            line,
        }
    }

    type QueryRows = Vec<BTreeMap<String, Value>>;

    #[derive(Debug)]
    struct MvsOutputs {
        handles: QueryRows,
        release_blockers: QueryRows,
        supersedes_chain: QueryRows,
        open_oqs: QueryRows,
        oq_pressure: QueryRows,
        oq_per_area: QueryRows,
    }

    fn mvs_outputs() -> &'static MvsOutputs {
        static OUTPUTS: OnceLock<MvsOutputs> = OnceLock::new();
        OUTPUTS.get_or_init(compute_mvs_outputs)
    }

    fn compute_mvs_outputs() -> MvsOutputs {
        let mut program = parse_program(
            "mvs.dl",
            r#"
            terminal(h) := *handle{id: h, status: "superseded"}.
            terminal(h) := *handle{id: h, status: "resolved"}.
            active(h) := *handle{id: h}, not terminal(h).
            settled(h) := *handle{id: h, status: "authoritative"}.
            settled(h) := *handle{id: h, status: "current"}.

            supersedes_chain(s, t, 1) := *edge{from: s, to: t, kind: "Supersedes"}.
            supersedes_chain(s, t, d + 1) :=
              *edge{from: s, to: mid, kind: "Supersedes"},
              supersedes_chain(mid, t, d).

            obligation(h) :=
              *handle{id: h, kind: "label", namespace: ns},
              *linear_namespace{namespace: ns}.
            discharged(h) := *edge{to: h, kind: "Discharges"}.
            undischarged(h) := obligation(h), not discharged(h), not terminal(h).

            diagnostic("E001", "error", src, file, line) :=
              *pending_edge{from: src, target: target, file: file, line: line},
              not *handle{id: target}.
            diagnostic("E002", "error", h, file, 1) :=
              undischarged(h),
              *handle{id: h, file: file}.
            diagnostic("W001", "warning", src, file, line) :=
              *edge{from: src, to: target, kind: "DependsOn", file: file, line: line},
              active(src),
              terminal(target).

            release_blocker(h, "broken_ref", file, line, null) :=
              diagnostic("E001", severity, h, file, line).
            release_blocker(h, "undischarged", null, null, null) :=
              diagnostic("E002", severity, h, file, line).
            release_blocker(h, "stale_dep", null, null, target) :=
              *edge{from: h, to: target, kind: "DependsOn"},
              active(h),
              terminal(target).

            open_oq(q) :=
              *handle{id: q, kind: "label", namespace: "OQ"},
              not terminal(q).
            downstream_settled(q, x) :=
              open_oq(q),
              *edge{from: x, to: q, kind: "DependsOn"},
              settled(x).
            oq_pressure(q, n) :=
              open_oq(q),
              n = Count{ x : downstream_settled(q, x) }.
            oq_in_area(area, q) :=
              *handle{id: q, kind: "label", namespace: "OQ", area: area},
              not terminal(q).
            oq_area(area) := *handle{kind: "label", namespace: "OQ", area: area}.
            oq_per_area(area, n) :=
              oq_area(area),
              n = Count{ q : oq_in_area(area, q) }.

            ? *handle{id, kind, status, namespace, area}.
            ? release_blocker(h, kind, file, line, target).
            ? supersedes_chain(start, target, depth), start = "formal-model/v17.md".
            ? open_oq(q).
            ? oq_pressure(q, n).
            ? oq_per_area(area, n).
            "#,
        )
        .expect("mvs program parses");
        mark_prelude(&mut program);
        let analyzed = analyze(program).expect("mvs program analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, mvs_database());
        evaluator.run_fixpoint().expect("mvs fixpoint");
        let mut rows = queries
            .iter()
            .map(|query| {
                let mut rows = evaluator
                    .eval_query(query)
                    .expect("mvs query evaluates")
                    .rows
                    .into_iter()
                    .map(|row| row.fields)
                    .collect::<Vec<_>>();
                rows.sort();
                rows
            })
            .collect::<Vec<_>>()
            .into_iter();
        let outputs = MvsOutputs {
            handles: rows.next().expect("mvs-1 query output"),
            release_blockers: rows.next().expect("mvs-2 query output"),
            supersedes_chain: rows.next().expect("mvs-3 query output"),
            open_oqs: rows.next().expect("mvs-4 query output"),
            oq_pressure: rows.next().expect("mvs-5a query output"),
            oq_per_area: rows.next().expect("mvs-5b query output"),
        };
        assert!(rows.next().is_none(), "unexpected extra mvs query output");
        outputs
    }

    fn mark_prelude(program: &mut crate::runtime::Program) {
        for statement in &mut program.statements {
            mark_statement_prelude(statement);
        }
    }

    fn mark_statement_prelude(statement: &mut Statement) {
        match statement {
            Statement::Rule(rule) => rule.origin.layer = RuleLayer::Prelude,
            Statement::Query(query) => {
                for rule in &mut query.local_rules {
                    rule.origin.layer = RuleLayer::Inline;
                }
            }
            Statement::AtBlock { statements, .. } => {
                for statement in statements {
                    mark_statement_prelude(statement);
                }
            }
            Statement::Fact(_)
            | Statement::Include(_)
            | Statement::Import(_)
            | Statement::Verb(_) => {}
        }
    }

    fn row(entries: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    fn assert_query_rows(
        actual: &[BTreeMap<String, Value>],
        mut expected: Vec<BTreeMap<String, Value>>,
    ) {
        expected.sort();
        assert_eq!(actual, expected.as_slice());
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

    fn value_f64(value: &Value) -> f64 {
        match value {
            Value::Number(NumberValue::Float(value)) => *value,
            other => panic!("expected float value, got {other:?}"),
        }
    }

    fn list(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    fn evaluate_queries(input: &str, database: Database) -> Vec<QueryRows> {
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        queries
            .iter()
            .map(|query| {
                let mut rows = evaluator
                    .eval_query(query)
                    .expect("query evaluates")
                    .rows
                    .into_iter()
                    .map(|row| row.fields)
                    .collect::<Vec<_>>();
                rows.sort();
                rows
            })
            .collect()
    }

    fn evaluate_query_output(input: &str, database: Database) -> QueryOutput {
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect("query evaluates")
    }

    fn evaluate_query_error(input: &str, database: Database) -> EvalError {
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect_err("query errors")
    }

    fn evaluate_query_output_with_options(
        input: &str,
        database: Database,
        options: EvalOptions,
    ) -> QueryOutput {
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect("query evaluates")
    }

    fn evaluate_query_error_with_options(
        input: &str,
        database: Database,
        options: EvalOptions,
    ) -> EvalError {
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator.run_fixpoint().expect("fixpoint evaluates");
        evaluator.eval_query(&query).expect_err("query errors")
    }

    #[test]
    fn graph_primitives_traverse_edges_relationally() {
        let outputs = evaluate_queries(
            r#"
            ? upstream("formal-model/v17.md", anc).
            ? downstream("OQ-22", desc).
            ? impact("OQ-22", x, depth).
            ? neighborhood("OQ-22", depth, member), depth <= 1.
            ? in_degree("OQ-22", n).
            ? out_degree("formal-model/v17.md", n).
            ? cite_count("formal-model/v17.md", n).
            ? cite_count("OQ-22", n).
            "#,
            mvs_database(),
        );

        let mut rows = outputs.into_iter();
        assert_query_rows(
            &rows.next().expect("upstream output"),
            vec![
                row([("anc", s("OQ-22"))]),
                row([("anc", s("OQ-23"))]),
                row([("anc", s("OQ-60"))]),
                row([("anc", s("formal-model/v14.md"))]),
                row([("anc", s("formal-model/v15.md"))]),
                row([("anc", s("formal-model/v16.md"))]),
            ],
        );
        assert_query_rows(
            &rows.next().expect("downstream output"),
            vec![
                row([("desc", s("compiler/exec.md"))]),
                row([("desc", s("compiler/jit-spec.md"))]),
                row([("desc", s("formal-model/v17.md"))]),
                row([("desc", s("research-log/2026-04-jit.md"))]),
            ],
        );
        assert_query_rows(
            &rows.next().expect("impact output"),
            vec![
                row([("depth", n(1)), ("x", s("compiler/jit-spec.md"))]),
                row([("depth", n(1)), ("x", s("formal-model/v17.md"))]),
                row([("depth", n(2)), ("x", s("compiler/exec.md"))]),
                row([("depth", n(2)), ("x", s("research-log/2026-04-jit.md"))]),
            ],
        );
        assert_query_rows(
            &rows.next().expect("neighborhood output"),
            vec![
                row([("depth", n(0)), ("member", s("OQ-22"))]),
                row([("depth", n(1)), ("member", s("compiler/jit-spec.md"))]),
                row([("depth", n(1)), ("member", s("formal-model/v17.md"))]),
            ],
        );
        assert_query_rows(
            &rows.next().expect("in_degree output"),
            vec![row([("n", n(3))])],
        );
        assert_query_rows(
            &rows.next().expect("out_degree output"),
            vec![row([("n", n(4))])],
        );
        assert_query_rows(
            &rows.next().expect("cite_count v17 output"),
            vec![row([("n", n(1))])],
        );
        assert_query_rows(
            &rows.next().expect("cite_count oq output"),
            vec![row([("n", n(0))])],
        );
        assert!(rows.next().is_none(), "unexpected extra primitive output");
    }

    #[test]
    fn read_returns_spans_in_source_order_within_budget() {
        let output = evaluate_query_output(
            r#"? read("alpha.md", 9, span_id, text, start_line, end_line, tokens)."#,
            content_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![
                row([
                    ("span_id", s("shared")),
                    ("text", s("intro line")),
                    ("start_line", n(1)),
                    ("end_line", n(1)),
                    ("tokens", n(4)),
                ]),
                row([
                    ("span_id", s("middle")),
                    ("text", s("urgent middle\nplain tail")),
                    ("start_line", n(10)),
                    ("end_line", n(12)),
                    ("tokens", n(5)),
                ]),
            ],
        );
    }

    #[test]
    fn read_honors_span_id_narrowing_and_handle_scoped_span_ids() {
        let output = evaluate_query_output(
            r#"? read("beta.md", 10, "shared", text, start_line, end_line, tokens)."#,
            content_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![row([
                ("text", s("beta urgent")),
                ("start_line", n(2)),
                ("end_line", n(2)),
                ("tokens", n(3)),
            ])],
        );
    }

    #[test]
    fn content_primitives_can_use_later_positive_inputs() {
        let output = evaluate_query_output(
            r"
            budget(9).
            ? read(h, b, span_id, text, start_line, end_line, tokens),
              *handle{id: h},
              budget(b).
            ",
            content_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![
                row([
                    ("b", n(9)),
                    ("h", s("alpha.md")),
                    ("span_id", s("shared")),
                    ("text", s("intro line")),
                    ("start_line", n(1)),
                    ("end_line", n(1)),
                    ("tokens", n(4)),
                ]),
                row([
                    ("b", n(9)),
                    ("h", s("alpha.md")),
                    ("span_id", s("middle")),
                    ("text", s("urgent middle\nplain tail")),
                    ("start_line", n(10)),
                    ("end_line", n(12)),
                    ("tokens", n(5)),
                ]),
                row([
                    ("b", n(9)),
                    ("h", s("beta.md")),
                    ("span_id", s("shared")),
                    ("text", s("beta urgent")),
                    ("start_line", n(2)),
                    ("end_line", n(2)),
                    ("tokens", n(3)),
                ]),
            ],
        );
    }

    #[test]
    fn match_uses_regex_over_content_lines() {
        let output = evaluate_query_output(
            r#"? *handle{id: handle}, match("urgent", handle, line, snippet)."#,
            content_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_query_rows(
            &rows,
            vec![
                row([
                    ("handle", s("alpha.md")),
                    ("line", n(10)),
                    ("snippet", s("urgent middle")),
                ]),
                row([
                    ("handle", s("alpha.md")),
                    ("line", n(30)),
                    ("snippet", s("final urgent")),
                ]),
                row([
                    ("handle", s("beta.md")),
                    ("line", n(2)),
                    ("snippet", s("beta urgent")),
                ]),
            ],
        );
    }

    #[test]
    fn match_reports_invalid_regex() {
        let err = evaluate_query_error(
            r#"? match("[", "alpha.md", line, snippet)."#,
            content_database(),
        );

        assert!(matches!(err, EvalError::InvalidRegex { pattern, .. } if pattern == "["));
    }

    #[test]
    fn search_returns_ranked_title_body_and_frontmatter_hits() {
        let output = evaluate_query_output(
            r#"? search("v17 conformance", h, span_id, score, reason, field, low_confidence)."#,
            search_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert!(
            rows.len() >= 4,
            "expected title, identifier, body, and frontmatter hits: {rows:?}"
        );
        assert_eq!(rows[0].get("h"), Some(&s("audit/v17.md")));
        assert_eq!(rows[0].get("span_id"), Some(&Value::Null));
        assert_eq!(rows[0].get("reason"), Some(&s("title-substring")));
        assert_eq!(rows[0].get("field"), Some(&s("title")));
        assert_eq!(rows[0].get("low_confidence"), Some(&Value::Bool(false)));
        assert!(
            (value_f64(rows[0].get("score").expect("score")) - f64::from(0.95_f32)).abs()
                < 0.000_001
        );

        assert!(rows.iter().any(|row| {
            row.get("h") == Some(&s("audit/v17.md"))
                && row.get("span_id") == Some(&s("body"))
                && row.get("field") == Some(&s("body"))
                && row.get("reason") == Some(&s("body-substring"))
        }));
        assert!(rows.iter().any(|row| {
            row.get("h") == Some(&s("audit/v17.md"))
                && row.get("field") == Some(&s("frontmatter:concern"))
                && row.get("reason") == Some(&s("frontmatter-value-match"))
        }));
        assert!(
            rows.windows(2).all(|pair| {
                value_f64(pair[0].get("score").expect("left score"))
                    >= value_f64(pair[1].get("score").expect("right score"))
            }),
            "search rows should be score-sorted: {rows:?}"
        );
        assert!(
            rows.iter().all(|row| {
                let score = value_f64(row.get("score").expect("score"));
                (0.0..=1.0).contains(&score)
            }),
            "scores must be calibrated into [0, 1]: {rows:?}"
        );
    }

    #[test]
    fn search_honors_handle_and_span_constraints() {
        let output = evaluate_query_output(
            r#"? search("conformance", "audit/v17.md", "body", score, reason, field, low_confidence)."#,
            search_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("field"), Some(&s("body")));
        assert_eq!(rows[0].get("reason"), Some(&s("body-substring")));
        assert_eq!(rows[0].get("low_confidence"), Some(&Value::Bool(false)));
    }

    #[test]
    fn search_tie_breaks_by_source_handle_span_field_and_reason() {
        let output = evaluate_query_output(
            r#"? search("same topic", h, span_id, score, reason, field, low_confidence)."#,
            search_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("h"), Some(&s("notes/tie-a.md")));
        assert_eq!(rows[1].get("h"), Some(&s("notes/tie-b.md")));
        assert_eq!(rows[0].get("score"), rows[1].get("score"));
    }

    #[test]
    fn read_full_is_capability_gated_and_budgeted() {
        let err = evaluate_query_error(r#"? read_full("alpha.md", content)."#, content_database());
        assert!(matches!(
            err,
            EvalError::CapabilityRequired {
                primitive: "read_full",
                capability: READ_FULL_CAPABILITY,
            }
        ));

        let output = evaluate_query_output_with_options(
            r#"? read_full("alpha.md", content)."#,
            content_database(),
            EvalOptions::default().with_capability(READ_FULL_CAPABILITY),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();
        assert_query_rows(
            &rows,
            vec![row([(
                "content",
                s("intro line\nurgent middle\nplain tail\nfinal urgent"),
            )])],
        );

        let err = evaluate_query_error_with_options(
            r#"? read_full("alpha.md", content)."#,
            content_database(),
            EvalOptions::default()
                .with_capability(READ_FULL_CAPABILITY)
                .with_read_full_token_limit(16),
        );
        assert!(matches!(
            err,
            EvalError::ReadFullBudgetExceeded {
                handle,
                tokens: 17,
                limit: 16,
            } if handle == "alpha.md"
        ));
    }

    #[test]
    fn graph_primitives_support_named_arguments() {
        let outputs = evaluate_queries(
            r#"
            ? upstream(h: "formal-model/v17.md", anc: anc).
            ? impact(x: "compiler/exec.md", h: h, depth: d).
            "#,
            mvs_database(),
        );

        let mut rows = outputs.into_iter();
        assert_query_rows(
            &rows.next().expect("named upstream output"),
            vec![
                row([("anc", s("OQ-22"))]),
                row([("anc", s("OQ-23"))]),
                row([("anc", s("OQ-60"))]),
                row([("anc", s("formal-model/v14.md"))]),
                row([("anc", s("formal-model/v15.md"))]),
                row([("anc", s("formal-model/v16.md"))]),
            ],
        );
        assert_query_rows(
            &rows.next().expect("named impact output"),
            vec![
                row([("d", n(1)), ("h", s("compiler/jit-spec.md"))]),
                row([("d", n(2)), ("h", s("OQ-22"))]),
                row([("d", n(2)), ("h", s("compiler/jit-stale.md"))]),
            ],
        );
        assert!(
            rows.next().is_none(),
            "unexpected extra named primitive output"
        );
    }

    #[test]
    fn upstream_primitive_handles_scaled_chain_fixture() {
        let outputs = evaluate_queries(
            r#"? upstream("n0", anc)."#,
            Database::from_store(&chain_store(4_096)),
        );
        assert_eq!(outputs[0].len(), 4_096);
        assert!(outputs[0].contains(&row([("anc", s("n1"))])));
        assert!(outputs[0].contains(&row([("anc", s("n4096"))])));
    }

    #[test]
    fn count_primitives_do_not_invent_unknown_handles() {
        let outputs = evaluate_queries(r#"? cite_count("missing", n)."#, mvs_database());
        assert_query_rows(&outputs[0], Vec::new());
    }

    #[test]
    fn lifecycle_primitives_use_configured_lattice_facts() {
        let outputs = evaluate_queries(
            r#"
            ? terminal("done.md").
            ? active("draft.md").
            ? active("nostatus.md").
            ? settled("stable.md").
            ? pipeline_position("draft.md", n).
            ? pipeline_position_for("stable", n).
            ? obligation("OQ-1").
            ? discharged("OQ-1").
            ? undischarged("OQ-2").
            ? discharge_count("OQ-1", n).
            ? discharge_count("OQ-2", n).
            ? token_estimate("draft.md", n).
            ? freshness("draft.md", days).
            ? flux("draft.md", 1000000, delta).
            "#,
            lifecycle_database(),
        );

        let mut rows = outputs.into_iter();
        assert_query_rows(&rows.next().expect("terminal output"), vec![row([])]);
        assert_query_rows(&rows.next().expect("active output"), vec![row([])]);
        assert_query_rows(
            &rows.next().expect("missing status active output"),
            vec![row([])],
        );
        assert_query_rows(&rows.next().expect("settled output"), vec![row([])]);
        assert_query_rows(
            &rows.next().expect("pipeline position output"),
            vec![row([("n", n(1))])],
        );
        assert_query_rows(
            &rows.next().expect("pipeline position for output"),
            vec![row([("n", n(2))])],
        );
        assert_query_rows(&rows.next().expect("obligation output"), vec![row([])]);
        assert_query_rows(&rows.next().expect("discharged output"), vec![row([])]);
        assert_query_rows(&rows.next().expect("undischarged output"), vec![row([])]);
        assert_query_rows(
            &rows.next().expect("discharge count output"),
            vec![row([("n", n(1))])],
        );
        assert_query_rows(
            &rows.next().expect("zero discharge count output"),
            vec![row([("n", n(0))])],
        );
        assert_query_rows(
            &rows.next().expect("token estimate output"),
            vec![row([("n", n(25))])],
        );
        assert_query_rows(
            &rows.next().expect("future freshness output"),
            vec![row([("days", n(0))])],
        );
        assert_query_rows(
            &rows.next().expect("flux output"),
            vec![row([("delta", n(1))])],
        );
        assert!(rows.next().is_none(), "unexpected extra lifecycle output");
    }

    #[test]
    fn config_rows_expose_explicit_ordinals_and_null_for_scalars() {
        let mut store = FactStore::default();
        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![
                    config("convergence.active", "draft"),
                    ordered_config("convergence.ordering", "draft", 1),
                ],
            )
            .expect("replace config");
        let outputs = evaluate_queries(
            r#"
            ? *config{key: "convergence.active", ordinal}.
            ? *config{key: "convergence.ordering", value, ordinal}.
            "#,
            Database::from_store(&store),
        );

        assert_query_rows(&outputs[0], vec![row([("ordinal", Value::Null)])]);
        assert_query_rows(
            &outputs[1],
            vec![row([("ordinal", n(1)), ("value", s("draft"))])],
        );
    }

    #[test]
    fn lifecycle_metrics_do_not_invent_unknown_handles_or_unbound_flux_windows() {
        let outputs = evaluate_queries(
            r#"
            ? token_estimate("missing.md", n).
            ? discharge_count("missing.md", n).
            ? freshness("missing.md", days).
            ? flux("draft.md", days, delta).
            ? flux("draft.md", "bad", delta).
            "#,
            lifecycle_database(),
        );

        for output in outputs {
            assert_query_rows(&output, Vec::new());
        }
    }

    #[test]
    fn at_snapshot_last_overlays_handle_status_and_warns_partial_history() {
        let output = evaluate_query_output(
            r#"
            ? *handle{id: h, status: current},
              at("snapshot:last") { *handle{id: h, status: prior} },
              prior != current.
            "#,
            time_travel_database(),
        );
        let mut rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();
        rows.sort();
        assert_query_rows(
            &rows,
            vec![row([
                ("current", s("current")),
                ("h", s("draft.md")),
                ("prior", s("draft")),
            ])],
        );
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, "partial_history");
        assert_eq!(
            output.warnings[0].reference.as_deref(),
            Some("snapshot:last")
        );
    }

    #[test]
    fn at_iso_date_uses_nearest_snapshot() {
        let output = evaluate_query_output(
            r#"
            ? at("2026-05-02") { *handle{id: "draft.md", status: prior} }.
            "#,
            time_travel_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("prior", s("raw"))])],
        );
    }

    #[test]
    fn at_snapshot_id_selects_named_snapshot() {
        let output = evaluate_query_output(
            r#"
            ? at("snapshot:s2") { *handle{id: "draft.md", status: prior} }.
            "#,
            time_travel_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("prior", s("draft"))])],
        );
    }

    #[test]
    fn at_iso_date_ties_choose_later_snapshot() {
        let output = evaluate_query_output(
            r#"
            ? at("2026-05-05") { *handle{id: "draft.md", status: prior} }.
            "#,
            tie_time_travel_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("prior", s("draft"))])],
        );
    }

    #[test]
    fn at_inside_rule_preserves_partial_history_warning() {
        let output = evaluate_query_output(
            r#"
            prior_status(h, prior) :=
              at("snapshot:last") { *handle{id: h, status: prior} }.
            ? prior_status("draft.md", prior).
            "#,
            time_travel_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("prior", s("draft"))])],
        );
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, "partial_history");
    }

    #[test]
    fn at_snapshot_fallback_rejects_current_non_handle_stored_relations() {
        let err = evaluate_query_error(
            r#"
            ? at("snapshot:last") { *edge{from: "draft.md", to, kind} }.
            "#,
            time_travel_database(),
        );

        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedStoredRelation { relation, .. }
                if relation.as_str() == "edge"
        ));
    }

    #[test]
    fn at_snapshot_fallback_rejects_current_edge_and_content_primitives() {
        let graph_err = evaluate_query_error(
            r#"
            ? at("snapshot:last") { upstream("draft.md", h) }.
            "#,
            time_travel_database(),
        );
        assert!(matches!(
            graph_err,
            EvalError::UnsupportedTimeScopedPrimitive { predicate, .. }
                if predicate.display_name() == "upstream"
        ));

        let content_err = evaluate_query_error(
            r#"
            ? at("snapshot:last") { token_estimate("draft.md", tokens) }.
            "#,
            time_travel_database(),
        );
        assert!(matches!(
            content_err,
            EvalError::UnsupportedTimeScopedPrimitive { predicate, .. }
                if predicate.display_name() == "token_estimate"
        ));
    }

    #[test]
    fn at_snapshot_fallback_rejects_current_derived_predicates() {
        let program = parse_program(
            "inline",
            r#"
            historical_current(h) := *handle{id: h, status: "current"}.
            ? at("snapshot:last") { historical_current("draft.md") }.
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::new(analyzed, time_travel_database());
        evaluator.run_fixpoint().expect("fixpoint evaluates");

        let err = evaluator
            .eval_query(&query)
            .expect_err("derived predicate is rejected in snapshot fallback");
        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedDerivedPredicate { .. }
        ));
    }

    #[test]
    fn flux_counts_rfc3339_snapshot_rows() {
        let outputs = evaluate_queries(
            r#"? flux("draft.md", 1000000, delta)."#,
            time_travel_database(),
        );

        assert_query_rows(&outputs[0], vec![row([("delta", n(2))])]);
    }

    #[test]
    fn flux_orders_same_day_snapshots_by_full_timestamp() {
        let outputs = evaluate_queries(
            r#"? flux("draft.md", 1000000, delta)."#,
            same_day_flux_database(),
        );

        assert_query_rows(&outputs[0], vec![row([("delta", n(2))])]);
    }

    #[test]
    fn at_does_not_synthesize_handles_from_snapshot_key_values() {
        let mut store = FactStore::default();
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot_fact(
                    "s1",
                    "2026-05-01",
                    "deleted.md",
                    "status",
                    "draft",
                )],
            )
            .expect("replace snapshots");
        let output = evaluate_query_output(
            r#"? at("snapshot:last") { *handle{id: h, status: s} }."#,
            Database::from_store(&store),
        );

        assert!(output.rows.is_empty());
        assert_eq!(output.warnings[0].code, "partial_history");
    }

    #[test]
    fn soft_lifecycle_rule_shadowing_replaces_default_primitive() {
        let outputs = evaluate_queries(
            r#"
            terminal(h) := *handle{id: h, status: "draft"}.
            ? terminal("draft.md").
            ? terminal("done.md").
            "#,
            lifecycle_database(),
        );

        let mut rows = outputs.into_iter();
        assert_query_rows(
            &rows.next().expect("shadowed terminal output"),
            vec![row([])],
        );
        assert_query_rows(
            &rows.next().expect("default terminal no longer applies"),
            Vec::new(),
        );
        assert!(rows.next().is_none(), "unexpected extra shadowing output");
    }

    #[test]
    fn mvs1_matches_spike_handle_rows() {
        assert_query_rows(
            &mvs_outputs().handles,
            vec![
                row([
                    ("area", s("compiler")),
                    ("id", s("OQ-60")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("open")),
                ]),
                row([
                    ("area", s("compiler")),
                    ("id", s("OQ-88")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("open")),
                ]),
                row([
                    ("area", s("compiler")),
                    ("id", s("compiler/exec.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("current")),
                ]),
                row([
                    ("area", s("compiler")),
                    ("id", s("compiler/jit-spec.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("draft")),
                ]),
                row([
                    ("area", s("compiler")),
                    ("id", s("compiler/jit-stale.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("superseded")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("OQ-22")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("open")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("OQ-23")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("open")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("OQ-99")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("resolved")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("formal-model/v14.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("superseded")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("formal-model/v15.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("superseded")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("formal-model/v16.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("superseded")),
                ]),
                row([
                    ("area", s("formal-model")),
                    ("id", s("formal-model/v17.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("authoritative")),
                ]),
                row([
                    ("area", s("research-log")),
                    ("id", s("OQ-77")),
                    ("kind", s("label")),
                    ("namespace", s("OQ")),
                    ("status", s("open")),
                ]),
                row([
                    ("area", s("research-log")),
                    ("id", s("research-log/2026-04-jit.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("research")),
                ]),
                row([
                    ("area", s("synthesis")),
                    ("id", s("synthesis/2026-04-discharge.md")),
                    ("kind", s("file")),
                    ("namespace", s("")),
                    ("status", s("current")),
                ]),
            ],
        );
    }

    #[test]
    fn mvs2_matches_spike_release_blocker_rows() {
        assert_query_rows(
            &mvs_outputs().release_blockers,
            vec![
                row([
                    ("file", Value::Null),
                    ("h", s("OQ-22")),
                    ("kind", s("undischarged")),
                    ("line", Value::Null),
                    ("target", Value::Null),
                ]),
                row([
                    ("file", Value::Null),
                    ("h", s("OQ-23")),
                    ("kind", s("undischarged")),
                    ("line", Value::Null),
                    ("target", Value::Null),
                ]),
                row([
                    ("file", Value::Null),
                    ("h", s("OQ-60")),
                    ("kind", s("undischarged")),
                    ("line", Value::Null),
                    ("target", Value::Null),
                ]),
                row([
                    ("file", Value::Null),
                    ("h", s("OQ-88")),
                    ("kind", s("undischarged")),
                    ("line", Value::Null),
                    ("target", Value::Null),
                ]),
                row([
                    ("file", s("compiler/jit-spec.md")),
                    ("h", s("compiler/jit-spec.md")),
                    ("kind", s("broken_ref")),
                    ("line", n(51)),
                    ("target", Value::Null),
                ]),
                row([
                    ("file", Value::Null),
                    ("h", s("compiler/jit-spec.md")),
                    ("kind", s("stale_dep")),
                    ("line", Value::Null),
                    ("target", s("compiler/jit-stale.md")),
                ]),
            ],
        );
    }

    #[test]
    fn mvs3_matches_spike_supersedes_chain_rows() {
        assert_query_rows(
            &mvs_outputs().supersedes_chain,
            vec![
                row([
                    ("depth", n(1)),
                    ("start", s("formal-model/v17.md")),
                    ("target", s("formal-model/v16.md")),
                ]),
                row([
                    ("depth", n(2)),
                    ("start", s("formal-model/v17.md")),
                    ("target", s("formal-model/v15.md")),
                ]),
                row([
                    ("depth", n(3)),
                    ("start", s("formal-model/v17.md")),
                    ("target", s("formal-model/v14.md")),
                ]),
            ],
        );
    }

    #[test]
    fn mvs4_matches_spike_open_oq_rows() {
        assert_query_rows(
            &mvs_outputs().open_oqs,
            vec![
                row([("q", s("OQ-22"))]),
                row([("q", s("OQ-23"))]),
                row([("q", s("OQ-60"))]),
                row([("q", s("OQ-77"))]),
                row([("q", s("OQ-88"))]),
            ],
        );
    }

    #[test]
    fn mvs5a_matches_spike_oq_pressure_rows_including_zero_counts() {
        assert_query_rows(
            &mvs_outputs().oq_pressure,
            vec![
                row([("n", n(1)), ("q", s("OQ-22"))]),
                row([("n", n(1)), ("q", s("OQ-23"))]),
                row([("n", n(1)), ("q", s("OQ-60"))]),
                row([("n", n(0)), ("q", s("OQ-77"))]),
                row([("n", n(0)), ("q", s("OQ-88"))]),
            ],
        );
    }

    #[test]
    fn mvs5b_matches_spike_oq_per_area_rows() {
        assert_query_rows(
            &mvs_outputs().oq_per_area,
            vec![
                row([("area", s("compiler")), ("n", n(2))]),
                row([("area", s("formal-model")), ("n", n(2))]),
                row([("area", s("research-log")), ("n", n(1))]),
            ],
        );
    }

    #[test]
    fn stored_relation_uses_bound_field_candidates() {
        let database = Database::from_store(&fixture_store());
        let relation = database
            .stored
            .get(&Ident::new_unchecked("handle"))
            .expect("handle relation");
        let candidates = relation
            .candidate_rows(&[(Ident::new_unchecked("id"), Value::String("v17".to_string()))])
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].get(&Ident::new_unchecked("id")),
            Some(&Value::String("v17".to_string()))
        );
    }

    #[test]
    fn fixed_point_evaluates_recursion_negation_and_count() {
        let program = parse_program(
            "fixture",
            r#"
            terminal(h) := *handle{id: h, status: "resolved"}.
            terminal(h) := *handle{id: h, status: "superseded"}.
            open_oq(h) := *handle{id: h, kind: "label", namespace: "OQ"}, not terminal(h).
            dep_path(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            dep_path(h, anc) := *edge{from: h, to: mid, kind: "DependsOn"}, dep_path(mid, anc).
            oq_area(area) := *handle{kind: "label", namespace: "OQ", area}.
            oq_per_area(area, n) := oq_area(area), n = Count{ h : open_oq(h), *handle{id: h, area} }.
            ? open_oq(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let open_oq = PredicateRef::new(Ident::new_unchecked("open_oq"));
        let rows = evaluator.database().derived(&open_oq).expect("open_oq");
        assert_eq!(rows.len(), 1);
        assert!(rows.contains(&Tuple(vec![Value::String("OQ-22".to_string())])));

        let oq_per_area = PredicateRef::new(Ident::new_unchecked("oq_per_area"));
        let counts = evaluator.database().derived(&oq_per_area).expect("counts");
        assert!(counts.contains(&Tuple(vec![
            Value::String("formal-model".to_string()),
            Value::Number(NumberValue::Int(1)),
        ])));

        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
    }

    #[test]
    fn count_aggregate_unifies_prebound_result_variable() {
        let program = parse_program(
            "fixture",
            r#"
            seed(0, 0).
            seed(1, 1).
            empty(x) := *handle{id: x, kind: "missing"}.
            matches(seed_value, n) :=
              seed(seed_value, n),
              n = Count{ x : empty(x) }.
            ? matches(seed_value, n).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        assert_query_rows(
            &evaluator
                .eval_query(&query)
                .expect("query evaluates")
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("n", n(0)), ("seed_value", n(0))])],
        );
    }

    #[test]
    fn count_aggregate_does_not_invent_empty_groups() {
        let program = parse_program(
            "fixture",
            r#"
            empty(area, h) := *handle{id: h, kind: "missing", area: area}.
            count_by_area(area, n) := n = Count{ h : empty(area, h) }.
            ? count_by_area(area, n).
            "#,
        )
        .expect("program parses");
        let err = analyze(program).expect_err("group key must be bound outside aggregate");
        assert!(matches!(
            err,
            StaticError::UnboundHeadVariable { variable, .. } if variable.as_str() == "area"
        ));
    }

    #[test]
    fn aggregate_groups_can_be_originated_by_later_positive_atoms() {
        let outputs = evaluate_queries(
            r#"
            open_oq(h) := *handle{id: h, kind: "label", namespace: "OQ", status: "open"}.
            oq_area(area) := *handle{kind: "label", namespace: "OQ", area}.
            oq_per_area(area, n) :=
              n = Count{ h : open_oq(h), *handle{id: h, area} },
              oq_area(area).
            ? oq_per_area(area, n).
            "#,
            Database::from_store(&fixture_store()),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("area", s("compiler")), ("n", n(0))]),
                row([("area", s("formal-model")), ("n", n(1))]),
            ],
        );
    }

    #[test]
    fn graph_primitives_can_use_later_positive_anchors() {
        let outputs = evaluate_queries(
            r#"
            ? downstream(h, desc),
              *handle{id: h, kind: "label", namespace: "OQ"}.
            "#,
            Database::from_store(&fixture_store()),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("desc", s("jit")), ("h", s("OQ-22"))]),
                row([("desc", s("v17")), ("h", s("OQ-22"))]),
            ],
        );
    }

    #[test]
    fn scalar_aggregates_compute_distinct_values() {
        let outputs = evaluate_queries(
            r"
            amount(2).
            amount(5).
            ? total = Sum{ value : amount(value) }.
            ? min = Min{ value : amount(value) }.
            ? max = Max{ value : amount(value) }.
            ? avg = Avg{ value : amount(value) }.
            ? values = List{ value : amount(value) }.
            ? values = Set{ value : amount(value) }.
            ",
            Database::default(),
        );

        assert_query_rows(&outputs[0], vec![row([("total", n(7))])]);
        assert_query_rows(&outputs[1], vec![row([("min", n(2))])]);
        assert_query_rows(&outputs[2], vec![row([("max", n(5))])]);
        assert_query_rows(&outputs[3], vec![row([("avg", f(3.5))])]);
        assert_query_rows(&outputs[4], vec![row([("values", list([n(2), n(5)]))])]);
        assert_query_rows(&outputs[5], vec![row([("values", list([n(2), n(5)]))])]);
    }

    #[test]
    fn top_k_selects_ranked_rows_and_unifies_result_tuple() {
        let outputs = evaluate_queries(
            r#"
            score("a", 5).
            score("b", 9).
            score("c", 9).
            ? (h, score) = TopK{ k: 2, key: score : (h, score) : score(h, score) }.
            wanted("b", 9).
            ? wanted(h, score), (h, score) = TopK{ k: 1, key: score : (h, score) : score(h, score) }.
            ? (h, score) = TopK{ k: 1, key: score : (h, score) : score(h, score) }.
            "#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("h", s("b")), ("score", n(9))]),
                row([("h", s("c")), ("score", n(9))]),
            ],
        );
        assert_query_rows(&outputs[1], vec![row([("h", s("b")), ("score", n(9))])]);
        assert_query_rows(&outputs[2], vec![row([("h", s("b")), ("score", n(9))])]);
    }

    #[test]
    fn aggregate_duplicate_args_are_rejected() {
        let program = parse_program(
            "inline",
            r#"
            score("a", 5).
            ? (h, score) = TopK{ k: 1, k: 2, key: score : (h, score) : score(h, score) }.
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let err = evaluator
            .eval_query(&query)
            .expect_err("duplicate aggregate arg rejected");
        assert!(matches!(
            err,
            EvalError::InvalidAggregateArg {
                argument: "duplicate",
                ..
            }
        ));
    }

    #[test]
    fn rank_binds_dense_ranks_before_evaluating_contribution() {
        let outputs = evaluate_queries(
            r#"
            score("a", 5).
            score("b", 9).
            score("c", 9).
            score("d", 2).
            ? (h, rank) = Rank{ key: score, rank: rank : (h, rank) : score(h, score) }.
            "#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("h", s("a")), ("rank", n(2))]),
                row([("h", s("b")), ("rank", n(1))]),
                row([("h", s("c")), ("rank", n(1))]),
                row([("h", s("d")), ("rank", n(3))]),
            ],
        );
    }

    #[test]
    fn take_until_sorts_by_key_and_stops_at_budget() {
        let outputs = evaluate_queries(
            r#"
            span("s1", 1, 3).
            span("s2", 2, 4).
            span("s3", 3, 2).
            ? (span_id, tokens) =
              TakeUntil{ budget: 7, sum: tokens, key: line :
                (span_id, tokens) :
                span(span_id, line, tokens)
              }.
            "#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("span_id", s("s1")), ("tokens", n(3))]),
                row([("span_id", s("s2")), ("tokens", n(4))]),
            ],
        );
    }

    #[test]
    fn row_producing_aggregates_handle_scaled_inputs_deterministically() {
        let mut input = String::new();
        for idx in 0..256 {
            writeln!(&mut input, r#"score("h{idx:04}", {idx})."#).expect("write score fixture");
            writeln!(&mut input, r#"span("s{idx:04}", {idx}, 1)."#).expect("write span fixture");
        }
        input.push_str(
            r"
            ? (h, score) = TopK{ k: 3, key: score : (h, score) : score(h, score) }.
            ? (span_id, tokens) =
              TakeUntil{ budget: 5, sum: tokens, key: line :
                (span_id, tokens) :
                span(span_id, line, tokens)
              }.
            ",
        );

        let outputs = evaluate_queries(&input, Database::default());

        assert_query_rows(
            &outputs[0],
            vec![
                row([("h", s("h0253")), ("score", n(253))]),
                row([("h", s("h0254")), ("score", n(254))]),
                row([("h", s("h0255")), ("score", n(255))]),
            ],
        );
        assert_query_rows(
            &outputs[1],
            (0..5)
                .map(|idx| row([("span_id", s(&format!("s{idx:04}"))), ("tokens", n(1))]))
                .collect(),
        );
    }

    #[test]
    fn non_count_aggregates_do_not_emit_empty_groups() {
        let outputs = evaluate_queries(
            r#"
            group("x").
            candidate("x", 1).
            empty_value(g, value) := candidate(g, value), value = 2.
            ? group(g), total = Sum{ value : empty_value(g, value) }.
            "#,
            Database::default(),
        );

        assert_query_rows(&outputs[0], Vec::new());
    }

    #[test]
    fn derived_relation_uses_bound_position_candidates() {
        let program = parse_program(
            "fixture",
            r#"
            dep_path(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            ? dep_path("v17", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let relation = evaluator
            .database
            .derived
            .get(&PredicateRef::new(Ident::new_unchecked("dep_path")))
            .expect("dep_path relation");
        let candidates = relation
            .candidate_tuples(&[(0, Value::String("v17".to_string()))])
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn stored_index_preserves_same_atom_expression_unification() {
        let program =
            parse_program("fixture", r"? *pair{n: x, next: x + 1}.").expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut database = Database::default();
        database.insert_stored_rows(
            "pair",
            [
                named_row([
                    ("n", Value::Number(NumberValue::Int(1))),
                    ("next", Value::Number(NumberValue::Int(2))),
                ]),
                named_row([
                    ("n", Value::Number(NumberValue::Int(1))),
                    ("next", Value::Number(NumberValue::Int(3))),
                ]),
            ],
        );
        let evaluator = Evaluator::new(analyzed, database);
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("x"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn derived_index_preserves_same_atom_expression_unification() {
        let program = parse_program("fixture", r"seed(1, 2). seed(1, 3). ? seed(x, x + 1).")
            .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("x"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn semi_naive_recursion_handles_chain_closure() {
        let program = parse_program(
            "fixture",
            r#"
            dep_path(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            dep_path(h, anc) := dep_path(h, mid), *edge{from: mid, to: anc, kind: "DependsOn"}.
            ? dep_path("n0", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&chain_store(256)));
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 256);
        assert!(output.rows.iter().any(|row| {
            row.fields
                .get("anc")
                .is_some_and(|value| value == &Value::String("n256".to_string()))
        }));
    }

    #[test]
    fn facts_are_seeded_as_derived_tuples() {
        let program =
            parse_program("fixture", r#"seed("alpha"). ? seed(value)."#).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("value"),
            Some(&Value::String("alpha".to_string()))
        );
    }

    #[test]
    fn positive_recursion_is_not_rule_order_dependent() {
        let program = parse_program(
            "fixture",
            r#"
            dep_path(h, anc) := *edge{from: h, to: mid, kind: "DependsOn"}, dep_path(mid, anc).
            dep_path(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            ? dep_path("v17", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("anc"),
            Some(&Value::String("OQ-22".to_string()))
        );
    }

    #[test]
    fn query_local_rules_execute() {
        let program = parse_program(
            "fixture",
            r#"
            ?
              where local_oq(h) := *handle{id: h, namespace: "OQ"}.
              local_oq(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 2);
    }

    #[test]
    fn named_derived_call_arguments_evaluate_in_signature_order() {
        let program = parse_program(
            "fixture",
            r#"
            left("a").
            right("b").
            pair(left, right) := left(left), right(right).
            ? pair(right: r, left: l).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].fields.get("l"), Some(&s("a")));
        assert_eq!(output.rows[0].fields.get("r"), Some(&s("b")));
    }

    #[test]
    fn mixed_positional_and_named_call_arguments_evaluate_in_signature_order() {
        let program = parse_program(
            "fixture",
            r#"
            left("a").
            right("b").
            pair(left, right) := left(left), right(right).
            ? pair(l, right: r).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].fields.get("l"), Some(&s("a")));
        assert_eq!(output.rows[0].fields.get("r"), Some(&s("b")));
    }

    #[test]
    fn named_query_local_call_arguments_evaluate_in_signature_order() {
        let program = parse_program(
            "fixture",
            r#"
            left("a").
            right("b").
            ?
              where pair(left, right) := left(left), right(right).
              pair(right: r, left: l).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].fields.get("l"), Some(&s("a")));
        assert_eq!(output.rows[0].fields.get("r"), Some(&s("b")));
    }

    #[test]
    fn source_identity_fields_are_queryable_on_source_facts() {
        let program = parse_program(
            "fixture",
            r#"? *handle{id: "v17", corpus, source, native_id, origin_uri, revision, generation}."#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        let row = &output.rows[0].fields;
        assert_eq!(row.get("corpus"), Some(&Value::String("test".to_string())));
        assert_eq!(
            row.get("source"),
            Some(&Value::String("fixture".to_string()))
        );
        assert_eq!(
            row.get("native_id"),
            Some(&Value::String("v17".to_string()))
        );
        assert_eq!(
            row.get("origin_uri"),
            Some(&Value::String("fixture://v17".to_string()))
        );
        assert_eq!(row.get("revision"), Some(&Value::String("rev".to_string())));
        assert_eq!(
            row.get("generation"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn query_rows_are_deterministic_by_variable_name() {
        let program = parse_program("fixture", r"? *handle{id: h, area}.").expect("parse");
        let analyzed = analyze(program).expect("analyze");
        let query = analyzed.queries().next().cloned().expect("query");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        let first = output.rows.first().expect("row");
        let keys = first.fields.keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys, vec!["area", "h"]);
    }
}
