//! CLI surface crate for anneal v2.
//!
//! The existing v1.x CLI remains in the root package during the first
//! foundation checkpoint. This crate is the destination for the v2
//! surface once the shared runtime is wired.

mod context;

pub use anneal_core::runtime::prelude::CONTEXT_OUTPUT_SCHEMA;
use anneal_core::runtime::prelude::{datalog_string_literal, low_confidence_filter};
use anneal_core::{ActorContext, VerbDispatchError, VerbEntry, VerbRegistry, VerbRunPlan};

pub use context::{
    ContextCommand, ContextGroupError, ContextHit, ContextNeighbor, ContextOutput, ContextSpan,
    DEFAULT_CONTEXT_BUDGET, DEFAULT_CONTEXT_HITS, DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH,
};

pub const SURFACE_NAME: &str = "anneal-cli";

pub const DEFAULT_SEARCH_LIMIT: usize = 25;
pub const DEFAULT_READ_BUDGET: i64 = 4_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliVerb {
    name: String,
    doc: String,
    default_args: Vec<String>,
}

impl CliVerb {
    fn from_entry(entry: &VerbEntry) -> Self {
        Self {
            name: entry.name().to_string(),
            doc: entry.doc().to_string(),
            default_args: entry.default_args().to_vec(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn default_args(&self) -> &[String] {
        &self.default_args
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliVerbProjection {
    verbs: Vec<CliVerb>,
}

impl CliVerbProjection {
    pub fn from_registry(registry: &VerbRegistry) -> Self {
        Self {
            verbs: registry.iter().map(CliVerb::from_entry).collect(),
        }
    }

    pub fn verbs(&self) -> &[CliVerb] {
        &self.verbs
    }

    pub fn run_plan(
        &self,
        registry: &VerbRegistry,
        actor: &ActorContext,
        name: &str,
    ) -> Result<VerbRunPlan, VerbDispatchError> {
        registry.run_plan_for_actor(name, actor)
    }
}

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
    use anneal_core::{ActorContext, VerbLayer, VerbRegistry};

    use super::*;

    fn registry(source: &str) -> VerbRegistry {
        let program = parse_program("anneal.dl", source).expect("program parses");
        VerbRegistry::from_layers(&[(VerbLayer::Project, &program)]).expect("registry builds")
    }

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

    #[test]
    fn cli_projects_resolved_registry_verbs() {
        let registry = registry(
            r#"
            @verb(
              name: "work",
              query: "? item(h).",
              doc: "Show work.",
              output_schema: "{\"h\":\"String\"}",
              default_args: ["limit"],
              capabilities: ["read"]
            ).
            item("h").
            "#,
        );

        let projection = CliVerbProjection::from_registry(&registry);
        assert_eq!(projection.verbs().len(), 1);
        assert_eq!(projection.verbs()[0].name(), "work");
        assert_eq!(projection.verbs()[0].default_args(), &["limit".to_string()]);
        let plan = projection
            .run_plan(&registry, &ActorContext::trusted_cli(), "work")
            .expect("work dispatches");
        assert_eq!(plan.query_source(), "? item(h).");
    }

    #[test]
    fn cli_run_plan_reports_registry_dispatch_errors() {
        let registry = registry(
            r#"
            @verb(
              name: "release",
              query: "? item(h).",
              doc: "Release.",
              output_schema: "{\"h\":\"String\"}",
              default_args: [],
              capabilities: ["release"]
            ).
            item("h").
            "#,
        );
        let projection = CliVerbProjection::from_registry(&registry);

        assert!(matches!(
            projection.run_plan(&registry, &ActorContext::anonymous_mcp(), "release"),
            Err(VerbDispatchError::CapabilityDenied { .. })
        ));
        assert!(matches!(
            projection.run_plan(&registry, &ActorContext::trusted_cli(), "missing"),
            Err(VerbDispatchError::MissingVerb { .. })
        ));
    }
}
