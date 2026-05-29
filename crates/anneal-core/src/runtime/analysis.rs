use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::facts::{STORED_RELATION_DESCRIPTORS, StoredRelationDescriptor};
use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, CallStyle, Comparison, DerivedAtom, Expr,
    Head, Ident, Literal, NegatedAtom, Negation, PredicateDecl, PredicateRef, Program, Query, Rule,
    RuleLayer, SourceLocation, Statement, StoredAtom, Term,
};
use crate::runtime::primitives::{PrimitivePredicate, PrimitiveSignature, primitive_signatures};
use crate::trail::TRAIL_RELATION_DESCRIPTORS;

type SignatureMap = BTreeMap<PredicateRef, PredicateSignature>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PredicateSignature {
    arity: usize,
    parameters: ParameterNames,
    kind: PredicateKind,
    explicit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PredicateKind {
    Derived,
    Primitive { sealed: bool },
}

impl PredicateKind {
    fn is_derived(self) -> bool {
        matches!(self, Self::Derived)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParameterNames {
    Unknown,
    Named(Vec<Ident>),
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFieldSet(Vec<&'static str>);

impl StoredFieldSet {
    fn from_descriptor(descriptor: StoredRelationDescriptor) -> Self {
        Self(descriptor.fields.to_vec())
    }

    pub fn as_slice(&self) -> &[&'static str] {
        &self.0
    }
}

impl fmt::Display for StoredFieldSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, field) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(field)?;
        }
        Ok(())
    }
}

pub fn analyze(program: Program) -> Result<AnalyzedProgram, StaticError> {
    Analyzer::new(program).analyze()
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnalyzedProgram {
    program: Program,
    signatures: SignatureMap,
    strata: Vec<Stratum>,
    queries: Vec<AnalyzedQuery>,
}

impl AnalyzedProgram {
    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn facts(&self) -> impl Iterator<Item = &Head> {
        self.program.facts()
    }

    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.program.rules()
    }

    pub fn queries(&self) -> impl Iterator<Item = &AnalyzedQuery> {
        self.queries.iter()
    }

    pub fn strata(&self) -> &[Stratum] {
        &self.strata
    }

    pub fn predicates(&self) -> impl Iterator<Item = &PredicateRef> {
        self.signatures
            .iter()
            .filter_map(|(predicate, signature)| signature.kind.is_derived().then_some(predicate))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnalyzedQuery {
    query: Query,
    local_strata: Vec<Stratum>,
    local_predicates: BTreeSet<PredicateRef>,
}

impl AnalyzedQuery {
    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn local_strata(&self) -> &[Stratum] {
        &self.local_strata
    }

    pub fn local_predicates(&self) -> impl Iterator<Item = &PredicateRef> {
        self.local_predicates.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stratum {
    pub level: usize,
    pub predicates: Vec<PredicateRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StaticError {
    #[error("{location}: unknown predicate '{predicate}/{arity}'{suggestion}")]
    UnknownPredicate {
        predicate: PredicateRef,
        arity: usize,
        location: SourceLocation,
        suggestion: Box<str>,
    },
    #[error(
        "{location}: predicate '{predicate}' used with arity {actual}, expected {expected}; signature: {expected_signature}"
    )]
    ArityMismatch {
        predicate: PredicateRef,
        expected: usize,
        actual: usize,
        expected_signature: Box<str>,
        location: SourceLocation,
    },
    #[error(
        "{location}: unsafe rule '{predicate}': head variable '{variable}' is not bound positively"
    )]
    UnboundHeadVariable {
        predicate: PredicateRef,
        variable: Ident,
        location: SourceLocation,
    },
    #[error(
        "{location}: unsafe expression in '{predicate}': variable '{variable}' is not bound positively"
    )]
    UnboundExpressionVariable {
        predicate: PredicateRef,
        variable: Ident,
        location: SourceLocation,
    },
    #[error(
        "{location}: unsafe negation in '{predicate}': variable '{variable}' is not bound positively"
    )]
    UnboundNegationVariable {
        predicate: PredicateRef,
        variable: Ident,
        location: SourceLocation,
    },
    #[error("{location}: cyclic negation between {cycle}")]
    CyclicNegation {
        cycle: DependencyCycle,
        location: SourceLocation,
    },
    #[error("{location}: cyclic stratifying dependency between {cycle}")]
    CyclicStratification {
        cycle: DependencyCycle,
        location: SourceLocation,
    },
    #[error(
        "{location}: named call to '{predicate}' requires a named predicate signature; add @predicate(name: \"{predicate}\", args: [...])"
    )]
    NamedCallRequiresSignature {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error(
        "{location}: unknown named argument '{argument}' for '{predicate}'; expected one of: {expected}"
    )]
    UnknownNamedArgument {
        predicate: Box<PredicateRef>,
        argument: Ident,
        expected: Box<str>,
        location: SourceLocation,
    },
    #[error("{location}: @predicate missing string field '{field}'")]
    PredicateMissingString {
        field: &'static str,
        location: SourceLocation,
    },
    #[error("{location}: @predicate name {name:?} is not a valid predicate name")]
    PredicateInvalidName {
        name: Box<str>,
        location: SourceLocation,
    },
    #[error("{location}: @predicate args must be a list of strings")]
    PredicateArgsMustBeList { location: SourceLocation },
    #[error("{location}: duplicate @predicate arg '{argument}'")]
    DuplicatePredicateArg {
        argument: Ident,
        location: SourceLocation,
    },
    #[error("{location}: unknown field '{field}' for '*{relation}'; expected one of: {expected}")]
    UnknownStoredField {
        relation: Ident,
        field: Ident,
        expected: StoredFieldSet,
        location: SourceLocation,
    },
    #[error("{location}: duplicate argument '{argument}' for '{predicate}'")]
    DuplicateNamedArgument {
        predicate: PredicateRef,
        argument: Ident,
        location: SourceLocation,
    },
    #[error("{location}: positional arguments must precede named arguments in '{predicate}'")]
    PositionalAfterNamedArgument {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error("{location}: named argument '{argument}' is not supported for function '{function}'")]
    UnsupportedNamedFunctionArgument {
        function: Ident,
        argument: Ident,
        location: SourceLocation,
    },
    #[error("{location}: engine primitive '{predicate}' cannot be defined by corpus rules")]
    PrimitiveRedefinition {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error(
        "{location}: optional discovery fact '{predicate}' must be consumed by the project loader"
    )]
    OptionalDiscoveryFact {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error("{location}: graph primitive '{predicate}' requires a bound endpoint argument")]
    UnboundGraphPrimitiveAnchor {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error("{location}: primitive '{predicate}' requires bound input argument '{argument}'")]
    UnboundPrimitiveInput {
        predicate: PredicateRef,
        argument: &'static str,
        location: SourceLocation,
    },
    #[error("{location}: diagnostic id for '{predicate}' must be a string literal first argument")]
    DiagnosticIdMustBeLiteral {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error("{second}: duplicate diagnostic id '{id}' first declared at {first}")]
    DuplicateDiagnosticId {
        id: String,
        first: SourceLocation,
        second: SourceLocation,
    },
    #[error(
        "{location}: reserved diagnostic id '{id}' may only be emitted by built-in check rules"
    )]
    ReservedDiagnosticId {
        id: String,
        location: SourceLocation,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyCycle {
    predicates: Vec<PredicateRef>,
}

impl DependencyCycle {
    fn new(predicates: Vec<PredicateRef>) -> Self {
        Self { predicates }
    }

    pub fn predicates(&self) -> &[PredicateRef] {
        &self.predicates
    }
}

impl fmt::Display for DependencyCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, predicate) in self.predicates.iter().enumerate() {
            if idx > 0 {
                f.write_str(" -> ")?;
            }
            write!(f, "{predicate}")?;
        }
        Ok(())
    }
}

struct Analyzer {
    program: Program,
    signatures: SignatureMap,
    edges: Vec<DependencyEdge>,
    diagnostic_ids: BTreeMap<String, SourceLocation>,
}

impl Analyzer {
    fn new(program: Program) -> Self {
        Self {
            program,
            signatures: builtin_signatures(),
            edges: Vec::new(),
            diagnostic_ids: BTreeMap::new(),
        }
    }

    fn analyze(mut self) -> Result<AnalyzedProgram, StaticError> {
        self.check_no_optional_discovery_facts()?;
        self.collect_global_signatures()?;
        normalize_global_named_calls(&mut self.program, &self.signatures)?;
        self.check_facts()?;
        self.check_rules()?;
        check_cyclic_stratification(&self.edges)?;
        let strata = compute_strata(&self.signatures, &self.edges, None);
        let queries = self.check_queries()?;
        Ok(AnalyzedProgram {
            program: self.program,
            signatures: self.signatures,
            strata,
            queries,
        })
    }

    fn check_no_optional_discovery_facts(&self) -> Result<(), StaticError> {
        for statement in &self.program.statements {
            check_no_optional_discovery_fact(statement)?;
        }
        Ok(())
    }

    fn collect_global_signatures(&mut self) -> Result<(), StaticError> {
        collect_explicit_predicate_signatures(&mut self.signatures, &self.program.statements)?;
        for fact in self.program.facts() {
            collect_signature(&mut self.signatures, fact)?;
        }
        for rule in self.program.rules() {
            collect_signature(&mut self.signatures, &rule.head)?;
        }
        Ok(())
    }

    fn check_facts(&self) -> Result<(), StaticError> {
        for fact in self.program.facts() {
            check_fact(fact)?;
        }
        Ok(())
    }

    fn check_rules(&mut self) -> Result<(), StaticError> {
        let rules: Vec<Rule> = self.program.rules().cloned().collect();
        for rule in &rules {
            check_rule(
                rule,
                &self.signatures,
                &mut self.edges,
                &mut self.diagnostic_ids,
            )?;
        }
        Ok(())
    }

    fn check_queries(&self) -> Result<Vec<AnalyzedQuery>, StaticError> {
        self.program
            .queries()
            .map(|query| self.check_query(query))
            .collect()
    }

    fn check_query(&self, query: &Query) -> Result<AnalyzedQuery, StaticError> {
        let mut query = query.clone();
        let mut signatures = self.signatures.clone();
        let mut local_predicates = BTreeSet::new();
        for rule in &query.local_rules {
            collect_signature(&mut signatures, &rule.head)?;
            local_predicates.insert(rule.head.predicate.clone());
        }
        normalize_query_named_calls(&mut query, &signatures)?;

        let mut edges = self.edges.clone();
        let mut diagnostic_ids = self.diagnostic_ids.clone();
        for rule in &query.local_rules {
            check_rule(rule, &signatures, &mut edges, &mut diagnostic_ids)?;
        }
        check_body(
            &PredicateRef::new(Ident::new_unchecked("query")),
            &query.body,
            &signatures,
            &mut edges,
        )?;
        check_body_safety(
            &PredicateRef::new(Ident::new_unchecked("query")),
            &query.body,
        )?;
        check_cyclic_stratification(&edges)?;
        let local_strata = compute_strata(&signatures, &edges, Some(&local_predicates));

        Ok(AnalyzedQuery {
            query,
            local_strata,
            local_predicates,
        })
    }
}

fn check_no_optional_discovery_fact(statement: &Statement) -> Result<(), StaticError> {
    match statement {
        Statement::OptionalFact(head) => Err(StaticError::OptionalDiscoveryFact {
            predicate: head.predicate.clone(),
            location: head.location.clone(),
        }),
        Statement::AtBlock { statements, .. } => {
            for statement in statements {
                check_no_optional_discovery_fact(statement)?;
            }
            Ok(())
        }
        Statement::Fact(_)
        | Statement::ConfigBlock(_)
        | Statement::SourceBlock(_)
        | Statement::Rule(_)
        | Statement::Query(_)
        | Statement::Include(_)
        | Statement::Import(_)
        | Statement::Verb(_)
        | Statement::Doc(_)
        | Statement::Predicate(_) => Ok(()),
    }
}

fn collect_explicit_predicate_signatures(
    signatures: &mut SignatureMap,
    statements: &[Statement],
) -> Result<(), StaticError> {
    for statement in statements {
        match statement {
            Statement::Predicate(decl) => collect_explicit_predicate_signature(signatures, decl)?,
            Statement::AtBlock { statements, .. } => {
                collect_explicit_predicate_signatures(signatures, statements)?;
            }
            Statement::Fact(_)
            | Statement::OptionalFact(_)
            | Statement::ConfigBlock(_)
            | Statement::SourceBlock(_)
            | Statement::Rule(_)
            | Statement::Query(_)
            | Statement::Include(_)
            | Statement::Import(_)
            | Statement::Verb(_)
            | Statement::Doc(_) => {}
        }
    }
    Ok(())
}

fn collect_explicit_predicate_signature(
    signatures: &mut SignatureMap,
    decl: &PredicateDecl,
) -> Result<(), StaticError> {
    let name = decl
        .string_arg("name")
        .ok_or_else(|| StaticError::PredicateMissingString {
            field: "name",
            location: decl.location().clone(),
        })?;
    let predicate = PredicateRef::parse(name).map_err(|_| StaticError::PredicateInvalidName {
        name: name.into(),
        location: decl.location().clone(),
    })?;
    let parameters = explicit_predicate_args(decl)?;
    let arity = parameters.len();
    match signatures.get_mut(&predicate) {
        Some(signature) if matches!(signature.kind, PredicateKind::Primitive { sealed: true }) => {
            Err(StaticError::PrimitiveRedefinition {
                predicate,
                location: decl.location().clone(),
            })
        }
        Some(signature) => {
            signature.arity = arity;
            signature.parameters = ParameterNames::Named(parameters);
            signature.explicit = true;
            Ok(())
        }
        None => {
            signatures.insert(
                predicate,
                PredicateSignature {
                    arity,
                    parameters: ParameterNames::Named(parameters),
                    kind: PredicateKind::Derived,
                    explicit: true,
                },
            );
            Ok(())
        }
    }
}

fn explicit_predicate_args(decl: &PredicateDecl) -> Result<Vec<Ident>, StaticError> {
    let Some(items) = decl.string_list_arg("args") else {
        return Err(StaticError::PredicateArgsMustBeList {
            location: decl.location().clone(),
        });
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for value in items {
        let ident =
            Ident::new(value.to_string()).map_err(|_| StaticError::PredicateArgsMustBeList {
                location: decl.location().clone(),
            })?;
        if !seen.insert(ident.clone()) {
            return Err(StaticError::DuplicatePredicateArg {
                argument: ident,
                location: decl.location().clone(),
            });
        }
        out.push(ident);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct DependencyEdge {
    from: PredicateRef,
    to: PredicateRef,
    kind: DependencyKind,
    location: SourceLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DependencyKind {
    Positive,
    Negation,
    Aggregation,
}

impl DependencyKind {
    const fn is_stratifying(self) -> bool {
        !matches!(self, Self::Positive)
    }
}

fn collect_signature(signatures: &mut SignatureMap, head: &Head) -> Result<(), StaticError> {
    let arity = head.arity();
    let parameters = head_parameter_names(head);
    match signatures.get_mut(&head.predicate) {
        Some(signature) if matches!(signature.kind, PredicateKind::Primitive { sealed: true }) => {
            Err(StaticError::PrimitiveRedefinition {
                predicate: head.predicate.clone(),
                location: head.location.clone(),
            })
        }
        Some(signature) if matches!(signature.kind, PredicateKind::Primitive { sealed: false }) => {
            if signature.arity != arity {
                return Err(arity_mismatch(
                    &head.predicate,
                    signature,
                    arity,
                    &head.location,
                ));
            }
            signature.parameters = parameters;
            signature.kind = PredicateKind::Derived;
            signature.explicit = false;
            Ok(())
        }
        Some(signature) if signature.arity != arity => Err(arity_mismatch(
            &head.predicate,
            signature,
            arity,
            &head.location,
        )),
        Some(signature) => {
            if signature.explicit {
                return Ok(());
            }
            signature.parameters.merge(parameters);
            Ok(())
        }
        None => {
            signatures.insert(
                head.predicate.clone(),
                PredicateSignature {
                    arity,
                    parameters,
                    kind: PredicateKind::Derived,
                    explicit: false,
                },
            );
            Ok(())
        }
    }
}

fn builtin_signatures() -> SignatureMap {
    primitive_signatures()
        .map(|(predicate, signature)| {
            (
                predicate,
                PredicateSignature {
                    arity: signature.arity(),
                    parameters: parameter_names(signature),
                    kind: PredicateKind::Primitive {
                        sealed: signature.sealed,
                    },
                    explicit: true,
                },
            )
        })
        .collect()
}

fn parameter_names(signature: PrimitiveSignature) -> ParameterNames {
    ParameterNames::Named(
        signature
            .parameters
            .iter()
            .map(|parameter| Ident::new_unchecked(*parameter))
            .collect(),
    )
}

fn arity_mismatch(
    predicate: &PredicateRef,
    signature: &PredicateSignature,
    actual: usize,
    location: &SourceLocation,
) -> StaticError {
    StaticError::ArityMismatch {
        predicate: predicate.clone(),
        expected: signature.arity,
        actual,
        expected_signature: expected_signature(predicate, signature).into_boxed_str(),
        location: location.clone(),
    }
}

fn expected_signature(predicate: &PredicateRef, signature: &PredicateSignature) -> String {
    let parameters = match &signature.parameters {
        ParameterNames::Named(parameters) => parameters
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ParameterNames::Unknown | ParameterNames::Ambiguous => (0..signature.arity)
            .map(|idx| format!("arg{idx}"))
            .collect::<Vec<_>>(),
    };
    format!("{}({})", predicate, parameters.join(", "))
}

fn head_parameter_names(head: &Head) -> ParameterNames {
    let mut names = Vec::with_capacity(head.terms.len());
    let mut seen = BTreeSet::new();
    for term in &head.terms {
        let Term::Expr(Expr::Var(name)) = term else {
            return ParameterNames::Unknown;
        };
        if !seen.insert(name.clone()) {
            return ParameterNames::Ambiguous;
        }
        names.push(name.clone());
    }
    ParameterNames::Named(names)
}

impl ParameterNames {
    fn merge(&mut self, incoming: Self) {
        match (&*self, incoming) {
            (Self::Ambiguous, _) | (_, Self::Ambiguous) => *self = Self::Ambiguous,
            (Self::Unknown, Self::Named(names)) => *self = Self::Named(names),
            (Self::Named(existing), Self::Named(incoming)) if existing != &incoming => {
                *self = Self::Ambiguous;
            }
            _ => {}
        }
    }
}

fn check_fact(head: &Head) -> Result<(), StaticError> {
    for term in &head.terms {
        if let Some(expr) = term.expr() {
            let mut vars = BTreeSet::new();
            expr.variables(&mut vars);
            if let Some(variable) = vars.into_iter().next() {
                return Err(StaticError::UnboundHeadVariable {
                    predicate: head.predicate.clone(),
                    variable,
                    location: head.location.clone(),
                });
            }
        }
    }
    Ok(())
}

fn normalize_global_named_calls(
    program: &mut Program,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    for statement in &mut program.statements {
        normalize_global_statement_named_calls(statement, signatures)?;
    }
    Ok(())
}

fn normalize_global_statement_named_calls(
    statement: &mut Statement,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    match statement {
        Statement::Fact(head) => normalize_head_named_calls(head),
        Statement::Rule(rule) => normalize_rule_named_calls(rule, signatures),
        Statement::AtBlock { statements, .. } => {
            for statement in statements {
                normalize_global_statement_named_calls(statement, signatures)?;
            }
            Ok(())
        }
        Statement::Query(_)
        | Statement::ConfigBlock(_)
        | Statement::SourceBlock(_)
        | Statement::OptionalFact(_)
        | Statement::Include(_)
        | Statement::Import(_)
        | Statement::Verb(_)
        | Statement::Doc(_)
        | Statement::Predicate(_) => Ok(()),
    }
}

fn normalize_query_named_calls(
    query: &mut Query,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    for rule in &mut query.local_rules {
        normalize_rule_named_calls(rule, signatures)?;
    }
    normalize_body_named_calls(&mut query.body, signatures)
}

fn normalize_rule_named_calls(
    rule: &mut Rule,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    normalize_head_named_calls(&mut rule.head)?;
    normalize_body_named_calls(&mut rule.body, signatures)
}

fn normalize_head_named_calls(head: &mut Head) -> Result<(), StaticError> {
    for term in &mut head.terms {
        if let Some(expr) = term.expr_mut() {
            normalize_expr_named_calls(expr)?;
        }
    }
    Ok(())
}

fn normalize_body_named_calls(
    body: &mut Body,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    for atom in &mut body.atoms {
        normalize_atom_named_calls(atom, signatures)?;
    }
    Ok(())
}

fn normalize_atom_named_calls(
    atom: &mut Atom,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    match atom {
        Atom::Stored(stored) => {
            normalize_stored_named_calls(stored)?;
        }
        Atom::Derived(derived) => normalize_derived_named_args(derived, signatures)?,
        Atom::Comparison(comparison) => {
            normalize_expr_named_calls(&mut comparison.left)?;
            normalize_expr_named_calls(&mut comparison.right)?;
        }
        Atom::Aggregation(aggregate) => normalize_aggregate_named_calls(aggregate, signatures)?,
        Atom::Negation(negation) => match &mut negation.atom {
            NegatedAtom::Stored(stored) => {
                normalize_stored_named_calls(stored)?;
            }
            NegatedAtom::Derived(derived) => normalize_derived_named_args(derived, signatures)?,
        },
        Atom::TimeBlock(time_block) => {
            normalize_body_named_calls(&mut time_block.body, signatures)?;
        }
    }
    Ok(())
}

fn normalize_stored_named_calls(stored: &mut StoredAtom) -> Result<(), StaticError> {
    validate_stored_fields(stored)?;
    for field in &mut stored.fields {
        if let Some(expr) = field.term.expr_mut() {
            normalize_expr_named_calls(expr)?;
        }
    }
    Ok(())
}

fn validate_stored_fields(stored: &StoredAtom) -> Result<(), StaticError> {
    let Some(descriptor) = stored_relation_descriptor(stored.relation.as_str()) else {
        return Ok(());
    };
    for field in &stored.fields {
        if !descriptor
            .fields
            .iter()
            .any(|expected| *expected == field.field.as_str())
        {
            return Err(StaticError::UnknownStoredField {
                relation: stored.relation.clone(),
                field: field.field.clone(),
                expected: StoredFieldSet::from_descriptor(descriptor),
                location: field.location.clone(),
            });
        }
    }
    Ok(())
}

fn stored_relation_descriptor(name: &str) -> Option<StoredRelationDescriptor> {
    STORED_RELATION_DESCRIPTORS
        .iter()
        .chain(TRAIL_RELATION_DESCRIPTORS)
        .find(|descriptor| descriptor.name == name)
        .copied()
}

fn unknown_predicate_error(
    predicate: PredicateRef,
    arity: usize,
    location: SourceLocation,
    signatures: Option<&SignatureMap>,
) -> StaticError {
    let suggestion = if predicate.module.is_none()
        && stored_relation_descriptor(predicate.name.as_str()).is_some()
    {
        format!(
            ". Did you mean '*{}{{...}}'? '{}' is a stored relation; stored relations use a '*' prefix and named fields.",
            predicate.name, predicate.name
        )
    } else if predicate.module.is_none()
        && let Some(replacement) = retired_predicate_replacement(predicate.name.as_str())
    {
        format!(
            ". Predicate '{}' was retired; use `{replacement}`.",
            predicate.name
        )
    } else if let Some(signatures) = signatures {
        let candidates = close_predicate_candidates(predicate.name.as_str(), arity, signatures);
        if candidates.is_empty() {
            ". Try `anneal schema` to list predicates, or `anneal describe convergence` for convergence vocabulary.".to_string()
        } else {
            format!(". Did you mean {}?", candidates.join(", "))
        }
    } else {
        String::new()
    };
    StaticError::UnknownPredicate {
        predicate,
        arity,
        location,
        suggestion: suggestion.into_boxed_str(),
    }
}

fn retired_predicate_replacement(name: &str) -> Option<&'static str> {
    match name {
        "top_work" => Some("frontier(h, energy)"),
        "blocked_row" => Some("blocker(h, energy, source)"),
        "recent" => Some("changed_within(h, days)"),
        _ => None,
    }
}

fn close_predicate_candidates(name: &str, arity: usize, signatures: &SignatureMap) -> Vec<String> {
    let mut scored = signatures
        .iter()
        .filter(|(predicate, _)| predicate.module.is_none())
        .filter(|(_, signature)| signature.arity == arity)
        .filter_map(|(predicate, signature)| {
            let distance = levenshtein(name, predicate.name.as_str());
            (distance <= 3).then(|| (distance, predicate.name.as_str(), signature))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate, signature)| format!("`{}`", render_signature(candidate, signature)))
        .collect()
}

fn render_signature(name: &str, signature: &PredicateSignature) -> String {
    match &signature.parameters {
        ParameterNames::Named(parameters) => {
            let params = parameters
                .iter()
                .map(Ident::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}({params})")
        }
        ParameterNames::Unknown | ParameterNames::Ambiguous => {
            format!("{name}/{}", signature.arity)
        }
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (i, left_ch) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_ch) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_ch != right_ch);
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            current[j + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn normalize_aggregate_named_calls(
    aggregate: &mut Aggregate,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    normalize_expr_named_calls(&mut aggregate.result)?;
    normalize_expr_named_calls(&mut aggregate.value)?;
    for arg in &mut aggregate.args {
        normalize_expr_named_calls(&mut arg.expr)?;
    }
    normalize_body_named_calls(&mut aggregate.body, signatures)
}

fn normalize_expr_named_calls(expr: &mut Expr) -> Result<(), StaticError> {
    match expr {
        Expr::Var(_) | Expr::Literal(_) => Ok(()),
        Expr::FunctionCall { function, args } => {
            for arg in args {
                if let CallArg::Named { name, location, .. } = arg {
                    return Err(StaticError::UnsupportedNamedFunctionArgument {
                        function: function.clone(),
                        argument: name.clone(),
                        location: location.clone(),
                    });
                }
                if let Some(expr) = arg.expr_mut() {
                    normalize_expr_named_calls(expr)?;
                }
            }
            Ok(())
        }
        Expr::Binary { left, right, .. } => {
            normalize_expr_named_calls(left)?;
            normalize_expr_named_calls(right)
        }
        Expr::Tuple(items) => {
            for item in items {
                normalize_expr_named_calls(item)?;
            }
            Ok(())
        }
    }
}

fn normalize_derived_named_args(
    derived: &mut DerivedAtom,
    signatures: &SignatureMap,
) -> Result<(), StaticError> {
    let has_named_args = derived
        .args
        .iter()
        .any(|arg| matches!(arg, CallArg::Named { .. }));
    if !has_named_args && derived.style.is_complete() {
        for arg in &mut derived.args {
            if let Some(expr) = arg.expr_mut() {
                normalize_expr_named_calls(expr)?;
            }
        }
        return Ok(());
    }

    let signature = signatures.get(&derived.predicate).ok_or_else(|| {
        unknown_predicate_error(
            derived.predicate.clone(),
            derived.args.len(),
            derived.location.clone(),
            Some(signatures),
        )
    })?;
    let normalized = normalize_call_args(
        &derived.predicate,
        &derived.args,
        signature,
        matches!(derived.style, CallStyle::Pattern),
    )?;
    derived.args = normalized;
    derived.style = CallStyle::Complete;
    for arg in &mut derived.args {
        if let Some(expr) = arg.expr_mut() {
            normalize_expr_named_calls(expr)?;
        }
    }
    Ok(())
}

fn normalize_call_args(
    predicate: &PredicateRef,
    args: &[CallArg],
    signature: &PredicateSignature,
    allow_omitted: bool,
) -> Result<Vec<CallArg>, StaticError> {
    let ParameterNames::Named(parameters) = &signature.parameters else {
        return Err(StaticError::NamedCallRequiresSignature {
            predicate: predicate.clone(),
            location: named_call_location(args),
        });
    };

    let mut values = vec![None; signature.arity];
    let mut seen_named = false;
    for (position, arg) in args.iter().enumerate() {
        match arg {
            CallArg::Positional { expr, location } => {
                if seen_named {
                    return Err(StaticError::PositionalAfterNamedArgument {
                        predicate: predicate.clone(),
                        location: location.clone(),
                    });
                }
                if position >= signature.arity {
                    let location = named_call_location(args);
                    return Err(arity_mismatch(predicate, signature, args.len(), &location));
                }
                values[position] = Some(Some(expr.clone()));
            }
            CallArg::Wildcard { location } => {
                if seen_named {
                    return Err(StaticError::PositionalAfterNamedArgument {
                        predicate: predicate.clone(),
                        location: location.clone(),
                    });
                }
                if position >= signature.arity {
                    let location = named_call_location(args);
                    return Err(arity_mismatch(predicate, signature, args.len(), &location));
                }
                values[position] = Some(None);
            }
            CallArg::Named {
                name,
                expr,
                location,
            } => {
                seen_named = true;
                let Some(index) = parameters.iter().position(|parameter| parameter == name) else {
                    return Err(StaticError::UnknownNamedArgument {
                        predicate: Box::new(predicate.clone()),
                        argument: name.clone(),
                        expected: expected_named_arguments(parameters),
                        location: location.clone(),
                    });
                };
                if values[index].is_some() {
                    return Err(StaticError::DuplicateNamedArgument {
                        predicate: predicate.clone(),
                        argument: name.clone(),
                        location: location.clone(),
                    });
                }
                if is_wildcard_expr(expr) {
                    values[index] = Some(None);
                } else {
                    values[index] = Some(Some(expr.clone()));
                }
            }
        }
    }

    if values.iter().any(Option::is_none) && !allow_omitted {
        let location = named_call_location(args);
        return Err(arity_mismatch(predicate, signature, args.len(), &location));
    }

    let location = named_call_location(args);
    Ok(values
        .into_iter()
        .map(|expr| match expr {
            Some(Some(expr)) => CallArg::Positional {
                expr,
                location: location.clone(),
            },
            Some(None) | None => CallArg::Wildcard {
                location: location.clone(),
            },
        })
        .collect())
}

fn expected_named_arguments(parameters: &[Ident]) -> Box<str> {
    parameters
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
        .into_boxed_str()
}

fn is_wildcard_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Var(var) if var.as_str() == "_")
}

fn named_call_location(args: &[CallArg]) -> SourceLocation {
    args.iter()
        .find_map(|arg| match arg {
            CallArg::Positional { .. } => None,
            CallArg::Named { location, .. } | CallArg::Wildcard { location } => {
                Some(location.clone())
            }
        })
        .unwrap_or_else(SourceLocation::unknown)
}

fn check_rule(
    rule: &Rule,
    signatures: &SignatureMap,
    edges: &mut Vec<DependencyEdge>,
    diagnostic_ids: &mut BTreeMap<String, SourceLocation>,
) -> Result<(), StaticError> {
    check_diagnostic_rule(rule, diagnostic_ids)?;
    check_head_safety(rule)?;
    check_body(&rule.head.predicate, &rule.body, signatures, edges)?;
    check_body_safety(&rule.head.predicate, &rule.body)
}

fn check_diagnostic_rule(
    rule: &Rule,
    diagnostic_ids: &mut BTreeMap<String, SourceLocation>,
) -> Result<(), StaticError> {
    if rule.head.predicate.name.as_str() != "diagnostic" {
        return Ok(());
    }

    let location = rule.origin().location().clone();
    let Some(Term::Expr(Expr::Literal(Literal::String(id)))) = rule.head.terms.first() else {
        return Err(StaticError::DiagnosticIdMustBeLiteral {
            predicate: rule.head.predicate.clone(),
            location,
        });
    };

    if let Some(first) = diagnostic_ids.get(id) {
        return Err(StaticError::DuplicateDiagnosticId {
            id: id.clone(),
            first: first.clone(),
            second: location,
        });
    }

    if is_reserved_diagnostic_id(id) && rule.origin().layer() != RuleLayer::Prelude {
        return Err(StaticError::ReservedDiagnosticId {
            id: id.clone(),
            location,
        });
    }

    diagnostic_ids.insert(id.clone(), location);
    Ok(())
}

fn is_reserved_diagnostic_id(id: &str) -> bool {
    matches!(id.as_bytes().first(), Some(b'E' | b'W' | b'I' | b'S'))
}

fn check_head_safety(rule: &Rule) -> Result<(), StaticError> {
    let mut bound = BTreeSet::new();
    collect_positive_body_vars(&rule.body, &mut bound);

    for term in &rule.head.terms {
        if let Some(expr) = term.expr() {
            let mut vars = BTreeSet::new();
            expr.variables(&mut vars);
            for variable in vars {
                if !bound.contains(&variable) {
                    return Err(StaticError::UnboundHeadVariable {
                        predicate: rule.head.predicate.clone(),
                        variable,
                        location: rule.head.location.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn check_body(
    head: &PredicateRef,
    body: &Body,
    signatures: &SignatureMap,
    edges: &mut Vec<DependencyEdge>,
) -> Result<(), StaticError> {
    check_body_with_dependency_kind(head, body, signatures, edges, DependencyKind::Positive)
}

fn check_body_with_dependency_kind(
    head: &PredicateRef,
    body: &Body,
    signatures: &SignatureMap,
    edges: &mut Vec<DependencyEdge>,
    default_dependency: DependencyKind,
) -> Result<(), StaticError> {
    for atom in &body.atoms {
        match atom {
            Atom::Stored(_)
            | Atom::Comparison(_)
            | Atom::Negation(Negation {
                atom: NegatedAtom::Stored(_),
                ..
            }) => {}
            Atom::Derived(derived) => {
                check_derived_dependency(
                    head,
                    derived,
                    default_dependency,
                    &derived.location,
                    signatures,
                    edges,
                )?;
            }
            Atom::Negation(negation) => {
                if let NegatedAtom::Derived(derived) = &negation.atom {
                    check_derived_dependency(
                        head,
                        derived,
                        DependencyKind::Negation,
                        &negation.location,
                        signatures,
                        edges,
                    )?;
                }
            }
            Atom::Aggregation(aggregate) => {
                check_body_with_dependency_kind(
                    head,
                    &aggregate.body,
                    signatures,
                    edges,
                    DependencyKind::Aggregation,
                )?;
            }
            Atom::TimeBlock(time_block) => {
                check_body_with_dependency_kind(
                    head,
                    &time_block.body,
                    signatures,
                    edges,
                    default_dependency,
                )?;
            }
        }
    }
    Ok(())
}

fn check_derived_dependency(
    head: &PredicateRef,
    derived: &DerivedAtom,
    kind: DependencyKind,
    edge_location: &SourceLocation,
    signatures: &SignatureMap,
    edges: &mut Vec<DependencyEdge>,
) -> Result<(), StaticError> {
    check_derived_call(
        signatures,
        &derived.predicate,
        derived.args.len(),
        &derived.location,
    )?;
    edges.push(DependencyEdge {
        from: head.clone(),
        to: derived.predicate.clone(),
        kind,
        location: edge_location.clone(),
    });
    Ok(())
}

fn check_derived_call(
    signatures: &SignatureMap,
    predicate: &PredicateRef,
    arity: usize,
    location: &SourceLocation,
) -> Result<(), StaticError> {
    match signatures.get(predicate) {
        Some(signature) if signature.arity == arity => Ok(()),
        Some(signature) => Err(arity_mismatch(predicate, signature, arity, location)),
        None => Err(unknown_predicate_error(
            predicate.clone(),
            arity,
            location.clone(),
            Some(signatures),
        )),
    }
}

fn check_body_safety(predicate: &PredicateRef, body: &Body) -> Result<(), StaticError> {
    check_body_safety_with_outer(predicate, body, &BTreeSet::new())
}

fn check_body_safety_with_outer(
    predicate: &PredicateRef,
    body: &Body,
    outer_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    let mut bound = outer_bound.clone();
    collect_positive_body_vars(body, &mut bound);

    for (atom_index, atom) in body.atoms.iter().enumerate() {
        match atom {
            Atom::Stored(stored) => {
                ensure_bound(
                    predicate,
                    stored_input_vars(stored),
                    &bound,
                    SafetyContext::Expression,
                    &stored.location,
                )?;
            }
            Atom::Derived(derived) => {
                ensure_bound(
                    predicate,
                    derived_input_vars(derived),
                    &bound,
                    SafetyContext::Expression,
                    &derived.location,
                )?;
                let outside_bound = positive_vars_outside(body, atom_index, outer_bound);
                check_primitive_input_safety(derived, &outside_bound)?;
            }
            Atom::Comparison(comparison) => {
                let mut vars = BTreeSet::new();
                comparison_vars(comparison, &mut vars);
                ensure_bound(
                    predicate,
                    vars,
                    &bound,
                    SafetyContext::Expression,
                    &comparison.location,
                )?;
            }
            Atom::Negation(negated) => {
                ensure_bound(
                    predicate,
                    negated_vars(&negated.atom),
                    &bound,
                    SafetyContext::Negation,
                    negated.location(),
                )?;
            }
            Atom::Aggregation(aggregate) => {
                let outside_bound = positive_vars_outside(body, atom_index, outer_bound);
                check_aggregate_safety(predicate, aggregate, &outside_bound)?;
            }
            Atom::TimeBlock(time_block) => {
                let outside_bound = positive_vars_outside(body, atom_index, outer_bound);
                ensure_bound(
                    predicate,
                    positive_body_input_vars(&time_block.body),
                    &bound,
                    SafetyContext::Expression,
                    &time_block.location,
                )?;
                check_body_safety_with_outer(predicate, &time_block.body, &outside_bound)?;
            }
        }
    }
    Ok(())
}

fn positive_vars_outside(
    body: &Body,
    excluded_index: usize,
    outer_bound: &BTreeSet<Ident>,
) -> BTreeSet<Ident> {
    let mut outside_bound = outer_bound.clone();
    for (other_index, other) in body.atoms.iter().enumerate() {
        if other_index != excluded_index {
            collect_positive_atom_vars(other, &mut outside_bound);
        }
    }
    outside_bound
}

fn check_primitive_input_safety(
    atom: &DerivedAtom,
    outside_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) else {
        return Ok(());
    };
    check_graph_primitive_anchor_safety(atom, primitive, outside_bound)?;
    check_content_primitive_input_safety(atom, primitive, outside_bound)
}

fn check_graph_primitive_anchor_safety(
    atom: &DerivedAtom,
    primitive: PrimitivePredicate,
    outside_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    let Some(anchor_positions) = primitive.graph_anchor_positions() else {
        return Ok(());
    };
    if anchor_positions.iter().any(|idx| {
        atom.args.get(*idx).is_some_and(|arg| {
            arg.expr()
                .is_some_and(|expr| expr_is_bound_by(expr, outside_bound))
        })
    }) {
        return Ok(());
    }
    Err(StaticError::UnboundGraphPrimitiveAnchor {
        predicate: atom.predicate.clone(),
        location: atom.location.clone(),
    })
}

fn check_content_primitive_input_safety(
    atom: &DerivedAtom,
    primitive: PrimitivePredicate,
    outside_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    for input in primitive.required_bound_inputs() {
        let Some(arg) = atom.args.get(input.position) else {
            continue;
        };
        if !arg
            .expr()
            .is_some_and(|expr| expr_is_bound_by(expr, outside_bound))
        {
            return Err(StaticError::UnboundPrimitiveInput {
                predicate: atom.predicate.clone(),
                argument: input.argument,
                location: atom.location.clone(),
            });
        }
    }
    Ok(())
}

fn expr_is_bound_by(expr: &Expr, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    vars.iter().all(|var| bound.contains(var))
}

fn check_aggregate_safety(
    predicate: &PredicateRef,
    aggregate: &Aggregate,
    outside_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    let mut body_bound = BTreeSet::new();
    collect_positive_body_vars(&aggregate.body, &mut body_bound);
    let mut row_bound = outside_bound.clone();
    row_bound.extend(body_bound);

    let rank_var = rank_arg_variable(aggregate);

    let mut value_vars = BTreeSet::new();
    aggregate.value.variables(&mut value_vars);
    if let Some(rank_var) = &rank_var {
        value_vars.remove(rank_var);
    }
    ensure_bound(
        predicate,
        value_vars,
        &row_bound,
        SafetyContext::Expression,
        &aggregate.location,
    )?;

    for arg in &aggregate.args {
        let mut arg_vars = BTreeSet::new();
        arg.expr.variables(&mut arg_vars);
        if aggregate.function == AggregateFunction::Rank && arg.name.as_str() == "rank" {
            continue;
        }
        let required_bound = if matches!(
            (aggregate.function, arg.name.as_str()),
            (AggregateFunction::TopK, "k") | (AggregateFunction::TakeUntil, "budget")
        ) {
            outside_bound
        } else {
            &row_bound
        };
        ensure_bound(
            predicate,
            arg_vars,
            required_bound,
            SafetyContext::Expression,
            &aggregate.location,
        )?;
    }
    check_body_safety_with_outer(predicate, &aggregate.body, outside_bound)
}

fn rank_arg_variable(aggregate: &Aggregate) -> Option<Ident> {
    if aggregate.function != AggregateFunction::Rank {
        return None;
    }
    aggregate
        .args
        .iter()
        .find(|arg| arg.name.as_str() == "rank")
        .and_then(|arg| match &arg.expr {
            Expr::Var(var) => Some(var.clone()),
            _ => None,
        })
}

#[derive(Clone, Copy)]
enum SafetyContext {
    Expression,
    Negation,
}

fn ensure_bound(
    predicate: &PredicateRef,
    vars: BTreeSet<Ident>,
    bound: &BTreeSet<Ident>,
    context: SafetyContext,
    location: &SourceLocation,
) -> Result<(), StaticError> {
    for variable in vars {
        if !bound.contains(&variable) {
            return match context {
                SafetyContext::Expression => Err(StaticError::UnboundExpressionVariable {
                    predicate: predicate.clone(),
                    variable,
                    location: location.clone(),
                }),
                SafetyContext::Negation => Err(StaticError::UnboundNegationVariable {
                    predicate: predicate.clone(),
                    variable,
                    location: location.clone(),
                }),
            };
        }
    }
    Ok(())
}

fn check_cyclic_stratification(edges: &[DependencyEdge]) -> Result<(), StaticError> {
    for edge in edges.iter().filter(|edge| edge.kind.is_stratifying()) {
        if let Some(mut path) = find_path(edges, &edge.to, &edge.from) {
            path.insert(0, edge.from.clone());
            path.insert(1, edge.to.clone());
            path.dedup();
            let cycle = DependencyCycle::new(path);
            return match edge.kind {
                DependencyKind::Negation => Err(StaticError::CyclicNegation {
                    cycle,
                    location: edge.location.clone(),
                }),
                DependencyKind::Aggregation => Err(StaticError::CyclicStratification {
                    cycle,
                    location: edge.location.clone(),
                }),
                DependencyKind::Positive => unreachable!("filtered to stratifying edges"),
            };
        }
    }
    Ok(())
}

fn find_path(
    edges: &[DependencyEdge],
    start: &PredicateRef,
    target: &PredicateRef,
) -> Option<Vec<PredicateRef>> {
    let mut stack = vec![(start.clone(), vec![start.clone()])];
    let mut seen = BTreeSet::new();
    while let Some((current, path)) = stack.pop() {
        if &current == target {
            return Some(path);
        }
        if !seen.insert(current.clone()) {
            continue;
        }
        for edge in edges.iter().filter(|edge| edge.from == current) {
            let mut next_path = path.clone();
            next_path.push(edge.to.clone());
            stack.push((edge.to.clone(), next_path));
        }
    }
    None
}

fn compute_strata(
    signatures: &SignatureMap,
    edges: &[DependencyEdge],
    only: Option<&BTreeSet<PredicateRef>>,
) -> Vec<Stratum> {
    let mut levels: BTreeMap<PredicateRef, usize> = signatures
        .iter()
        .filter(|(_, signature)| signature.kind.is_derived())
        .map(|(predicate, _)| predicate)
        .filter(|predicate| only.is_none_or(|wanted| wanted.contains(*predicate)))
        .map(|predicate| (predicate.clone(), 0))
        .collect();
    let max_iterations = levels.len().saturating_mul(levels.len()).max(1);
    for _ in 0..max_iterations {
        let mut changed = false;
        for edge in edges {
            if !levels.contains_key(&edge.from) {
                continue;
            }
            let required = levels.get(&edge.to).copied().unwrap_or(0)
                + usize::from(edge.kind.is_stratifying());
            let entry = levels.entry(edge.from.clone()).or_default();
            if *entry < required {
                *entry = required;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut grouped: BTreeMap<usize, Vec<PredicateRef>> = BTreeMap::new();
    for (predicate, level) in levels {
        grouped.entry(level).or_default().push(predicate);
    }
    grouped
        .into_iter()
        .map(|(level, predicates)| Stratum { level, predicates })
        .collect()
}

fn comparison_vars(comparison: &Comparison, out: &mut BTreeSet<Ident>) {
    comparison.left.variables(out);
    comparison.right.variables(out);
}

fn negated_vars(negated: &NegatedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    match negated {
        NegatedAtom::Stored(stored) => vars.extend(stored_vars(stored)),
        NegatedAtom::Derived(derived) => {
            for arg in &derived.args {
                if let Some(expr) = arg.expr() {
                    expr.variables(&mut vars);
                }
            }
        }
    }
    vars
}

fn collect_positive_atom_vars(atom: &Atom, out: &mut BTreeSet<Ident>) {
    atom.collect_positive_binding_variables(out);
}

fn collect_positive_body_vars(body: &Body, out: &mut BTreeSet<Ident>) {
    body.collect_positive_binding_variables(out);
}

fn stored_vars(stored: &StoredAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for field in &stored.fields {
        if let Term::Expr(expr) = &field.term {
            expr.variables(&mut vars);
        }
    }
    vars
}

fn stored_input_vars(stored: &StoredAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for field in &stored.fields {
        if let Term::Expr(expr) = &field.term {
            expr.input_variables(&mut vars);
        }
    }
    vars
}

fn derived_input_vars(derived: &DerivedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for arg in &derived.args {
        if let Some(expr) = arg.expr() {
            expr.input_variables(&mut vars);
        }
    }
    vars
}

fn positive_body_input_vars(body: &Body) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => vars.extend(stored_input_vars(stored)),
            Atom::Derived(derived) => vars.extend(derived_input_vars(derived)),
            Atom::TimeBlock(time_block) => vars.extend(positive_body_input_vars(&time_block.body)),
            Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::runtime::loader::{load_prelude, load_program};
    use crate::runtime::parser::{parse_prelude_program, parse_program};

    fn analyze_err(source: &str, input: &str) -> StaticError {
        let program = parse_program(source, input).expect("program parses");
        analyze(program).expect_err("program rejected")
    }

    fn location(source: &str, input: &str, needle: &str) -> SourceLocation {
        let column = input.find(needle).expect("needle appears") + 1;
        SourceLocation::new(source, 1, column)
    }

    #[test]
    fn rejects_unbound_head_variable() {
        let input = r"bad(h) := *handle{id: other}.";
        let err = analyze_err("inline", input);
        let StaticError::UnboundHeadVariable {
            location: actual, ..
        } = &err
        else {
            panic!("expected unbound head variable");
        };
        assert_eq!(actual, &location("inline", input, "bad(h)"));
        assert!(err.to_string().contains("inline:1:1"));
    }

    #[test]
    fn rejects_unbound_negation_variable() {
        let input =
            r"terminal(h) := *handle{id: h}. bad(h) := *handle{id: h}, not terminal(missing).";
        let err = analyze_err("inline", input);
        let StaticError::UnboundNegationVariable {
            location: actual, ..
        } = &err
        else {
            panic!("expected unbound negation variable");
        };
        assert_eq!(actual, &location("inline", input, "not terminal(missing)"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "not terminal(missing)").to_string())
        );
    }

    #[test]
    fn accepts_negation_bound_later_in_the_same_rule() {
        let program = parse_program(
            "inline",
            r#"
            terminal(h) := *handle{id: h, status: "resolved"}.
            ok(h) := not terminal(h), *handle{id: h}.
            "#,
        )
        .unwrap();
        analyze(program).expect("negation safety is order-independent");
    }

    #[test]
    fn rejects_unbound_comparison_variable() {
        let input = r"bad(h) := *handle{id: h}, h = missing.";
        let err = analyze_err("inline", input);
        let StaticError::UnboundExpressionVariable {
            location: actual, ..
        } = &err
        else {
            panic!("expected unbound expression variable");
        };
        assert_eq!(actual, &location("inline", input, "h = missing"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "h = missing").to_string())
        );
    }

    #[test]
    fn expression_inputs_do_not_satisfy_positive_binding_safety() {
        let input = r"bad(x) := *pair{next: x + 1}.";
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnboundHeadVariable { variable, .. } if variable.as_str() == "x"
        ));
    }

    #[test]
    fn positive_expression_inputs_may_be_bound_later() {
        let program = parse_program(
            "inline",
            r"
            ok(x) := *pair{next: x + 1}, *pair{n: x}.
            ",
        )
        .expect("program parses");
        analyze(program).expect("expression input is bound elsewhere in the body");
    }

    #[test]
    fn aggregate_body_variables_do_not_satisfy_head_safety() {
        let input = r#"item("a", "h"). bad(area, n) := n = Count{ h : item(area, h) }."#;
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnboundHeadVariable { variable, .. } if variable.as_str() == "area"
        ));
    }

    #[test]
    fn aggregate_group_level_args_must_be_bound_outside_aggregate() {
        let input = r#"limit("a", 2). score("a", 5). bad(h) := (h, s) = TopK{ k: k, key: s : (h, s) : limit(h, k), score(h, s) }."#;
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnboundExpressionVariable { variable, .. } if variable.as_str() == "k"
        ));
    }

    #[test]
    fn rank_generated_variable_is_not_available_to_key_arg() {
        let input = r#"score("a", 5). bad(h, rank) := (h, rank) = Rank{ key: rank, rank: rank : (h, rank) : score(h, score) }."#;
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnboundExpressionVariable { variable, .. } if variable.as_str() == "rank"
        ));
    }

    #[test]
    fn rejects_unknown_predicates() {
        let input = r"bad(h) := missing(h).";
        let err = analyze_err("inline", input);
        let StaticError::UnknownPredicate {
            location: actual, ..
        } = &err
        else {
            panic!("expected unknown predicate");
        };
        assert_eq!(actual, &location("inline", input, "missing(h)"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "missing(h)").to_string())
        );
    }

    #[test]
    fn unknown_predicate_suggests_close_schema_candidate() {
        let input = r#"
        potential("x", 1).
        ? potentiel(h, energy).
        "#;
        let err = analyze_err("inline", input);

        assert!(
            err.to_string().contains("Did you mean `potential/2`?"),
            "{err}"
        );
    }

    #[test]
    fn unknown_predicate_teaches_retired_alias_replacement() {
        let input = r"? recent(h, 7).";
        let err = analyze_err("inline", input);

        assert!(
            err.to_string()
                .contains("Predicate 'recent' was retired; use `changed_within(h, days)`"),
            "{err}"
        );
    }

    #[test]
    fn unknown_predicate_suggests_stored_relation_prefix() {
        let input = r#"? handle(id: h, kind: "file")."#;
        let err = analyze_err("inline", input);
        assert!(
            err.to_string().contains("Did you mean '*handle{...}'?"),
            "{err}"
        );
        assert!(
            err.to_string()
                .contains("stored relations use a '*' prefix and named fields"),
            "{err}"
        );
    }

    #[test]
    fn rejects_unanchored_graph_primitive_traversal() {
        let input = r"? upstream(h, anc).";
        let err = analyze_err("inline", input);
        let StaticError::UnboundGraphPrimitiveAnchor {
            predicate,
            location: actual,
        } = &err
        else {
            panic!("expected unbound graph primitive anchor");
        };
        assert_eq!(predicate.name.as_str(), "upstream");
        assert_eq!(actual, &location("inline", input, "upstream(h, anc)"));
    }

    #[test]
    fn graph_primitive_depth_does_not_count_as_anchor() {
        let input = r"? neighborhood(h, 1, member).";
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnboundGraphPrimitiveAnchor { predicate, .. }
                if predicate.name.as_str() == "neighborhood"
        ));
    }

    #[test]
    fn rejects_unanchored_downstream_and_impact_traversal() {
        for (name, input) in [
            ("downstream", r"? downstream(h, desc)."),
            ("impact", r"? impact(h, x, depth)."),
            ("impact", r"? impact(h, x, 1)."),
        ] {
            let err = analyze_err("inline", input);
            assert!(
                matches!(
                    err,
                    StaticError::UnboundGraphPrimitiveAnchor { ref predicate, .. }
                        if predicate.name.as_str() == name
                ),
                "{input} should reject with unbound graph primitive anchor, got {err:?}"
            );
        }
    }

    #[test]
    fn accepts_graph_primitive_with_bound_reverse_endpoint() {
        let program = parse_program(
            "inline",
            r#"
            target("OQ-22").
            ? target(anc), upstream(h, anc).
            "#,
        )
        .expect("program parses");
        analyze(program).expect("reverse endpoint anchors graph traversal");
    }

    #[test]
    fn rejects_content_primitives_without_required_bound_inputs() {
        for (input, name, argument) in [
            (
                r"? search(query, handle, span_id, score, reason, field, low_confidence).",
                "search",
                "query",
            ),
            (
                r"? read(h, 100, span_id, text, start_line, end_line, tokens).",
                "read",
                "handle",
            ),
            (
                r#"? read("doc.md", budget, span_id, text, start_line, end_line, tokens)."#,
                "read",
                "budget",
            ),
            (r"? read_full(h, content).", "read_full", "handle"),
            (
                r"? match(pattern, handle, line, snippet).",
                "match",
                "pattern",
            ),
            (r#"? match(".", handle, line, snippet)."#, "match", "handle"),
        ] {
            let err = analyze_err("inline", input);
            assert!(
                matches!(
                    err,
                    StaticError::UnboundPrimitiveInput {
                        ref predicate,
                        argument: actual,
                        ..
                    } if predicate.name.as_str() == name && actual == argument
                ),
                "{input} should reject unbound {argument}, got {err:?}"
            );
        }
    }

    #[test]
    fn accepts_content_primitives_with_required_bound_inputs() {
        let program = parse_program(
            "inline",
            r#"
            budget(100).
            pattern("urgent").
            query("conformance").
            ? query(q), search(q, h, span_id, score, reason, field, low_confidence).
            ? *handle{id: h}, budget(b), read(h, b, span_id, text, start_line, end_line, tokens).
            ? read_full("doc.md", content).
            ? *handle{id: h}, pattern(p), match(p, h, line, snippet).
            "#,
        )
        .expect("program parses");

        analyze(program).expect("content primitive inputs are bound");
    }

    #[test]
    fn accepts_engine_primitive_calls_without_rule_definitions() {
        let program = parse_program(
            "inline",
            r#"
            impacted(x) := upstream(h: "formal-model/v17.md", anc: x).
            ? impacted(x).
            "#,
        )
        .unwrap();
        analyze(program).expect("engine primitives have builtin signatures");
    }

    #[test]
    fn rejects_engine_primitive_rule_definitions() {
        let input = r"upstream(h, anc) := *edge{from: h, to: anc}.";
        let err = analyze_err("inline", input);
        let StaticError::PrimitiveRedefinition {
            location: actual, ..
        } = &err
        else {
            panic!("expected primitive redefinition");
        };
        assert_eq!(actual, &location("inline", input, "upstream(h, anc)"));
        assert!(
            err.to_string()
                .contains("engine primitive 'upstream' cannot be defined")
        );
    }

    #[test]
    fn accepts_soft_lifecycle_primitive_rule_definitions() {
        let program = parse_program(
            "inline",
            r#"
            terminal(handle) := *handle{id: handle, status: "done"}.
            ? terminal(handle: h).
            "#,
        )
        .unwrap();
        analyze(program).expect("soft lifecycle primitives can be shadowed");
    }

    #[test]
    fn rejects_soft_lifecycle_primitive_arity_mismatch() {
        let input = r"terminal(h, reason) := *handle{id: h}.";
        let err = analyze_err("inline", input);
        assert!(matches!(err, StaticError::ArityMismatch { .. }));
    }

    #[test]
    fn rejects_arity_mismatch_with_call_location() {
        let input = r#"known("a"). bad(h) := known(h, h)."#;
        let err = analyze_err("inline", input);
        let StaticError::ArityMismatch {
            location: actual, ..
        } = &err
        else {
            panic!("expected arity mismatch");
        };
        assert_eq!(actual, &location("inline", input, "known(h, h)"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "known(h, h)").to_string())
        );
    }

    #[test]
    fn arity_mismatch_reports_expected_signature() {
        let err = analyze_err("inline", "? verbs(name, doc).");

        assert!(
            err.to_string()
                .contains("signature: verbs(name, query, doc, output_schema)"),
            "{err}"
        );
    }

    #[test]
    fn rejects_unknown_named_call_arguments() {
        let input = r#"left("a"). right("b"). pair(left, right) := left(left), right(right). ? pair(missing: x, left: y)."#;
        let err = analyze_err("inline", input);
        let StaticError::UnknownNamedArgument {
            argument,
            location: actual,
            ..
        } = &err
        else {
            panic!("expected unknown named argument");
        };
        assert_eq!(argument.as_str(), "missing");
        assert_eq!(actual, &location("inline", input, "missing: x"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "missing: x").to_string())
        );
    }

    #[test]
    fn primitive_named_calls_reject_unknown_parameter_names() {
        let input = r"? predicates(name: n, arity: a).";
        let err = analyze_err("inline", input);
        let StaticError::UnknownNamedArgument {
            predicate,
            argument,
            location: actual,
            ..
        } = &err
        else {
            panic!("expected unknown named argument");
        };
        assert_eq!(predicate.name.as_str(), "predicates");
        assert_eq!(argument.as_str(), "arity");
        assert_eq!(actual, &location("inline", input, "arity: a"));
        assert!(err.to_string().contains("unknown named argument 'arity'"));
        assert!(
            err.to_string()
                .contains(&location("inline", input, "arity: a").to_string())
        );
    }

    #[test]
    fn explicit_predicate_signature_enables_constant_head_pattern_calls() {
        let input = r#"
        @predicate(name: "event", args: ["code", "severity", "subject", "file", "line", "evidence"]).
        source("h1", "a.md", 1, "broken").
        event("E001", "error", h, file, line, evidence) := source(h, file, line, evidence).
        ? event{code: "E001", subject: h}.
        "#;
        analyze(parse_program("inline", input).expect("program parses"))
            .expect("explicit signature analyzes");
    }

    #[test]
    fn rejects_invalid_explicit_predicate_name() {
        let input = r#"@predicate(name: "Bad.Name", args: ["code"])."#;
        let err = analyze_err("inline", input);
        let StaticError::PredicateInvalidName { name, .. } = &err else {
            panic!("expected invalid predicate name");
        };
        assert_eq!(name.as_ref(), "Bad.Name");
        assert!(err.to_string().contains("not a valid predicate name"));
    }

    #[test]
    fn relation_pattern_unknown_field_reports_expected_names() {
        let input = r#"
        @predicate(name: "diagnostic", args: ["code", "severity", "subject"]).
        ? diagnostic{severty: "error"}.
        "#;
        let err = analyze_err("inline", input);
        let StaticError::UnknownNamedArgument {
            argument, expected, ..
        } = &err
        else {
            panic!("expected unknown named argument");
        };
        assert_eq!(argument.as_str(), "severty");
        assert!(expected.contains("severity"));
        assert!(
            err.to_string()
                .contains("expected one of: code, severity, subject")
        );
    }

    #[test]
    fn rejects_unknown_stored_relation_fields() {
        let input = r"? *handle{id: h, namspace: ns}.";
        let err = analyze_err("inline", input);
        let StaticError::UnknownStoredField {
            relation,
            field,
            expected,
            location: actual,
        } = &err
        else {
            panic!("expected unknown stored field");
        };
        assert_eq!(relation.as_str(), "handle");
        assert_eq!(field.as_str(), "namspace");
        assert!(expected.as_slice().contains(&"namespace"));
        assert!(expected.to_string().contains("namespace"));
        assert_eq!(actual, &location("inline", input, "namspace"));
        assert!(err.to_string().contains("expected one of"));
    }

    #[test]
    fn rejects_unknown_stored_relation_fields_inside_at_blocks() {
        let input = r#"at("snap") { bad(h) := *handle{namspace: h}. } ? bad(h)."#;
        let err = analyze_err("inline", input);
        assert!(matches!(
            err,
            StaticError::UnknownStoredField { field, .. } if field.as_str() == "namspace"
        ));
    }

    #[test]
    fn rejects_named_calls_without_named_signature() {
        let input = r#"pair("a", "b"). ? pair(left: x, right: y)."#;
        let err = analyze_err("inline", input);
        let StaticError::NamedCallRequiresSignature {
            location: actual, ..
        } = &err
        else {
            panic!("expected missing named signature");
        };
        assert_eq!(actual, &location("inline", input, "left: x"));
    }

    #[test]
    fn rejects_named_calls_to_ambiguous_signatures() {
        let input = r#"seed("a", "b"). pair(left, right) := seed(left, right). pair(src, dst) := seed(src, dst). ? pair(left: x, right: y)."#;
        let err = analyze_err("inline", input);
        let StaticError::NamedCallRequiresSignature {
            location: actual, ..
        } = &err
        else {
            panic!("expected ambiguous named signature");
        };
        assert_eq!(actual, &location("inline", input, "left: x"));
    }

    #[test]
    fn rejects_positional_arguments_after_named_arguments() {
        let input = r#"left("a"). right("b"). pair(left, right) := left(left), right(right). ? pair(right: r, l)."#;
        let err = analyze_err("inline", input);
        let StaticError::PositionalAfterNamedArgument {
            location: actual, ..
        } = &err
        else {
            panic!("expected positional-after-named error");
        };
        let column = input.find("right: r, l").expect("needle appears") + "right: r, ".len() + 1;
        assert_eq!(actual, &SourceLocation::new("inline", 1, column));
    }

    #[test]
    fn rejects_named_argument_that_duplicates_positional_slot() {
        let input = r#"left("a"). right("b"). pair(left, right) := left(left), right(right). ? pair(l, left: x)."#;
        let err = analyze_err("inline", input);
        let StaticError::DuplicateNamedArgument {
            argument,
            location: actual,
            ..
        } = &err
        else {
            panic!("expected duplicate named argument");
        };
        assert_eq!(argument.as_str(), "left");
        assert_eq!(actual, &location("inline", input, "left: x"));
    }

    #[test]
    fn rejects_named_function_arguments_with_location() {
        let input = r#"bad(h) := *handle{id: h}, lower(value: h) = "x"."#;
        let err = analyze_err("inline", input);
        let StaticError::UnsupportedNamedFunctionArgument {
            argument,
            location: actual,
            ..
        } = &err
        else {
            panic!("expected named function argument error");
        };
        assert_eq!(argument.as_str(), "value");
        assert_eq!(actual, &location("inline", input, "value: h"));
    }

    #[test]
    fn rejects_non_literal_diagnostic_id() {
        let program = parse_program(
            "checks.dl",
            r#"diagnostic(code, "error") := *handle{id: h}."#,
        )
        .unwrap();
        let err = analyze(program).expect_err("diagnostic id rejected");
        assert!(matches!(err, StaticError::DiagnosticIdMustBeLiteral { .. }));
    }

    #[test]
    fn rejects_duplicate_diagnostic_ids() {
        let program = parse_program(
            "checks.dl",
            r#"
            diagnostic("PROJ-001", "error", h) := *handle{id: h}.
            diagnostic("PROJ-001", "warning", h) := *handle{id: h}.
            "#,
        )
        .unwrap();
        let err = analyze(program).expect_err("duplicate diagnostic id rejected");
        assert!(matches!(err, StaticError::DuplicateDiagnosticId { .. }));
    }

    #[test]
    fn rejects_project_reserved_diagnostic_prefixes() {
        let program = parse_program(
            "checks.dl",
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        )
        .unwrap();
        let err = analyze(program).expect_err("reserved diagnostic id rejected");
        assert!(matches!(err, StaticError::ReservedDiagnosticId { .. }));
    }

    #[test]
    fn loaded_project_rejects_reserved_diagnostic_prefixes() {
        let root = tempfile::tempdir().expect("temp rule root");
        fs::write(
            root.path().join("anneal.dl"),
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        )
        .expect("write project rules");

        let program = load_program(root.path(), "anneal.dl").expect("project loads");
        let err = analyze(program).expect_err("reserved diagnostic id rejected");
        assert!(matches!(err, StaticError::ReservedDiagnosticId { .. }));
    }

    #[test]
    fn loaded_prelude_allows_reserved_diagnostic_prefixes() {
        let root = tempfile::tempdir().expect("temp rule root");
        fs::write(
            root.path().join("checks.dl"),
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        )
        .expect("write prelude rules");

        let program = load_prelude(root.path(), "checks.dl").expect("prelude loads");
        analyze(program).expect("prelude diagnostics analyze");
    }

    #[test]
    fn duplicate_diagnostic_id_beats_reserved_prefix_error() {
        let mut program = parse_prelude_program(
            "prelude.dl",
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        )
        .unwrap();

        program.statements.extend(
            parse_program(
                "project.dl",
                r#"diagnostic("E001", "warning", h) := *handle{id: h}."#,
            )
            .unwrap()
            .statements,
        );

        let err = analyze(program).expect_err("duplicate id rejected");
        assert!(matches!(err, StaticError::DuplicateDiagnosticId { .. }));
    }

    #[test]
    fn loaded_child_file_diagnostic_locations_survive_analysis() {
        let root = tempfile::tempdir().expect("temp rule root");
        fs::create_dir_all(root.path().join("checks")).expect("create checks dir");
        fs::write(root.path().join("anneal.dl"), r#"include "checks/bad.dl"."#)
            .expect("write entry");
        fs::write(root.path().join("checks/bad.dl"), r"bad(h) := missing(h).")
            .expect("write child");

        let program = load_program(root.path(), "anneal.dl").expect("program loads");
        let err = analyze(program).expect_err("unknown predicate rejected");
        let StaticError::UnknownPredicate {
            location: actual, ..
        } = err
        else {
            panic!("expected unknown predicate");
        };
        assert_eq!(
            actual,
            SourceLocation::new("checks/bad.dl", 1, "bad(h) := ".len() + 1)
        );
    }

    #[test]
    fn loaded_import_qualification_survives_analysis() {
        let root = tempfile::tempdir().expect("temp rule root");
        fs::write(
            root.path().join("anneal.dl"),
            r#"
            active(h) := *handle{id: h}.
            import strict from "strict.dl".
            ? strict.blocker(h).
            "#,
        )
        .expect("write project");
        fs::write(root.path().join("strict.dl"), r"blocker(h) := active(h).")
            .expect("write import");

        let program = load_program(root.path(), "anneal.dl").expect("program loads");
        analyze(program).expect("imported rule can call global predicate");
    }

    #[test]
    fn query_local_rules_are_scoped_into_query_analysis() {
        let program = parse_program(
            "inline",
            r"
            ?
              where local(h) := *handle{id: h}.
              local(h).
            ",
        )
        .unwrap();
        let analyzed = analyze(program).expect("local rule analyzes");
        let query = analyzed.queries().next().expect("query");
        assert_eq!(query.local_predicates().count(), 1);
    }

    #[test]
    fn facts_contribute_predicate_signatures() {
        let program = parse_program("inline", r#"seed("a"). ? seed(h)."#).unwrap();
        let analyzed = analyze(program).expect("fact signature analyzes");
        assert!(
            analyzed
                .predicates()
                .any(|predicate| predicate.display_name() == "seed")
        );
    }

    #[test]
    fn rejects_cyclic_negation_with_both_predicates_named() {
        let input = r"blocked(h) := *handle{id: h}, not advancing(h). advancing(h) := *handle{id: h}, not blocked(h).";
        let err = analyze_err("anneal.dl", input);
        let StaticError::CyclicNegation {
            cycle,
            location: actual,
        } = &err
        else {
            panic!("expected cyclic negation");
        };
        assert_eq!(actual, &location("anneal.dl", input, "not advancing(h)"));
        let names = cycle
            .predicates()
            .iter()
            .map(PredicateRef::display_name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "blocked"));
        assert!(names.iter().any(|name| name == "advancing"));
    }

    #[test]
    fn computes_higher_stratum_for_negative_dependency() {
        let program = parse_program(
            "inline",
            r#"
            terminal(h) := *handle{id: h, status: "resolved"}.
            open(h) := *handle{id: h}, not terminal(h).
            "#,
        )
        .unwrap();
        let analyzed = analyze(program).expect("program analyzes");
        let open = PredicateRef::new(Ident::new_unchecked("open"));
        let open_stratum = analyzed
            .strata()
            .iter()
            .find(|stratum| stratum.predicates.contains(&open))
            .expect("open stratum");
        assert_eq!(open_stratum.level, 1);
    }

    #[test]
    fn computes_higher_stratum_for_aggregate_body_dependency() {
        let program = parse_program(
            "inline",
            r"
            value(h, 1) := *handle{id: h}.
            subject(h) := value(h, n).
            total(h, n) := subject(h), n = Sum{ v : value(h, v) }.
            ",
        )
        .unwrap();
        let analyzed = analyze(program).expect("program analyzes");
        let value = PredicateRef::new(Ident::new_unchecked("value"));
        let total = PredicateRef::new(Ident::new_unchecked("total"));
        let value_level = analyzed
            .strata()
            .iter()
            .find(|stratum| stratum.predicates.contains(&value))
            .expect("value stratum")
            .level;
        let total_level = analyzed
            .strata()
            .iter()
            .find(|stratum| stratum.predicates.contains(&total))
            .expect("total stratum")
            .level;
        assert!(total_level > value_level);
    }

    #[test]
    fn rejects_cyclic_aggregate_dependency() {
        let input = "total(h, n) := *handle{id: h}, n = Sum{ v : total(h, v) }.";
        let program = parse_program("inline", input).unwrap();
        let err = analyze(program).expect_err("cyclic aggregate should fail");
        let StaticError::CyclicStratification {
            cycle,
            location: actual,
        } = &err
        else {
            panic!("expected cyclic stratification");
        };
        assert_eq!(actual, &location("inline", input, "total(h, v)"));
        let names = cycle
            .predicates()
            .iter()
            .map(PredicateRef::display_name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "total"));
    }
}
