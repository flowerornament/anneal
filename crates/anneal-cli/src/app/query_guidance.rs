//! Query evidence demand, warning, and zero-result authority.

use std::collections::BTreeSet;

use anneal_core::runtime::DerivedAtom;
use anneal_core::runtime::QueryWarning;
use anneal_core::runtime::datalog_string_literal;
use anneal_core::runtime::{
    AnalyzedProgram, Atom, Body, CallArg, CallStyle, Expr, Literal, NegatedAtom, NumberLiteral,
    Query, Row, Statement, StoredAtom, parse_program, stored_relation_fields,
};
use chrono::NaiveDate;

use super::command::RuntimeCommand;
use super::output::eval_zero_result_hint;

#[cfg(test)]
mod tests;

/// Derive zero-result guidance from the parsed query and rendered rows.
pub(super) fn zero_result_hint_for_query(query_source: &str, rows: &[Row]) -> Option<String> {
    if !rows.is_empty() {
        return None;
    }
    let query = parse_query_fragment(query_source)?;
    Some(eval_zero_result_hint(&query))
}

fn parse_query_fragment(query_source: &str) -> Option<Query> {
    parse_program("cli-query-hint", query_source)
        .ok()?
        .statements
        .into_iter()
        .find_map(|statement| match statement {
            Statement::Query(query) => Some(query),
            _ => None,
        })
}

/// Return the projected handle field when the query targets `ranked_anchor`.
pub(super) fn ranked_anchor_handle_field(query_source: &str) -> Option<String> {
    let program = parse_program("ranked-anchor-detect", query_source).ok()?;
    let query = program
        .statements
        .iter()
        .find_map(|statement| match statement {
            Statement::Query(query) => Some(query),
            _ => None,
        })?;
    let mut handle_fields = query
        .body
        .atoms
        .iter()
        .filter_map(ranked_anchor_atom_handle_field)
        .collect::<BTreeSet<_>>();
    if handle_fields.len() != 1 {
        return None;
    }
    let handle_field = handle_fields.pop_first()?;
    if handle_field == "signals" {
        return None;
    }
    Some(handle_field)
}

fn ranked_anchor_atom_handle_field(atom: &Atom) -> Option<String> {
    let Atom::Derived(derived) = atom else {
        return None;
    };
    if derived.predicate.module.is_some() || derived.predicate.name.as_str() != "ranked_anchor" {
        return None;
    }
    let first_arg = derived.args.first()?;
    let expr = match first_arg {
        CallArg::Positional { expr, .. } | CallArg::Named { expr, .. } => expr,
        CallArg::Wildcard { .. } => return None,
    };
    let Expr::Var(variable) = expr else {
        return None;
    };
    Some(variable.as_str().to_string())
}

impl RuntimeCommand {
    /// Return whether execution needs code-target history facts.
    pub(super) fn demands_code_target_history(&self) -> bool {
        match self {
            Self::Status | Self::Verb { .. } | Self::Check { .. } | Self::Handle { .. } => true,
            Self::Eval { query, .. } => query_demands_code_target_history(query),
            Self::Describe { name } => matches!(
                name.as_str(),
                "W006"
                    | "spec_code_drift"
                    | "target_exists"
                    | "target_history_status"
                    | "target_probe_base"
                    | "target_resolved_path"
            ),
            Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Schema
            | Self::Help { .. }
            | Self::HelpName { .. } => false,
        }
    }

    /// Return whether execution needs design-code drift evidence.
    pub(super) fn demands_code_drift_evidence(&self) -> bool {
        match self {
            Self::Status | Self::Check { .. } | Self::Handle { .. } => true,
            Self::Eval { query, .. } => query_demands_code_drift_evidence(query),
            Self::Describe { name } => matches!(
                name.as_str(),
                "referent_disposition"
                    | "assertion_drift"
                    | "referent_moved_head"
                    | "drift_profile"
            ),
            Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Schema
            | Self::Verb { .. }
            | Self::Help { .. }
            | Self::HelpName { .. } => false,
        }
    }

    /// Return whether this command explicitly refreshes drift evidence.
    pub(super) const fn refreshes_code_drift_evidence(&self) -> bool {
        matches!(
            self,
            Self::Check {
                refresh_drift: true
            }
        )
    }

    /// Return whether execution needs edge-assertion provenance.
    pub(super) fn demands_edge_assertions(&self) -> bool {
        match self {
            Self::Eval { query, .. } => query_demands_edge_assertions(query),
            Self::Describe { name } => {
                matches!(
                    name.as_str(),
                    "edge" | "*edge" | "assertion_date" | "assertion_revision"
                )
            }
            Self::Status
            | Self::Verb { .. }
            | Self::Check { .. }
            | Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Handle { .. }
            | Self::Schema
            | Self::Help { .. }
            | Self::HelpName { .. } => false,
        }
    }
}

/// Detect code-target-history predicates without accepting identifier substrings.
pub(super) fn query_demands_code_target_history(query: &str) -> bool {
    [
        "diagnostic",
        "spec_code_drift",
        "target_exists",
        "target_history_status",
        "target_probe_base",
        "target_resolved_path",
        "entropy",
        "primary_entropy",
        "potential",
        "potential_subject",
        "frontier",
        "ranked_work",
        "area_frontier",
        "blocked",
        "blocker",
        "holding",
        "flow",
        "status_item",
        "status_metric",
    ]
    .iter()
    .any(|needle| query_contains_identifier(query, needle))
}

/// Detect drift predicates without accepting identifier substrings.
pub(super) fn query_demands_code_drift_evidence(query: &str) -> bool {
    [
        "referent_disposition",
        "assertion_drift",
        "referent_moved_head",
        "drift_profile",
        "code_ref",
        "code.referent_disposition",
        "code.referent_commits_since",
        "code.referent_moved_to",
        "code.referent_move_candidate",
    ]
    .iter()
    .any(|needle| query_contains_identifier(query, needle))
}

/// Detect edge-assertion predicates without accepting identifier substrings.
pub(super) fn query_demands_edge_assertions(query: &str) -> bool {
    ["assertion_date", "assertion_revision"]
        .iter()
        .any(|needle| query_contains_identifier(query, needle))
}

fn query_contains_identifier(query: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = query[start..].find(needle) {
        let match_start = start + offset;
        let match_end = match_start + needle.len();
        let before = query[..match_start].chars().next_back();
        let after = query[match_end..].chars().next();
        if before.is_none_or(|ch| !is_ident_char(ch)) && after.is_none_or(|ch| !is_ident_char(ch)) {
            return true;
        }
        start = match_end;
    }
    false
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Teach a projection when a query matched rows but bound no output fields.
pub(super) fn empty_binding_example(analyzed: &AnalyzedProgram, body: &Body) -> Option<String> {
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => {
                let example = empty_binding_example_for_stored(stored)?;
                return Some(example);
            }
            Atom::Derived(derived) => {
                if !is_introspection_predicate(derived.predicate.name.as_str()) {
                    let example = empty_binding_example_for_derived(analyzed, derived)?;
                    return Some(example);
                }
            }
            Atom::TimeBlock(time_block) => {
                if let Some(example) = empty_binding_example(analyzed, &time_block.body) {
                    return Some(example);
                }
            }
            Atom::Aggregation(aggregate) => {
                if let Some(example) = empty_binding_example(analyzed, &aggregate.body) {
                    return Some(example);
                }
            }
            Atom::Comparison(_) | Atom::Negation(_) => {}
        }
    }
    None
}

/// Render structured query warnings for stderr.
pub(super) fn warning_texts(warnings: &[QueryWarning]) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| format!("warning: {}", warning.message))
        .collect()
}

/// Compute authored age from an exact ISO date.
pub(super) fn authored_age_days(date: &str, today: NaiveDate) -> Option<i64> {
    let authored = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(today.signed_duration_since(authored).num_days().max(0))
}

/// Classify one currency hit from its age and supersession evidence.
pub(super) fn currency_disposition(
    handle: &str,
    superseded: &BTreeSet<&str>,
    successors: &BTreeSet<&str>,
) -> &'static str {
    if superseded.contains(handle) {
        "superseded"
    } else if successors.contains(handle) {
        "current_head"
    } else {
        "current"
    }
}

/// Return whether a warning applies to the selected query vocabulary.
pub(super) fn warning_applies_to_query(query_source: &str, warning: &QueryWarning) -> bool {
    warning.reference.as_deref().is_none_or(|reference| {
        query_source.contains(reference)
            || query_source.contains(&format!("at({})", datalog_string_literal(reference)))
    })
}

/// Warn when a query filters the retired section-handle kind.
pub(super) fn retired_section_kind_warning(body: &Body) -> Option<QueryWarning> {
    body_filters_retired_section_kind(body).then(|| QueryWarning {
        code: "retired_section_kind".to_string(),
        message: "the section handle kind was retired in v0.14; use `*span{id: span_id, handle: file, summary: heading}` for heading spans".to_string(),
        reference: None,
        source: None,
        relation: Some("handle".to_string()),
    })
}

fn body_filters_retired_section_kind(body: &Body) -> bool {
    body.atoms.iter().any(atom_filters_retired_section_kind)
}

fn atom_filters_retired_section_kind(atom: &Atom) -> bool {
    match atom {
        Atom::Stored(stored) => stored_filters_retired_section_kind(stored),
        Atom::Aggregation(aggregate) => body_filters_retired_section_kind(&aggregate.body),
        Atom::Negation(negation) => negated_atom_filters_retired_section_kind(&negation.atom),
        Atom::TimeBlock(time_block) => body_filters_retired_section_kind(&time_block.body),
        Atom::Derived(_) | Atom::Comparison(_) => false,
    }
}

fn negated_atom_filters_retired_section_kind(atom: &NegatedAtom) -> bool {
    match atom {
        NegatedAtom::Stored(stored) => stored_filters_retired_section_kind(stored),
        NegatedAtom::Derived(_) => false,
    }
}

fn stored_filters_retired_section_kind(stored: &StoredAtom) -> bool {
    if stored.relation.as_str() != "handle" {
        return false;
    }
    if stored_literal_field(stored, "source").is_some_and(|source| source != "markdown") {
        return false;
    }
    stored_literal_field(stored, "kind").is_some_and(|kind| kind == "section")
}

fn stored_literal_field<'a>(stored: &'a StoredAtom, name: &str) -> Option<&'a str> {
    stored.fields.iter().find_map(|field| {
        (field.field.as_str() == name).then(|| match field.term.expr() {
            Some(Expr::Literal(Literal::String(value))) => Some(value.as_str()),
            _ => None,
        })?
    })
}

fn empty_binding_example_for_stored(stored: &StoredAtom) -> Option<String> {
    let fields = stored_relation_fields(stored.relation.as_str())?;
    let existing_fields = stored
        .fields
        .iter()
        .map(|field| field.field.as_str())
        .collect::<BTreeSet<_>>();
    let field = fields
        .as_slice()
        .iter()
        .copied()
        .find(|field| !existing_fields.contains(field))?;
    let mut parts = render_literal_field_patterns(&stored.fields);
    parts.push(format!("{field}: {}", variable_for_field(field)));
    Some(format!("? *{}{{{}}}.", stored.relation, parts.join(", ")))
}

fn empty_binding_example_for_derived(
    analyzed: &AnalyzedProgram,
    derived: &DerivedAtom,
) -> Option<String> {
    let fields = analyzed.predicate_parameter_names(&derived.predicate)?;
    if matches!(derived.style, CallStyle::Pattern)
        || derived
            .args
            .iter()
            .any(|arg| matches!(arg, CallArg::Named { .. } | CallArg::Wildcard { .. }))
    {
        return empty_binding_example_for_pattern_derived(derived, &fields);
    }
    empty_binding_example_for_positional_derived(derived, &fields)
}

fn empty_binding_example_for_pattern_derived(
    derived: &DerivedAtom,
    fields: &[String],
) -> Option<String> {
    let suggested_index = derived
        .args
        .iter()
        .position(|arg| matches!(arg, CallArg::Wildcard { .. }))
        .unwrap_or(0);
    let field = fields.get(suggested_index).map(String::as_str)?;
    let mut parts = derived
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            if index == suggested_index {
                return None;
            }
            let field = fields.get(index)?;
            match arg {
                CallArg::Named { expr, .. } | CallArg::Positional { expr, .. } => {
                    render_literal_expr(expr).map(|value| format!("{field}: {value}"))
                }
                CallArg::Wildcard { .. } => None,
            }
        })
        .collect::<Vec<_>>();
    parts.push(format!("{field}: {}", variable_for_field(field)));
    Some(format!(
        "? {}{{{}}}.",
        derived.predicate.name,
        parts.join(", ")
    ))
}

fn empty_binding_example_for_positional_derived(
    derived: &DerivedAtom,
    fields: &[String],
) -> Option<String> {
    let arity = derived.args.len();
    if arity == 0 {
        return None;
    }
    let suggested_index = derived
        .args
        .iter()
        .position(|arg| !matches!(arg, CallArg::Wildcard { .. }))
        .unwrap_or(0);
    let args = (0..arity)
        .map(|index| {
            if index == suggested_index {
                Some(
                    fields
                        .get(index)
                        .map_or_else(|| "value".to_string(), |field| variable_for_field(field)),
                )
            } else {
                render_call_arg(&derived.args[index])
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "? {}({}).",
        derived.predicate.name,
        args.join(", ")
    ))
}

fn render_literal_field_patterns(fields: &[anneal_core::runtime::FieldPattern]) -> Vec<String> {
    fields
        .iter()
        .filter_map(|field| {
            let expr = field.term.expr()?;
            render_literal_expr(expr).map(|value| format!("{}: {value}", field.field))
        })
        .collect()
}

fn render_call_arg(arg: &CallArg) -> Option<String> {
    match arg {
        CallArg::Positional { expr, .. } | CallArg::Named { expr, .. } => render_literal_expr(expr),
        CallArg::Wildcard { .. } => Some("_".to_string()),
    }
}

fn render_literal_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(literal) => Some(render_literal(literal)),
        Expr::Var(_) | Expr::FunctionCall { .. } | Expr::Binary { .. } | Expr::Tuple(_) => None,
    }
}

fn render_literal(literal: &Literal) -> String {
    match literal {
        Literal::String(value) => datalog_string_literal(value),
        Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
        Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_string(),
        Literal::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn variable_for_field(field: &str) -> String {
    match field {
        "id" | "h" | "handle" | "subject" => "h".to_string(),
        "from" => "src".to_string(),
        "to" => "dst".to_string(),
        "affected" => "affected".to_string(),
        "depth" => "depth".to_string(),
        "code" => "code".to_string(),
        "severity" => "severity".to_string(),
        "file" => "file".to_string(),
        "line" => "line".to_string(),
        "energy" | "score" | "weight" => field.to_string(),
        "source" => "source".to_string(),
        "area" => "area".to_string(),
        "count" => "count".to_string(),
        "status" => "status".to_string(),
        "kind" => "kind".to_string(),
        "evidence" => "evidence".to_string(),
        other => other
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect(),
    }
}

/// Return whether a query target is runtime self-description.
pub(super) fn is_introspection_predicate(name: &str) -> bool {
    matches!(
        name,
        "schema" | "predicates" | "verbs" | "describe" | "examples" | "sources"
    )
}
