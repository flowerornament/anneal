use std::collections::{BTreeMap, BTreeSet};

use crate::facts::STORED_RELATION_DESCRIPTORS;
use crate::source::{SourceCapabilities, SourceInfo};
use crate::trail::TRAIL_RELATION_DESCRIPTORS;
use crate::verbs::VerbRegistry;

use super::analysis::{AnalyzedProgram, AnalyzedQuery};
use super::ast::{DocDecl, Expr, Head, Program, RuleLayer, SourceLocation, Statement};
use super::eval::{Tuple, Value};
use super::primitives::PrimitivePredicate;

#[derive(Clone, Debug, Default)]
pub(crate) struct IntrospectionIndex {
    source_descriptions: Vec<Tuple>,
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
            .map(|source| Tuple(vec![string_value(source.name), string_value(source.doc)]))
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
        self.program
            .describe
            .iter()
            .chain(&self.source_descriptions)
            .filter(|tuple| tuple.matches_constraints(constraints))
            .cloned()
            .collect()
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
    describe: Vec<Tuple>,
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
    describe: BTreeSet<Tuple>,
    source_of: BTreeSet<Tuple>,
    examples: BTreeSet<Tuple>,
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
        self.describe.insert(Tuple(vec![
            string_value("runtime"),
            string_value(&describe_card(DescribeCard {
                summary: "Query stored corpus facts, compose graph/lifecycle/content/search primitives, load Datalog rules, and discover the available model.",
                kind: Some("runtime topic"),
                examples: vec![
                    "? schema(name, kind, signature, determinism, provenance).",
                    "? describe(\"search\", doc).",
                    "? examples(\"search\", example).",
                ],
                ..DescribeCard::default()
            })),
        ]));
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
            );
        }
    }

    fn add_stored_relation(
        &mut self,
        name: &str,
        fields: &[impl AsRef<str>],
        doc: &str,
        provenance: &str,
    ) {
        let example = stored_relation_example(name, fields);
        self.schema.insert(schema_tuple(
            name,
            "stored",
            &stored_signature(name, fields),
            "input",
            provenance,
        ));
        let signature = stored_signature(name, fields);
        self.describe.insert(Tuple(vec![
            string_value(name),
            string_value(&describe_card(DescribeCard {
                summary: doc,
                kind: Some("stored relation"),
                signature: Some(&signature),
                source: Some(".design/2026-05-13-corpus-runtime.md"),
                examples: vec![example.as_str()],
                ..DescribeCard::default()
            })),
        ]));
        self.describe.insert(Tuple(vec![
            string_value(&format!("*{name}")),
            string_value(&describe_card(DescribeCard {
                summary: doc,
                kind: Some("stored relation"),
                signature: Some(&signature),
                source: Some(".design/2026-05-13-corpus-runtime.md"),
                examples: vec![example.as_str()],
                ..DescribeCard::default()
            })),
        ]));
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
            .insert(Tuple(vec![string_value(name), string_value(&example)]));
        self.examples.insert(Tuple(vec![
            string_value(&format!("*{name}")),
            string_value(&example),
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
            self.describe.insert(Tuple(vec![
                string_value(name),
                string_value(&describe_card(DescribeCard {
                    summary: primitive_doc(*primitive),
                    kind: Some("engine primitive"),
                    signature: Some(&call_signature(name, signature.parameters)),
                    source: Some("crates/anneal-core/src/runtime/primitives.rs"),
                    requires: primitive_requires(*primitive),
                    examples: primitive_example(*primitive).into_iter().collect(),
                    ..DescribeCard::default()
                })),
            ]));
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
            self.describe.insert(Tuple(vec![
                string_value(entry.name().as_str()),
                string_value(&describe_card(DescribeCard {
                    summary: entry.doc(),
                    kind: Some("verb"),
                    signature: Some(&format!("anneal {}", entry.name())),
                    source: Some(&format!(
                        "{}:{}",
                        entry.source().location().source_name,
                        source_line_text(entry.source().location())
                    )),
                    extra_lines: vec![format!("Output schema: {}", entry.output_schema())],
                    ..DescribeCard::default()
                })),
            ]));
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
            self.describe.insert(Tuple(vec![
                string_value(name),
                string_value(&describe_card(DescribeCard {
                    summary: &info.doc,
                    kind: Some("runtime topic"),
                    source: info.primary_source().as_deref(),
                    ..DescribeCard::default()
                })),
            ]));
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
            let source = info.source_lines.first_line_text();
            self.describe.insert(Tuple(vec![
                string_value(&name),
                string_value(&describe_card(DescribeCard {
                    summary: doc,
                    kind: Some("derived predicate"),
                    signature: Some(&signature),
                    source: source.as_deref(),
                    requires: predicate_requires(&name),
                    examples: predicate_example(&name).into_iter().collect(),
                    ..DescribeCard::default()
                })),
            ]));
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

    fn add_head(&mut self, head: &Head, layer: RuleLayer, location: &SourceLocation) {
        merge_parameter_names(&mut self.parameters, &head_parameter_names(head));
        self.add_source(layer, location);
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
        "profile_doc_corpus" | "profile_code_corpus" | "profile_issue_corpus" => Some(&["profile"]),
        _ => None,
    }
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
    kind: Option<&'a str>,
    signature: Option<&'a str>,
    source: Option<&'a str>,
    requires: &'a [&'a str],
    examples: Vec<&'a str>,
    extra_lines: Vec<String>,
}

fn describe_card(card: DescribeCard<'_>) -> String {
    // `describe(name, doc)` is the prose teaching surface. Machine callers should
    // use schema/source_of/examples for the same facts as structured relations.
    let mut lines = Vec::new();
    lines.push(card.summary.trim().to_string());
    if let Some(kind) = card.kind {
        lines.push(format!("Kind: {kind}."));
    }
    if let Some(signature) = card.signature {
        lines.push(format!("Signature: {signature}."));
    }
    lines.extend(card.extra_lines);
    for requirement in card.requires {
        lines.push(format!("Requires: {requirement}"));
    }
    for example in card.examples {
        lines.push(format!("Example: {example}"));
    }
    if let Some(source) = card.source {
        lines.push(format!("Source: {source}."));
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

fn predicate_requires(name: &str) -> &'static [&'static str] {
    match name {
        "entropy" | "potential_subject" | "potential" | "work_candidate" | "top_work"
        | "ranked_work" => &[
            "stored handles plus the relevant diagnostic, obligation, lifecycle, freshness, or graph facts that create unsettled-work signals.",
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

fn stored_relation_example(name: &str, fields: &[impl AsRef<str>]) -> String {
    match name {
        "handle" => r#"? *handle{id: h, kind: "file", status: status}."#.to_string(),
        "edge" => r#"? *edge{from: src, to: dst, kind: "DependsOn"}."#.to_string(),
        "meta" => r"? *meta{handle: h, key: key, value: value}.".to_string(),
        "content" => r"? *content{handle: h, span_id: span, tokens: tokens}.".to_string(),
        "span" => r"? *span{id: span, handle: h, start_line: start, end_line: end}.".to_string(),
        "concern" => r"? *concern{name: concern, member: h}.".to_string(),
        "config" => r"? *config{key: key, value: value, ordinal: ordinal}.".to_string(),
        "snapshot" => {
            r"? *snapshot{snapshot: snapshot, id: h, key: key, value: value}.".to_string()
        }
        "generation" => r"? *generation{source: source, current: generation}.".to_string(),
        "trail" => r"? *trail{session_id: session, step: step, verb: verb}.".to_string(),
        "trail_ref" => r"? *trail_ref{session_id: session, kind: kind, handle: h}.".to_string(),
        "trail_generation" => {
            r"? *trail_generation{session_id: session, source: source, generation: generation}."
                .to_string()
        }
        _ => {
            let field = fields
                .iter()
                .find_map(|field| {
                    let field = field.as_ref();
                    (!matches!(
                        field,
                        "corpus"
                            | "source"
                            | "native_id"
                            | "origin_uri"
                            | "revision"
                            | "generation"
                    ))
                    .then_some(field)
                })
                .unwrap_or_else(|| fields.first().map_or("value", AsRef::as_ref));
            format!("? *{name}{{{field}: value}}.")
        }
    }
}

fn predicate_example(name: &str) -> Option<&'static str> {
    match name {
        "entropy" => Some("? entropy(h, source)."),
        "potential" => Some("? potential(h, energy)."),
        "blocked" => Some("? blocked(h)."),
        "advancing" => Some("? advancing(h)."),
        "top_work" => Some("? top_work(h, energy)."),
        "ranked_work" => Some("? ranked_work(h, energy, rank)."),
        "incoming_edge" => Some(r#"? incoming_edge("REQ-1", from, kind)."#),
        "outgoing_edge" => Some(r#"? outgoing_edge("plan.md", to, kind)."#),
        "area_of" => Some("? area_of(h, area)."),
        "namespace_of" => Some("? namespace_of(h, namespace)."),
        "status_of" => Some("? status_of(h, status)."),
        "hub" => Some("? hub(h, degree)."),
        "orphan" => Some("? orphan(h)."),
        "stub" => Some("? stub(h)."),
        "diagnostic" => Some("? diagnostic(code, severity, subject, file, line, evidence)."),
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
