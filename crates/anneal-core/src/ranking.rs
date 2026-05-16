//! Search hit ranking contracts.
//!
//! Adapters and hosts may provide their own rankers, but the public
//! `search(...)` relation always exposes calibrated scores in `[0, 1]`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::retrieval::SearchSpanScope;
use crate::source::SearchInfo;

pub const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f32 = 0.5;
const PARENT_CLUSTER_RAW_SCORE_BOOST: f32 = 0.15;

pub const FIELD_IDENTIFIER: &str = "identifier";
pub const FIELD_TITLE: &str = "title";
pub const FIELD_BODY: &str = "body";
pub const FIELD_FRONTMATTER_GLOB: &str = "frontmatter:*";

pub const REASON_IDENTIFIER_SUBSTRING: &str = "identifier-substring";
pub const REASON_TITLE_SUBSTRING: &str = "title-substring";
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
            REASON_FRONTMATTER_KEY_MATCH,
            REASON_FRONTMATTER_VALUE_MATCH,
            REASON_BODY_SUBSTRING,
            REASON_PARENT_CLUSTER,
        ],
        fields: vec![
            FIELD_IDENTIFIER,
            FIELD_TITLE,
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
        SearchScore::new(hit.raw_score().get() * field_weight(hit.field())).get()
    }

    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering {
        a.corpus()
            .cmp(b.corpus())
            .then_with(|| a.source().cmp(b.source()))
            .then_with(|| reason_priority(a.reason()).cmp(&reason_priority(b.reason())))
            .then_with(|| a.handle().cmp(b.handle()))
            .then_with(|| a.span_id().cmp(&b.span_id()))
            .then_with(|| a.field().cmp(b.field()))
            .then_with(|| a.reason().cmp(b.reason()))
    }
}

fn reason_priority(reason: &str) -> u8 {
    u8::from(reason != REASON_PARENT_CLUSTER)
}

fn field_weight(field: &str) -> f32 {
    match field {
        FIELD_IDENTIFIER => 1.0,
        FIELD_TITLE => 0.95,
        FIELD_BODY => 0.82,
        _ if field.starts_with("frontmatter:") => 0.88,
        _ => 0.75,
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SearchIndex {
    documents: BTreeMap<SearchDocumentKey, SearchDocument>,
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
    fields: Vec<SearchField>,
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
            );
        }
        if let Some(handle) = handle {
            hits.retain(|hit| hit.handle() == handle);
        }
        if cluster_only {
            hits.retain(|hit| hit.reason() == REASON_PARENT_CLUSTER);
        }
        hits
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

        hits.extend(clusters.into_iter().filter_map(|(parent_key, cluster)| {
            let cluster_score = cluster.cluster_score(query);
            let direct_signal = self.direct_parent_signal(&parent_key, query);
            (cluster.is_actionable()
                && cluster.best_raw_score > direct_signal.structural_raw_score
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
        }));
    }

    fn direct_parent_signal(
        &self,
        parent_key: &SearchDocumentKey,
        query: &SearchQuery,
    ) -> ParentDirectSignal {
        let Some(parent) = self.documents.get(parent_key) else {
            return ParentDirectSignal::default();
        };
        let ctx = RankingContext::new(query.original(), DEFAULT_LOW_CONFIDENCE_THRESHOLD);
        parent
            .search_hits(parent_key, query, SearchSpanScope::Any, None, None)
            .fold(ParentDirectSignal::default(), |mut signal, hit| {
                if matches!(hit.field(), FIELD_IDENTIFIER | FIELD_TITLE) {
                    signal.structural_raw_score =
                        signal.structural_raw_score.max(hit.raw_score().get());
                }
                signal.calibrated_score = signal
                    .calibrated_score
                    .max(DefaultRanker.calibrate(&hit, &ctx));
                signal
            })
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
        self.children.len() >= 2
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
        DefaultRanker.calibrate(&hit, &ctx)
    }
}

impl SearchDocument {
    fn search_hits<'a>(
        &'a self,
        key: &'a SearchDocumentKey,
        query: &'a SearchQuery,
        span_filter: SearchSpanScope<'a>,
        reason_filter: Option<&'a str>,
        field_filter: Option<&'a str>,
    ) -> impl Iterator<Item = SearchHit> + 'a {
        self.fields
            .iter()
            .filter(move |field| span_filter.accepts(field.span_id.as_deref()))
            .filter(move |field| reason_filter.is_none_or(|reason| field.reason == reason))
            .filter(move |field| field_filter.is_none_or(|field_name| field.field == field_name))
            .filter_map(move |field| {
                query.score_normalized(&field.normalized_text).map(|score| {
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
    total_weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct QueryTerm {
    value: String,
    weight: f32,
}

impl SearchQuery {
    pub(crate) fn parse(query: &str) -> Option<Self> {
        let original_terms = search_tokens(query);
        let normalized = original_terms.join(" ");
        if normalized.is_empty() {
            return None;
        }
        let total_weight = f32::from(u16::try_from(original_terms.len()).unwrap_or(u16::MAX));
        let terms = expanded_query_terms(&original_terms);
        Some(Self {
            original: query.to_owned(),
            normalized,
            terms,
            total_weight,
        })
    }

    fn original(&self) -> &str {
        &self.original
    }

    fn score_normalized(&self, normalized_text: &str) -> Option<f32> {
        if normalized_text.is_empty() {
            return None;
        }
        if normalized_text.contains(&self.normalized) {
            return Some(1.0);
        }
        let matched_weight = self
            .terms
            .iter()
            .filter(|term| normalized_text.contains(term.value.as_str()))
            .map(|term| term.weight)
            .sum::<f32>();
        (matched_weight > 0.0)
            .then(|| 0.35 + (0.55 * (matched_weight / self.total_weight).min(1.0)))
    }
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
        index.insert_handle(SearchHandleDocument {
            corpus: "test",
            source: "fixture",
            handle,
            file,
            summary: Some(summary),
            status: None,
            namespace: None,
            area: None,
            kind: Some("file"),
        });
    }
}
