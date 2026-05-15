//! Search hit ranking contracts.
//!
//! Adapters and hosts may provide their own rankers, but the public
//! `search(...)` relation always exposes calibrated scores in `[0, 1]`.

use std::cmp::Ordering;

/// Internal search candidate produced from stored corpus facts.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub source: String,
    pub handle: String,
    pub span_id: Option<String>,
    pub raw_score: f32,
    pub reason: String,
    pub field: String,
}

impl SearchHit {
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        handle: impl Into<String>,
        span_id: Option<String>,
        raw_score: f32,
        reason: impl Into<String>,
        field: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            handle: handle.into(),
            span_id,
            raw_score: raw_score.clamp(0.0, 1.0),
            reason: reason.into(),
            field: field.into(),
        }
    }
}

/// Query-local ranking inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct RankingContext {
    pub query: String,
    pub low_confidence_threshold: f32,
}

impl RankingContext {
    #[must_use]
    pub fn new(query: impl Into<String>, low_confidence_threshold: f32) -> Self {
        Self {
            query: query.into(),
            low_confidence_threshold: low_confidence_threshold.clamp(0.0, 1.0),
        }
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
        (hit.raw_score * field_weight(&hit.field)).clamp(0.0, 1.0)
    }

    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering {
        a.source
            .cmp(&b.source)
            .then_with(|| a.handle.cmp(&b.handle))
            .then_with(|| a.span_id.cmp(&b.span_id))
            .then_with(|| a.field.cmp(&b.field))
            .then_with(|| a.reason.cmp(&b.reason))
    }
}

fn field_weight(field: &str) -> f32 {
    match field {
        "identifier" => 1.0,
        "title" => 0.95,
        "body" => 0.82,
        _ if field.starts_with("frontmatter:") => 0.88,
        _ => 0.75,
    }
}
