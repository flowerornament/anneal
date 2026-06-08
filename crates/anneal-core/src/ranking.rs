//! Search hit ranking contracts.
//!
//! Adapters and hosts may provide their own rankers, but the public
//! `search(...)` relation always exposes calibrated scores in `[0, 1]`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8Path;

use crate::config_schema::{
    SEARCH_BOOST_HUB_KEY, SEARCH_BOOST_STATUS_ENTRY_PREFIX, parse_search_boost_value,
};
use crate::retrieval::SearchSpanScope;
use crate::source::SearchInfo;

pub const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f32 = 0.5;
const PARENT_CLUSTER_RAW_SCORE_BOOST: f32 = 0.15;
const PARENT_CLUSTER_MIN_CHILD_RAW_SCORE: f32 = 0.70;
const PARENT_CLUSTER_MAX_SCORE: f32 = 0.99;
const HISTORICAL_PATH_SCORE_PENALTY: f32 = 0.08;
const DEFAULT_HUB_EDGE_SCORE_BOOST: f32 = 0.01;
const HUB_SCORE_BOOST_MAX: f32 = 0.12;
const CURRENT_HEAD_SCORE_BOOST: f32 = 0.10;
const SUPERSEDED_SCORE_PENALTY: f32 = 0.12;
const TERM_SPECIFICITY_MAX_FACTOR: f32 = 2.5;
const HANDLE_KIND_LABEL: &str = "label";
const HANDLE_KIND_EXTERNAL: &str = "external";
const HANDLE_KIND_FILE: &str = "file";
const SUPERSEDES_EDGE_KIND: &str = "Supersedes";

pub const FIELD_IDENTIFIER: &str = "identifier";
pub const FIELD_TITLE: &str = "title";
pub const FIELD_HEADING: &str = "heading";
pub const FIELD_BODY: &str = "body";
pub const FIELD_FRONTMATTER_PREFIX: &str = "frontmatter:";
pub const FIELD_FRONTMATTER_GLOB: &str = "frontmatter:*";

pub const REASON_IDENTIFIER_SUBSTRING: &str = "identifier-substring";
pub const REASON_TITLE_SUBSTRING: &str = "title-substring";
pub const REASON_HEADING_SUBSTRING: &str = "heading-substring";
pub const REASON_FRONTMATTER_KEY_MATCH: &str = "frontmatter-key-match";
pub const REASON_FRONTMATTER_VALUE_MATCH: &str = "frontmatter-value-match";
pub const REASON_BODY_SUBSTRING: &str = "body-substring";
pub const REASON_PARENT_CLUSTER: &str = "parent-cluster";

#[must_use]
pub fn default_lexical_search_info() -> SearchInfo {
    SearchInfo {
        reason_vocabulary: vec![
            REASON_IDENTIFIER_SUBSTRING,
            REASON_TITLE_SUBSTRING,
            REASON_HEADING_SUBSTRING,
            REASON_FRONTMATTER_KEY_MATCH,
            REASON_FRONTMATTER_VALUE_MATCH,
            REASON_BODY_SUBSTRING,
            REASON_PARENT_CLUSTER,
        ],
        fields: vec![
            FIELD_IDENTIFIER,
            FIELD_TITLE,
            FIELD_HEADING,
            FIELD_BODY,
            FIELD_FRONTMATTER_GLOB,
        ],
        low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct SearchScore(f32);

impl SearchScore {
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self(if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        })
    }

    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

/// Internal search candidate produced from stored corpus facts.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    corpus: String,
    source: String,
    handle: String,
    span_id: Option<String>,
    raw_score: SearchScore,
    score_boost: SearchScore,
    reason: String,
    field: String,
}

impl SearchHit {
    #[must_use]
    pub fn new(
        corpus: impl Into<String>,
        source: impl Into<String>,
        handle: impl Into<String>,
        span_id: Option<String>,
        raw_score: f32,
        reason: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self {
            corpus: corpus.into(),
            source: source.into(),
            handle: handle.into(),
            span_id,
            raw_score: SearchScore::new(raw_score),
            score_boost: SearchScore::new(0.0),
            reason: reason.into(),
            field: field.into(),
        }
    }

    #[must_use]
    pub fn corpus(&self) -> &str {
        &self.corpus
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }

    #[must_use]
    pub fn span_id(&self) -> Option<&str> {
        self.span_id.as_deref()
    }

    #[must_use]
    pub fn raw_score(&self) -> SearchScore {
        self.raw_score
    }

    #[must_use]
    pub fn score_boost(&self) -> SearchScore {
        self.score_boost
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }
}

/// Query-local ranking inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct RankingContext {
    query: String,
    low_confidence_threshold: SearchScore,
}

impl RankingContext {
    #[must_use]
    pub fn new(query: impl Into<String>, low_confidence_threshold: f32) -> Self {
        Self {
            query: query.into(),
            low_confidence_threshold: SearchScore::new(low_confidence_threshold),
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub fn low_confidence_threshold(&self) -> SearchScore {
        self.low_confidence_threshold
    }
}

/// Search score calibration and tie-break policy.
pub trait Ranker: Send + Sync {
    fn calibrate(&self, hit: &SearchHit, ctx: &RankingContext) -> f32;
    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering;
}

/// Deterministic lexical ranker used when no adapter or project ranker is active.
#[derive(Clone, Debug, Default)]
pub struct DefaultRanker;

impl Ranker for DefaultRanker {
    fn calibrate(&self, hit: &SearchHit, _ctx: &RankingContext) -> f32 {
        SearchScore::new(
            hit.raw_score().get() * field_weight(hit.field()) + hit.score_boost().get()
                - historical_path_penalty(hit.handle()),
        )
        .get()
    }

    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering {
        a.corpus()
            .cmp(b.corpus())
            .then_with(|| a.source().cmp(b.source()))
            .then_with(|| hit_tie_priority(a).cmp(&hit_tie_priority(b)))
            .then_with(|| {
                handle_fragment_priority(a.handle()).cmp(&handle_fragment_priority(b.handle()))
            })
            .then_with(|| a.handle().cmp(b.handle()))
            .then_with(|| a.span_id().cmp(&b.span_id()))
            .then_with(|| a.field().cmp(b.field()))
            .then_with(|| a.reason().cmp(b.reason()))
    }
}

fn hit_tie_priority(hit: &SearchHit) -> u8 {
    match (hit.reason(), hit.field()) {
        (REASON_IDENTIFIER_SUBSTRING, FIELD_IDENTIFIER) | (REASON_TITLE_SUBSTRING, FIELD_TITLE) => {
            0
        }
        (REASON_HEADING_SUBSTRING, FIELD_HEADING) => 1,
        (REASON_PARENT_CLUSTER, FIELD_IDENTIFIER) => 2,
        (REASON_FRONTMATTER_KEY_MATCH | REASON_FRONTMATTER_VALUE_MATCH, _) => 3,
        (REASON_BODY_SUBSTRING, FIELD_BODY) => 4,
        _ => 5,
    }
}

fn handle_fragment_priority(handle: &str) -> u8 {
    u8::from(handle.contains('#'))
}

fn field_weight(field: &str) -> f32 {
    match field {
        FIELD_IDENTIFIER => 1.0,
        FIELD_TITLE => 0.95,
        FIELD_HEADING => 0.90,
        FIELD_BODY => 0.82,
        _ if field.starts_with("frontmatter:") => 0.88,
        _ => 0.75,
    }
}

#[must_use]
pub fn context_sort_score(score: f64, reason: &str, field: &str) -> f64 {
    score + context_reason_bonus(reason) + context_field_bonus(field)
}

pub const CONTEXT_NEIGHBOR_GROUP_CURRENT: &str = "current";
pub const CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT: &str = "in_flight";
pub const CONTEXT_NEIGHBOR_GROUP_SUPERSEDED: &str = "superseded";
pub const CONTEXT_NEIGHBOR_GROUP_HIDDEN: &str = "hidden";

#[must_use]
pub fn context_neighbor_sort_score(
    group: &str,
    disposition: &str,
    degree: i64,
    is_self: bool,
) -> f64 {
    context_neighbor_group_bonus(group)
        + context_neighbor_disposition_bonus(disposition)
        + context_neighbor_self_bonus(is_self)
        - context_neighbor_degree_penalty(degree)
}

fn context_neighbor_group_bonus(group: &str) -> f64 {
    match group {
        CONTEXT_NEIGHBOR_GROUP_CURRENT => 300.0,
        CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT => 200.0,
        CONTEXT_NEIGHBOR_GROUP_SUPERSEDED => 100.0,
        CONTEXT_NEIGHBOR_GROUP_HIDDEN => 0.0,
        _ => 150.0,
    }
}

fn context_neighbor_disposition_bonus(disposition: &str) -> f64 {
    match disposition {
        "current_head" => 35.0,
        "current" => 20.0,
        "superseded" => -40.0,
        _ => 0.0,
    }
}

fn context_neighbor_self_bonus(is_self: bool) -> f64 {
    if is_self { 50.0 } else { 0.0 }
}

fn context_neighbor_degree_penalty(degree: i64) -> f64 {
    let degree = degree.max(0);
    match degree {
        0..=9 => 0.0,
        10..=24 => 8.0,
        25..=49 => 16.0,
        50..=99 => 28.0,
        100..=249 => 40.0,
        _ => 55.0,
    }
}

fn context_reason_bonus(reason: &str) -> f64 {
    match reason {
        REASON_PARENT_CLUSTER => 0.250,
        _ => 0.0,
    }
}

fn context_field_bonus(field: &str) -> f64 {
    match field {
        FIELD_HEADING => 0.040,
        FIELD_BODY => 0.015,
        FIELD_TITLE | FIELD_IDENTIFIER => 0.005,
        field if field.starts_with(FIELD_FRONTMATTER_PREFIX) => 0.002,
        _ => 0.0,
    }
}

fn historical_path_penalty(handle: &str) -> f32 {
    if handle.contains("/history/") || handle.contains("/prior/") {
        HISTORICAL_PATH_SCORE_PENALTY
    } else {
        0.0
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SearchIndex {
    documents: BTreeMap<SearchDocumentKey, SearchDocument>,
    incoming_edge_counts: BTreeMap<SearchDocumentKey, u32>,
    supersedes_from: BTreeSet<SearchDocumentKey>,
    supersedes_to: BTreeSet<SearchDocumentKey>,
    boosts: SearchBoosts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SearchHandleDocument<'a> {
    pub(crate) corpus: &'a str,
    pub(crate) source: &'a str,
    pub(crate) handle: &'a str,
    pub(crate) file: &'a str,
    pub(crate) summary: Option<&'a str>,
    pub(crate) status: Option<&'a str>,
    pub(crate) namespace: Option<&'a str>,
    pub(crate) area: Option<&'a str>,
    pub(crate) kind: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SearchDocumentKey {
    corpus: String,
    source: String,
    handle: String,
}

impl SearchDocumentKey {
    fn new(corpus: &str, source: &str, handle: &str) -> Self {
        Self {
            corpus: corpus.to_owned(),
            source: source.to_owned(),
            handle: handle.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SearchDocument {
    parent_file: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    fields: Vec<SearchField>,
}

#[derive(Clone, Debug, PartialEq)]
struct SearchBoosts {
    status: BTreeMap<String, f32>,
    hub_edge: f32,
}

impl Default for SearchBoosts {
    fn default() -> Self {
        let status = [
            ("authoritative", 0.08),
            ("current", 0.08),
            ("stable", 0.08),
            ("living", 0.08),
            ("published", 0.08),
            ("approved", 0.06),
            ("active", 0.04),
            ("review", 0.04),
            ("research", 0.03),
            ("plan", 0.03),
            ("exploratory", 0.02),
            ("draft", 0.0),
            ("raw", 0.0),
        ]
        .into_iter()
        .map(|(status, boost)| (status.to_string(), boost))
        .collect();
        Self {
            status,
            hub_edge: DEFAULT_HUB_EDGE_SCORE_BOOST,
        }
    }
}

impl SearchBoosts {
    fn status_boost(&self, status: Option<&str>) -> f32 {
        status
            .and_then(|status| self.status.get(status).copied())
            .unwrap_or(0.0)
    }

    fn hub_boost(&self, incoming_edges: u32) -> f32 {
        let bounded_edges = u16::try_from(incoming_edges).unwrap_or(u16::MAX);
        (f32::from(bounded_edges) * self.hub_edge).min(HUB_SCORE_BOOST_MAX)
    }

    fn insert_config(&mut self, key: &str, value: &str) {
        if let Some(status) = key.strip_prefix(SEARCH_BOOST_STATUS_ENTRY_PREFIX) {
            if let Some(boost) = parse_search_boost_value(value) {
                self.status.insert(status.to_ascii_lowercase(), boost);
            }
        } else if key == SEARCH_BOOST_HUB_KEY
            && let Some(boost) = parse_search_boost_value(value)
        {
            self.hub_edge = boost;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SearchField {
    span_id: Option<String>,
    field: String,
    reason: String,
    normalized_text: String,
}

impl SearchField {
    fn new(
        span_id: Option<String>,
        field: impl Into<String>,
        reason: impl Into<String>,
        text: &str,
    ) -> Option<Self> {
        let normalized_text = normalize_search_text(text);
        (!normalized_text.is_empty()).then(|| Self {
            span_id,
            field: field.into(),
            reason: reason.into(),
            normalized_text,
        })
    }
}

impl SearchIndex {
    pub(crate) fn len(&self) -> usize {
        self.documents.len()
    }

    pub(crate) fn insert_handle(&mut self, fields: SearchHandleDocument<'_>) {
        let document = self.document_mut(fields.corpus, fields.source, fields.handle);
        document.parent_file = (fields.file != fields.handle).then(|| fields.file.to_owned());
        document.kind = fields.kind.map(str::to_owned);
        document.status = fields.status.map(str::to_ascii_lowercase);
        push_field(
            document,
            None,
            FIELD_IDENTIFIER,
            REASON_IDENTIFIER_SUBSTRING,
            fields.handle,
        );
        if let Some(summary) = fields.summary {
            push_field(document, None, FIELD_TITLE, REASON_TITLE_SUBSTRING, summary);
        }
        for (field, value) in [
            ("frontmatter:status", fields.status),
            ("frontmatter:namespace", fields.namespace),
            ("frontmatter:area", fields.area),
            ("frontmatter:kind", fields.kind),
        ] {
            if let Some(value) = value {
                push_field(document, None, field, REASON_FRONTMATTER_VALUE_MATCH, value);
            }
        }
    }

    pub(crate) fn insert_config(&mut self, key: &str, value: &str) {
        self.boosts.insert_config(key, value);
    }

    pub(crate) fn insert_meta(
        &mut self,
        corpus: &str,
        source: &str,
        handle: &str,
        key: &str,
        value: &str,
    ) {
        let field = frontmatter_field(key);
        let document = self.document_mut(corpus, source, handle);
        push_field(
            document,
            None,
            field.as_str(),
            REASON_FRONTMATTER_KEY_MATCH,
            key,
        );
        push_field(
            document,
            None,
            field.as_str(),
            REASON_FRONTMATTER_VALUE_MATCH,
            value,
        );
    }

    pub(crate) fn insert_content(
        &mut self,
        corpus: &str,
        source: &str,
        handle: &str,
        span_id: &str,
        text: &str,
    ) {
        let document = self.document_mut(corpus, source, handle);
        push_field(
            document,
            Some(span_id.to_owned()),
            FIELD_BODY,
            REASON_BODY_SUBSTRING,
            text,
        );
    }

    pub(crate) fn insert_span_summary(
        &mut self,
        corpus: &str,
        source: &str,
        handle: &str,
        span_id: &str,
        summary: &str,
    ) {
        if !is_heading_span_id(span_id) {
            return;
        }
        let document = self.document_mut(corpus, source, handle);
        push_field(
            document,
            Some(span_id.to_owned()),
            FIELD_HEADING,
            REASON_HEADING_SUBSTRING,
            summary,
        );
    }

    pub(crate) fn search_hits(
        &self,
        query: &SearchQuery,
        handle: Option<&str>,
        span_filter: SearchSpanScope<'_>,
        reason_filter: Option<&str>,
        field_filter: Option<&str>,
    ) -> Vec<SearchHit> {
        let cluster_only = reason_filter == Some(REASON_PARENT_CLUSTER);
        if cluster_only
            && (!span_filter.accepts(None)
                || field_filter.is_some_and(|field| field != FIELD_IDENTIFIER))
        {
            return Vec::new();
        }
        let base_reason_filter = if cluster_only { None } else { reason_filter };
        let base_field_filter = if cluster_only { None } else { field_filter };
        let term_specificity = self.term_specificity(query);
        let mut hits = self
            .documents
            .iter()
            .filter(|(key, document)| match handle {
                Some(handle) if cluster_only || base_reason_filter.is_none() => {
                    key.handle == handle || document.parent_file.as_deref() == Some(handle)
                }
                Some(handle) => key.handle == handle,
                None => true,
            })
            .flat_map(|(key, document)| {
                document.search_hits(
                    key,
                    query,
                    span_filter,
                    base_reason_filter,
                    base_field_filter,
                    &term_specificity,
                )
            })
            .collect::<Vec<_>>();
        if handle.is_none() || cluster_only || base_reason_filter.is_none() {
            self.push_parent_cluster_hits(
                &mut hits,
                query,
                span_filter,
                reason_filter,
                field_filter,
                &term_specificity,
            );
        }
        if let Some(handle) = handle {
            hits.retain(|hit| hit.handle() == handle);
        }
        if cluster_only {
            hits.retain(|hit| hit.reason() == REASON_PARENT_CLUSTER);
        }
        self.apply_ranker_boosts(&mut hits);
        hits
    }

    pub(crate) fn insert_edge(
        &mut self,
        corpus: &str,
        source: &str,
        from: &str,
        to: &str,
        kind: &str,
    ) {
        *self
            .incoming_edge_counts
            .entry(SearchDocumentKey::new(corpus, source, to))
            .or_default() += 1;
        if kind == SUPERSEDES_EDGE_KIND {
            self.supersedes_from
                .insert(SearchDocumentKey::new(corpus, source, from));
            self.supersedes_to
                .insert(SearchDocumentKey::new(corpus, source, to));
        }
    }

    fn document_mut(&mut self, corpus: &str, source: &str, handle: &str) -> &mut SearchDocument {
        self.documents
            .entry(SearchDocumentKey::new(corpus, source, handle))
            .or_default()
    }

    fn push_parent_cluster_hits(
        &self,
        hits: &mut Vec<SearchHit>,
        query: &SearchQuery,
        span_filter: SearchSpanScope<'_>,
        reason_filter: Option<&str>,
        field_filter: Option<&str>,
        term_specificity: &TermSpecificity,
    ) {
        if !span_filter.accepts(None)
            || reason_filter.is_some_and(|reason| reason != REASON_PARENT_CLUSTER)
            || field_filter.is_some_and(|field| field != FIELD_IDENTIFIER)
        {
            return;
        }

        let mut clusters = BTreeMap::<SearchDocumentKey, ParentCluster>::new();
        for hit in hits.iter() {
            let child_key = SearchDocumentKey::new(hit.corpus(), hit.source(), hit.handle());
            let Some(child) = self.documents.get(&child_key) else {
                continue;
            };
            let Some(parent_handle) = child.parent_file.as_deref() else {
                continue;
            };
            let parent_key = SearchDocumentKey::new(hit.corpus(), hit.source(), parent_handle);
            if !self.documents.contains_key(&parent_key) {
                continue;
            }
            clusters
                .entry(parent_key)
                .or_default()
                .record_child(hit.handle(), hit.raw_score().get());
        }

        hits.extend(
            clusters
                .into_iter()
                .filter(|(parent_key, cluster)| {
                    cluster.is_actionable()
                        && !is_label_inventory_handle(parent_key.handle.as_str())
                })
                .filter_map(|(parent_key, cluster)| {
                    let cluster_score = cluster.cluster_score(query);
                    let direct_signal =
                        self.direct_parent_signal(&parent_key, query, term_specificity);
                    (cluster.best_raw_score > direct_signal.structural_raw_score
                        && cluster_score > direct_signal.calibrated_score)
                        .then(|| {
                            SearchHit::new(
                                parent_key.corpus,
                                parent_key.source,
                                parent_key.handle,
                                None,
                                cluster_score,
                                REASON_PARENT_CLUSTER,
                                FIELD_IDENTIFIER,
                            )
                        })
                }),
        );
    }

    fn direct_parent_signal(
        &self,
        parent_key: &SearchDocumentKey,
        query: &SearchQuery,
        term_specificity: &TermSpecificity,
    ) -> ParentDirectSignal {
        let Some(parent) = self.documents.get(parent_key) else {
            return ParentDirectSignal::default();
        };
        let ctx = RankingContext::new(query.original(), DEFAULT_LOW_CONFIDENCE_THRESHOLD);
        parent
            .search_hits(
                parent_key,
                query,
                SearchSpanScope::Any,
                None,
                None,
                term_specificity,
            )
            .fold(ParentDirectSignal::default(), |mut signal, hit| {
                if matches!(hit.field(), FIELD_IDENTIFIER | FIELD_TITLE) {
                    signal.structural_raw_score =
                        signal.structural_raw_score.max(hit.raw_score().get());
                }
                signal.calibrated_score = signal
                    .calibrated_score
                    .max(self.calibrate_direct_hit(&hit, &ctx));
                signal
            })
    }

    fn calibrate_direct_hit(&self, hit: &SearchHit, ctx: &RankingContext) -> f32 {
        let key = SearchDocumentKey::new(hit.corpus(), hit.source(), hit.handle());
        let mut boosted = hit.clone();
        boosted.score_boost =
            SearchScore::new(boosted.score_boost().get() + self.ranker_boost_for_key(&key));
        DefaultRanker.calibrate(&boosted, ctx)
    }

    fn apply_ranker_boosts(&self, hits: &mut [SearchHit]) {
        for hit in hits {
            if hit.reason() == REASON_PARENT_CLUSTER {
                continue;
            }
            let key = SearchDocumentKey::new(hit.corpus(), hit.source(), hit.handle());
            hit.score_boost =
                SearchScore::new(hit.score_boost.get() + self.ranker_boost_for_key(&key));
        }
    }

    fn ranker_boost_for_key(&self, key: &SearchDocumentKey) -> f32 {
        let count = self.incoming_edge_counts.get(key).copied().unwrap_or(0);
        let status = self.documents.get(key).and_then(boosted_status);
        self.boosts.status_boost(status)
            + self.boosts.hub_boost(count)
            + self.currency_boost_for_key(key)
    }

    fn currency_boost_for_key(&self, key: &SearchDocumentKey) -> f32 {
        match self.currency_disposition_for_key(key) {
            Some(CurrencyDisposition::CurrentHead) if self.is_operative_key(key) => {
                CURRENT_HEAD_SCORE_BOOST
            }
            Some(CurrencyDisposition::Superseded) => -SUPERSEDED_SCORE_PENALTY,
            Some(CurrencyDisposition::CurrentHead | CurrencyDisposition::Current) | None => 0.0,
        }
    }

    fn currency_disposition_for_key(&self, key: &SearchDocumentKey) -> Option<CurrencyDisposition> {
        let document = self.documents.get(key)?;
        if document.kind.as_deref() != Some(HANDLE_KIND_FILE) {
            return None;
        }
        if self.supersedes_from.contains(key) {
            return Some(CurrencyDisposition::Superseded);
        }
        if self.supersedes_to.contains(key) {
            Some(CurrencyDisposition::CurrentHead)
        } else {
            Some(CurrencyDisposition::Current)
        }
    }

    fn is_operative_key(&self, key: &SearchDocumentKey) -> bool {
        self.documents
            .get(key)
            .and_then(|document| document.status.as_deref())
            .is_some_and(is_operative_status)
    }

    fn term_specificity(&self, query: &SearchQuery) -> TermSpecificity {
        let field_count = self
            .documents
            .values()
            .map(|document| document.fields.len())
            .sum::<usize>();
        if field_count == 0 {
            return TermSpecificity::uniform(query);
        }

        let mut field_frequency = BTreeMap::<&str, usize>::new();
        for term in &query.terms {
            let count = self
                .documents
                .values()
                .flat_map(|document| &document.fields)
                .filter(|field| term_matches(field.normalized_text.as_str(), term))
                .count();
            field_frequency.insert(term.value.as_str(), count);
        }
        TermSpecificity::from_frequency(query, field_count, &field_frequency)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CurrencyDisposition {
    Current,
    CurrentHead,
    Superseded,
}

fn is_operative_status(status: &str) -> bool {
    matches!(
        status,
        "authoritative" | "current" | "active" | "stable" | "living"
    )
}

fn boosted_status(document: &SearchDocument) -> Option<&str> {
    match document.kind.as_deref() {
        Some(HANDLE_KIND_LABEL | HANDLE_KIND_EXTERNAL) => None,
        _ => document.status.as_deref(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ParentDirectSignal {
    structural_raw_score: f32,
    calibrated_score: f32,
}

#[derive(Clone, Debug, Default)]
struct ParentCluster {
    children: BTreeSet<String>,
    best_raw_score: f32,
}

impl ParentCluster {
    fn record_child(&mut self, child: &str, raw_score: f32) {
        self.children.insert(child.to_owned());
        self.best_raw_score = self.best_raw_score.max(raw_score);
    }

    fn is_actionable(&self) -> bool {
        self.children.len() >= 2 && self.best_raw_score >= PARENT_CLUSTER_MIN_CHILD_RAW_SCORE
    }

    fn cluster_score(&self, query: &SearchQuery) -> f32 {
        let hit = SearchHit::new(
            "",
            "",
            "",
            None,
            self.best_raw_score + PARENT_CLUSTER_RAW_SCORE_BOOST,
            REASON_PARENT_CLUSTER,
            FIELD_IDENTIFIER,
        );
        let ctx = RankingContext::new(query.original(), DEFAULT_LOW_CONFIDENCE_THRESHOLD);
        DefaultRanker
            .calibrate(&hit, &ctx)
            .min(PARENT_CLUSTER_MAX_SCORE)
    }
}

fn is_label_inventory_handle(handle: &str) -> bool {
    Utf8Path::new(handle)
        .file_name()
        .is_some_and(|file_name| file_name.eq_ignore_ascii_case("labels.md"))
}

impl SearchDocument {
    fn search_hits<'a>(
        &'a self,
        key: &'a SearchDocumentKey,
        query: &'a SearchQuery,
        span_filter: SearchSpanScope<'a>,
        reason_filter: Option<&'a str>,
        field_filter: Option<&'a str>,
        term_specificity: &'a TermSpecificity,
    ) -> impl Iterator<Item = SearchHit> + 'a {
        let should_hide_full_body_span = matches!(span_filter, SearchSpanScope::Any)
            && reason_filter.is_none_or(|reason| reason == REASON_BODY_SUBSTRING)
            && field_filter.is_none_or(|field_name| field_name == FIELD_BODY)
            && self.fields.iter().any(|field| {
                field.field == FIELD_BODY
                    && field
                        .span_id
                        .as_deref()
                        .is_some_and(|span_id| !is_full_span_id(key.handle.as_str(), span_id))
            });
        self.fields
            .iter()
            .filter(move |field| {
                !should_hide_full_body_span
                    || field.field != FIELD_BODY
                    || !field
                        .span_id
                        .as_deref()
                        .is_some_and(|span_id| is_full_span_id(key.handle.as_str(), span_id))
            })
            .filter(move |field| span_filter.accepts(field.span_id.as_deref()))
            .filter(move |field| reason_filter.is_none_or(|reason| field.reason == reason))
            .filter(move |field| field_filter.is_none_or(|field_name| field.field == field_name))
            .filter_map(move |field| {
                query
                    .score_normalized_with_specificity(&field.normalized_text, term_specificity)
                    .map(|score| {
                        SearchHit::new(
                            key.corpus.as_str(),
                            key.source.as_str(),
                            key.handle.as_str(),
                            field.span_id.clone(),
                            score,
                            field.reason.clone(),
                            field.field.clone(),
                        )
                    })
            })
    }
}

fn is_full_span_id(handle: &str, span_id: &str) -> bool {
    span_id
        .strip_prefix(handle)
        .is_some_and(|suffix| suffix == "#full")
}

fn is_heading_span_id(span_id: &str) -> bool {
    span_id.contains("#h/")
}

fn push_field(
    document: &mut SearchDocument,
    span_id: Option<String>,
    field: &str,
    reason: &str,
    text: &str,
) {
    if let Some(field) = SearchField::new(span_id, field, reason, text) {
        document.fields.push(field);
    }
}

fn frontmatter_field(key: &str) -> String {
    format!("frontmatter:{key}")
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchQuery {
    original: String,
    normalized: String,
    terms: Vec<QueryTerm>,
}

#[derive(Clone, Debug, PartialEq)]
struct QueryTerm {
    value: String,
    weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct TermSpecificity {
    weights: BTreeMap<String, f32>,
    total_weight: f32,
}

impl TermSpecificity {
    fn uniform(query: &SearchQuery) -> Self {
        let weights = query
            .terms
            .iter()
            .map(|term| (term.value.clone(), term.weight))
            .collect::<BTreeMap<_, _>>();
        let total_weight = weights.values().sum::<f32>().max(1.0);
        Self {
            weights,
            total_weight,
        }
    }

    fn from_frequency(
        query: &SearchQuery,
        field_count: usize,
        document_frequency: &BTreeMap<&str, usize>,
    ) -> Self {
        let field_count = f32::from(u16::try_from(field_count).unwrap_or(u16::MAX)).max(1.0);
        let weights = query
            .terms
            .iter()
            .map(|term| {
                let frequency = document_frequency
                    .get(term.value.as_str())
                    .copied()
                    .unwrap_or(0);
                let frequency = f32::from(u16::try_from(frequency).unwrap_or(u16::MAX));
                let rarity = ((field_count - frequency).max(0.0) / field_count).clamp(0.0, 1.0);
                let factor = 1.0 + (rarity * (TERM_SPECIFICITY_MAX_FACTOR - 1.0));
                (term.value.clone(), term.weight * factor)
            })
            .collect::<BTreeMap<_, _>>();
        let total_weight = weights.values().sum::<f32>().max(1.0);
        Self {
            weights,
            total_weight,
        }
    }

    fn weight_for(&self, term: &QueryTerm) -> f32 {
        self.weights
            .get(term.value.as_str())
            .copied()
            .unwrap_or(term.weight)
    }
}

impl SearchQuery {
    pub(crate) fn parse(query: &str) -> Option<Self> {
        let original_terms = search_tokens(query);
        let normalized = original_terms.join(" ");
        if normalized.is_empty() {
            return None;
        }
        let terms = expanded_query_terms(&original_terms);
        Some(Self {
            original: query.to_owned(),
            normalized,
            terms,
        })
    }

    fn original(&self) -> &str {
        &self.original
    }

    #[cfg(test)]
    fn score_normalized(&self, normalized_text: &str) -> Option<f32> {
        let specificity = TermSpecificity::uniform(self);
        self.score_normalized_with_specificity(normalized_text, &specificity)
    }

    fn score_normalized_with_specificity(
        &self,
        normalized_text: &str,
        specificity: &TermSpecificity,
    ) -> Option<f32> {
        if normalized_text.is_empty() {
            return None;
        }
        if normalized_text.contains(&self.normalized) {
            return Some(1.0);
        }
        let matched_weight = self
            .terms
            .iter()
            .filter(|term| term_matches(normalized_text, term))
            .map(|term| specificity.weight_for(term))
            .sum::<f32>();
        (matched_weight > 0.0)
            .then(|| 0.35 + (0.55 * (matched_weight / specificity.total_weight).min(1.0)))
    }
}

fn term_matches(normalized_text: &str, term: &QueryTerm) -> bool {
    normalized_text.contains(term.value.as_str())
}

fn normalize_search_text(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut token = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            token.push(ch);
        } else {
            push_normalized_token(&mut normalized, &mut token);
        }
    }
    push_normalized_token(&mut normalized, &mut token);
    normalized
}

fn search_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            token.push(ch);
        } else {
            push_search_token(&mut tokens, &mut token);
        }
    }
    push_search_token(&mut tokens, &mut token);
    tokens
}

fn push_normalized_token(normalized: &mut String, token: &mut String) {
    canonicalize_search_token(token);
    if token.is_empty() {
        return;
    }
    if !normalized.is_empty() {
        normalized.push(' ');
    }
    normalized.push_str(token);
    token.clear();
}

fn push_search_token(tokens: &mut Vec<String>, token: &mut String) {
    canonicalize_search_token(token);
    if !token.is_empty() {
        tokens.push(std::mem::take(token));
    }
    token.clear();
}

fn canonical_search_token(token: &str) -> String {
    let mut stem = token.to_owned();
    canonicalize_search_token(&mut stem);
    stem
}

fn canonicalize_search_token(stem: &mut String) {
    if stem.len() > 6 && stem.ends_with("ation") {
        stem.truncate(stem.len() - "ation".len());
    } else if stem.len() > 5 && stem.ends_with("ing") {
        stem.truncate(stem.len() - "ing".len());
        trim_doubled_final_consonant(stem);
    } else if stem.len() > 4 && stem.ends_with("ies") {
        stem.truncate(stem.len() - "ies".len());
        stem.push('y');
    } else if stem.len() > 4 && stem.ends_with("ed") {
        stem.truncate(stem.len() - "ed".len());
        trim_doubled_final_consonant(stem);
    } else if stem.len() > 3 && stem.ends_with('s') {
        stem.pop();
    }
    if stem.len() > 6 && stem.ends_with("ure") {
        stem.pop();
    }
}

fn trim_doubled_final_consonant(value: &mut String) {
    let mut chars = value.chars().rev();
    let Some(last) = chars.next() else {
        return;
    };
    let Some(previous) = chars.next() else {
        return;
    };
    if last == previous && !matches!(last, 'a' | 'e' | 'i' | 'o' | 'u') {
        value.pop();
    }
}

fn expanded_query_terms(original_terms: &[String]) -> Vec<QueryTerm> {
    let mut weights = BTreeMap::<String, f32>::new();
    for term in original_terms {
        insert_term_weight(&mut weights, term, 1.0);
        for (expanded, weight) in abbreviation_expansions(term) {
            insert_term_weight(&mut weights, expanded, *weight);
        }
    }
    for window in original_terms.windows(2) {
        insert_term_weight(&mut weights, &window.join(" "), 2.0);
    }
    for window in original_terms.windows(3) {
        insert_term_weight(&mut weights, &window.join(" "), 3.0);
    }
    for window in original_terms.windows(2) {
        if window[0] == "open" && window[1] == "question" {
            insert_term_weight(&mut weights, "oq", 2.0);
        }
    }
    for window in original_terms.windows(3) {
        if window[0] == "architecture" && window[1] == "decision" && window[2] == "record" {
            insert_term_weight(&mut weights, "adr", 3.0);
        } else if window[0] == "request" && window[1] == "for" && window[2] == "comment" {
            insert_term_weight(&mut weights, "rfc", 3.0);
        }
    }
    weights
        .into_iter()
        .map(|(value, weight)| QueryTerm { value, weight })
        .collect()
}

fn insert_term_weight(weights: &mut BTreeMap<String, f32>, term: &str, weight: f32) {
    let term = canonical_search_token(term);
    weights
        .entry(term)
        .and_modify(|existing| *existing = existing.max(weight))
        .or_insert(weight);
}

fn abbreviation_expansions(term: &str) -> &'static [(&'static str, f32)] {
    match term {
        "oq" => &[("open", 0.25), ("question", 0.25)],
        "adr" => &[("architecture", 0.2), ("decision", 0.2), ("record", 0.2)],
        "rfc" => &[("request", 0.2), ("comment", 0.2)],
        _ => &[],
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RankedSearchHit {
    hit: SearchHit,
    score: SearchScore,
    low_confidence: bool,
}

impl RankedSearchHit {
    pub(crate) fn hit(&self) -> &SearchHit {
        &self.hit
    }

    pub(crate) fn score(&self) -> SearchScore {
        self.score
    }

    pub(crate) fn low_confidence(&self) -> bool {
        self.low_confidence
    }
}

pub(crate) fn rank_search_hits(
    hits: impl IntoIterator<Item = SearchHit>,
    ctx: &RankingContext,
    ranker: &dyn Ranker,
) -> Vec<RankedSearchHit> {
    let mut ranked = hits
        .into_iter()
        .map(|hit| rank_search_hit(hit, ctx, ranker))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| compare_ranked_search_hits(left, right, ranker));
    ranked
}

fn rank_search_hit(hit: SearchHit, ctx: &RankingContext, ranker: &dyn Ranker) -> RankedSearchHit {
    let score = SearchScore::new(ranker.calibrate(&hit, ctx));
    RankedSearchHit {
        low_confidence: score.get() < ctx.low_confidence_threshold().get(),
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
        .get()
        .total_cmp(&left.score.get())
        .then_with(|| ranker.tie_break(&left.hit, &right.hit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_sort_score_applies_reason_and_field_policy() {
        let base = context_sort_score(0.8, REASON_BODY_SUBSTRING, FIELD_BODY);
        let heading = context_sort_score(0.8, REASON_HEADING_SUBSTRING, FIELD_HEADING);
        let clustered = context_sort_score(0.8, REASON_PARENT_CLUSTER, FIELD_IDENTIFIER);
        let frontmatter =
            context_sort_score(0.8, REASON_FRONTMATTER_VALUE_MATCH, "frontmatter:status");

        assert!(
            clustered > heading && heading > base && base > frontmatter,
            "context ranking should prefer clustered canonical hits, then headings, then body, then frontmatter"
        );
    }

    #[test]
    fn clustered_child_hits_promote_canonical_parent_file() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "Reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/canonical.md",
            "milestone chain beta",
        );
        insert_handle(
            &mut index,
            "MCD-3",
            "docs/other.md",
            "milestone chain gamma",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("milestone chain", 0.5),
            &DefaultRanker,
        );

        let first = ranked.first().expect("ranked hit");
        assert_eq!(first.hit().handle(), "docs/canonical.md");
        assert_eq!(first.hit().reason(), REASON_PARENT_CLUSTER);
        assert!(ranked.iter().any(|hit| hit.hit().handle() == "MCD-1"));
        assert!(ranked.iter().any(|hit| hit.hit().handle() == "MCD-2"));
        assert!(
            !ranked
                .iter()
                .any(|hit| hit.hit().handle() == "docs/other.md")
        );
    }

    #[test]
    fn single_child_hit_does_not_promote_parent_file() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "Reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);

        assert!(!hits.iter().any(
            |hit| hit.handle() == "docs/canonical.md" && hit.reason() == REASON_PARENT_CLUSTER
        ));
    }

    #[test]
    fn parent_cluster_reason_filter_returns_synthesized_hits() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "Reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/canonical.md",
            "milestone chain beta",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(
            &query,
            None,
            SearchSpanScope::Any,
            Some(REASON_PARENT_CLUSTER),
            None,
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].handle(), "docs/canonical.md");
        assert_eq!(hits[0].reason(), REASON_PARENT_CLUSTER);
    }

    #[test]
    fn parent_cluster_reason_filter_honors_parent_handle_constraint() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "Reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/canonical.md",
            "milestone chain beta",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(
            &query,
            Some("docs/canonical.md"),
            SearchSpanScope::Any,
            Some(REASON_PARENT_CLUSTER),
            Some(FIELD_IDENTIFIER),
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].handle(), "docs/canonical.md");
        assert_eq!(hits[0].reason(), REASON_PARENT_CLUSTER);
    }

    #[test]
    fn parent_cluster_reason_filter_returns_empty_for_incompatible_field() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "Reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/canonical.md",
            "milestone chain beta",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(
            &query,
            None,
            SearchSpanScope::Any,
            Some(REASON_PARENT_CLUSTER),
            Some(FIELD_BODY),
        );

        assert!(hits.is_empty());
    }

    #[test]
    fn parent_cluster_does_not_duplicate_stronger_direct_parent_hit() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/milestone-chain.md",
            "docs/milestone-chain.md",
            "milestone chain reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/milestone-chain.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/milestone-chain.md",
            "milestone chain beta",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let parent_cluster_hits = hits
            .iter()
            .filter(|hit| {
                hit.handle() == "docs/milestone-chain.md" && hit.reason() == REASON_PARENT_CLUSTER
            })
            .collect::<Vec<_>>();

        assert!(parent_cluster_hits.is_empty());
    }

    #[test]
    fn parent_cluster_does_not_duplicate_exact_parent_title_hit() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/canonical.md",
            "docs/canonical.md",
            "milestone chain reference",
        );
        insert_handle(
            &mut index,
            "MCD-1",
            "docs/canonical.md",
            "milestone chain alpha",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "docs/canonical.md",
            "milestone chain beta",
        );

        let query = SearchQuery::parse("milestone chain").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);

        assert!(!hits.iter().any(
            |hit| hit.handle() == "docs/canonical.md" && hit.reason() == REASON_PARENT_CLUSTER
        ));
    }

    #[test]
    fn weak_repeated_child_hits_do_not_promote_parent_file() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/code-review.md",
            "docs/code-review.md",
            "Code review",
        );
        insert_handle(
            &mut index,
            "docs/code-review.md#medium-type-system-finding",
            "docs/code-review.md",
            "medium type system finding",
        );
        insert_handle(
            &mut index,
            "docs/code-review.md#medium-performance-finding",
            "docs/code-review.md",
            "medium performance finding",
        );

        let query =
            SearchQuery::parse("medium content decomposition milestones").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);

        assert!(
            !hits.iter().any(|hit| hit.handle() == "docs/code-review.md"
                && hit.reason() == REASON_PARENT_CLUSTER)
        );
    }

    #[test]
    fn direct_structural_hit_beats_equal_parent_cluster_tie() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/codebase-review.md",
            "docs/codebase-review.md",
            "Codebase review",
        );
        insert_handle(
            &mut index,
            "docs/codebase-review.md#formal-model-v17-conformance-audit",
            "docs/codebase-review.md",
            "Formal model v17 conformance audit",
        );
        insert_handle(
            &mut index,
            "docs/codebase-review.md#formal-model-v17-conformance-audit-followup",
            "docs/codebase-review.md",
            "Formal model v17 conformance audit followup",
        );
        insert_handle(
            &mut index,
            "docs/formal-model-v17-conformance-audit.md",
            "docs/formal-model-v17-conformance-audit.md",
            "Formal model v17 conformance audit",
        );

        let query = SearchQuery::parse("formal model v17 conformance audit").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("formal model v17 conformance audit", 0.5),
            &DefaultRanker,
        );

        let first = ranked.first().expect("ranked hit").hit();
        assert_eq!(first.handle(), "docs/formal-model-v17-conformance-audit.md");
        assert_eq!(first.reason(), REASON_IDENTIFIER_SUBSTRING);
    }

    #[test]
    fn label_inventory_files_do_not_receive_parent_cluster_hits() {
        let mut index = SearchIndex::default();
        insert_handle(&mut index, "LABELS.md", "LABELS.md", "Label inventory");
        insert_handle(
            &mut index,
            "MCD-1",
            "LABELS.md",
            "medium content decomposition milestone one",
        );
        insert_handle(
            &mut index,
            "MCD-2",
            "LABELS.md",
            "medium content decomposition milestone two",
        );

        let query =
            SearchQuery::parse("medium content decomposition milestones").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);

        assert!(
            !hits
                .iter()
                .any(|hit| hit.handle() == "LABELS.md" && hit.reason() == REASON_PARENT_CLUSTER)
        );
    }

    #[test]
    fn query_expansion_connects_open_question_to_oq_namespace() {
        let mut index = SearchIndex::default();
        index.insert_handle(SearchHandleDocument {
            corpus: "test",
            source: "fixture",
            handle: "OQ-42",
            file: "design.md",
            summary: Some("Runtime question"),
            status: Some("open"),
            namespace: Some("OQ"),
            area: Some("runtime"),
            kind: Some("label"),
        });

        let query = SearchQuery::parse("open questions").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("open questions", 0.5),
            &DefaultRanker,
        );

        let hit = ranked.first().expect("expanded query finds OQ");
        assert_eq!(hit.hit().handle(), "OQ-42");
        assert!(!hit.low_confidence());
    }

    #[test]
    fn inbound_edge_authority_boosts_canonical_source_above_tied_history() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "formal-model/history/sample-formal-model-v16.md",
            "formal-model/history/sample-formal-model-v16.md",
            "Formal model v16",
        );
        insert_handle(
            &mut index,
            "formal-model/sample-formal-model-v17.md",
            "formal-model/sample-formal-model-v17.md",
            "Formal model v17",
        );
        index.insert_content(
            "test",
            "fixture",
            "formal-model/history/sample-formal-model-v16.md",
            "body",
            "block boundary precedence rule",
        );
        index.insert_content(
            "test",
            "fixture",
            "formal-model/sample-formal-model-v17.md",
            "body",
            "block boundary precedence rule",
        );
        for idx in 0..12 {
            index.insert_edge(
                "test",
                "fixture",
                &format!("ref-{idx}.md"),
                "formal-model/sample-formal-model-v17.md",
                "Cites",
            );
        }

        let query = SearchQuery::parse("block boundary precedence rule").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("block boundary precedence rule", 0.5),
            &DefaultRanker,
        );

        assert_eq!(
            ranked.first().expect("ranked hit").hit().handle(),
            "formal-model/sample-formal-model-v17.md"
        );
    }

    #[test]
    fn status_boost_ranks_authoritative_match_above_draft_match() {
        let mut index = SearchIndex::default();
        insert_handle_with_status(
            &mut index,
            "draft.md",
            "draft.md",
            "Draft note",
            Some("draft"),
        );
        insert_handle_with_status(
            &mut index,
            "authority.md",
            "authority.md",
            "Authoritative note",
            Some("authoritative"),
        );
        index.insert_content(
            "test",
            "fixture",
            "draft.md",
            "draft.md#h/protocol",
            "lease protocol",
        );
        index.insert_content(
            "test",
            "fixture",
            "authority.md",
            "authority.md#h/protocol",
            "lease protocol",
        );

        let query = SearchQuery::parse("lease protocol").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("lease protocol", 0.5),
            &DefaultRanker,
        );

        assert_eq!(
            ranked.first().expect("ranked hit").hit().handle(),
            "authority.md"
        );
    }

    #[test]
    fn search_boost_config_overrides_status_and_hub_defaults() {
        let mut index = SearchIndex::default();
        index.insert_config("search_boost.status.draft", "0.09");
        index.insert_config("search_boost.status.authoritative", "0");
        index.insert_config("search_boost.hub", "0");
        insert_handle_with_status(
            &mut index,
            "draft.md",
            "draft.md",
            "Draft note",
            Some("draft"),
        );
        insert_handle_with_status(
            &mut index,
            "authority.md",
            "authority.md",
            "Authoritative note",
            Some("authoritative"),
        );
        index.insert_content(
            "test",
            "fixture",
            "draft.md",
            "draft.md#h/protocol",
            "lease protocol",
        );
        index.insert_content(
            "test",
            "fixture",
            "authority.md",
            "authority.md#h/protocol",
            "lease protocol",
        );

        let query = SearchQuery::parse("lease protocol").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("lease protocol", 0.5),
            &DefaultRanker,
        );

        assert_eq!(
            ranked.first().expect("ranked hit").hit().handle(),
            "draft.md"
        );
    }

    #[test]
    fn currency_boost_prefers_current_head_without_filtering_history() {
        let mut index = SearchIndex::default();
        insert_handle_with_status(
            &mut index,
            "perf/2026-05-30.md",
            "perf/2026-05-30.md",
            "Parametric performance",
            Some("active"),
        );
        insert_handle_with_status(
            &mut index,
            "perf/2026-05-31.md",
            "perf/2026-05-31.md",
            "Parametric performance",
            Some("active"),
        );
        index.insert_content(
            "test",
            "fixture",
            "perf/2026-05-30.md",
            "body",
            "program space parametric performance",
        );
        index.insert_content(
            "test",
            "fixture",
            "perf/2026-05-31.md",
            "body",
            "program space parametric performance",
        );
        index.insert_edge(
            "test",
            "fixture",
            "perf/2026-05-30.md",
            "perf/2026-05-31.md",
            SUPERSEDES_EDGE_KIND,
        );

        let query =
            SearchQuery::parse("program space parametric performance").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let ranked = rank_search_hits(
            hits,
            &RankingContext::new("program space parametric performance", 0.5),
            &DefaultRanker,
        );

        assert_eq!(
            ranked.first().expect("ranked hit").hit().handle(),
            "perf/2026-05-31.md"
        );
        assert!(
            ranked
                .iter()
                .any(|hit| hit.hit().handle() == "perf/2026-05-30.md"),
            "superseded material stays reachable"
        );
    }

    #[test]
    fn heading_summaries_search_as_span_hits() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/protocol.md",
            "docs/protocol.md",
            "Protocol",
        );
        index.insert_span_summary(
            "test",
            "fixture",
            "docs/protocol.md",
            "docs/protocol.md#h/lease-protocol",
            "Lease Protocol",
        );

        let query = SearchQuery::parse("lease protocol").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, None);
        let heading_hit = hits
            .iter()
            .find(|hit| hit.field() == FIELD_HEADING)
            .expect("heading hit is indexed");

        assert_eq!(heading_hit.handle(), "docs/protocol.md");
        assert_eq!(
            heading_hit.span_id(),
            Some("docs/protocol.md#h/lease-protocol")
        );
        assert_eq!(heading_hit.reason(), REASON_HEADING_SUBSTRING);
    }

    #[test]
    fn full_body_span_is_hidden_when_heading_body_spans_exist() {
        let mut index = SearchIndex::default();
        insert_handle(
            &mut index,
            "docs/protocol.md",
            "docs/protocol.md",
            "Protocol",
        );
        index.insert_content(
            "test",
            "fixture",
            "docs/protocol.md",
            "docs/protocol.md#full",
            "needle term in whole document",
        );
        index.insert_content(
            "test",
            "fixture",
            "docs/protocol.md",
            "docs/protocol.md#h/target",
            "needle term in target heading",
        );

        let query = SearchQuery::parse("needle term").expect("query parses");
        let hits = index.search_hits(&query, None, SearchSpanScope::Any, None, Some(FIELD_BODY));

        assert!(hits.iter().any(|hit| {
            hit.span_id() == Some("docs/protocol.md#h/target") && hit.field() == FIELD_BODY
        }));
        assert!(
            !hits
                .iter()
                .any(|hit| hit.span_id() == Some("docs/protocol.md#full"))
        );

        let exact_full_hits = index.search_hits(
            &query,
            Some("docs/protocol.md"),
            SearchSpanScope::Exact("docs/protocol.md#full"),
            None,
            Some(FIELD_BODY),
        );
        assert_eq!(exact_full_hits.len(), 1);
        assert_eq!(exact_full_hits[0].span_id(), Some("docs/protocol.md#full"));
    }

    #[test]
    fn light_stemming_connects_configure_and_configuration() {
        let query = SearchQuery::parse("configure").expect("query parses");

        assert!(
            query
                .score_normalized(&normalize_search_text("configuration"))
                .is_some()
        );
    }

    #[test]
    fn acronym_expansion_keeps_single_generic_words_low_confidence() {
        let query = SearchQuery::parse("RFC").expect("query parses");
        let raw_score = query
            .score_normalized(&normalize_search_text("request"))
            .expect("single expansion term matches");
        let hit = SearchHit::new(
            "test",
            "fixture",
            "draft.md",
            None,
            raw_score,
            REASON_TITLE_SUBSTRING,
            FIELD_TITLE,
        );
        let ranked = rank_search_hits(
            [hit],
            &RankingContext::new("RFC", DEFAULT_LOW_CONFIDENCE_THRESHOLD),
            &DefaultRanker,
        );

        assert!(ranked[0].low_confidence());
    }

    fn insert_handle(index: &mut SearchIndex, handle: &str, file: &str, summary: &str) {
        insert_handle_with_status(index, handle, file, summary, None);
    }

    fn insert_handle_with_status(
        index: &mut SearchIndex,
        handle: &str,
        file: &str,
        summary: &str,
        status: Option<&str>,
    ) {
        index.insert_handle(SearchHandleDocument {
            corpus: "test",
            source: "fixture",
            handle,
            file,
            summary: Some(summary),
            status,
            namespace: None,
            area: None,
            kind: Some("file"),
        });
    }
}
