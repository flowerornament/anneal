//! Runtime schema and predicate introspection.

use std::collections::{BTreeMap, BTreeSet};

use crate::facts::STORED_RELATION_DESCRIPTORS;
use crate::source::{SourceCapabilities, SourceInfo};
use crate::trail::TRAIL_RELATION_DESCRIPTORS;
use crate::verbs::VerbRegistry;

use super::analysis::{AnalyzedProgram, AnalyzedQuery};
use super::ast::{
    DocDecl, Expr, Head, PredicateDecl, Program, RuleLayer, SourceLocation, Statement,
};
use super::eval::{Tuple, Value};
use super::primitives::PrimitivePredicate;

#[derive(Clone, Debug, Default)]
pub(crate) struct IntrospectionIndex {
    source_descriptions: Vec<DescribeEntry>,
    source_rows: Vec<Tuple>,
    program: ProgramIntrospection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredRelationSummary {
    pub(crate) name: String,
    pub(crate) fields: Vec<String>,
}

impl IntrospectionIndex {
    pub(crate) fn from_sources(sources: Vec<SourceInfo>) -> Self {
        let mut sources = sources;
        sources.sort_by(|left, right| left.name.cmp(right.name));
        let source_descriptions = sources
            .iter()
            .map(|source| {
                let recognizes = source
                    .recognizes
                    .iter()
                    .map(|pattern| pattern.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let capabilities =
                    source_capability_names(&source.capabilities, source.search.is_some())
                        .collect::<Vec<_>>()
                        .join(", ");
                describe_entry(
                    source.name,
                    DescribeKind::SourceAdapter,
                    &describe_card(DescribeCard {
                        summary: source.doc,
                        kind: Some(DescribeKind::SourceAdapter),
                        extra_lines: vec![
                            format!("Recognizes: {recognizes}."),
                            format!("Capabilities: [{capabilities}]."),
                        ],
                        ..DescribeCard::default()
                    }),
                )
            })
            .collect();
        let source_rows = sources.iter().map(source_tuple).collect();
        Self {
            source_descriptions,
            source_rows,
            program: ProgramIntrospection::default(),
        }
    }

    pub(crate) fn for_program(
        &self,
        program: &AnalyzedProgram,
        dynamic_stored: Vec<StoredRelationSummary>,
    ) -> Self {
        Self {
            source_descriptions: self.source_descriptions.clone(),
            source_rows: self.source_rows.clone(),
            program: ProgramIntrospection::from_program(program, dynamic_stored),
        }
    }

    pub(crate) fn for_query(&self, query: &AnalyzedQuery) -> Self {
        Self {
            source_descriptions: self.source_descriptions.clone(),
            source_rows: self.source_rows.clone(),
            program: self.program.with_query(query),
        }
    }

    pub(crate) fn tuples(
        &self,
        primitive: PrimitivePredicate,
        constraints: &[(usize, Value)],
    ) -> Vec<Tuple> {
        match primitive {
            PrimitivePredicate::Schema => matching_tuples(&self.program.schema, constraints),
            PrimitivePredicate::Predicates => {
                matching_tuples(&self.program.predicates, constraints)
            }
            PrimitivePredicate::Verbs => matching_tuples(&self.program.verbs, constraints),
            PrimitivePredicate::Describe => self.describe_tuples(constraints),
            PrimitivePredicate::SourceOf => matching_tuples(&self.program.source_of, constraints),
            PrimitivePredicate::Examples => matching_tuples(&self.program.examples, constraints),
            PrimitivePredicate::Sources => matching_tuples(&self.source_rows, constraints),
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
            | PrimitivePredicate::TokenEstimate
            | PrimitivePredicate::Search
            | PrimitivePredicate::Read
            | PrimitivePredicate::ReadFull
            | PrimitivePredicate::Match => Vec::new(),
        }
    }

    fn describe_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let mut entries = self
            .program
            .describe
            .iter()
            .chain(&self.source_descriptions)
            .filter(|entry| entry.matches_constraints(constraints))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort();
        entries.into_iter().map(|entry| entry.tuple).collect()
    }
}

pub(crate) fn is_static_stored_relation(name: &str) -> bool {
    STORED_RELATION_DESCRIPTORS
        .iter()
        .chain(TRAIL_RELATION_DESCRIPTORS)
        .any(|relation| relation.name == name)
}

#[derive(Clone, Debug, Default)]
struct ProgramIntrospection {
    schema: Vec<Tuple>,
    predicates: Vec<Tuple>,
    verbs: Vec<Tuple>,
    describe: Vec<DescribeEntry>,
    source_of: Vec<Tuple>,
    examples: Vec<Tuple>,
}

impl ProgramIntrospection {
    fn from_program(program: &AnalyzedProgram, dynamic_stored: Vec<StoredRelationSummary>) -> Self {
        let mut builder = IntrospectionBuilder::default();
        builder.add_runtime_overview();
        builder.add_stored_relations(dynamic_stored);
        builder.add_primitives();
        builder.add_program(program.program());
        builder.add_diagnostic_codes();
        builder.finish()
    }

    fn with_query(&self, query: &AnalyzedQuery) -> Self {
        let mut builder = IntrospectionBuilder::from_existing(self);
        builder.add_query(query);
        builder.finish()
    }
}

#[derive(Default)]
struct IntrospectionBuilder {
    schema: BTreeSet<Tuple>,
    predicates: BTreeSet<Tuple>,
    verbs: BTreeSet<Tuple>,
    describe: BTreeSet<DescribeEntry>,
    source_of: BTreeSet<Tuple>,
    examples: BTreeSet<Tuple>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescribeKind {
    RuntimeTopic,
    SourceAdapter,
    StoredRelation,
    EnginePrimitive,
    DerivedPredicate,
    Verb,
}

impl DescribeKind {
    const fn label(self) -> &'static str {
        match self {
            Self::RuntimeTopic => "runtime topic",
            Self::SourceAdapter => "source adapter",
            Self::StoredRelation => "stored relation",
            Self::EnginePrimitive => "engine primitive",
            Self::DerivedPredicate => "derived predicate",
            Self::Verb => "verb",
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Verb => 0,
            Self::DerivedPredicate => 1,
            Self::EnginePrimitive => 2,
            Self::StoredRelation => 3,
            Self::RuntimeTopic => 4,
            Self::SourceAdapter => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DescribeEntry {
    rank: u8,
    name: String,
    tuple: Tuple,
}

impl DescribeEntry {
    fn matches_constraints(&self, constraints: &[(usize, Value)]) -> bool {
        self.tuple.matches_constraints(constraints)
    }
}

fn describe_entry(name: &str, kind: DescribeKind, doc: &str) -> DescribeEntry {
    DescribeEntry {
        rank: kind.rank(),
        name: name.to_string(),
        tuple: Tuple(vec![string_value(name), string_value(doc)]),
    }
}

impl IntrospectionBuilder {
    fn from_existing(existing: &ProgramIntrospection) -> Self {
        Self {
            schema: existing.schema.iter().cloned().collect(),
            predicates: existing.predicates.iter().cloned().collect(),
            verbs: existing.verbs.iter().cloned().collect(),
            describe: existing.describe.iter().cloned().collect(),
            source_of: existing.source_of.iter().cloned().collect(),
            examples: existing.examples.iter().cloned().collect(),
        }
    }

    fn add_runtime_overview(&mut self) {
        self.describe.insert(describe_entry(
            "runtime",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: "Query stored corpus facts, compose graph/lifecycle/content/search primitives, load Datalog rules, and discover the available model.",
                kind: Some(DescribeKind::RuntimeTopic),
                extra_lines: vec![
                    "Visible commands: status, context, search, read, handle, schema, describe, eval, init.".to_string(),
                    "Hidden support commands: check, prime.".to_string(),
                    "Agent briefing: anneal help agent (or hidden alias anneal prime).".to_string(),
                    "Use schema for the callable catalog, describe NAME for examples and joins, and eval/-e for composition.".to_string(),
                    "Dimensional map: axis(name, question, oracle, disposition) lists runtime axes, axis_of(predicate, axis) places vocabulary, and describe <axis> opens the teaching card for currency, lifecycle, recency, relevance, importance, convergence, structure, obligations, or topic.".to_string(),
                    "Schema discovery is interactive: unknown predicate or field errors include nearby names and allowed fields.".to_string(),
                    "Observed vocabulary recipes: query *handle.status, *edge.kind, *handle.namespace, or *meta.key directly.".to_string(),
                    "Orientation predicates:".to_string(),
                    "  - recent_frontier(h, rank, recency) ranks goal-less reading candidates: date-backed authored age first, coarse git change-recency only for undated files.".to_string(),
                    "  - anchor(h, score, why) is the uncapped durable-spine relation.".to_string(),
                    "  - ranked_anchor(h, rank, score, why) is the rank projection used by status pointers.".to_string(),
                    "Cold-start ladder: status for aggregate vital signs, recent_frontier/ranked_anchor for goal-less reading, context GOAL for focused retrieval.".to_string(),
                    "Recent-change recipes: join *handle.file to git_mtime(file, instant), or use changed_within(h, days); these are git-backed change signals, not authored age.".to_string(),
                    "History concepts:".to_string(),
                    "  - snapshots capture graph state over time for at(\"snapshot:last\") queries.".to_string(),
                    "  - generations mark source refresh epochs for atomic fact replacement.".to_string(),
                    "  - trails record per-query provenance and surfaced/consumed references.".to_string(),
                ],
                examples: vec![
                    "? schema(name, kind, signature, determinism, provenance).",
                    "? describe(\"search\", doc).",
                    "? describe(\"convergence\", doc).",
                    "? axis(name, question, oracle, disposition).",
                    "? axis_of(\"currency_suspect\", axis).",
                    "? describe(\"topic\", doc).",
                    "? examples(\"search\", example).",
                    "? *handle{status: status}, status != null.",
                    "? *edge{kind: kind}.",
                    "? *handle{id: h, file: file}, git_mtime(file, instant).",
                    "? changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}.",
                    "? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.",
                    "? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.",
                    "? flow(h, direction), *handle{id: h, summary: summary}.",
                ],
                ..DescribeCard::default()
            }),
        ));
        self.describe.insert(describe_entry(
            "check",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: "Hidden CI-gate alias for the error-only diagnostic view.",
                kind: Some(DescribeKind::RuntimeTopic),
                relationship: Some("Use eval for agent workflows; `anneal check` remains callable for CI and pre-commit gates, exits 1 when any error row exists, and is intentionally hidden from the default command surface."),
                common_joins: &[
                    "`diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}` mirrors the rows checked by the hidden CI gate",
                    "`diagnostic(code, severity, subject, file, line, evidence)` for the full diagnostic stream",
                ],
                extra_lines: vec![
                    "Canonical eval: anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'".to_string(),
                    "Exit code: `anneal check` returns 1 when error-severity diagnostics exist, 0 otherwise.".to_string(),
                    "Deprecation: hidden alias retained for CI muscle memory; prefer eval composition in agent-facing workflows.".to_string(),
                ],
                see_also: &["diagnostic", "status", "help eval"],
                examples: vec![
                    "? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.",
                ],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("runtime"),
            string_value(r#"? describe("runtime", doc)."#),
        ]));
        self.examples.insert(Tuple(vec![
            string_value("runtime"),
            string_value("? *handle{namespace: ns}, ns != \"\"."),
        ]));
        self.describe.insert(describe_entry(
            "code_path_root",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: "Project config section for extra in-repo code-reference roots scanned from markdown bodies.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some("config code_path_root { root([...]). }"),
                extra_lines: vec![
                    "Defaults already recognize crates/, lib/, src/, app/, test/, priv/, and native/.".to_string(),
                    "Build-output roots _build/, target/, and node_modules/ are always ignored.".to_string(),
                    "Recognized refs become external handles with external_class=\"code\" and ordinary Cites edges.".to_string(),
                ],
                common_joins: &[
                    "`*config{key: \"code_path_root.root\", value: root}` to inspect configured extra roots",
                    "`*meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}` to inspect captured code refs",
                ],
                examples: vec![
                    "? *config{key: \"code_path_root.root\", value: root}.",
                    "? *meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}.",
                ],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("code_path_root"),
            string_value(r#"? *config{key: "code_path_root.root", value: root}."#),
        ]));
        self.examples.insert(Tuple(vec![
            string_value("asserts_code"),
            string_value("? asserts_code(status)."),
        ]));
        self.describe.insert(describe_entry(
            "search_boost",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: "Project config section for tuning search ranking boosts by lifecycle status and hub degree.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some("config search_boost { status(\"status\", boost). hub(boost). }"),
                extra_lines: vec![
                    "Defaults boost authoritative/current/stable handles above active/review handles, and active/review handles above draft/raw handles.".to_string(),
                    "The hub boost is a bounded per-incoming-edge score bump; set hub(0) to disable it for one corpus.".to_string(),
                    "Boosts are additive score calibration, not filters: low-confidence filtering still happens after ranking.".to_string(),
                ],
                common_joins: &[
                    "`*config{key: \"search_boost.status.authoritative\", value: boost}` to inspect a status override",
                    "`*config{key: \"search_boost.hub\", value: boost}` to inspect the hub-edge override",
                    "`search{query: \"text\", handle: h, score: score}, *handle{id: h, status: status}` to see boosted statuses in ranked rows",
                ],
                examples: vec![
                    "? *config{key: \"search_boost.status.authoritative\", value: boost}.",
                    "? search{query: \"conformance\", handle: h, score: score}, *handle{id: h, status: status}.",
                ],
                see_also: &["search", "context", "schema"],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("search_boost"),
            string_value(r#"? *config{key: "search_boost.status.authoritative", value: boost}."#),
        ]));
        self.describe.insert(describe_entry(
            "external",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: "External handles mark references outside the markdown corpus boundary, including URLs, other repos, and in-repo code paths.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some(r#"*handle{kind: "external"} plus optional external_class metadata"#),
                extra_lines: vec![
                    "Document-like external refs use the same substrate kind as code refs so the graph stays small and composable.".to_string(),
                    "In-repo code refs carry standard metadata: external_class=\"code\", target_path, target_start_line, and target_end_line.".to_string(),
                    "Future code adapters can promote code refs into first-class code handles without changing today's Cites edges.".to_string(),
                ],
                common_joins: &[
                    "`*handle{id: h, kind: \"external\"}, *edge{to: h, from: src, kind: \"Cites\"}` to find who cites an external target",
                    "`*meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}` to keep code refs only",
                ],
                examples: vec![
                    "? *handle{id: h, kind: \"external\"}.",
                    "? *meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}.",
                ],
                see_also: &["external_class", "target_path", "handle", "code_path_root", "*handle", "*meta"],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("external"),
            string_value(r#"? *handle{id: h, kind: "external"}."#),
        ]));
        self.describe.insert(describe_entry(
            "external_class",
            DescribeKind::RuntimeTopic,
            &describe_card(DescribeCard {
                summary: r#"Discriminator for *handle{kind: "external"} sub-classes."#,
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some(r#"*meta{handle: h, key: "external_class", value: class}"#),
                extra_lines: vec![
                    "Known values (standard, adapter-neutral):".to_string(),
                    r#"- "code": target_path, target_start_line, target_end_line, target_exists, and target_history_status describe source-code locations."#.to_string(),
                    r#"- Future "url": target_url."#.to_string(),
                    r#"- Future "issue": target_repo and target_number."#.to_string(),
                    "A new external_class value is an anneal standard-key decision.".to_string(),
                    "Sources may emit additional source-specific discriminators in their own namespace, such as md.link_type.".to_string(),
                ],
                common_joins: &[
                    r#"`*handle{id: h, kind: "external"}, *meta{handle: h, key: "external_class", value: "code"}` to find all code-target external handles"#,
                    r#"`*meta{handle: h, key: "external_class", value: "code"}, *meta{handle: h, key: "target_path", value: path}` to add the code location"#,
                ],
                examples: vec![
                    r#"? *handle{id: h, kind: "external"}, *meta{handle: h, key: "external_class", value: "code"}."#,
                    r#"? *meta{handle: h, key: "external_class", value: "code"}, *meta{handle: h, key: "target_path", value: path}."#,
                ],
                see_also: &["*meta", "*handle", "target_path"],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("external_class"),
            string_value(r#"? *meta{handle: h, key: "external_class", value: "code"}."#),
        ]));
        for (name, summary, detail) in [
            (
                "target_path",
                r"Standard metadata key for the path an external handle points at.",
                r#"For external_class="code", this is the in-repo source path without a line range."#,
            ),
            (
                "target_start_line",
                r"Standard metadata key for the first target line an external handle points at.",
                r#"For external_class="code", this is the first line in the code location when a range was present."#,
            ),
            (
                "target_end_line",
                r"Standard metadata key for the last target line an external handle points at.",
                r#"For external_class="code", this is the inclusive end line when a range was present."#,
            ),
            (
                "target_exists",
                r"Standard metadata key for whether an external handle's target exists or confidently drifted.",
                r#"For external_class="code", this is true when present on disk, false when absent but present in HEAD history, and unknown when history cannot prove drift."#,
            ),
            (
                "target_history_status",
                r"Standard metadata key for whether a code target appears in HEAD history.",
                r#"For external_class="code", values are present, absent, or unavailable. target_exists=false is evidence-backed drift only when history status is present."#,
            ),
            (
                "target_probe_base",
                r"Standard metadata key for the base directory used to probe target existence.",
                r#"For external_class="code", this records the repository, workspace, or corpus root used to resolve target_path."#,
            ),
            (
                "target_resolved_path",
                r"Standard metadata key for the resolved on-disk target when one was found.",
                r#"For external_class="code", this records the path that made target_exists true."#,
            ),
        ] {
            let signature = format!(r#"*meta{{handle: h, key: "{name}", value: value}}"#);
            let example = format!(r#"? *meta{{handle: h, key: "{name}", value: value}}."#);
            let common_join = format!(
                r#"`*meta{{handle: h, key: "external_class", value: "code"}}, *meta{{handle: h, key: "{name}", value: value}}` to inspect code target metadata"#
            );
            self.describe.insert(describe_entry(
                name,
                DescribeKind::RuntimeTopic,
                &describe_card(DescribeCard {
                    summary,
                    kind: Some(DescribeKind::RuntimeTopic),
                    signature: Some(signature.as_str()),
                    extra_lines: vec![
                        detail.to_string(),
                        "The key is standard: anneal defines it and it has the same meaning on any corpus.".to_string(),
                    ],
                    common_joins: &[common_join.as_str()],
                    examples: vec![example.as_str()],
                    see_also: &["external_class", "*meta", "external"],
                    ..DescribeCard::default()
                }),
            ));
            self.examples
                .insert(Tuple(vec![string_value(name), string_value(&example)]));
        }
    }

    fn add_stored_relations(&mut self, dynamic_stored: Vec<StoredRelationSummary>) {
        let static_names = STORED_RELATION_DESCRIPTORS
            .iter()
            .chain(TRAIL_RELATION_DESCRIPTORS)
            .map(|relation| relation.name)
            .collect::<BTreeSet<_>>();
        for relation in STORED_RELATION_DESCRIPTORS
            .iter()
            .chain(TRAIL_RELATION_DESCRIPTORS)
        {
            self.add_stored_relation(
                relation.name,
                relation.fields,
                relation.doc,
                relation.provenance,
                relation.example,
            );
        }
        for relation in dynamic_stored {
            if static_names.contains(relation.name.as_str()) {
                continue;
            }
            self.add_stored_relation(
                &relation.name,
                &relation.fields,
                "Stored relation discovered from runtime rows.",
                "runtime",
                &fallback_stored_relation_example(&relation.name, &relation.fields),
            );
        }
    }

    fn add_stored_relation(
        &mut self,
        name: &str,
        fields: &[impl AsRef<str>],
        doc: &str,
        provenance: &str,
        example: &str,
    ) {
        self.schema.insert(schema_tuple(
            name,
            "stored",
            &stored_signature(name, fields),
            "input",
            provenance,
        ));
        let signature = stored_signature(name, fields);
        self.describe.insert(describe_entry(
            name,
            DescribeKind::StoredRelation,
            &describe_card(DescribeCard {
                summary: doc,
                kind: Some(DescribeKind::StoredRelation),
                signature: Some(&signature),
                common_joins: common_joins(name),
                see_also: stored_relation_see_also(name),
                examples: vec![example],
                extra_lines: stored_relation_extra_lines(name),
                ..DescribeCard::default()
            }),
        ));
        let star_name = format!("*{name}");
        self.describe.insert(describe_entry(
            &star_name,
            DescribeKind::StoredRelation,
            &describe_card(DescribeCard {
                summary: doc,
                kind: Some(DescribeKind::StoredRelation),
                signature: Some(&signature),
                common_joins: common_joins(name),
                see_also: stored_relation_see_also(name),
                examples: vec![example],
                extra_lines: stored_relation_extra_lines(name),
                ..DescribeCard::default()
            }),
        ));
        self.source_of.insert(Tuple(vec![
            string_value(name),
            string_value(".design/2026-05-13-corpus-runtime.md"),
            string_value("unknown"),
        ]));
        self.source_of.insert(Tuple(vec![
            string_value(&format!("*{name}")),
            string_value(".design/2026-05-13-corpus-runtime.md"),
            string_value("unknown"),
        ]));
        self.examples
            .insert(Tuple(vec![string_value(name), string_value(example)]));
        self.examples.insert(Tuple(vec![
            string_value(&format!("*{name}")),
            string_value(example),
        ]));
    }

    fn add_primitives(&mut self) {
        for primitive in PrimitivePredicate::ALL {
            let name = primitive.name();
            let signature = primitive.signature();
            self.schema.insert(schema_tuple(
                name,
                "primitive",
                &call_signature(name, signature.parameters),
                primitive_determinism(*primitive),
                "engine",
            ));
            self.describe.insert(describe_entry(
                name,
                DescribeKind::EnginePrimitive,
                &describe_card(DescribeCard {
                    summary: primitive_doc(*primitive),
                    kind: Some(DescribeKind::EnginePrimitive),
                    signature: Some(&call_signature(name, signature.parameters)),
                    relationship: primitive_relationship(*primitive),
                    common_joins: common_joins(name),
                    requires: primitive_requires(*primitive),
                    see_also: primitive_see_also(*primitive),
                    examples: primitive_example(*primitive).into_iter().collect(),
                    extra_lines: predicate_extra_lines(name),
                }),
            ));
            self.source_of.insert(Tuple(vec![
                string_value(name),
                string_value("crates/anneal-core/src/runtime/primitives.rs"),
                string_value("unknown"),
            ]));
            if let Some(example) = primitive_example(*primitive) {
                self.examples
                    .insert(Tuple(vec![string_value(name), string_value(example)]));
            }
        }
    }

    fn add_program(&mut self, program: &Program) {
        let scanned = ProgramScanner::scan(program);
        let predicate_names = scanned.predicates.keys().cloned().collect::<BTreeSet<_>>();
        self.add_predicates(scanned.predicates, &scanned.docs);
        self.add_docs(&scanned.docs, &predicate_names);

        let registry = VerbRegistry::from_ordered_program(program).unwrap_or_default();
        for entry in registry.iter() {
            self.verbs.insert(Tuple(vec![
                string_value(entry.name().as_str()),
                string_value(entry.query_source()),
                string_value(entry.doc()),
                string_value(&entry.output_schema().to_string()),
            ]));
            self.describe.insert(describe_entry(
                entry.name().as_str(),
                DescribeKind::Verb,
                &describe_card(DescribeCard {
                    summary: entry.doc(),
                    kind: Some(DescribeKind::Verb),
                    signature: Some(&format!("anneal {}", entry.name())),
                    relationship: Some(verb_relationship(entry.name().as_str())),
                    common_joins: common_joins(entry.name().as_str()),
                    see_also: verb_see_also(entry.name().as_str()),
                    examples: verb_example(entry.name().as_str()).into_iter().collect(),
                    extra_lines: vec![format!("Output schema: {}", entry.output_schema())],
                    ..DescribeCard::default()
                }),
            ));
            self.source_of.insert(Tuple(vec![
                string_value(entry.name().as_str()),
                string_value(&entry.source().location().source_name),
                string_value(&source_line_text(entry.source().location())),
            ]));
            for example in entry.examples() {
                self.examples.insert(Tuple(vec![
                    string_value(entry.name().as_str()),
                    string_value(example),
                ]));
            }
        }
    }

    fn add_diagnostic_codes(&mut self) {
        for code in DIAGNOSTIC_CODE_CARDS {
            let mut extra_lines = vec![
                format!("Diagnostic code: {}.", code.code),
                format!("Severity: {}.", code.severity),
                format!("Rule predicate: {}.", code.rule),
                format!("Evidence: {}.", code.evidence),
            ];
            extra_lines.extend(diagnostic_code_extra_lines(code.code));
            self.describe.insert(describe_entry(
                code.code,
                DescribeKind::RuntimeTopic,
                &describe_card(DescribeCard {
                    summary: code.summary,
                    kind: Some(DescribeKind::RuntimeTopic),
                    relationship: Some("Diagnostic catalog entry; query rows through `diagnostic(...)` and inspect the deriving rule predicate for structure."),
                    common_joins: code.common_joins,
                    see_also: code.see_also,
                    examples: vec![code.example],
                    extra_lines,
                    ..DescribeCard::default()
                }),
            ));
            self.examples.insert(Tuple(vec![
                string_value(code.code),
                string_value(code.example),
            ]));
            self.source_of.insert(Tuple(vec![
                string_value(code.code),
                string_value("crates/anneal-core/src/prelude/checks.dl"),
                string_value("unknown"),
            ]));
        }
    }

    fn add_query(&mut self, query: &AnalyzedQuery) {
        let mut predicates = BTreeMap::<String, PredicateInfo>::new();
        for rule in &query.query().local_rules {
            add_predicate_head(
                &mut predicates,
                &rule.head,
                RuleLayer::Inline,
                rule.origin().location(),
            );
        }
        self.add_predicates(predicates, &BTreeMap::new());
    }

    fn add_docs(&mut self, docs: &BTreeMap<String, DocInfo>, predicate_names: &BTreeSet<String>) {
        for (name, info) in docs {
            if predicate_names.contains(name) {
                continue;
            }
            let doc = axis_topic_card(name).unwrap_or_else(|| {
                if name == "convergence" {
                    convergence_topic_card()
                } else {
                    describe_card(DescribeCard {
                        summary: &info.doc,
                        kind: Some(DescribeKind::RuntimeTopic),
                        ..DescribeCard::default()
                    })
                }
            });
            self.describe
                .insert(describe_entry(name, DescribeKind::RuntimeTopic, &doc));
            for (file, line_text) in info.source_lines.iter_line_text() {
                self.source_of.insert(Tuple(vec![
                    string_value(name),
                    string_value(file),
                    string_value(&line_text),
                ]));
            }
        }
    }

    fn add_predicates(
        &mut self,
        predicates: BTreeMap<String, PredicateInfo>,
        docs: &BTreeMap<String, DocInfo>,
    ) {
        for (name, info) in predicates {
            let doc = docs
                .get(&name)
                .map_or(info.doc.as_str(), |doc| doc.doc.as_str());
            self.schema.insert(schema_tuple(
                &name,
                "derived",
                &info.signature(),
                "deterministic",
                &info.provenance(),
            ));
            for (file, line_text) in info.source_lines.iter_line_text() {
                self.predicates.insert(Tuple(vec![
                    string_value(&name),
                    string_value(doc),
                    string_value(file),
                    string_value(&line_text),
                ]));
                self.source_of.insert(Tuple(vec![
                    string_value(&name),
                    string_value(file),
                    string_value(&line_text),
                ]));
            }
            if let Some(doc_info) = docs.get(&name) {
                for (file, line_text) in doc_info.source_lines.iter_line_text() {
                    self.source_of.insert(Tuple(vec![
                        string_value(&name),
                        string_value(file),
                        string_value(&line_text),
                    ]));
                }
            }
            if let Some(example) = predicate_example(&name) {
                self.examples
                    .insert(Tuple(vec![string_value(&name), string_value(example)]));
            }
            let signature = info.signature();
            self.describe.insert(describe_entry(
                &name,
                DescribeKind::DerivedPredicate,
                &describe_card(DescribeCard {
                    summary: doc,
                    kind: Some(DescribeKind::DerivedPredicate),
                    signature: Some(&signature),
                    relationship: predicate_relationship(&name),
                    common_joins: common_joins(&name),
                    requires: predicate_requires(&name),
                    see_also: predicate_see_also(&name),
                    examples: predicate_example(&name).into_iter().collect(),
                    extra_lines: predicate_extra_lines(&name),
                }),
            ));
        }
    }

    fn finish(self) -> ProgramIntrospection {
        ProgramIntrospection {
            schema: self.schema.into_iter().collect(),
            predicates: self.predicates.into_iter().collect(),
            verbs: self.verbs.into_iter().collect(),
            describe: self.describe.into_iter().collect(),
            source_of: self.source_of.into_iter().collect(),
            examples: self.examples.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct PredicateInfo {
    name: String,
    parameters: Vec<ParameterName>,
    doc: String,
    layers: BTreeSet<RuleLayer>,
    source_lines: SourceLines,
}

impl PredicateInfo {
    fn new(head: &Head, layer: RuleLayer, location: &SourceLocation) -> Self {
        let name = head.predicate.display_name();
        let mut info = Self {
            name: name.clone(),
            parameters: head_parameter_names(head),
            doc: format!("Rule-defined predicate {name}."),
            layers: BTreeSet::new(),
            source_lines: SourceLines::default(),
        };
        info.add_source(layer, location);
        info
    }

    fn from_decl(name: &str, decl: &PredicateDecl) -> Self {
        let mut info = Self {
            name: name.to_string(),
            parameters: predicate_decl_parameters(decl).unwrap_or_default(),
            doc: format!("Rule-defined predicate {name}."),
            layers: BTreeSet::new(),
            source_lines: SourceLines::default(),
        };
        info.source_lines.add(decl.location());
        info
    }

    fn add_head(&mut self, head: &Head, layer: RuleLayer, location: &SourceLocation) {
        merge_parameter_names(&mut self.parameters, &head_parameter_names(head));
        self.add_source(layer, location);
    }

    fn apply_decl(&mut self, decl: &PredicateDecl) {
        if let Some(parameters) = predicate_decl_parameters(decl) {
            self.parameters = parameters;
        }
        self.source_lines.add(decl.location());
    }

    fn add_source(&mut self, layer: RuleLayer, location: &SourceLocation) {
        self.layers.insert(layer);
        self.source_lines.add(location);
    }

    fn provenance(&self) -> String {
        self.layers
            .iter()
            .map(|layer| match layer {
                RuleLayer::Unknown => "unknown",
                RuleLayer::Prelude => "prelude",
                RuleLayer::Project => "project",
                RuleLayer::Import => "import",
                RuleLayer::Inline => "inline",
            })
            .collect::<Vec<_>>()
            .join("+")
    }

    fn signature(&self) -> String {
        let parameters = self
            .parameters
            .iter()
            .enumerate()
            .map(|(idx, parameter)| display_parameter_name(&self.name, idx, parameter))
            .collect::<Vec<_>>();
        call_signature(&self.name, &parameters)
    }
}

fn display_parameter_name(predicate_name: &str, idx: usize, parameter: &ParameterName) -> String {
    if let ParameterName::Named(name) = parameter {
        return name.clone();
    }
    if let Some(names) = documented_parameter_names(predicate_name)
        && let Some(name) = names.get(idx)
    {
        return (*name).to_string();
    }
    format!("arg{idx}")
}

fn documented_parameter_names(predicate_name: &str) -> Option<&'static [&'static str]> {
    match predicate_name {
        "diagnostic" => Some(&["code", "severity", "subject", "file", "line", "evidence"]),
        "entropy" | "primary_entropy" => Some(&["h", "source"]),
        "potential_weight" => Some(&["source", "weight"]),
        "potential_subject" | "advancing" | "holding" | "regressed" | "re_opened" | "drifting" => {
            Some(&["h"])
        }
        "potential" | "frontier" => Some(&["h", "energy"]),
        "blocker" => Some(&["h", "energy", "source"]),
        "ranked_work" => Some(&["h", "energy", "rank"]),
        "flow" => Some(&["h", "direction"]),
        "area" => Some(&["area"]),
        "area_file_count" => Some(&["area", "files"]),
        "area_error_location_count" => Some(&["area", "code", "subject", "file", "line", "count"]),
        "area_error_count" => Some(&["area", "errors"]),
        "area_cross_edges" => Some(&["area", "cross_edges"]),
        "area_health" => Some(&["area", "grade", "files", "errors", "cross_edges"]),
        "area_frontier" => Some(&["area", "h", "score", "why"]),
        "profile_doc_corpus" | "profile_code_corpus" | "profile_issue_corpus" => Some(&["profile"]),
        _ => None,
    }
}

fn predicate_decl_parameters(decl: &PredicateDecl) -> Option<Vec<ParameterName>> {
    Some(
        decl.string_list_arg("args")?
            .into_iter()
            .map(|value| ParameterName::Named(value.to_string()))
            .collect(),
    )
}

#[derive(Clone, Debug)]
struct DocInfo {
    doc: String,
    source_lines: SourceLines,
}

impl DocInfo {
    fn from_decl(decl: &DocDecl) -> Self {
        let mut info = Self {
            doc: decl.doc().to_string(),
            source_lines: SourceLines::default(),
        };
        info.add_source(decl.location());
        info
    }

    fn replace_from_decl(&mut self, decl: &DocDecl) {
        self.doc = decl.doc().to_string();
        self.source_lines.replace_with(decl.location());
    }

    fn add_source(&mut self, location: &SourceLocation) {
        self.source_lines.add(location);
    }
}

#[derive(Clone, Debug, Default)]
struct SourceLines(BTreeMap<String, BTreeSet<usize>>);

impl SourceLines {
    fn add(&mut self, location: &SourceLocation) {
        if location.line > 0 {
            self.0
                .entry(location.source_name.clone())
                .or_default()
                .insert(location.line);
        } else {
            self.0.entry(location.source_name.clone()).or_default();
        }
    }

    fn replace_with(&mut self, location: &SourceLocation) {
        self.0.clear();
        self.add(location);
    }

    fn iter_line_text(&self) -> impl Iterator<Item = (&str, String)> {
        self.0
            .iter()
            .map(|(file, lines)| (file.as_str(), line_list(lines)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParameterName {
    Unknown,
    Named(String),
    Ambiguous,
}

fn merge_parameter_names(existing: &mut [ParameterName], observed: &[ParameterName]) {
    for (left, right) in existing.iter_mut().zip(observed) {
        if let ParameterName::Named(right_name) = right {
            match left {
                ParameterName::Unknown => {
                    *left = ParameterName::Named(right_name.clone());
                }
                ParameterName::Named(left_name) if left_name != right_name => {
                    *left = ParameterName::Ambiguous;
                }
                ParameterName::Named(_) | ParameterName::Ambiguous => {}
            }
        }
    }
}

#[derive(Default)]
struct ProgramScanner {
    docs: BTreeMap<String, DocInfo>,
    predicates: BTreeMap<String, PredicateInfo>,
}

impl ProgramScanner {
    fn scan(program: &Program) -> Self {
        let mut scanner = Self::default();
        scanner.scan_statements(&program.statements);
        scanner
    }

    fn scan_statements(&mut self, statements: &[Statement]) {
        for statement in statements {
            match statement {
                Statement::Fact(head) => {
                    add_predicate_head(
                        &mut self.predicates,
                        head,
                        RuleLayer::Unknown,
                        &head.location,
                    );
                }
                Statement::Rule(rule) => {
                    add_predicate_head(
                        &mut self.predicates,
                        &rule.head,
                        rule.origin().layer(),
                        rule.origin().location(),
                    );
                }
                Statement::AtBlock { statements, .. } => {
                    self.scan_statements(statements);
                }
                Statement::Doc(doc) => {
                    if let Some(existing) = self.docs.get_mut(doc.name()) {
                        existing.replace_from_decl(doc);
                    } else {
                        self.docs
                            .insert(doc.name().to_string(), DocInfo::from_decl(doc));
                    }
                }
                Statement::Predicate(decl) => {
                    if let Some(name) = decl.string_arg("name") {
                        self.predicates
                            .entry(name.to_string())
                            .and_modify(|info| info.apply_decl(decl))
                            .or_insert_with(|| PredicateInfo::from_decl(name, decl));
                    }
                }
                Statement::Query(_)
                | Statement::ConfigBlock(_)
                | Statement::SourceBlock(_)
                | Statement::Verb(_)
                | Statement::Include(_)
                | Statement::Import(_)
                | Statement::OptionalFact(_) => {}
            }
        }
    }
}

fn add_predicate_head(
    out: &mut BTreeMap<String, PredicateInfo>,
    head: &Head,
    layer: RuleLayer,
    location: &SourceLocation,
) {
    let name = head.predicate.display_name();
    out.entry(name)
        .and_modify(|info| info.add_head(head, layer, location))
        .or_insert_with(|| PredicateInfo::new(head, layer, location));
}

fn head_parameter_names(head: &Head) -> Vec<ParameterName> {
    head.terms
        .iter()
        .map(|term| match term {
            super::ast::Term::Expr(Expr::Var(var)) => ParameterName::Named(var.to_string()),
            _ => ParameterName::Unknown,
        })
        .collect()
}

fn source_capability_names(
    capabilities: &SourceCapabilities,
    supports_search: bool,
) -> impl Iterator<Item = &'static str> {
    [
        (capabilities.supports_git_ref, "supports_git_ref"),
        (
            capabilities.supports_time_snapshot,
            "supports_time_snapshot",
        ),
        (capabilities.supports_incremental, "supports_incremental"),
        (capabilities.live_only, "live_only"),
        (supports_search, "search"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
}

fn source_tuple(source: &SourceInfo) -> Tuple {
    Tuple(vec![
        string_value(source.name),
        list_value(source.recognizes.iter().map(|pattern| pattern.0.as_str())),
        list_value(source_capability_names(
            &source.capabilities,
            source.search.is_some(),
        )),
        string_value(source.doc),
    ])
}

fn source_line_text(location: &SourceLocation) -> String {
    if location.line == 0 {
        "unknown".to_string()
    } else {
        location.line.to_string()
    }
}

fn line_list(lines: &BTreeSet<usize>) -> String {
    if lines.is_empty() {
        return "unknown".to_string();
    }
    lines
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn schema_tuple(
    name: &str,
    kind: &str,
    signature: &str,
    determinism: &str,
    source_provenance: &str,
) -> Tuple {
    Tuple(vec![
        string_value(name),
        string_value(kind),
        string_value(signature),
        string_value(determinism),
        string_value(source_provenance),
    ])
}

fn stored_signature(name: &str, fields: &[impl AsRef<str>]) -> String {
    let fields = fields
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ");
    format!("*{name}{{{fields}}}")
}

fn call_signature(name: &str, parameters: &[impl AsRef<str>]) -> String {
    let params = parameters
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({params})")
}

fn stored_relation_extra_lines(name: &str) -> Vec<String> {
    match name {
        "meta" => vec![
            "Open metadata extension on handles. Three kinds of keys:".to_string(),
            "STANDARD (defined by anneal, same meaning on any corpus): external_class, target_path, target_start_line, target_end_line, target_exists, target_history_status, target_probe_base, target_resolved_path.".to_string(),
            "SOURCE (produced by a specific source adapter, prefix tells you which): md.resolved_file, md.parent_dir.".to_string(),
            "FRONTMATTER (passed through from YAML, corpus-defined): status, date, author, depends-on, tags, and project-specific fields.".to_string(),
            r"Discover frontmatter keys with `? *meta{handle: h, key: k}.` on your corpus.".to_string(),
        ],
        "snapshot" => vec![
            "Automatic status snapshots power `at(\"snapshot:last\")` queries; agents do not manage a snapshot command.".to_string(),
            "Retired diff equivalent: `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`.".to_string(),
            "Use raw *snapshot rows only when you need key/value history rather than an at-block composition.".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn stored_relation_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "meta" => &["external_class", "target_path", "*handle", "schema"],
        "snapshot" => &["*handle", "diagnostic", "runtime"],
        _ => &[],
    }
}

#[derive(Default)]
struct DescribeCard<'a> {
    summary: &'a str,
    kind: Option<DescribeKind>,
    signature: Option<&'a str>,
    relationship: Option<&'a str>,
    common_joins: &'a [&'a str],
    requires: &'a [&'a str],
    see_also: &'a [&'a str],
    examples: Vec<&'a str>,
    extra_lines: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct DiagnosticCodeCard {
    code: &'static str,
    severity: &'static str,
    summary: &'static str,
    rule: &'static str,
    evidence: &'static str,
    common_joins: &'static [&'static str],
    example: &'static str,
    see_also: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
struct AxisTopicCard {
    name: &'static str,
    summary: &'static str,
    question: &'static str,
    oracle: &'static str,
    disposition: &'static str,
    member_predicates: &'static str,
    common_joins: &'static [&'static str],
    examples: &'static [&'static str],
    see_also: &'static [&'static str],
}

const AXIS_TOPIC_CARDS: &[AxisTopicCard] = &[
    AxisTopicCard {
        name: "currency",
        summary: "Currency asks whether a file handle has been displaced by a marked Supersedes edge.",
        question: "displaced?",
        oracle: "old-to-new file Supersedes edges; status strings such as superseded stay on the lifecycle axis.",
        disposition: "REPORT: down-rank and annotate; never hide superseded material.",
        member_predicates: "currency_current, currency_current_head, currency_successor, currency_superseded, currency_disposition, hit_currency_disposition, orientation_replaced.",
        common_joins: &[
            "`currency_disposition(h, disposition), *handle{id: h, file: file, status: status}` to read displacement beside lifecycle status",
            "`currency_current_head(h), operative(h), *handle{id: h, file: file}` for boosted current heads",
            "`currency_superseded(h), *edge{from: h, to: newer, kind: \"Supersedes\"}` to inspect the replacement edge",
        ],
        examples: &[
            "? currency_disposition(h, disposition), *handle{id: h, file: file, status: status}.",
            "? currency_current_head(h), operative(h), *handle{id: h, file: file}.",
            "? axis_of(\"currency_current_head\", axis).",
        ],
        see_also: &["lifecycle", "structure", "ranked_anchor"],
    },
    AxisTopicCard {
        name: "lifecycle",
        summary: "Lifecycle asks where a handle sits in the corpus status band: draft, operative, retired, or project-specific equivalents.",
        question: "draft, operative, or retired?",
        oracle: "source status values interpreted through project convergence config and lifecycle helpers.",
        disposition: "REPORT / PRE-FLIGHT: report observed status; declare missing config or missing status before relying on lifecycle-sensitive claims.",
        member_predicates: "status_of, operative, lifecycle_status_candidate, orientation_retired_status, asserts_code, aspirational_code_status, frontmatter_adoption_high.",
        common_joins: &[
            "`status_of(h, status), *handle{id: h, file: file}` to inspect source-provided status",
            "`operative(h), *handle{id: h, file: file}` for handles eligible for current-head ranking boosts",
            "`lifecycle_config_gap(status, count, variant), diagnostic(\"W005\", severity, status, file, line, evidence)` for config evidence",
        ],
        examples: &[
            "? status_of(h, status), *handle{id: h, file: file}.",
            "? operative(h), *handle{id: h, file: file}.",
            "? axis_of(\"operative\", axis).",
        ],
        see_also: &["currency", "convergence", "diagnostic"],
    },
    AxisTopicCard {
        name: "recency",
        summary: "Recency asks when a handle was authored, changed, or observed, while keeping the three clocks separate.",
        question: "authored, changed, or observed when?",
        oracle: "date-backed authored_age for authored age, git mtime for lower-authority change recency, snapshots for observed history.",
        disposition: "REPORT; flux and snapshot comparisons are TREND because they need a baseline.",
        member_predicates: "authored_age, changed_recently, snapshot_history_exists, snapshot_history_present; primitives include freshness, changed_within, git_mtime, flux.",
        common_joins: &[
            "`authored_age(h, days), *handle{id: h, file: file}` for date-backed age",
            "`changed_recently(h, band), *handle{id: h, file: file}` for coarse git-backed change recency",
            "`at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now` for observed status movement",
        ],
        examples: &[
            "? authored_age(h, days), *handle{id: h, file: file}.",
            "? changed_recently(h, band), *handle{id: h, file: file}.",
            "? axis_of(\"authored_age\", axis).",
        ],
        see_also: &["recent_frontier", "convergence", "runtime"],
    },
    AxisTopicCard {
        name: "relevance",
        summary: "Relevance asks whether a handle or span matches the current query.",
        question: "matches my query?",
        oracle: "text and query scored by the ranker/search provider.",
        disposition: "REPORT: relevance scores inform retrieval, not corpus validity.",
        member_predicates: "search and match are primitives; verb-local search/context rows project this axis into product surfaces.",
        common_joins: &[
            "`search(query, h, span_id, score, reason, field, low_confidence), *handle{id: h, file: file}` for raw hit evidence",
            "`search{query: \"TERM\", handle: h, score: score}, *span{handle: h, id: span_id, summary: summary}` for span summaries",
            "`axis_of(p, \"composition\")` to inspect rankers that combine relevance with other axes",
        ],
        examples: &[
            "? search{query: \"convergence\", handle: h, score: score}.",
            "? axis(\"relevance\", question, oracle, disposition).",
        ],
        see_also: &["search", "context", "importance"],
    },
    AxisTopicCard {
        name: "importance",
        summary: "Importance asks how central a handle is in the graph.",
        question: "how central?",
        oracle: "degree, citations, impact, and neighborhood graph primitives over current edges.",
        disposition: "REPORT: centrality changes ranking and navigation, not validity.",
        member_predicates: "hub, incoming_edge, outgoing_edge, incident, orientation_inbound_count; primitives include cite_count, in_degree, out_degree, impact, neighborhood, upstream, downstream.",
        common_joins: &[
            "`incoming_edge(h, from, kind), *handle{id: h, file: file}` for inbound evidence",
            "`out_degree(h, degree), *handle{id: h, file: file}` for broad hubs",
            "`hub(h, degree), *handle{id: h, file: file}` to spot maps or over-broad index handles",
        ],
        examples: &[
            "? hub(h, degree), *handle{id: h, file: file}.",
            "? incoming_edge(h, from, kind), *handle{id: h, file: file}.",
            "? axis_of(\"hub\", axis).",
        ],
        see_also: &["structure", "relevance", "context"],
    },
    AxisTopicCard {
        name: "structure",
        summary: "Structure asks how corpus handles are organized and connected.",
        question: "organized or connected?",
        oracle: "stored edges plus adapter-provided areas, namespaces, sections, and pipeline structure.",
        disposition: "REPORT: structure orients navigation; diagnostics decide when a structural fact becomes a gate.",
        member_predicates: "area_of, namespace_of, handle_file, section_ref, area_health, area_frontier, parent_dir_* and namespace_* helpers, top_pair, orphan, stub.",
        common_joins: &[
            "`area_of(h, area), *handle{id: h, file: file}` to group handles by source area",
            "`namespace_of(h, namespace), *handle{id: h, kind: kind}` to inspect label families",
            "`section_ref_edge(edge_id), *edge{id: edge_id, from: src, to: dst, kind: kind}` for markdown section-reference evidence",
        ],
        examples: &[
            "? area_health(area, grade, files, errors, cross_edges).",
            "? namespace_of(h, namespace), *handle{id: h, kind: kind}.",
            "? axis_of(\"area_of\", axis).",
        ],
        see_also: &["importance", "diagnostic", "area_health"],
    },
    AxisTopicCard {
        name: "obligations",
        summary: "Obligations ask what has been promised and whether the corpus records a discharge.",
        question: "owed?",
        oracle: "obligation and discharge facts over handles.",
        disposition: "GATE-able through E002: undischarged obligations are allowed to block release or review gates.",
        member_predicates: "undischarged_obligation, multiple_discharge; primitives include obligation, discharged, undischarged, discharge_count.",
        common_joins: &[
            "`undischarged(h), obligation(h), *handle{id: h, file: file, status: status}` for owed work",
            "`multiple_discharge(h, count), diagnostic(\"W003\", severity, h, file, line, evidence)` for duplicate discharge evidence",
            "`axis_of(\"undischarged_obligation\", axis)` to inspect obligation-axis placement",
        ],
        examples: &[
            "? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.",
            "? multiple_discharge(h, count).",
            "? axis_of(\"undischarged_obligation\", axis).",
        ],
        see_also: &["convergence", "diagnostic", "check"],
    },
    AxisTopicCard {
        name: "topic",
        summary: "Topic asks whether two files are likely on the same subject through shared discriminative citation targets.",
        question: "same subject?",
        oracle: "pairwise shared Cites targets after excluding curated inventory handles and mega-targets.",
        disposition: "REPORT: annotate possible topical relation; never assert an edge or hidden supersession.",
        member_predicates: "topic_citation_target, topic_target_citation_count, topic_mega_target_cap, topic_nondiscriminative_target, topic_shared_target, topic_pair, topic_sibling.",
        common_joins: &[
            "`topic_sibling(a, b, shared), *handle{id: a, file: left}, *handle{id: b, file: right}` to inspect same-subject file pairs",
            "`topic_nondiscriminative_target(t), topic_target_citation_count(t, n)` to see why broad targets are excluded",
            "`topic_pair(left, right, shared)` when you need canonical pair rows without symmetric duplicates",
        ],
        examples: &[
            "? topic_sibling(a, b, shared), shared >= 2.",
            "? topic_nondiscriminative_target(t), topic_target_citation_count(t, n).",
            "? axis_of(\"topic_sibling\", axis).",
        ],
        see_also: &["currency", "importance", "structure", "context"],
    },
];

fn axis_topic_card(name: &str) -> Option<String> {
    let card = AXIS_TOPIC_CARDS.iter().find(|card| card.name == name)?;
    Some(describe_card(DescribeCard {
        summary: card.summary,
        kind: Some(DescribeKind::RuntimeTopic),
        relationship: Some(
            "Axis card from CR-D104: use `axis` for the machine question/oracle/disposition row and `axis_of` to place predicates.",
        ),
        common_joins: card.common_joins,
        examples: card.examples.to_vec(),
        see_also: card.see_also,
        extra_lines: vec![
            format!("Question: {}", card.question),
            format!("Oracle: {}", card.oracle),
            format!("Disposition: {}", card.disposition),
            format!("Member predicates: {}", card.member_predicates),
            "Placement categories outside axes: composition, diagnostic, infrastructure."
                .to_string(),
        ],
        ..DescribeCard::default()
    }))
}

fn convergence_topic_card() -> String {
    describe_card(DescribeCard {
        summary: "Convergence is anneal's physics: corpus facts create energy, energy creates a frontier, agents do work, and snapshots show whether the landscape is flattening.",
        kind: Some(DescribeKind::RuntimeTopic),
        relationship: Some("This topic names the act as well as the vocabulary. Use `status` for a landing view, then compose `potential`, `frontier`, `blocker`, diagnostics, and `flow` in eval."),
        common_joins: &[
            "`potential(h, energy), primary_entropy(h, source)` to see why a handle has energy",
            "`frontier(h, energy), *handle{id: h, file: file, summary: summary}` for the global work frontier",
            "`blocker(h, energy, source), primary_entropy(h, source)` for one blocker reason per handle",
            "`flow(h, direction), *handle{id: h, status: status}` to inspect convergence flow",
        ],
        extra_lines: vec![
            "The Act: agents dissipate potential by editing corpus facts, then rerun status/check to verify energy moved.".to_string(),
            "Vocabulary: entropy is an unsettled signal; potential is weighted energy; frontier is the highest-energy projection; blocker is stalled energy; flow is advancing, holding, or drifting.".to_string(),
            "Flow: settled handles are outside flow by design; regressed(h) and re_opened(h) explain drifting(h) leaves.".to_string(),
            "Tuning: project rules can shadow `potential_weight(source, weight)` to retune convergence energy.".to_string(),
        ],
        requires: &["snapshot history for flow predicates that compare at(\"snapshot:last\") with the current graph."],
        see_also: &[
            "status",
            "potential",
            "frontier",
            "blocker",
            "flow",
            "potential_weight",
        ],
        examples: vec![
            "? frontier(h, energy), primary_entropy(h, source).",
            "? flow(h, direction), *handle{id: h, summary: summary}.",
            "? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.",
        ],
        ..DescribeCard::default()
    })
}

const DIAGNOSTIC_CODE_CARDS: &[DiagnosticCodeCard] = &[
    DiagnosticCodeCard {
        code: "E001",
        severity: "error",
        summary: "Broken reference: a corpus edge points at a handle that does not exist.",
        rule: "broken_reference",
        evidence: r#"("broken_ref", target)"#,
        common_joins: &[
            "`diagnostic{code: \"E001\", subject: src}, broken_reference(src, target, file, line)` to inspect the missing target",
            "`broken_reference(src, target, file, line), read{handle: src, budget: 1200, text: text}` to read the source context",
        ],
        example: r#"? diagnostic{code: "E001", severity: severity, subject: src}."#,
        see_also: &["diagnostic", "broken_reference", "W004"],
    },
    DiagnosticCodeCard {
        code: "E002",
        severity: "error",
        summary: "Undischarged obligation: a live obligation handle has no Discharges edge.",
        rule: "undischarged_obligation",
        evidence: r#""undischarged""#,
        common_joins: &[
            "`diagnostic{code: \"E002\", subject: h}, undischarged_obligation(h, file)` to inspect open obligations",
            "`undischarged_obligation(h, file), area_of{h: h, area: area}` to group open obligations by area",
        ],
        example: r#"? diagnostic{code: "E002", subject: h, file: file}."#,
        see_also: &[
            "diagnostic",
            "undischarged_obligation",
            "undischarged",
            "I002",
        ],
    },
    DiagnosticCodeCard {
        code: "W001",
        severity: "warning",
        summary: "Stale reference: an active handle depends on a terminal handle.",
        rule: "stale_reference",
        evidence: r#"("stale_ref", source_status, target_status)"#,
        common_joins: &[
            "`diagnostic{code: \"W001\", subject: src}, stale_reference(src, target, file, source_status, target_status)` to inspect the stale edge",
            "`stale_reference(src, target, file, source_status, target_status), *handle{id: target, summary: summary}` to add target context",
        ],
        example: r#"? diagnostic{code: "W001", subject: src, file: file}."#,
        see_also: &["diagnostic", "stale_reference", "W002"],
    },
    DiagnosticCodeCard {
        code: "W002",
        severity: "warning",
        summary: "Confidence gap: a dependency target is behind its source in the configured lifecycle order.",
        rule: "confidence_gap",
        evidence: r#"("confidence_gap", source_status, source_level, target_status, target_level)"#,
        common_joins: &[
            "`diagnostic{code: \"W002\", subject: src}, confidence_gap(src, target, file, source_status, source_level, target_status, target_level)` to inspect lifecycle levels",
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), area_of{h: src, area: area}` to group gaps by area",
        ],
        example: r#"? diagnostic{code: "W002", subject: src, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "confidence_gap",
            "configured_pipeline_status",
            "W001",
        ],
    },
    DiagnosticCodeCard {
        code: "W003",
        severity: "warning",
        summary: "Missing frontmatter: a file lacks status frontmatter in a directory where frontmatter is otherwise established.",
        rule: "missing_frontmatter_file",
        evidence: "null",
        common_joins: &[
            "`diagnostic{code: \"W003\", subject: h}, missing_frontmatter_file(h, dir, file)` to inspect the missing metadata",
            "`missing_frontmatter_file(h, dir, file), area_of{h: h, area: area}` to group missing metadata by area",
        ],
        example: r#"? diagnostic{code: "W003", subject: h, file: file}."#,
        see_also: &["diagnostic", "missing_frontmatter_file", "W004"],
    },
    DiagnosticCodeCard {
        code: "W004",
        severity: "warning",
        summary: "Implausible reference: markdown extraction saw a reference-like token that was rejected as implausible.",
        rule: "implausible_ref",
        evidence: r#"("implausible_ref", value)"#,
        common_joins: &[
            "`diagnostic{code: \"W004\", subject: h}, implausible_ref(h, file, value)` to inspect rejected tokens",
            "`implausible_ref(h, file, value), read{handle: h, budget: 1200, text: text}` to read the local context",
        ],
        example: r#"? diagnostic{code: "W004", subject: h, evidence: evidence}."#,
        see_also: &["diagnostic", "implausible_ref", "E001", "W003"],
    },
    DiagnosticCodeCard {
        code: "W005",
        severity: "warning",
        summary: "Lifecycle config gap: a status appears in handles or ordering without a matching active/terminal partition, or the ordering cannot terminate.",
        rule: "lifecycle_config_gap",
        evidence: r#"("lifecycle_config_gap", status, count, variant)"#,
        common_joins: &[
            "`diagnostic{code: \"W005\", subject: status}, lifecycle_config_gap(status, count, variant)` to inspect lifecycle config drift",
            "`lifecycle_config_gap(status, count, variant), configured_pipeline_status(status, level)` to compare against ordering",
        ],
        example: r#"? diagnostic{code: "W005", subject: status, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "lifecycle_config_gap",
            "configured_pipeline_status",
            "pipeline_stall",
        ],
    },
    DiagnosticCodeCard {
        code: "W006",
        severity: "warning",
        summary: "Spec-code drift: a spec that asserts current code cites a path that existed in HEAD history but is now missing on disk.",
        rule: "spec_code_drift",
        evidence: r#"("spec_code_drift", target_path, source_status)"#,
        common_joins: &[
            "`diagnostic{code: \"W006\", subject: src}, spec_code_drift(src, target_path, file, line, source_status)` to inspect the missing code target",
            "`spec_code_drift(src, target_path, file, line, source_status), read{handle: src, budget: 1200, text: text}` to read the live spec context",
            "`spec_code_drift(src, target_path, file, line, source_status), asserts_code(source_status)` to inspect the lifecycle gate",
            "`spec_code_drift(src, target_path, file, line, source_status), *edge{from: src, to: ref, kind: \"Cites\"}, *meta{handle: ref, key: \"target_history_status\", value: \"present\"}` to audit history evidence",
        ],
        example: r#"? diagnostic{code: "W006", subject: src, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "spec_code_drift",
            "asserts_code",
            "target_exists",
            "target_history_status",
            "external_class",
        ],
    },
    DiagnosticCodeCard {
        code: "I001",
        severity: "info",
        summary: "Section references present: section-reference placeholders exist and are counted separately from broken handles.",
        rule: "section_ref_total",
        evidence: r#"("section_refs", count)"#,
        common_joins: &[
            "`diagnostic{code: \"I001\", evidence: evidence}` to see whether section references were counted",
            "`section_ref_total(count), diagnostic{code: \"I001\"}` to inspect the section-reference total",
        ],
        example: r#"? diagnostic{code: "I001", evidence: evidence}."#,
        see_also: &["diagnostic", "section_ref_total", "E001"],
    },
    DiagnosticCodeCard {
        code: "I002",
        severity: "info",
        summary: "Multiple discharges: a live obligation has more than one Discharges edge.",
        rule: "multiple_discharge",
        evidence: r#"("multiple_discharges", count)"#,
        common_joins: &[
            "`diagnostic{code: \"I002\", subject: h}, multiple_discharge(h, file, count)` to inspect redundant discharges",
            "`multiple_discharge(h, file, count), discharge_count(h, n)` to compare the reported count",
        ],
        example: r#"? diagnostic{code: "I002", subject: h, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "multiple_discharge",
            "E002",
            "discharge_count",
        ],
    },
    DiagnosticCodeCard {
        code: "S001",
        severity: "suggestion",
        summary: "Orphaned handle: a label or version handle has no incoming references.",
        rule: "orphaned_handle",
        evidence: r#"("orphaned_handle", h)"#,
        common_joins: &[
            "`diagnostic{code: \"S001\", subject: h}, orphaned_handle(h)` to inspect orphaned handles",
            "`orphaned_handle(h), *handle{id: h, namespace: namespace}` to group orphans by namespace",
        ],
        example: r#"? diagnostic{code: "S001", subject: h, file: file}."#,
        see_also: &["diagnostic", "orphaned_handle", "orphan", "S004"],
    },
    DiagnosticCodeCard {
        code: "S003",
        severity: "suggestion",
        summary: "Pipeline stall: snapshot history shows a lifecycle status accumulating without movement to the next configured status.",
        rule: "pipeline_stall",
        evidence: r#"("pipeline_stall", status, count, next_status, based_on_history)"#,
        common_joins: &[
            "`diagnostic{code: \"S003\", subject: status}, pipeline_stall(status, count, next_status, based_on_history)` to inspect the stalled status",
            "`snapshot_history_present(count), pipeline_stall(status, stalled, next_status, true)` to confirm automatic status snapshots have accrued",
        ],
        example: r#"? diagnostic{code: "S003", subject: status, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "pipeline_stall",
            "advancing",
            "snapshot_history_present",
            "W005",
        ],
    },
    DiagnosticCodeCard {
        code: "S004",
        severity: "suggestion",
        summary: "Abandoned namespace: an active namespace's members are all terminal or stale.",
        rule: "abandoned_namespace",
        evidence: r#"("abandoned_namespace", namespace, total, terminal_count, stale_count)"#,
        common_joins: &[
            "`diagnostic{code: \"S004\", subject: namespace}, abandoned_namespace(namespace, total, terminal_count, stale_count)` to inspect the namespace",
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), namespace_label(namespace, h)` to inspect members",
        ],
        example: r#"? diagnostic{code: "S004", subject: namespace, evidence: evidence}."#,
        see_also: &["diagnostic", "abandoned_namespace", "S001", "freshness"],
    },
    DiagnosticCodeCard {
        code: "S005",
        severity: "suggestion",
        summary: "Concern-group candidate: two label namespaces frequently co-occur and may deserve a configured concern group.",
        rule: "top_pair",
        evidence: r#"("concern_group_candidate", left_prefix, right_prefix, count)"#,
        common_joins: &[
            "`diagnostic{code: \"S005\", subject: left_prefix}, top_pair(left_prefix, right_prefix, count)` to inspect candidate concern groups",
            "`top_pair(left_prefix, right_prefix, count), same_concern_pair(left_prefix, right_prefix)` to test whether a concern already covers it",
        ],
        example: r#"? diagnostic{code: "S005", subject: left_prefix, evidence: evidence}."#,
        see_also: &["diagnostic", "top_pair", "*concern", "same_concern_pair"],
    },
];

fn describe_card(card: DescribeCard<'_>) -> String {
    // `describe(name, doc)` is the prose teaching surface. Machine callers should
    // use schema/source_of/examples for the same facts as structured relations.
    let mut lines = Vec::new();
    lines.push(card.summary.trim().to_string());
    if let Some(kind) = card.kind {
        lines.push(format!("Kind: {}.", kind.label()));
    }
    if let Some(signature) = card.signature {
        lines.push(format!("Signature: {signature}."));
    }
    if let Some(relationship) = card.relationship {
        lines.push(format!("Relationship: {relationship}"));
    }
    if !card.common_joins.is_empty() {
        lines.push("Common joins:".to_string());
        for join in card.common_joins {
            lines.push(format!("- {}", with_output_shape(join)));
        }
    }
    lines.extend(card.extra_lines);
    for requirement in card.requires {
        lines.push(format!("Requires: {requirement}"));
    }
    if !card.see_also.is_empty() {
        lines.push(format!("See also: {}.", card.see_also.join(", ")));
    }
    for example in card.examples {
        lines.push(format!("Example: {}", with_output_shape(example)));
    }
    lines.join("\n")
}

fn with_output_shape(text: &str) -> String {
    let columns = projected_columns(text);
    if columns.is_empty() {
        return text.to_string();
    }
    format!("{text} -> Output: {}", columns.join(", "))
}

fn projected_columns(text: &str) -> Vec<String> {
    let fragment = query_fragment(text).trim();
    if !is_output_shape_candidate(fragment) {
        return Vec::new();
    }
    let fragment = strip_string_literals(fragment);
    let chars = fragment.chars().collect::<Vec<_>>();
    let mut columns = Vec::<String>::new();
    let mut index = 0;
    while index < chars.len() {
        if !is_ident_start(chars[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < chars.len() && is_ident_continue(chars[index]) {
            index += 1;
        }
        let token = chars[start..index].iter().collect::<String>();
        let next = next_non_ws(&chars, index);
        if matches!(next, Some(':' | '(' | '{')) || is_reserved_token(&token) {
            continue;
        }
        if !columns.iter().any(|column| column == &token) {
            columns.push(token);
        }
    }
    columns
}

fn is_output_shape_candidate(fragment: &str) -> bool {
    !fragment.starts_with("anneal ")
        && (fragment.starts_with('?')
            || fragment.starts_with('*')
            || fragment.contains(":=")
            || fragment.contains('{')
            || fragment.contains('('))
}

fn query_fragment(text: &str) -> &str {
    if let Some(start) = text.find('`')
        && let Some(end) = text[start + 1..].find('`')
    {
        return &text[start + 1..start + 1 + end];
    }
    text
}

fn strip_string_literals(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            out.push(' ');
        } else if ch == '"' {
            in_string = true;
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn next_non_ws(chars: &[char], index: usize) -> Option<char> {
    chars
        .iter()
        .skip(index)
        .copied()
        .find(|ch| !ch.is_whitespace())
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_reserved_token(token: &str) -> bool {
    matches!(
        token,
        "not"
            | "in"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "matches"
            | "true"
            | "false"
            | "null"
    )
}

fn primitive_determinism(primitive: PrimitivePredicate) -> &'static str {
    match primitive {
        PrimitivePredicate::Search => "ranker-dependent deterministic",
        _ => "deterministic",
    }
}

fn primitive_doc(primitive: PrimitivePredicate) -> &'static str {
    match primitive {
        PrimitivePredicate::Upstream => {
            "Find handles that the starting handle depends on, following incoming dependency-style edges through the graph."
        }
        PrimitivePredicate::Downstream => {
            "Find handles that depend on the starting handle, following outgoing dependency-style edges through the graph."
        }
        PrimitivePredicate::Impact => {
            "Find graph nodes that could be affected if a handle changes, with the number of hops from the starting handle."
        }
        PrimitivePredicate::Neighborhood => {
            "Find handles near a starting handle within a bounded number of graph hops."
        }
        PrimitivePredicate::Terminal => {
            "Return handles whose status means the work is done, archived, rejected, or otherwise no longer active."
        }
        PrimitivePredicate::Active => {
            "Return handles whose status means an agent may still need to read, change, or resolve them."
        }
        PrimitivePredicate::Settled => {
            "Return handles that the corpus considers resolved enough to use as stable context."
        }
        PrimitivePredicate::PipelinePosition => {
            "Return the numeric order for a handle's status, so queries can compare whether one status is ahead of another."
        }
        PrimitivePredicate::PipelinePositionFor => {
            "Return the numeric order for a status value, so queries can compare lifecycle progress."
        }
        PrimitivePredicate::Obligation => {
            "Return labels in namespaces that the project treats as obligations: promises, questions, requirements, or tasks that must be discharged."
        }
        PrimitivePredicate::Discharged => {
            "Return handles that already have at least one incoming Discharges edge."
        }
        PrimitivePredicate::Undischarged => {
            "Return obligations that still need a Discharges edge and are not terminal."
        }
        PrimitivePredicate::CiteCount => "Count incoming Cites edges for each handle.",
        PrimitivePredicate::InDegree => "Count all incoming graph edges for each handle.",
        PrimitivePredicate::OutDegree => "Count all outgoing graph edges for each handle.",
        PrimitivePredicate::DischargeCount => "Count incoming Discharges edges for each handle.",
        PrimitivePredicate::Freshness => {
            "Return how many days have passed since a handle's dated observation at the active time reference."
        }
        PrimitivePredicate::Flux => {
            "Count status changes for a handle over a recent day window, using snapshot history."
        }
        PrimitivePredicate::GitMtime => {
            "Return the latest git commit timestamp observed for a tracked corpus file."
        }
        PrimitivePredicate::ChangedWithin => {
            "Return handles whose backing file changed within a bound number of days according to git history."
        }
        PrimitivePredicate::TokenEstimate => {
            "Return the estimated number of stored content tokens for a handle."
        }
        PrimitivePredicate::Search => {
            "Search handle identities, metadata, headings, and content text, returning ranked span-granular hits with heading ids, reasons, and calibrated scores."
        }
        PrimitivePredicate::Read => {
            "Read content spans for one handle, optionally narrowed to the exact span_id returned by search or context."
        }
        PrimitivePredicate::ReadFull => {
            "Read all stored content for one handle. This bypasses the normal budget guard and requires the read_full capability."
        }
        PrimitivePredicate::Match => {
            "Run a regular expression against stored content for one already-bound handle and return matching lines."
        }
        PrimitivePredicate::Schema => {
            "List queryable stored relations, derived predicates, and engine primitives. The signature column is both the positional argument order and the accepted named-call parameter set."
        }
        PrimitivePredicate::Predicates => {
            "List rule-defined predicates with documentation and source locations."
        }
        PrimitivePredicate::Verbs => {
            "List declared verbs with query, documentation, and output schema."
        }
        PrimitivePredicate::Describe => {
            "Return documentation for a relation, predicate, primitive, verb, source, or runtime topic."
        }
        PrimitivePredicate::SourceOf => {
            "Return source file and line information for queryable runtime names."
        }
        PrimitivePredicate::Examples => "Return worked query examples for runtime names.",
        PrimitivePredicate::Sources => {
            "List linked adapters with recognition patterns, capabilities, and documentation."
        }
    }
}

fn primitive_requires(primitive: PrimitivePredicate) -> &'static [&'static str] {
    match primitive {
        PrimitivePredicate::Obligation | PrimitivePredicate::Undischarged => &[
            "`config handles { linear([...]). }` in anneal.dl. Without a linear namespace policy, no labels become obligations.",
        ],
        PrimitivePredicate::Flux => {
            &["snapshot history. On a corpus with no snapshots, status-change counts are zero."]
        }
        PrimitivePredicate::GitMtime | PrimitivePredicate::ChangedWithin => &[
            "git metadata supplied by the runtime host. Untracked files and non-git corpora produce no rows.",
        ],
        PrimitivePredicate::ReadFull => &[
            "the read_full runtime capability. Prefer read(handle, budget, ...) unless the full file is intentional.",
        ],
        PrimitivePredicate::Match => &[
            "the handle argument must already be bound; match does not scan the whole corpus by itself.",
        ],
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::TokenEstimate
        | PrimitivePredicate::Search
        | PrimitivePredicate::Read
        | PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => &[],
    }
}

fn primitive_relationship(primitive: PrimitivePredicate) -> Option<&'static str> {
    match primitive {
        PrimitivePredicate::Search => Some(
            "The `search` verb wraps this primitive with TopK ranking, filters out low-confidence hits by default, and joins span hits to heading-path metadata. Scores include lexical strength plus configured status and hub boosts.",
        ),
        PrimitivePredicate::Read => Some(
            "The `read` verb wraps this primitive with typed CLI arguments for handle, budget, and targeted span reads; use a search hit's span_id to read the matched section body.",
        ),
        PrimitivePredicate::ChangedWithin => Some(
            "Lower-authority change-recency primitive over git file mtimes. Join `*handle{kind: \"file\"}` when you want one row per changed file; use `authored_age` when you need date-backed age.",
        ),
        PrimitivePredicate::GitMtime => Some(
            "Raw git timestamp primitive used by `changed_within`; compose it directly when you need exact commit times. Bulk commits can make this a degraded change oracle, so it is not authored age.",
        ),
        PrimitivePredicate::Schema => Some("The `schema` verb projects this primitive directly."),
        PrimitivePredicate::Verbs => {
            Some("Use `schema` for the verb catalog and `describe NAME` for a verb teaching card.")
        }
        PrimitivePredicate::Describe => {
            Some("The `describe` verb projects this primitive as teaching cards.")
        }
        PrimitivePredicate::SourceOf => {
            Some("The `source-of` verb projects this primitive directly.")
        }
        PrimitivePredicate::Examples => Some("`describe NAME` shows these examples inline."),
        PrimitivePredicate::Sources => Some(
            "Query this primitive directly with `anneal -e '? sources(name, recognizes, capabilities, doc).'`.",
        ),
        _ => None,
    }
}

fn primitive_see_also(primitive: PrimitivePredicate) -> &'static [&'static str] {
    match primitive {
        PrimitivePredicate::Search => &["search", "context", "read", "describe"],
        PrimitivePredicate::Read | PrimitivePredicate::ReadFull => &["read", "*content", "*span"],
        PrimitivePredicate::Schema => &["describe", "examples"],
        PrimitivePredicate::Describe => &["schema", "examples"],
        PrimitivePredicate::Examples => &["describe", "schema"],
        PrimitivePredicate::GitMtime | PrimitivePredicate::ChangedWithin => {
            &["*handle", "freshness"]
        }
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact => {
            &["incoming_edge", "outgoing_edge", "neighborhood", "*edge"]
        }
        PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::DischargeCount => &["diagnostic", "blocked", "*config"],
        _ => &[],
    }
}

fn predicate_requires(name: &str) -> &'static [&'static str] {
    match name {
        "entropy" | "primary_entropy" | "potential_subject" | "potential" | "frontier"
        | "ranked_work" => &[
            "stored handles plus the relevant diagnostic, obligation, lifecycle, freshness, or graph facts that create unsettled-work signals.",
        ],
        "entropy_priority" => {
            &["`potential_weight` rows for the same source; lower priority values win ties."]
        }
        "area"
        | "area_file_count"
        | "area_error_location_count"
        | "area_error_count"
        | "area_cross_edges"
        | "area_health"
        | "area_frontier" => &[
            "`area_of` rows from source facts. Area health also uses diagnostics, edges, and potential convergence signals.",
        ],
        "blocked" | "blocker" => {
            &["active lifecycle config, at least one potential signal, and no recent status flux."]
        }
        "broken_reference" => {
            &["stored edges and handles; section-reference placeholders are excluded."]
        }
        "stale_reference" | "confidence_gap" => {
            &["DependsOn edges plus lifecycle status facts for both source and target handles."]
        }
        "undischarged_obligation" | "multiple_discharge" => {
            &["linear namespace policy in anneal.dl plus Discharges edge counts."]
        }
        "implausible_ref" => {
            &["markdown extraction metadata for references rejected by the plausibility filter."]
        }
        "lifecycle_config_gap" => {
            &["handle statuses plus `config convergence` active, terminal, and ordering entries."]
        }
        "missing_frontmatter_file" => &[
            "parent-directory metadata and enough neighboring frontmatter adoption to make the omission suspicious.",
        ],
        "orphaned_handle" => &["label or version handles plus graph in-degree counts."],
        "pipeline_stall" | "s003_pipeline_stall" => &[
            "configured lifecycle ordering, current status population, and automatic snapshot history.",
        ],
        "abandoned_namespace" | "s004_abandoned_namespace" => {
            &["active namespace membership, lifecycle status, and freshness."]
        }
        "top_pair" | "s005_top_pair" => {
            &["namespace co-occurrence in file references plus configured concern groups."]
        }
        "advancing"
        | "recently_advanced"
        | "holding"
        | "regressed"
        | "re_opened"
        | "drifting"
        | "flow"
        | "snapshot_history_present" => &[
            "snapshot history and configured lifecycle ordering. On a corpus with no snapshots, these predicates return no rows.",
        ],
        "recent_frontier" => &[
            "date-backed authored_age is the dominant clock, with git-backed changed_recently only as a coarse lower-authority no-date fallback. Terminal and superseded files are excluded; statusless files remain eligible.",
        ],
        "anchor" => &[
            "file handles plus authority, curated-name, incoming-edge, and weak recency signals. Terminal files need an explicit authoritative-style status to remain eligible.",
        ],
        "ranked_anchor" => &[
            "the uncapped anchor(h, score, why) relation. Use rank filters for score tiers and --limit for display budgets.",
        ],
        "configured_pipeline_status"
        | "next_pipeline_status"
        | "status_population"
        | "previous_status_population" => {
            &["`config convergence { ordering([...]). }` in anneal.dl."]
        }
        _ => &[],
    }
}

fn predicate_relationship(name: &str) -> Option<&'static str> {
    match name {
        "diagnostic" => Some(
            "Shared diagnostic stream used by `status`, `check`, and eval diagnostics; individual rules contribute rows by diagnostic code.",
        ),
        "potential" => Some(
            "Canonical raw-energy predicate for handles an agent could improve; use `frontier` for the capped top projection.",
        ),
        "potential_weight" => Some(
            "Default calibration table. Project rules may shadow this predicate by name and arity to retune convergence energy.",
        ),
        "frontier" => Some(
            "Canonical global convergence frontier; paired with `area_frontier` for area-scoped work.",
        ),
        "recent_frontier" => Some(
            "Goal-less orientation frontier: date-backed authored-recent files a cold agent should inspect first, with only coarse lower-authority git change bands for undated files. Unlike `frontier`, this is about reading orientation, not potential work energy.",
        ),
        "anchor" => Some(
            "Goal-less orientation anchors: durable read-first files such as authoritative models, living READMEs, curated indexes, and high-inbound references.",
        ),
        "ranked_anchor" => Some(
            "Dense-ranked projection of `anchor`; useful when you want the top few durable read-first files.",
        ),
        "blocked" => Some("Used by `blocker` and the blocked section of `status`."),
        "blocker" => Some(
            "Canonical focused blocker view: blocked handle, total energy, and each signal explaining it. Join `primary_entropy` when you want one row per handle.",
        ),
        "holding" => Some(
            "A flow leaf for active handles with remaining potential whose status did not change since the latest snapshot.",
        ),
        "regressed" => Some(
            "A drifting leaf for active handles that moved backward in the configured lifecycle since the latest snapshot.",
        ),
        "re_opened" => Some(
            "A drifting leaf for handles that were terminal at the latest snapshot and active now.",
        ),
        "drifting" => Some(
            "A flow leaf that unifies `regressed(h)` and `re_opened(h)` as movement away from settledness.",
        ),
        "flow" => Some(
            "Coarse convergence direction: advancing, holding, or drifting. Settled handles are intentionally outside flow.",
        ),
        "broken_reference" => {
            Some("Diagnostic-rule predicate behind E001 broken-reference errors.")
        }
        "undischarged_obligation" => {
            Some("Diagnostic-rule predicate behind E002 undischarged-obligation errors.")
        }
        "stale_reference" => {
            Some("Diagnostic-rule predicate behind W001 stale-reference warnings.")
        }
        "spec_code_drift" => Some(
            "Diagnostic-rule predicate behind W006 spec-code-drift warnings. It uses asserts_code(status), not bare active(h), and target history, not bare absence, to avoid warning on examples, forward plans, or external-code studies.",
        ),
        "confidence_gap" => Some("Diagnostic-rule predicate behind W002 confidence-gap warnings."),
        "missing_frontmatter_file" => {
            Some("Diagnostic-rule predicate behind W003 missing-frontmatter warnings.")
        }
        "implausible_ref" => {
            Some("Diagnostic-rule predicate behind W004 implausible-reference warnings.")
        }
        "lifecycle_config_gap" => {
            Some("Diagnostic-rule predicate behind W005 lifecycle-config-gap warnings.")
        }
        "orphaned_handle" => {
            Some("Diagnostic-rule predicate behind S001 orphaned-handle suggestions.")
        }
        "pipeline_stall" => Some(
            "Diagnostic-rule predicate behind S003 pipeline-stall suggestions. It only emits after automatic snapshot history exists.",
        ),
        "abandoned_namespace" => {
            Some("Diagnostic-rule predicate behind S004 abandoned-namespace suggestions.")
        }
        "top_pair" => {
            Some("Diagnostic-rule predicate behind S005 concern-group-candidate suggestions.")
        }
        "area_of" => Some(
            "Source-neutral area lens over `*handle.area`; use it to group queries by corpus area.",
        ),
        "area_health" => Some(
            "Use directly in eval to grade each corpus area by local errors and cross-area connectivity.",
        ),
        "area_frontier" => Some(
            "Use directly in eval to pick the strongest unsettled-work handles inside each area.",
        ),
        _ => None,
    }
}

fn predicate_extra_lines(name: &str) -> Vec<String> {
    match name {
        "potential_weight" => vec![
            "Default weights in v0.15: undischarged=5, broken_ref=4, stale_dep=3, spec_code_drift=3, confidence_gap=3, freshness_decay=1, missing_meta=1, orphan_label=1.".to_string(),
            "Retune by declaring a project predicate with the same name and arity, for example `potential_weight(\"freshness_decay\", 0).`.".to_string(),
        ],
        "diagnostic" => vec![
            "Hidden CI gate: `anneal check` is retained for pre-commit/release gates and exits 1 when error rows exist.".to_string(),
            "Canonical error query: `anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'`.".to_string(),
        ],
        "flow" => vec![
            "Directions are exactly \"advancing\", \"holding\", and \"drifting\". Leaf predicates explain why a handle entered a direction.".to_string(),
            "`regressed(h)` and `re_opened(h)` are drifting leaves, not extra flow directions.".to_string(),
            "Settled handles are excluded so flow stays about active movement, not completed state.".to_string(),
        ],
        "holding" => vec![
            "Holding means stuck with work remaining: the handle is active, has potential, and has the same status at snapshot:last and now.".to_string(),
            "This intentionally excludes settled or inactive handles that simply did not change.".to_string(),
        ],
        "drifting" => vec![
            "Drifting means moving away from settledness. Inspect `regressed(h)` and `re_opened(h)` to see which leaf fired.".to_string(),
            "Use `flow(h, \"drifting\")` when you only need the coarse direction.".to_string(),
        ],
        "regressed" => vec![
            "Regression compares configured pipeline positions at snapshot:last and now.".to_string(),
            "If a status has no configured position, it cannot produce a regression row.".to_string(),
        ],
        "re_opened" => vec![
            "Re-opened handles were terminal at snapshot:last and active now.".to_string(),
            "This is tracked separately from generic regression because terminal-to-active movement often means a settled claim was reopened.".to_string(),
        ],
        "blocker" => vec![
            "A blocked handle can emit multiple rows when several entropy sources explain it.".to_string(),
            "Join `primary_entropy(h, source)` with the same source variable for one row per blocked handle.".to_string(),
        ],
        "recent_frontier" => vec![
            "Ranking shape: lower recency means newer; authored dates dominate, active status is a boost, statusless files remain eligible, and curated hubs are de-prioritized.".to_string(),
            "Use `--limit` on the eval command for a reading-list budget; then join to `read` or `context` when you have a goal.".to_string(),
        ],
        "anchor" => vec![
            "Ranking shape: explicit authoritative/living/current status outranks curated names; incoming degree and recency are bounded supporting signals.".to_string(),
            "The predicate is intentionally uncapped; wrap it in `TopK` for a bounded read-first list.".to_string(),
            "The `why` column names the strongest signal: authoritative_status, curated_name, inbound_degree, or recent.".to_string(),
        ],
        "ranked_anchor" => vec![
            "Add `order by rank asc --limit N` when you need a budgeted top-N anchor list.".to_string(),
        ],
        "asserts_code" => vec![
            "Config syntax: config convergence { asserts_code([stable, current, authoritative, active, draft]). }".to_string(),
            "Default when unconfigured: active status-bearing handles minus the aspirational study tier: plan, research, reference, exploratory.".to_string(),
            "W006 spec_code_drift uses this gate instead of bare active(h), so forward plans and external-code studies do not look like rot.".to_string(),
        ],
        "obligation" | "undischarged" => vec![
            "Retired obligations equivalent: `anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'`.".to_string(),
        ],
        "lifecycle_config_gap" => lifecycle_config_gap_variant_lines(),
        _ => Vec::new(),
    }
}

fn diagnostic_code_extra_lines(code: &str) -> Vec<String> {
    match code {
        "W005" => lifecycle_config_gap_variant_lines(),
        _ => Vec::new(),
    }
}

fn lifecycle_config_gap_variant_lines() -> Vec<String> {
    vec![
        "Variants: used_status_unpartitioned = a handle uses a status outside active/terminal.".to_string(),
        "Variants: ordering_status_unpartitioned = convergence.ordering names a status outside active/terminal.".to_string(),
        "Variants: ordering_not_terminal = the final ordered status is not terminal, so the lattice cannot settle.".to_string(),
    ]
}

fn common_joins(name: &str) -> &'static [&'static str] {
    match name {
        "diagnostic" => &[
            "`diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}` mirrors `anneal check` rows",
            "`diagnostic{subject: h}, area_of{h: h, area: \"X\"}` for area filtering",
            "`diagnostic{subject: h}, *handle{id: h, kind: \"file\"}` for file-handle diagnostics",
        ],
        "snapshot" => &[
            "`at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now` mirrors retired diff",
            "`*snapshot{snapshot: snapshot, id: h, key: \"status\", value: status}` to inspect raw status history rows",
        ],
        "search" => &[
            "`search{query: \"text\", handle: h, span_id: span_id, score: score}, *span{handle: h, id: span_id, summary: summary}` to add span summary",
            "`search{query: \"text\", handle: h, score: score}, *handle{id: h, status: status}` to inspect status-aware ranking",
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` to read matched spans",
        ],
        "meta" => &[
            "`*meta{handle: h, key: \"external_class\", value: class}` to inspect standard external sub-classes",
            "`*meta{handle: h, key: \"target_path\", value: path}` to inspect standard external targets",
            "`*meta{handle: h, key: k}` to discover corpus frontmatter keys",
        ],
        "read" => &[
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` to read the matched heading span",
            "`*span{handle: h, id: span_id, summary: summary}, read(h, 4000, span_id, text, start, end, tokens)` to read by heading hierarchy",
        ],
        "context" => &[
            "`context` composes ranked section search, compact span metadata, and graph neighborhood rows for cold-agent orientation",
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` when you need the same retrieval pieces manually",
        ],
        "handle" => &[
            "`*edge{to: h, from: src}, *handle{id: src, kind: kind}` mirrors `anneal handle H --impact` direct reverse dependencies",
            "`impact(h, affected, depth), *handle{id: affected, file: file}` for composable downstream traversal",
        ],
        "upstream" => &[
            "`upstream{h: h, anc: anc}, diagnostic{subject: anc}` to find broken upstream context",
            "`upstream{h: h, anc: anc}, *handle{id: anc, kind: \"file\"}` to keep upstream files only",
        ],
        "downstream" => &[
            "`downstream{h: h, desc: desc}, diagnostic{subject: desc}` to find affected diagnostics",
            "`downstream{h: h, desc: desc}, area_of{h: desc, area: area}` to group dependents by area",
        ],
        "potential" => &[
            "`potential(h, energy), entropy(h, source)` to explain raw energy",
            "`potential(h, energy), primary_entropy(h, source)` to keep one strongest reason per handle",
            "`potential(h, energy), frontier(h, energy)` to keep only the global frontier",
        ],
        "frontier" => &[
            "`frontier(h, energy), diagnostic{subject: h}` to see what blocks the frontier",
            "`frontier(h, energy), area_of{h: h, area: \"X\"}` for area-scoped frontier work",
        ],
        "recent_frontier" => &[
            "`recent_frontier(h, rank, recency), *handle{id: h, file: file, status: status} order by rank asc` for a goal-less reading frontier",
            "`recent_frontier(h, rank, recency), area_of{h: h, area: \"X\"}` to scope orientation to one area",
            "`recent_frontier(h, rank, recency), read(h, 1200, null, text, start, end, tokens)` to sample each file body",
        ],
        "anchor" => &[
            "`anchor(h, score, why), *handle{id: h, file: file, status: status}` for durable read-first files",
            "`anchor(h, score, why), incoming_edge(h, from, kind)` to inspect why a graph hub matters",
            "`anchor(h, score, why), area_of{h: h, area: area}` to group anchors by corpus area",
        ],
        "ranked_anchor" => &[
            "`ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc` with eval `--limit 12` for a budgeted anchor list",
            "`ranked_anchor(h, rank, score, why), h = \"HANDLE\"` to inspect one anchor's rank",
        ],
        "blocked" | "blocker" => &[
            "`blocked(h), entropy(h, source)` to see the unsettled signal",
            "`blocker(h, energy, source), primary_entropy(h, source)` to keep one strongest blocker row per handle",
            "`blocker(h, energy, source), *handle{id: h, file: file}` to add location metadata",
            "`blocked(h), area_of{h: h, area: \"X\"}` for area-scoped blockers",
        ],
        "broken_reference" => &[
            "`broken_reference(src, target, file, line), diagnostic{code: \"E001\", subject: src}` to inspect broken-reference diagnostics",
            "`broken_reference(src, target, file, line), *handle{id: src, summary: summary}` to add source context",
        ],
        "undischarged_obligation" => &[
            "`undischarged_obligation(h, file), diagnostic{code: \"E002\", subject: h}` to inspect undischarged-obligation errors",
            "`undischarged_obligation(h, file), area_of{h: h, area: area}` to group open obligations by area",
        ],
        "stale_reference" => &[
            "`stale_reference(src, target, file, source_status, target_status), diagnostic{code: \"W001\", subject: src}` to inspect stale-reference warnings",
            "`stale_reference(src, target, file, source_status, target_status), *handle{id: target, summary: summary}` to add target context",
        ],
        "spec_code_drift" => &[
            "`spec_code_drift(src, target_path, file, line, source_status), diagnostic{code: \"W006\", subject: src}` to inspect spec-code drift warnings",
            "`spec_code_drift(src, target_path, file, line, source_status), asserts_code(source_status)` to inspect the lifecycle gate",
            "`spec_code_drift(src, target_path, file, line, source_status), read{handle: src, budget: 1200, text: text}` to read the live spec context",
            "`spec_code_drift(src, target_path, file, line, source_status), *edge{from: src, to: ref, kind: \"Cites\"}, *meta{handle: ref, key: \"target_probe_base\", value: base}` to audit path resolution",
        ],
        "asserts_code" => &[
            "`asserts_code(status)` to inspect the effective status set",
            "`*config{key: \"convergence.asserts_code\", value: status}` to inspect explicit project config",
            "`spec_code_drift(src, target_path, file, line, status), asserts_code(status)` to audit W006 gating",
        ],
        "confidence_gap" => &[
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), diagnostic{code: \"W002\", subject: src}` to inspect confidence-gap warnings",
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), area_of{h: src, area: area}` to group gaps by area",
        ],
        "missing_frontmatter_file" => &[
            "`missing_frontmatter_file(h, dir, file), diagnostic{code: \"W003\", subject: h}` to inspect missing-frontmatter warnings",
            "`missing_frontmatter_file(h, dir, file), area_of{h: h, area: area}` to group missing metadata by area",
        ],
        "implausible_ref" => &[
            "`implausible_ref(h, file, value), diagnostic{code: \"W004\", subject: h}` to inspect implausible-reference warnings",
            "`implausible_ref(h, file, value), read{handle: h, budget: 1200, text: text}` to read nearby evidence",
        ],
        "lifecycle_config_gap" => &[
            "`lifecycle_config_gap(status, count, variant), diagnostic{code: \"W005\", subject: status}` to inspect lifecycle config warnings",
            "`lifecycle_config_gap(status, count, variant), configured_pipeline_status(status, level)` to compare against ordering",
        ],
        "orphaned_handle" => &[
            "`orphaned_handle(h), diagnostic{code: \"S001\", subject: h}` to inspect orphaned-handle suggestions",
            "`orphaned_handle(h), *handle{id: h, namespace: namespace}` to group orphans by namespace",
        ],
        "pipeline_stall" | "s003_pipeline_stall" => &[
            "`pipeline_stall(status, count, next_status, based_on_history), diagnostic{code: \"S003\", subject: status}` to inspect stalled lifecycle statuses",
            "`snapshot_history_present(count), pipeline_stall(status, stalled, next_status, true)` to confirm automatic status snapshots have accrued",
        ],
        "abandoned_namespace" | "s004_abandoned_namespace" => &[
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), diagnostic{code: \"S004\", subject: namespace}` to inspect abandoned namespace suggestions",
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), namespace_label(namespace, h)` to inspect members",
        ],
        "top_pair" | "s005_top_pair" => &[
            "`top_pair(left_prefix, right_prefix, count), diagnostic{code: \"S005\", subject: left_prefix}` to inspect concern-group candidates",
            "`top_pair(left_prefix, right_prefix, count), same_concern_pair(left_prefix, right_prefix)` to test whether a configured concern already covers it",
        ],
        "entropy" => &[
            "`entropy(h, source), potential(h, energy)` to see weighted convergence reasons",
            "`entropy(h, source), diagnostic{subject: h}` to connect signals to diagnostics",
        ],
        "obligation" | "undischarged" => &[
            "`undischarged(h), obligation(h), *handle{id: h, file: file, status: status}` mirrors retired obligations",
            "`undischarged(h), *handle{id: h, namespace: \"OQ\"}` for namespace-scoped obligations",
            "`undischarged(h), area_of{h: h, area: area}` to group open obligations by area",
        ],
        "git_mtime" | "changed_within" => &[
            "`*handle{id: h, file: file}, git_mtime(file, instant)` to inspect raw git-backed change time",
            "`changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}` to keep the result at file granularity",
            "`changed_within(h, 7), search{query: \"text\", handle: h}` for lower-authority recently-edited search hits",
        ],
        "potential_weight" => &[
            "`potential_weight(source, weight), entropy(h, source)` to see which handles use each weight",
            "`potential(h, energy), primary_entropy(h, source)` to inspect the weighted result",
        ],
        "advancing" | "holding" | "regressed" | "re_opened" | "drifting" | "flow" => &[
            "`flow(h, direction), *handle{id: h, status: status}` to add current lifecycle state",
            "`drifting(h), re_opened(h)` to separate reopened handles from ordinary regressions",
            "`holding(h), potential(h, energy)` to prioritize stuck handles with work remaining",
        ],
        "area_of" | "area_health" | "area_frontier" => &[
            "`area_of{h: h, area: \"X\"}, frontier(h, energy)` for area-scoped work",
            "`area_of{h: h, area: \"X\"}, diagnostic{subject: h}` for area-scoped diagnostics",
        ],
        _ => &[],
    }
}

fn predicate_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "diagnostic" => &[
            "status",
            "E001",
            "E002",
            "broken_reference",
            "undischarged_obligation",
            "pipeline_stall",
            "abandoned_namespace",
            "top_pair",
        ],
        "entropy" | "primary_entropy" | "potential" | "potential_subject" | "potential_weight"
        | "frontier" | "ranked_work" => &[
            "diagnostic",
            "obligation",
            "freshness",
            "hub",
            "orphan",
            "entropy_priority",
        ],
        "recent_frontier" => &[
            "anchor",
            "authored_age",
            "changed_recently",
            "freshness",
            "changed_within",
            "*handle",
            "status",
        ],
        "anchor" => &[
            "recent_frontier",
            "ranked_anchor",
            "incoming_edge",
            "hub",
            "freshness",
            "*handle",
        ],
        "ranked_anchor" => &[
            "anchor",
            "recent_frontier",
            "incoming_edge",
            "hub",
            "freshness",
            "*handle",
        ],
        "blocked" | "blocker" => &["potential", "primary_entropy", "entropy", "flux", "status"],
        "broken_reference" => &["E001", "diagnostic", "*edge", "*handle"],
        "undischarged_obligation" => &["E002", "diagnostic", "obligation", "discharge_count"],
        "stale_reference" => &["W001", "diagnostic", "active", "terminal"],
        "spec_code_drift" => &[
            "W006",
            "diagnostic",
            "asserts_code",
            "external_class",
            "target_path",
        ],
        "asserts_code" => &["W006", "spec_code_drift", "convergence", "*config"],
        "confidence_gap" => &[
            "W002",
            "diagnostic",
            "configured_pipeline_status",
            "pipeline_position_for",
        ],
        "missing_frontmatter_file" => &["W003", "diagnostic", "*handle", "*meta"],
        "implausible_ref" => &["W004", "diagnostic", "*meta"],
        "lifecycle_config_gap" => &[
            "W005",
            "diagnostic",
            "configured_pipeline_status",
            "pipeline_stall",
        ],
        "orphaned_handle" => &["S001", "diagnostic", "in_degree"],
        "pipeline_stall" | "s003_pipeline_stall" => &[
            "S003",
            "diagnostic",
            "status_population",
            "snapshot_history_present",
            "W005",
        ],
        "abandoned_namespace" | "s004_abandoned_namespace" => {
            &["S004", "diagnostic", "namespace_label", "freshness"]
        }
        "top_pair" | "s005_top_pair" => &["S005", "diagnostic", "*concern", "same_concern_pair"],
        "area_of" => &["area", "area_health", "area_frontier", "*handle", "schema"],
        "area"
        | "area_file_count"
        | "area_error_location_count"
        | "area_error_count"
        | "area_cross_edges"
        | "area_health"
        | "area_frontier" => &[
            "area_of",
            "diagnostic",
            "potential",
            "primary_entropy",
            "area_health",
        ],
        "advancing" | "holding" | "regressed" | "re_opened" | "drifting" | "flow" => &[
            "convergence",
            "snapshot_history_present",
            "potential",
            "settled",
        ],
        "obligation" | "undischarged" => &["*config", "discharged", "discharge_count"],
        _ => &[],
    }
}

fn verb_relationship(name: &str) -> &'static str {
    match name {
        "status" => {
            "Saved query over `primary_entropy`, non-blocked `potential` rows, `flow`, and `diagnostic`; human rendering summarizes convergence counts and sorts rows for arrival."
        }
        "search" => {
            "Saved query over the `search` primitive; applies TopK by calibrated score, filters `low_confidence = false`, and adds summary for span hits."
        }
        "context" => {
            "Saved query that composes boosted span-granular `search`, span metadata, `neighborhood`, TopK, and TakeUntil into one orientation bundle. The CLI `--read-spans` flag expands matched span bodies."
        }
        "read" => {
            "Saved query over the `read` primitive; the CLI can target one heading span with `--span-id`, usually copied from search/context output."
        }
        "handle" => {
            "Saved query over `*handle` and `*edge` for one focused handle; `anneal handle H --impact` adds reverse-dependency impact rows."
        }
        "describe" => "Saved query over the `describe` primitive.",
        "schema" => "Saved query over the `schema` primitive.",
        _ => "Saved @verb projected from the resolved prelude/project registry.",
    }
}

fn verb_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "status" => &[
            "frontier",
            "blocked",
            "flow",
            "diagnostic",
            "snapshot_history_present",
        ],
        "context" => &["search", "read", "handle"],
        "search" => &["context", "read", "schema"],
        "handle" => &["*handle", "*edge", "search"],
        "describe" => &["schema", "examples", "source-of"],
        "schema" => &["describe", "examples"],
        _ => &[],
    }
}

fn verb_example(name: &str) -> Option<&'static str> {
    match name {
        "status" => Some("anneal status"),
        "context" => Some(r#"anneal context "v17 conformance audit" --hits 3"#),
        "search" => Some(r#"anneal search "v17 conformance audit" --limit 5"#),
        "read" => Some("anneal read formal-model/v17.md --budget 4000"),
        "handle" => Some("anneal handle formal-model/v17.md --impact"),
        "describe" => Some("anneal describe search"),
        "schema" => Some("anneal schema"),
        _ => None,
    }
}

fn primitive_example(primitive: PrimitivePredicate) -> Option<&'static str> {
    match primitive {
        PrimitivePredicate::Obligation => Some("? obligation(h)."),
        PrimitivePredicate::Discharged => Some("? discharged(h)."),
        PrimitivePredicate::Undischarged => Some("? undischarged(h)."),
        PrimitivePredicate::DischargeCount => Some("? discharge_count(h, n)."),
        PrimitivePredicate::Upstream => Some(r#"? upstream("formal-model/v17.md", ancestor)."#),
        PrimitivePredicate::Downstream => {
            Some(r#"? downstream("formal-model/v17.md", dependent)."#)
        }
        PrimitivePredicate::Neighborhood => {
            Some(r#"? neighborhood("formal-model/v17.md", 1, member)."#)
        }
        PrimitivePredicate::Impact => Some(r#"? impact("formal-model/v17.md", affected, depth)."#),
        PrimitivePredicate::Search => {
            Some(r#"? search("v17 conformance audit", h, span_id, score, reason, field, low)."#)
        }
        PrimitivePredicate::Read => {
            Some(r#"? read("formal-model/v17.md", 4000, span, text, start, end, tokens)."#)
        }
        PrimitivePredicate::Schema => {
            Some("? schema(name, kind, signature, determinism, provenance).")
        }
        PrimitivePredicate::Describe => Some(r#"? describe("runtime", doc)."#),
        PrimitivePredicate::Sources => Some("? sources(name, recognizes, capabilities, doc)."),
        PrimitivePredicate::SourceOf => Some(r#"? source_of("search", file, lines)."#),
        PrimitivePredicate::Predicates => Some("? predicates(name, doc, file, lines)."),
        PrimitivePredicate::Verbs => Some("? verbs(name, query, doc, output_schema)."),
        PrimitivePredicate::Examples => Some(r#"? examples("search", example)."#),
        PrimitivePredicate::GitMtime => Some("? git_mtime(file, instant)."),
        PrimitivePredicate::ChangedWithin => Some("? changed_within(h, 7)."),
        PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::TokenEstimate
        | PrimitivePredicate::ReadFull
        | PrimitivePredicate::Match => None,
    }
}

fn fallback_stored_relation_example(name: &str, fields: &[impl AsRef<str>]) -> String {
    let field = fields
        .iter()
        .find_map(|field| {
            let field = field.as_ref();
            (!matches!(
                field,
                "corpus" | "source" | "native_id" | "origin_uri" | "revision" | "generation"
            ))
            .then_some(field)
        })
        .unwrap_or_else(|| fields.first().map_or("value", AsRef::as_ref));
    format!("? *{name}{{{field}: value}}.")
}

fn predicate_example(name: &str) -> Option<&'static str> {
    match name {
        "entropy" => Some(r#"? entropy("formal-model/v17.md", source)."#),
        "entropy_priority" => Some(r#"? entropy_priority("stale_dep", priority)."#),
        "potential_weight" => Some(r#"? potential_weight("freshness_decay", weight)."#),
        "primary_entropy" => Some(r#"? primary_entropy("formal-model/v17.md", source)."#),
        "area" => Some("? area(area)."),
        "area_file_count" => Some("? area_file_count(area, files)."),
        "area_error_location_count" => {
            Some("? area_error_location_count(area, code, subject, file, line, count).")
        }
        "area_error_count" => Some("? area_error_count(area, errors)."),
        "area_cross_edges" => Some("? area_cross_edges(area, cross_edges)."),
        "area_health" => Some("? area_health{area: area, grade: grade}."),
        "area_frontier" => Some("? area_frontier{area: area, h: h, score: score}."),
        "potential" => Some(r#"? potential("formal-model/v17.md", energy)."#),
        "blocked" => Some(r#"? blocked("formal-model/v17.md")."#),
        "blocker" => Some(r#"? blocker("formal-model/v17.md", energy, source)."#),
        "advancing" => Some(r#"? advancing("formal-model/v17.md")."#),
        "holding" => Some(r#"? holding("formal-model/v17.md")."#),
        "regressed" => Some(r#"? regressed("formal-model/v17.md")."#),
        "re_opened" => Some(r#"? re_opened("formal-model/v17.md")."#),
        "drifting" => Some(r#"? drifting("formal-model/v17.md")."#),
        "flow" => Some("? flow(h, direction)."),
        "frontier" => Some("? frontier(h, energy)."),
        "recent_frontier" => Some("? recent_frontier(h, rank, recency)."),
        "authored_age" => Some("? authored_age(h, days)."),
        "changed_recently" => Some("? changed_recently(h, band)."),
        "anchor" => Some("? anchor(h, score, why)."),
        "ranked_anchor" => Some("? ranked_anchor(h, rank, score, why)."),
        "ranked_work" => Some("? ranked_work(h, energy, rank)."),
        "broken_reference" => Some("? broken_reference(src, target, file, line)."),
        "undischarged_obligation" => Some("? undischarged_obligation(h, file)."),
        "stale_reference" => {
            Some("? stale_reference(src, target, file, source_status, target_status).")
        }
        "spec_code_drift" => {
            Some("? spec_code_drift(src, target_path, file, line, source_status).")
        }
        "asserts_code" => Some("? asserts_code(status)."),
        "confidence_gap" => Some(
            "? confidence_gap(src, target, file, source_status, source_level, target_status, target_level).",
        ),
        "missing_frontmatter_file" => Some("? missing_frontmatter_file(h, dir, file)."),
        "implausible_ref" => Some("? implausible_ref(h, file, value)."),
        "lifecycle_config_gap" => Some("? lifecycle_config_gap(status, count, variant)."),
        "orphaned_handle" => Some("? orphaned_handle(h)."),
        "pipeline_stall" | "s003_pipeline_stall" => {
            Some("? pipeline_stall(status, count, next_status, based_on_history).")
        }
        "abandoned_namespace" | "s004_abandoned_namespace" => {
            Some("? abandoned_namespace(namespace, total, terminal_count, stale_count).")
        }
        "top_pair" | "s005_top_pair" => Some("? top_pair(left_prefix, right_prefix, count)."),
        "incoming_edge" => Some(r#"? incoming_edge("REQ-1", from, kind)."#),
        "outgoing_edge" => Some(r#"? outgoing_edge("plan.md", to, kind)."#),
        "area_of" => Some(r#"? area_of{h: "formal-model/v17.md", area: area}."#),
        "namespace_of" => Some(r#"? namespace_of("OQ-1", namespace)."#),
        "status_of" => Some(r#"? status_of("formal-model/v17.md", status)."#),
        "hub" => Some("? hub(h, degree)."),
        "orphan" => Some("? orphan(h)."),
        "stub" => Some("? stub(h)."),
        "diagnostic" => {
            Some(r#"? diagnostic{code: "E001", severity: severity, subject: subject}."#)
        }
        "obligation" | "undischarged" => {
            Some("? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.")
        }
        _ => None,
    }
}

fn matching_tuples(tuples: &[Tuple], constraints: &[(usize, Value)]) -> Vec<Tuple> {
    tuples
        .iter()
        .filter(|tuple| tuple.matches_constraints(constraints))
        .cloned()
        .collect()
}

fn string_value(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn list_value<'a>(values: impl IntoIterator<Item = &'a str>) -> Value {
    Value::List(values.into_iter().map(string_value).collect())
}
