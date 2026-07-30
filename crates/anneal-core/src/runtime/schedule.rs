//! Static rule-body scheduling shared by planner and evaluator.

use std::collections::BTreeSet;

use super::ast::{
    Aggregate, AggregateFunction, Atom, Body, ComparisonResolution, Expr, Ident, NegatedAtom,
    StoredAtom,
};
use super::primitives::PrimitivePredicate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComparisonAction {
    Filter,
    BindLeft,
    BindRight,
}

pub(crate) struct ExecutionSchedule {
    pub(crate) atom_indexes: Vec<usize>,
    pub(crate) comparison_actions: Vec<Option<ComparisonAction>>,
    pub(crate) bound_before: Vec<BTreeSet<Ident>>,
}

pub(crate) fn greedy_execution_schedule(
    body: &Body,
    initial_bound: &BTreeSet<Ident>,
) -> ExecutionSchedule {
    try_execution_schedule(body, initial_bound)
        .expect("analyzed body has a complete execution schedule")
}

fn try_execution_schedule(
    body: &Body,
    initial_bound: &BTreeSet<Ident>,
) -> Option<ExecutionSchedule> {
    let mut remaining = body.atoms.iter().enumerate().collect::<Vec<_>>();
    let mut bound = initial_bound.clone();
    let mut order = Vec::with_capacity(remaining.len());
    let mut comparison_actions = vec![None; remaining.len()];
    let mut bound_before = vec![BTreeSet::new(); remaining.len()];
    while !remaining.is_empty() {
        let next_index = remaining
            .iter()
            .position(|(atom_index, atom)| atom_ready(body, *atom_index, atom, &bound))?;
        let (atom_index, atom) = remaining.remove(next_index);
        order.push(atom_index);
        bound_before[atom_index].clone_from(&bound);
        match atom {
            Atom::Aggregation(aggregate) => aggregate.result.binding_variables(&mut bound),
            Atom::Comparison(comparison) => {
                let action = comparison_action(comparison, &bound)
                    .expect("analyzed comparison has a planned execution action");
                comparison_actions[atom_index] = Some(action);
                match action {
                    ComparisonAction::Filter => {}
                    ComparisonAction::BindLeft => {
                        bind_bare_variable(&comparison.left, &mut bound);
                    }
                    ComparisonAction::BindRight => {
                        bind_bare_variable(&comparison.right, &mut bound);
                    }
                }
            }
            _ => collect_non_aggregate_positive_atom_variables(atom, &mut bound),
        }
    }
    Some(ExecutionSchedule {
        atom_indexes: order,
        comparison_actions,
        bound_before,
    })
}

pub(crate) fn atom_ready(
    body: &Body,
    atom_index: usize,
    atom: &Atom,
    bound: &BTreeSet<Ident>,
) -> bool {
    match atom {
        Atom::Stored(stored) => variables_are_bound(&stored_atom_input_variables(stored), bound),
        Atom::TimeBlock(time_block) => try_execution_schedule(&time_block.body, bound).is_some(),
        Atom::Derived(derived) => derived_atom_ready(derived, bound),
        Atom::Comparison(comparison) => comparison_action(comparison, bound).is_some(),
        Atom::Aggregation(aggregate) => aggregate_atom_ready(body, atom_index, aggregate, bound),
        Atom::Negation(negation) => negated_atom_variables_are_bound(&negation.atom, bound),
    }
}

pub(crate) fn comparison_action(
    comparison: &super::ast::Comparison,
    bound: &BTreeSet<Ident>,
) -> Option<ComparisonAction> {
    match comparison.resolve(bound)? {
        ComparisonResolution::Filter => Some(ComparisonAction::Filter),
        ComparisonResolution::Bind { target, .. } => match &comparison.left {
            Expr::Var(left) if left == target => Some(ComparisonAction::BindLeft),
            _ => Some(ComparisonAction::BindRight),
        },
    }
}

pub(crate) fn collect_positive_atom_binding_variables<'a>(
    atoms: &(impl Clone + Iterator<Item = &'a Atom>),
    out: &mut BTreeSet<Ident>,
) {
    for atom in atoms.clone() {
        match atom {
            Atom::TimeBlock(time_block) => {
                time_block.body.collect_positive_binding_variables(out);
            }
            Atom::Stored(_) | Atom::Derived(_) => atom.collect_positive_binding_variables(out),
            Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
        }
    }

    loop {
        let before = out.len();
        for atom in atoms.clone() {
            let Atom::Comparison(comparison) = atom else {
                continue;
            };
            if let Some(ComparisonResolution::Bind { target, .. }) = comparison.resolve(out) {
                out.insert(target.clone());
            }
        }
        if out.len() == before {
            break;
        }
    }
}

fn bind_bare_variable(expr: &Expr, bound: &mut BTreeSet<Ident>) {
    let Expr::Var(variable) = expr else {
        unreachable!("binding comparisons target a bare variable");
    };
    bound.insert(variable.clone());
}

fn derived_atom_ready(atom: &super::ast::DerivedAtom, bound: &BTreeSet<Ident>) -> bool {
    if !variables_are_bound(&derived_atom_input_variables(atom), bound) {
        return false;
    }
    let Some(primitive) = PrimitivePredicate::from_predicate(&atom.predicate) else {
        return true;
    };
    let graph_ready = primitive.graph_anchor_positions().is_none_or(|positions| {
        positions.iter().any(|idx| {
            atom.args.get(*idx).is_some_and(|arg| {
                arg.expr()
                    .is_some_and(|expr| expr_variables_are_bound(expr, bound))
            })
        })
    });
    graph_ready && content_primitive_inputs_ready(atom, primitive, bound)
}

fn content_primitive_inputs_ready(
    atom: &super::ast::DerivedAtom,
    primitive: PrimitivePredicate,
    bound: &BTreeSet<Ident>,
) -> bool {
    primitive.required_bound_inputs().iter().all(|input| {
        atom.args.get(input.position).is_some_and(|arg| {
            arg.expr()
                .is_some_and(|expr| expr_variables_are_bound(expr, bound))
        })
    })
}

fn aggregate_atom_ready(
    body: &Body,
    atom_index: usize,
    aggregate: &Aggregate,
    bound: &BTreeSet<Ident>,
) -> bool {
    let mut outside = BTreeSet::new();
    let atoms = body
        .atoms
        .iter()
        .enumerate()
        .filter(|(other_index, _)| *other_index != atom_index)
        .map(|(_, atom)| atom);
    collect_positive_atom_binding_variables(&atoms, &mut outside);

    let mut inner = outside.clone();
    aggregate
        .body
        .collect_positive_binding_variables(&mut inner);

    let mut required = inner
        .intersection(&outside)
        .cloned()
        .collect::<BTreeSet<_>>();
    required.extend(
        positive_body_input_variables(&aggregate.body)
            .into_iter()
            .filter(|var| !inner.contains(var)),
    );
    collect_aggregate_outer_input_variables(aggregate, &inner, &mut required);
    required.iter().all(|var| bound.contains(var))
}

fn collect_aggregate_outer_input_variables(
    aggregate: &Aggregate,
    inner_bound: &BTreeSet<Ident>,
    out: &mut BTreeSet<Ident>,
) {
    let rank_var = rank_arg_variable(aggregate);
    let mut value_vars = BTreeSet::new();
    aggregate.value.variables(&mut value_vars);
    if let Some(rank_var) = &rank_var {
        value_vars.remove(rank_var);
    }
    out.extend(
        value_vars
            .into_iter()
            .filter(|var| !inner_bound.contains(var)),
    );

    for arg in &aggregate.args {
        if aggregate.function == AggregateFunction::Rank && arg.name.as_str() == "rank" {
            continue;
        }
        let mut arg_vars = BTreeSet::new();
        arg.expr.variables(&mut arg_vars);
        if !matches!(
            (aggregate.function, arg.name.as_str()),
            (AggregateFunction::TopK, "k") | (AggregateFunction::TakeUntil, "budget")
        ) {
            arg_vars.retain(|var| !inner_bound.contains(var));
        }
        out.extend(arg_vars);
    }
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

fn negated_atom_variables_are_bound(atom: &NegatedAtom, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    collect_negated_atom_variables(atom, &mut vars);
    vars.iter().all(|var| bound.contains(var))
}

fn expr_variables_are_bound(expr: &Expr, bound: &BTreeSet<Ident>) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    variables_are_bound(&vars, bound)
}

fn variables_are_bound(vars: &BTreeSet<Ident>, bound: &BTreeSet<Ident>) -> bool {
    vars.iter().all(|var| bound.contains(var))
}

fn collect_non_aggregate_positive_atom_variables(atom: &Atom, out: &mut BTreeSet<Ident>) {
    match atom {
        Atom::Stored(stored) => collect_stored_atom_binding_variables(stored, out),
        Atom::Derived(derived) => collect_derived_atom_binding_variables(derived, out),
        Atom::TimeBlock(time_block) => time_block.body.collect_positive_binding_variables(out),
        Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
    }
}

fn collect_negated_atom_variables(atom: &NegatedAtom, out: &mut BTreeSet<Ident>) {
    match atom {
        NegatedAtom::Stored(stored) => collect_stored_atom_variables(stored, out),
        NegatedAtom::Derived(derived) => collect_derived_atom_variables(derived, out),
    }
}

fn collect_stored_atom_variables(atom: &StoredAtom, out: &mut BTreeSet<Ident>) {
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.variables(out);
        }
    }
}

fn collect_derived_atom_variables(atom: &super::ast::DerivedAtom, out: &mut BTreeSet<Ident>) {
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.variables(out);
        }
    }
}

fn collect_stored_atom_binding_variables(atom: &StoredAtom, out: &mut BTreeSet<Ident>) {
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.binding_variables(out);
        }
    }
}

fn collect_derived_atom_binding_variables(
    atom: &super::ast::DerivedAtom,
    out: &mut BTreeSet<Ident>,
) {
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.binding_variables(out);
        }
    }
}

fn stored_atom_input_variables(atom: &StoredAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.input_variables(&mut vars);
            expr.binding_variables(&mut bindings);
        }
    }
    vars.retain(|var| !bindings.contains(var));
    vars
}

fn derived_atom_input_variables(atom: &super::ast::DerivedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.input_variables(&mut vars);
            expr.binding_variables(&mut bindings);
        }
    }
    vars.retain(|var| !bindings.contains(var));
    vars
}

fn positive_body_input_variables(body: &Body) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => vars.extend(stored_atom_input_variables(stored)),
            Atom::Derived(derived) => vars.extend(derived_atom_input_variables(derived)),
            Atom::TimeBlock(time_block) => {
                vars.extend(positive_body_input_variables(&time_block.body));
            }
            Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::parser::parse_program;

    #[test]
    fn computed_aggregate_inputs_are_bound_inside_the_aggregate() {
        let program = parse_program(
            "schedule-test",
            r"? (h, weighted) = TopK{ k: 1, key: weighted :
                (h, weighted) : score(h, raw), weighted = raw * 2
              }.",
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");
        let Atom::Aggregation(aggregate) = &query.body.atoms[0] else {
            panic!("expected aggregate");
        };

        assert!(aggregate_atom_ready(
            &query.body,
            0,
            aggregate,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn sibling_aggregate_results_do_not_become_grouping_inputs() {
        let program = parse_program(
            "schedule-test",
            r"
            use(key, sample) := pair(key, sample).
            weight(key, unit, weight) := weighted(key, unit, weight).
            ? use(key, sample),
              count = Count{ h : use(key, h) },
              signal(key),
              rank = Sum{ weight : weight(key, unit, weight) }.
            ",
        )
        .expect("program parses");
        let query = program.queries().next().expect("query");

        assert!(try_execution_schedule(&query.body, &BTreeSet::new()).is_some());
    }

    #[test]
    fn time_block_equations_do_not_become_outer_inputs() {
        let program = parse_program(
            "schedule-test",
            r#"
            ? at("snapshot:last") { n = 1 + 1, pipeline_position_for("draft", n) }.
            ? at("snapshot:last") { n = outer + 1 }.
            "#,
        )
        .expect("program parses");
        let queries = program.queries().collect::<Vec<_>>();
        let Atom::TimeBlock(constant) = &queries[0].body.atoms[0] else {
            panic!("expected time block");
        };
        let Atom::TimeBlock(correlated) = &queries[1].body.atoms[0] else {
            panic!("expected time block");
        };

        assert!(try_execution_schedule(&constant.body, &BTreeSet::new()).is_some());
        assert!(try_execution_schedule(&correlated.body, &BTreeSet::new()).is_none());
        assert!(
            try_execution_schedule(
                &correlated.body,
                &BTreeSet::from([Ident::new_unchecked("outer")])
            )
            .is_some()
        );
    }
}
