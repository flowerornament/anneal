use std::collections::{BTreeMap, BTreeSet};

use crate::facts::STORED_RELATION_DESCRIPTORS;
use crate::source::{SourceCapabilities, SourceInfo};
use crate::trail::TRAIL_RELATION_DESCRIPTORS;

use super::analysis::{AnalyzedProgram, AnalyzedQuery};
use super::ast::{DocDecl, Expr, Head, Program, RuleLayer, SourceLocation, Statement, VerbDecl};
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
            string_value(
                "anneal runtime: query stored corpus facts, compose graph/lifecycle/content/search primitives, load Datalog rules, and discover the available model with schema, predicates, verbs, describe, source_of, examples, and sources.",
            ),
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
        self.schema.insert(schema_tuple(
            name,
            "stored",
            &stored_signature(name, fields),
            "input",
            provenance,
        ));
        self.describe
            .insert(Tuple(vec![string_value(name), string_value(doc)]));
        self.source_of.insert(Tuple(vec![
            string_value(name),
            string_value(".design/2026-05-13-corpus-runtime.md"),
            string_value("unknown"),
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
                string_value(primitive_doc(*primitive)),
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
        self.add_predicates(scanned.predicates, &scanned.docs);
        self.add_docs(&scanned.docs);

        for verb in scanned.verbs {
            let Some(info) = VerbInfo::from_decl(verb) else {
                continue;
            };
            self.verbs.insert(Tuple(vec![
                string_value(&info.name),
                string_value(&info.query),
                string_value(&info.doc),
                string_value(&info.output_schema),
            ]));
            self.describe.insert(Tuple(vec![
                string_value(&info.name),
                string_value(&info.doc),
            ]));
            self.source_of.insert(Tuple(vec![
                string_value(&info.name),
                string_value(&verb.location().source_name),
                string_value(&source_line_text(verb.location())),
            ]));
            self.examples.insert(Tuple(vec![
                string_value(&info.name),
                string_value(&info.query),
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

    fn add_docs(&mut self, docs: &BTreeMap<String, DocInfo>) {
        for (name, info) in docs {
            self.describe
                .insert(Tuple(vec![string_value(name), string_value(&info.doc)]));
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
            self.describe
                .insert(Tuple(vec![string_value(&name), string_value(doc)]));
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
            .map(|(idx, parameter)| match parameter {
                ParameterName::Named(name) => name.as_str().to_string(),
                ParameterName::Unknown | ParameterName::Ambiguous => format!("arg{idx}"),
            })
            .collect::<Vec<_>>();
        call_signature(&self.name, &parameters)
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
struct ProgramScanner<'a> {
    docs: BTreeMap<String, DocInfo>,
    predicates: BTreeMap<String, PredicateInfo>,
    verbs: Vec<&'a VerbDecl>,
}

impl<'a> ProgramScanner<'a> {
    fn scan(program: &'a Program) -> Self {
        let mut scanner = Self::default();
        scanner.scan_statements(&program.statements);
        scanner
    }

    fn scan_statements(&mut self, statements: &'a [Statement]) {
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
                Statement::Verb(verb) => self.verbs.push(verb),
                Statement::Doc(doc) => {
                    if let Some(existing) = self.docs.get_mut(doc.name()) {
                        existing.replace_from_decl(doc);
                    } else {
                        self.docs
                            .insert(doc.name().to_string(), DocInfo::from_decl(doc));
                    }
                }
                Statement::Query(_)
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

struct VerbInfo {
    name: String,
    query: String,
    doc: String,
    output_schema: String,
}

impl VerbInfo {
    fn from_decl(verb: &VerbDecl) -> Option<Self> {
        Some(Self {
            name: verb.string_arg("name")?.to_string(),
            query: verb.string_arg("query")?.to_string(),
            doc: verb.string_arg("doc")?.to_string(),
            output_schema: verb.string_arg("output_schema")?.to_string(),
        })
    }
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

fn primitive_determinism(primitive: PrimitivePredicate) -> &'static str {
    match primitive {
        PrimitivePredicate::Search => "ranker-dependent deterministic",
        _ => "deterministic",
    }
}

fn primitive_doc(primitive: PrimitivePredicate) -> &'static str {
    match primitive {
        PrimitivePredicate::Upstream => "Bind ancestors reachable through incoming edges.",
        PrimitivePredicate::Downstream => "Bind descendants reachable through outgoing edges.",
        PrimitivePredicate::Impact => "Bind graph nodes impacted by a handle with traversal depth.",
        PrimitivePredicate::Neighborhood => "Bind nearby graph nodes within a depth budget.",
        PrimitivePredicate::Terminal => {
            "Bind handles whose status is terminal in the configured lattice."
        }
        PrimitivePredicate::Active => {
            "Bind handles whose status is active in the configured lattice."
        }
        PrimitivePredicate::Settled => "Bind handles considered settled by the configured lattice.",
        PrimitivePredicate::PipelinePosition => {
            "Bind a handle and its configured status pipeline position."
        }
        PrimitivePredicate::PipelinePositionFor => {
            "Bind a status and its configured pipeline position."
        }
        PrimitivePredicate::Obligation => "Bind handles that are open obligations.",
        PrimitivePredicate::Discharged => "Bind obligations with a discharge edge.",
        PrimitivePredicate::Undischarged => "Bind obligations without a discharge edge.",
        PrimitivePredicate::CiteCount => "Bind per-handle incoming citation counts.",
        PrimitivePredicate::InDegree => "Bind per-handle incoming edge counts.",
        PrimitivePredicate::OutDegree => "Bind per-handle outgoing edge counts.",
        PrimitivePredicate::DischargeCount => "Bind per-handle incoming discharge edge counts.",
        PrimitivePredicate::Freshness => {
            "Bind days since the handle's dated observation at the active time reference."
        }
        PrimitivePredicate::Flux => "Bind status-change count over a grounded day window.",
        PrimitivePredicate::TokenEstimate => "Bind estimated stored content tokens for a handle.",
        PrimitivePredicate::Search => {
            "Search stored handle, metadata, and content text with source-calibrated ranking."
        }
        PrimitivePredicate::Read => "Read stored content spans for a handle within a token budget.",
        PrimitivePredicate::ReadFull => {
            "Read complete stored content for a handle behind the read_full capability."
        }
        PrimitivePredicate::Match => {
            "Match a regular expression against stored content for one bound handle."
        }
        PrimitivePredicate::Schema => {
            "List queryable stored relations, derived predicates, and engine primitives."
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

fn primitive_example(primitive: PrimitivePredicate) -> Option<&'static str> {
    match primitive {
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
        | PrimitivePredicate::ReadFull
        | PrimitivePredicate::Match => None,
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
