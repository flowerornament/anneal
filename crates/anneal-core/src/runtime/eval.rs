use std::cmp::Ordering;
use std::collections::btree_set;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io;
use std::num::NonZeroUsize;
use std::slice;
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde::Serialize;
use serde::ser::SerializeMap;

use crate::facts::FactIdentity;
#[cfg(any(not(feature = "physical-substrate"), test))]
use crate::facts::SnapshotFact;
#[cfg(any(not(feature = "physical-substrate"), test))]
use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, HandleFact, MetaFact, SpanFact,
};
use crate::ids::Generation;
#[cfg(feature = "physical-substrate")]
use crate::ir::ids::RowId;
#[cfg(test)]
use crate::policy::ActionKind;
#[cfg(test)]
use crate::policy::PolicyDecision;
use crate::policy::{
    Action, AllowAllPolicy, AuthorizationError, Policy, authorize_action,
    authorize_capability_action,
};
use crate::ranking::{
    DEFAULT_LOW_CONFIDENCE_THRESHOLD, DefaultRanker, Ranker, RankingContext, SearchHandleDocument,
    SearchIndex, SearchQuery, rank_search_hits,
};
use crate::retrieval::{
    ContentProvider, ReadChunk, ReadContext, ReadError, ReadFullContent, ReadFullRequest,
    ReadRequest, SearchContext, SearchError, SearchProvider, SearchRequest, SearchSpanScope,
};
use crate::runtime::analysis::{AnalyzedProgram, AnalyzedQuery};
use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, ComparisonOp, Expr,
    FieldPattern, Head, Ident, Literal, NegatedAtom, NumberLiteral, OrderDirection, OrderKey,
    PredicateRef, Rule, SourceLocation, StoredAtom, Term,
};
use crate::runtime::introspection::{
    IntrospectionIndex, StoredRelationSummary, is_static_stored_relation,
};
use crate::runtime::primitives::PrimitivePredicate;
use crate::source::{ActorContext, RuntimeCapability, SourceInfo};
use crate::store::FactStore;
use crate::time::{
    current_days_since_epoch, iso_days_since_epoch, relative_days_reference,
    snapshot_days_since_epoch,
};
use crate::trail::{
    TRAIL_GENERATION_RELATION, TRAIL_REF_RELATION, TRAIL_RELATION, TrailContext,
    TrailEntryRedacted, TrailError, TrailGeneration, TrailQuery, TrailRefKind, TrailReference,
    TrailStore,
};
use crate::visibility::FactVisibility;
#[cfg(feature = "physical-substrate")]
use crate::vm::store::{RelationStore, TupleDb, TupleRow};
pub use crate::vm::value::NumberValue;
#[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
use crate::vm::value::PhysicalValue;

pub type Binding = BTreeMap<Ident, Value>;
type DeltaMap = BTreeMap<PredicateRef, DerivedRelation>;
type DerivationRef = Arc<DerivationNode>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Tuple(pub Vec<Value>);

impl Tuple {
    pub(crate) fn matches_constraints(&self, constraints: &[(usize, Value)]) -> bool {
        constraints
            .iter()
            .all(|(idx, value)| self.0.get(*idx) == Some(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub fields: BTreeMap<String, Value>,
    pub derivation: Option<DerivationNode>,
}

#[cfg(feature = "physical-substrate")]
#[derive(Clone, Debug, Default)]
struct TupleOverlay {
    relations: BTreeMap<Ident, RelationStore>,
}

#[cfg(feature = "physical-substrate")]
impl TupleOverlay {
    fn relation(&self, relation: &Ident) -> Option<&RelationStore> {
        self.relations.get(relation)
    }

    #[cfg(not(feature = "legacy-time-clone"))]
    fn insert(&mut self, relation: Ident, store: RelationStore) {
        self.relations.insert(relation, store);
    }
}

impl Serialize for Row {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let derivation_replaces_field =
            self.derivation.is_some() && self.fields.contains_key("_derivation");
        let len = self.fields.len() + usize::from(self.derivation.is_some())
            - usize::from(derivation_replaces_field);
        let mut map = serializer.serialize_map(Some(len))?;
        for (key, value) in &self.fields {
            if derivation_replaces_field && key == "_derivation" {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        if let Some(derivation) = &self.derivation {
            map.serialize_entry("_derivation", derivation)?;
        }
        map.end()
    }
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
const DEFAULT_EXPLAIN_DEPTH: usize = 5;
const DEFAULT_EXPLAIN_ROW_LIMIT: usize = 3;
const MAX_AGGREGATE_DERIVATION_CHILDREN: usize = 32;
const CONFIG_IMPACT_TRAVERSE: &str = "impact.traverse";
const DEFAULT_IMPACT_TRAVERSE: &[&str] = &["DependsOn", "Supersedes", "Verifies"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainDepth(usize);

impl ExplainDepth {
    #[must_use]
    pub fn new(depth: usize) -> Self {
        Self(depth.max(1))
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl Default for ExplainDepth {
    fn default() -> Self {
        Self(DEFAULT_EXPLAIN_DEPTH)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplainOptions {
    depth: ExplainDepth,
    row_limit: ExplainRowLimit,
    enabled: bool,
    explicit_depth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplainRowLimit {
    First(NonZeroUsize),
    All,
}

impl Default for ExplainRowLimit {
    fn default() -> Self {
        Self::First(
            NonZeroUsize::new(DEFAULT_EXPLAIN_ROW_LIMIT)
                .expect("default explain row limit is nonzero"),
        )
    }
}

impl ExplainOptions {
    #[must_use]
    pub fn disabled() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_depth(depth: usize) -> Self {
        Self {
            enabled: true,
            depth: ExplainDepth::new(depth),
            explicit_depth: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_depth_limit(mut self, depth: usize) -> Self {
        self.enabled = true;
        self.depth = ExplainDepth::new(depth);
        self.explicit_depth = true;
        self
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn depth(&self) -> ExplainDepth {
        self.depth
    }

    #[must_use]
    pub const fn row_limit(&self) -> ExplainRowLimit {
        self.row_limit
    }

    #[must_use]
    pub const fn explicit_depth(&self) -> bool {
        self.explicit_depth
    }

    #[must_use]
    pub fn with_first_rows(mut self, rows: usize) -> Self {
        self.enabled = true;
        self.row_limit =
            ExplainRowLimit::First(NonZeroUsize::new(rows).unwrap_or(NonZeroUsize::MIN));
        self
    }

    #[must_use]
    pub fn with_all_rows(mut self) -> Self {
        self.enabled = true;
        self.row_limit = ExplainRowLimit::All;
        self
    }

    #[must_use]
    pub const fn explains_row(&self, index: usize) -> bool {
        if !self.enabled {
            return false;
        }
        match self.row_limit {
            ExplainRowLimit::First(rows) => index < rows.get(),
            ExplainRowLimit::All => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DerivationNode {
    kind: DerivationKind,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tuple: Vec<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    truncated: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<Self>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivationKind {
    Query,
    Rule,
    Fact,
    Stored,
    Primitive,
    Comparison,
    Aggregate,
    Negation,
    TimeBlock,
    RecursiveChain,
    Truncated,
}

impl DerivationNode {
    #[must_use]
    pub fn synthetic_query(children: Vec<Self>) -> Self {
        Self::query(children)
    }

    #[must_use]
    pub const fn kind(&self) -> DerivationKind {
        self.kind
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    fn query(children: Vec<Self>) -> Self {
        Self {
            kind: DerivationKind::Query,
            label: "query output row".to_string(),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children,
        }
    }

    fn rule(rule: &Rule, tuple: &Tuple, children: Vec<Self>) -> Self {
        let origin = rule.origin();
        let location = origin.location();
        Self {
            kind: DerivationKind::Rule,
            label: format!(
                "rule {} fired from {:?}",
                rule.head.predicate.display_name(),
                origin.layer()
            ),
            relation: None,
            predicate: Some(rule.head.predicate.display_name()),
            tuple: tuple.0.clone(),
            fields: BTreeMap::new(),
            source: Some(location.source_name.clone()),
            line: non_zero(location.line),
            column: non_zero(location.column),
            truncated: None,
            children,
        }
    }

    fn fact(predicate: &PredicateRef, tuple: &Tuple) -> Self {
        Self {
            kind: DerivationKind::Fact,
            label: format!("fact {}", predicate.display_name()),
            relation: None,
            predicate: Some(predicate.display_name()),
            tuple: tuple.0.clone(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    fn stored(relation: &Ident, row: &NamedRow) -> Self {
        Self {
            kind: DerivationKind::Stored,
            label: format!("stored *{relation} row matched"),
            relation: Some(relation.to_string()),
            predicate: None,
            tuple: Vec::new(),
            fields: compact_stored_row(relation, row),
            source: row_string(row, SOURCE_FIELD).map(str::to_owned),
            line: row_i64(row, "line").and_then(i64_to_usize),
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    fn primitive(predicate: &PredicateRef, tuple: &Tuple) -> Self {
        Self {
            kind: DerivationKind::Primitive,
            label: format!("primitive {} returned a tuple", predicate.display_name()),
            relation: None,
            predicate: Some(predicate.display_name()),
            tuple: tuple.0.clone(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    fn comparison(comparison: &Comparison) -> Self {
        Self::located(
            DerivationKind::Comparison,
            "comparison matched",
            comparison.location.clone(),
        )
    }

    fn aggregate(aggregate: &Aggregate, children: Vec<Self>) -> Self {
        let mut node = Self::located(
            DerivationKind::Aggregate,
            &format!("aggregate {:?} produced a value", aggregate.function),
            aggregate.location.clone(),
        );
        node.children = children;
        node
    }

    fn negation(negation: &NegatedAtom) -> Self {
        let location = match negation {
            NegatedAtom::Stored(atom) => atom.location.clone(),
            NegatedAtom::Derived(atom) => atom.location.clone(),
        };
        Self::located(
            DerivationKind::Negation,
            "negated atom had no matches",
            location,
        )
    }

    fn time_block(reference: &str, location: SourceLocation, children: Vec<Self>) -> Self {
        let mut node = Self::located(
            DerivationKind::TimeBlock,
            &format!("evaluated at {reference:?}"),
            location,
        );
        node.children = children;
        node
    }

    fn located(kind: DerivationKind, label: &str, location: SourceLocation) -> Self {
        Self {
            kind,
            label: label.to_string(),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: Some(location.source_name),
            line: non_zero(location.line),
            column: non_zero(location.column),
            truncated: None,
            children: Vec::new(),
        }
    }

    fn bounded(&self, options: &ExplainOptions) -> Self {
        let mut rule_stack = Vec::new();
        self.bounded_inner(options.depth().get(), options, &mut rule_stack)
    }

    fn evidence_truncated(omitted: usize) -> Self {
        Self {
            kind: DerivationKind::Truncated,
            label: format!("... {omitted} more aggregate evidence nodes omitted"),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: Some("aggregate evidence limit reached".to_string()),
            children: Vec::new(),
        }
    }

    fn bounded_inner(
        &self,
        remaining_depth: usize,
        options: &ExplainOptions,
        rule_stack: &mut Vec<String>,
    ) -> Self {
        if remaining_depth == 0 {
            return Self {
                kind: DerivationKind::Truncated,
                label: "... more derivation levels (use --explain-depth)".to_string(),
                relation: None,
                predicate: None,
                tuple: Vec::new(),
                fields: BTreeMap::new(),
                source: None,
                line: None,
                column: None,
                truncated: Some("depth limit reached".to_string()),
                children: Vec::new(),
            };
        }

        let fingerprint = self.rule_fingerprint();
        if !options.explicit_depth()
            && let Some(fingerprint) = &fingerprint
            && rule_stack.contains(fingerprint)
        {
            let hops = rule_stack
                .iter()
                .filter(|existing| *existing == fingerprint)
                .count()
                + 1;
            return Self {
                kind: DerivationKind::RecursiveChain,
                label: format!("via {fingerprint} x {hops} recursive hops"),
                relation: None,
                predicate: self.predicate.clone(),
                tuple: self.tuple.clone(),
                fields: BTreeMap::new(),
                source: self.source.clone(),
                line: self.line,
                column: self.column,
                truncated: Some("recursive chain summarized".to_string()),
                children: Vec::new(),
            };
        }

        if let Some(fingerprint) = &fingerprint {
            rule_stack.push(fingerprint.clone());
        }
        let mut node = self.clone();
        node.children = self
            .children
            .iter()
            .map(|child| child.bounded_inner(remaining_depth - 1, options, rule_stack))
            .collect();
        if fingerprint.is_some() {
            rule_stack.pop();
        }
        node
    }

    fn rule_fingerprint(&self) -> Option<String> {
        if self.kind != DerivationKind::Rule {
            return None;
        }
        let predicate = self.predicate.as_ref()?;
        Some(match (&self.source, self.line) {
            (Some(source), Some(line)) => format!("{predicate}@{source}:{line}"),
            (Some(source), None) => format!("{predicate}@{source}"),
            (None, _) => predicate.clone(),
        })
    }
}

fn non_zero(value: usize) -> Option<usize> {
    (value != 0).then_some(value)
}

fn i64_to_usize(value: i64) -> Option<usize> {
    usize::try_from(value).ok()
}

#[derive(Clone)]
pub struct EvalOptions {
    actor: ActorContext,
    read_full_token_limit: i64,
    low_confidence_threshold: f32,
    explain: ExplainOptions,
    ranker: Arc<dyn Ranker>,
    policy: Arc<dyn Policy>,
}

impl EvalOptions {
    pub fn actor(&self) -> &ActorContext {
        &self.actor
    }

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

    pub fn with_explain(mut self) -> Self {
        if !self.explain.is_enabled() {
            self.explain = ExplainOptions::enabled();
        }
        self
    }

    pub fn with_explain_depth(mut self, depth: usize) -> Self {
        self.explain.enabled = true;
        self.explain.depth = ExplainDepth::new(depth);
        self.explain.explicit_depth = true;
        self
    }

    pub fn with_explain_first(mut self, rows: usize) -> Self {
        self.explain = self.explain.with_first_rows(rows);
        self
    }

    pub fn with_explain_all(mut self) -> Self {
        self.explain = self.explain.with_all_rows();
        self
    }

    pub fn with_explain_options(mut self, explain: ExplainOptions) -> Self {
        self.explain = explain;
        self
    }

    #[must_use]
    pub const fn explain(&self) -> &ExplainOptions {
        &self.explain
    }

    pub fn with_ranker(mut self, ranker: impl Ranker + 'static) -> Self {
        self.ranker = Arc::new(ranker);
        self
    }

    pub fn with_policy(mut self, policy: impl Policy + 'static) -> Self {
        self.policy = Arc::new(policy);
        self
    }

    pub fn authorize_eval(&self) -> Result<(), EvalError> {
        authorize_capability_action(
            &self.actor,
            self.policy.as_ref(),
            Action::Eval,
            RuntimeCapability::Eval,
        )
        .map_err(EvalError::from)
    }

    fn has_capability(&self, capability: RuntimeCapability) -> bool {
        self.actor.has_runtime_capability(capability)
    }

    fn authorize(&self, action: Action) -> Result<(), EvalError> {
        authorize_action(&self.actor, self.policy.as_ref(), action).map_err(EvalError::from)
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
            actor: ActorContext::anonymous_cli().with_runtime_capability(RuntimeCapability::Eval),
            read_full_token_limit: DEFAULT_READ_FULL_TOKEN_LIMIT,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            explain: ExplainOptions::default(),
            ranker: Arc::new(DefaultRanker),
            policy: Arc::new(AllowAllPolicy),
        }
    }
}

impl fmt::Debug for EvalOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvalOptions")
            .field("actor", &self.actor)
            .field("read_full_token_limit", &self.read_full_token_limit)
            .field("low_confidence_threshold", &self.low_confidence_threshold)
            .field("explain", &self.explain)
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

#[derive(Clone)]
pub struct Database {
    stored: BTreeMap<Ident, StoredRelation>,
    #[cfg(feature = "physical-substrate")]
    tuples: Arc<TupleDb>,
    #[cfg(feature = "physical-substrate")]
    tuple_overlay: Arc<TupleOverlay>,
    #[cfg(feature = "physical-substrate")]
    tuple_content: Arc<TupleContentIndex>,
    derived: BTreeMap<PredicateRef, DerivedRelation>,
    graph: Arc<GraphIndex>,
    content: Arc<ContentIndex>,
    search: Arc<OnceLock<SearchIndex>>,
    hidden_handles: Arc<BTreeSet<String>>,
    hidden_content_spans: Arc<BTreeMap<String, BTreeSet<String>>>,
    content_provider: Option<Arc<dyn ContentProvider>>,
    search_provider: Option<Arc<dyn SearchProvider>>,
    introspection: Arc<IntrospectionIndex>,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            stored: BTreeMap::new(),
            #[cfg(feature = "physical-substrate")]
            tuples: Arc::new(TupleDb::default()),
            #[cfg(feature = "physical-substrate")]
            tuple_overlay: Arc::new(TupleOverlay::default()),
            #[cfg(feature = "physical-substrate")]
            tuple_content: Arc::new(TupleContentIndex::default()),
            derived: BTreeMap::new(),
            graph: Arc::new(GraphIndex::default()),
            content: Arc::new(ContentIndex::default()),
            search: Arc::new(OnceLock::new()),
            hidden_handles: Arc::new(BTreeSet::new()),
            hidden_content_spans: Arc::new(BTreeMap::new()),
            content_provider: None,
            search_provider: None,
            introspection: Arc::new(IntrospectionIndex::default()),
        }
    }
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
            .field(
                "search_documents",
                &self.search.get().map_or(0, SearchIndex::len),
            )
            .field("hidden_handles", &self.hidden_handles.len())
            .field(
                "hidden_content_spans",
                &hidden_content_span_count(&self.hidden_content_spans),
            )
            .field("custom_content_provider", &self.content_provider.is_some())
            .field("custom_search_provider", &self.search_provider.is_some())
            .finish_non_exhaustive()
    }
}

impl Database {
    pub fn from_store(store: &FactStore) -> Self {
        Self::from_store_with_visibility(store, |_| true)
    }

    pub fn from_store_for_actor(store: &FactStore, actor: &ActorContext) -> Self {
        Self::from_store_with_visibility(store, |identity| {
            actor.can_see_fact_visibility(store.visibility_for(identity))
        })
    }

    pub fn from_store_for_options(store: &FactStore, options: &EvalOptions) -> Self {
        Self::from_store_for_actor(store, options.actor())
    }

    fn from_store_with_visibility(
        store: &FactStore,
        fact_visible: impl Fn(&FactIdentity) -> bool,
    ) -> Self {
        #[cfg(feature = "physical-substrate")]
        {
            let tuples = TupleDb::from_store_with_visibility(store, &fact_visible);
            let tuple_content = TupleContentIndex::from_tuples(&tuples);
            let hidden_handles = hidden_handles(store, &fact_visible);
            let hidden_content_spans = hidden_content_spans(store, &fact_visible);
            let mut db = Self {
                tuples: Arc::new(tuples),
                tuple_overlay: Arc::new(TupleOverlay::default()),
                tuple_content: Arc::new(tuple_content),
                hidden_handles: Arc::new(hidden_handles),
                hidden_content_spans: Arc::new(hidden_content_spans),
                ..Self::default()
            };
            db.seed_indexes_from_tuples();
            return db;
        }

        #[cfg(not(feature = "physical-substrate"))]
        {
            let mut db = Self::default();
            let hidden_handles = hidden_handles(store, &fact_visible);
            let hidden_content_spans = hidden_content_spans(store, &fact_visible);
            db.insert_named_rows(
                "handle",
                store
                    .handles()
                    .iter()
                    .filter(|fact| fact_visible(&fact.identity))
                    .map(handle_row),
            );
            db.insert_named_rows(
                "edge",
                store
                    .edges()
                    .iter()
                    .filter(|fact| {
                        fact_visible(&fact.identity)
                            && !hidden_handles.contains(&fact.from)
                            && !hidden_handles.contains(&fact.to)
                    })
                    .map(edge_row),
            );
            db.insert_named_rows(
                "meta",
                store
                    .meta()
                    .iter()
                    .filter(|fact| {
                        fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                    })
                    .map(meta_row),
            );
            db.insert_named_rows(
                "content",
                store
                    .content()
                    .iter()
                    .filter(|fact| {
                        fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                    })
                    .map(content_row),
            );
            db.insert_named_rows(
                "span",
                store
                    .spans()
                    .iter()
                    .filter(|fact| {
                        fact_visible(&fact.identity) && !hidden_handles.contains(&fact.handle)
                    })
                    .map(span_row),
            );
            db.insert_named_rows(
                "concern",
                store
                    .concerns()
                    .iter()
                    .filter(|fact| {
                        fact_visible(&fact.identity) && !hidden_handles.contains(&fact.member)
                    })
                    .map(concern_row),
            );
            db.insert_named_rows("config", store.configs().iter().map(config_row));
            db.insert_named_rows(
                "snapshot",
                store
                    .snapshots()
                    .iter()
                    .filter(|fact| !hidden_handles.contains(&fact.id))
                    .map(snapshot_row),
            );
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
            db.hidden_handles = Arc::new(hidden_handles);
            db.hidden_content_spans = Arc::new(hidden_content_spans);
            db
        }
    }

    pub fn with_sources(mut self, sources: impl IntoIterator<Item = SourceInfo>) -> Self {
        self.introspection = Arc::new(IntrospectionIndex::from_sources(
            sources.into_iter().collect(),
        ));
        self
    }

    #[must_use]
    pub fn with_git_mtimes(mut self, mtimes: impl IntoIterator<Item = (String, String)>) -> Self {
        Arc::make_mut(&mut self.graph).git_mtimes = mtimes.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_evaluation_day(mut self, day: i64) -> Self {
        Arc::make_mut(&mut self.graph).evaluation_day = Some(day);
        self
    }

    #[must_use]
    pub fn with_content_provider(mut self, provider: impl ContentProvider + 'static) -> Self {
        self.content_provider = Some(Arc::new(provider));
        self
    }

    #[must_use]
    pub fn with_search_provider(mut self, provider: impl SearchProvider + 'static) -> Self {
        self.search_provider = Some(Arc::new(provider));
        self
    }

    pub fn insert_stored_rows(
        &mut self,
        relation: impl Into<String>,
        rows: impl IntoIterator<Item = NamedRow>,
    ) {
        self.insert_named_rows(&relation.into(), rows);
    }

    pub fn with_trail_store(
        mut self,
        store: &dyn TrailStore,
        request: TrailQuery,
        options: &EvalOptions,
    ) -> Result<Self, TrailError> {
        self.ensure_trail_relations();
        let ctx = TrailContext::new(options.actor(), options.policy.as_ref());
        let entries = store.query(request, &ctx)?;
        self.insert_trail_entries(entries);
        Ok(self)
    }

    fn insert_trail_entries(&mut self, entries: impl IntoIterator<Item = TrailEntryRedacted>) {
        for entry in entries {
            self.insert_trail_entry(&entry);
        }
    }

    pub fn derived(&self, predicate: &PredicateRef) -> Option<&BTreeSet<Tuple>> {
        self.derived.get(predicate).map(DerivedRelation::tuples)
    }

    fn search_provider(&self) -> &dyn SearchProvider {
        match self.search_provider.as_deref() {
            Some(provider) => provider,
            #[cfg(feature = "physical-substrate")]
            None => self.search.get_or_init(|| self.build_search_index()),
            #[cfg(not(feature = "physical-substrate"))]
            None => self.search.get_or_init(|| build_search_index(&self.stored)),
        }
    }

    #[cfg(feature = "physical-substrate")]
    fn seed_indexes_from_tuples(&mut self) {
        let graph = Arc::make_mut(&mut self.graph);
        self.tuples
            .for_each_relation_row(|relation, row| graph.insert_tuple_row(relation, row));
    }

    #[cfg(feature = "physical-substrate")]
    fn build_search_index(&self) -> SearchIndex {
        let mut search = SearchIndex::default();
        self.tuples.for_each_relation_row(|relation, row| {
            insert_search_tuple_row(&mut search, relation, row)
        });
        for (relation, rows) in &self.stored {
            for row in &rows.rows {
                insert_search_row(&mut search, relation, row);
            }
        }
        search
    }

    fn insert_trail_entry(&mut self, entry: &TrailEntryRedacted) {
        self.insert_named_rows(TRAIL_RELATION, [trail_row(entry)]);
        self.insert_named_rows(
            TRAIL_REF_RELATION,
            entry
                .surfaced_refs
                .iter()
                .take(MAX_TRAIL_REFS_PER_ENTRY)
                .enumerate()
                .map(|(ordinal, reference)| {
                    trail_ref_row(entry, TrailRefKind::Surfaced, ordinal, reference)
                })
                .chain(
                    entry
                        .consumed_refs
                        .iter()
                        .take(MAX_TRAIL_REFS_PER_ENTRY)
                        .enumerate()
                        .map(|(ordinal, reference)| {
                            trail_ref_row(entry, TrailRefKind::Consumed, ordinal, reference)
                        }),
                ),
        );
        self.insert_named_rows(
            TRAIL_GENERATION_RELATION,
            entry
                .source_generations
                .iter()
                .take(MAX_TRAIL_GENERATIONS_PER_ENTRY)
                .map(|generation| trail_generation_row(entry, generation)),
        );
    }

    fn ensure_trail_relations(&mut self) {
        for relation in [
            TRAIL_RELATION,
            TRAIL_REF_RELATION,
            TRAIL_GENERATION_RELATION,
        ] {
            let relation = Ident::new_unchecked(relation);
            self.stored
                .entry(relation.clone())
                .or_insert_with(|| StoredRelation::new(relation));
        }
    }

    fn search_tuples(
        &self,
        constraints: &[(usize, Value)],
        options: &EvalOptions,
    ) -> Result<Vec<Tuple>, EvalError> {
        let ArgConstraint::Exact(query_text) = string_constraint(constraints, 0) else {
            return Ok(Vec::new());
        };
        if SearchQuery::parse(query_text).is_none() {
            return Err(EvalError::SearchProvider(SearchError::EmptyQuery));
        }
        let handle = match string_constraint(constraints, 1) {
            ArgConstraint::Any => None,
            ArgConstraint::Exact(handle) => Some(handle),
            ArgConstraint::Impossible => return Ok(Vec::new()),
        };
        options.authorize(Action::Search {
            query: query_text.to_owned(),
            handle: handle.map(str::to_owned),
        })?;
        let span = match search_span_filter(constraints, 2) {
            SearchSpanConstraint::Any => SearchSpanScope::Any,
            SearchSpanConstraint::Null => SearchSpanScope::Null,
            SearchSpanConstraint::Exact(span_id) => SearchSpanScope::Exact(span_id),
            SearchSpanConstraint::Impossible => return Ok(Vec::new()),
        };
        let reason = optional_string_constraint(constraints, 4);
        let field = optional_string_constraint(constraints, 5);
        let request = SearchRequest::new(query_text, handle, span, reason, field);
        let search_ctx = SearchContext::new(&options.actor);
        let hits = self
            .search_provider()
            .search(request, &search_ctx)
            .map_err(EvalError::SearchProvider)?;
        let ctx = options.ranking_context(query_text);
        let ranker = options.ranker();
        let ranked = rank_search_hits(
            hits.into_iter()
                .filter(|hit| self.hit_is_visible(hit.handle(), hit.span_id())),
            &ctx,
            ranker,
        );

        let mut seen = BTreeSet::new();
        Ok(ranked
            .into_iter()
            .map(|hit| search_tuple(&hit, query_text))
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .filter(|tuple| seen.insert(tuple.clone()))
            .collect())
    }

    fn read_tuples(
        &self,
        constraints: &[(usize, Value)],
        options: &EvalOptions,
    ) -> Result<Vec<Tuple>, EvalError> {
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 0) else {
            return Ok(Vec::new());
        };
        let ArgConstraint::Exact(budget) = i64_constraint(constraints, 1) else {
            return Ok(Vec::new());
        };
        if budget < 0 {
            return Ok(Vec::new());
        }
        options.authorize(Action::Read {
            handle: handle.to_owned(),
        })?;
        if self.hidden_handles.contains(handle) {
            return Ok(Vec::new());
        }
        let span_id = match string_constraint(constraints, 2) {
            ArgConstraint::Any => None,
            ArgConstraint::Exact(span_id) => Some(span_id),
            ArgConstraint::Impossible => return Ok(Vec::new()),
        };
        if span_id.is_some_and(|span_id| self.span_is_hidden(handle, span_id)) {
            return Ok(Vec::new());
        }
        let chunks = if let Some(provider) = self.content_provider.as_deref() {
            let read_ctx = ReadContext::new(&options.actor);
            provider
                .read(ReadRequest::new(handle, budget, span_id), &read_ctx)
                .map_err(map_read_error)?
        } else {
            #[cfg(feature = "physical-substrate")]
            {
                self.read_chunks_from_tuples(handle, budget, span_id)
            }
            #[cfg(not(feature = "physical-substrate"))]
            {
                let read_ctx = ReadContext::new(&options.actor);
                self.content
                    .read(ReadRequest::new(handle, budget, span_id), &read_ctx)
                    .map_err(map_read_error)?
            }
        };
        let chunks = chunks
            .into_iter()
            .filter(|chunk| self.hit_is_visible(chunk.handle(), Some(chunk.span_id())))
            .collect();
        Ok(enforce_read_budget(chunks, budget, span_id.is_some())
            .into_iter()
            .map(|chunk| read_tuple(chunk, budget))
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .collect())
    }

    fn read_full_tuples(
        &self,
        constraints: &[(usize, Value)],
        options: &EvalOptions,
    ) -> Result<Vec<Tuple>, EvalError> {
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 0) else {
            return Ok(Vec::new());
        };
        options.authorize(Action::ReadFull {
            handle: handle.to_owned(),
        })?;
        if self.hidden_handles.contains(handle)
            || (self.content_provider.is_some() && self.handle_has_hidden_spans(handle))
        {
            return Ok(Vec::new());
        }
        #[cfg(feature = "physical-substrate")]
        if self.content_provider.is_none() && self.handle_has_hidden_spans(handle) {
            return Ok(Vec::new());
        }
        let content = if let Some(provider) = self.content_provider.as_deref() {
            let read_ctx = ReadContext::new(&options.actor);
            provider
                .read_full(
                    ReadFullRequest::new(handle, options.read_full_token_limit),
                    &read_ctx,
                )
                .map_err(map_read_error)?
        } else {
            #[cfg(feature = "physical-substrate")]
            {
                self.full_content_from_tuples(handle, options.read_full_token_limit)?
            }
            #[cfg(not(feature = "physical-substrate"))]
            {
                let read_ctx = ReadContext::new(&options.actor);
                self.content
                    .read_full(
                        ReadFullRequest::new(handle, options.read_full_token_limit),
                        &read_ctx,
                    )
                    .map_err(map_read_error)?
            }
        };
        let Some(content) = content else {
            return Ok(Vec::new());
        };
        if !self.hit_is_visible(content.handle(), None) {
            return Ok(Vec::new());
        }
        if content.tokens() > options.read_full_token_limit {
            return Err(EvalError::ReadFullBudgetExceeded {
                handle: content.handle().to_owned(),
                tokens: content.tokens(),
                limit: options.read_full_token_limit,
            });
        }
        let tuple = Tuple(vec![
            string_value(content.handle()),
            Value::String(content.text().to_owned()),
        ]);
        Ok(tuple_matches_constraints(&tuple, constraints)
            .then_some(tuple)
            .into_iter()
            .collect())
    }

    fn hit_is_visible(&self, handle: &str, span_id: Option<&str>) -> bool {
        !self.hidden_handles.contains(handle)
            && span_id.is_none_or(|span_id| !self.span_is_hidden(handle, span_id))
    }

    fn span_is_hidden(&self, handle: &str, span_id: &str) -> bool {
        self.hidden_content_spans
            .get(handle)
            .is_some_and(|spans| spans.contains(span_id))
    }

    fn handle_has_hidden_spans(&self, handle: &str) -> bool {
        self.hidden_content_spans.contains_key(handle)
    }

    #[cfg(feature = "physical-substrate")]
    fn read_chunks_from_tuples(
        &self,
        handle: &str,
        budget: i64,
        span_id: Option<&str>,
    ) -> Vec<ReadChunk> {
        if budget < 0 {
            return Vec::new();
        }
        if let Some(span_id) = span_id {
            return self
                .tuple_content
                .content_spans_for_handle_and_span(&self.tuples, handle, span_id)
                .into_iter()
                .filter_map(|span| read_chunk_with_budget_from_tuple(span, budget))
                .collect();
        }
        let mut used = 0_i64;
        let mut out = Vec::new();
        for span in self
            .tuple_content
            .content_spans_for_handle(&self.tuples, handle)
        {
            let next = used.saturating_add(span.tokens);
            if next > budget {
                if out.is_empty()
                    && let Some(chunk) =
                        read_chunk_with_budget_from_tuple(span, budget.saturating_sub(used))
                {
                    out.push(chunk);
                }
                break;
            }
            used = next;
            out.push(read_chunk_from_tuple(span));
        }
        out
    }

    #[cfg(feature = "physical-substrate")]
    fn full_content_from_tuples(
        &self,
        handle: &str,
        token_limit: i64,
    ) -> Result<Option<ReadFullContent>, EvalError> {
        let mut tokens = 0_i64;
        let mut content = String::new();
        for span in self
            .tuple_content
            .content_spans_for_handle(&self.tuples, handle)
        {
            tokens = tokens.saturating_add(span.tokens);
            if tokens > token_limit {
                return Err(EvalError::ReadFullBudgetExceeded {
                    handle: handle.to_owned(),
                    tokens,
                    limit: token_limit,
                });
            }
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(span.text);
        }
        if content.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ReadFullContent::new(handle, content, tokens)))
        }
    }

    #[cfg(feature = "physical-substrate")]
    fn match_tuples_from_tuples(
        &self,
        constraints: &[(usize, Value)],
        regex: &Regex,
    ) -> Vec<Tuple> {
        let ArgConstraint::Exact(pattern) = string_constraint(constraints, 0) else {
            return Vec::new();
        };
        let ArgConstraint::Exact(handle) = string_constraint(constraints, 1) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for span in self
            .tuple_content
            .content_spans_for_handle(&self.tuples, handle)
        {
            for (line_offset, line) in span.text.lines().enumerate() {
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

    fn ensure_derived(&mut self, predicates: impl IntoIterator<Item = PredicateRef>) {
        for predicate in predicates {
            self.derived.entry(predicate).or_default();
        }
    }

    fn install_program_introspection(&mut self, program: &AnalyzedProgram) {
        self.introspection = Arc::new(
            self.introspection
                .for_program(program, self.stored_relation_summaries()),
        );
    }

    fn install_query_introspection(&mut self, query: &AnalyzedQuery) {
        self.introspection = Arc::new(self.introspection.for_query(query));
    }

    fn stored_relation_summaries(&self) -> Vec<StoredRelationSummary> {
        self.stored
            .iter()
            .filter(|(name, _)| !is_static_stored_relation(name.as_str()))
            .map(|(name, relation)| StoredRelationSummary {
                name: name.to_string(),
                fields: relation.field_names(),
            })
            .collect()
    }

    #[cfg(feature = "physical-substrate")]
    fn projected_tuple_rows(&self, relation: &Ident) -> Vec<NamedRow> {
        self.tuples
            .projected_rows(relation.as_str())
            .into_iter()
            .map(string_map_to_named_row)
            .collect()
    }

    #[cfg(feature = "physical-substrate")]
    fn candidate_tuple_rows(
        &self,
        relation: &Ident,
        constraints: &[(Ident, Value)],
    ) -> crate::vm::store::RowCandidates<'_> {
        let constraints = constraints
            .iter()
            .map(|(field, value)| (field.to_string(), value.clone()))
            .collect::<Vec<_>>();
        if let Some(store) = self.tuple_overlay.relation(relation) {
            return self
                .tuples
                .candidate_rows_in_store(relation.as_str(), store, &constraints);
        }
        self.tuples.candidate_rows(relation.as_str(), &constraints)
    }

    #[cfg(feature = "physical-substrate")]
    fn tuple_field_value(&self, relation: &Ident, row: RowId, field: &Ident) -> Option<Value> {
        if let Some(store) = self.tuple_overlay.relation(relation) {
            return self
                .tuples
                .tuple_row_in_named_store(relation.as_str(), store, row)?
                .logical(field.as_str());
        }
        self.tuples
            .logical_field_value(relation.as_str(), row, field.as_str())
    }

    #[cfg(feature = "physical-substrate")]
    fn tuple_named_row(&self, relation: &Ident, row: RowId) -> Option<NamedRow> {
        if let Some(store) = self.tuple_overlay.relation(relation) {
            return self
                .tuples
                .project_row_in_store(relation.as_str(), store, row)
                .map(string_map_to_named_row);
        }
        self.tuples
            .project_row(relation.as_str(), row)
            .map(string_map_to_named_row)
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
            if let Some(search) = Arc::make_mut(&mut self.search).get_mut() {
                insert_search_row(search, &relation, &row);
            }
            stored.push(row);
        }
    }

    #[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
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

    fn scoped_to_time_ref(&self, reference: &str) -> Result<(Self, Vec<QueryWarning>), EvalError> {
        let Some(selection) = self.resolve_snapshot_selection(reference) else {
            return Err(EvalError::UnsupportedTimeRef {
                reference: reference.to_string(),
            });
        };

        #[cfg(not(feature = "legacy-time-clone"))]
        let scoped = self.time_scope_overlay(&selection);
        #[cfg(feature = "legacy-time-clone")]
        let scoped = self.clone_for_time_scope_selection(&selection);

        Ok((
            scoped,
            self.snapshot_partial_history_warnings(reference, &selection.snapshot),
        ))
    }

    #[cfg(any(
        feature = "legacy-time-clone",
        all(test, not(feature = "physical-substrate"))
    ))]
    fn clone_for_time_scope_selection(&self, selection: &SnapshotSelection) -> Self {
        let mut scoped = self.clone_for_time_scope();
        scoped.set_stored_relation_rows(SNAPSHOT_RELATION, selection.rows.clone());
        scoped.apply_handle_snapshot(&selection.rows);
        scoped.graph = Arc::new(self.graph.scoped_to_snapshot(selection));
        scoped
    }

    #[cfg(not(feature = "legacy-time-clone"))]
    fn time_scope_overlay(&self, selection: &SnapshotSelection) -> Self {
        let mut scoped = self.clone_shell_for_time_scope();
        #[cfg(feature = "physical-substrate")]
        {
            scoped.tuple_overlay = Arc::new(self.time_scope_tuple_overlay(selection));
        }
        #[cfg(not(feature = "physical-substrate"))]
        {
            scoped.set_stored_relation_rows(SNAPSHOT_RELATION, selection.rows.clone());
            if let Some(rows) = self.time_scoped_handle_rows(&selection.rows) {
                scoped.set_stored_relation_rows(HANDLE_RELATION, rows);
            }
            scoped.graph = Arc::new(self.graph.scoped_to_snapshot(selection));
        }
        #[cfg(feature = "physical-substrate")]
        {
            scoped.graph = Arc::new(
                self.graph
                    .scoped_to_snapshot_tuples(&self.tuples, selection),
            );
        }
        scoped
    }

    fn resolve_snapshot_selection(&self, reference: &str) -> Option<SnapshotSelection> {
        #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
        let candidates = self.snapshot_candidates_from_tuples();
        #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
        let candidates = (!candidates.is_empty()).then_some(candidates)?;

        #[cfg(not(feature = "physical-substrate"))]
        let candidates = {
            let relation = self.stored.get(&Ident::new_unchecked(SNAPSHOT_RELATION))?;
            snapshot_candidates(&relation.rows)
        };
        #[cfg(all(feature = "physical-substrate", feature = "legacy-time-clone"))]
        let candidates = {
            let rows = self.relation_rows_for_scope(SNAPSHOT_RELATION)?;
            snapshot_candidates(&rows)
        };
        match snapshot_reference(reference)? {
            SnapshotReference::Last => latest_snapshot_candidate(candidates.into_values()),
            SnapshotReference::Snapshot(id) => candidates.get(&id).cloned().map(Into::into),
            SnapshotReference::Day(target_day) => {
                nearest_snapshot_candidate(candidates.into_values(), target_day)
            }
        }
    }

    #[cfg(any(
        all(feature = "legacy-time-clone", not(feature = "physical-substrate")),
        all(test, not(feature = "physical-substrate"))
    ))]
    fn clone_for_time_scope(&self) -> Self {
        self.clone_for_time_scope_with_stored(self.stored.clone())
    }

    #[cfg(all(feature = "legacy-time-clone", feature = "physical-substrate"))]
    fn clone_for_time_scope(&self) -> Self {
        self.clone_for_time_scope_with_stored(self.materialized_stored_relations())
    }

    #[cfg(feature = "physical-substrate")]
    #[allow(dead_code)]
    fn materialized_stored_relations(&self) -> BTreeMap<Ident, StoredRelation> {
        let mut stored = self.stored.clone();
        for relation in self.tuples.relation_names() {
            let relation = Ident::new_unchecked(relation);
            stored.entry(relation.clone()).or_insert_with(|| {
                StoredRelation::from_rows(relation.clone(), self.projected_tuple_rows(&relation))
            });
        }
        stored
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn snapshot_candidates_from_tuples(&self) -> BTreeMap<String, SnapshotCandidate> {
        let mut candidates = BTreeMap::<String, SnapshotCandidate>::new();
        self.tuples
            .for_each_tuple_row_id(SNAPSHOT_RELATION, |row_id, row| {
                let Some(at) = row.string(AT_FIELD) else {
                    return;
                };
                let Some(day) = snapshot_days_since_epoch(at) else {
                    return;
                };
                let snapshot = row.string(SNAPSHOT_FIELD).unwrap_or(at).to_string();
                candidates
                    .entry(snapshot.clone())
                    .or_insert_with(|| SnapshotCandidate {
                        snapshot,
                        day,
                        sort_at: at.to_string(),
                        rows: Vec::new(),
                        tuple_rows: Vec::new(),
                    })
                    .tuple_rows
                    .push(row_id);
            });
        candidates
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn time_scope_tuple_overlay(&self, selection: &SnapshotSelection) -> TupleOverlay {
        let mut overlay = TupleOverlay::default();
        if let Some(snapshot_store) = self.snapshot_overlay_store(&selection.tuple_rows) {
            overlay.insert(Ident::new_unchecked(SNAPSHOT_RELATION), snapshot_store);
        }
        if let Some(handle_store) = self.handle_overlay_store(&selection.tuple_rows) {
            overlay.insert(Ident::new_unchecked(HANDLE_RELATION), handle_store);
        }
        overlay
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn snapshot_overlay_store(&self, snapshot_rows: &[RowId]) -> Option<RelationStore> {
        let mut store = self.tuples.empty_relation_store(SNAPSHOT_RELATION)?;
        for row in snapshot_rows {
            if let Some(tuple) = self.tuples.clone_tuple(SNAPSHOT_RELATION, *row) {
                store.push(tuple);
            }
        }
        Some(store)
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn handle_overlay_store(&self, snapshot_rows: &[RowId]) -> Option<RelationStore> {
        let patches = self.handle_snapshot_tuple_patches(snapshot_rows);
        if patches.is_empty() {
            return None;
        }
        let mut store = self.tuples.empty_relation_store(HANDLE_RELATION)?;
        self.tuples
            .for_each_tuple_row_id(HANDLE_RELATION, |row_id, row| {
                let tuple = match (row.string(CORPUS_FIELD), row.string(ID_FIELD)) {
                    (Some(corpus), Some(handle)) => patches
                        .get(&(corpus.to_owned(), handle.to_owned()))
                        .and_then(|fields| {
                            self.tuples
                                .clone_tuple_with_patches(HANDLE_RELATION, row_id, fields)
                        })
                        .or_else(|| self.tuples.clone_tuple(HANDLE_RELATION, row_id)),
                    _ => self.tuples.clone_tuple(HANDLE_RELATION, row_id),
                };
                if let Some(tuple) = tuple {
                    store.push(tuple);
                }
            });
        Some(store)
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn handle_snapshot_tuple_patches(
        &self,
        snapshot_rows: &[RowId],
    ) -> BTreeMap<(String, String), BTreeMap<&'static str, PhysicalValue>> {
        let mut patches =
            BTreeMap::<(String, String), BTreeMap<&'static str, PhysicalValue>>::new();
        for row in snapshot_rows {
            let Some(row) = self.tuples.tuple_row(SNAPSHOT_RELATION, *row) else {
                continue;
            };
            let (Some(corpus), Some(id), Some(key)) = (
                row.string(CORPUS_FIELD),
                row.string(ID_FIELD),
                row.string(KEY_FIELD),
            ) else {
                continue;
            };
            let Some(value) = row.physical(VALUE_FIELD) else {
                continue;
            };
            let Some(field) = handle_snapshot_patch_field(&key) else {
                continue;
            };
            patches
                .entry((corpus.to_owned(), id.to_owned()))
                .or_default()
                .insert(field, value);
        }
        patches
    }

    #[cfg(not(feature = "legacy-time-clone"))]
    fn clone_shell_for_time_scope(&self) -> Self {
        self.clone_for_time_scope_with_stored(BTreeMap::new())
    }

    fn clone_for_time_scope_with_stored(&self, stored: BTreeMap<Ident, StoredRelation>) -> Self {
        Self {
            stored,
            #[cfg(feature = "physical-substrate")]
            tuples: Arc::clone(&self.tuples),
            #[cfg(feature = "physical-substrate")]
            tuple_overlay: Arc::new(TupleOverlay::default()),
            #[cfg(feature = "physical-substrate")]
            tuple_content: Arc::clone(&self.tuple_content),
            derived: BTreeMap::new(),
            graph: Arc::clone(&self.graph),
            content: Arc::clone(&self.content),
            search: Arc::clone(&self.search),
            hidden_handles: Arc::clone(&self.hidden_handles),
            hidden_content_spans: Arc::clone(&self.hidden_content_spans),
            content_provider: self.content_provider.clone(),
            search_provider: self.search_provider.clone(),
            introspection: Arc::clone(&self.introspection),
        }
    }

    #[cfg(any(
        feature = "legacy-time-clone",
        all(test, not(feature = "physical-substrate"))
    ))]
    fn apply_handle_snapshot(&mut self, snapshot_rows: &[NamedRow]) {
        let Some(handles) = self.stored.get(&Ident::new_unchecked(HANDLE_RELATION)) else {
            return;
        };
        if let Some(rows) = patched_handle_rows(&handles.rows, snapshot_rows) {
            self.set_stored_relation_rows(HANDLE_RELATION, rows);
        }
    }

    #[cfg(all(
        not(feature = "legacy-time-clone"),
        not(feature = "physical-substrate")
    ))]
    fn time_scoped_handle_rows(&self, snapshot_rows: &[NamedRow]) -> Option<Vec<NamedRow>> {
        let handles = self.stored.get(&Ident::new_unchecked(HANDLE_RELATION))?;
        Some(
            patched_handle_rows(&handles.rows, snapshot_rows)
                .unwrap_or_else(|| handles.rows.clone()),
        )
    }

    #[cfg(all(feature = "physical-substrate", feature = "legacy-time-clone"))]
    fn relation_rows_for_scope(&self, relation: &str) -> Option<Vec<NamedRow>> {
        let relation_ident = Ident::new_unchecked(relation);
        if let Some(stored) = self.stored.get(&relation_ident) {
            return Some(stored.rows.clone());
        }
        let rows = self.projected_tuple_rows(&relation_ident);
        (!rows.is_empty()).then_some(rows)
    }

    fn snapshot_partial_history_warnings(
        &self,
        reference: &str,
        snapshot: &str,
    ) -> Vec<QueryWarning> {
        #[cfg(feature = "physical-substrate")]
        let sources = self.handle_sources_from_tuples();
        #[cfg(not(feature = "physical-substrate"))]
        let handle_rows = self
            .stored
            .get(&Ident::new_unchecked(HANDLE_RELATION))
            .map(|relation| relation.rows.clone());

        #[cfg(not(feature = "physical-substrate"))]
        let sources = handle_rows
            .into_iter()
            .flat_map(std::iter::IntoIterator::into_iter)
            .filter_map(|row| row_string(&row, SOURCE_FIELD).map(str::to_string))
            .collect::<BTreeSet<_>>();
        if sources.is_empty() {
            return vec![snapshot_partial_history_warning(reference, snapshot, None)];
        }
        sources
            .into_iter()
            .map(|source| snapshot_partial_history_warning(reference, snapshot, Some(source)))
            .collect()
    }

    #[cfg(feature = "physical-substrate")]
    fn handle_sources_from_tuples(&self) -> BTreeSet<String> {
        let mut sources = BTreeSet::new();
        self.tuples.for_each_tuple_row(HANDLE_RELATION, |row| {
            if let Some(source) = row.string(SOURCE_FIELD) {
                sources.insert(source.to_owned());
            }
        });
        sources
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotSelection {
    snapshot: String,
    day: i64,
    rows: Vec<NamedRow>,
    #[cfg(feature = "physical-substrate")]
    tuple_rows: Vec<RowId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotCandidate {
    snapshot: String,
    day: i64,
    sort_at: String,
    rows: Vec<NamedRow>,
    #[cfg(feature = "physical-substrate")]
    tuple_rows: Vec<RowId>,
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

#[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
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
                #[cfg(feature = "physical-substrate")]
                tuple_rows: Vec::new(),
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
            #[cfg(feature = "physical-substrate")]
            tuple_rows: candidate.tuple_rows,
        }
    }
}

#[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
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

#[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
fn patched_handle_rows(handles: &[NamedRow], snapshot_rows: &[NamedRow]) -> Option<Vec<NamedRow>> {
    let patches = handle_snapshot_patches(snapshot_rows);
    if patches.is_empty() {
        return None;
    }
    Some(
        handles
            .iter()
            .map(|row| apply_handle_snapshot_patch(row, &patches))
            .collect(),
    )
}

#[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
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

#[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
fn handle_snapshot_patch_field(key: &str) -> Option<&'static str> {
    match key {
        KIND_FIELD => Some(KIND_FIELD),
        STATUS_FIELD => Some(STATUS_FIELD),
        NAMESPACE_FIELD => Some(NAMESPACE_FIELD),
        FILE_FIELD => Some(FILE_FIELD),
        DATE_FIELD => Some(DATE_FIELD),
        AREA_FIELD => Some(AREA_FIELD),
        SUMMARY_FIELD => Some(SUMMARY_FIELD),
        _ => None,
    }
}

fn push_warnings(out: &mut Vec<QueryWarning>, warnings: Vec<QueryWarning>) {
    for warning in warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
}

fn map_read_error(error: ReadError) -> EvalError {
    match error {
        ReadError::BudgetExceeded {
            handle,
            tokens,
            limit,
        } => EvalError::ReadFullBudgetExceeded {
            handle,
            tokens,
            limit,
        },
        other @ ReadError::Other(_) => EvalError::ReadProvider(other),
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

    #[allow(dead_code)]
    fn from_rows(relation: Ident, rows: impl IntoIterator<Item = NamedRow>) -> Self {
        let mut stored = Self::new(relation);
        for row in rows {
            stored.push(row);
        }
        stored
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

    fn field_names(&self) -> Vec<String> {
        let mut fields = BTreeSet::new();
        for row in &self.rows {
            fields.extend(row.keys().map(ToString::to_string));
        }
        fields.into_iter().collect()
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
    span_keys_by_handle_span: BTreeMap<(String, String), BTreeSet<ContentKey>>,
    span_order_by_handle: BTreeMap<String, BTreeSet<OrderedSpanKey>>,
}

#[cfg(feature = "physical-substrate")]
#[derive(Clone, Debug, Default)]
struct TupleContentIndex {
    content_rows: BTreeMap<ContentKey, RowId>,
    spans: BTreeMap<ContentKey, SpanPayload>,
    span_keys_by_handle_span: BTreeMap<(String, String), BTreeSet<ContentKey>>,
    span_order_by_handle: BTreeMap<String, BTreeSet<OrderedSpanKey>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContentKey {
    corpus: String,
    source: String,
    handle: String,
    span_id: String,
}

impl ContentKey {
    fn new(corpus: &str, source: &str, handle: &str, span_id: &str) -> Self {
        Self {
            corpus: corpus.to_owned(),
            source: source.to_owned(),
            handle: handle.to_owned(),
            span_id: span_id.to_owned(),
        }
    }
}

impl Ord for ContentKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.corpus
            .cmp(&other.corpus)
            .then_with(|| self.source.cmp(&other.source))
            .then_with(|| self.handle.cmp(&other.handle))
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
    corpus: String,
    source: String,
    start_line: i64,
    span_id: String,
}

impl OrderedSpanKey {
    fn new(corpus: &str, source: &str, span_id: &str, start_line: i64) -> Self {
        Self {
            corpus: corpus.to_owned(),
            source: source.to_owned(),
            start_line,
            span_id: span_id.to_owned(),
        }
    }
}

impl Ord for OrderedSpanKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.corpus
            .cmp(&other.corpus)
            .then_with(|| self.source.cmp(&other.source))
            .then_with(|| self.start_line.cmp(&other.start_line))
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

#[cfg(feature = "physical-substrate")]
#[derive(Clone, Copy, Debug)]
struct TupleContentSpan<'a> {
    key: &'a ContentKey,
    text: &'a str,
    tokens: i64,
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
        let (Some(corpus), Some(source), Some(handle), Some(span_id), Some(text), Some(tokens)) = (
            row_string(row, CORPUS_FIELD),
            row_string(row, SOURCE_FIELD),
            row_string(row, HANDLE_FIELD),
            row_string(row, SPAN_ID_FIELD),
            row_string(row, TEXT_FIELD),
            row_i64(row, TOKENS_FIELD),
        ) else {
            return;
        };
        let key = ContentKey::new(corpus, source, handle, span_id);
        let payload = ContentPayload {
            text: text.to_owned(),
            tokens,
        };
        self.content.insert(key, payload);
    }

    fn insert_span(&mut self, row: &NamedRow) {
        let (
            Some(corpus),
            Some(source),
            Some(handle),
            Some(span_id),
            Some(start_line),
            Some(end_line),
        ) = (
            row_string(row, CORPUS_FIELD),
            row_string(row, SOURCE_FIELD),
            row_string(row, HANDLE_FIELD),
            row_string(row, ID_FIELD),
            row_i64(row, START_LINE_FIELD),
            row_i64(row, END_LINE_FIELD),
        )
        else {
            return;
        };
        let key = ContentKey::new(corpus, source, handle, span_id);
        let payload = SpanPayload {
            start_line,
            end_line,
        };
        self.span_order_by_handle
            .entry(handle.to_owned())
            .or_default()
            .insert(OrderedSpanKey::new(corpus, source, span_id, start_line));
        self.span_keys_by_handle_span
            .entry((handle.to_owned(), span_id.to_owned()))
            .or_default()
            .insert(key.clone());
        self.spans.insert(key, payload);
    }

    #[cfg(not(feature = "physical-substrate"))]
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
                    self.content_span(&ContentKey::new(
                        &ordered_key.corpus,
                        &ordered_key.source,
                        handle,
                        &ordered_key.span_id,
                    ))
                })
            })
    }

    fn content_spans_for_handle_and_span<'a>(
        &'a self,
        handle: &'a str,
        span_id: &'a str,
    ) -> impl Iterator<Item = ContentSpan<'a>> + 'a {
        self.span_keys_by_handle_span
            .get(&(handle.to_owned(), span_id.to_owned()))
            .into_iter()
            .flat_map(|keys| keys.iter().filter_map(|key| self.content_span(key)))
    }

    fn full_content_under_limit(
        &self,
        handle: &str,
        token_limit: i64,
    ) -> Result<Option<ReadFullContent>, ReadError> {
        let mut tokens = 0_i64;
        let mut content = String::new();
        for span in self.content_spans_for_handle(handle) {
            tokens = tokens.saturating_add(span.content.tokens);
            if tokens > token_limit {
                return Err(ReadError::BudgetExceeded {
                    handle: handle.to_owned(),
                    tokens,
                    limit: token_limit,
                });
            }
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&span.content.text);
        }
        if content.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ReadFullContent::new(handle, content, tokens)))
        }
    }
}

#[cfg(feature = "physical-substrate")]
impl TupleContentIndex {
    fn from_tuples(tuples: &TupleDb) -> Self {
        let mut index = Self::default();
        tuples.for_each_tuple_row_id(CONTENT_RELATION, |row_id, row| {
            let (Some(corpus), Some(source), Some(handle), Some(span_id)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(HANDLE_FIELD),
                row.string(SPAN_ID_FIELD),
            ) else {
                return;
            };
            index
                .content_rows
                .insert(ContentKey::new(corpus, source, handle, span_id), row_id);
        });
        tuples.for_each_tuple_row(SPAN_RELATION, |row| {
            let (
                Some(corpus),
                Some(source),
                Some(handle),
                Some(span_id),
                Some(start_line),
                Some(end_line),
            ) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(HANDLE_FIELD),
                row.string(ID_FIELD),
                row.i64(START_LINE_FIELD),
                row.i64(END_LINE_FIELD),
            )
            else {
                return;
            };
            let key = ContentKey::new(corpus, source, handle, span_id);
            index
                .span_order_by_handle
                .entry(handle.to_owned())
                .or_default()
                .insert(OrderedSpanKey::new(corpus, source, span_id, start_line));
            index
                .span_keys_by_handle_span
                .entry((handle.to_owned(), span_id.to_owned()))
                .or_default()
                .insert(key.clone());
            index.spans.insert(
                key,
                SpanPayload {
                    start_line,
                    end_line,
                },
            );
        });
        index
    }

    fn content_span<'a>(
        &'a self,
        tuples: &'a TupleDb,
        key: &'a ContentKey,
    ) -> Option<TupleContentSpan<'a>> {
        let span = self.spans.get(key)?;
        let row = tuples.tuple_row(CONTENT_RELATION, *self.content_rows.get(key)?)?;
        Some(TupleContentSpan {
            key,
            text: row.string(TEXT_FIELD)?,
            tokens: row.i64(TOKENS_FIELD)?,
            span,
        })
    }

    fn content_span_for_ordered_key<'a>(
        &'a self,
        tuples: &'a TupleDb,
        handle: &str,
        ordered_key: &OrderedSpanKey,
    ) -> Option<TupleContentSpan<'a>> {
        let lookup = ContentKey::new(
            &ordered_key.corpus,
            &ordered_key.source,
            handle,
            &ordered_key.span_id,
        );
        let (key, _) = self.content_rows.get_key_value(&lookup)?;
        self.content_span(tuples, key)
    }

    fn content_spans_for_handle<'a>(
        &'a self,
        tuples: &'a TupleDb,
        handle: &'a str,
    ) -> Vec<TupleContentSpan<'a>> {
        self.span_order_by_handle
            .get(handle)
            .into_iter()
            .flat_map(move |ordered_keys| {
                ordered_keys.iter().filter_map(move |ordered_key| {
                    self.content_span_for_ordered_key(tuples, handle, ordered_key)
                })
            })
            .collect()
    }

    fn content_spans_for_handle_and_span<'a>(
        &'a self,
        tuples: &'a TupleDb,
        handle: &'a str,
        span_id: &'a str,
    ) -> Vec<TupleContentSpan<'a>> {
        self.span_keys_by_handle_span
            .get(&(handle.to_owned(), span_id.to_owned()))
            .into_iter()
            .flat_map(|keys| keys.iter().filter_map(|key| self.content_span(tuples, key)))
            .collect()
    }
}

impl ContentProvider for ContentIndex {
    fn read(
        &self,
        request: ReadRequest<'_>,
        _ctx: &ReadContext<'_>,
    ) -> Result<Vec<ReadChunk>, ReadError> {
        if request.budget() < 0 {
            return Ok(Vec::new());
        }
        if let Some(span_id) = request.span_id() {
            return Ok(self
                .content_spans_for_handle_and_span(request.handle(), span_id)
                .filter_map(|span| read_chunk_with_budget(span, request.budget()))
                .collect());
        }
        let mut used = 0_i64;
        let mut out = Vec::new();
        for span in self.content_spans_for_handle(request.handle()) {
            let next = used.saturating_add(span.content.tokens);
            if next > request.budget() {
                if out.is_empty()
                    && let Some(chunk) =
                        read_chunk_with_budget(span, request.budget().saturating_sub(used))
                {
                    out.push(chunk);
                }
                break;
            }
            used = next;
            out.push(read_chunk(span));
        }
        Ok(out)
    }

    fn read_full(
        &self,
        request: ReadFullRequest<'_>,
        _ctx: &ReadContext<'_>,
    ) -> Result<Option<ReadFullContent>, ReadError> {
        self.full_content_under_limit(request.handle(), request.token_limit())
    }
}

fn read_chunk(span: ContentSpan<'_>) -> ReadChunk {
    ReadChunk::new(
        &span.key.handle,
        &span.key.span_id,
        span.content.text.clone(),
        span.span.start_line,
        span.span.end_line,
        span.content.tokens,
    )
}

fn read_chunk_with_budget(span: ContentSpan<'_>, budget: i64) -> Option<ReadChunk> {
    if budget <= 0 {
        return None;
    }
    if span.content.tokens <= budget {
        return Some(read_chunk(span));
    }
    Some(ReadChunk::new(
        &span.key.handle,
        &span.key.span_id,
        clip_text_to_budget(&span.content.text, budget),
        span.span.start_line,
        span.span.end_line,
        budget,
    ))
}

#[cfg(feature = "physical-substrate")]
fn read_chunk_from_tuple(span: TupleContentSpan<'_>) -> ReadChunk {
    ReadChunk::new(
        &span.key.handle,
        &span.key.span_id,
        span.text.to_owned(),
        span.span.start_line,
        span.span.end_line,
        span.tokens,
    )
}

#[cfg(feature = "physical-substrate")]
fn read_chunk_with_budget_from_tuple(span: TupleContentSpan<'_>, budget: i64) -> Option<ReadChunk> {
    if budget <= 0 {
        return None;
    }
    if span.tokens <= budget {
        return Some(read_chunk_from_tuple(span));
    }
    Some(ReadChunk::new(
        &span.key.handle,
        &span.key.span_id,
        clip_text_to_budget(span.text, budget),
        span.span.start_line,
        span.span.end_line,
        budget,
    ))
}

fn clip_text_to_budget(text: &str, budget: i64) -> String {
    let char_budget = usize::try_from(budget)
        .ok()
        .and_then(|budget| budget.checked_mul(4))
        .unwrap_or(usize::MAX);
    let mut clipped = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index == char_budget {
            clipped.push_str("\n...");
            break;
        }
        clipped.push(ch);
    }
    clipped
}

fn enforce_read_budget(chunks: Vec<ReadChunk>, budget: i64, exact_span: bool) -> Vec<ReadChunk> {
    if exact_span {
        return chunks
            .into_iter()
            .filter(|chunk| chunk.tokens() <= budget)
            .collect();
    }
    let mut used = 0_i64;
    let mut out = Vec::new();
    for chunk in chunks {
        let next = used.saturating_add(chunk.tokens());
        if next > budget {
            break;
        }
        used = next;
        out.push(chunk);
    }
    out
}

fn read_tuple(chunk: ReadChunk, budget: i64) -> Tuple {
    let chunk = chunk.into_parts();
    Tuple(vec![
        string_value(&chunk.handle),
        int_value(budget),
        string_value(&chunk.span_id),
        Value::String(chunk.text),
        int_value(chunk.start_line),
        int_value(chunk.end_line),
        int_value(chunk.tokens),
    ])
}

fn search_tuple(hit: &crate::ranking::RankedSearchHit, query: &str) -> Tuple {
    let hit_data = hit.hit();
    Tuple(vec![
        string_value(query),
        string_value(hit_data.handle()),
        hit_data.span_id().map_or(Value::Null, string_value),
        float_value(f64::from(hit.score().get())),
        string_value(hit_data.reason()),
        string_value(hit_data.field()),
        Value::Bool(hit.low_confidence()),
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchSpanConstraint<'a> {
    Any,
    Null,
    Exact(&'a str),
    Impossible,
}

fn search_span_filter(constraints: &[(usize, Value)], position: usize) -> SearchSpanConstraint<'_> {
    let Some((_, value)) = constraints.iter().find(|(idx, _)| *idx == position) else {
        return SearchSpanConstraint::Any;
    };
    match value {
        Value::Null => SearchSpanConstraint::Null,
        Value::String(value) => SearchSpanConstraint::Exact(value),
        Value::Number(_) | Value::Bool(_) | Value::List(_) => SearchSpanConstraint::Impossible,
    }
}

fn optional_string_constraint(constraints: &[(usize, Value)], position: usize) -> Option<&str> {
    match string_constraint(constraints, position) {
        ArgConstraint::Any | ArgConstraint::Impossible => None,
        ArgConstraint::Exact(value) => Some(value),
    }
}

#[cfg(not(feature = "physical-substrate"))]
fn build_search_index(stored: &BTreeMap<Ident, StoredRelation>) -> SearchIndex {
    let mut search = SearchIndex::default();
    for (relation, rows) in stored {
        for row in &rows.rows {
            insert_search_row(&mut search, relation, row);
        }
    }
    search
}

fn insert_search_row(search: &mut SearchIndex, relation: &Ident, row: &NamedRow) {
    match relation.as_str() {
        HANDLE_RELATION => {
            let (Some(corpus), Some(source), Some(handle)) = (
                row_string(row, CORPUS_FIELD),
                row_string(row, SOURCE_FIELD),
                row_string(row, ID_FIELD),
            ) else {
                return;
            };
            let file = row_string(row, FILE_FIELD).unwrap_or(handle);
            search.insert_handle(SearchHandleDocument {
                corpus,
                source,
                handle,
                file,
                summary: row_string(row, SUMMARY_FIELD),
                status: row_string(row, STATUS_FIELD),
                namespace: row_string(row, NAMESPACE_FIELD),
                area: row_string(row, AREA_FIELD),
                kind: row_string(row, KIND_FIELD),
            });
        }
        EDGE_RELATION => {
            let (Some(corpus), Some(source), Some(from), Some(to)) = (
                row_string(row, CORPUS_FIELD),
                row_string(row, SOURCE_FIELD),
                row_string(row, FROM_FIELD),
                row_string(row, TO_FIELD),
            ) else {
                return;
            };
            search.insert_edge(corpus, source, from, to);
        }
        META_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(key), Some(value)) = (
                row_string(row, CORPUS_FIELD),
                row_string(row, SOURCE_FIELD),
                row_string(row, HANDLE_FIELD),
                row_string(row, KEY_FIELD),
                row_string(row, VALUE_FIELD),
            ) else {
                return;
            };
            search.insert_meta(corpus, source, handle, key, value);
        }
        CONFIG_RELATION => {
            let (Some(key), Some(value)) =
                (row_string(row, KEY_FIELD), row_string(row, VALUE_FIELD))
            else {
                return;
            };
            search.insert_config(key, value);
        }
        CONTENT_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(span_id), Some(text)) = (
                row_string(row, CORPUS_FIELD),
                row_string(row, SOURCE_FIELD),
                row_string(row, HANDLE_FIELD),
                row_string(row, SPAN_ID_FIELD),
                row_string(row, TEXT_FIELD),
            ) else {
                return;
            };
            search.insert_content(corpus, source, handle, span_id, text);
        }
        SPAN_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(span_id), Some(summary)) = (
                row_string(row, CORPUS_FIELD),
                row_string(row, SOURCE_FIELD),
                row_string(row, HANDLE_FIELD),
                row_string(row, ID_FIELD),
                row_string(row, SUMMARY_FIELD),
            ) else {
                return;
            };
            search.insert_span_summary(corpus, source, handle, span_id, summary);
        }
        _ => {}
    }
}

#[cfg(feature = "physical-substrate")]
fn insert_search_tuple_row(search: &mut SearchIndex, relation: &str, row: TupleRow<'_>) {
    match relation {
        HANDLE_RELATION => {
            let (Some(corpus), Some(source), Some(handle)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(ID_FIELD),
            ) else {
                return;
            };
            let file = row.string(FILE_FIELD).unwrap_or(handle);
            search.insert_handle(SearchHandleDocument {
                corpus,
                source,
                handle,
                file,
                summary: row.string(SUMMARY_FIELD),
                status: row.string(STATUS_FIELD),
                namespace: row.string(NAMESPACE_FIELD),
                area: row.string(AREA_FIELD),
                kind: row.string(KIND_FIELD),
            });
        }
        EDGE_RELATION => {
            let (Some(corpus), Some(source), Some(from), Some(to)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(FROM_FIELD),
                row.string(TO_FIELD),
            ) else {
                return;
            };
            search.insert_edge(corpus, source, from, to);
        }
        META_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(key), Some(value)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(HANDLE_FIELD),
                row.string(KEY_FIELD),
                row.string(VALUE_FIELD),
            ) else {
                return;
            };
            search.insert_meta(corpus, source, handle, key, value);
        }
        CONFIG_RELATION => {
            let (Some(key), Some(value)) = (row.string(KEY_FIELD), row.string(VALUE_FIELD)) else {
                return;
            };
            search.insert_config(key, value);
        }
        CONTENT_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(span_id), Some(text)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(HANDLE_FIELD),
                row.string(SPAN_ID_FIELD),
                row.string(TEXT_FIELD),
            ) else {
                return;
            };
            search.insert_content(corpus, source, handle, span_id, text);
        }
        SPAN_RELATION => {
            let (Some(corpus), Some(source), Some(handle), Some(span_id), Some(summary)) = (
                row.string(CORPUS_FIELD),
                row.string(SOURCE_FIELD),
                row.string(HANDLE_FIELD),
                row.string(ID_FIELD),
                row.string(SUMMARY_FIELD),
            ) else {
                return;
            };
            search.insert_span_summary(corpus, source, handle, span_id, summary);
        }
        _ => {}
    }
}

impl SearchProvider for SearchIndex {
    fn search(
        &self,
        request: SearchRequest<'_>,
        _ctx: &SearchContext<'_>,
    ) -> Result<Vec<crate::ranking::SearchHit>, SearchError> {
        let Some(query) = SearchQuery::parse(request.query()) else {
            return Err(SearchError::EmptyQuery);
        };
        Ok(self.search_hits(
            &query,
            request.handle(),
            request.span(),
            request.reason(),
            request.field(),
        ))
    }
}

#[derive(Clone, Debug, Default)]
struct GraphIndex {
    nodes: BTreeSet<String>,
    handles: BTreeMap<String, HandleState>,
    outgoing: BTreeMap<String, BTreeSet<String>>,
    incoming: BTreeMap<String, BTreeSet<String>>,
    outgoing_edges: BTreeMap<String, BTreeSet<(String, String)>>,
    incoming_edges: BTreeMap<String, BTreeSet<(String, String)>>,
    impact_traverse: BTreeSet<String>,
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
    git_mtimes: BTreeMap<String, String>,
    evaluation_day: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HandleState {
    kind: String,
    status: Option<String>,
    namespace: String,
    file: String,
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
                            file: row_string(row, FILE_FIELD).unwrap_or_default().to_owned(),
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
                if let Some(kind) = row_string(row, KIND_FIELD) {
                    self.outgoing_edges
                        .entry(from.clone())
                        .or_default()
                        .insert((kind.to_owned(), to.clone()));
                    self.incoming_edges
                        .entry(to.clone())
                        .or_default()
                        .insert((kind.to_owned(), from.clone()));
                }
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

    #[cfg(feature = "physical-substrate")]
    fn insert_tuple_row(&mut self, relation: &str, row: TupleRow<'_>) {
        match relation {
            HANDLE_RELATION => {
                if let Some(id) = row.string(ID_FIELD) {
                    self.nodes.insert(id.to_owned());
                    self.handles.insert(
                        id.to_owned(),
                        HandleState {
                            kind: row.string(KIND_FIELD).unwrap_or_default().to_owned(),
                            status: row.string(STATUS_FIELD).map(str::to_owned),
                            namespace: row.string(NAMESPACE_FIELD).unwrap_or_default().to_owned(),
                            file: row.string(FILE_FIELD).unwrap_or_default().to_owned(),
                            date: row.string(DATE_FIELD).and_then(iso_days_since_epoch),
                        },
                    );
                }
            }
            EDGE_RELATION => {
                let (Some(from), Some(to)) = (row.string(FROM_FIELD), row.string(TO_FIELD)) else {
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
                if let Some(kind) = row.string(KIND_FIELD) {
                    self.outgoing_edges
                        .entry(from.clone())
                        .or_default()
                        .insert((kind.to_owned(), to.clone()));
                    self.incoming_edges
                        .entry(to.clone())
                        .or_default()
                        .insert((kind.to_owned(), from.clone()));
                }
                *self.out_edge_count.entry(from).or_default() += 1;
                *self.in_edge_count.entry(to.clone()).or_default() += 1;
                if row.string(KIND_FIELD) == Some(CITES_EDGE_KIND) {
                    *self.cite_count.entry(to).or_default() += 1;
                } else if row.string(KIND_FIELD) == Some(DISCHARGES_EDGE_KIND) {
                    *self.discharge_count.entry(to).or_default() += 1;
                }
            }
            CONFIG_RELATION => self.insert_config_tuple(row),
            CONTENT_RELATION => {
                let (Some(handle), Some(tokens)) =
                    (row.string(HANDLE_FIELD), row.i64(TOKENS_FIELD))
                else {
                    return;
                };
                let tokens = usize::try_from(tokens).unwrap_or(0);
                *self.content_tokens.entry(handle.to_owned()).or_default() += tokens;
            }
            SNAPSHOT_RELATION => {
                let (Some(id), Some(key), Some(status), Some(at)) = (
                    row.string(ID_FIELD),
                    row.string(KEY_FIELD),
                    row.string(VALUE_FIELD),
                    row.string(AT_FIELD),
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
                if let Some(namespace) = row.string(NAMESPACE_FIELD) {
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

    #[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
    fn scoped_to_snapshot(&self, selection: &SnapshotSelection) -> Self {
        let mut graph = self.clone();
        graph.evaluation_day = Some(selection.day);
        graph.apply_snapshot_rows(&selection.rows);
        graph
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn scoped_to_snapshot_tuples(&self, tuples: &TupleDb, selection: &SnapshotSelection) -> Self {
        let mut graph = self.clone();
        graph.evaluation_day = Some(selection.day);
        graph.apply_snapshot_tuple_rows(tuples, &selection.tuple_rows);
        graph
    }

    #[cfg(any(not(feature = "physical-substrate"), feature = "legacy-time-clone"))]
    fn apply_snapshot_rows(&mut self, snapshot_rows: &[NamedRow]) {
        for ((_corpus, id), values) in handle_snapshot_patches(snapshot_rows) {
            let Some(state) = self.handles.get_mut(&id) else {
                continue;
            };
            for (key, value) in values {
                match key.as_str() {
                    KIND_FIELD => state.kind = value,
                    STATUS_FIELD => state.status = Some(value),
                    NAMESPACE_FIELD => state.namespace = value,
                    DATE_FIELD => state.date = iso_days_since_epoch(&value),
                    _ => {}
                }
            }
        }
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    fn apply_snapshot_tuple_rows(&mut self, tuples: &TupleDb, snapshot_rows: &[RowId]) {
        for row in snapshot_rows {
            let Some(row) = tuples.tuple_row(SNAPSHOT_RELATION, *row) else {
                continue;
            };
            let (Some(id), Some(key), Some(value)) = (
                row.string(ID_FIELD),
                row.string(KEY_FIELD),
                row.string(VALUE_FIELD),
            ) else {
                continue;
            };
            let Some(state) = self.handles.get_mut(id) else {
                continue;
            };
            match key {
                KIND_FIELD => state.kind = value.to_owned(),
                STATUS_FIELD => state.status = Some(value.to_owned()),
                NAMESPACE_FIELD => state.namespace = value.to_owned(),
                DATE_FIELD => state.date = iso_days_since_epoch(value),
                _ => {}
            }
        }
    }

    fn insert_config(&mut self, row: &NamedRow) {
        let (Some(key), Some(value)) = (row_string(row, KEY_FIELD), row_string(row, VALUE_FIELD))
        else {
            return;
        };
        let ordinal = row_i64(row, ORDINAL_FIELD);
        self.insert_config_values(key, value, ordinal);
    }

    #[cfg(feature = "physical-substrate")]
    fn insert_config_tuple(&mut self, row: TupleRow<'_>) {
        let (Some(key), Some(value)) = (row.string(KEY_FIELD), row.string(VALUE_FIELD)) else {
            return;
        };
        self.insert_config_values(key, value, row.i64(ORDINAL_FIELD));
    }

    fn insert_config_values(&mut self, key: &str, value: &str, ordinal: Option<i64>) {
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
                let position = ordinal.unwrap_or_else(|| {
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
            CONFIG_IMPACT_TRAVERSE => {
                self.impact_traverse.insert(value.to_owned());
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
            PrimitivePredicate::GitMtime => self.git_mtime_tuples(constraints),
            PrimitivePredicate::ChangedWithin => self.recent_tuples(constraints),
            PrimitivePredicate::TokenEstimate => {
                self.handle_count_tuples(constraints, &self.content_tokens)
            }
            PrimitivePredicate::Search
            | PrimitivePredicate::Read
            | PrimitivePredicate::ReadFull
            | PrimitivePredicate::Match
            | PrimitivePredicate::Schema
            | PrimitivePredicate::Predicates
            | PrimitivePredicate::Verbs
            | PrimitivePredicate::Describe
            | PrimitivePredicate::SourceOf
            | PrimitivePredicate::Examples
            | PrimitivePredicate::Sources => Vec::new(),
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
                .impact_reachable_from(start, Direction::Incoming, max_depth)
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
                .impact_reachable_from(end, Direction::Outgoing, max_depth)
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
                    self.impact_reachable_from(start, Direction::Incoming, max_depth)
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
        let today = self.evaluation_day.or_else(current_days_since_epoch);
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
        let today = self.evaluation_day.or_else(current_days_since_epoch);
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

    fn git_mtime_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let file = string_constraint(constraints, 0);
        let instant = string_constraint(constraints, 1);
        match (file, instant) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(file), _) => self
                .git_mtimes
                .get(file)
                .map(|mtime| Tuple(vec![string_value(file), string_value(mtime)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .git_mtimes
                .iter()
                .map(|(file, instant)| Tuple(vec![string_value(file), string_value(instant)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn recent_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = match i64_constraint(constraints, 1) {
            ArgConstraint::Exact(days) if days >= 0 => days,
            ArgConstraint::Any | ArgConstraint::Exact(_) | ArgConstraint::Impossible => {
                return Vec::new();
            }
        };
        let Some(today) = self.evaluation_day.or_else(current_days_since_epoch) else {
            return Vec::new();
        };
        let cutoff = today.saturating_sub(days);
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(handle) => self
                .recent_tuple_for(handle, days, cutoff)
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            ArgConstraint::Any => self
                .handles
                .keys()
                .filter_map(|handle| self.recent_tuple_for(handle, days, cutoff))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn recent_tuple_for(&self, handle: &str, days: i64, cutoff: i64) -> Option<Tuple> {
        let state = self.handles.get(handle)?;
        let instant = self.git_mtimes.get(&state.file)?;
        let mtime_day = snapshot_days_since_epoch(instant)?;
        (mtime_day >= cutoff).then(|| Tuple(vec![string_value(handle), int_value(days)]))
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

    fn impact_reachable_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        self.walk_impact_from(start, direction, max_depth)
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

    fn walk_impact_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([start.to_owned()]);
        let mut queue = VecDeque::from([(start.to_owned(), 0_i64)]);
        while let Some((node, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            self.visit_impact_neighbors(&node, direction, |next| {
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

    fn visit_impact_neighbors(
        &self,
        node: &str,
        direction: Direction,
        mut visit: impl FnMut(&String),
    ) {
        match direction {
            Direction::Outgoing => {
                self.visit_impact_edges(self.outgoing_edges.get(node), &mut visit);
            }
            Direction::Incoming => {
                self.visit_impact_edges(self.incoming_edges.get(node), &mut visit);
            }
            Direction::Undirected => {
                self.visit_impact_edges(self.incoming_edges.get(node), &mut visit);
                self.visit_impact_edges(self.outgoing_edges.get(node), &mut visit);
            }
        }
    }

    fn visit_impact_edges(
        &self,
        edges: Option<&BTreeSet<(String, String)>>,
        visit: &mut impl FnMut(&String),
    ) {
        let Some(edges) = edges else {
            return;
        };
        for (kind, next) in edges {
            if self.impact_traverses(kind) {
                visit(next);
            }
        }
    }

    fn impact_traverses(&self, kind: &str) -> bool {
        if self.impact_traverse.is_empty() {
            DEFAULT_IMPACT_TRAVERSE.contains(&kind)
        } else {
            self.impact_traverse.contains(kind)
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
const NATIVE_ID_FIELD: &str = "native_id";
const ORIGIN_URI_FIELD: &str = "origin_uri";
const REVISION_FIELD: &str = "revision";
const SNAPSHOT_FIELD: &str = "snapshot";
const ID_FIELD: &str = "id";
const FILE_FIELD: &str = "file";
const LINE_FIELD: &str = "line";
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
const SESSION_ID_FIELD: &str = "session_id";
const STEP_FIELD: &str = "step";
const ACTOR_FIELD: &str = "actor";
const VERB_FIELD: &str = "verb";
const GENERATION_FIELD: &str = "generation";
const TRAIL_VISIBILITY_FIELD: &str = "visibility";
const LABEL_KIND: &str = "label";
const CITES_EDGE_KIND: &str = "Cites";
const DISCHARGES_EDGE_KIND: &str = "Discharges";
const MAX_TRAIL_REFS_PER_ENTRY: usize = 256;
const MAX_TRAIL_GENERATIONS_PER_ENTRY: usize = 64;
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
    tuple.matches_constraints(constraints)
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
    match (relation.as_str(), field.as_str()) {
        (
            TRAIL_RELATION,
            SESSION_ID_FIELD
            | STEP_FIELD
            | ACTOR_FIELD
            | CORPUS_FIELD
            | VERB_FIELD
            | TRAIL_VISIBILITY_FIELD,
        )
        | (
            TRAIL_REF_RELATION,
            SESSION_ID_FIELD | STEP_FIELD | KIND_FIELD | CORPUS_FIELD | SOURCE_FIELD | HANDLE_FIELD
            | SPAN_ID_FIELD,
        )
        | (
            TRAIL_GENERATION_RELATION,
            SESSION_ID_FIELD | STEP_FIELD | CORPUS_FIELD | SOURCE_FIELD | GENERATION_FIELD,
        ) => true,
        (TRAIL_RELATION | TRAIL_REF_RELATION | TRAIL_GENERATION_RELATION, _)
        | ("content", "text")
        | ("span" | "handle", "summary")
        | ("meta" | "config" | "snapshot", "value") => false,
        _ => true,
    }
}

#[derive(Clone, Debug, Default)]
struct DerivedRelation {
    tuples: BTreeSet<Tuple>,
    derivations: BTreeMap<Tuple, DerivationRef>,
    indexes: Vec<BTreeMap<Value, Vec<Tuple>>>,
}

impl DerivedRelation {
    fn len(&self) -> usize {
        self.tuples.len()
    }

    fn tuples(&self) -> &BTreeSet<Tuple> {
        &self.tuples
    }

    fn insert_with_derivation(&mut self, tuple: &Tuple, derivation: Option<DerivationRef>) -> bool {
        if !self.tuples.insert(tuple.clone()) {
            return false;
        }
        if let Some(derivation) = derivation {
            self.derivations.insert(tuple.clone(), derivation);
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

    fn derivation(&self, tuple: &Tuple) -> Option<DerivationRef> {
        self.derivations.get(tuple).map(Arc::clone)
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
    #[error("policy denied action '{action}' for actor '{actor}'")]
    PolicyDenied { actor: String, action: Action },
    #[error("read_full({handle:?}) would return {tokens} tokens, exceeding the hard limit {limit}")]
    ReadFullBudgetExceeded {
        handle: String,
        tokens: i64,
        limit: i64,
    },
    #[error("content provider failed: {0}")]
    ReadProvider(ReadError),
    #[error("search provider failed: {0}")]
    SearchProvider(SearchError),
    #[error("invalid regex pattern {pattern:?}: {source}")]
    InvalidRegex {
        pattern: String,
        source: regex::Error,
    },
    #[error("unsupported expression")]
    UnsupportedExpression,
    #[error("division by zero")]
    DivisionByZero,
    #[error("reserved output field '{field}' cannot be bound when explain output is enabled")]
    ReservedExplainField { field: &'static str },
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<AuthorizationError> for EvalError {
    fn from(error: AuthorizationError) -> Self {
        match error {
            AuthorizationError::CapabilityRequired { action, capability } => {
                Self::CapabilityRequired {
                    primitive: action.as_str(),
                    capability,
                }
            }
            AuthorizationError::PolicyDenied { actor, action } => {
                Self::PolicyDenied { actor, action }
            }
        }
    }
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
        database.install_program_introspection(&program);
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
        self.options.authorize_eval()?;
        self.seed_facts()?;
        self.run_fixpoint_matching(|_| true)
    }

    pub fn run_fixpoint_for_query(&mut self, query: &AnalyzedQuery) -> Result<(), EvalError> {
        self.options.authorize_eval()?;
        self.seed_facts()?;
        let needed = global_predicate_dependencies_for_query(&self.program, query);
        self.run_fixpoint_matching(|predicate| needed.contains(predicate))
    }

    fn run_fixpoint_matching(
        &mut self,
        predicate_needed: impl Fn(&PredicateRef) -> bool,
    ) -> Result<(), EvalError> {
        let strata = self.program.strata().to_vec();
        for stratum in strata {
            if !stratum.predicates.iter().any(&predicate_needed) {
                continue;
            }
            let rules = self
                .program
                .rules()
                .filter(|rule| {
                    stratum.predicates.contains(&rule.head.predicate)
                        && predicate_needed(&rule.head.predicate)
                })
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
        self.options.authorize_eval()?;
        let query_ast = query.query();
        let mut warnings = self.warnings.clone();
        if query_ast.local_rules.is_empty() {
            if self.options.explain().is_enabled() {
                let mut bindings = eval_body_traced(
                    &query_ast.body,
                    vec![TracedBinding::empty()],
                    &self.database,
                    None,
                    &mut warnings,
                    &self.options,
                )?;
                ensure_no_reserved_explain_fields(&bindings)?;
                sort_traced_bindings_for_query(&query_ast.ordering, &mut bindings)?;
                return Ok(QueryOutput {
                    rows: traced_bindings_to_rows(bindings, self.options.explain()),
                    warnings,
                });
            }
            let mut bindings = eval_body(
                &query_ast.body,
                vec![Binding::new()],
                &self.database,
                &mut warnings,
                &self.options,
            )?;
            sort_bindings_for_query(&query_ast.ordering, &mut bindings)?;
            return Ok(QueryOutput {
                rows: bindings.into_iter().map(binding_to_row).collect(),
                warnings,
            });
        }

        let mut database = self.database.clone();
        database.ensure_derived(query.local_predicates().cloned());
        database.install_query_introspection(query);
        for stratum in query.local_strata() {
            let rules = query_ast
                .local_rules
                .iter()
                .filter(|rule| stratum.predicates.contains(&rule.head.predicate))
                .cloned()
                .collect::<Vec<_>>();
            run_rule_group(&mut database, &rules, &mut warnings, &self.options)?;
        }
        let rows = if self.options.explain().is_enabled() {
            let mut bindings = eval_body_traced(
                &query_ast.body,
                vec![TracedBinding::empty()],
                &database,
                None,
                &mut warnings,
                &self.options,
            )?;
            ensure_no_reserved_explain_fields(&bindings)?;
            sort_traced_bindings_for_query(&query_ast.ordering, &mut bindings)?;
            traced_bindings_to_rows(bindings, self.options.explain())
        } else {
            let mut bindings = eval_body(
                &query_ast.body,
                vec![Binding::new()],
                &database,
                &mut warnings,
                &self.options,
            )?;
            sort_bindings_for_query(&query_ast.ordering, &mut bindings)?;
            bindings.into_iter().map(binding_to_row).collect()
        };
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
            let derivation = self
                .options
                .explain()
                .is_enabled()
                .then(|| Arc::new(DerivationNode::fact(&fact.predicate, &tuple)));
            self.database
                .derived
                .entry(fact.predicate.clone())
                .or_default()
                .insert_with_derivation(&tuple, derivation);
        }
        self.facts_seeded = true;
        Ok(())
    }
}

fn global_predicate_dependencies_for_query(
    program: &AnalyzedProgram,
    query: &AnalyzedQuery,
) -> BTreeSet<PredicateRef> {
    let mut needed = BTreeSet::new();
    collect_body_global_predicates(&query.query().body, query, &mut needed);
    for rule in &query.query().local_rules {
        collect_body_global_predicates(&rule.body, query, &mut needed);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for rule in program.rules() {
            if !needed.contains(&rule.head.predicate) {
                continue;
            }
            let before = needed.len();
            collect_body_global_predicates(&rule.body, query, &mut needed);
            changed |= needed.len() != before;
        }
    }
    needed
}

fn collect_body_global_predicates(
    body: &Body,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    for atom in &body.atoms {
        collect_atom_global_predicates(atom, query, out);
    }
}

fn collect_atom_global_predicates(
    atom: &Atom,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    match atom {
        Atom::Derived(derived) => collect_global_predicate(&derived.predicate, query, out),
        Atom::Aggregation(aggregate) => {
            collect_body_global_predicates(&aggregate.body, query, out);
        }
        Atom::Negation(negation) => {
            if let NegatedAtom::Derived(derived) = &negation.atom {
                collect_global_predicate(&derived.predicate, query, out);
            }
        }
        Atom::TimeBlock(time_block) => {
            collect_body_global_predicates(&time_block.body, query, out);
        }
        Atom::Stored(_) | Atom::Comparison(_) => {}
    }
}

fn collect_global_predicate(
    predicate: &PredicateRef,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    if PrimitivePredicate::from_predicate(predicate).is_some() {
        return;
    }
    if query.local_predicates().any(|local| local == predicate) {
        return;
    }
    out.insert(predicate.clone());
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

#[derive(Clone, Debug)]
struct DerivedTuple {
    tuple: Tuple,
    derivation: Option<DerivationRef>,
}

#[derive(Clone, Debug)]
struct TracedBinding {
    values: Binding,
    steps: Vec<DerivationRef>,
}

impl TracedBinding {
    fn empty() -> Self {
        Self {
            values: Binding::new(),
            steps: Vec::new(),
        }
    }

    fn with_values(values: Binding) -> Self {
        Self {
            values,
            steps: Vec::new(),
        }
    }

    fn push_step(mut self, step: DerivationRef) -> Self {
        self.steps.push(step);
        self
    }

    fn push_step_if(self, trace: bool, step: impl FnOnce() -> DerivationRef) -> Self {
        if trace { self.push_step(step()) } else { self }
    }
}

fn clone_derivation_refs(steps: &[DerivationRef]) -> Vec<DerivationNode> {
    steps.iter().map(|step| step.as_ref().clone()).collect()
}

fn derivation_ref(node: DerivationNode) -> DerivationRef {
    Arc::new(node)
}

fn eval_rule(
    rule: &Rule,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<DerivedTuple>, EvalError> {
    if options.explain().is_enabled() {
        let bindings = eval_body_traced(
            &rule.body,
            vec![TracedBinding::empty()],
            database,
            None,
            warnings,
            options,
        )?;
        return bindings
            .into_iter()
            .map(|binding| {
                let tuple = project_head(&rule.head, &binding.values)?;
                let derivation =
                    DerivationNode::rule(rule, &tuple, clone_derivation_refs(&binding.steps))
                        .bounded(options.explain());
                Ok(DerivedTuple {
                    tuple,
                    derivation: Some(Arc::new(derivation)),
                })
            })
            .collect();
    }

    eval_body(
        &rule.body,
        vec![Binding::new()],
        database,
        warnings,
        options,
    )?
    .into_iter()
    .map(|binding| {
        Ok(DerivedTuple {
            tuple: project_head(&rule.head, &binding)?,
            derivation: None,
        })
    })
    .collect()
}

fn eval_rule_with_delta(
    rule: &Rule,
    database: &Database,
    delta: &DeltaMap,
    atom_index: usize,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<DerivedTuple>, EvalError> {
    if options.explain().is_enabled() {
        let bindings = eval_body_traced(
            &rule.body,
            vec![TracedBinding::empty()],
            database,
            Some(DeltaView { delta, atom_index }),
            warnings,
            options,
        )?;
        return bindings
            .into_iter()
            .map(|binding| {
                let tuple = project_head(&rule.head, &binding.values)?;
                let derivation =
                    DerivationNode::rule(rule, &tuple, clone_derivation_refs(&binding.steps))
                        .bounded(options.explain());
                Ok(DerivedTuple {
                    tuple,
                    derivation: Some(Arc::new(derivation)),
                })
            })
            .collect();
    }

    eval_body_with_delta(
        &rule.body,
        vec![Binding::new()],
        database,
        Some(DeltaView { delta, atom_index }),
        warnings,
        options,
    )?
    .into_iter()
    .map(|binding| {
        Ok(DerivedTuple {
            tuple: project_head(&rule.head, &binding)?,
            derivation: None,
        })
    })
    .collect()
}

fn insert_new_tuples(
    database: &mut Database,
    predicate: &PredicateRef,
    tuples: Vec<DerivedTuple>,
    delta: &mut DeltaMap,
) -> bool {
    let relation = database.derived.entry(predicate.clone()).or_default();
    let mut changed = false;
    for derived in tuples {
        if relation.insert_with_derivation(&derived.tuple, derived.derivation.clone()) {
            delta
                .entry(predicate.clone())
                .or_default()
                .insert_with_derivation(&derived.tuple, derived.derivation);
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
    bindings: Vec<Binding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<Binding>, EvalError> {
    let bindings = bindings
        .into_iter()
        .map(TracedBinding::with_values)
        .collect::<Vec<_>>();
    eval_body_traced(body, bindings, database, delta, warnings, options)
        .map(|bindings| bindings.into_iter().map(|binding| binding.values).collect())
}

fn eval_body_traced(
    body: &Body,
    mut bindings: Vec<TracedBinding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut remaining = body.atoms.iter().enumerate().collect::<Vec<_>>();
    while !remaining.is_empty() {
        if bindings.is_empty() {
            break;
        }
        let bound = common_bound_variables_traced(&bindings);
        let next_index = remaining
            .iter()
            .position(|(atom_index, atom)| atom_ready(body, *atom_index, atom, &bound))
            .unwrap_or(0);
        let (atom_index, atom) = remaining.remove(next_index);
        let atom_delta = delta.filter(|view| view.atom_index == atom_index);
        bindings = eval_atom_traced(atom, bindings, database, atom_delta, warnings, options)?;
    }
    Ok(bindings)
}

fn common_bound_variables_traced(bindings: &[TracedBinding]) -> BTreeSet<Ident> {
    let Some((first, rest)) = bindings.split_first() else {
        return BTreeSet::new();
    };
    let mut common = first.values.keys().cloned().collect::<BTreeSet<_>>();
    for binding in rest {
        common.retain(|var| binding.values.contains_key(var));
    }
    common
}

fn atom_ready(body: &Body, atom_index: usize, atom: &Atom, bound: &BTreeSet<Ident>) -> bool {
    match atom {
        Atom::Stored(stored) => variables_are_bound(&stored_atom_input_variables(stored), bound),
        Atom::TimeBlock(time_block) => variables_are_bound(
            &positive_body_outer_input_variables(&time_block.body),
            bound,
        ),
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
    if !variables_are_bound(&derived_atom_input_variables(atom), bound) {
        return false;
    }
    let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) else {
        return true;
    };
    let graph_ready = primitive.graph_anchor_positions().is_none_or(|positions| {
        positions.iter().any(|idx| {
            atom.args.get(*idx).is_some_and(|arg| {
                arg.expr()
                    .is_some_and(|expr| expr_variables_are_bound(expr, bound))
            })
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
        atom.args.get(input.position).is_some_and(|arg| {
            arg.expr()
                .is_some_and(|expr| expr_variables_are_bound(expr, bound))
        })
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
    required.extend(
        positive_body_input_variables(&aggregate.body)
            .into_iter()
            .filter(|var| !inner.contains(var)),
    );
    collect_aggregate_outer_input_variables(aggregate, &inner, &mut required);
    required.iter().all(|var| bound.contains(var))
}

fn collect_aggregate_outer_input_variables(
    aggregate: &Aggregate,
    inner_bound: &BTreeSet<Ident>,
    out: &mut BTreeSet<Ident>,
) {
    let rank_var = rank_arg_variable(aggregate);
    let mut value_vars = BTreeSet::new();
    aggregate.value.variables(&mut value_vars);
    if let Some(rank_var) = &rank_var {
        value_vars.remove(rank_var);
    }
    out.extend(
        value_vars
            .into_iter()
            .filter(|var| !inner_bound.contains(var)),
    );

    for arg in &aggregate.args {
        if aggregate.function == AggregateFunction::Rank && arg.name.as_str() == "rank" {
            continue;
        }
        let mut arg_vars = BTreeSet::new();
        if matches!(
            (aggregate.function, arg.name.as_str()),
            (AggregateFunction::TopK, "k") | (AggregateFunction::TakeUntil, "budget")
        ) {
            arg.expr.variables(&mut arg_vars);
        } else {
            arg.expr.variables(&mut arg_vars);
            arg_vars.retain(|var| !inner_bound.contains(var));
        }
        out.extend(arg_vars);
    }
}

fn rank_arg_variable(aggregate: &Aggregate) -> Option<Ident> {
    if aggregate.function != AggregateFunction::Rank {
        return None;
    }
    aggregate
        .args
        .iter()
        .find(|arg| arg.name.as_str() == "rank")
        .and_then(|arg| match &arg.expr {
            Expr::Var(var) => Some(var.clone()),
            _ => None,
        })
}

fn negated_atom_variables_are_bound(atom: &NegatedAtom, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    collect_negated_atom_variables(atom, &mut vars);
    vars.iter().all(|var| bound.contains(var))
}

fn expr_variables_are_bound(expr: &Expr, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    variables_are_bound(&vars, bound)
}

fn variables_are_bound(vars: &BTreeSet<Ident>, bound: &BTreeSet<Ident>) -> bool {
    vars.iter().all(|var| bound.contains(var))
}

fn collect_non_aggregate_positive_atom_variables(atom: &Atom, out: &mut BTreeSet<Ident>) {
    match atom {
        Atom::Stored(stored) => collect_stored_atom_binding_variables(stored, out),
        Atom::Derived(derived) => collect_derived_atom_binding_variables(derived, out),
        Atom::TimeBlock(time_block) => collect_positive_body_variables(&time_block.body, out),
        Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
    }
}

fn collect_positive_body_variables(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        match atom {
            Atom::Aggregation(aggregate) => aggregate.result.binding_variables(out),
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
        if let Some(expr) = arg.expr() {
            expr.variables(out);
        }
    }
}

fn collect_stored_atom_binding_variables(atom: &StoredAtom, out: &mut BTreeSet<Ident>) {
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.binding_variables(out);
        }
    }
}

fn collect_derived_atom_binding_variables(
    atom: &crate::runtime::ast::DerivedAtom,
    out: &mut BTreeSet<Ident>,
) {
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.binding_variables(out);
        }
    }
}

fn stored_atom_input_variables(atom: &StoredAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.input_variables(&mut vars);
        }
    }
    vars
}

fn derived_atom_input_variables(atom: &crate::runtime::ast::DerivedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.input_variables(&mut vars);
        }
    }
    vars
}

fn positive_body_input_variables(body: &Body) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => vars.extend(stored_atom_input_variables(stored)),
            Atom::Derived(derived) => vars.extend(derived_atom_input_variables(derived)),
            Atom::TimeBlock(time_block) => {
                vars.extend(positive_body_input_variables(&time_block.body));
            }
            Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
        }
    }
    vars
}

fn positive_body_outer_input_variables(body: &Body) -> BTreeSet<Ident> {
    let mut inputs = positive_body_input_variables(body);
    let mut binders = BTreeSet::new();
    collect_positive_body_variables(body, &mut binders);
    inputs.retain(|var| !binders.contains(var));
    inputs
}

fn eval_atom_traced(
    atom: &Atom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    match atom {
        Atom::Stored(stored) => eval_stored_traced(stored, bindings, database, options),
        Atom::Derived(derived) => {
            if let Some(view) = delta {
                eval_derived_from_delta_traced(
                    derived,
                    bindings,
                    view.delta,
                    options.explain().is_enabled(),
                )
            } else {
                eval_derived_traced(derived, bindings, database, options)
            }
        }
        Atom::Comparison(comparison) => {
            eval_comparison_traced(comparison, bindings, options.explain().is_enabled())
        }
        Atom::Aggregation(aggregate) => {
            eval_aggregate_traced(aggregate, bindings, database, warnings, options)
        }
        Atom::Negation(negation) => {
            eval_negation_traced(&negation.atom, bindings, database, warnings, options)
        }
        Atom::TimeBlock(time_block) => {
            let trace = options.explain().is_enabled();
            ensure_snapshot_time_body_supported(&time_block.reference, &time_block.body, database)?;
            let (scoped, scoped_warnings) = database.scoped_to_time_ref(&time_block.reference)?;
            push_warnings(warnings, scoped_warnings);
            let mut out = Vec::new();
            for binding in bindings {
                let TracedBinding { values, steps } = binding;
                let children = eval_body_traced(
                    &time_block.body,
                    vec![TracedBinding::with_values(values)],
                    &scoped,
                    None,
                    warnings,
                    options,
                )?;
                out.extend(children.into_iter().map(|child| {
                    if trace {
                        TracedBinding {
                            values: child.values,
                            steps: steps
                                .clone()
                                .into_iter()
                                .chain([derivation_ref(DerivationNode::time_block(
                                    &time_block.reference,
                                    time_block.location.clone(),
                                    clone_derivation_refs(&child.steps),
                                ))])
                                .collect(),
                        }
                    } else {
                        TracedBinding {
                            values: child.values,
                            steps: steps.clone(),
                        }
                    }
                }));
            }
            Ok(out)
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

fn eval_stored_traced(
    atom: &StoredAtom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    if let Some(relation) = database.stored.get(&atom.relation) {
        return eval_stored_relation_traced(atom, bindings, relation, options);
    }

    #[cfg(feature = "physical-substrate")]
    {
        if database.tuples.has_relation(atom.relation.as_str()) {
            return eval_tuple_stored_traced(atom, bindings, database, options);
        }
    }

    Err(EvalError::UnknownStoredRelation {
        relation: atom.relation.clone(),
    })
}

fn eval_stored_relation_traced(
    atom: &StoredAtom,
    bindings: Vec<TracedBinding>,
    relation: &StoredRelation,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    let trace = options.explain().is_enabled();
    for binding in bindings {
        let constraints = stored_constraints(&atom.fields, &binding.values)?;
        for row in relation.candidate_rows(&constraints) {
            if !stored_row_visible(&atom.relation, row, options) {
                continue;
            }
            if let Some(next) = unify_stored_fields(&atom.fields, row, &binding.values)? {
                let binding = TracedBinding {
                    values: next,
                    steps: binding.steps.clone(),
                }
                .push_step_if(trace, || {
                    derivation_ref(DerivationNode::stored(&atom.relation, row))
                });
                out.push(binding);
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "physical-substrate")]
fn eval_tuple_stored_traced(
    atom: &StoredAtom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    let trace = options.explain().is_enabled();
    for binding in bindings {
        let constraints = stored_constraints(&atom.fields, &binding.values)?;
        for row in database.candidate_tuple_rows(&atom.relation, &constraints) {
            if let Some(next) = unify_tuple_stored_fields(atom, database, row, &binding.values)? {
                let binding = TracedBinding {
                    values: next,
                    steps: binding.steps.clone(),
                }
                .push_step_if(trace, || {
                    let row = database
                        .tuple_named_row(&atom.relation, row)
                        .unwrap_or_default();
                    derivation_ref(DerivationNode::stored(&atom.relation, &row))
                });
                out.push(binding);
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "physical-substrate")]
fn unify_tuple_stored_fields(
    atom: &StoredAtom,
    database: &Database,
    row: RowId,
    binding: &Binding,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    for field in &atom.fields {
        let Some(value) = database.tuple_field_value(&atom.relation, row, &field.field) else {
            return Ok(None);
        };
        if !unify_term(&field.term, &value, binding, &mut next)? {
            return Ok(None);
        }
    }
    Ok(Some(next.unwrap_or_else(|| binding.clone())))
}

fn stored_row_visible(relation: &Ident, row: &NamedRow, options: &EvalOptions) -> bool {
    if !matches!(
        relation.as_str(),
        TRAIL_RELATION | TRAIL_REF_RELATION | TRAIL_GENERATION_RELATION
    ) {
        return true;
    }
    match row_string(row, TRAIL_VISIBILITY_FIELD) {
        Some("private") => {
            options
                .actor()
                .can_see_fact_visibility(FactVisibility::Private)
                && options.has_capability(RuntimeCapability::TrailPrivate)
                && options.authorize(Action::TrailPrivateRead).is_ok()
        }
        Some("team") => options
            .actor()
            .can_see_fact_visibility(FactVisibility::Team),
        Some("public") | None => true,
        Some(_) => false,
    }
}

fn eval_derived_traced(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    if let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) {
        if primitive.is_soft() && database.derived.contains_key(&atom.predicate) {
            let relation = database.derived.get(&atom.predicate).ok_or_else(|| {
                EvalError::UnknownDerivedPredicate {
                    predicate: atom.predicate.clone(),
                }
            })?;
            return eval_derived_from_relation_traced(
                atom,
                bindings,
                relation,
                options.explain().is_enabled(),
            );
        }
        return eval_primitive_traced(primitive, atom, bindings, database, options);
    }
    let relation = database.derived.get(&atom.predicate).ok_or_else(|| {
        EvalError::UnknownDerivedPredicate {
            predicate: atom.predicate.clone(),
        }
    })?;
    eval_derived_from_relation_traced(atom, bindings, relation, options.explain().is_enabled())
}

fn eval_derived_from_delta_traced(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<TracedBinding>,
    delta: &DeltaMap,
    trace: bool,
) -> Result<Vec<TracedBinding>, EvalError> {
    let Some(relation) = delta.get(&atom.predicate) else {
        return Ok(Vec::new());
    };
    eval_derived_from_relation_traced(atom, bindings, relation, trace)
}

fn eval_derived_from_relation_traced(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<TracedBinding>,
    relation: &DerivedRelation,
    trace: bool,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let constraints = call_constraints(&atom.args, &binding.values)?;
        for tuple in relation.candidate_tuples(&constraints) {
            if tuple.0.len() != atom.args.len() {
                continue;
            }
            if let Some(next) = unify_call_args(&atom.args, tuple, &binding.values)? {
                let binding = TracedBinding {
                    values: next,
                    steps: binding.steps.clone(),
                }
                .push_step_if(trace, || {
                    relation.derivation(tuple).unwrap_or_else(|| {
                        derivation_ref(DerivationNode::fact(&atom.predicate, tuple))
                    })
                });
                out.push(binding);
            }
        }
    }
    Ok(out)
}

fn eval_primitive_traced(
    primitive: PrimitivePredicate,
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    let mut regex_cache = BTreeMap::<String, Regex>::new();
    let trace = options.explain().is_enabled();
    for binding in bindings {
        let constraints = call_constraints(&atom.args, &binding.values)?;
        let tuples =
            primitive_tuples(primitive, &constraints, database, options, &mut regex_cache)?;
        for tuple in tuples {
            if let Some(next) = unify_call_args(&atom.args, &tuple, &binding.values)? {
                let binding = TracedBinding {
                    values: next,
                    steps: binding.steps.clone(),
                }
                .push_step_if(trace, || {
                    derivation_ref(DerivationNode::primitive(&atom.predicate, &tuple))
                });
                out.push(binding);
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
        PrimitivePredicate::Search => database.search_tuples(constraints, options),
        PrimitivePredicate::Read => database.read_tuples(constraints, options),
        PrimitivePredicate::ReadFull => {
            if !options.has_capability(READ_FULL_CAPABILITY) {
                return Err(EvalError::CapabilityRequired {
                    primitive: "read_full",
                    capability: READ_FULL_CAPABILITY,
                });
            }
            database.read_full_tuples(constraints, options)
        }
        PrimitivePredicate::Match => {
            let ArgConstraint::Exact(pattern) = string_constraint(constraints, 0) else {
                return Ok(Vec::new());
            };
            let handle = match string_constraint(constraints, 1) {
                ArgConstraint::Any => None,
                ArgConstraint::Impossible => return Ok(Vec::new()),
                ArgConstraint::Exact(handle) => Some(handle.to_owned()),
            };
            options.authorize(Action::Match {
                pattern: pattern.to_owned(),
                handle,
            })?;
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
            #[cfg(feature = "physical-substrate")]
            {
                Ok(database.match_tuples_from_tuples(constraints, regex))
            }
            #[cfg(not(feature = "physical-substrate"))]
            {
                Ok(database.content.match_tuples(constraints, regex))
            }
        }
        PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => Ok(database.introspection.tuples(primitive, constraints)),
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
        | PrimitivePredicate::GitMtime
        | PrimitivePredicate::ChangedWithin
        | PrimitivePredicate::TokenEstimate => Ok(database.graph.tuples(primitive, constraints)),
    }
}

fn eval_comparison_traced(
    comparison: &Comparison,
    bindings: Vec<TracedBinding>,
    trace: bool,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let left = eval_expr(&comparison.left, &binding.values)?;
        let right = eval_expr(&comparison.right, &binding.values)?;
        if compare(&left, comparison.op, &right)? {
            out.push(binding.push_step_if(trace, || {
                derivation_ref(DerivationNode::comparison(comparison))
            }));
        }
    }
    Ok(out)
}

fn eval_negation_traced(
    negated: &NegatedAtom,
    bindings: Vec<TracedBinding>,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    let mut out = Vec::new();
    let trace = options.explain().is_enabled();
    for binding in bindings {
        let atom = match negated {
            NegatedAtom::Stored(stored) => Atom::Stored(stored.clone()),
            NegatedAtom::Derived(derived) => Atom::Derived(derived.clone()),
        };
        let matches = eval_atom_traced(
            &atom,
            vec![TracedBinding::with_values(binding.values.clone())],
            database,
            None,
            warnings,
            options,
        )?;
        if matches.is_empty() {
            out.push(
                binding.push_step_if(trace, || derivation_ref(DerivationNode::negation(negated))),
            );
        }
    }
    Ok(out)
}

fn eval_aggregate_traced(
    aggregate: &Aggregate,
    bindings: Vec<TracedBinding>,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<TracedBinding>, EvalError> {
    validate_aggregate_args(aggregate)?;

    let mut out = Vec::new();
    let trace = options.explain().is_enabled();
    for binding in bindings {
        let inner = eval_body_traced(
            &aggregate.body,
            vec![TracedBinding::with_values(binding.values.clone())],
            database,
            None,
            warnings,
            options,
        )?;
        if inner.is_empty() {
            if aggregate.function == AggregateFunction::Count
                && let Some(group) = bind_aggregate_result(
                    &aggregate.result,
                    &binding.values,
                    &Value::Number(NumberValue::Int(0)),
                )?
            {
                out.push(
                    TracedBinding {
                        values: group,
                        steps: binding.steps.clone(),
                    }
                    .push_step_if(trace, || {
                        derivation_ref(DerivationNode::aggregate(aggregate, Vec::new()))
                    }),
                );
            }
            continue;
        }
        let rows = inner
            .iter()
            .map(|row| row.values.clone())
            .collect::<Vec<_>>();
        let aggregate_steps = trace.then(|| aggregate_derivation_steps(&inner));
        for values in eval_aggregate_group(aggregate, &binding.values, &rows)? {
            out.push(
                TracedBinding {
                    values,
                    steps: binding.steps.clone(),
                }
                .push_step_if(trace, || {
                    derivation_ref(DerivationNode::aggregate(
                        aggregate,
                        clone_derivation_refs(aggregate_steps.as_deref().unwrap_or_default()),
                    ))
                }),
            );
        }
    }
    Ok(out)
}

fn aggregate_derivation_steps(rows: &[TracedBinding]) -> Vec<DerivationRef> {
    let mut out = Vec::new();
    let mut omitted = 0_usize;
    for step in rows.iter().flat_map(|row| row.steps.iter()) {
        if out.len() < MAX_AGGREGATE_DERIVATION_CHILDREN {
            out.push(Arc::clone(step));
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        out.push(derivation_ref(DerivationNode::evidence_truncated(omitted)));
    }
    out
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
            let values = aggregate_values(aggregate, rows)?;
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

fn aggregate_values(aggregate: &Aggregate, rows: &[Binding]) -> Result<Vec<Value>, EvalError> {
    rows.iter()
        .map(|row| eval_expr(&aggregate.value, row))
        .collect()
}

fn scalar_aggregate_value(
    function: AggregateFunction,
    values: &[Value],
) -> Result<Option<Value>, EvalError> {
    if values.is_empty() && function != AggregateFunction::Count {
        return Ok(None);
    }
    match function {
        AggregateFunction::Count => Ok(Some(Value::Number(NumberValue::Int(
            i64::try_from(distinct_aggregate_values(values).len()).unwrap_or(i64::MAX),
        )))),
        AggregateFunction::Sum => numeric_sum(values).map(Some),
        AggregateFunction::Min => Ok(values.iter().min().cloned()),
        AggregateFunction::Max => Ok(values.iter().max().cloned()),
        AggregateFunction::Avg => numeric_avg(values).map(Some),
        AggregateFunction::List | AggregateFunction::Set => Ok(Some(Value::List(
            distinct_aggregate_values(values).into_iter().collect(),
        ))),
        AggregateFunction::TopK | AggregateFunction::Rank | AggregateFunction::TakeUntil => {
            Err(EvalError::UnsupportedAggregate { function })
        }
    }
}

fn distinct_aggregate_values(values: &[Value]) -> BTreeSet<Value> {
    values.iter().cloned().collect()
}

fn numeric_sum(values: &[Value]) -> Result<Value, EvalError> {
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

fn numeric_avg(values: &[Value]) -> Result<Value, EvalError> {
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
        let Some(expr) = arg.expr() else {
            continue;
        };
        if let Some(value) = bound_value_for_expr(expr, binding)? {
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
        let Some(expr) = arg.expr() else {
            continue;
        };
        if !unify_expr(expr, value, binding, &mut next)? {
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

pub(crate) fn value_from_literal(literal: &Literal) -> Value {
    match literal {
        Literal::String(value) => Value::String(value.clone()),
        Literal::Number(NumberLiteral::Int(value)) => Value::Number(NumberValue::Int(*value)),
        Literal::Number(NumberLiteral::Float(value)) => Value::Number(NumberValue::Float(*value)),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Null => Value::Null,
        Literal::List(items) => Value::List(items.iter().map(value_from_literal).collect()),
    }
}

fn sort_bindings_for_query(
    ordering: &[OrderKey],
    bindings: &mut Vec<Binding>,
) -> Result<(), EvalError> {
    sort_query_items(ordering, bindings, |binding| binding)
}

fn sort_traced_bindings_for_query(
    ordering: &[OrderKey],
    bindings: &mut Vec<TracedBinding>,
) -> Result<(), EvalError> {
    sort_query_items(ordering, bindings, |binding| &binding.values)
}

fn sort_query_items<T>(
    ordering: &[OrderKey],
    items: &mut Vec<T>,
    binding: impl Fn(&T) -> &Binding,
) -> Result<(), EvalError> {
    if ordering.is_empty() {
        return Ok(());
    }
    let mut keyed = std::mem::take(items)
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let keys = eval_order_keys(ordering, binding(&item))?;
            Ok((index, keys, item))
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    keyed.sort_by(|left, right| compare_ordered_query_rows(ordering, left, right));
    items.extend(keyed.into_iter().map(|(_, _, item)| item));
    Ok(())
}

enum QueryOrderKeys {
    One(Value),
    Many(Vec<Value>),
}

fn eval_order_keys(ordering: &[OrderKey], binding: &Binding) -> Result<QueryOrderKeys, EvalError> {
    if let [key] = ordering {
        return eval_expr(&key.expr, binding).map(QueryOrderKeys::One);
    }
    ordering
        .iter()
        .map(|key| eval_expr(&key.expr, binding))
        .collect::<Result<Vec<_>, _>>()
        .map(QueryOrderKeys::Many)
}

fn compare_ordered_query_rows<T>(
    ordering: &[OrderKey],
    left: &(usize, QueryOrderKeys, T),
    right: &(usize, QueryOrderKeys, T),
) -> Ordering {
    for (index, key) in ordering.iter().enumerate() {
        let (left_key, right_key) = order_key_values(index, &left.1, &right.1);
        let comparison = match key.direction {
            OrderDirection::Asc => left_key.cmp(right_key),
            OrderDirection::Desc => right_key.cmp(left_key),
        };
        if comparison != Ordering::Equal {
            return comparison;
        }
    }
    left.0.cmp(&right.0)
}

fn order_key_values<'a>(
    index: usize,
    left: &'a QueryOrderKeys,
    right: &'a QueryOrderKeys,
) -> (&'a Value, &'a Value) {
    match (left, right) {
        (QueryOrderKeys::One(left), QueryOrderKeys::One(right)) => {
            debug_assert_eq!(index, 0);
            (left, right)
        }
        (QueryOrderKeys::Many(left), QueryOrderKeys::Many(right)) => (&left[index], &right[index]),
        (QueryOrderKeys::One(_), QueryOrderKeys::Many(_))
        | (QueryOrderKeys::Many(_), QueryOrderKeys::One(_)) => {
            unreachable!("order key arity is fixed for a query")
        }
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
        derivation: None,
    }
}

fn traced_bindings_to_rows(bindings: Vec<TracedBinding>, options: &ExplainOptions) -> Vec<Row> {
    bindings
        .into_iter()
        .enumerate()
        .map(|(index, binding)| {
            traced_binding_to_row(binding, options, options.explains_row(index))
        })
        .collect()
}

fn traced_binding_to_row(
    binding: TracedBinding,
    options: &ExplainOptions,
    include_derivation: bool,
) -> Row {
    debug_assert!(
        !binding
            .values
            .contains_key(&Ident::new_unchecked("_derivation")),
        "explain output reserves _derivation"
    );
    Row {
        fields: binding
            .values
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
        derivation: include_derivation
            .then(|| DerivationNode::query(clone_derivation_refs(&binding.steps)).bounded(options)),
    }
}

fn ensure_no_reserved_explain_fields(bindings: &[TracedBinding]) -> Result<(), EvalError> {
    let reserved = Ident::new_unchecked("_derivation");
    if bindings
        .iter()
        .any(|binding| binding.values.contains_key(&reserved))
    {
        return Err(EvalError::ReservedExplainField {
            field: "_derivation",
        });
    }
    Ok(())
}

fn compact_stored_row(relation: &Ident, row: &NamedRow) -> BTreeMap<String, Value> {
    row.iter()
        .filter(|(field, _)| explain_field_visible(relation.as_str(), field.as_str()))
        .map(|(field, value)| (field.to_string(), value.clone()))
        .collect()
}

fn explain_field_visible(relation: &str, field: &str) -> bool {
    let identity = matches!(
        field,
        CORPUS_FIELD
            | SOURCE_FIELD
            | NATIVE_ID_FIELD
            | ORIGIN_URI_FIELD
            | REVISION_FIELD
            | GENERATION_FIELD
    );
    identity
        || match relation {
            HANDLE_RELATION => matches!(
                field,
                ID_FIELD
                    | KIND_FIELD
                    | STATUS_FIELD
                    | NAMESPACE_FIELD
                    | FILE_FIELD
                    | LINE_FIELD
                    | DATE_FIELD
                    | AREA_FIELD
            ),
            EDGE_RELATION => matches!(field, FROM_FIELD | TO_FIELD | KIND_FIELD),
            META_RELATION | CONFIG_RELATION | SNAPSHOT_RELATION => matches!(
                field,
                HANDLE_FIELD
                    | ID_FIELD
                    | KEY_FIELD
                    | VALUE_FIELD
                    | ORDINAL_FIELD
                    | AT_FIELD
                    | SNAPSHOT_FIELD
            ),
            CONTENT_RELATION | SPAN_RELATION => matches!(
                field,
                HANDLE_FIELD
                    | SPAN_ID_FIELD
                    | ID_FIELD
                    | START_LINE_FIELD
                    | END_LINE_FIELD
                    | TOKENS_FIELD
            ),
            TRAIL_RELATION => matches!(
                field,
                SESSION_ID_FIELD
                    | STEP_FIELD
                    | ACTOR_FIELD
                    | CORPUS_FIELD
                    | VERB_FIELD
                    | TRAIL_VISIBILITY_FIELD
            ),
            TRAIL_REF_RELATION => matches!(
                field,
                SESSION_ID_FIELD
                    | STEP_FIELD
                    | KIND_FIELD
                    | ORDINAL_FIELD
                    | CORPUS_FIELD
                    | SOURCE_FIELD
                    | HANDLE_FIELD
                    | SPAN_ID_FIELD
                    | "score"
            ),
            TRAIL_GENERATION_RELATION => matches!(
                field,
                SESSION_ID_FIELD | STEP_FIELD | CORPUS_FIELD | SOURCE_FIELD | GENERATION_FIELD
            ),
            _ => false,
        }
}

fn named_row(entries: impl IntoIterator<Item = (&'static str, Value)>) -> NamedRow {
    entries
        .into_iter()
        .map(|(key, value)| (Ident::new_unchecked(key), value))
        .collect()
}

#[cfg(feature = "physical-substrate")]
fn string_map_to_named_row(row: BTreeMap<String, Value>) -> NamedRow {
    row.into_iter()
        .map(|(key, value)| (Ident::new_unchecked(key), value))
        .collect()
}

#[cfg(any(not(feature = "physical-substrate"), test))]
fn source_fact_row(
    identity: &FactIdentity,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) -> NamedRow {
    let mut row = identity_row(identity);
    row.extend(named_row(entries));
    row
}

#[cfg(any(not(feature = "physical-substrate"), test))]
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

fn u64_value(value: u64) -> Value {
    int_value(i64::try_from(value).unwrap_or(i64::MAX))
}

fn usize_value(value: usize) -> Value {
    int_value(i64::try_from(value).unwrap_or(i64::MAX))
}

fn score_value(score: Option<f32>) -> Value {
    score.map_or(Value::Null, |score| {
        if score.is_finite() && (0.0..=1.0).contains(&score) {
            float_value(f64::from(score))
        } else {
            Value::Null
        }
    })
}

fn trail_row(entry: &TrailEntryRedacted) -> NamedRow {
    named_row([
        ("session_id", Value::String(entry.session_id.to_string())),
        ("step", u64_value(entry.step)),
        ("timestamp", Value::String(entry.timestamp.clone())),
        ("actor", Value::String(entry.actor.clone())),
        ("corpus", Value::String(entry.corpus.to_string())),
        ("verb", Value::String(entry.verb.clone())),
        ("redacted_expr", Value::String(entry.redacted_expr.clone())),
        ("input_hash", Value::String(entry.input_hash.clone())),
        ("prelude_hash", Value::String(entry.prelude_hash.clone())),
        (
            "visibility",
            Value::String(entry.visibility.as_str().to_string()),
        ),
        ("retention", opt_string(entry.retention.as_ref())),
    ])
}

fn trail_ref_row(
    entry: &TrailEntryRedacted,
    kind: TrailRefKind,
    ordinal: usize,
    reference: &TrailReference,
) -> NamedRow {
    named_row([
        ("session_id", Value::String(entry.session_id.to_string())),
        ("step", u64_value(entry.step)),
        ("kind", Value::String(kind.as_str().to_string())),
        ("ordinal", usize_value(ordinal)),
        ("corpus", Value::String(reference.corpus.to_string())),
        ("source", Value::String(reference.source.to_string())),
        ("handle", Value::String(reference.handle.clone())),
        ("span_id", opt_string(reference.span_id.as_ref())),
        ("score", score_value(reference.score)),
    ])
}

fn trail_generation_row(entry: &TrailEntryRedacted, generation: &TrailGeneration) -> NamedRow {
    named_row([
        ("session_id", Value::String(entry.session_id.to_string())),
        ("step", u64_value(entry.step)),
        ("corpus", Value::String(generation.corpus.to_string())),
        ("source", Value::String(generation.source.to_string())),
        ("generation", generation_value(generation.generation)),
    ])
}

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
fn concern_row(fact: &ConcernFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("name", Value::String(fact.name.clone())),
            ("member", Value::String(fact.member.clone())),
        ],
    )
}

fn hidden_handles<F>(store: &FactStore, fact_visible: &F) -> BTreeSet<String>
where
    F: Fn(&FactIdentity) -> bool,
{
    store
        .handles()
        .iter()
        .filter(|fact| !fact_visible(&fact.identity))
        .map(|fact| fact.id.clone())
        .collect()
}

fn hidden_content_spans<F>(
    store: &FactStore,
    fact_visible: &F,
) -> BTreeMap<String, BTreeSet<String>>
where
    F: Fn(&FactIdentity) -> bool,
{
    let mut hidden = BTreeMap::<String, BTreeSet<String>>::new();
    for (handle, span_id) in store
        .content()
        .iter()
        .filter(|fact| !fact_visible(&fact.identity))
        .map(|fact| (&fact.handle, &fact.span_id))
        .chain(
            store
                .spans()
                .iter()
                .filter(|fact| !fact_visible(&fact.identity))
                .map(|fact| (&fact.handle, &fact.id)),
        )
    {
        hidden
            .entry(handle.clone())
            .or_default()
            .insert(span_id.clone());
    }
    hidden
}

fn hidden_content_span_count(spans_by_handle: &BTreeMap<String, BTreeSet<String>>) -> usize {
    spans_by_handle.values().map(BTreeSet::len).sum()
}

#[cfg(any(not(feature = "physical-substrate"), test))]
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

#[cfg(any(not(feature = "physical-substrate"), test))]
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

    use camino::Utf8PathBuf;
    use tempfile::{TempDir, tempdir};

    use crate::facts::{FactBatch, FactBatchMode, FactIdentity, SnapshotFact};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::ranking::{SearchHit, default_lexical_search_info};
    use crate::runtime::{StaticError, analyze, parse_prelude_program, parse_program};
    use crate::source::{Pattern, SourceCapabilities};
    use crate::trail::{
        DefaultTrailRedactor, DefaultTrailSummarizer, JsonlTrailStore, TrailEntryInProgress,
        TrailGeneration, TrailQuery, TrailRedactor, TrailReference, TrailSessionId, TrailStore,
        summarize_trail_session,
    };
    use crate::visibility::FactVisibility;
    #[cfg(feature = "physical-substrate")]
    use crate::{facts::STORED_RELATION_DESCRIPTORS, vm::store::TupleDb};

    fn identity(native_id: &str) -> FactIdentity {
        identity_for_source("fixture", native_id)
    }

    fn identity_for_source(source: &str, native_id: &str) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from(source),
            NativeId::from(native_id),
            OriginUri::from(format!("{source}://{native_id}")),
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

    fn oversized_content_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![handle("long.md", "file", "current", "", "core")];
        batch.content = vec![content_with_text(
            "long.md",
            "full",
            "abcdefghijklmnop oversized span",
            20,
        )];
        batch.spans = vec![span("long.md", "full", 1, 99)];
        let mut store = FactStore::default();
        store.merge(batch).expect("oversized content fixture merge");
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

    fn visibility_store() -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("public.md", "file", "current", "", "security"),
            handle("team.md", "file", "current", "", "security"),
            handle("secret.md", "file", "current", "", "security"),
        ];
        batch.content = vec![
            content_with_text("public.md", "body", "public roadmap", 4),
            content_with_text("secret.md", "body", "secret roadmap", 4),
        ];
        batch.spans = vec![
            span("public.md", "body", 1, 1),
            span("secret.md", "body", 1, 1),
        ];
        batch.meta = vec![meta("secret.md", "leaks-diagnostic", "true")];
        batch.set_visibility(NativeId::from("team.md"), FactVisibility::Team);
        batch.set_visibility(NativeId::from("secret.md"), FactVisibility::Private);
        let mut store = FactStore::default();
        store.merge(batch).expect("visibility fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-15", "public.md", "status", "current"),
                    snapshot_fact("s1", "2026-05-15", "secret.md", "status", "current"),
                ],
            )
            .expect("visibility snapshot fixture");
        store
    }

    #[cfg(feature = "physical-substrate")]
    #[test]
    fn tuple_db_lowering_matches_named_database_rows() {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle_with_summary(
                "b.md",
                "file",
                "draft",
                "",
                "core",
                "second handle sorts after a.md",
            ),
            handle_with_summary(
                "a.md",
                "file",
                "stable",
                "",
                "core",
                "first handle after canonicalization",
            ),
        ];
        batch.edges = vec![edge("a.md", "b.md", "DependsOn")];
        batch.meta = vec![meta("a.md", "external_class", "code")];
        batch.content = vec![content_with_text("a.md", "body", "body text", 2)];
        batch.spans = vec![span("a.md", "body", 3, 4)];
        batch.concerns = vec![ConcernFact {
            identity: identity("concern:C-core:a.md"),
            name: "C-core".to_string(),
            member: "a.md".to_string(),
        }];

        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merge");
        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![
                    ordered_config("convergence.ordering", "draft", 0),
                    config("convergence.active", "draft"),
                ],
            )
            .expect("config rows");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot_fact("s1", "2026-06-02", "a.md", "status", "draft")],
            )
            .expect("snapshot rows");

        let named = legacy_named_database_from_store(&store);
        let tuple = TupleDb::from_store_with_visibility(&store, |_| true);

        let named_relations = named
            .stored
            .keys()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(tuple.relation_names(), named_relations);

        for descriptor in STORED_RELATION_DESCRIPTORS {
            let relation = Ident::new_unchecked(descriptor.name);
            let expected = named
                .stored
                .get(&relation)
                .expect("named relation exists")
                .rows
                .iter()
                .map(named_row_to_string_map)
                .collect::<Vec<_>>();
            let actual = tuple.projected_rows(descriptor.name);
            assert_eq!(
                actual, expected,
                "tuple lowering must preserve values and canonical row order for {}",
                descriptor.name
            );
        }
    }

    #[cfg(feature = "physical-substrate")]
    fn named_row_to_string_map(row: &NamedRow) -> BTreeMap<String, Value> {
        row.iter()
            .map(|(field, value)| (field.to_string(), value.clone()))
            .collect()
    }

    #[cfg(feature = "physical-substrate")]
    fn legacy_named_database_from_store(store: &FactStore) -> Database {
        let mut db = Database::default();
        let hidden_handles = hidden_handles(store, &|_| true);
        db.insert_named_rows("handle", store.handles().iter().map(handle_row));
        db.insert_named_rows(
            "edge",
            store
                .edges()
                .iter()
                .filter(|fact| {
                    !hidden_handles.contains(&fact.from) && !hidden_handles.contains(&fact.to)
                })
                .map(edge_row),
        );
        db.insert_named_rows(
            "meta",
            store
                .meta()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.handle))
                .map(meta_row),
        );
        db.insert_named_rows(
            "content",
            store
                .content()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.handle))
                .map(content_row),
        );
        db.insert_named_rows(
            "span",
            store
                .spans()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.handle))
                .map(span_row),
        );
        db.insert_named_rows(
            "concern",
            store
                .concerns()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.member))
                .map(concern_row),
        );
        db.insert_named_rows("config", store.configs().iter().map(config_row));
        db.insert_named_rows(
            "snapshot",
            store
                .snapshots()
                .iter()
                .filter(|fact| !hidden_handles.contains(&fact.id))
                .map(snapshot_row),
        );
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

    fn restricted_actor() -> ActorContext {
        ActorContext {
            actor: "restricted".to_string(),
            capabilities: BTreeSet::new(),
        }
    }

    fn trail_session_id(value: &str) -> TrailSessionId {
        TrailSessionId::new(value).expect("valid trail session id")
    }

    fn trail_fixture_entry(session_id: &str, step: u64, expr: &str) -> TrailEntryInProgress {
        TrailEntryInProgress {
            session_id: trail_session_id(session_id),
            step,
            timestamp: "2026-05-16T00:00:00Z".to_string(),
            corpus: CorpusId::from("test"),
            verb: "-e".to_string(),
            expr: expr.to_string(),
            surfaced_refs: vec![TrailReference {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                handle: "alpha.md".to_string(),
                span_id: Some("body".to_string()),
                score: Some(0.875),
            }],
            consumed_refs: vec![TrailReference {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                handle: "beta.md".to_string(),
                span_id: None,
                score: None,
            }],
            prelude_hash: "prelude-v1".to_string(),
            source_generations: vec![TrailGeneration {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                generation: Generation::new(3),
            }],
            retention: None,
        }
    }

    fn trail_store(temp: &TempDir) -> JsonlTrailStore {
        JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(temp.path().join("trails")).expect("tempdir path is utf-8"),
        )
    }

    fn multi_source_search_database() -> Database {
        let mut lexical = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("lexical"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        let mut lexical_handle =
            handle_with_summary("lexical.md", "file", "current", "", "notes", "Same topic");
        lexical_handle.identity = identity_for_source("lexical", "lexical.md");
        lexical.handles = vec![lexical_handle];

        let mut semantic = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("semantic"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        let mut semantic_handle =
            handle_with_summary("semantic.md", "file", "current", "", "notes", "Same topic");
        semantic_handle.identity = identity_for_source("semantic", "semantic.md");
        semantic.handles = vec![semantic_handle];

        let mut store = FactStore::default();
        store.merge(lexical).expect("lexical fixture merge");
        store.merge(semantic).expect("semantic fixture merge");
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

    fn time_travel_metric_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![handle_with_options(
            "draft.md",
            "file",
            Some("current"),
            "",
            "core",
            Some("2026-05-01"),
        )];
        let mut store = FactStore::default();
        store
            .merge(batch)
            .expect("time travel metric fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "draft.md", "status", "draft"),
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
        let program = parse_prelude_program(
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

    fn derivation_contains(node: &DerivationNode, kind: DerivationKind) -> bool {
        node.kind == kind
            || node
                .children
                .iter()
                .any(|child| derivation_contains(child, kind))
    }

    fn derivation_rule_depth(node: &DerivationNode) -> usize {
        let child_depth = node
            .children
            .iter()
            .map(derivation_rule_depth)
            .max()
            .unwrap_or(0);
        if node.kind == DerivationKind::Rule {
            child_depth + 1
        } else {
            child_depth
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
    fn read_clips_first_oversized_span_to_budget() {
        let output = evaluate_query_output(
            r#"? read("long.md", 3, span_id, text, start_line, end_line, tokens)."#,
            oversized_content_database(),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![row([
                ("span_id", s("full")),
                ("text", s("abcdefghijkl\n...")),
                ("start_line", n(1)),
                ("end_line", n(99)),
                ("tokens", n(3)),
            ])],
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

    #[derive(Debug)]
    struct StaticContentProvider;

    impl ContentProvider for StaticContentProvider {
        fn read(
            &self,
            request: ReadRequest<'_>,
            ctx: &ReadContext<'_>,
        ) -> Result<Vec<ReadChunk>, ReadError> {
            assert_eq!(ctx.actor().actor, "anonymous-cli");
            assert_eq!(request.handle(), "external.md");
            assert_eq!(request.budget(), 20);
            assert_eq!(request.span_id(), Some("s2"));
            Ok(vec![ReadChunk::new(
                request.handle(),
                "s2",
                "lazy provider content",
                40,
                41,
                7,
            )])
        }

        fn read_full(
            &self,
            request: ReadFullRequest<'_>,
            _ctx: &ReadContext<'_>,
        ) -> Result<Option<ReadFullContent>, ReadError> {
            Ok(Some(ReadFullContent::new(
                request.handle(),
                "lazy provider content",
                7,
            )))
        }
    }

    #[test]
    fn read_primitive_uses_configured_content_provider() {
        let output = evaluate_query_output(
            r#"? read("external.md", 20, "s2", text, start_line, end_line, tokens)."#,
            Database::default().with_content_provider(StaticContentProvider),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![row([
                ("text", s("lazy provider content")),
                ("start_line", n(40)),
                ("end_line", n(41)),
                ("tokens", n(7)),
            ])],
        );
    }

    #[derive(Debug)]
    struct DenyActionPolicy(ActionKind);

    impl Policy for DenyActionPolicy {
        fn check(&self, _actor: &ActorContext, action: &Action) -> PolicyDecision {
            if action.kind() == self.0 {
                PolicyDecision::Deny
            } else {
                PolicyDecision::Allow
            }
        }
    }

    #[derive(Debug)]
    struct DenyActorActionPolicy {
        actor: &'static str,
        action: ActionKind,
    }

    impl Policy for DenyActorActionPolicy {
        fn check(&self, actor: &ActorContext, action: &Action) -> PolicyDecision {
            if actor.actor == self.actor && action.kind() == self.action {
                PolicyDecision::Deny
            } else {
                PolicyDecision::Allow
            }
        }
    }

    #[derive(Debug)]
    struct DenyReadHandlePolicy(&'static str);

    impl Policy for DenyReadHandlePolicy {
        fn check(&self, _actor: &ActorContext, action: &Action) -> PolicyDecision {
            if matches!(action, Action::Read { handle } if handle == self.0) {
                PolicyDecision::Deny
            } else {
                PolicyDecision::Allow
            }
        }
    }

    #[derive(Debug)]
    struct PanicContentProvider;

    impl ContentProvider for PanicContentProvider {
        fn read(
            &self,
            _request: ReadRequest<'_>,
            _ctx: &ReadContext<'_>,
        ) -> Result<Vec<ReadChunk>, ReadError> {
            panic!("read provider should not be invoked after policy denial");
        }

        fn read_full(
            &self,
            _request: ReadFullRequest<'_>,
            _ctx: &ReadContext<'_>,
        ) -> Result<Option<ReadFullContent>, ReadError> {
            panic!("read_full provider should not be invoked after policy denial");
        }
    }

    #[derive(Debug)]
    struct PanicSearchProvider;

    impl SearchProvider for PanicSearchProvider {
        fn search(
            &self,
            _request: SearchRequest<'_>,
            _ctx: &SearchContext<'_>,
        ) -> Result<Vec<crate::SearchHit>, SearchError> {
            panic!("search provider should not be invoked after policy denial");
        }
    }

    #[test]
    fn policy_denies_primitives_before_provider_or_regex_work() {
        let search_err = evaluate_query_error_with_options(
            r#"? search("needle", h, span_id, score, reason, field, low_confidence)."#,
            Database::default().with_search_provider(PanicSearchProvider),
            EvalOptions::default().with_policy(DenyActionPolicy(ActionKind::Search)),
        );
        assert!(matches!(
            search_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Search
        ));

        let read_err = evaluate_query_error_with_options(
            r#"? read("external.md", 10, span_id, text, start_line, end_line, tokens)."#,
            Database::default().with_content_provider(PanicContentProvider),
            EvalOptions::default().with_policy(DenyActionPolicy(ActionKind::Read)),
        );
        assert!(matches!(
            read_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Read
        ));

        let full_err = evaluate_query_error_with_options(
            r#"? read_full("external.md", content)."#,
            Database::default().with_content_provider(PanicContentProvider),
            EvalOptions::default()
                .with_capability(READ_FULL_CAPABILITY)
                .with_policy(DenyActionPolicy(ActionKind::ReadFull)),
        );
        assert!(matches!(
            full_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::ReadFull
        ));

        let match_err = evaluate_query_error_with_options(
            r#"? match("[", "alpha.md", line, snippet)."#,
            content_database(),
            EvalOptions::default().with_policy(DenyActionPolicy(ActionKind::Match)),
        );
        assert!(matches!(
            match_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Match
        ));
    }

    #[test]
    fn policy_can_deny_by_actor_identity_and_allow_other_actors() {
        let denied = evaluate_query_error_with_options(
            r#"? read("alpha.md", 9, span_id, text, start_line, end_line, tokens)."#,
            content_database(),
            EvalOptions::default()
                .with_actor(
                    ActorContext {
                        actor: "blocked".to_string(),
                        capabilities: BTreeSet::new(),
                    }
                    .with_runtime_capability(RuntimeCapability::Eval),
                )
                .with_policy(DenyActorActionPolicy {
                    actor: "blocked",
                    action: ActionKind::Read,
                }),
        );
        assert!(matches!(
            denied,
            EvalError::PolicyDenied {
                actor,
                action,
            } if actor == "blocked" && action.kind() == ActionKind::Read
        ));

        let output = evaluate_query_output_with_options(
            r#"? read("alpha.md", 9, span_id, text, start_line, end_line, tokens)."#,
            content_database(),
            EvalOptions::default()
                .with_actor(
                    ActorContext {
                        actor: "allowed".to_string(),
                        capabilities: BTreeSet::new(),
                    }
                    .with_runtime_capability(RuntimeCapability::Eval),
                )
                .with_policy(DenyActorActionPolicy {
                    actor: "blocked",
                    action: ActionKind::Read,
                }),
        );
        assert_eq!(output.rows.len(), 2);
    }

    #[test]
    fn policy_actions_include_resource_targets() {
        let denied = evaluate_query_error_with_options(
            r#"? read("alpha.md", 9, span_id, text, start_line, end_line, tokens)."#,
            content_database(),
            EvalOptions::default().with_policy(DenyReadHandlePolicy("alpha.md")),
        );
        assert!(matches!(
            denied,
            EvalError::PolicyDenied {
                action: Action::Read { handle },
                ..
            } if handle == "alpha.md"
        ));

        let output = evaluate_query_output_with_options(
            r#"? read("beta.md", 10, "shared", text, start_line, end_line, tokens)."#,
            content_database(),
            EvalOptions::default().with_policy(DenyReadHandlePolicy("alpha.md")),
        );
        assert_eq!(output.rows.len(), 1);
    }

    #[test]
    fn eval_action_authorization_distinguishes_capability_and_policy() {
        let missing = EvalOptions::default()
            .with_actor(ActorContext::anonymous_mcp())
            .authorize_eval()
            .expect_err("eval capability is required");
        assert!(matches!(
            missing,
            EvalError::CapabilityRequired {
                primitive: "eval",
                capability: RuntimeCapability::Eval,
            }
        ));

        let denied = EvalOptions::default()
            .with_capability(RuntimeCapability::Eval)
            .with_policy(DenyActionPolicy(ActionKind::Eval))
            .authorize_eval()
            .expect_err("policy can deny eval after capability passes");
        assert!(matches!(
            denied,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Eval
        ));

        EvalOptions::default()
            .with_capability(RuntimeCapability::Eval)
            .authorize_eval()
            .expect("default policy allows eval once capability is present");
    }

    #[test]
    fn evaluator_entrypoints_enforce_eval_policy() {
        let program = parse_program("inline", r"? *handle{id: h}.").expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query present");
        let mut evaluator = Evaluator::with_options(
            analyzed,
            content_database(),
            EvalOptions::default().with_policy(DenyActionPolicy(ActionKind::Eval)),
        );

        let fixpoint_err = evaluator
            .run_fixpoint()
            .expect_err("run_fixpoint is eval-gated");
        assert!(matches!(
            fixpoint_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Eval
        ));

        let query_err = evaluator
            .eval_query(&query)
            .expect_err("eval_query is eval-gated");
        assert!(matches!(
            query_err,
            EvalError::PolicyDenied {
                action,
                ..
            } if action.kind() == ActionKind::Eval
        ));
    }

    #[derive(Debug)]
    struct OvereagerContentProvider;

    impl ContentProvider for OvereagerContentProvider {
        fn read(
            &self,
            request: ReadRequest<'_>,
            _ctx: &ReadContext<'_>,
        ) -> Result<Vec<ReadChunk>, ReadError> {
            Ok(vec![
                ReadChunk::new(request.handle(), "a", "fits", 1, 1, 4),
                ReadChunk::new(request.handle(), "b", "too far", 2, 2, 100),
                ReadChunk::new(request.handle(), "c", "would fit only if skipping", 3, 3, 1),
            ])
        }

        fn read_full(
            &self,
            request: ReadFullRequest<'_>,
            _ctx: &ReadContext<'_>,
        ) -> Result<Option<ReadFullContent>, ReadError> {
            Ok(Some(ReadFullContent::new(
                request.handle(),
                "too much content",
                request.token_limit().saturating_add(1),
            )))
        }
    }

    #[test]
    fn runtime_enforces_read_budget_over_custom_provider_chunks() {
        let output = evaluate_query_output(
            r#"? read("external.md", 10, span_id, text, start_line, end_line, tokens)."#,
            Database::default().with_content_provider(OvereagerContentProvider),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![row([
                ("span_id", s("a")),
                ("text", s("fits")),
                ("start_line", n(1)),
                ("end_line", n(1)),
                ("tokens", n(4)),
            ])],
        );
    }

    #[test]
    fn runtime_enforces_read_full_limit_over_custom_provider() {
        let err = evaluate_query_error_with_options(
            r#"? read_full("external.md", content)."#,
            Database::default().with_content_provider(OvereagerContentProvider),
            EvalOptions::default()
                .with_capability(READ_FULL_CAPABILITY)
                .with_read_full_token_limit(10),
        );

        assert!(matches!(
            err,
            EvalError::ReadFullBudgetExceeded {
                handle,
                tokens: 11,
                limit: 10
            } if handle == "external.md"
        ));
    }

    #[test]
    fn content_provider_results_are_filtered_for_known_hidden_handles() {
        let store = visibility_store();
        let database = Database::from_store_for_actor(&store, &restricted_actor())
            .with_content_provider(OvereagerContentProvider);

        let read_output = evaluate_query_output(
            r#"? read("secret.md", 10, span_id, text, start_line, end_line, tokens)."#,
            database.clone(),
        );
        assert!(read_output.rows.is_empty());

        let full_output = evaluate_query_output_with_options(
            r#"? read_full("secret.md", content)."#,
            database,
            EvalOptions::default().with_capability(READ_FULL_CAPABILITY),
        );
        assert!(full_output.rows.is_empty());
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
        assert!((value_f64(rows[0].get("score").expect("score")) - 1.0).abs() < 0.000_001);

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
    fn empty_search_query_is_invalid_at_runtime_boundary() {
        let err = evaluate_query_error(
            r#"? search("   ", h, span_id, score, reason, field, low_confidence)."#,
            search_database(),
        );

        assert!(matches!(
            err,
            EvalError::SearchProvider(SearchError::EmptyQuery)
        ));
    }

    #[test]
    fn actor_scoped_database_filters_private_facts_before_derivation() {
        let store = visibility_store();
        let database = Database::from_store_for_actor(&store, &restricted_actor());
        let outputs = evaluate_queries(
            r#"
            diagnostic_leak(h) := *meta{handle: h, key: "leaks-diagnostic", value: "true"}.
            ? count = Count{ h : *handle{id: h} }.
            ? search("secret", h, span_id, score, reason, field, low_confidence).
            ? read("secret.md", 10, span_id, text, start_line, end_line, tokens).
            ? diagnostic_leak(h).
            ? *snapshot{id: "secret.md", key, value}.
            "#,
            database,
        );

        assert_query_rows(&outputs[0], vec![row([("count", n(1))])]);
        assert!(outputs[1].is_empty(), "search leaked hidden row");
        assert!(outputs[2].is_empty(), "read leaked hidden row");
        assert!(outputs[3].is_empty(), "derivation leaked hidden row");
        assert!(outputs[4].is_empty(), "snapshot leaked hidden row");
    }

    #[test]
    fn all_visible_database_preserves_private_fact_behavior_for_cli_callers() {
        let store = visibility_store();
        let outputs = evaluate_queries(
            r#"
            diagnostic_leak(h) := *meta{handle: h, key: "leaks-diagnostic", value: "true"}.
            ? count = Count{ h : *handle{id: h} }.
            ? search("secret", h, span_id, score, reason, field, low_confidence).
            ? read("secret.md", 10, span_id, text, start_line, end_line, tokens).
            ? diagnostic_leak(h).
            ? *snapshot{id: "secret.md", key, value}.
            "#,
            Database::from_store(&store),
        );

        assert_query_rows(&outputs[0], vec![row([("count", n(3))])]);
        assert!(
            outputs[1]
                .iter()
                .any(|row| row.get("h") == Some(&s("secret.md"))),
            "all-visible search should include private fixture"
        );
        assert_query_rows(
            &outputs[2],
            vec![row([
                ("span_id", s("body")),
                ("text", s("secret roadmap")),
                ("start_line", n(1)),
                ("end_line", n(1)),
                ("tokens", n(4)),
            ])],
        );
        assert_query_rows(&outputs[3], vec![row([("h", s("secret.md"))])]);
        assert_query_rows(
            &outputs[4],
            vec![row([("key", s("status")), ("value", s("current"))])],
        );
    }

    #[test]
    fn private_visibility_capability_admits_private_rows() {
        let store = visibility_store();
        let actor = restricted_actor().with_fact_visibility_capability(FactVisibility::Private);
        let output = evaluate_query_output(
            r"? *handle{id: h}.",
            Database::from_store_for_actor(&store, &actor),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 3);
        assert!(rows.contains(&row([("h", s("public.md"))])));
        assert!(rows.contains(&row([("h", s("team.md"))])));
        assert!(rows.contains(&row([("h", s("secret.md"))])));
    }

    #[test]
    fn team_visibility_capability_excludes_private_rows() {
        let store = visibility_store();
        let actor = restricted_actor().with_fact_visibility_capability(FactVisibility::Team);
        let output = evaluate_query_output(
            r"? *handle{id: h}.",
            Database::from_store_for_actor(&store, &actor),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&row([("h", s("public.md"))])));
        assert!(rows.contains(&row([("h", s("team.md"))])));
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

    #[derive(Debug)]
    struct StaticSearchProvider;

    impl SearchProvider for StaticSearchProvider {
        fn search(
            &self,
            request: SearchRequest<'_>,
            ctx: &SearchContext<'_>,
        ) -> Result<Vec<SearchHit>, SearchError> {
            assert_eq!(ctx.actor().actor, "anonymous-cli");
            assert_eq!(request.query(), "needle");
            assert_eq!(request.handle(), None);
            assert_eq!(request.span(), SearchSpanScope::Any);
            assert_eq!(request.reason(), None);
            assert_eq!(request.field(), None);
            Ok(vec![
                SearchHit::new(
                    "test",
                    "lexical",
                    "lexical.md",
                    Some("body".to_string()),
                    1.0,
                    "provider",
                    "body",
                ),
                SearchHit::new(
                    "test",
                    "semantic",
                    "semantic.md",
                    Some("body".to_string()),
                    0.2,
                    "provider",
                    "body",
                ),
            ])
        }
    }

    #[derive(Debug)]
    struct HiddenSearchProvider;

    impl SearchProvider for HiddenSearchProvider {
        fn search(
            &self,
            _request: SearchRequest<'_>,
            _ctx: &SearchContext<'_>,
        ) -> Result<Vec<SearchHit>, SearchError> {
            Ok(vec![SearchHit::new(
                "test",
                "external",
                "secret.md",
                Some("body".to_string()),
                1.0,
                "provider",
                "body",
            )])
        }
    }

    #[derive(Debug)]
    struct PreferSemanticRanker;

    impl Ranker for PreferSemanticRanker {
        fn calibrate(&self, hit: &SearchHit, _ctx: &RankingContext) -> f32 {
            if hit.source() == "semantic" { 1.0 } else { 0.1 }
        }

        fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering {
            DefaultRanker.tie_break(a, b)
        }
    }

    #[test]
    fn search_primitive_uses_provider_before_runtime_ranking() {
        let output = evaluate_query_output_with_options(
            r#"? search("needle", h, span_id, score, reason, field, low_confidence)."#,
            Database::default().with_search_provider(StaticSearchProvider),
            EvalOptions::default().with_ranker(PreferSemanticRanker),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("h"), Some(&s("semantic.md")));
        assert_eq!(rows[0].get("score"), Some(&f(1.0)));
        assert_eq!(rows[1].get("h"), Some(&s("lexical.md")));
        assert!((value_f64(rows[1].get("score").expect("score")) - 0.1).abs() < 0.000_001);
    }

    #[test]
    fn search_provider_results_are_filtered_for_known_hidden_handles() {
        let store = visibility_store();
        let output = evaluate_query_output(
            r#"? search("secret", h, span_id, score, reason, field, low_confidence)."#,
            Database::from_store_for_actor(&store, &restricted_actor())
                .with_search_provider(HiddenSearchProvider),
        );

        assert!(output.rows.is_empty());
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
    fn low_confidence_policy_is_executable_before_top_k() {
        let options = EvalOptions::default().with_low_confidence_threshold(0.99);
        let raw = evaluate_query_output_with_options(
            r#"? search("C-conformance", h, span_id, score, reason, field, low_confidence)."#,
            search_database(),
            options.clone(),
        );
        assert!(
            raw.rows
                .iter()
                .any(|row| row.fields.get("low_confidence") == Some(&Value::Bool(true))),
            "fixture should produce low-confidence rows at the stricter threshold"
        );

        let filtered = evaluate_query_output_with_options(
            r#"
            ? (h, score, low_confidence) = TopK{ k: 10, key: score :
                (h, score, low_confidence) :
                search("C-conformance", h, span_id, score, reason, field, low_confidence),
                low_confidence = false
              }.
            "#,
            search_database(),
            options.clone(),
        );
        assert!(filtered.rows.is_empty());

        let included = evaluate_query_output_with_options(
            r#"
            ? (h, score, low_confidence) = TopK{ k: 10, key: score :
                (h, score, low_confidence) :
                search("C-conformance", h, span_id, score, reason, field, low_confidence)
              }.
            "#,
            search_database(),
            options,
        );
        assert!(!included.rows.is_empty());
    }

    #[derive(Clone, Debug)]
    struct SourceCalibratingRanker;

    impl Ranker for SourceCalibratingRanker {
        fn calibrate(&self, hit: &SearchHit, _ctx: &RankingContext) -> f32 {
            if hit.source() == "semantic" {
                0.95
            } else {
                0.45
            }
        }

        fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering {
            DefaultRanker.tie_break(a, b)
        }
    }

    #[test]
    fn custom_ranker_can_calibrate_across_sources() {
        let output = evaluate_query_output_with_options(
            r#"? search("same topic", h, span_id, score, reason, field, low_confidence)."#,
            multi_source_search_database(),
            EvalOptions::default().with_ranker(SourceCalibratingRanker),
        );
        let rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("h"), Some(&s("semantic.md")));
        assert_eq!(rows[0].get("low_confidence"), Some(&Value::Bool(false)));
        assert_eq!(rows[1].get("h"), Some(&s("lexical.md")));
        assert_eq!(rows[1].get("low_confidence"), Some(&Value::Bool(true)));
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
    fn self_description_primitives_are_queryable() {
        let outputs = evaluate_queries(
            r#"issue("a", "error").
release_blocker(code) := issue(code, "error").
@verb(name: "broken", query: "? release_blocker(code).", doc: "Show release blockers.", output_schema: "{\"code\":\"String\"}", args: [], capabilities: []).
? schema("search", kind, signature, determinism, provenance).
? schema("release_blocker", kind, signature, determinism, provenance).
? predicates("release_blocker", doc, file, lines).
? verbs("broken", query, doc, output_schema).
? describe("runtime", doc).
? source_of("release_blocker", file, lines).
? examples("search", example)."#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![row([
                ("kind", s("primitive")),
                (
                    "signature",
                    s("search(query, handle, span_id, score, reason, field, low_confidence)"),
                ),
                ("determinism", s("ranker-dependent deterministic")),
                ("provenance", s("engine")),
            ])],
        );
        assert_query_rows(
            &outputs[1],
            vec![row([
                ("kind", s("derived")),
                ("signature", s("release_blocker(code)")),
                ("determinism", s("deterministic")),
                ("provenance", s("unknown")),
            ])],
        );
        assert_query_rows(
            &outputs[2],
            vec![row([
                ("doc", s("Rule-defined predicate release_blocker.")),
                ("file", s("inline")),
                ("lines", s("2")),
            ])],
        );
        assert_query_rows(
            &outputs[3],
            vec![row([
                ("query", s("? release_blocker(code).")),
                ("doc", s("Show release blockers.")),
                ("output_schema", s(r#"{"code":"String"}"#)),
            ])],
        );
        assert!(
            matches!(
                outputs[4][0].get("doc"),
                Some(Value::String(doc))
                    if doc.contains("schema")
                        && doc.contains("describe")
                        && doc.contains("discover")
            ),
            "runtime description should orient a cold agent"
        );
        assert_query_rows(
            &outputs[5],
            vec![row([("file", s("inline")), ("lines", s("2"))])],
        );
        assert!(
            matches!(
                outputs[6][0].get("example"),
                Some(Value::String(example)) if example.contains("v17 conformance audit")
            ),
            "search example should be concrete"
        );
    }

    #[test]
    fn sources_primitive_lists_linked_adapter_capabilities() {
        let source = SourceInfo {
            name: "markdown",
            recognizes: vec![Pattern::new("**/*.md")],
            doc: "Markdown source",
            config_keys: Vec::new(),
            capabilities: SourceCapabilities {
                supports_git_ref: false,
                supports_time_snapshot: false,
                supports_incremental: true,
                live_only: false,
            },
            search: Some(default_lexical_search_info()),
        };

        let outputs = evaluate_queries(
            "? sources(name, recognizes, capabilities, doc).",
            Database::default().with_sources([source]),
        );

        assert_query_rows(
            &outputs[0],
            vec![row([
                ("name", s("markdown")),
                ("recognizes", list([s("**/*.md")])),
                (
                    "capabilities",
                    list([s("supports_incremental"), s("search")]),
                ),
                ("doc", s("Markdown source")),
            ])],
        );
    }

    #[test]
    fn trail_store_projects_redacted_entries_into_relations() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let redactor = DefaultTrailRedactor::default();
        let actor = ActorContext::anonymous_cli();
        let ctx = TrailContext::from(&actor).with_visibility(FactVisibility::Public);
        let options = EvalOptions::default();
        store
            .append(
                redactor.redact(
                    trail_fixture_entry("session-1", 1, r#"? read("alpha.md", span, text)."#),
                    &ctx,
                ),
                &ctx,
            )
            .expect("append public trail row");

        let database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1").expect("valid query"),
                &options,
            )
            .expect("trail rows load");
        let outputs = evaluate_queries(
            r#"
? *trail{session_id, step, actor, redacted_expr, prelude_hash, visibility, retention}.
? *trail_ref{session_id, step, kind, ordinal, handle, span_id, score}.
? *trail_generation{session_id, step, corpus, source, generation}.
? schema("trail_ref", kind, signature, determinism, provenance)."#,
            database,
        );

        assert_query_rows(
            &outputs[0],
            vec![row([
                ("session_id", s("session-1")),
                ("step", n(1)),
                ("actor", s("anonymous-cli")),
                ("redacted_expr", s(r#"? read("<redacted>", span, text)."#)),
                ("prelude_hash", s("prelude-v1")),
                ("visibility", s("public")),
                ("retention", Value::Null),
            ])],
        );
        assert_query_rows(
            &outputs[1],
            vec![
                row([
                    ("session_id", s("session-1")),
                    ("step", n(1)),
                    ("kind", s("consumed")),
                    ("ordinal", n(0)),
                    ("handle", s("beta.md")),
                    ("span_id", Value::Null),
                    ("score", Value::Null),
                ]),
                row([
                    ("session_id", s("session-1")),
                    ("step", n(1)),
                    ("kind", s("surfaced")),
                    ("ordinal", n(0)),
                    ("handle", s("alpha.md")),
                    ("span_id", s("body")),
                    ("score", f(0.875)),
                ]),
            ],
        );
        assert_query_rows(
            &outputs[2],
            vec![row([
                ("session_id", s("session-1")),
                ("step", n(1)),
                ("corpus", s("test")),
                ("source", s("md")),
                ("generation", n(3)),
            ])],
        );
        assert_query_rows(
            &outputs[3],
            vec![row([
                ("kind", s("stored")),
                (
                    "signature",
                    s(
                        "*trail_ref{session_id, step, kind, ordinal, corpus, source, handle, span_id, score}",
                    ),
                ),
                ("determinism", s("input")),
                ("provenance", s("runtime")),
            ])],
        );
    }

    #[test]
    fn trail_projection_enforces_store_visibility() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let redactor = DefaultTrailRedactor::default();
        let public_actor = ActorContext::anonymous_cli();
        let public_ctx = TrailContext::from(&public_actor).with_visibility(FactVisibility::Public);
        store
            .append(
                redactor.redact(
                    trail_fixture_entry("session-1", 1, "? work(h)."),
                    &public_ctx,
                ),
                &public_ctx,
            )
            .expect("append public trail row");
        let private_actor = ActorContext::trusted_cli();
        let private_ctx =
            TrailContext::from(&private_actor).with_visibility(FactVisibility::Private);
        store
            .append(
                redactor.redact(
                    trail_fixture_entry("session-1", 2, "? blocked(h)."),
                    &private_ctx,
                ),
                &private_ctx,
            )
            .expect("append private trail row");

        let restricted = restricted_actor();
        let restricted_options = EvalOptions::default().with_actor(
            restricted
                .clone()
                .with_runtime_capability(RuntimeCapability::Eval),
        );
        let public_database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1").expect("valid query"),
                &restricted_options,
            )
            .expect("visible trail rows load");
        let outputs = evaluate_queries(r"? *trail{session_id, step, visibility}.", public_database);
        assert_query_rows(
            &outputs[0],
            vec![row([
                ("session_id", s("session-1")),
                ("step", n(1)),
                ("visibility", s("public")),
            ])],
        );

        let err = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .include_private(true),
                &restricted_options,
            )
            .expect_err("private trail load requires capability");
        assert!(matches!(
            err,
            TrailError::Authorization(AuthorizationError::CapabilityRequired { .. })
        ));

        let private_options = EvalOptions::default().with_actor(private_actor.clone());
        let private_database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .include_private(true),
                &private_options,
            )
            .expect("private trail rows load for trusted actor");
        let restricted_output = evaluate_query_output_with_options(
            r"? *trail{session_id, step, visibility}.",
            private_database.clone(),
            restricted_options,
        );
        let mut rows = restricted_output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();
        rows.sort();
        assert_query_rows(
            &rows,
            vec![row([
                ("session_id", s("session-1")),
                ("step", n(1)),
                ("visibility", s("public")),
            ])],
        );

        let output = evaluate_query_output_with_options(
            r"? *trail{session_id, step, visibility}.",
            private_database,
            private_options,
        );
        let mut rows = output
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();
        rows.sort();
        assert_query_rows(
            &rows,
            vec![
                row([
                    ("session_id", s("session-1")),
                    ("step", n(1)),
                    ("visibility", s("public")),
                ]),
                row([
                    ("session_id", s("session-1")),
                    ("step", n(2)),
                    ("visibility", s("private")),
                ]),
            ],
        );
    }

    #[test]
    fn explain_attaches_per_row_stored_and_rule_derivations() {
        let output = evaluate_query_output_with_options(
            r#"
            blocked(h) := *handle{id: h, status: "open"}.
            ? blocked(h).
            "#,
            Database::from_store(&fixture_store()),
            EvalOptions::default().with_explain(),
        );

        assert_eq!(output.rows.len(), 1);
        let derivation = output.rows[0]
            .derivation
            .as_ref()
            .expect("row has derivation");
        assert_eq!(derivation.kind, DerivationKind::Query);
        assert!(derivation_contains(derivation, DerivationKind::Rule));
        assert!(derivation_contains(derivation, DerivationKind::Stored));
        assert!(!output.rows[0].fields.contains_key("_derivation"));
    }

    #[test]
    fn explain_defaults_to_first_three_rows() {
        let output = evaluate_query_output_with_options(
            r"? *handle{id: h}.",
            Database::from_store(&fixture_store()),
            EvalOptions::default().with_explain(),
        );

        assert_eq!(output.rows.len(), 5);
        assert_eq!(
            output
                .rows
                .iter()
                .filter(|row| row.derivation.is_some())
                .count(),
            3
        );
        assert!(
            output.rows[3..].iter().all(|row| row.derivation.is_none()),
            "rows after the default explain cap should remain bare"
        );
    }

    #[test]
    fn explain_first_and_all_control_row_count() {
        let first_two = evaluate_query_output_with_options(
            r"? *handle{id: h}.",
            Database::from_store(&fixture_store()),
            EvalOptions::default().with_explain_first(2),
        );
        assert_eq!(
            first_two
                .rows
                .iter()
                .filter(|row| row.derivation.is_some())
                .count(),
            2
        );

        let all = evaluate_query_output_with_options(
            r"? *handle{id: h}.",
            Database::from_store(&fixture_store()),
            EvalOptions::default().with_explain_all(),
        );
        assert_eq!(
            all.rows
                .iter()
                .filter(|row| row.derivation.is_some())
                .count(),
            all.rows.len()
        );
    }

    #[test]
    fn explain_depth_bounds_recursive_rule_chains_by_default() {
        let output = evaluate_query_output_with_options(
            r#"
            edge("a", "b").
            edge("b", "c").
            edge("c", "d").
            path(x, y) := edge(x, y).
            path(x, z) := edge(x, y), path(y, z).
            ? path("a", "d").
            "#,
            Database::default(),
            EvalOptions::default().with_explain(),
        );

        assert_eq!(output.rows.len(), 1);
        let derivation = output.rows[0]
            .derivation
            .as_ref()
            .expect("row has derivation");
        assert!(derivation_contains(
            derivation,
            DerivationKind::RecursiveChain
        ));
    }

    #[test]
    fn explicit_explain_depth_expands_recursive_rule_chains_until_limit() {
        let output = evaluate_query_output_with_options(
            r#"
            edge("a", "b").
            edge("b", "c").
            edge("c", "d").
            path(x, y) := edge(x, y).
            path(x, z) := edge(x, y), path(y, z).
            ? path("a", "d").
            "#,
            Database::default(),
            EvalOptions::default().with_explain_depth(8),
        );

        assert_eq!(output.rows.len(), 1);
        let derivation = output.rows[0]
            .derivation
            .as_ref()
            .expect("row has derivation");
        assert!(!derivation_contains(
            derivation,
            DerivationKind::RecursiveChain
        ));
        assert!(
            derivation_rule_depth(derivation) >= 2,
            "explicit depth should expose recursive rule chain: {derivation:?}"
        );
    }

    #[test]
    fn explain_rejects_reserved_derivation_output_binding() {
        let err = evaluate_query_error_with_options(
            r"? *handle{id: _derivation}.",
            Database::from_store(&fixture_store()),
            EvalOptions::default().with_explain(),
        );

        assert!(matches!(
            err,
            EvalError::ReservedExplainField {
                field: "_derivation"
            }
        ));
    }

    #[test]
    fn explain_uses_visible_trail_projection_and_compact_rows() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let redactor = DefaultTrailRedactor::default();
        let public_actor = ActorContext::anonymous_cli();
        let public_ctx = TrailContext::from(&public_actor).with_visibility(FactVisibility::Public);
        store
            .append(
                redactor.redact(
                    trail_fixture_entry("session-1", 1, r#"? read("customer secret", h)."#),
                    &public_ctx,
                ),
                &public_ctx,
            )
            .expect("append trail row");
        let options = EvalOptions::default().with_explain();
        let database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1").expect("valid query"),
                &options,
            )
            .expect("trail rows load");
        let output =
            evaluate_query_output_with_options(r"? *trail{session_id, step}.", database, options);

        assert_eq!(output.rows.len(), 1);
        let derivation = output.rows[0]
            .derivation
            .as_ref()
            .expect("row has derivation");
        assert!(derivation_contains(derivation, DerivationKind::Stored));
        let stored = derivation
            .children
            .iter()
            .find(|node| derivation_contains(node, DerivationKind::Stored))
            .expect("stored trail derivation present");
        assert!(
            !format!("{stored:?}").contains("customer"),
            "explain should use compact redacted trail projection"
        );
    }

    #[test]
    fn trail_summary_reads_from_store() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let redactor = DefaultTrailRedactor::default();
        let actor = ActorContext::anonymous_cli();
        let ctx = TrailContext::from(&actor).with_visibility(FactVisibility::Public);
        store
            .append(
                redactor.redact(trail_fixture_entry("session-1", 1, "? work(h)."), &ctx),
                &ctx,
            )
            .expect("append first trail row");
        store
            .append(
                redactor.redact(trail_fixture_entry("session-1", 2, "? read(h)."), &ctx),
                &ctx,
            )
            .expect("append second trail row");

        let summary = summarize_trail_session(
            &store,
            &trail_session_id("session-1"),
            false,
            &DefaultTrailSummarizer,
            &ctx,
        )
        .expect("summarize persisted trail");
        assert_eq!(summary.session_id, trail_session_id("session-1"));
        assert_eq!(summary.steps, 2);
        assert_eq!(summary.consumed_refs, 2);
    }

    #[test]
    fn empty_trail_projection_leaves_queryable_empty_relations() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("missing-session").expect("valid query"),
                &EvalOptions::default(),
            )
            .expect("empty trail projection loads");
        let outputs = evaluate_queries(
            r"
? *trail{session_id}.
? *trail_ref{session_id}.
? *trail_generation{session_id}.",
            database,
        );
        assert_query_rows(&outputs[0], Vec::new());
        assert_query_rows(&outputs[1], Vec::new());
        assert_query_rows(&outputs[2], Vec::new());
    }

    #[test]
    fn trail_projection_bounds_fanout_and_sanitizes_scores() {
        let temp = tempdir().expect("tempdir");
        let store = trail_store(&temp);
        let redactor = DefaultTrailRedactor::default();
        let actor = ActorContext::anonymous_cli();
        let ctx = TrailContext::from(&actor).with_visibility(FactVisibility::Public);
        let mut entry = trail_fixture_entry("session-1", 1, "? work(h).");
        entry.surfaced_refs = (0..300)
            .map(|idx| TrailReference {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                handle: format!("surfaced-{idx}.md"),
                span_id: None,
                score: Some(if idx == 0 { f32::NAN } else { 0.5 }),
            })
            .collect();
        entry.consumed_refs = (0..300)
            .map(|idx| TrailReference {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                handle: format!("consumed-{idx}.md"),
                span_id: None,
                score: Some(if idx == 0 { 1.5 } else { 0.25 }),
            })
            .collect();
        entry.source_generations = (0..80)
            .map(|idx| TrailGeneration {
                corpus: CorpusId::from("test"),
                source: SourceName::from(format!("source-{idx}")),
                generation: Generation::new(idx + 1),
            })
            .collect();
        store
            .append(redactor.redact(entry, &ctx), &ctx)
            .expect("append bounded fanout trail row");

        let database = Database::default()
            .with_trail_store(
                &store,
                TrailQuery::for_session("session-1").expect("valid query"),
                &EvalOptions::default(),
            )
            .expect("trail rows load");
        let outputs = evaluate_queries(
            r#"
? surfaced = Count{ h : *trail_ref{kind: "surfaced", handle: h} }.
? consumed = Count{ h : *trail_ref{kind: "consumed", handle: h} }.
? generations = Count{ source : *trail_generation{source} }.
? *trail_ref{handle: "surfaced-0.md", score}.
? *trail_ref{handle: "consumed-0.md", score}.
? *trail_ref{handle: "surfaced-1.md", score}."#,
            database,
        );

        assert_query_rows(
            &outputs[0],
            vec![row([(
                "surfaced",
                n(i64::try_from(MAX_TRAIL_REFS_PER_ENTRY).expect("limit fits i64")),
            )])],
        );
        assert_query_rows(
            &outputs[1],
            vec![row([(
                "consumed",
                n(i64::try_from(MAX_TRAIL_REFS_PER_ENTRY).expect("limit fits i64")),
            )])],
        );
        assert_query_rows(
            &outputs[2],
            vec![row([(
                "generations",
                n(i64::try_from(MAX_TRAIL_GENERATIONS_PER_ENTRY).expect("limit fits i64")),
            )])],
        );
        assert_query_rows(&outputs[3], vec![row([("score", Value::Null)])]);
        assert_query_rows(&outputs[4], vec![row([("score", Value::Null)])]);
        assert_query_rows(&outputs[5], vec![row([("score", f(0.5))])]);
    }

    #[test]
    fn trail_relations_index_only_queryable_identity_fields() {
        assert!(should_index_stored_field(
            &Ident::new_unchecked(TRAIL_RELATION),
            &Ident::new_unchecked(SESSION_ID_FIELD),
        ));
        assert!(should_index_stored_field(
            &Ident::new_unchecked(TRAIL_REF_RELATION),
            &Ident::new_unchecked(HANDLE_FIELD),
        ));
        assert!(!should_index_stored_field(
            &Ident::new_unchecked(TRAIL_RELATION),
            &Ident::new_unchecked("redacted_expr"),
        ));
        assert!(!should_index_stored_field(
            &Ident::new_unchecked(TRAIL_REF_RELATION),
            &Ident::new_unchecked("score"),
        ));
    }

    #[test]
    fn query_local_predicates_do_not_leak_between_introspection_queries() {
        let outputs = evaluate_queries(
            r#"seed("a").
?
  where local_only(x) := seed(x).
  schema("local_only", kind, signature, determinism, provenance).
? schema("local_only", kind, signature, determinism, provenance)."#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![row([
                ("kind", s("derived")),
                ("signature", s("local_only(x)")),
                ("determinism", s("deterministic")),
                ("provenance", s("inline")),
            ])],
        );
        assert_query_rows(&outputs[1], Vec::new());
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

    #[cfg(all(
        not(feature = "physical-substrate"),
        not(feature = "legacy-time-clone")
    ))]
    #[test]
    fn time_scope_overlay_matches_clone_scope_for_snapshot_rows_and_graph_primitives() {
        let database = time_travel_metric_database();
        let selection = database
            .resolve_snapshot_selection("snapshot:s2")
            .expect("snapshot fixture resolves");
        let clone_scoped = database.clone_for_time_scope_selection(&selection);
        let overlay_scoped = database.time_scope_overlay(&selection);
        let query = r#"
            ? *handle{id: h, status: status}.
            ? *snapshot{snapshot: snapshot, id: h, key, value}.
            ? active(h).
            ? freshness("draft.md", days).
            ? flux("draft.md", 20, delta).
        "#;

        assert_eq!(
            evaluate_queries(query, clone_scoped),
            evaluate_queries(query, overlay_scoped)
        );
    }

    #[cfg(all(feature = "physical-substrate", not(feature = "legacy-time-clone")))]
    #[test]
    fn tuple_time_scope_overlay_exposes_snapshot_rows_and_patched_handle_tuples() {
        let database = time_travel_metric_database();
        let selection = database
            .resolve_snapshot_selection("snapshot:s2")
            .expect("snapshot fixture resolves");
        let overlay_scoped = database.time_scope_overlay(&selection);
        let query = r#"
            ? *handle{id: h, status: status}.
            ? *snapshot{snapshot: snapshot, id: h, key, value}.
            ? active(h).
            ? freshness("draft.md", days).
            ? flux("draft.md", 20, delta).
        "#;

        let outputs = evaluate_queries(query, overlay_scoped);
        assert_query_rows(
            &outputs[0],
            vec![row([("h", s("draft.md")), ("status", s("draft"))])],
        );
        assert_query_rows(
            &outputs[1],
            vec![row([
                ("h", s("draft.md")),
                ("key", s("status")),
                ("snapshot", s("s2")),
                ("value", s("draft")),
            ])],
        );
        assert_query_rows(&outputs[2], vec![row([("h", s("draft.md"))])]);
        assert_query_rows(&outputs[3], vec![row([("days", n(9))])]);
        assert_query_rows(&outputs[4], vec![row([("delta", n(1))])]);
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
    fn at_snapshot_freshness_uses_selected_snapshot_day() {
        let output = evaluate_query_output(
            r#"
            ? at("snapshot:s2") { freshness("draft.md", days) }.
            "#,
            time_travel_metric_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("days", n(9))])],
        );
    }

    #[test]
    fn at_snapshot_flux_uses_full_status_history() {
        let output = evaluate_query_output(
            r#"
            ? at("snapshot:s2") { flux("draft.md", 20, delta) }.
            "#,
            time_travel_metric_database(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("delta", n(1))])],
        );
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
    fn git_mtime_returns_file_instants() {
        let output = evaluate_query_output(
            r"? git_mtime(file, instant).",
            lifecycle_database().with_git_mtimes([(
                "core/draft.md.md".to_string(),
                "2026-05-20T12:00:00Z".to_string(),
            )]),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([
                ("file", s("core/draft.md.md")),
                ("instant", s("2026-05-20T12:00:00Z")),
            ])],
        );
    }

    #[test]
    fn changed_within_filters_handles_by_git_mtime_window() {
        let output = evaluate_query_output_with_options(
            r"? changed_within(h, 7).",
            lifecycle_database()
                .with_git_mtimes([
                    (
                        "core/draft.md.md".to_string(),
                        "2026-05-20T12:00:00Z".to_string(),
                    ),
                    (
                        "core/done.md.md".to_string(),
                        "2026-04-01T12:00:00Z".to_string(),
                    ),
                ])
                .with_evaluation_day(
                    snapshot_days_since_epoch("2026-05-27").expect("fixture date parses"),
                ),
            EvalOptions::default(),
        );

        assert_query_rows(
            &output
                .rows
                .into_iter()
                .map(|row| row.fields)
                .collect::<Vec<_>>(),
            vec![row([("h", s("draft.md"))])],
        );
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

    #[cfg(not(feature = "physical-substrate"))]
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

    #[cfg(feature = "physical-substrate")]
    #[test]
    fn tuple_relation_uses_bound_field_candidates() {
        let database = Database::from_store(&fixture_store());
        let relation = Ident::new_unchecked("handle");
        let field = Ident::new_unchecked("id");
        let candidates = database
            .candidate_tuple_rows(
                &relation,
                &[(Ident::new_unchecked("id"), Value::String("v17".to_string()))],
            )
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            database.tuple_field_value(&relation, candidates[0], &field),
            Some(Value::String("v17".to_string()))
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
    fn scalar_aggregates_use_documented_value_semantics() {
        let outputs = evaluate_queries(
            r#"
            amount("a", 2).
            amount("b", 2).
            amount("c", 5).
            ? total = Sum{ value : amount(id, value) }.
            ? count = Count{ value : amount(id, value) }.
            ? min = Min{ value : amount(id, value) }.
            ? max = Max{ value : amount(id, value) }.
            ? avg = Avg{ value : amount(id, value) }.
            ? values = List{ value : amount(id, value) }.
            ? values = Set{ value : amount(id, value) }.
            "#,
            Database::default(),
        );

        assert_query_rows(&outputs[0], vec![row([("total", n(9))])]);
        assert_query_rows(&outputs[1], vec![row([("count", n(2))])]);
        assert_query_rows(&outputs[2], vec![row([("min", n(2))])]);
        assert_query_rows(&outputs[3], vec![row([("max", n(5))])]);
        assert_query_rows(&outputs[4], vec![row([("avg", f(3.0))])]);
        assert_query_rows(&outputs[5], vec![row([("values", list([n(2), n(5)]))])]);
        assert_query_rows(&outputs[6], vec![row([("values", list([n(2), n(5)]))])]);
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
    fn row_producing_aggregates_are_independent_of_rule_source_order() {
        let outputs = evaluate_queries(
            r#"
            top_work(h, energy) :=
              (h, energy) = TopK{ k: 2, key: energy :
                (h, energy) :
                work_candidate(h, energy)
              }.

            work_candidate(h, energy) := potential(h, energy).
            potential("low", 1).
            potential("mid", 5).
            potential("high", 9).

            ? top_work(h, energy).
            "#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![
                row([("energy", n(5)), ("h", s("mid"))]),
                row([("energy", n(9)), ("h", s("high"))]),
            ],
        );
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
    fn stored_atoms_wait_for_later_bound_expression_inputs() {
        let program = parse_program("fixture", r"? *pair{next: x + 1}, *pair{n: x}.")
            .expect("program parses");
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
                    ("n", Value::Number(NumberValue::Int(2))),
                    ("next", Value::Number(NumberValue::Int(3))),
                ]),
            ],
        );
        let evaluator = Evaluator::new(analyzed, database);
        let mut rows = evaluator
            .eval_query(&query)
            .expect("query")
            .rows
            .into_iter()
            .map(|row| row.fields)
            .collect::<Vec<_>>();
        rows.sort();
        assert_eq!(rows, vec![row([("x", n(1))]), row([("x", n(2))])]);
    }

    #[test]
    fn derived_atoms_wait_for_later_bound_expression_inputs() {
        let outputs = evaluate_queries(
            r"
            seed(1, 2).
            seed(2, 4).
            binder(1).
            ? seed(x, x + 1), binder(x).
            ",
            Database::default(),
        );

        assert_query_rows(&outputs[0], vec![row([("x", n(1))])]);
    }

    #[test]
    fn aggregates_wait_for_outer_expression_inputs() {
        let outputs = evaluate_queries(
            r#"
            score("a", 5).
            factor(10).
            offset_for_key(10).
            offset_for_pair(1).
            pair(2, "a").
            ? total = Sum{ score + factor : score(h, score) }, factor(factor).
            ? (h, score) = TopK{ k: 1, key: score + offset : (h, score) : score(h, score) },
              offset_for_key(offset).
            ? n = Count{ item : pair(offset + 1, item) }, offset_for_pair(offset).
            "#,
            Database::default(),
        );

        assert_query_rows(
            &outputs[0],
            vec![row([("factor", n(10)), ("total", n(15))])],
        );
        assert_query_rows(
            &outputs[1],
            vec![row([("h", s("a")), ("offset", n(10)), ("score", n(5))])],
        );
        assert_query_rows(&outputs[2], vec![row([("n", n(1)), ("offset", n(1))])]);
    }

    #[test]
    fn query_scoped_fixpoint_skips_unneeded_global_rules() {
        let program = parse_program(
            "fixture",
            r#"
            seed("ok").
            needed(h) := seed(h).
            unused(h) := seed(h).
            ? needed(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator
            .run_fixpoint_for_query(&query)
            .expect("scoped fixpoint evaluates");
        let output = evaluator.eval_query(&query).expect("query evaluates");

        assert_query_rows(
            &[output.rows[0].fields.clone()],
            vec![row([("h", s("ok"))])],
        );
        let unused = PredicateRef::parse("unused").expect("predicate parses");
        assert!(
            evaluator
                .database()
                .derived
                .get(&unused)
                .is_none_or(|relation| relation.tuples().is_empty()),
            "unused rule should not run for query-scoped fixpoint"
        );
    }

    #[test]
    fn query_scoped_fixpoint_includes_global_deps_for_local_rules() {
        let program = parse_program(
            "fixture",
            r#"
            seed("ok").
            base(h) := seed(h).
            unused(h) := seed(h).
            ?
              where local(h) := base(h).
              local(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator
            .run_fixpoint_for_query(&query)
            .expect("scoped fixpoint evaluates");
        let output = evaluator.eval_query(&query).expect("query evaluates");

        assert_query_rows(
            &[output.rows[0].fields.clone()],
            vec![row([("h", s("ok"))])],
        );
        let unused = PredicateRef::parse("unused").expect("predicate parses");
        assert!(
            evaluator
                .database()
                .derived
                .get(&unused)
                .is_none_or(|relation| relation.tuples().is_empty()),
            "unneeded global rule should not run through a local query"
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
    fn query_order_by_desc_sorts_before_projection() {
        let program = parse_program(
            "fixture",
            r#"
            score("a", 1).
            score("b", 3).
            score("c", 2).
            ? score(h, rank) order by rank desc.
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        let handles = output
            .rows
            .iter()
            .map(|row| row.fields.get("h").expect("h"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(handles, vec![s("b"), s("c"), s("a")]);
    }

    #[test]
    fn query_order_by_multi_key_and_stable_tie_break() {
        let input = r#"
            score("a", 1, "z").
            score("b", 1, "x").
            score("c", 2, "y").
            score("d", 2, "w").
            ? score(h, rank, label) order by rank desc, label asc.
            ? score(h, rank, label) order by rank asc.
            ? score(h, rank, label).
        "#;
        let program = parse_program("fixture", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");

        let multi = evaluator.eval_query(&queries[0]).expect("multi-key query");
        let handles = multi
            .rows
            .iter()
            .map(|row| row.fields.get("h").expect("h"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(handles, vec![s("d"), s("c"), s("b"), s("a")]);

        let rank_only = evaluator.eval_query(&queries[1]).expect("rank query");
        let unordered = evaluator.eval_query(&queries[2]).expect("unordered query");
        let tied_rank_one = rank_only
            .rows
            .iter()
            .filter(|row| row.fields.get("rank") == Some(&n(1)))
            .map(|row| row.fields.get("h").expect("h").clone())
            .collect::<Vec<_>>();
        let unordered_rank_one = unordered
            .rows
            .iter()
            .filter(|row| row.fields.get("rank") == Some(&n(1)))
            .map(|row| row.fields.get("h").expect("h").clone())
            .collect::<Vec<_>>();
        assert_eq!(tied_rank_one, unordered_rank_one);
    }

    #[test]
    fn query_order_by_uses_value_ordering() {
        let program = parse_program(
            "fixture",
            r#"
            value(2).
            value("a").
            value(1).
            ? value(v) order by v asc.
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        let values = output
            .rows
            .iter()
            .map(|row| row.fields.get("v").expect("v"))
            .cloned()
            .collect::<Vec<_>>();
        let mut expected = values.clone();
        expected.sort();
        assert_eq!(values, expected);
    }

    #[test]
    fn query_order_by_supports_arithmetic_key_expression() {
        let program = parse_program(
            "fixture",
            r#"
            score("a", 3, 1).
            score("b", 1, 4).
            score("c", 2, 2).
            ? score(h, left, right) order by left + right desc.
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        let handles = output
            .rows
            .iter()
            .map(|row| row.fields.get("h").expect("h"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(handles, vec![s("b"), s("a"), s("c")]);
    }

    #[test]
    fn query_order_by_unbound_key_is_static_error() {
        let program = parse_program(
            "fixture",
            r#"
            score("a", 1).
            ? score(h, rank) order by missing.
            "#,
        )
        .expect("program parses");
        let err = analyze(program).expect_err("program rejects");
        assert!(matches!(
            err,
            StaticError::UnboundExpressionVariable { variable, .. }
                if variable.as_str() == "missing"
        ));
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
    fn relation_pattern_calls_omit_hidden_fields() {
        let output = evaluate_query_output(
            r#"
            @predicate(name: "diagnostic", args: ["code", "severity", "subject", "file", "line", "evidence"]).
            diagnostic("E001", "error", "h1", "a.md", 7, "broken").
            diagnostic("W001", "warning", "h2", "b.md", 9, "stale").
            ? diagnostic{severity: "error", subject: h}.
            "#,
            Database::default(),
        );
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].fields.get("h"), Some(&s("h1")));
        assert_eq!(output.rows[0].fields.len(), 1);
    }

    #[test]
    fn positional_wildcards_do_not_project() {
        let output = evaluate_query_output(
            r#"
            diagnostic("E001", "error", "h1").
            ? diagnostic(_, "error", h).
            "#,
            Database::default(),
        );
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].fields.get("h"), Some(&s("h1")));
        assert_eq!(output.rows[0].fields.len(), 1);
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
