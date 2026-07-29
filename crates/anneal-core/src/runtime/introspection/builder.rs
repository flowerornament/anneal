//! Builds the runtime index from stored, primitive, and analyzed-program vocabulary.

use std::collections::{BTreeMap, BTreeSet};

use crate::facts::STORED_RELATION_DESCRIPTORS;
use crate::trail::TRAIL_RELATION_DESCRIPTORS;
use crate::verbs::VerbRegistry;

use super::super::analysis::AnalyzedQuery;
use super::super::ast::{Program, RuleLayer};
use super::super::primitives::PrimitivePredicate;
use super::{
    DescribeEntry, DescribeKind, ProgramIntrospection, StoredRelationSummary, Tuple, catalog,
    describe_entry, program, projection, render, string_value, topics,
};

#[derive(Default)]
/// Accumulates each introspection relation as a sorted set before final projection.
pub(super) struct IntrospectionBuilder {
    schema: BTreeSet<Tuple>,
    predicates: BTreeSet<Tuple>,
    verbs: BTreeSet<Tuple>,
    describe: BTreeSet<DescribeEntry>,
    source_of: BTreeSet<Tuple>,
    examples: BTreeSet<Tuple>,
}

impl IntrospectionBuilder {
    /// Seeds query-local construction from an immutable program-level index.
    pub(super) fn from_existing(existing: &ProgramIntrospection) -> Self {
        Self {
            schema: existing.schema.iter().cloned().collect(),
            predicates: existing.predicates.iter().cloned().collect(),
            verbs: existing.verbs.iter().cloned().collect(),
            describe: existing.describe.iter().cloned().collect(),
            source_of: existing.source_of.iter().cloned().collect(),
            examples: existing.examples.iter().cloned().collect(),
        }
    }
}

impl IntrospectionBuilder {
    /// Installs hand-authored runtime and configuration topics.
    pub(super) fn add_runtime_overview(&mut self) {
        let catalog = topics::runtime_topic_catalog();
        self.describe.extend(catalog.descriptions);
        self.examples.extend(catalog.examples);
    }
}

impl IntrospectionBuilder {
    /// Adds static and source-declared stored relations without shadow duplicates.
    pub(super) fn add_stored_relations(&mut self, dynamic_stored: Vec<StoredRelationSummary>) {
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
                &catalog::fallback_stored_relation_example(&relation.name, &relation.fields),
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
        self.schema.insert(projection::schema_tuple(
            name,
            "stored",
            &catalog::stored_signature(name, fields),
            "input",
            provenance,
        ));
        let signature = catalog::stored_signature(name, fields);
        let card = render::describe_card(render::DescribeCard {
            summary: doc,
            kind: Some(DescribeKind::StoredRelation),
            signature: Some(&signature),
            common_joins: catalog::common_joins(name),
            see_also: catalog::stored_relation_see_also(name),
            examples: vec![example],
            extra_lines: catalog::stored_relation_extra_lines(name),
            ..render::DescribeCard::default()
        });
        let star_name = format!("*{name}");
        for describe_name in [name, star_name.as_str()] {
            self.describe.insert(describe_entry(
                describe_name,
                DescribeKind::StoredRelation,
                &card,
            ));
        }
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
}

impl IntrospectionBuilder {
    /// Projects engine primitives through the same schema and card relations as rules.
    pub(super) fn add_primitives(&mut self) {
        for primitive in PrimitivePredicate::ALL {
            let name = primitive.name();
            let signature = primitive.signature();
            self.schema.insert(projection::schema_tuple(
                name,
                "primitive",
                &projection::call_signature(name, signature.parameters),
                catalog::primitive_determinism(*primitive),
                "engine",
            ));
            self.describe.insert(describe_entry(
                name,
                DescribeKind::EnginePrimitive,
                &render::describe_card(render::DescribeCard {
                    summary: catalog::primitive_doc(*primitive),
                    kind: Some(DescribeKind::EnginePrimitive),
                    signature: Some(&projection::call_signature(name, signature.parameters)),
                    relationship: catalog::primitive_relationship(*primitive),
                    common_joins: catalog::common_joins(name),
                    requires: catalog::primitive_requires(*primitive),
                    see_also: catalog::primitive_see_also(*primitive),
                    examples: catalog::primitive_example(*primitive).into_iter().collect(),
                    extra_lines: catalog::predicate_extra_lines(name),
                }),
            ));
            self.source_of.insert(Tuple(vec![
                string_value(name),
                string_value("crates/anneal-core/src/runtime/primitives.rs"),
                string_value("unknown"),
            ]));
            if let Some(example) = catalog::primitive_example(*primitive) {
                self.examples
                    .insert(Tuple(vec![string_value(name), string_value(example)]));
            }
        }
    }
}

impl IntrospectionBuilder {
    /// Derives rule, verb, documentation, example, and source rows from one program.
    pub(super) fn add_program(&mut self, program: &Program) {
        let scanned = program::ProgramScanner::scan(program);
        let (docs, predicates) = scanned.into_parts();
        let predicate_names = predicates.keys().cloned().collect::<BTreeSet<_>>();
        self.add_predicates(predicates, &docs);
        self.add_docs(&docs, &predicate_names);

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
                &render::describe_card(render::DescribeCard {
                    summary: entry.doc(),
                    kind: Some(DescribeKind::Verb),
                    signature: Some(&format!("anneal {}", entry.name())),
                    relationship: Some(catalog::verb_relationship(entry.name().as_str())),
                    common_joins: catalog::common_joins(entry.name().as_str()),
                    see_also: catalog::verb_see_also(entry.name().as_str()),
                    examples: catalog::verb_example(entry.name().as_str())
                        .into_iter()
                        .collect(),
                    extra_lines: vec![format!("Output schema: {}", entry.output_schema())],
                    ..render::DescribeCard::default()
                }),
            ));
            self.source_of.insert(Tuple(vec![
                string_value(entry.name().as_str()),
                string_value(&entry.source().location().source_name),
                string_value(&projection::source_line_text(entry.source().location())),
            ]));
            for example in entry.examples() {
                self.examples.insert(Tuple(vec![
                    string_value(entry.name().as_str()),
                    string_value(example),
                ]));
            }
        }
    }
}

impl IntrospectionBuilder {
    /// Adds the static diagnostic-code cards owned by anneal's checks vocabulary.
    pub(super) fn add_diagnostic_codes(&mut self) {
        for code in topics::DIAGNOSTIC_CODE_CARDS {
            let mut extra_lines = vec![
                format!("Diagnostic code: {}.", code.code),
                format!("Severity: {}.", code.severity),
                format!("Rule predicate: {}.", code.rule),
                format!("Evidence: {}.", code.evidence),
            ];
            extra_lines.extend(catalog::diagnostic_code_extra_lines(code.code));
            self.describe.insert(describe_entry(
                code.code,
                DescribeKind::RuntimeTopic,
                &render::describe_card(render::DescribeCard {
                    summary: code.summary,
                    kind: Some(DescribeKind::RuntimeTopic),
                    relationship: Some("Diagnostic catalog entry; query rows through `diagnostic(...)` and inspect the deriving rule predicate for structure."),
                    common_joins: code.common_joins,
                    see_also: code.see_also,
                    examples: vec![code.example],
                    extra_lines,
                    ..render::DescribeCard::default()
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
}

impl IntrospectionBuilder {
    /// Adds query-local rules while preserving the immutable program index.
    pub(super) fn add_query(&mut self, query: &AnalyzedQuery) {
        let mut predicates = BTreeMap::<String, program::PredicateInfo>::new();
        for rule in &query.query().local_rules {
            program::add_predicate_head(
                &mut predicates,
                &rule.head,
                RuleLayer::Inline,
                rule.origin().location(),
            );
        }
        self.add_predicates(predicates, &BTreeMap::new());
    }

    fn add_docs(
        &mut self,
        docs: &BTreeMap<String, program::DocInfo>,
        predicate_names: &BTreeSet<String>,
    ) {
        for (name, info) in docs {
            if predicate_names.contains(name) {
                continue;
            }
            let doc = topics::axis_topic_card(name).unwrap_or_else(|| {
                if name == "convergence" {
                    topics::convergence_topic_card()
                } else {
                    render::describe_card(render::DescribeCard {
                        summary: info.doc(),
                        kind: Some(DescribeKind::RuntimeTopic),
                        ..render::DescribeCard::default()
                    })
                }
            });
            self.describe
                .insert(describe_entry(name, DescribeKind::RuntimeTopic, &doc));
            for (file, line_text) in info.source_lines().iter_line_text() {
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
        predicates: BTreeMap<String, program::PredicateInfo>,
        docs: &BTreeMap<String, program::DocInfo>,
    ) {
        for (name, info) in predicates {
            let doc = docs.get(&name).map_or(info.doc(), program::DocInfo::doc);
            self.schema.insert(projection::schema_tuple(
                &name,
                "derived",
                &info.signature(),
                "deterministic",
                &info.provenance(),
            ));
            if name == "ranked_anchor" {
                self.schema.insert(projection::schema_tuple(
                    "ranked_anchor.signals",
                    "rendering",
                    "signals: [{why, score}]",
                    "deterministic",
                    "CLI JSON enrichment sourced from anchor_signal(h, score, priority, why)",
                ));
            }
            for (file, line_text) in info.source_lines().iter_line_text() {
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
                for (file, line_text) in doc_info.source_lines().iter_line_text() {
                    self.source_of.insert(Tuple(vec![
                        string_value(&name),
                        string_value(file),
                        string_value(&line_text),
                    ]));
                }
            }
            if let Some(example) = catalog::predicate_example(&name) {
                self.examples
                    .insert(Tuple(vec![string_value(&name), string_value(example)]));
            }
            let signature = info.signature();
            self.describe.insert(describe_entry(
                &name,
                DescribeKind::DerivedPredicate,
                &render::describe_card(render::DescribeCard {
                    summary: doc,
                    kind: Some(DescribeKind::DerivedPredicate),
                    signature: Some(&signature),
                    relationship: catalog::predicate_relationship(&name),
                    common_joins: catalog::common_joins(&name),
                    requires: catalog::predicate_requires(&name),
                    see_also: catalog::predicate_see_also(&name),
                    examples: catalog::predicate_example(&name).into_iter().collect(),
                    extra_lines: catalog::predicate_extra_lines(&name),
                }),
            ));
        }
    }
}

impl IntrospectionBuilder {
    /// Converts sorted sets into the stable vectors consumed by runtime primitives.
    pub(super) fn finish(self) -> ProgramIntrospection {
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
