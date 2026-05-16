//! Retrieval-provider contracts for runtime content access.
//!
//! Sources emit durable facts. Retrieval providers are the separate access
//! path used by read/search primitives, so hosts can serve lazy content or
//! indexed search without pretending those operations are extraction.

use std::fmt;

use crate::ranking::SearchHit;
use crate::source::ActorContext;

/// Request for bounded span retrieval behind the public `read(...)` relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadRequest<'a> {
    handle: &'a str,
    budget: i64,
    span_id: Option<&'a str>,
}

impl<'a> ReadRequest<'a> {
    #[must_use]
    pub const fn new(handle: &'a str, budget: i64, span_id: Option<&'a str>) -> Self {
        Self {
            handle,
            budget,
            span_id,
        }
    }

    #[must_use]
    pub const fn handle(&self) -> &'a str {
        self.handle
    }

    #[must_use]
    pub const fn budget(&self) -> i64 {
        self.budget
    }

    #[must_use]
    pub const fn span_id(&self) -> Option<&'a str> {
        self.span_id
    }
}

/// Request for whole-handle retrieval behind the gated `read_full(...)` primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadFullRequest<'a> {
    handle: &'a str,
    token_limit: i64,
}

impl<'a> ReadFullRequest<'a> {
    #[must_use]
    pub const fn new(handle: &'a str, token_limit: i64) -> Self {
        Self {
            handle,
            token_limit,
        }
    }

    #[must_use]
    pub const fn handle(&self) -> &'a str {
        self.handle
    }

    #[must_use]
    pub const fn token_limit(&self) -> i64 {
        self.token_limit
    }
}

/// Request for raw search candidates behind the public `search(...)` relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRequest<'a> {
    query: &'a str,
    handle: Option<&'a str>,
    span: SearchSpanScope<'a>,
    reason: Option<&'a str>,
    field: Option<&'a str>,
}

impl<'a> SearchRequest<'a> {
    #[must_use]
    pub const fn new(
        query: &'a str,
        handle: Option<&'a str>,
        span: SearchSpanScope<'a>,
        reason: Option<&'a str>,
        field: Option<&'a str>,
    ) -> Self {
        Self {
            query,
            handle,
            span,
            reason,
            field,
        }
    }

    #[must_use]
    pub const fn query(&self) -> &'a str {
        self.query
    }

    #[must_use]
    pub const fn handle(&self) -> Option<&'a str> {
        self.handle
    }

    #[must_use]
    pub const fn span(&self) -> SearchSpanScope<'a> {
        self.span
    }

    #[must_use]
    pub const fn reason(&self) -> Option<&'a str> {
        self.reason
    }

    #[must_use]
    pub const fn field(&self) -> Option<&'a str> {
        self.field
    }
}

/// Span constraint supplied to a search provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchSpanScope<'a> {
    Any,
    Null,
    Exact(&'a str),
}

impl SearchSpanScope<'_> {
    #[must_use]
    pub fn accepts(self, span_id: Option<&str>) -> bool {
        match self {
            Self::Any => true,
            Self::Null => span_id.is_none(),
            Self::Exact(expected) => span_id == Some(expected),
        }
    }
}

/// One bounded span returned by a content provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadChunk {
    handle: String,
    span_id: String,
    text: String,
    start_line: i64,
    end_line: i64,
    tokens: i64,
}

impl ReadChunk {
    #[must_use]
    pub fn new(
        handle: impl Into<String>,
        span_id: impl Into<String>,
        text: impl Into<String>,
        start_line: i64,
        end_line: i64,
        tokens: i64,
    ) -> Self {
        Self {
            handle: handle.into(),
            span_id: span_id.into(),
            text: text.into(),
            start_line,
            end_line,
            tokens,
        }
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }

    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn start_line(&self) -> i64 {
        self.start_line
    }

    #[must_use]
    pub const fn end_line(&self) -> i64 {
        self.end_line
    }

    #[must_use]
    pub const fn tokens(&self) -> i64 {
        self.tokens
    }

    #[must_use]
    pub fn into_parts(self) -> ReadChunkParts {
        ReadChunkParts {
            handle: self.handle,
            span_id: self.span_id,
            text: self.text,
            start_line: self.start_line,
            end_line: self.end_line,
            tokens: self.tokens,
        }
    }
}

/// Owned fields of a `ReadChunk`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadChunkParts {
    pub handle: String,
    pub span_id: String,
    pub text: String,
    pub start_line: i64,
    pub end_line: i64,
    pub tokens: i64,
}

/// Whole-handle content returned by `read_full(...)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadFullContent {
    handle: String,
    text: String,
    tokens: i64,
}

impl ReadFullContent {
    #[must_use]
    pub fn new(handle: impl Into<String>, text: impl Into<String>, tokens: i64) -> Self {
        Self {
            handle: handle.into(),
            text: text.into(),
            tokens,
        }
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn tokens(&self) -> i64 {
        self.tokens
    }
}

/// Query-local retrieval context.
#[derive(Clone, Copy, Debug)]
pub struct RetrievalContext<'a> {
    actor: &'a ActorContext,
}

impl<'a> RetrievalContext<'a> {
    #[must_use]
    pub const fn new(actor: &'a ActorContext) -> Self {
        Self { actor }
    }

    #[must_use]
    pub const fn actor(&self) -> &'a ActorContext {
        self.actor
    }
}

pub type ReadContext<'a> = RetrievalContext<'a>;
pub type SearchContext<'a> = RetrievalContext<'a>;

/// Provider for bounded content retrieval.
pub trait ContentProvider: Send + Sync {
    fn read(
        &self,
        request: ReadRequest<'_>,
        ctx: &ReadContext<'_>,
    ) -> Result<Vec<ReadChunk>, ReadError>;

    fn read_full(
        &self,
        request: ReadFullRequest<'_>,
        ctx: &ReadContext<'_>,
    ) -> Result<Option<ReadFullContent>, ReadError>;
}

/// Provider for raw search candidates. The runtime still owns calibration.
pub trait SearchProvider: Send + Sync {
    fn search(
        &self,
        request: SearchRequest<'_>,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<SearchHit>, SearchError>;
}

#[derive(Debug)]
pub enum ReadError {
    BudgetExceeded {
        handle: String,
        tokens: i64,
        limit: i64,
    },
    Other(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded {
                handle,
                tokens,
                limit,
            } => write!(
                f,
                "read_full({handle:?}) would return {tokens} tokens, exceeding the hard limit {limit}"
            ),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ReadError {}

#[derive(Debug)]
pub enum SearchError {
    Other(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SearchError {}
