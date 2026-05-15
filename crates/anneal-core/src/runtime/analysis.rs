use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, DerivedAtom, Expr, Head, Ident,
    Literal, NegatedAtom, Negation, PredicateRef, Program, Query, Rule, RuleLayer, SourceLocation,
    Statement, StoredAtom, Term,
};
use crate::runtime::primitives::{PrimitivePredicate, PrimitiveSignature, primitive_signatures};

type SignatureMap = BTreeMap<PredicateRef, PredicateSignature>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PredicateSignature {
    arity: usize,
    parameters: ParameterNames,
    kind: PredicateKind,
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
    #[error("{location}: unknown predicate '{predicate}/{arity}'")]
    UnknownPredicate {
        predicate: PredicateRef,
        arity: usize,
        location: SourceLocation,
    },
    #[error("{location}: predicate '{predicate}' used with arity {actual}, expected {expected}")]
    ArityMismatch {
        predicate: PredicateRef,
        expected: usize,
        actual: usize,
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
        cycle: NegationCycle,
        location: SourceLocation,
    },
    #[error("{location}: named call to '{predicate}' requires a named predicate signature")]
    NamedCallRequiresSignature {
        predicate: PredicateRef,
        location: SourceLocation,
    },
    #[error("{location}: unknown named argument '{argument}' for '{predicate}'")]
    UnknownNamedArgument {
        predicate: PredicateRef,
        argument: Ident,
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
pub struct NegationCycle {
    predicates: Vec<PredicateRef>,
}

impl NegationCycle {
    fn new(predicates: Vec<PredicateRef>) -> Self {
        Self { predicates }
    }

    pub fn predicates(&self) -> &[PredicateRef] {
        &self.predicates
    }
}

impl fmt::Display for NegationCycle {
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
        self.collect_global_signatures()?;
        normalize_global_named_calls(&mut self.program, &self.signatures)?;
        self.check_facts()?;
        self.check_rules()?;
        check_cyclic_negation(&self.edges)?;
        let strata = compute_strata(&self.signatures, &self.edges, None);
        let queries = self.check_queries()?;
        Ok(AnalyzedProgram {
            program: self.program,
            signatures: self.signatures,
            strata,
            queries,
        })
    }

    fn collect_global_signatures(&mut self) -> Result<(), StaticError> {
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
        check_cyclic_negation(&edges)?;
        let local_strata = compute_strata(&signatures, &edges, Some(&local_predicates));

        Ok(AnalyzedQuery {
            query,
            local_strata,
            local_predicates,
        })
    }
}

#[derive(Clone, Debug)]
struct DependencyEdge {
    from: PredicateRef,
    to: PredicateRef,
    negative: bool,
    location: SourceLocation,
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
                return Err(StaticError::ArityMismatch {
                    predicate: head.predicate.clone(),
                    expected: signature.arity,
                    actual: arity,
                    location: head.location.clone(),
                });
            }
            signature.parameters = parameters;
            signature.kind = PredicateKind::Derived;
            Ok(())
        }
        Some(signature) if signature.arity != arity => Err(StaticError::ArityMismatch {
            predicate: head.predicate.clone(),
            expected: signature.arity,
            actual: arity,
            location: head.location.clone(),
        }),
        Some(signature) => {
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
        Statement::AtBlock { .. } => Ok(()),
        Statement::Query(_) | Statement::Include(_) | Statement::Import(_) | Statement::Verb(_) => {
            Ok(())
        }
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
    for field in &mut stored.fields {
        if let Some(expr) = field.term.expr_mut() {
            normalize_expr_named_calls(expr)?;
        }
    }
    Ok(())
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
                normalize_expr_named_calls(arg.expr_mut())?;
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
    if !derived
        .args
        .iter()
        .any(|arg| matches!(arg, CallArg::Named { .. }))
    {
        for arg in &mut derived.args {
            normalize_expr_named_calls(arg.expr_mut())?;
        }
        return Ok(());
    }

    let signature =
        signatures
            .get(&derived.predicate)
            .ok_or_else(|| StaticError::UnknownPredicate {
                predicate: derived.predicate.clone(),
                arity: derived.args.len(),
                location: derived.location.clone(),
            })?;
    let normalized = normalize_call_args(&derived.predicate, &derived.args, signature)?;
    derived.args = normalized;
    for arg in &mut derived.args {
        normalize_expr_named_calls(arg.expr_mut())?;
    }
    Ok(())
}

fn normalize_call_args(
    predicate: &PredicateRef,
    args: &[CallArg],
    signature: &PredicateSignature,
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
                    return Err(StaticError::ArityMismatch {
                        predicate: predicate.clone(),
                        expected: signature.arity,
                        actual: args.len(),
                        location: named_call_location(args),
                    });
                }
                values[position] = Some(expr.clone());
            }
            CallArg::Named {
                name,
                expr,
                location,
            } => {
                seen_named = true;
                let Some(index) = parameters.iter().position(|parameter| parameter == name) else {
                    return Err(StaticError::UnknownNamedArgument {
                        predicate: predicate.clone(),
                        argument: name.clone(),
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
                values[index] = Some(expr.clone());
            }
        }
    }

    if values.iter().any(Option::is_none) {
        return Err(StaticError::ArityMismatch {
            predicate: predicate.clone(),
            expected: signature.arity,
            actual: args.len(),
            location: named_call_location(args),
        });
    }

    Ok(values
        .into_iter()
        .map(|expr| CallArg::Positional {
            expr: expr.expect("all arguments filled"),
            location: named_call_location(args),
        })
        .collect())
}

fn named_call_location(args: &[CallArg]) -> SourceLocation {
    args.iter()
        .find_map(|arg| match arg {
            CallArg::Named { location, .. } => Some(location.clone()),
            CallArg::Positional { .. } => None,
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

    let location = rule.origin.location.clone();
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

    if is_reserved_diagnostic_id(id) && rule.origin.layer != RuleLayer::Prelude {
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
                    false,
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
                        true,
                        &negation.location,
                        signatures,
                        edges,
                    )?;
                }
            }
            Atom::Aggregation(aggregate) => {
                check_body(head, &aggregate.body, signatures, edges)?;
            }
            Atom::TimeBlock(time_block) => {
                check_body(head, &time_block.body, signatures, edges)?;
            }
        }
    }
    Ok(())
}

fn check_derived_dependency(
    head: &PredicateRef,
    derived: &DerivedAtom,
    negative: bool,
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
        negative,
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
        Some(signature) => Err(StaticError::ArityMismatch {
            predicate: predicate.clone(),
            expected: signature.arity,
            actual: arity,
            location: location.clone(),
        }),
        None => Err(StaticError::UnknownPredicate {
            predicate: predicate.clone(),
            arity,
            location: location.clone(),
        }),
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
        atom.args
            .get(*idx)
            .is_some_and(|arg| expr_is_bound_by(arg.expr(), outside_bound))
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
        if !expr_is_bound_by(arg.expr(), outside_bound) {
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

fn check_cyclic_negation(edges: &[DependencyEdge]) -> Result<(), StaticError> {
    for edge in edges.iter().filter(|edge| edge.negative) {
        if let Some(mut path) = find_path(edges, &edge.to, &edge.from) {
            path.insert(0, edge.from.clone());
            path.insert(1, edge.to.clone());
            path.dedup();
            return Err(StaticError::CyclicNegation {
                cycle: NegationCycle::new(path),
                location: edge.location.clone(),
            });
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
            let required = levels.get(&edge.to).copied().unwrap_or(0) + usize::from(edge.negative);
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
                arg.expr().variables(&mut vars);
            }
        }
    }
    vars
}

fn collect_positive_atom_vars(atom: &Atom, out: &mut BTreeSet<Ident>) {
    match atom {
        Atom::Stored(stored) => stored_binding_vars(stored, out),
        Atom::Derived(derived) => {
            derived_binding_vars(derived, out);
        }
        Atom::Comparison(_) | Atom::Negation(_) => {}
        Atom::Aggregation(aggregate) => {
            aggregate.result.binding_variables(out);
        }
        Atom::TimeBlock(time_block) => collect_positive_body_vars(&time_block.body, out),
    }
}

fn collect_positive_body_vars(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        collect_positive_atom_vars(atom, out);
    }
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

fn stored_binding_vars(stored: &StoredAtom, out: &mut BTreeSet<Ident>) {
    for field in &stored.fields {
        if let Term::Expr(expr) = &field.term {
            expr.binding_variables(out);
        }
    }
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

fn derived_binding_vars(derived: &DerivedAtom, out: &mut BTreeSet<Ident>) {
    for arg in &derived.args {
        arg.expr().binding_variables(out);
    }
}

fn derived_input_vars(derived: &DerivedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for arg in &derived.args {
        arg.expr().input_variables(&mut vars);
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
    use crate::runtime::parser::parse_program;

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
}
