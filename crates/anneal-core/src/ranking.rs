//! Search hit ranking contracts.
//!
//! Adapters and hosts may provide their own rankers, but the public
//! `search(...)` relation always exposes calibrated scores in `[0, 1]`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::retrieval::SearchSpanScope;
use crate::source::SearchInfo;

pub const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f32 = 0.5;

pub const FIELD_IDENTIFIER: &str = "identifier";
pub const FIELD_TITLE: &str = "title";
pub const FIELD_BODY: &str = "body";
pub const FIELD_FRONTMATTER_GLOB: &str = "frontmatter:*";

pub const REASON_IDENTIFIER_SUBSTRING: &str = "identifier-substring";
pub const REASON_TITLE_SUBSTRING: &str = "title-substring";
pub const REASON_FRONTMATTER_KEY_MATCH: &str = "frontmatter-key-match";
pub const REASON_FRONTMATTER_VALUE_MATCH: &str = "frontmatter-value-match";
pub const REASON_BODY_SUBSTRING: &str = "body-substring";

#[must_use]
pub fn default_lexical_search_info() -> SearchInfo {
    SearchInfo {
        reason_vocabulary: vec![
            REASON_IDENTIFIER_SUBSTRING,
            REASON_TITLE_SUBSTRING,
            REASON_FRONTMATTER_KEY_MATCH,
            REASON_FRONTMATTER_VALUE_MATCH,
            REASON_BODY_SUBSTRING,
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
            .then_with(|| a.handle().cmp(b.handle()))
            .then_with(|| a.span_id().cmp(&b.span_id()))
            .then_with(|| a.field().cmp(b.field()))
            .then_with(|| a.reason().cmp(b.reason()))
    }
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
        self.documents
            .iter()
            .filter(|(key, _)| handle.is_none_or(|handle| key.handle == handle))
            .flat_map(|(key, document)| {
                document.search_hits(key, query, span_filter, reason_filter, field_filter)
            })
            .collect()
    }

    fn document_mut(&mut self, corpus: &str, source: &str, handle: &str) -> &mut SearchDocument {
        self.documents
            .entry(SearchDocumentKey::new(corpus, source, handle))
            .or_default()
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchQuery {
    normalized: String,
    terms: Vec<String>,
}

impl SearchQuery {
    pub(crate) fn parse(query: &str) -> Option<Self> {
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

    fn score_normalized(&self, normalized_text: &str) -> Option<f32> {
        if normalized_text.is_empty() {
            return None;
        }
        if normalized_text.contains(&self.normalized) {
            return Some(1.0);
        }
        let matched = self
            .terms
            .iter()
            .filter(|term| normalized_text.contains(term.as_str()))
            .count();
        (matched > 0).then(|| {
            let matched = u16::try_from(matched).unwrap_or(u16::MAX);
            let total = u16::try_from(self.terms.len().max(1)).unwrap_or(u16::MAX);
            0.35 + (0.55 * (f32::from(matched) / f32::from(total)))
        })
    }
}

fn normalize_search_text(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_space = true;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            normalized.push(ch);
            previous_was_space = false;
        } else if !previous_was_space {
            normalized.push(' ');
            previous_was_space = true;
        }
    }
    if normalized.ends_with(' ') {
        normalized.pop();
    }
    normalized
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
