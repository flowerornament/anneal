//! Honest zero-result and diagnostic-adjacency guidance.

use anneal_core::runtime::{Atom, Body, Expr, NegatedAtom, Query, Row};
use anyhow::Result;

use super::value::required_string;

/// Partition one snapshot into rendered errors and adjacent non-errors.
pub(in crate::app) fn partition_check_diagnostics(rows: Vec<Row>) -> Result<(Vec<Row>, usize)> {
    let mut error_rows = Vec::new();
    let mut non_error_count = 0;
    for mut row in rows {
        if required_string(&row, "severity")? == "error" {
            row.fields.remove("severity");
            error_rows.push(row);
        } else {
            non_error_count += 1;
        }
    }
    Ok((error_rows, non_error_count))
}

/// Teach the adjacent search move selected by confidence policy.
pub(in crate::app) fn search_zero_result_hint(include_low_confidence: bool) -> String {
    if include_low_confidence {
        "hint: search returned 0 rows including low-confidence matches; retry with broader terms."
            .to_string()
    } else {
        "hint: search returned 0 rows after excluding low-confidence matches; retry with --include-low-confidence or broader terms."
            .to_string()
    }
}

/// Explain a zero-row query without running a second evaluation.
pub(in crate::app) fn eval_zero_result_hint(query: &Query) -> String {
    if let Some(predicate) = bare_relation_name(query) {
        return format!(
            "hint: {predicate} currently has no rows; run `anneal describe {predicate}` for requirements and common joins."
        );
    }
    let recovery = first_relation_name(&query.body).map_or_else(
        || "Relax one constraint at a time.".to_string(),
        |predicate| format!("Relax one constraint at a time or run `anneal describe {predicate}`."),
    );
    format!(
        "hint: this filtered or joined query returned 0 rows; that does not establish its predicates are empty. {recovery}"
    )
}

fn bare_relation_name(query: &Query) -> Option<String> {
    if !query.local_rules.is_empty() || query.body.atoms.len() != 1 {
        return None;
    }
    match query.body.atoms.first()? {
        Atom::Stored(stored)
            if stored.fields.iter().all(|field| {
                field
                    .term
                    .expr()
                    .is_none_or(|expr| matches!(expr, Expr::Var(_)))
            }) =>
        {
            Some(stored.relation.to_string())
        }
        Atom::Derived(derived)
            if derived
                .args
                .iter()
                .all(|arg| arg.expr().is_none_or(|expr| matches!(expr, Expr::Var(_)))) =>
        {
            Some(derived.predicate.display_name())
        }
        Atom::Stored(_)
        | Atom::Derived(_)
        | Atom::Comparison(_)
        | Atom::Aggregation(_)
        | Atom::Negation(_)
        | Atom::TimeBlock(_) => None,
    }
}

fn first_relation_name(body: &Body) -> Option<String> {
    body.atoms.iter().find_map(|atom| match atom {
        Atom::Stored(stored) => Some(stored.relation.to_string()),
        Atom::Derived(derived) => Some(derived.predicate.display_name()),
        Atom::Aggregation(aggregate) => first_relation_name(&aggregate.body),
        Atom::TimeBlock(time_block) => first_relation_name(&time_block.body),
        Atom::Negation(negation) => match &negation.atom {
            NegatedAtom::Stored(stored) => Some(stored.relation.to_string()),
            NegatedAtom::Derived(derived) => Some(derived.predicate.display_name()),
        },
        Atom::Comparison(_) => None,
    })
}
