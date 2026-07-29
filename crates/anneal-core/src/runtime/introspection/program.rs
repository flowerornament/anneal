//! Derives predicate signatures, docs, sources, and parameters from analyzed programs.

use std::collections::{BTreeMap, BTreeSet};

use super::super::ast::{
    DocDecl, Expr, Head, PredicateDecl, Program, RuleLayer, SourceLocation, Statement, Term,
};
use super::projection;

#[derive(Clone, Debug)]
/// Merged declaration and rule-head evidence for one derived predicate.
pub(super) struct PredicateInfo {
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

    /// Names every rule layer that contributes to the predicate.
    pub(super) fn provenance(&self) -> String {
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

    /// Renders the effective signature after declaration and observed-head merging.
    pub(super) fn signature(&self) -> String {
        let parameters = self
            .parameters
            .iter()
            .enumerate()
            .map(|(idx, parameter)| display_parameter_name(&self.name, idx, parameter))
            .collect::<Vec<_>>();
        projection::call_signature(&self.name, &parameters)
    }

    /// Returns the effective documentation after declaration merging.
    pub(super) fn doc(&self) -> &str {
        &self.doc
    }

    /// Returns every source location that contributes to the predicate.
    pub(super) const fn source_lines(&self) -> &SourceLines {
        &self.source_lines
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
/// Latest documentation text plus every source location that declared it.
pub(super) struct DocInfo {
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

    /// Returns the latest documentation declaration.
    pub(super) fn doc(&self) -> &str {
        &self.doc
    }

    /// Returns every source location that declared the topic.
    pub(super) const fn source_lines(&self) -> &SourceLines {
        &self.source_lines
    }
}

#[derive(Clone, Debug, Default)]
/// Canonical source-file to declaration-line map used by `source_of`.
pub(super) struct SourceLines(BTreeMap<String, BTreeSet<usize>>);

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

    /// Renders deterministic source rows while preserving all declaration lines.
    pub(super) fn iter_line_text(&self) -> impl Iterator<Item = (&str, String)> {
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
/// One-pass analyzer over declarations, facts, rules, nested `at` blocks, and docs.
pub(super) struct ProgramScanner {
    docs: BTreeMap<String, DocInfo>,
    predicates: BTreeMap<String, PredicateInfo>,
}

impl ProgramScanner {
    /// Scans a program into merged documentation and predicate records.
    pub(super) fn scan(program: &Program) -> Self {
        let mut scanner = Self::default();
        scanner.scan_statements(&program.statements);
        scanner
    }

    /// Separates the completed scan into the two projections consumed by the builder.
    pub(super) fn into_parts(self) -> (BTreeMap<String, DocInfo>, BTreeMap<String, PredicateInfo>) {
        (self.docs, self.predicates)
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

/// Merges one predicate head into the scanner's name-indexed record.
pub(super) fn add_predicate_head(
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
            Term::Expr(Expr::Var(var)) => ParameterName::Named(var.to_string()),
            _ => ParameterName::Unknown,
        })
        .collect()
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
