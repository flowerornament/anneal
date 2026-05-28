//! CLI surface crate for anneal.
//!
//! This crate owns the programmable runtime commands while the legacy crate
//! keeps the compatibility corpus-health surface available.

pub mod app;
mod context;

pub use anneal_core::runtime::prelude::CONTEXT_OUTPUT_SCHEMA;
use anneal_core::runtime::prelude::{datalog_string_literal, low_confidence_filter};
use anneal_core::{ActorContext, VerbArg, VerbDispatchError, VerbEntry, VerbRegistry, VerbRunPlan};

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
    args: Vec<VerbArg>,
}

impl CliVerb {
    fn from_entry(entry: &VerbEntry) -> Self {
        Self {
            name: entry.name().to_string(),
            doc: entry.doc().to_string(),
            args: entry.args().to_vec(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn args(&self) -> &[VerbArg] {
        &self.args
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
        registry.run_plan_for_actor(canonical_cli_verb_name(name), actor)
    }
}

fn canonical_cli_verb_name(name: &str) -> &str {
    match name {
        "H" => "handle",
        _ => name,
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

#[cfg(test)]
mod tests {
    use anneal_core::runtime::prelude::standard_prelude_program;
    use anneal_core::runtime::{
        Database, EvalOptions, Evaluator, Literal, NumberLiteral, Program, QueryOutput, Row,
        analyze, parse_program,
    };
    use anneal_core::{
        ActorContext, ConfigFact, ConfigKey, ContentFact, CorpusId, EdgeFact, FactBatch,
        FactBatchMode, FactIdentity, FactStore, Generation, HandleFact, NativeId, OriginUri,
        Pattern, Revision, SnapshotFact, SourceCapabilities, SourceInfo, SourceName, SpanFact,
        VerbLayer, VerbRegistry,
    };

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
    fn cli_projects_resolved_registry_verbs() {
        let registry = registry(
            r#"
            @verb(
              name: "work",
              query: "? item(h).",
              doc: "Show work.",
              output_schema: "{\"h\":\"String\"}",
              args: ["limit:Number"],
              capabilities: ["read"]
            ).
            item("h").
            "#,
        );

        let projection = CliVerbProjection::from_registry(&registry);
        assert_eq!(projection.verbs().len(), 1);
        assert_eq!(projection.verbs()[0].name(), "work");
        assert_eq!(projection.verbs()[0].args().len(), 1);
        assert_eq!(projection.verbs()[0].args()[0].name(), "limit");
        let plan = projection
            .run_plan(&registry, &ActorContext::trusted_cli(), "work")
            .expect("work dispatches");
        assert_eq!(plan.query_source(), "? item(h).");
    }

    #[test]
    fn cli_h_alias_dispatches_to_handle_verb() {
        let registry = registry(
            r#"
            @verb(
              name: "handle",
              query: "? handle(h).",
              doc: "Show handle.",
              output_schema: "{\"h\":\"String\"}",
              args: ["h:HandleId"],
              capabilities: ["read"]
            ).
            handle("ticket-1").
            "#,
        );
        let projection = CliVerbProjection::from_registry(&registry);

        let plan = projection
            .run_plan(&registry, &ActorContext::trusted_cli(), "H")
            .expect("H dispatches to handle");

        assert_eq!(plan.query_source(), "? handle(h).");
    }

    #[test]
    fn phase10_starter_verbs_emit_valid_ndjson() {
        let prelude = standard_prelude_program().expect("standard prelude parses");
        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Prelude, &prelude)]).expect("registry builds");
        let projection = CliVerbProjection::from_registry(&registry);
        let database = surface_database();
        let mut names = projection
            .verbs()
            .iter()
            .map(|verb| verb.name().to_string())
            .collect::<Vec<_>>();
        names.push("H".to_string());

        for name in names {
            let plan = projection
                .run_plan(&registry, &ActorContext::trusted_cli(), &name)
                .unwrap_or_else(|err| panic!("{name} should dispatch: {err}"));
            let output = evaluate_surface_plan(&name, plan.query_source(), database.clone());
            assert!(
                !output.rows.is_empty(),
                "{name} should emit at least one row for the Phase 10 fixture"
            );
            let ndjson = encode_ndjson(&output.rows);

            for line in ndjson.lines() {
                let value = serde_json::from_str::<serde_json::Value>(line)
                    .unwrap_or_else(|err| panic!("{name} emitted invalid NDJSON: {err}"));
                assert!(
                    value.as_object().is_some_and(|object| !object.is_empty()),
                    "{name} should emit object rows"
                );
            }
        }
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
              args: [],
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

    fn evaluate_surface_plan(name: &str, query_source: &str, database: Database) -> QueryOutput {
        let mut query_program = parse_program(&format!("phase10:{name}"), query_source)
            .unwrap_or_else(|err| panic!("{name} query should parse: {err}"));
        bind_surface_args(name, &mut query_program);
        let mut program = standard_prelude_program().expect("standard prelude parses");
        program.statements.extend(query_program.statements);
        let analyzed =
            analyze(program).unwrap_or_else(|err| panic!("{name} should analyze: {err}"));
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .unwrap_or_else(|| panic!("{name} should contain one query"));
        let mut evaluator = Evaluator::with_options(analyzed, database, EvalOptions::default());
        evaluator
            .run_fixpoint()
            .unwrap_or_else(|err| panic!("{name} fixpoint should run: {err}"));
        evaluator
            .eval_query(&query)
            .unwrap_or_else(|err| panic!("{name} query should evaluate: {err}"))
    }

    fn bind_surface_args(name: &str, program: &mut Program) {
        match canonical_cli_verb_name(name) {
            "handle" | "blocked" => {
                bind_parameter_fact(program, ParameterBinding::string("h", "ticket-1"));
            }
            "search" => {
                bind_parameter_fact(program, ParameterBinding::string("query", "ticket"));
                bind_parameter_fact(program, ParameterBinding::int("limit", 10));
            }
            "context" => {
                bind_parameter_fact(program, ParameterBinding::string("goal", "ticket"));
                bind_parameter_fact(program, ParameterBinding::int("hits", 3));
                bind_parameter_fact(program, ParameterBinding::int("budget", 2400));
                bind_parameter_fact(program, ParameterBinding::int("depth", 1));
            }
            "read" => {
                bind_parameter_fact(program, ParameterBinding::string("h", "ticket-1"));
                bind_parameter_fact(program, ParameterBinding::int("budget", 4000));
            }
            "describe" => bind_parameter_fact(program, ParameterBinding::string("name", "runtime")),
            "source-of" => {
                bind_parameter_fact(program, ParameterBinding::string("name", "frontier"));
            }
            "examples" => bind_parameter_fact(program, ParameterBinding::string("name", "search")),
            _ => {}
        }
    }

    struct ParameterBinding {
        name: &'static str,
        value: Literal,
    }

    impl ParameterBinding {
        fn string(name: &'static str, value: &str) -> Self {
            Self {
                name,
                value: Literal::String(value.to_string()),
            }
        }

        fn int(name: &'static str, value: i64) -> Self {
            Self {
                name,
                value: Literal::Number(NumberLiteral::Int(value)),
            }
        }
    }

    fn bind_parameter_fact(program: &mut Program, binding: ParameterBinding) {
        let ParameterBinding { name, value } = binding;
        let fact_source = format!(
            "verb_arg({}, {}).",
            datalog_string_literal(name),
            literal_to_datalog(&value)
        );
        let facts = parse_program("verb-arg", &fact_source).expect("verb_arg fact should parse");
        program.statements.splice(0..0, facts.statements);
    }

    fn literal_to_datalog(value: &Literal) -> String {
        match value {
            Literal::String(value) => datalog_string_literal(value),
            Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
            Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
            Literal::Bool(value) => value.to_string(),
            Literal::Null => "null".to_string(),
            Literal::List(_) => panic!("test helper only supports scalar verb args"),
        }
    }

    fn encode_ndjson(rows: &[Row]) -> String {
        let mut out = Vec::new();
        anneal_core::runtime::write_ndjson(&mut out, rows.iter()).expect("rows encode");
        String::from_utf8(out).expect("json is utf8")
    }

    fn surface_database() -> Database {
        let mut store = FactStore::default();
        let mut batch = FactBatch::new(
            CorpusId::from("phase10"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("ticket-1", "issue", Some("open"), "ticket one urgent work"),
            handle(
                "ticket-2",
                "issue",
                Some("review"),
                "ticket two review work",
            ),
            handle("done-1", "issue", Some("closed"), "closed dependency"),
        ];
        batch.content = vec![
            content(
                "ticket-1",
                "intro",
                "ticket one has urgent work and a broken reference",
                9,
            ),
            content("ticket-2", "intro", "ticket two recently advanced", 5),
        ];
        batch.spans = vec![
            span("ticket-1", "intro", 1, 3),
            span("ticket-2", "intro", 1, 2),
        ];
        batch.edges = vec![
            edge("ticket-1", "done-1", "DependsOn", 4),
            edge("ticket-1", "missing.md", "Cites", 7),
        ];
        store.merge(batch).expect("merge surface fixture");
        store
            .replace_configs(
                &CorpusId::from("phase10"),
                vec![
                    config("convergence.active", "open", None),
                    config("convergence.active", "review", None),
                    config("convergence.terminal", "closed", None),
                    config("convergence.ordering", "open", Some(0)),
                    config("convergence.ordering", "review", Some(1)),
                    config("convergence.ordering", "closed", Some(2)),
                ],
            )
            .expect("replace configs");
        store
            .replace_snapshots(
                &CorpusId::from("phase10"),
                vec![SnapshotFact {
                    corpus: CorpusId::from("phase10"),
                    snapshot: "s1".to_string(),
                    at: "2026-05-01".to_string(),
                    id: "ticket-2".to_string(),
                    key: "status".to_string(),
                    value: "open".to_string(),
                }],
            )
            .expect("replace snapshots");
        Database::from_store(&store).with_sources([SourceInfo {
            name: "fixture",
            recognizes: vec![Pattern::new("*.md")],
            doc: "Phase 10 surface fixture.",
            config_keys: vec![ConfigKey::required("md.file_extension")],
            capabilities: SourceCapabilities {
                supports_git_ref: false,
                supports_time_snapshot: true,
                supports_incremental: false,
                live_only: false,
            },
            search: None,
        }])
    }

    fn handle(id: &str, kind: &str, status: Option<&str>, summary: &str) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(ToString::to_string),
            namespace: String::new(),
            file: format!("{id}.md"),
            line: 1,
            date: None,
            area: "phase10".to_string(),
            summary: summary.to_string(),
        }
    }

    fn edge(from: &str, to: &str, kind: &str, line: u32) -> EdgeFact {
        EdgeFact {
            identity: identity(&format!("{from}->{to}:{kind}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: format!("{from}.md"),
            line,
        }
    }

    fn content(handle: &str, span_id: &str, text: &str, tokens: u32) -> ContentFact {
        ContentFact {
            identity: identity(&format!("{handle}#{span_id}:content")),
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

    fn config(key: &str, value: &str, ordinal: Option<u32>) -> ConfigFact {
        ConfigFact {
            corpus: CorpusId::from("phase10"),
            key: key.to_string(),
            value: value.to_string(),
            ordinal,
        }
    }

    fn identity(native_id: &str) -> FactIdentity {
        FactIdentity {
            corpus: CorpusId::from("phase10"),
            source: SourceName::from("fixture"),
            native_id: NativeId::from(native_id),
            origin_uri: OriginUri::from(format!("fixture://{native_id}")),
            revision: Revision::from("rev"),
            generation: Generation::initial(),
        }
    }
}
