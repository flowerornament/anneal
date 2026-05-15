//! CLI surface crate for anneal v2.
//!
//! The existing v1.x CLI remains in the root package during the first
//! foundation checkpoint. This crate is the destination for the v2
//! surface once the shared runtime is wired.

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
        let query = datalog_string(&self.query);
        let confidence_filter = if self.include_low_confidence {
            String::new()
        } else {
            ",\n        low_confidence = false".to_string()
        };
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
            datalog_string(&self.handle),
            self.budget,
        )
    }
}

fn datalog_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
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
}
