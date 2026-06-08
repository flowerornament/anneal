//! Fixpoint and stage scheduling for planned rule execution.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::plan::{PlanCatalog, RuleGroupPlan, RuleStagePlan, StageExecution, StratumPlan};
use crate::runtime::ast::PredicateRef;
use crate::runtime::eval::{Database, EvalError, EvalOptions, QueryWarning};
use crate::vm::execute::{
    DeltaMap, PlannedDeltaView, eval_planned_rule_group, eval_planned_rule_group_with_delta,
};

pub(crate) fn run_rule_group(
    database: &mut Database,
    active_predicates: &BTreeSet<PredicateRef>,
    planned_stratum: &StratumPlan,
    catalog: &PlanCatalog,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<(), EvalError> {
    database.ensure_derived(active_predicates.iter().cloned());
    run_staged_rule_group(
        database,
        active_predicates,
        planned_stratum,
        catalog,
        warnings,
        options,
    )
}

fn run_staged_rule_group(
    database: &mut Database,
    active_predicates: &BTreeSet<PredicateRef>,
    planned_stratum: &StratumPlan,
    catalog: &PlanCatalog,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<(), EvalError> {
    if active_predicates.is_empty() {
        return Err(EvalError::UnsupportedExpression);
    }
    for stage in &planned_stratum.stages {
        let Some(stage_predicate) =
            active_stage_predicate(stage, planned_stratum, active_predicates)?
        else {
            continue;
        };
        if !stage.authoritative_planned {
            return Err(EvalError::PlannedExecutorUnsupported {
                predicate: stage_predicate,
                reasons: format!("{:?}", stage.migration.reasons),
            });
        }
        if stage.execution.is_recursive() {
            run_planned_recursive_stage(
                database,
                stage,
                planned_stratum,
                catalog,
                active_predicates,
                warnings,
                options,
            )?;
            continue;
        }
        let planned_groups = planned_authoritative_for_stage(
            stage,
            planned_stratum,
            stage_predicate,
            active_predicates,
        )?;
        for (_, planned, predicate) in planned_groups {
            let tuples = eval_planned_rule_group(planned, catalog, database, warnings, options)?;
            database.insert_derived_tuples(&predicate, tuples);
        }
    }
    Ok(())
}

fn active_stage_predicate(
    stage: &RuleStagePlan,
    planned_stratum: &StratumPlan,
    active_predicates: &BTreeSet<PredicateRef>,
) -> Result<Option<PredicateRef>, EvalError> {
    for group_index in &stage.rule_groups {
        let group = planned_stratum
            .rule_groups
            .get(*group_index)
            .ok_or(EvalError::UnsupportedExpression)?;
        let Some(provenance) = &group.provenance else {
            continue;
        };
        if active_predicates.contains(&provenance.predicate) {
            return Ok(Some(provenance.predicate.clone()));
        }
    }
    Ok(None)
}

fn run_planned_recursive_stage(
    database: &mut Database,
    stage: &RuleStagePlan,
    planned_stratum: &StratumPlan,
    catalog: &PlanCatalog,
    active_predicates: &BTreeSet<PredicateRef>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<(), EvalError> {
    let active_predicate = active_stage_predicate(stage, planned_stratum, active_predicates)?
        .ok_or(EvalError::UnsupportedExpression)?;
    let planned_groups = planned_authoritative_for_stage(
        stage,
        planned_stratum,
        active_predicate,
        active_predicates,
    )?;
    let mut delta = DeltaMap::new();
    for (_, planned, predicate) in &planned_groups {
        let tuples = eval_planned_rule_group(planned, catalog, database, warnings, options)?;
        database.insert_new_derived_tuples(predicate, tuples, &mut delta);
    }
    let planned_by_index = planned_groups
        .into_iter()
        .map(|(index, group, predicate)| (index, (group, predicate)))
        .collect::<BTreeMap<_, _>>();
    while !delta.is_empty() {
        let previous_delta = delta;
        delta = DeltaMap::new();
        let StageExecution::Recursive { deltas } = &stage.execution else {
            return Err(EvalError::UnsupportedExpression);
        };
        for delta_plan in deltas {
            let Some((planned, predicate)) = planned_by_index.get(&delta_plan.rule_group) else {
                continue;
            };
            let tuples = eval_planned_rule_group_with_delta(
                planned,
                catalog,
                database,
                PlannedDeltaView {
                    delta: &previous_delta,
                    atom_index: delta_plan.atom_index,
                    delta_relation: delta_plan.delta_relation,
                },
                warnings,
                options,
            )?;
            database.insert_new_derived_tuples(predicate, tuples, &mut delta);
        }
    }
    Ok(())
}

fn planned_authoritative_for_stage<'a>(
    stage: &RuleStagePlan,
    planned_stratum: &'a StratumPlan,
    active_predicate: PredicateRef,
    active_predicates: &BTreeSet<PredicateRef>,
) -> Result<Vec<(usize, &'a RuleGroupPlan, PredicateRef)>, EvalError> {
    let mut planned = Vec::new();
    for group_index in &stage.rule_groups {
        let group = planned_stratum
            .rule_groups
            .get(*group_index)
            .ok_or_else(|| EvalError::PlannedExecutorMissingAuthoritative {
                predicate: active_predicate.clone(),
            })?;
        let Some(provenance) = &group.provenance else {
            continue;
        };
        if !active_predicates.contains(&provenance.predicate) {
            continue;
        }
        if stage
            .authoritative_predicates
            .contains(&provenance.predicate)
        {
            planned.push((*group_index, group, provenance.predicate.clone()));
        }
    }
    if planned.is_empty() {
        return Err(EvalError::PlannedExecutorMixedAuthoritative {
            predicate: active_predicate,
        });
    }
    Ok(planned)
}
