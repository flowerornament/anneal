use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::runtime::ast::{
    Aggregate, Atom, Body, Comparison, Head, Ident, NegatedAtom, PredicateRef, Program, Query,
    Rule, StoredAtom, Term,
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
    #[error("unknown predicate '{predicate}/{arity}'")]
    UnknownPredicate {
        predicate: PredicateRef,
        arity: usize,
    },
    #[error("predicate '{predicate}' used with arity {actual}, expected {expected}")]
    ArityMismatch {
        predicate: PredicateRef,
        expected: usize,
        actual: usize,
    },
    #[error("unsafe rule '{predicate}': head variable '{variable}' is not bound positively")]
    UnboundHeadVariable {
        predicate: PredicateRef,
        variable: Ident,
    },
    #[error("unsafe expression in '{predicate}': variable '{variable}' is not bound positively")]
    UnboundExpressionVariable {
        predicate: PredicateRef,
        variable: Ident,
    },
    #[error("unsafe negation in '{predicate}': variable '{variable}' is not bound positively")]
    UnboundNegationVariable {
        predicate: PredicateRef,
        variable: Ident,
    },
    #[error("cyclic negation between {cycle}")]
    CyclicNegation { cycle: NegationCycle },
    #[error("reserved diagnostic id '{id}' may only be emitted by built-in check rules")]
    ReservedDiagnosticId { id: String },
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
}

impl Analyzer {
    fn new(program: Program) -> Self {
        Self {
            program,
            signatures: BTreeMap::new(),
            edges: Vec::new(),
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
            check_rule(rule, &self.signatures, &mut self.edges)?;
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
        for rule in &query.local_rules {
            check_rule(rule, &signatures, &mut edges)?;
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
}

fn collect_signature(signatures: &mut SignatureMap, head: &Head) -> Result<(), StaticError> {
    let arity = head.arity();
    match signatures.get(&head.predicate) {
        Some(expected) if *expected != arity => Err(StaticError::ArityMismatch {
            predicate: head.predicate.clone(),
            expected: *expected,
            actual: arity,
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
) -> Result<(), StaticError> {
    check_head_safety(rule)?;
    check_body(&rule.head.predicate, &rule.body, signatures, edges)?;
    check_body_safety(&rule.head.predicate, &rule.body)
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
            Atom::Stored(_) | Atom::Comparison(_) | Atom::Negation(NegatedAtom::Stored(_)) => {}
            Atom::Derived(derived) => {
                check_derived_call(signatures, &derived.predicate, derived.args.len())?;
                edges.push(DependencyEdge {
                    from: head.clone(),
                    to: derived.predicate.clone(),
                    negative: false,
                });
            }
            Atom::Negation(NegatedAtom::Derived(derived)) => {
                check_derived_call(signatures, &derived.predicate, derived.args.len())?;
                edges.push(DependencyEdge {
                    from: head.clone(),
                    to: derived.predicate.clone(),
                    negative: true,
                });
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

fn check_derived_call(
    signatures: &SignatureMap,
    predicate: &PredicateRef,
    arity: usize,
) -> Result<(), StaticError> {
    match signatures.get(predicate) {
        Some(expected) if *expected == arity => Ok(()),
        Some(expected) => Err(StaticError::ArityMismatch {
            predicate: predicate.clone(),
            expected: *expected,
            actual: arity,
        }),
        None => Err(StaticError::UnknownPredicate {
            predicate: predicate.clone(),
            arity,
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
                ensure_bound(predicate, vars, &bound, SafetyContext::Expression)?;
            }
            Atom::Negation(negated) => {
                ensure_bound(
                    predicate,
                    negated_vars(negated),
                    &bound,
                    SafetyContext::Negation,
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
    ensure_bound(predicate, vars, outer_bound, SafetyContext::Expression)?;
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
) -> Result<(), StaticError> {
    for variable in vars {
        if !bound.contains(&variable) {
            return match context {
                SafetyContext::Expression => Err(StaticError::UnboundExpressionVariable {
                    predicate: predicate.clone(),
                    variable,
                }),
                SafetyContext::Negation => Err(StaticError::UnboundNegationVariable {
                    predicate: predicate.clone(),
                    variable,
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

    #[test]
    fn rejects_unbound_head_variable() {
        let program = parse_program("inline", r"bad(h) := *handle{id: other}.").unwrap();
        let err = analyze(program).expect_err("unsafe rule rejected");
        assert!(matches!(err, StaticError::UnboundHeadVariable { .. }));
    }

    #[test]
    fn rejects_unbound_negation_variable() {
        let program = parse_program(
            "inline",
            r#"
            terminal(h) := *handle{id: h, status: "resolved"}.
            bad(h) := *handle{id: h}, not terminal(missing).
            "#,
        )
        .unwrap();
        let err = analyze(program).expect_err("unsafe negation rejected");
        assert!(matches!(err, StaticError::UnboundNegationVariable { .. }));
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
        let program = parse_program("inline", r"bad(h) := *handle{id: h}, h = missing.").unwrap();
        let err = analyze(program).expect_err("unsafe comparison rejected");
        assert!(matches!(err, StaticError::UnboundExpressionVariable { .. }));
    }

    #[test]
    fn rejects_unknown_predicates() {
        let program = parse_program("inline", r"bad(h) := missing(h).").unwrap();
        let err = analyze(program).expect_err("unknown predicate rejected");
        assert!(matches!(err, StaticError::UnknownPredicate { .. }));
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
        let program = parse_program(
            "anneal.dl",
            r"
            blocked(h) := *handle{id: h}, not advancing(h).
            advancing(h) := *handle{id: h}, not blocked(h).
            ",
        )
        .unwrap();
        let err = analyze(program).expect_err("cyclic negation rejected");
        let StaticError::CyclicNegation { cycle } = err else {
            panic!("expected cyclic negation");
        };
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
