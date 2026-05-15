use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::runtime::ast::{
    Aggregate, Atom, Body, Comparison, DerivedAtom, Expr, Head, Ident, Literal, NegatedAtom,
    Negation, PredicateRef, Program, Query, Rule, RuleLayer, SourceLocation, StoredAtom, Term,
};

type SignatureMap = BTreeMap<PredicateRef, usize>;

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
        self.signatures.keys()
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
            signatures: BTreeMap::new(),
            edges: Vec::new(),
            diagnostic_ids: BTreeMap::new(),
        }
    }

    fn analyze(mut self) -> Result<AnalyzedProgram, StaticError> {
        self.collect_global_signatures()?;
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
        let mut signatures = self.signatures.clone();
        let mut local_predicates = BTreeSet::new();
        for rule in &query.local_rules {
            collect_signature(&mut signatures, &rule.head)?;
            local_predicates.insert(rule.head.predicate.clone());
        }

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
            query: query.clone(),
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
    match signatures.get(&head.predicate) {
        Some(expected) if *expected != arity => Err(StaticError::ArityMismatch {
            predicate: head.predicate.clone(),
            expected: *expected,
            actual: arity,
            location: head.location.clone(),
        }),
        Some(_) => Ok(()),
        None => {
            signatures.insert(head.predicate.clone(), arity);
            Ok(())
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
        Some(expected) if *expected == arity => Ok(()),
        Some(expected) => Err(StaticError::ArityMismatch {
            predicate: predicate.clone(),
            expected: *expected,
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
    let mut bound = BTreeSet::new();
    collect_positive_body_vars(body, &mut bound);

    for atom in &body.atoms {
        match atom {
            Atom::Stored(_) | Atom::Derived(_) => {}
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
                check_aggregate_safety(predicate, aggregate, &bound)?;
            }
            Atom::TimeBlock(time_block) => {
                check_body_safety(predicate, &time_block.body)?;
            }
        }
    }
    Ok(())
}

fn check_aggregate_safety(
    predicate: &PredicateRef,
    aggregate: &Aggregate,
    outer_bound: &BTreeSet<Ident>,
) -> Result<(), StaticError> {
    let mut vars = BTreeSet::new();
    aggregate.value.variables(&mut vars);
    for arg in &aggregate.args {
        arg.expr.variables(&mut vars);
    }
    ensure_bound(
        predicate,
        vars,
        outer_bound,
        SafetyContext::Expression,
        &aggregate.location,
    )?;
    check_body_safety(predicate, &aggregate.body)
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
        .keys()
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
        Atom::Stored(stored) => out.extend(stored_vars(stored)),
        Atom::Derived(derived) => {
            for arg in &derived.args {
                arg.expr().variables(out);
            }
        }
        Atom::Comparison(_) | Atom::Negation(_) => {}
        Atom::Aggregation(aggregate) => {
            aggregate.result.variables(out);
            collect_positive_body_vars(&aggregate.body, out);
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
