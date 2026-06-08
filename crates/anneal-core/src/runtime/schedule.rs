//! Static rule-body scheduling shared by planner and evaluator.

use std::collections::BTreeSet;

use super::ast::{Aggregate, AggregateFunction, Atom, Body, Expr, Ident, NegatedAtom, StoredAtom};
use super::primitives::PrimitivePredicate;

pub(crate) fn greedy_execution_order(body: &Body) -> Vec<usize> {
    let mut remaining = body.atoms.iter().enumerate().collect::<Vec<_>>();
    let mut bound = BTreeSet::new();
    let mut order = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let next_index = remaining
            .iter()
            .position(|(atom_index, atom)| atom_ready(body, *atom_index, atom, &bound))
            .unwrap_or(0);
        let (atom_index, atom) = remaining.remove(next_index);
        order.push(atom_index);
        match atom {
            Atom::Aggregation(aggregate) => aggregate.result.binding_variables(&mut bound),
            _ => collect_non_aggregate_positive_atom_variables(atom, &mut bound),
        }
    }
    order
}

pub(crate) fn atom_ready(
    body: &Body,
    atom_index: usize,
    atom: &Atom,
    bound: &BTreeSet<Ident>,
) -> bool {
    match atom {
        Atom::Stored(stored) => variables_are_bound(&stored_atom_input_variables(stored), bound),
        Atom::TimeBlock(time_block) => variables_are_bound(
            &positive_body_outer_input_variables(&time_block.body),
            bound,
        ),
        Atom::Derived(derived) => derived_atom_ready(derived, bound),
        Atom::Comparison(comparison) => {
            expr_variables_are_bound(&comparison.left, bound)
                && expr_variables_are_bound(&comparison.right, bound)
        }
        Atom::Aggregation(aggregate) => aggregate_atom_ready(body, atom_index, aggregate, bound),
        Atom::Negation(negation) => negated_atom_variables_are_bound(&negation.atom, bound),
    }
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
    for (other_index, atom) in body.atoms.iter().enumerate() {
        if other_index != atom_index {
            collect_non_aggregate_positive_atom_variables(atom, &mut outside);
        }
    }

    let mut inner = BTreeSet::new();
    collect_positive_body_variables(&aggregate.body, &mut inner);

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
        if matches!(
            (aggregate.function, arg.name.as_str()),
            (AggregateFunction::TopK, "k") | (AggregateFunction::TakeUntil, "budget")
        ) {
            arg.expr.variables(&mut arg_vars);
        } else {
            arg.expr.variables(&mut arg_vars);
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
        Atom::TimeBlock(time_block) => collect_positive_body_variables(&time_block.body, out),
        Atom::Comparison(_) | Atom::Aggregation(_) | Atom::Negation(_) => {}
    }
}

fn collect_positive_body_variables(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        match atom {
            Atom::Aggregation(aggregate) => aggregate.result.binding_variables(out),
            _ => collect_non_aggregate_positive_atom_variables(atom, out),
        }
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
    for field in &atom.fields {
        if let Some(expr) = field.term.expr() {
            expr.input_variables(&mut vars);
        }
    }
    vars
}

fn derived_atom_input_variables(atom: &super::ast::DerivedAtom) -> BTreeSet<Ident> {
    let mut vars = BTreeSet::new();
    for arg in &atom.args {
        if let Some(expr) = arg.expr() {
            expr.input_variables(&mut vars);
        }
    }
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

fn positive_body_outer_input_variables(body: &Body) -> BTreeSet<Ident> {
    let mut inputs = positive_body_input_variables(body);
    let mut binders = BTreeSet::new();
    collect_positive_body_variables(body, &mut binders);
    inputs.retain(|var| !binders.contains(var));
    inputs
}
