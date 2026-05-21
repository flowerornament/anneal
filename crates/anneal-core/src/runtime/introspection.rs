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
                        source_label: Some("Adapter"),
                        source: Some(source.name),
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
                source_label: Some("Topic source"),
                examples: vec![
                    "? schema(name, kind, signature, determinism, provenance).",
                    "? describe(\"search\", doc).",
                    "? examples(\"search\", example).",
                ],
                ..DescribeCard::default()
            }),
        ));
        self.examples.insert(Tuple(vec![
            string_value("runtime"),
            string_value(r#"? describe("runtime", doc)."#),
        ]));
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
                source_label: Some("Contract"),
                source: Some(".design/2026-05-13-corpus-runtime.md"),
                examples: vec![example],
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
                source_label: Some("Contract"),
                source: Some(".design/2026-05-13-corpus-runtime.md"),
                examples: vec![example],
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
                    source_label: Some("Implementation"),
                    source: Some("crates/anneal-core/src/runtime/primitives.rs"),
                    requires: primitive_requires(*primitive),
                    see_also: primitive_see_also(*primitive),
                    examples: primitive_example(*primitive).into_iter().collect(),
                    ..DescribeCard::default()
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
                    source_label: Some("Verb source"),
                    source: Some(&format!(
                        "{}:{}",
                        entry.source().location().source_name,
                        source_line_text(entry.source().location())
                    )),
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
            self.describe.insert(describe_entry(
                name,
                DescribeKind::RuntimeTopic,
                &describe_card(DescribeCard {
                    summary: &info.doc,
                    kind: Some(DescribeKind::RuntimeTopic),
                    source_label: Some("Topic source"),
                    source: info.primary_source().as_deref(),
                    ..DescribeCard::default()
                }),
            ));
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
            let source = info.source_lines.compact_rule_source();
            self.describe.insert(describe_entry(
                &name,
                DescribeKind::DerivedPredicate,
                &describe_card(DescribeCard {
                    summary: doc,
                    kind: Some(DescribeKind::DerivedPredicate),
                    signature: Some(&signature),
                    relationship: predicate_relationship(&name),
                    source_label: Some("Rule source"),
                    source: source.as_deref(),
                    requires: predicate_requires(&name),
                    see_also: predicate_see_also(&name),
                    examples: predicate_example(&name).into_iter().collect(),
                    ..DescribeCard::default()
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
        "entropy" => Some(&["h", "source"]),
        "potential_weight" => Some(&["source", "weight"]),
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

    fn first_line_text(&self) -> Option<String> {
        self.0
            .iter()
            .next()
            .map(|(file, lines)| format!("{file}:{}", line_list(lines)))
    }

    fn compact_rule_source(&self) -> Option<String> {
        self.0.iter().next().map(|(file, lines)| {
            if lines.len() > 4 {
                format!("{file} ({} rules)", lines.len())
            } else {
                format!("{file}:{}", line_list(lines))
            }
        })
    }
}

impl DocInfo {
    fn primary_source(&self) -> Option<String> {
        self.source_lines.first_line_text()
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

#[derive(Default)]
struct DescribeCard<'a> {
    summary: &'a str,
    kind: Option<DescribeKind>,
    signature: Option<&'a str>,
    relationship: Option<&'a str>,
    source_label: Option<&'a str>,
    source: Option<&'a str>,
    requires: &'a [&'a str],
    see_also: &'a [&'a str],
    examples: Vec<&'a str>,
    extra_lines: Vec<String>,
}

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
    lines.extend(card.extra_lines);
    for requirement in card.requires {
        lines.push(format!("Requires: {requirement}"));
    }
    if !card.see_also.is_empty() {
        lines.push(format!("See also: {}.", card.see_also.join(", ")));
    }
    for example in card.examples {
        lines.push(format!("Example: {example}"));
    }
    if let Some(source) = card.source {
        let label = card.source_label.unwrap_or("Source");
        lines.push(format!("{label}: {source}."));
    }
    lines.join("\n")
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
        PrimitivePredicate::TokenEstimate => {
            "Return the estimated number of stored content tokens for a handle."
        }
        PrimitivePredicate::Search => {
            "Search handle identities, metadata, and content text, returning ranked hits with a reason for each score."
        }
        PrimitivePredicate::Read => {
            "Read content spans for one handle, stopping when the token budget is reached."
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
            "The `search` verb wraps this primitive with TopK ranking and filters out low-confidence hits by default.",
        ),
        PrimitivePredicate::Read => Some(
            "The `read` verb wraps this primitive with typed CLI arguments for handle and budget.",
        ),
        PrimitivePredicate::Schema => Some("The `schema` verb projects this primitive directly."),
        PrimitivePredicate::Verbs => Some("The `verbs` verb projects this primitive directly."),
        PrimitivePredicate::Describe => {
            Some("The `describe` verb projects this primitive as teaching cards.")
        }
        PrimitivePredicate::SourceOf => {
            Some("The `source-of` verb projects this primitive directly.")
        }
        PrimitivePredicate::Examples => {
            Some("The `examples` verb projects this primitive directly.")
        }
        PrimitivePredicate::Sources => Some("The `sources` verb projects this primitive directly."),
        _ => None,
    }
}

fn primitive_see_also(primitive: PrimitivePredicate) -> &'static [&'static str] {
    match primitive {
        PrimitivePredicate::Search => &["search", "context", "read", "examples"],
        PrimitivePredicate::Read | PrimitivePredicate::ReadFull => &["read", "*content", "*span"],
        PrimitivePredicate::Schema => &["describe", "examples", "verbs"],
        PrimitivePredicate::Describe => &["schema", "examples", "verbs"],
        PrimitivePredicate::Examples => &["describe", "schema", "verbs"],
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
        "entropy" | "primary_entropy" | "potential_subject" | "potential" | "work_candidate"
        | "top_work" | "ranked_work" => &[
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
            "`area_of` rows from source facts. Area health also uses diagnostics, edges, and work-candidate convergence signals.",
        ],
        "blocked" => {
            &["active lifecycle config, at least one potential signal, and no recent status flux."]
        }
        "advancing" | "recently_advanced" | "snapshot_history_present" => &[
            "snapshot history and configured lifecycle ordering. On a corpus with no snapshots, these predicates return no rows.",
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
            "Shared diagnostic stream used by `broken`, `status`, and `work`; individual rules contribute rows by diagnostic code.",
        ),
        "top_work" => Some(
            "Used by the `work` verb; `status` uses the same work-candidate vocabulary but removes already-blocked handles from its arrival projection.",
        ),
        "blocked" => Some("Used by the `blocked` verb and the blocked section of `status`."),
        "area_of" => Some(
            "Source-neutral area lens over `*handle.area`; use it to group queries by corpus area.",
        ),
        "area_health" => Some(
            "Used by the `areas` verb; grades each corpus area by local errors and cross-area connectivity.",
        ),
        "area_frontier" => Some(
            "Used by the `areas` verb; picks the strongest unsettled-work handles inside each area.",
        ),
        _ => None,
    }
}

fn predicate_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "diagnostic" => &[
            "broken",
            "broken_reference",
            "obligation",
            "s001_orphaned",
            "s003_pipeline_stall",
            "s004_abandoned_namespace",
            "s005_top_pair",
        ],
        "entropy" | "primary_entropy" | "potential" | "potential_subject" | "work_candidate"
        | "top_work" | "ranked_work" => &[
            "diagnostic",
            "obligation",
            "freshness",
            "hub",
            "orphan",
            "entropy_priority",
        ],
        "blocked" => &["potential", "entropy", "flux", "status"],
        "area_of" => &["area", "area_health", "area_frontier", "*handle", "vocab"],
        "area"
        | "area_file_count"
        | "area_error_location_count"
        | "area_error_count"
        | "area_cross_edges"
        | "area_health"
        | "area_frontier" => &[
            "area_of",
            "diagnostic",
            "work_candidate",
            "primary_entropy",
            "areas",
        ],
        "obligation" | "undischarged" => &["*config", "discharged", "discharge_count"],
        _ => &[],
    }
}

fn verb_relationship(name: &str) -> &'static str {
    match name {
        "status" => {
            "Saved query over `primary_entropy`, non-blocked `work_candidate` rows, `advancing`, and `diagnostic`; human rendering summarizes convergence counts and sorts rows for arrival."
        }
        "search" => {
            "Saved query over the `search` primitive; applies TopK by score and filters `low_confidence = false`."
        }
        "context" => {
            "Saved query that composes `search`, `read`, `neighborhood`, TopK, and TakeUntil into one orientation bundle."
        }
        "read" => "Saved query over the `read` primitive.",
        "handle" => "Saved query over `*handle` and `*edge` for one focused handle.",
        "blocked" => {
            "Saved query over `potential`, `entropy`, and `*handle` for one focused handle."
        }
        "broken" => "Saved query over `diagnostic` filtered to severity `error`.",
        "work" => "Saved query over `top_work` joined to `*handle` metadata.",
        "areas" => {
            "Saved query over `area_health` and `area_frontier`; it is the per-area drill-down from `status`."
        }
        "describe" => "Saved query over the `describe` primitive.",
        "examples" => "Saved query over the `examples` primitive.",
        "schema" => "Saved query over the `schema` primitive.",
        "verbs" => "Saved query over the `verbs` primitive.",
        "sources" => "Saved query over the `sources` primitive.",
        "vocab" => {
            "Saved query over stored relations and config rows that reveal observed corpus vocabulary."
        }
        "find" => {
            "Saved query over `*handle.id contains text`; use `search` for ranked content retrieval."
        }
        _ => "Saved @verb projected from the resolved prelude/project registry.",
    }
}

fn verb_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "status" => &["work", "blocked", "broken", "trend"],
        "context" => &["search", "read", "handle"],
        "search" => &["context", "read", "schema"],
        "handle" => &["*handle", "*edge", "search"],
        "blocked" => &["potential", "entropy", "status"],
        "broken" => &["diagnostic", "status"],
        "work" => &["top_work", "ranked_work", "status"],
        "areas" => &["area_health", "area_frontier", "area_of", "status"],
        "describe" => &["schema", "examples", "source-of"],
        "schema" => &["describe", "examples", "verbs"],
        "vocab" => &["*handle", "*edge", "*meta", "*config"],
        _ => &[],
    }
}

fn verb_example(name: &str) -> Option<&'static str> {
    match name {
        "status" => Some("anneal status"),
        "context" => Some(r#"anneal context "v17 conformance audit" --hits 3"#),
        "search" => Some(r#"anneal search "v17 conformance audit" --limit 5"#),
        "read" => Some("anneal read formal-model/v17.md --budget 4000"),
        "handle" => Some("anneal handle formal-model/v17.md"),
        "blocked" => Some("anneal blocked formal-model/v17.md --explain"),
        "broken" => Some("anneal broken"),
        "work" => Some("anneal work"),
        "areas" => Some("anneal areas"),
        "describe" => Some("anneal describe search"),
        "schema" => Some("anneal schema"),
        "verbs" => Some("anneal verbs"),
        "examples" => Some("anneal examples search"),
        "vocab" => Some("anneal vocab"),
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
        "advancing" => Some(r#"? advancing("formal-model/v17.md")."#),
        "top_work" => Some("? top_work(h, energy)."),
        "ranked_work" => Some("? ranked_work(h, energy, rank)."),
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
