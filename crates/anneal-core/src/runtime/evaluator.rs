//! Analysis-aware runtime evaluator facade.

use std::collections::BTreeSet;

use crate::ir::plan::{PlanError, ProgramPlan, plan};
use crate::runtime::analysis::{AnalyzedProgram, AnalyzedQuery};
use crate::runtime::ast::{Atom, Body, NegatedAtom, PredicateRef};
use crate::runtime::eval::{
    Database, DerivationNode, EvalError, EvalOptions, QueryOutput, QueryWarning, project_fact_head,
};
use crate::runtime::primitives::PrimitivePredicate;
use crate::vm::execute::{DerivedTuple, eval_planned_query_output};
use crate::vm::fixpoint::run_rule_group;
use crate::vm::provenance::derivation_ref;

pub struct Evaluator {
    program: AnalyzedProgram,
    planned: Result<ProgramPlan, PlanError>,
    database: Database,
    facts_seeded: bool,
    warnings: Vec<QueryWarning>,
    options: EvalOptions,
}

impl Evaluator {
    pub fn new(program: AnalyzedProgram, database: Database) -> Self {
        Self::with_options(program, database, EvalOptions::default())
    }

    pub fn with_options(
        program: AnalyzedProgram,
        mut database: Database,
        options: EvalOptions,
    ) -> Self {
        let planned = plan(&program);
        database.install_program_introspection(&program);
        database.ensure_derived(program.predicates().cloned());
        Self {
            program,
            planned,
            database,
            facts_seeded: false,
            warnings: Vec::new(),
            options,
        }
    }

    pub fn run_fixpoint(&mut self) -> Result<(), EvalError> {
        self.options.authorize_eval()?;
        self.seed_facts()?;
        self.run_fixpoint_matching(|_| true)
    }

    pub fn run_fixpoint_for_query(&mut self, query: &AnalyzedQuery) -> Result<(), EvalError> {
        self.options.authorize_eval()?;
        self.seed_facts()?;
        let needed = global_predicate_dependencies_for_query(&self.program, query);
        self.run_fixpoint_matching(|predicate| needed.contains(predicate))
    }

    fn run_fixpoint_matching(
        &mut self,
        predicate_needed: impl Fn(&PredicateRef) -> bool,
    ) -> Result<(), EvalError> {
        let planned = self
            .planned
            .as_ref()
            .map_err(|error| EvalError::from(error.clone()))?;
        let strata = self.program.strata().to_vec();
        for (stratum_index, stratum) in strata.into_iter().enumerate() {
            let active_predicates = stratum
                .predicates
                .iter()
                .filter(|predicate| predicate_needed(predicate))
                .cloned()
                .collect::<BTreeSet<_>>();
            if active_predicates.is_empty() {
                continue;
            }
            let planned_stratum = planned.global.strata.get(stratum_index);
            let planned_stratum =
                planned_stratum.ok_or_else(|| EvalError::PlannedExecutorMissingAuthoritative {
                    predicate: active_predicates
                        .iter()
                        .next()
                        .cloned()
                        .expect("non-empty rule group"),
                })?;
            run_rule_group(
                &mut self.database,
                &active_predicates,
                planned_stratum,
                &planned.catalog,
                &mut self.warnings,
                &self.options,
            )?;
        }
        Ok(())
    }

    pub fn eval_query(&self, query: &AnalyzedQuery) -> Result<QueryOutput, EvalError> {
        self.options.authorize_eval()?;
        let query_ast = query.query();
        let mut warnings = self.warnings.clone();
        let planned = self
            .planned
            .as_ref()
            .map_err(|error| EvalError::from(error.clone()))?;
        let query_plan = self
            .program
            .queries()
            .position(|candidate| candidate == query)
            .and_then(|index| planned.queries.get(index));
        if query_ast.local_rules.is_empty() {
            return eval_planned_query_output(
                query_plan,
                planned,
                &self.database,
                &warnings,
                &self.options,
            );
        }

        let mut database = self.database.clone();
        database.ensure_derived(query.local_predicates().cloned());
        database.install_query_introspection(query);
        for (stratum_index, stratum) in query.local_strata().iter().enumerate() {
            let active_predicates = stratum.predicates.iter().cloned().collect::<BTreeSet<_>>();
            let planned_stratum =
                query_plan.and_then(|query_plan| query_plan.plan.strata.get(stratum_index));
            let planned_stratum =
                planned_stratum.ok_or_else(|| EvalError::PlannedExecutorMissingAuthoritative {
                    predicate: active_predicates
                        .iter()
                        .next()
                        .cloned()
                        .expect("non-empty local rule group"),
                })?;
            run_rule_group(
                &mut database,
                &active_predicates,
                planned_stratum,
                &planned.catalog,
                &mut warnings,
                &self.options,
            )?;
        }
        eval_planned_query_output(query_plan, planned, &database, &warnings, &self.options)
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    fn seed_facts(&mut self) -> Result<(), EvalError> {
        if self.facts_seeded {
            return Ok(());
        }
        for fact in self.program.facts() {
            let tuple = project_fact_head(fact)?;
            let derivation = self
                .options
                .explain()
                .is_enabled()
                .then(|| derivation_ref(DerivationNode::fact(&fact.predicate, &tuple.0)));
            self.database
                .insert_derived_tuple(&fact.predicate, DerivedTuple { tuple, derivation });
        }
        self.facts_seeded = true;
        Ok(())
    }
}

fn global_predicate_dependencies_for_query(
    program: &AnalyzedProgram,
    query: &AnalyzedQuery,
) -> BTreeSet<PredicateRef> {
    let mut needed = BTreeSet::new();
    collect_body_global_predicates(&query.query().body, query, &mut needed);
    for rule in &query.query().local_rules {
        collect_body_global_predicates(&rule.body, query, &mut needed);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for rule in program.rules() {
            if !needed.contains(&rule.head.predicate) {
                continue;
            }
            let before = needed.len();
            collect_body_global_predicates(&rule.body, query, &mut needed);
            changed |= needed.len() != before;
        }
    }
    needed
}

fn collect_body_global_predicates(
    body: &Body,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    for atom in &body.atoms {
        collect_atom_global_predicates(atom, query, out);
    }
}

fn collect_atom_global_predicates(
    atom: &Atom,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    match atom {
        Atom::Derived(derived) => collect_global_predicate(&derived.predicate, query, out),
        Atom::Aggregation(aggregate) => {
            collect_body_global_predicates(&aggregate.body, query, out);
        }
        Atom::Negation(negation) => {
            if let NegatedAtom::Derived(derived) = &negation.atom {
                collect_global_predicate(&derived.predicate, query, out);
            }
        }
        Atom::TimeBlock(time_block) => {
            collect_body_global_predicates(&time_block.body, query, out);
        }
        Atom::Stored(_) | Atom::Comparison(_) => {}
    }
}

fn collect_global_predicate(
    predicate: &PredicateRef,
    query: &AnalyzedQuery,
    out: &mut BTreeSet<PredicateRef>,
) {
    if PrimitivePredicate::from_predicate(predicate).is_some() {
        return;
    }
    if query.local_predicates().any(|local| local == predicate) {
        return;
    }
    out.insert(predicate.clone());
}
