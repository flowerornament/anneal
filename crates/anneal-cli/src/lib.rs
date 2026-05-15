//! CLI surface crate for anneal v2.
//!
//! The existing v1.x CLI remains in the root package during the first
//! foundation checkpoint. This crate is the destination for the v2
//! surface once the shared runtime is wired.

mod context;

pub use anneal_core::runtime::prelude::CONTEXT_OUTPUT_SCHEMA;
use anneal_core::runtime::prelude::{datalog_string_literal, low_confidence_filter};

pub use context::{
    ContextCommand, ContextGroupError, ContextHit, ContextNeighbor, ContextOutput, ContextSpan,
    DEFAULT_CONTEXT_BUDGET, DEFAULT_CONTEXT_HITS, DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH,
};

pub const SURFACE_NAME: &str = "anneal-cli";

pub const DEFAULT_SEARCH_LIMIT: usize = 25;
pub const DEFAULT_READ_BUDGET: i64 = 4_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCommand {
    query: String,
    limit: usize,
    include_low_confidence: bool,
}

impl SearchCommand {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: DEFAULT_SEARCH_LIMIT,
            include_low_confidence: false,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn include_low_confidence(mut self, include: bool) -> Self {
        self.include_low_confidence = include;
        self
    }

    pub fn datalog(&self) -> String {
        let query = datalog_string_literal(&self.query);
        let confidence_filter = low_confidence_filter(self.include_low_confidence);
        format!(
            "\
? (h, span_id, score, reason, field, low_confidence) = TopK{{ k: {limit}, key: score :
    (h, span_id, score, reason, field, low_confidence) :
        search({query}, h, span_id, score, reason, field, low_confidence){confidence_filter}
}}.",
            limit = self.limit,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadCommand {
    handle: String,
    budget: i64,
}

impl ReadCommand {
    pub fn new(handle: impl Into<String>) -> Self {
        Self {
            handle: handle.into(),
            budget: DEFAULT_READ_BUDGET,
        }
    }

    pub fn with_budget(mut self, budget: i64) -> Self {
        self.budget = budget.max(0);
        self
    }

    pub fn datalog(&self) -> String {
        format!(
            "? read({}, {}, span_id, text, start_line, end_line, tokens).",
            datalog_string_literal(&self.handle),
            self.budget,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescribeCommand {
    name: String,
}

impl DescribeCommand {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn runtime() -> Self {
        Self::new("runtime")
    }

    pub fn datalog(&self) -> String {
        format!("? describe({}, doc).", datalog_string_literal(&self.name))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourcesCommand;

impl SourcesCommand {
    pub fn datalog(self) -> &'static str {
        "? sources(name, recognizes, capabilities, doc)."
    }
}

#[cfg(test)]
mod tests {
    use anneal_core::runtime::{analyze, parse_program};

    use super::*;

    #[test]
    fn search_template_filters_low_confidence_by_default() {
        let query = SearchCommand::new("v17 conformance audit")
            .with_limit(3)
            .datalog();

        assert!(query.contains("TopK{ k: 3"));
        assert!(query.contains("low_confidence = false"));
        analyze(parse_program("search", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn search_template_can_include_low_confidence() {
        let query = SearchCommand::new("v17 conformance audit")
            .include_low_confidence(true)
            .datalog();

        assert!(!query.contains("low_confidence = false"));
        analyze(parse_program("search", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn read_template_escapes_handle_literals() {
        let query = ReadCommand::new("notes/\"quoted\".md")
            .with_budget(1200)
            .datalog();

        assert!(query.contains(r#""notes/\"quoted\".md""#));
        analyze(parse_program("read", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn describe_runtime_template_targets_self_description() {
        let query = DescribeCommand::runtime().datalog();

        assert_eq!(query, r#"? describe("runtime", doc)."#);
        analyze(parse_program("describe", &query).expect("query parses")).expect("query analyzes");
    }

    #[test]
    fn sources_template_lists_linked_adapters() {
        let query = SourcesCommand.datalog();

        assert_eq!(query, "? sources(name, recognizes, capabilities, doc).");
        analyze(parse_program("sources", query).expect("query parses")).expect("query analyzes");
    }
}
