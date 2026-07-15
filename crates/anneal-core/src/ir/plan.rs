//! Planned IR for the runtime executor.
//!
//! This module lowers an already-analyzed program into relation, field, and slot
//! ids. The executor consumes this artifact directly, so predicate meaning,
//! stage migration, variable slots, and capability support are decided here
//! rather than rediscovered at runtime.

use std::collections::{BTreeMap, BTreeSet};

use crate::facts::{HANDLE_RELATION_NAME, SNAPSHOT_RELATION_NAME, STORED_RELATION_DESCRIPTORS};
use crate::runtime::analysis::{
    AnalyzedParameterNames, AnalyzedPredicateKind, AnalyzedPredicateSignature, AnalyzedProgram,
    AnalyzedQuery,
};
use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, ComparisonOp, Expr,
    FieldPattern, Head, Ident, Literal, NegatedAtom, NumberLiteral, OrderDirection, PredicateRef,
    Rule, RuleLayer, SourceLocation, StoredAtom, Term, TimeBlock,
};
use crate::runtime::primitives::PrimitivePredicate;
use crate::runtime::schedule::greedy_execution_order;
use crate::trail::TRAIL_RELATION_DESCRIPTORS;

use super::ids::{FieldId, RelationId, SlotId};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProgramPlan {
    pub(crate) catalog: PlanCatalog,
    pub(crate) global: Plan,
    pub(crate) queries: Vec<QueryPlan>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryPlan {
    pub(crate) plan: Plan,
}

impl QueryPlan {
    pub(crate) fn output_group(&self) -> Option<&RuleGroupPlan> {
        self.plan
            .strata
            .last()
            .and_then(|stratum| stratum.rule_groups.first())
    }

    pub(crate) fn output_stage(&self) -> Option<&RuleStagePlan> {
        self.plan
            .strata
            .last()
            .and_then(|stratum| stratum.stages.first())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    pub(crate) kind: PlanKind,
    pub(crate) strata: Vec<StratumPlan>,
    pub(crate) output: OutputPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanKind {
    GlobalFixpoint,
    Query,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StratumPlan {
    pub(crate) rule_groups: Vec<RuleGroupPlan>,
    pub(crate) stages: Vec<RuleStagePlan>,
    pub(crate) recursive: bool,
    pub(crate) authoritative_planned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleStagePlan {
    pub(crate) rule_groups: Vec<usize>,
    pub(crate) predicates: BTreeSet<PredicateRef>,
    pub(crate) execution: StageExecution,
    pub(crate) authoritative_predicates: BTreeSet<PredicateRef>,
    pub(crate) authoritative_planned: bool,
    pub(crate) migration: StageMigration,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StageExecution {
    SinglePass,
    Recursive { deltas: Vec<DeltaPlan> },
}

impl StageExecution {
    pub(crate) fn is_recursive(&self) -> bool {
        matches!(self, Self::Recursive { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StageMigration {
    pub(crate) mode: StageMigrationMode,
    pub(crate) reasons: BTreeSet<UnsupportedReason>,
}

impl StageMigration {
    fn planned() -> Self {
        Self {
            mode: StageMigrationMode::Planned,
            reasons: BTreeSet::new(),
        }
    }

    fn unsupported(reasons: BTreeSet<UnsupportedReason>) -> Self {
        Self {
            mode: StageMigrationMode::Unsupported,
            reasons,
        }
    }

    pub(crate) fn is_planned(&self) -> bool {
        self.mode == StageMigrationMode::Planned
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageMigrationMode {
    Planned,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UnsupportedReason {
    TimeScope,
    UnsupportedAggregate(AggregateFunction),
    UnsupportedExpression,
    UnsupportedComparison(ComparisonOp),
    MissingProvenance,
    Recursive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnsupportedTimeScopeAtom {
    StoredRelation { relation: Ident },
    DerivedPredicate { predicate: PredicateRef },
    Primitive { predicate: PredicateRef },
    UnknownRelation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnsupportedTimeScope {
    pub(crate) reference: String,
    pub(crate) atom: UnsupportedTimeScopeAtom,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeltaPlan {
    pub(crate) rule_group: usize,
    pub(crate) atom_index: usize,
    pub(crate) delta_relation: RelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleGroupPlan {
    pub(crate) head: Option<RelationId>,
    pub(crate) head_terms: Vec<TermPlan>,
    pub(crate) body: RuleBodyPlan,
    pub(crate) slots: SlotLayout,
    pub(crate) provenance: Option<RuleProvenance>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RuleBodyPlan {
    pub(crate) atoms: Vec<AtomPlan>,
    pub(crate) execution_atoms: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleProvenance {
    pub(crate) predicate: PredicateRef,
    pub(crate) layer: RuleLayer,
    pub(crate) location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AggregateProvenance {
    pub(crate) function: AggregateFunction,
    pub(crate) location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompareProvenance {
    pub(crate) location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NegationProvenance {
    pub(crate) location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimeScopeProvenance {
    pub(crate) reference: String,
    pub(crate) location: SourceLocation,
    pub(crate) unsupported: Option<UnsupportedTimeScope>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AtomPlan {
    Scan {
        relation: RelationId,
        patterns: Vec<ColumnPatternPlan>,
    },
    Filter {
        comparison: ComparePlan,
    },
    Negation {
        inner: Box<RuleBodyPlan>,
        bound_inputs: Vec<SlotId>,
        provenance: NegationProvenance,
    },
    PrimitiveCall {
        predicate: PredicateRef,
        primitive: PrimitivePredicate,
        args: Vec<CallArgPlan>,
        input_slots: Vec<SlotId>,
        output_slots: Vec<SlotId>,
        provider: ProviderRef,
        capability: CapabilityAction,
        demand: DemandPolicy,
    },
    Aggregate(Box<AggregatePlan>),
    TimeScope {
        reference: String,
        inner: Box<RuleBodyPlan>,
        outer_slots: Vec<SlotId>,
        provenance: TimeScopeProvenance,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ColumnPatternPlan {
    pub(crate) field: FieldId,
    pub(crate) term: TermPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CallArgPlan {
    pub(crate) position: usize,
    pub(crate) term: TermPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TermPlan {
    Wildcard,
    Expr(ExprPlan),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ComparePlan {
    pub(crate) left: ExprPlan,
    pub(crate) op: ComparisonOp,
    pub(crate) right: ExprPlan,
    pub(crate) provenance: CompareProvenance,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AggregatePlan {
    pub(crate) function: AggregateFunction,
    pub(crate) provenance: AggregateProvenance,
    pub(crate) inner: Box<RuleBodyPlan>,
    pub(crate) inner_slots: SlotLayout,
    pub(crate) outer_to_inner_slots: Vec<(SlotId, SlotId)>,
    pub(crate) outer_slots: Vec<SlotId>,
    pub(crate) args: AggregateArgsPlan,
    pub(crate) value: ExprPlan,
    pub(crate) result: ExprPlan,
    pub(crate) result_slots: Vec<SlotId>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExprPlan {
    Slot(SlotId),
    Literal(LiteralPlan),
    Binary {
        left: Box<ExprPlan>,
        op: crate::runtime::ast::ArithmeticOp,
        right: Box<ExprPlan>,
    },
    Tuple(Vec<ExprPlan>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LiteralPlan {
    String(String),
    Number(NumberLiteral),
    Bool(bool),
    Null,
    List(Vec<LiteralPlan>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AggregateArgsPlan {
    pub(crate) k: Option<ExprPlan>,
    pub(crate) budget: Option<ExprPlan>,
    pub(crate) sum: Option<ExprPlan>,
    pub(crate) key: Option<ExprPlan>,
    pub(crate) synthetic_rank_slot: Option<SlotId>,
    pub(crate) error: Option<AggregateArgPlanError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AggregateArgPlanError {
    Unknown,
    Duplicate,
    Missing(AggregateArgName),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AggregateArgName {
    K,
    Budget,
    Sum,
    Key,
    Rank,
}

impl AggregateArgName {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "k" => Some(Self::K),
            "budget" => Some(Self::Budget),
            "sum" => Some(Self::Sum),
            "key" => Some(Self::Key),
            "rank" => Some(Self::Rank),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::K => "k",
            Self::Budget => "budget",
            Self::Sum => "sum",
            Self::Key => "key",
            Self::Rank => "rank",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderRef {
    Graph,
    Content,
    Search,
    Introspection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapabilityAction {
    None,
    Search,
    Read,
    ReadFull,
    Match,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DemandPolicy {
    Eager,
    Lazy,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OutputPlan {
    pub(crate) projection: Vec<(Ident, ExprPlan)>,
    pub(crate) ordering: Vec<OrderKeyPlan>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OrderKeyPlan {
    pub(crate) expr: ExprPlan,
    pub(crate) direction: OrderDirection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlotLayout {
    vars: BTreeMap<Ident, SlotId>,
    slots: Vec<Ident>,
}

impl SlotLayout {
    fn from_vars(vars: BTreeSet<Ident>) -> Self {
        let slots = vars.into_iter().collect::<Vec<_>>();
        let vars = slots
            .iter()
            .enumerate()
            .map(|(index, var)| (var.clone(), SlotId::from_index(index)))
            .collect();
        Self { vars, slots }
    }

    fn slot(&self, var: &Ident) -> Result<SlotId, PlanError> {
        self.vars
            .get(var)
            .copied()
            .ok_or_else(|| PlanError::UnplannedVariable {
                variable: var.clone(),
            })
    }

    pub(crate) fn vars(&self) -> &[Ident] {
        &self.slots
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlanCatalog {
    relations: Vec<PlanRelation>,
    by_stored: BTreeMap<Ident, RelationId>,
    by_predicate: BTreeMap<PredicateRef, RelationId>,
    query_locals: Vec<BTreeMap<PredicateRef, RelationId>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlanRelation {
    pub(crate) id: RelationId,
    pub(crate) name: String,
    pub(crate) kind: PlanRelationKind,
    fields: Vec<Ident>,
    by_field: BTreeMap<Ident, FieldId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanRelationKind {
    Stored,
    Derived,
    Primitive {
        primitive: PrimitivePredicate,
        sealed: bool,
    },
}

impl PlanCatalog {
    fn from_analyzed(program: &AnalyzedProgram) -> Result<Self, PlanError> {
        let mut catalog = Self {
            relations: Vec::new(),
            by_stored: BTreeMap::new(),
            by_predicate: BTreeMap::new(),
            query_locals: Vec::new(),
        };
        catalog.register_stored_relations();
        for (predicate, signature) in program.predicate_signatures() {
            catalog.register_predicate(predicate.clone(), signature)?;
        }
        for (query_index, query) in program.queries().enumerate() {
            catalog.register_query_local_predicates(query_index, query);
        }
        Ok(catalog)
    }

    fn register_stored_relations(&mut self) {
        for descriptor in STORED_RELATION_DESCRIPTORS
            .iter()
            .chain(TRAIL_RELATION_DESCRIPTORS.iter())
        {
            let relation = self.push_relation(
                descriptor.name.to_string(),
                PlanRelationKind::Stored,
                descriptor
                    .fields
                    .iter()
                    .map(|field| Ident::new_unchecked(*field)),
            );
            self.by_stored
                .insert(Ident::new_unchecked(descriptor.name), relation);
        }
    }

    fn register_predicate(
        &mut self,
        predicate: PredicateRef,
        signature: AnalyzedPredicateSignature,
    ) -> Result<RelationId, PlanError> {
        if let Some(relation) = self.by_predicate.get(&predicate) {
            return Ok(*relation);
        }
        let kind = match signature.kind {
            AnalyzedPredicateKind::Derived => PlanRelationKind::Derived,
            AnalyzedPredicateKind::Primitive { sealed } => {
                let primitive =
                    PrimitivePredicate::from_predicate(&predicate).ok_or_else(|| {
                        PlanError::UnknownPrimitive {
                            predicate: predicate.clone(),
                        }
                    })?;
                PlanRelationKind::Primitive { primitive, sealed }
            }
        };
        let fields = fields_from_signature(signature);
        let relation = self.push_relation(predicate.display_name(), kind, fields);
        self.by_predicate.insert(predicate, relation);
        Ok(relation)
    }

    fn register_query_local_predicates(&mut self, query_index: usize, query: &AnalyzedQuery) {
        while self.query_locals.len() <= query_index {
            self.query_locals.push(BTreeMap::new());
        }
        for rule in &query.query().local_rules {
            let predicate = rule.head.predicate.clone();
            if self.query_locals[query_index].contains_key(&predicate) {
                continue;
            }
            let signature = AnalyzedPredicateSignature {
                arity: rule.head.arity(),
                parameters: head_parameters(&rule.head),
                kind: AnalyzedPredicateKind::Derived,
            };
            let fields = fields_from_signature(signature);
            let relation =
                self.push_relation(predicate.display_name(), PlanRelationKind::Derived, fields);
            self.query_locals[query_index].insert(predicate, relation);
        }
    }

    fn push_relation(
        &mut self,
        name: String,
        kind: PlanRelationKind,
        fields: impl IntoIterator<Item = Ident>,
    ) -> RelationId {
        let id = RelationId::from_index(self.relations.len());
        let fields = fields.into_iter().collect::<Vec<_>>();
        let by_field = fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.clone(), FieldId::from_index(index)))
            .collect();
        self.relations.push(PlanRelation {
            id,
            name,
            kind,
            fields,
            by_field,
        });
        id
    }

    pub(crate) fn relation(&self, id: RelationId) -> Option<&PlanRelation> {
        self.relations.get(id.index())
    }

    pub(crate) fn stored_relation(&self, relation: &Ident) -> Option<&PlanRelation> {
        self.by_stored
            .get(relation)
            .and_then(|id| self.relation(*id))
    }

    pub(crate) fn predicate_relation(&self, predicate: &PredicateRef) -> Option<&PlanRelation> {
        self.by_predicate
            .get(predicate)
            .and_then(|id| self.relation(*id))
    }

    fn predicate_relation_in_scope(
        &self,
        predicate: &PredicateRef,
        query_index: Option<usize>,
    ) -> Option<&PlanRelation> {
        if let Some(query_index) = query_index
            && let Some(relation) = self
                .query_locals
                .get(query_index)
                .and_then(|locals| locals.get(predicate))
                .and_then(|id| self.relation(*id))
        {
            return Some(relation);
        }
        self.predicate_relation(predicate)
    }

    #[cfg(test)]
    pub(crate) fn relations(&self) -> &[PlanRelation] {
        &self.relations
    }
}

impl PlanRelation {
    fn field(&self, field: &Ident) -> Result<FieldId, PlanError> {
        self.by_field
            .get(field)
            .copied()
            .ok_or_else(|| PlanError::UnknownField {
                relation: self.name.clone(),
                field: field.clone(),
            })
    }

    fn field_by_index(&self, index: usize) -> Result<FieldId, PlanError> {
        if index < self.fields.len() {
            Ok(FieldId::from_index(index))
        } else {
            Err(PlanError::ArityMismatch {
                relation: self.name.clone(),
                expected: self.fields.len(),
                actual: index + 1,
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn fields(&self) -> &[Ident] {
        &self.fields
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PlanError {
    #[error("unknown stored relation '*{relation}'")]
    UnknownStoredRelation { relation: Ident },
    #[error("unknown predicate '{predicate}'")]
    UnknownPredicate { predicate: PredicateRef },
    #[error("unknown primitive '{predicate}'")]
    UnknownPrimitive { predicate: PredicateRef },
    #[error("unknown field '{field}' for relation '{relation}'")]
    UnknownField { relation: String, field: Ident },
    #[error("arity mismatch for relation '{relation}': expected {expected}, got {actual}")]
    ArityMismatch {
        relation: String,
        expected: usize,
        actual: usize,
    },
    #[error("unplanned variable '{variable}'")]
    UnplannedVariable { variable: Ident },
    #[error("unsupported expression")]
    UnsupportedExpression,
}

pub(crate) fn plan(program: &AnalyzedProgram) -> Result<ProgramPlan, PlanError> {
    let catalog = PlanCatalog::from_analyzed(program)?;
    let global = plan_global(program, &catalog)?;
    let queries = program
        .queries()
        .enumerate()
        .map(|(query_index, query)| plan_query(query_index, query, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProgramPlan {
        catalog,
        global,
        queries,
    })
}

fn plan_global(program: &AnalyzedProgram, catalog: &PlanCatalog) -> Result<Plan, PlanError> {
    let strata = program
        .strata()
        .iter()
        .map(|stratum| {
            let stratum_predicates = stratum.predicates.iter().cloned().collect::<BTreeSet<_>>();
            let rules = program
                .rules()
                .filter(|rule| stratum_predicates.contains(&rule.head.predicate))
                .collect::<Vec<_>>();
            let mut rule_groups = Vec::new();
            for rule in &rules {
                let rule_group = plan_rule(rule, catalog, None)?;
                rule_groups.push(rule_group);
            }
            let stages =
                plan_rule_stages(&rules, &rule_groups, &stratum_predicates, catalog, None)?;
            Ok(StratumPlan {
                recursive: stages.iter().any(|stage| stage.execution.is_recursive()),
                authoritative_planned: stages.iter().any(|stage| stage.authoritative_planned),
                rule_groups,
                stages,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    Ok(Plan {
        kind: PlanKind::GlobalFixpoint,
        strata,
        output: OutputPlan::default(),
    })
}

fn plan_query(
    query_index: usize,
    query: &AnalyzedQuery,
    catalog: &PlanCatalog,
) -> Result<QueryPlan, PlanError> {
    let strata = query
        .local_strata()
        .iter()
        .map(|stratum| {
            let stratum_predicates = stratum.predicates.iter().cloned().collect::<BTreeSet<_>>();
            let rules = query
                .query()
                .local_rules
                .iter()
                .filter(|rule| stratum_predicates.contains(&rule.head.predicate))
                .collect::<Vec<_>>();
            let rule_groups = rules
                .iter()
                .map(|rule| plan_rule(rule, catalog, Some(query_index)))
                .collect::<Result<Vec<_>, _>>()?;
            let stages = plan_rule_stages(
                &rules,
                &rule_groups,
                &stratum_predicates,
                catalog,
                Some(query_index),
            )?;
            Ok(StratumPlan {
                recursive: stages.iter().any(|stage| stage.execution.is_recursive()),
                authoritative_planned: stages.iter().any(|stage| stage.authoritative_planned),
                rule_groups,
                stages,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let slots = SlotLayout::from_vars(query.query().body.positive_binding_variables());
    let body = plan_body(&query.query().body, catalog, &slots, Some(query_index))?;
    let projection = slots
        .vars()
        .iter()
        .map(|var| Ok((var.clone(), ExprPlan::Slot(slots.slot(var)?))))
        .collect::<Result<Vec<_>, PlanError>>()?;
    let ordering = query
        .query()
        .ordering
        .iter()
        .map(|key| {
            Ok(OrderKeyPlan {
                expr: plan_expr(&key.expr, &slots)?,
                direction: key.direction,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let output_group = RuleGroupPlan {
        head: None,
        head_terms: Vec::new(),
        body,
        slots,
        provenance: None,
    };
    let output_migration = query_output_migration(&output_group, catalog);
    let output_authoritative = output_migration.is_planned();
    Ok(QueryPlan {
        plan: Plan {
            kind: PlanKind::Query,
            strata: {
                let mut all = strata;
                all.push(StratumPlan {
                    recursive: false,
                    authoritative_planned: output_authoritative,
                    rule_groups: vec![output_group],
                    stages: vec![RuleStagePlan {
                        rule_groups: vec![0],
                        predicates: BTreeSet::new(),
                        execution: StageExecution::SinglePass,
                        authoritative_predicates: BTreeSet::new(),
                        authoritative_planned: output_authoritative,
                        migration: output_migration,
                    }],
                });
                all
            },
            output: OutputPlan {
                projection,
                ordering,
            },
        },
    })
}

fn plan_rule(
    rule: &Rule,
    catalog: &PlanCatalog,
    query_index: Option<usize>,
) -> Result<RuleGroupPlan, PlanError> {
    let mut vars = BTreeSet::new();
    collect_head_vars(&rule.head, &mut vars);
    collect_rule_body_vars(&rule.body, &mut vars);
    let slots = SlotLayout::from_vars(vars);
    let head = catalog
        .predicate_relation_in_scope(&rule.head.predicate, query_index)
        .ok_or_else(|| PlanError::UnknownPredicate {
            predicate: rule.head.predicate.clone(),
        })?
        .id;
    let head_terms = rule
        .head
        .terms
        .iter()
        .map(|term| plan_term(term, &slots))
        .collect::<Result<Vec<_>, _>>()?;
    let body = plan_body(&rule.body, catalog, &slots, query_index)?;
    Ok(RuleGroupPlan {
        head: Some(head),
        head_terms,
        body,
        slots,
        provenance: Some(RuleProvenance {
            predicate: rule.head.predicate.clone(),
            layer: rule.origin().layer(),
            location: rule.origin().location().clone(),
        }),
    })
}

fn plan_rule_stages(
    rules: &[&Rule],
    rule_groups: &[RuleGroupPlan],
    stratum_predicates: &BTreeSet<PredicateRef>,
    catalog: &PlanCatalog,
    query_index: Option<usize>,
) -> Result<Vec<RuleStagePlan>, PlanError> {
    let mut dependencies = BTreeMap::<PredicateRef, BTreeSet<PredicateRef>>::new();
    let mut groups_by_predicate = BTreeMap::<PredicateRef, Vec<usize>>::new();

    for (index, rule) in rules.iter().enumerate() {
        groups_by_predicate
            .entry(rule.head.predicate.clone())
            .or_default()
            .push(index);
        let mut deps = BTreeSet::new();
        collect_positive_dependencies(&rule.body, stratum_predicates, &mut deps);
        dependencies
            .entry(rule.head.predicate.clone())
            .or_default()
            .extend(deps);
    }
    for predicate in stratum_predicates {
        dependencies.entry(predicate.clone()).or_default();
    }

    let components = positive_dependency_components(stratum_predicates, &dependencies);
    let mut remaining = components
        .iter()
        .enumerate()
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();
    let mut completed = BTreeSet::new();
    let mut stages = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .copied()
            .filter(|component_index| {
                component_dependencies(&components[*component_index], &dependencies)
                    .into_iter()
                    .all(|dependency| completed.contains(&dependency))
            })
            .collect::<BTreeSet<_>>();
        if ready.is_empty() {
            return Err(PlanError::UnsupportedExpression);
        }
        for component_index in ready {
            let predicates = components[component_index].clone();
            let rule_group_indexes = rules
                .iter()
                .enumerate()
                .filter(|(_, rule)| predicates.contains(&rule.head.predicate))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let execution = stage_execution(
                &predicates,
                &rule_group_indexes,
                rules,
                catalog,
                query_index,
                &dependencies,
            )?;
            let migration = stage_migration(&rule_group_indexes, rule_groups, catalog, &execution);
            let authoritative_predicates = predicates
                .iter()
                .filter(|predicate| {
                    groups_by_predicate.contains_key(*predicate) && migration.is_planned()
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            let authoritative_planned = !authoritative_predicates.is_empty();
            stages.push(RuleStagePlan {
                rule_groups: rule_group_indexes,
                predicates: predicates.clone(),
                execution,
                authoritative_predicates,
                authoritative_planned,
                migration,
            });
            remaining.remove(&component_index);
            completed.extend(predicates);
        }
    }
    Ok(stages)
}

fn positive_dependency_components(
    predicates: &BTreeSet<PredicateRef>,
    dependencies: &BTreeMap<PredicateRef, BTreeSet<PredicateRef>>,
) -> Vec<BTreeSet<PredicateRef>> {
    let mut search = PositiveDependencyComponents {
        dependencies,
        scope: predicates,
        next_index: 0,
        indexes: BTreeMap::new(),
        lowlinks: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        components: Vec::new(),
    };
    for predicate in predicates {
        if !search.indexes.contains_key(predicate) {
            search.connect(predicate);
        }
    }
    search
        .components
        .sort_by(|left, right| left.iter().next().cmp(&right.iter().next()));
    search.components
}

struct PositiveDependencyComponents<'a> {
    dependencies: &'a BTreeMap<PredicateRef, BTreeSet<PredicateRef>>,
    scope: &'a BTreeSet<PredicateRef>,
    next_index: usize,
    indexes: BTreeMap<PredicateRef, usize>,
    lowlinks: BTreeMap<PredicateRef, usize>,
    stack: Vec<PredicateRef>,
    on_stack: BTreeSet<PredicateRef>,
    components: Vec<BTreeSet<PredicateRef>>,
}

impl PositiveDependencyComponents<'_> {
    fn connect(&mut self, predicate: &PredicateRef) {
        let index = self.next_index;
        self.next_index += 1;
        self.indexes.insert(predicate.clone(), index);
        self.lowlinks.insert(predicate.clone(), index);
        self.stack.push(predicate.clone());
        self.on_stack.insert(predicate.clone());

        let dependencies = self
            .dependencies
            .get(predicate)
            .into_iter()
            .flatten()
            .filter(|dependency| self.scope.contains(*dependency))
            .cloned()
            .collect::<Vec<_>>();
        for dependency in dependencies {
            if !self.indexes.contains_key(&dependency) {
                self.connect(&dependency);
                let dependency_lowlink = self
                    .lowlinks
                    .get(&dependency)
                    .copied()
                    .expect("connected dependency has a lowlink");
                let lowlink = self
                    .lowlinks
                    .get_mut(predicate)
                    .expect("connected predicate has a lowlink");
                *lowlink = (*lowlink).min(dependency_lowlink);
            } else if self.on_stack.contains(&dependency) {
                let dependency_index = self
                    .indexes
                    .get(&dependency)
                    .copied()
                    .expect("visited dependency has an index");
                let lowlink = self
                    .lowlinks
                    .get_mut(predicate)
                    .expect("connected predicate has a lowlink");
                *lowlink = (*lowlink).min(dependency_index);
            }
        }

        if self
            .lowlinks
            .get(predicate)
            .copied()
            .expect("connected predicate has a lowlink")
            == index
        {
            let mut component = BTreeSet::new();
            loop {
                let member = self
                    .stack
                    .pop()
                    .expect("Tarjan root has a matching stack entry");
                self.on_stack.remove(&member);
                component.insert(member.clone());
                if member == *predicate {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

fn component_dependencies(
    component: &BTreeSet<PredicateRef>,
    dependencies: &BTreeMap<PredicateRef, BTreeSet<PredicateRef>>,
) -> BTreeSet<PredicateRef> {
    component
        .iter()
        .filter_map(|predicate| dependencies.get(predicate))
        .flat_map(|deps| deps.iter())
        .filter(|dependency| !component.contains(*dependency))
        .cloned()
        .collect()
}

fn stage_execution(
    predicates: &BTreeSet<PredicateRef>,
    rule_group_indexes: &[usize],
    rules: &[&Rule],
    catalog: &PlanCatalog,
    query_index: Option<usize>,
    dependencies: &BTreeMap<PredicateRef, BTreeSet<PredicateRef>>,
) -> Result<StageExecution, PlanError> {
    let recursive = predicates.len() > 1
        || predicates.iter().any(|predicate| {
            dependencies
                .get(predicate)
                .is_some_and(|deps| deps.contains(predicate))
        });
    if !recursive {
        return Ok(StageExecution::SinglePass);
    }

    let mut deltas = Vec::new();
    for rule_group in rule_group_indexes {
        let Some(rule) = rules.get(*rule_group) else {
            return Err(PlanError::UnsupportedExpression);
        };
        for (atom_index, predicate) in recursive_atom_predicates(&rule.body, predicates) {
            let delta_relation = catalog
                .predicate_relation_in_scope(&predicate, query_index)
                .ok_or_else(|| PlanError::UnknownPredicate {
                    predicate: predicate.clone(),
                })?
                .id;
            deltas.push(DeltaPlan {
                rule_group: *rule_group,
                atom_index,
                delta_relation,
            });
        }
    }
    Ok(StageExecution::Recursive { deltas })
}

fn stage_migration(
    rule_group_indexes: &[usize],
    rule_groups: &[RuleGroupPlan],
    catalog: &PlanCatalog,
    execution: &StageExecution,
) -> StageMigration {
    let mut reasons = BTreeSet::new();
    for index in rule_group_indexes {
        let Some(group) = rule_groups.get(*index) else {
            reasons.insert(UnsupportedReason::UnsupportedExpression);
            continue;
        };
        reasons.extend(planned_rule_group_unsupported_reasons(group, catalog));
    }
    if let StageExecution::Recursive { deltas } = execution
        && deltas.is_empty()
    {
        reasons.insert(UnsupportedReason::Recursive);
    }
    if reasons.is_empty() {
        StageMigration::planned()
    } else {
        StageMigration::unsupported(reasons)
    }
}

fn query_output_migration(output_group: &RuleGroupPlan, catalog: &PlanCatalog) -> StageMigration {
    let mut reasons = BTreeSet::new();
    collect_body_unsupported_reasons(&output_group.body, catalog, &mut reasons);
    if reasons.is_empty() {
        StageMigration::planned()
    } else {
        StageMigration::unsupported(reasons)
    }
}

#[cfg(test)]
pub(crate) fn planned_rule_group_executable(
    planned: &RuleGroupPlan,
    catalog: &PlanCatalog,
) -> bool {
    planned_rule_group_unsupported_reasons(planned, catalog).is_empty()
}

pub(crate) fn planned_aggregate_executable(function: AggregateFunction) -> bool {
    matches!(
        function,
        AggregateFunction::Count
            | AggregateFunction::Sum
            | AggregateFunction::Min
            | AggregateFunction::Max
            | AggregateFunction::Avg
            | AggregateFunction::List
            | AggregateFunction::Set
            | AggregateFunction::TopK
            | AggregateFunction::Rank
            | AggregateFunction::TakeUntil
    )
}

pub(crate) fn aggregate_allowed_args(function: AggregateFunction) -> &'static [AggregateArgName] {
    match function {
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::Avg
        | AggregateFunction::List
        | AggregateFunction::Set => &[],
        AggregateFunction::TopK => &[AggregateArgName::K, AggregateArgName::Key],
        AggregateFunction::Rank => &[AggregateArgName::Key, AggregateArgName::Rank],
        AggregateFunction::TakeUntil => &[
            AggregateArgName::Budget,
            AggregateArgName::Sum,
            AggregateArgName::Key,
        ],
    }
}

pub(crate) fn planned_comparison_executable(op: ComparisonOp) -> bool {
    op != ComparisonOp::Matches
}

pub(crate) fn time_scope_executable(
    reference: &str,
    inner: &RuleBodyPlan,
    catalog: &PlanCatalog,
) -> bool {
    time_scope_unsupported_atom(reference, inner, catalog).is_none()
}

pub(crate) fn time_scope_unsupported_atom(
    reference: &str,
    inner: &RuleBodyPlan,
    catalog: &PlanCatalog,
) -> Option<UnsupportedTimeScope> {
    inner
        .atoms
        .iter()
        .find_map(|atom| time_scope_atom_unsupported(reference, atom, catalog))
}

fn time_scope_atom_unsupported(
    reference: &str,
    atom: &AtomPlan,
    catalog: &PlanCatalog,
) -> Option<UnsupportedTimeScope> {
    let atom = match atom {
        AtomPlan::Scan { relation, .. } => {
            let Some(relation) = catalog.relation(*relation) else {
                return Some(UnsupportedTimeScope {
                    reference: reference.to_string(),
                    atom: UnsupportedTimeScopeAtom::UnknownRelation,
                });
            };
            match relation.kind {
                PlanRelationKind::Stored => {
                    (!time_scoped_stored_relation_supported_name(&relation.name)).then(|| {
                        UnsupportedTimeScopeAtom::StoredRelation {
                            relation: Ident::new_unchecked(relation.name.clone()),
                        }
                    })
                }
                PlanRelationKind::Derived => {
                    let Ok(predicate) = PredicateRef::parse(&relation.name) else {
                        return Some(UnsupportedTimeScope {
                            reference: reference.to_string(),
                            atom: UnsupportedTimeScopeAtom::UnknownRelation,
                        });
                    };
                    Some(UnsupportedTimeScopeAtom::DerivedPredicate { predicate })
                }
                PlanRelationKind::Primitive { .. } => {
                    Some(UnsupportedTimeScopeAtom::UnknownRelation)
                }
            }
        }
        AtomPlan::PrimitiveCall {
            predicate,
            primitive,
            ..
        } => (!time_scoped_primitive_supported(*primitive)).then_some(
            UnsupportedTimeScopeAtom::Primitive {
                predicate: predicate.clone(),
            },
        ),
        AtomPlan::Filter { .. } => None,
        AtomPlan::Aggregate(aggregate) => {
            return time_scope_unsupported_atom(reference, &aggregate.inner, catalog);
        }
        AtomPlan::Negation { inner, .. } => {
            return time_scope_unsupported_atom(reference, inner, catalog);
        }
        AtomPlan::TimeScope {
            reference: nested,
            inner,
            ..
        } => return time_scope_unsupported_atom(nested, inner, catalog),
    };
    atom.map(|atom| UnsupportedTimeScope {
        reference: reference.to_string(),
        atom,
    })
}

fn time_scoped_stored_relation_supported_name(relation: &str) -> bool {
    matches!(relation, HANDLE_RELATION_NAME | SNAPSHOT_RELATION_NAME)
}

pub(crate) fn time_scoped_primitive_supported(primitive: PrimitivePredicate) -> bool {
    matches!(
        primitive,
        PrimitivePredicate::Terminal
            | PrimitivePredicate::Active
            | PrimitivePredicate::Settled
            | PrimitivePredicate::PipelinePosition
            | PrimitivePredicate::PipelinePositionFor
            | PrimitivePredicate::Obligation
            | PrimitivePredicate::Freshness
            | PrimitivePredicate::Flux
    )
}
pub(crate) fn planned_rule_group_unsupported_reasons(
    planned: &RuleGroupPlan,
    catalog: &PlanCatalog,
) -> BTreeSet<UnsupportedReason> {
    let mut reasons = BTreeSet::new();
    if planned.provenance.is_none() {
        reasons.insert(UnsupportedReason::MissingProvenance);
    }
    collect_body_unsupported_reasons(&planned.body, catalog, &mut reasons);
    reasons
}

fn collect_body_unsupported_reasons(
    body: &RuleBodyPlan,
    catalog: &PlanCatalog,
    out: &mut BTreeSet<UnsupportedReason>,
) {
    for atom in &body.atoms {
        collect_atom_unsupported_reasons(atom, catalog, out);
    }
}

fn collect_atom_unsupported_reasons(
    atom: &AtomPlan,
    catalog: &PlanCatalog,
    out: &mut BTreeSet<UnsupportedReason>,
) {
    match atom {
        AtomPlan::Scan { .. } | AtomPlan::PrimitiveCall { .. } => {}
        AtomPlan::Filter { comparison } => {
            if !planned_comparison_executable(comparison.op) {
                out.insert(UnsupportedReason::UnsupportedComparison(comparison.op));
            }
        }
        AtomPlan::Aggregate(aggregate) => {
            if !planned_aggregate_executable(aggregate.function) {
                out.insert(UnsupportedReason::UnsupportedAggregate(aggregate.function));
            }
            collect_body_unsupported_reasons(&aggregate.inner, catalog, out);
        }
        AtomPlan::Negation { inner, .. } => collect_body_unsupported_reasons(inner, catalog, out),
        AtomPlan::TimeScope {
            reference, inner, ..
        } => {
            collect_body_unsupported_reasons(inner, catalog, out);
            if !time_scope_executable(reference, inner, catalog) {
                out.insert(UnsupportedReason::TimeScope);
            }
        }
    }
}

fn collect_positive_dependencies(
    body: &Body,
    stratum_predicates: &BTreeSet<PredicateRef>,
    out: &mut BTreeSet<PredicateRef>,
) {
    for atom in &body.atoms {
        match atom {
            Atom::Derived(derived) if stratum_predicates.contains(&derived.predicate) => {
                out.insert(derived.predicate.clone());
            }
            Atom::TimeBlock(time_block) => {
                collect_positive_dependencies(&time_block.body, stratum_predicates, out);
            }
            Atom::Stored(_)
            | Atom::Derived(_)
            | Atom::Comparison(_)
            | Atom::Aggregation(_)
            | Atom::Negation(_) => {}
        }
    }
}

fn plan_body(
    body: &Body,
    catalog: &PlanCatalog,
    slots: &SlotLayout,
    query_index: Option<usize>,
) -> Result<RuleBodyPlan, PlanError> {
    let atoms = body
        .atoms
        .iter()
        .map(|atom| plan_atom(atom, catalog, slots, query_index))
        .collect::<Result<Vec<_>, _>>()?;
    let execution_atoms = greedy_execution_order(body);
    Ok(RuleBodyPlan {
        atoms,
        execution_atoms,
    })
}

fn plan_atom(
    atom: &Atom,
    catalog: &PlanCatalog,
    slots: &SlotLayout,
    query_index: Option<usize>,
) -> Result<AtomPlan, PlanError> {
    match atom {
        Atom::Stored(stored) => plan_stored_atom(stored, catalog, slots),
        Atom::Derived(derived) => {
            let relation = catalog
                .predicate_relation_in_scope(&derived.predicate, query_index)
                .ok_or_else(|| PlanError::UnknownPredicate {
                    predicate: derived.predicate.clone(),
                })?;
            match relation.kind {
                PlanRelationKind::Primitive { primitive, .. } => plan_primitive_atom(
                    derived.predicate.clone(),
                    primitive,
                    relation,
                    &derived.args,
                    slots,
                ),
                PlanRelationKind::Derived => plan_derived_scan(relation, &derived.args, slots),
                PlanRelationKind::Stored => Err(PlanError::UnknownPredicate {
                    predicate: derived.predicate.clone(),
                }),
            }
        }
        Atom::Comparison(comparison) => plan_comparison(comparison, slots),
        Atom::Aggregation(aggregate) => plan_aggregate(aggregate, catalog, slots, query_index),
        Atom::Negation(negation) => plan_negation(negation, catalog, slots, query_index),
        Atom::TimeBlock(time_block) => plan_time_scope(time_block, catalog, slots, query_index),
    }
}

fn plan_stored_atom(
    atom: &StoredAtom,
    catalog: &PlanCatalog,
    slots: &SlotLayout,
) -> Result<AtomPlan, PlanError> {
    let relation = catalog.stored_relation(&atom.relation).ok_or_else(|| {
        PlanError::UnknownStoredRelation {
            relation: atom.relation.clone(),
        }
    })?;
    let patterns = atom
        .fields
        .iter()
        .map(|field| plan_field_pattern(relation, field, slots))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AtomPlan::Scan {
        relation: relation.id,
        patterns,
    })
}

fn plan_derived_scan(
    relation: &PlanRelation,
    args: &[CallArg],
    slots: &SlotLayout,
) -> Result<AtomPlan, PlanError> {
    let patterns = args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            Ok(ColumnPatternPlan {
                field: relation.field_by_index(index)?,
                term: plan_call_arg(arg, slots)?,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    Ok(AtomPlan::Scan {
        relation: relation.id,
        patterns,
    })
}

fn plan_primitive_atom(
    predicate: PredicateRef,
    primitive: PrimitivePredicate,
    relation: &PlanRelation,
    args: &[CallArg],
    slots: &SlotLayout,
) -> Result<AtomPlan, PlanError> {
    let mut planned_args = Vec::with_capacity(args.len());
    let required_positions = primitive
        .required_bound_inputs()
        .iter()
        .map(|input| input.position)
        .collect::<BTreeSet<_>>();
    let mut input_slots = BTreeSet::new();
    let mut output_slots = BTreeSet::new();
    for (index, arg) in args.iter().enumerate() {
        let term = plan_call_arg(arg, slots)?;
        if required_positions.contains(&index) {
            collect_term_input_slots(&term, &mut input_slots);
        } else {
            collect_term_output_slots(&term, &mut output_slots);
        }
        if index >= relation.fields.len() {
            return Err(PlanError::ArityMismatch {
                relation: relation.name.clone(),
                expected: relation.fields.len(),
                actual: index + 1,
            });
        }
        planned_args.push(CallArgPlan {
            position: index,
            term,
        });
    }
    Ok(AtomPlan::PrimitiveCall {
        predicate,
        primitive,
        args: planned_args,
        input_slots: input_slots.into_iter().collect(),
        output_slots: output_slots.into_iter().collect(),
        provider: provider_for_primitive(primitive),
        capability: capability_for_primitive(primitive),
        demand: demand_for_primitive(primitive),
    })
}

fn plan_field_pattern(
    relation: &PlanRelation,
    field: &FieldPattern,
    slots: &SlotLayout,
) -> Result<ColumnPatternPlan, PlanError> {
    Ok(ColumnPatternPlan {
        field: relation.field(&field.field)?,
        term: plan_term(&field.term, slots)?,
    })
}

fn plan_call_arg(arg: &CallArg, slots: &SlotLayout) -> Result<TermPlan, PlanError> {
    match arg {
        CallArg::Positional { expr, .. } | CallArg::Named { expr, .. } => {
            Ok(TermPlan::Expr(plan_expr(expr, slots)?))
        }
        CallArg::Wildcard { .. } => Ok(TermPlan::Wildcard),
    }
}

fn plan_comparison(comparison: &Comparison, slots: &SlotLayout) -> Result<AtomPlan, PlanError> {
    Ok(AtomPlan::Filter {
        comparison: ComparePlan {
            left: plan_expr(&comparison.left, slots)?,
            op: comparison.op,
            right: plan_expr(&comparison.right, slots)?,
            provenance: CompareProvenance {
                location: comparison.location.clone(),
            },
        },
    })
}

fn plan_aggregate(
    aggregate: &Aggregate,
    catalog: &PlanCatalog,
    outer_slots: &SlotLayout,
    query_index: Option<usize>,
) -> Result<AtomPlan, PlanError> {
    let mut inner_vars = BTreeSet::new();
    collect_body_vars(&aggregate.body, &mut inner_vars);
    collect_expr_vars(&aggregate.value, &mut inner_vars);
    collect_expr_vars(&aggregate.result, &mut inner_vars);
    for arg in &aggregate.args {
        collect_expr_vars(&arg.expr, &mut inner_vars);
    }
    inner_vars.extend(outer_slots.vars().iter().cloned());
    let inner_slots = SlotLayout::from_vars(inner_vars);
    let outer_to_inner_slots = outer_slots
        .vars()
        .iter()
        .filter_map(|var| {
            let outer = outer_slots.slot(var).ok()?;
            let inner = inner_slots.slot(var).ok()?;
            Some((outer, inner))
        })
        .collect::<Vec<_>>();
    let synthetic_rank_slot = aggregate_rank_var(aggregate)
        .map(|var| inner_slots.slot(var))
        .transpose()?;
    let args = plan_aggregate_args(aggregate, &inner_slots, synthetic_rank_slot)?;
    let value = plan_expr(&aggregate.value, &inner_slots)?;
    let result = plan_expr(&aggregate.result, outer_slots).or_else(|_| {
        // Rank injects a synthetic variable inside the aggregate body before the
        // value/result expression is evaluated. Keep that slot explicit in the
        // aggregate node so execution can read it without reconstructing scope.
        plan_expr(&aggregate.result, &inner_slots)
    })?;
    let mut result_slots = BTreeSet::new();
    collect_expr_output_slots(&result, &mut result_slots);
    let mut aggregate_vars = BTreeSet::new();
    collect_body_vars(&aggregate.body, &mut aggregate_vars);
    collect_expr_vars(&aggregate.value, &mut aggregate_vars);
    collect_expr_vars(&aggregate.result, &mut aggregate_vars);
    for arg in &aggregate.args {
        collect_expr_vars(&arg.expr, &mut aggregate_vars);
    }
    let outer_slot_set = aggregate_vars
        .into_iter()
        .filter(|var| outer_slots.vars.contains_key(var))
        .map(|var| outer_slots.slot(&var))
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(AtomPlan::Aggregate(Box::new(AggregatePlan {
        function: aggregate.function,
        provenance: AggregateProvenance {
            function: aggregate.function,
            location: aggregate.location.clone(),
        },
        inner: Box::new(plan_body(
            &aggregate.body,
            catalog,
            &inner_slots,
            query_index,
        )?),
        inner_slots,
        outer_to_inner_slots,
        outer_slots: outer_slot_set.into_iter().collect(),
        args,
        value,
        result,
        result_slots: result_slots.into_iter().collect(),
    })))
}

fn plan_aggregate_args(
    aggregate: &Aggregate,
    slots: &SlotLayout,
    synthetic_rank_slot: Option<SlotId>,
) -> Result<AggregateArgsPlan, PlanError> {
    let mut args = AggregateArgsPlan {
        synthetic_rank_slot,
        ..AggregateArgsPlan::default()
    };
    for arg in &aggregate.args {
        let expr = plan_expr(&arg.expr, slots)?;
        match arg.name.as_str() {
            "k" => args.k = Some(expr),
            "budget" => args.budget = Some(expr),
            "sum" => args.sum = Some(expr),
            "key" => args.key = Some(expr),
            _ => {}
        }
    }
    args.error = aggregate_arg_error(aggregate, &args);
    Ok(args)
}

fn aggregate_arg_error(
    aggregate: &Aggregate,
    args: &AggregateArgsPlan,
) -> Option<AggregateArgPlanError> {
    if let Some(error) = aggregate_invalid_argument(aggregate) {
        return Some(error);
    }
    aggregate_allowed_args(aggregate.function)
        .iter()
        .find(|arg| !args.has(**arg))
        .copied()
        .map(AggregateArgPlanError::Missing)
}

impl AggregateArgsPlan {
    fn has(&self, name: AggregateArgName) -> bool {
        match name {
            AggregateArgName::K => self.k.is_some(),
            AggregateArgName::Budget => self.budget.is_some(),
            AggregateArgName::Sum => self.sum.is_some(),
            AggregateArgName::Key => self.key.is_some(),
            AggregateArgName::Rank => self.synthetic_rank_slot.is_some(),
        }
    }
}

fn aggregate_invalid_argument(aggregate: &Aggregate) -> Option<AggregateArgPlanError> {
    let allowed = aggregate_allowed_args(aggregate.function);
    let mut seen = BTreeSet::new();
    for arg in &aggregate.args {
        let Some(name) = AggregateArgName::parse(arg.name.as_str()) else {
            return Some(AggregateArgPlanError::Unknown);
        };
        if !allowed.contains(&name) {
            return Some(AggregateArgPlanError::Unknown);
        }
        if !seen.insert(name) {
            return Some(AggregateArgPlanError::Duplicate);
        }
    }
    None
}

fn plan_negation(
    negation: &crate::runtime::ast::Negation,
    catalog: &PlanCatalog,
    slots: &SlotLayout,
    query_index: Option<usize>,
) -> Result<AtomPlan, PlanError> {
    let atom = match &negation.atom {
        NegatedAtom::Stored(stored) => Atom::Stored(stored.clone()),
        NegatedAtom::Derived(derived) => Atom::Derived(derived.clone()),
    };
    let mut bound_inputs = BTreeSet::new();
    collect_atom_input_slots(&atom, slots, &mut bound_inputs)?;
    let planned_atom = match &negation.atom {
        NegatedAtom::Stored(stored) => plan_stored_atom(stored, catalog, slots)?,
        NegatedAtom::Derived(derived) => {
            let relation = catalog
                .predicate_relation_in_scope(&derived.predicate, query_index)
                .ok_or_else(|| PlanError::UnknownPredicate {
                    predicate: derived.predicate.clone(),
                })?;
            match relation.kind {
                PlanRelationKind::Primitive { primitive, .. } => plan_primitive_atom(
                    derived.predicate.clone(),
                    primitive,
                    relation,
                    &derived.args,
                    slots,
                )?,
                PlanRelationKind::Derived => plan_derived_scan(relation, &derived.args, slots)?,
                PlanRelationKind::Stored => {
                    return Err(PlanError::UnknownPredicate {
                        predicate: derived.predicate.clone(),
                    });
                }
            }
        }
    };
    let location = match &negation.atom {
        NegatedAtom::Stored(stored) => stored.location.clone(),
        NegatedAtom::Derived(derived) => derived.location.clone(),
    };
    Ok(AtomPlan::Negation {
        inner: Box::new(RuleBodyPlan {
            atoms: vec![planned_atom],
            execution_atoms: vec![0],
        }),
        bound_inputs: bound_inputs.into_iter().collect(),
        provenance: NegationProvenance { location },
    })
}

fn plan_time_scope(
    time_block: &TimeBlock,
    catalog: &PlanCatalog,
    slots: &SlotLayout,
    query_index: Option<usize>,
) -> Result<AtomPlan, PlanError> {
    let mut outer_slots = BTreeSet::new();
    let mut scoped_vars = BTreeSet::new();
    collect_body_vars(&time_block.body, &mut scoped_vars);
    for var in scoped_vars {
        outer_slots.insert(slots.slot(&var)?);
    }
    let inner = plan_body(&time_block.body, catalog, slots, query_index)?;
    let unsupported = time_scope_unsupported_atom(&time_block.reference, &inner, catalog);
    Ok(AtomPlan::TimeScope {
        reference: time_block.reference.clone(),
        inner: Box::new(inner),
        outer_slots: outer_slots.into_iter().collect(),
        provenance: TimeScopeProvenance {
            reference: time_block.reference.clone(),
            location: time_block.location.clone(),
            unsupported,
        },
    })
}

fn plan_term(term: &Term, slots: &SlotLayout) -> Result<TermPlan, PlanError> {
    match term {
        Term::Wildcard => Ok(TermPlan::Wildcard),
        Term::Expr(expr) => Ok(TermPlan::Expr(plan_expr(expr, slots)?)),
    }
}

fn plan_expr(expr: &Expr, slots: &SlotLayout) -> Result<ExprPlan, PlanError> {
    match expr {
        Expr::Var(var) => Ok(ExprPlan::Slot(slots.slot(var)?)),
        Expr::Literal(literal) => Ok(ExprPlan::Literal(plan_literal(literal))),
        Expr::Binary { left, op, right } => Ok(ExprPlan::Binary {
            left: Box::new(plan_expr(left, slots)?),
            op: *op,
            right: Box::new(plan_expr(right, slots)?),
        }),
        Expr::Tuple(items) => items
            .iter()
            .map(|item| plan_expr(item, slots))
            .collect::<Result<Vec<_>, _>>()
            .map(ExprPlan::Tuple),
        Expr::FunctionCall { .. } => Err(PlanError::UnsupportedExpression),
    }
}

fn plan_literal(literal: &Literal) -> LiteralPlan {
    match literal {
        Literal::String(value) => LiteralPlan::String(value.clone()),
        Literal::Number(value) => LiteralPlan::Number(value.clone()),
        Literal::Bool(value) => LiteralPlan::Bool(*value),
        Literal::Null => LiteralPlan::Null,
        Literal::List(items) => LiteralPlan::List(items.iter().map(plan_literal).collect()),
    }
}

fn fields_from_signature(signature: AnalyzedPredicateSignature) -> Vec<Ident> {
    match signature.parameters {
        AnalyzedParameterNames::Named(fields) => fields,
        AnalyzedParameterNames::Unknown | AnalyzedParameterNames::Ambiguous => (0..signature.arity)
            .map(|idx| Ident::new_unchecked(format!("arg{idx}")))
            .collect(),
    }
}

fn head_parameters(head: &Head) -> AnalyzedParameterNames {
    let mut names = Vec::with_capacity(head.terms.len());
    let mut seen = BTreeSet::new();
    for term in &head.terms {
        let Term::Expr(Expr::Var(var)) = term else {
            return AnalyzedParameterNames::Unknown;
        };
        if !seen.insert(var.clone()) {
            return AnalyzedParameterNames::Ambiguous;
        }
        names.push(var.clone());
    }
    AnalyzedParameterNames::Named(names)
}

fn recursive_atom_predicates(
    body: &Body,
    stage_predicates: &BTreeSet<PredicateRef>,
) -> Vec<(usize, PredicateRef)> {
    body.atoms
        .iter()
        .enumerate()
        .filter_map(|(idx, atom)| match atom {
            Atom::Derived(derived) if stage_predicates.contains(&derived.predicate) => {
                Some((idx, derived.predicate.clone()))
            }
            _ => None,
        })
        .collect()
}

fn provider_for_primitive(primitive: PrimitivePredicate) -> ProviderRef {
    match primitive {
        PrimitivePredicate::Search => ProviderRef::Search,
        PrimitivePredicate::Read | PrimitivePredicate::ReadFull | PrimitivePredicate::Match => {
            ProviderRef::Content
        }
        PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => ProviderRef::Introspection,
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::GitMtime
        | PrimitivePredicate::ChangedWithin
        | PrimitivePredicate::TokenEstimate => ProviderRef::Graph,
    }
}

fn capability_for_primitive(primitive: PrimitivePredicate) -> CapabilityAction {
    match primitive {
        PrimitivePredicate::Search => CapabilityAction::Search,
        PrimitivePredicate::Read => CapabilityAction::Read,
        PrimitivePredicate::ReadFull => CapabilityAction::ReadFull,
        PrimitivePredicate::Match => CapabilityAction::Match,
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::GitMtime
        | PrimitivePredicate::ChangedWithin
        | PrimitivePredicate::TokenEstimate
        | PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => CapabilityAction::None,
    }
}

fn demand_for_primitive(primitive: PrimitivePredicate) -> DemandPolicy {
    match primitive {
        PrimitivePredicate::Search
        | PrimitivePredicate::Read
        | PrimitivePredicate::ReadFull
        | PrimitivePredicate::Match
        | PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => DemandPolicy::Lazy,
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::GitMtime
        | PrimitivePredicate::ChangedWithin
        | PrimitivePredicate::TokenEstimate => DemandPolicy::Eager,
    }
}

fn collect_head_vars(head: &Head, out: &mut BTreeSet<Ident>) {
    for term in &head.terms {
        if let Term::Expr(expr) = term {
            collect_expr_vars(expr, out);
        }
    }
}

fn collect_body_vars(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        collect_atom_vars(atom, out);
    }
}

fn collect_rule_body_vars(body: &Body, out: &mut BTreeSet<Ident>) {
    for atom in &body.atoms {
        match atom {
            Atom::Aggregation(aggregate) => {
                collect_expr_vars(&aggregate.result, out);
            }
            Atom::TimeBlock(time_block) => collect_rule_body_vars(&time_block.body, out),
            _ => collect_atom_vars(atom, out),
        }
    }
}

fn collect_atom_vars(atom: &Atom, out: &mut BTreeSet<Ident>) {
    match atom {
        Atom::Stored(stored) => {
            for field in &stored.fields {
                if let Term::Expr(expr) = &field.term {
                    collect_expr_vars(expr, out);
                }
            }
        }
        Atom::Derived(derived) => {
            for arg in &derived.args {
                if let Some(expr) = arg.expr() {
                    collect_expr_vars(expr, out);
                }
            }
        }
        Atom::Comparison(comparison) => {
            collect_expr_vars(&comparison.left, out);
            collect_expr_vars(&comparison.right, out);
        }
        Atom::Aggregation(aggregate) => {
            collect_expr_vars(&aggregate.result, out);
            collect_expr_vars(&aggregate.value, out);
            for arg in &aggregate.args {
                collect_expr_vars(&arg.expr, out);
            }
            collect_body_vars(&aggregate.body, out);
        }
        Atom::Negation(negation) => match &negation.atom {
            NegatedAtom::Stored(stored) => {
                for field in &stored.fields {
                    if let Term::Expr(expr) = &field.term {
                        collect_expr_vars(expr, out);
                    }
                }
            }
            NegatedAtom::Derived(derived) => {
                for arg in &derived.args {
                    if let Some(expr) = arg.expr() {
                        collect_expr_vars(expr, out);
                    }
                }
            }
        },
        Atom::TimeBlock(time_block) => collect_body_vars(&time_block.body, out),
    }
}

fn collect_expr_vars(expr: &Expr, out: &mut BTreeSet<Ident>) {
    expr.variables(out);
}

fn collect_atom_input_slots(
    atom: &Atom,
    slots: &SlotLayout,
    out: &mut BTreeSet<SlotId>,
) -> Result<(), PlanError> {
    let mut vars = BTreeSet::new();
    match atom {
        Atom::Stored(stored) => {
            for field in &stored.fields {
                if let Term::Expr(expr) = &field.term {
                    collect_expr_vars(expr, &mut vars);
                }
            }
        }
        Atom::Derived(derived) => {
            for arg in &derived.args {
                if let Some(expr) = arg.expr() {
                    collect_expr_vars(expr, &mut vars);
                }
            }
        }
        Atom::Comparison(comparison) => {
            collect_expr_vars(&comparison.left, &mut vars);
            collect_expr_vars(&comparison.right, &mut vars);
        }
        Atom::Aggregation(aggregate) => {
            collect_expr_vars(&aggregate.result, &mut vars);
            collect_expr_vars(&aggregate.value, &mut vars);
        }
        Atom::Negation(_) => {}
        Atom::TimeBlock(time_block) => collect_body_vars(&time_block.body, &mut vars),
    }
    for var in vars {
        out.insert(slots.slot(&var)?);
    }
    Ok(())
}

fn collect_term_input_slots(term: &TermPlan, out: &mut BTreeSet<SlotId>) {
    if let TermPlan::Expr(expr) = term {
        collect_expr_input_slots(expr, out);
    }
}

fn collect_expr_input_slots(expr: &ExprPlan, out: &mut BTreeSet<SlotId>) {
    match expr {
        ExprPlan::Slot(slot) => {
            out.insert(*slot);
        }
        ExprPlan::Literal(_) => {}
        ExprPlan::Binary { left, right, .. } => {
            collect_expr_input_slots(left, out);
            collect_expr_input_slots(right, out);
        }
        ExprPlan::Tuple(items) => {
            for item in items {
                collect_expr_input_slots(item, out);
            }
        }
    }
}

fn collect_term_output_slots(term: &TermPlan, out: &mut BTreeSet<SlotId>) {
    if let TermPlan::Expr(expr) = term {
        collect_expr_output_slots(expr, out);
    }
}

fn collect_expr_output_slots(expr: &ExprPlan, out: &mut BTreeSet<SlotId>) {
    match expr {
        ExprPlan::Slot(slot) => {
            out.insert(*slot);
        }
        ExprPlan::Literal(_) => {}
        ExprPlan::Binary { left, right, .. } => {
            collect_expr_output_slots(left, out);
            collect_expr_output_slots(right, out);
        }
        ExprPlan::Tuple(items) => {
            for item in items {
                collect_expr_output_slots(item, out);
            }
        }
    }
}

fn aggregate_rank_var(aggregate: &Aggregate) -> Option<&Ident> {
    if aggregate.function != AggregateFunction::Rank {
        return None;
    }
    aggregate
        .args
        .iter()
        .find(|arg| arg.name.as_str() == "rank")
        .and_then(|arg| match &arg.expr {
            Expr::Var(var) => Some(var),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_lexical_search_info;
    use crate::project::{load_project_extension, merge_program_layers};
    use crate::runtime::analysis::analyze;
    use crate::runtime::parser::{parse_prelude_program, parse_program};
    use crate::runtime::prelude::PreludeSet;
    use crate::source::{ConfigKey, Pattern, SourceCapabilities, SourceInfo};

    fn analyzed(source: &str) -> AnalyzedProgram {
        analyze(parse_program("plan-test", source).expect("program parses"))
            .expect("program analyzes")
    }

    fn markdown_source_info() -> SourceInfo {
        SourceInfo {
            name: "markdown",
            recognizes: vec![Pattern::new("**/*.md")],
            doc: "Test markdown source declaration.",
            config_keys: vec![
                ConfigKey::required_exact("md.file_extension", 1),
                ConfigKey::required_exact("md.scan_root", 1),
                ConfigKey::optional_at_least("md.scan_exclude", 1),
                ConfigKey::optional_exact("md.label_pattern", 3),
                ConfigKey::optional_exact("md.linear_namespace", 1),
                ConfigKey::optional_exact("md.version_pattern", 2),
                ConfigKey::optional_exact("md.section_min_depth", 1),
                ConfigKey::optional_exact("md.section_max_depth", 1),
            ],
            capabilities: SourceCapabilities {
                supports_git_ref: false,
                supports_time_snapshot: false,
                supports_incremental: false,
                live_only: false,
            },
            search: Some(default_lexical_search_info()),
        }
    }

    #[test]
    fn plan_catalog_registers_derived_local_and_primitive_schemas() {
        let analyzed = analyzed(
            r#"
            @predicate(name: "risk", args: ["h", "score"]).
            risk(h, 1) := *handle{id: h}.
            ? where local_item(h) := risk(h, score). local_item(h).
            "#,
        );

        let planned = plan(&analyzed).expect("program plans");

        let risk = PredicateRef::parse("risk").expect("predicate");
        let local = PredicateRef::parse("local_item").expect("predicate");
        let search = PredicateRef::parse("search").expect("predicate");
        assert_eq!(
            planned
                .catalog
                .predicate_relation(&risk)
                .expect("risk relation")
                .fields()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["h", "score"]
        );
        assert!(matches!(
            planned
                .catalog
                .predicate_relation_in_scope(&local, Some(0))
                .expect("local relation")
                .kind,
            PlanRelationKind::Derived
        ));
        assert!(matches!(
            planned
                .catalog
                .predicate_relation(&search)
                .expect("search primitive")
                .kind,
            PlanRelationKind::Primitive { .. }
        ));
    }

    #[test]
    fn query_local_predicates_are_scoped_per_query() {
        let analyzed = analyzed(
            r"
            ? where local_item(h) := *handle{id: h}. local_item(h).
            ? where local_item(h, status) := *handle{id: h, status: status}. local_item(h, status).
            ",
        );

        let planned = plan(&analyzed).expect("program plans");
        let local = PredicateRef::parse("local_item").expect("predicate");
        let first = planned
            .catalog
            .predicate_relation_in_scope(&local, Some(0))
            .expect("first local relation");
        let second = planned
            .catalog
            .predicate_relation_in_scope(&local, Some(1))
            .expect("second local relation");

        assert_ne!(first.id, second.id);
        assert_eq!(first.fields().len(), 1);
        assert_eq!(second.fields().len(), 2);
        assert!(planned.catalog.predicate_relation(&local).is_none());
    }

    #[test]
    fn query_local_stages_use_the_migration_certificate() {
        let analyzed = analyzed(
            r"
            ?
              where local_item(h) := *handle{id: h}, active(h).
              local_item(h) order by h asc.
            ",
        );

        let planned = plan(&analyzed).expect("program plans");
        let query = planned.queries.first().expect("query planned");
        let local = query.plan.strata.first().expect("local stratum planned");
        let output = query.plan.strata.last().expect("output stratum planned");

        assert!(
            local.authoritative_planned,
            "query-local rule stage should be certified planned"
        );
        assert!(
            local
                .stages
                .iter()
                .all(|stage| stage.migration.is_planned()),
            "query-local stage reasons should be empty for supported atoms"
        );
        assert!(
            output.authoritative_planned,
            "final query body is a planned projection stage"
        );
    }

    #[test]
    fn soft_primitive_override_resolves_as_derived_not_primitive() {
        let active = PredicateRef::parse("active").expect("predicate");
        let analyzed = analyzed(
            r#"
            active(h) := *handle{id: h, status: "draft"}.
            open(h) := active(h).
            "#,
        );

        let planned = plan(&analyzed).expect("program plans");

        let relation = planned
            .catalog
            .predicate_relation(&active)
            .expect("active relation");
        assert_eq!(relation.kind, PlanRelationKind::Derived);
        let open = PredicateRef::parse("open").expect("predicate");
        let open_relation = planned
            .catalog
            .predicate_relation(&open)
            .expect("open relation")
            .id;
        let open_rule = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .find(|rule| rule.head == Some(open_relation))
            .expect("open rule is planned");
        assert!(open_rule.body.atoms.iter().any(|atom| {
            matches!(
                atom,
                AtomPlan::Scan {
                    relation: id,
                    ..
                } if *id == relation.id
            )
        }));
    }

    #[test]
    fn migration_target_is_a_plan_property() {
        let analyzed = analyzed(
            r"
            incoming_edge(h, src, kind) := *edge{to: h, from: src, kind: kind}.
            ",
        );

        let planned = plan(&analyzed).expect("program plans");
        let incoming = PredicateRef::parse("incoming_edge").expect("predicate");
        let incoming_relation = planned
            .catalog
            .predicate_relation(&incoming)
            .expect("incoming_edge relation")
            .id;

        let incoming_rule = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .find(|rule| rule.head == Some(incoming_relation))
            .expect("incoming_edge rule");
        assert!(planned_rule_group_executable(
            incoming_rule,
            &planned.catalog
        ));
        let migration = predicate_migration(&planned, &incoming).expect("incoming_edge stage");
        assert_eq!(migration.mode, StageMigrationMode::Planned);
        assert!(migration.reasons.is_empty());
    }

    #[test]
    fn stages_topologically_order_positive_dependencies() {
        let analyzed = analyzed(
            r#"
            source(h) := *handle{id: h}.
            diagnostic("T001", "error", h, file, line, h) :=
              source(h),
              *handle{id: h, file: file, line: line}.
            entropy(h, "broken_ref") :=
              diagnostic("T001", severity, h, file, line, evidence).
            "#,
        );

        let planned = plan(&analyzed).expect("program plans");
        let source = PredicateRef::parse("source").expect("predicate");
        let diagnostic = PredicateRef::parse("diagnostic").expect("predicate");
        let entropy = PredicateRef::parse("entropy").expect("predicate");
        let source_stage = predicate_stage(&planned, &source).expect("source stage");
        let diagnostic_stage = predicate_stage(&planned, &diagnostic).expect("diagnostic stage");
        let entropy_stage = predicate_stage(&planned, &entropy).expect("entropy stage");

        assert!(source_stage < diagnostic_stage);
        assert!(diagnostic_stage < entropy_stage);
    }

    #[test]
    fn positive_dependency_components_preserve_deterministic_scc_order() {
        let predicate = |name| PredicateRef::parse(name).expect("predicate parses");
        let alpha = predicate("a");
        let beta = predicate("b");
        let gamma = predicate("c");
        let delta = predicate("d");
        let epsilon = predicate("e");
        let predicates = [
            alpha.clone(),
            beta.clone(),
            gamma.clone(),
            delta.clone(),
            epsilon.clone(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let dependencies = BTreeMap::from([
            (alpha.clone(), BTreeSet::from([beta.clone()])),
            (beta.clone(), BTreeSet::from([alpha.clone(), gamma.clone()])),
            (gamma.clone(), BTreeSet::from([gamma.clone()])),
            (
                delta.clone(),
                BTreeSet::from([gamma.clone(), epsilon.clone()]),
            ),
            (epsilon.clone(), BTreeSet::from([delta.clone()])),
        ]);

        assert_eq!(
            positive_dependency_components(&predicates, &dependencies),
            vec![
                BTreeSet::from([alpha, beta]),
                BTreeSet::from([gamma]),
                BTreeSet::from([delta, epsilon]),
            ]
        );
    }

    #[test]
    fn positive_dependency_components_match_mutual_reachability() {
        let predicates = ["a", "b", "c"]
            .map(|name| PredicateRef::parse(name).expect("predicate parses"))
            .into_iter()
            .collect::<BTreeSet<_>>();
        let predicate_list = predicates.iter().cloned().collect::<Vec<_>>();

        for edge_mask in 0_u16..(1 << 9) {
            let mut dependencies = BTreeMap::<_, BTreeSet<_>>::new();
            for (edge, (source, target)) in predicate_list
                .iter()
                .flat_map(|source| predicate_list.iter().map(move |target| (source, target)))
                .enumerate()
            {
                if edge_mask & (1 << edge) != 0 {
                    dependencies
                        .entry(source.clone())
                        .or_default()
                        .insert(target.clone());
                }
            }

            assert_eq!(
                positive_dependency_components(&predicates, &dependencies),
                mutual_reachability_components(&predicates, &dependencies),
                "edge mask {edge_mask:#011b}"
            );
        }
    }

    fn mutual_reachability_components(
        predicates: &BTreeSet<PredicateRef>,
        dependencies: &BTreeMap<PredicateRef, BTreeSet<PredicateRef>>,
    ) -> Vec<BTreeSet<PredicateRef>> {
        let reachable = |start: &PredicateRef| {
            let mut seen = BTreeSet::new();
            let mut pending = vec![start.clone()];
            while let Some(predicate) = pending.pop() {
                if seen.insert(predicate.clone())
                    && let Some(next) = dependencies.get(&predicate)
                {
                    pending.extend(
                        next.iter()
                            .filter(|candidate| predicates.contains(*candidate))
                            .cloned(),
                    );
                }
            }
            seen
        };

        let mut remaining = predicates.clone();
        let mut components = Vec::new();
        while let Some(seed) = remaining.iter().next().cloned() {
            let from_seed = reachable(&seed);
            let component = remaining
                .iter()
                .filter(|predicate| {
                    from_seed.contains(*predicate) && reachable(predicate).contains(&seed)
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            remaining.retain(|predicate| !component.contains(predicate));
            components.push(component);
        }
        components
    }

    #[test]
    fn aggregate_time_scope_and_negation_lower_with_slots() {
        let analyzed = analyzed(
            r#"
            lonely(h) := *handle{id: h}, not *edge{from: h}.
            prior(h, status) := at("snapshot:last") { *handle{id: h, status: status} }.
            ranked(h, rank) :=
              (h, rank) = Rank{ key: score, rank: rank :
                h : *handle{id: h}, in_degree(h, score)
              }.
            "#,
        );

        let planned = plan(&analyzed).expect("program plans");
        let atoms = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .flat_map(|rule| &rule.body.atoms)
            .collect::<Vec<_>>();
        let ranked = PredicateRef::parse("ranked").expect("predicate");
        let ranked_relation = planned
            .catalog
            .predicate_relation(&ranked)
            .expect("ranked relation")
            .id;
        let ranked_rule = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .find(|rule| rule.head == Some(ranked_relation))
            .expect("ranked rule is planned");

        assert!(atoms.iter().any(|atom| matches!(atom, AtomPlan::Negation { bound_inputs, .. } if !bound_inputs.is_empty())));
        assert!(atoms.iter().any(|atom| matches!(atom, AtomPlan::TimeScope { reference, outer_slots, .. } if reference == "snapshot:last" && !outer_slots.is_empty())));
        assert!(atoms.iter().any(|atom| matches!(atom, AtomPlan::Aggregate(aggregate) if aggregate.args.synthetic_rank_slot.is_some() && !aggregate.outer_slots.is_empty())));
        assert!(
            !ranked_rule
                .slots
                .vars()
                .contains(&Ident::new_unchecked("score")),
            "aggregate-local variables must not leak into parent slots"
        );
    }

    #[test]
    fn primitive_slots_distinguish_required_inputs_and_outputs() {
        let analyzed = analyzed(
            r#"
            query("load shedding").
            found(h, score) :=
              query(q),
              search(q, h, span, score, reason, field, low_confidence).
            "#,
        );

        let planned = plan(&analyzed).expect("program plans");
        let search_atom = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .flat_map(|rule| &rule.body.atoms)
            .find_map(|atom| match atom {
                AtomPlan::PrimitiveCall {
                    primitive: PrimitivePredicate::Search,
                    input_slots,
                    output_slots,
                    ..
                } => Some((input_slots, output_slots)),
                _ => None,
            })
            .expect("search primitive is planned");

        assert_eq!(search_atom.0.len(), 1);
        assert!(!search_atom.1.is_empty());
        assert!(
            search_atom.1.len() > search_atom.0.len(),
            "non-required search fields should be modeled as candidate outputs"
        );
    }

    #[test]
    fn standard_prelude_plans_without_executing() {
        let prelude = PreludeSet::standard()
            .program()
            .expect("checked-in prelude parses");
        let analyzed = analyze(prelude).expect("checked-in prelude analyzes");

        let planned = plan(&analyzed).expect("checked-in prelude plans");

        assert!(!planned.global.strata.is_empty());
        assert!(
            planned
                .catalog
                .relations()
                .iter()
                .any(|relation| relation.name == "recent_frontier")
        );
        let diagnostic = PredicateRef::parse("diagnostic").expect("predicate parses");
        let entropy = PredicateRef::parse("entropy").expect("predicate parses");
        let diagnostic_stage = predicate_stage(&planned, &diagnostic).expect("diagnostic stage");
        let entropy_stage = predicate_stage(&planned, &entropy).expect("entropy stage");
        assert!(
            diagnostic_stage < entropy_stage,
            "entropy must run after diagnostic in the planned positive-DAG stage order"
        );
        assert!(
            planned
                .global
                .strata
                .iter()
                .flat_map(|stratum| &stratum.stages)
                .any(|stage| {
                    stage.authoritative_planned
                        && stage
                            .authoritative_predicates
                            .iter()
                            .any(|predicate| predicate.display_name() == "entropy")
                        && stage
                            .predicates
                            .iter()
                            .any(|predicate| predicate.display_name() == "entropy")
                }),
            "entropy should be marked as a stage-level authoritative target"
        );
    }

    #[test]
    fn standard_prelude_migration_policy_explains_planned_and_excluded_stages() {
        let prelude = PreludeSet::standard()
            .program()
            .expect("checked-in prelude parses");
        let analyzed = analyze(prelude).expect("checked-in prelude analyzes");
        let planned = plan(&analyzed).expect("checked-in prelude plans");

        let entropy = PredicateRef::parse("entropy").expect("predicate parses");
        let entropy_migration = predicate_migration(&planned, &entropy).expect("entropy stage");
        assert_eq!(entropy_migration.mode, StageMigrationMode::Planned);
        assert!(entropy_migration.reasons.is_empty());

        let potential = PredicateRef::parse("potential").expect("predicate parses");
        let potential_migration =
            predicate_migration(&planned, &potential).expect("potential stage");
        assert_eq!(potential_migration.mode, StageMigrationMode::Planned);
        assert!(
            potential_migration.reasons.is_empty(),
            "supported scalar aggregates should clear the certificate reasons"
        );

        for predicate in [
            "primary_entropy",
            "holding",
            "regressed",
            "re_opened",
            "recently_advanced",
            "previous_status_population",
        ] {
            let predicate = PredicateRef::parse(predicate).expect("predicate parses");
            let migration = predicate_migration(&planned, &predicate).expect("predicate stage");
            assert_eq!(migration.mode, StageMigrationMode::Planned);
            assert!(
                migration.reasons.is_empty(),
                "supported time scopes should clear the certificate reasons"
            );
        }
    }

    #[test]
    fn aggregate_argument_errors_stay_planned_and_report_at_eval() {
        let program = parse_prelude_program(
            "<aggregate-arg-certificate>",
            r#"
            amount("a", 1).
            invalid_unknown(h, score) :=
              (h, score) = TopK{ k: 1, bogus: 2, key: score : (h, score) : amount(h, score) }.
            invalid_duplicate(h, score) :=
              (h, score) = TopK{ k: 1, k: 2, key: score : (h, score) : amount(h, score) }.
            invalid_missing(h, score) :=
              (h, score) = TopK{ k: 1 : (h, score) : amount(h, score) }.
            ? invalid_unknown(h, score).
            ? invalid_duplicate(h, score).
            ? invalid_missing(h, score).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");

        for predicate in ["invalid_unknown", "invalid_duplicate", "invalid_missing"] {
            let predicate = PredicateRef::parse(predicate).expect("predicate parses");
            let migration = predicate_migration(&planned, &predicate).expect("predicate stage");
            assert_eq!(migration.mode, StageMigrationMode::Planned);
            assert!(migration.reasons.is_empty());
        }

        let cases = [
            ("invalid_unknown", AggregateArgPlanError::Unknown),
            ("invalid_duplicate", AggregateArgPlanError::Duplicate),
            (
                "invalid_missing",
                AggregateArgPlanError::Missing(AggregateArgName::Key),
            ),
        ];
        for (predicate, expected) in cases {
            let predicate = PredicateRef::parse(predicate).expect("predicate parses");
            let aggregate = predicate_aggregate(&planned, &predicate).expect("aggregate plan");
            assert_eq!(aggregate.args.error, Some(expected));
        }
    }

    #[test]
    fn unsupported_time_scope_shape_stays_unplanned_by_certificate() {
        let program = parse_prelude_program(
            "<time-scope-certificate>",
            r#"
            unsupported_stored(from, to) :=
              at("snapshot:last") { *edge{from: from, to: to} }.
            unsupported_primitive(h) :=
              at("snapshot:last") { upstream(h, "x") }.
            inner(h) := *handle{id: h}.
            unsupported_derived(h) :=
              at("snapshot:last") { inner(h) }.
            ? unsupported_stored(from, to).
            ? unsupported_primitive(h).
            ? unsupported_derived(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");

        for predicate in [
            "unsupported_stored",
            "unsupported_primitive",
            "unsupported_derived",
        ] {
            let predicate = PredicateRef::parse(predicate).expect("predicate parses");
            let migration = predicate_migration(&planned, &predicate).expect("predicate stage");
            assert_eq!(migration.mode, StageMigrationMode::Unsupported);
            assert!(
                migration.reasons.contains(&UnsupportedReason::TimeScope),
                "unsupported time-scope shape must keep the stage unplanned"
            );
        }
    }

    #[test]
    fn all_prelude_files_plan_individually_with_standard_context() {
        let mut statements = PreludeSet::standard()
            .program()
            .expect("checked-in prelude parses")
            .statements;
        let source = r#"
            ? recent_frontier(h, rank, recency) order by rank asc.
            ? ranked_anchor(h, rank, score, why) order by score desc.
            ? diagnostic(code, severity, subject, file, line, evidence).
            ? at("snapshot:last") { *handle{id: h, status: status} }.
        "#;
        let query_program =
            parse_prelude_program("<planning-test>", source).expect("query battery parses");
        statements.extend(query_program.statements);
        let analyzed = analyze(crate::runtime::ast::Program::new(statements))
            .expect("prelude query battery analyzes");

        let planned = plan(&analyzed).expect("prelude query battery plans");

        assert_eq!(planned.queries.len(), 4);
    }

    #[test]
    fn external_corpus_query_battery_plans_when_configured() {
        let Ok(root) = std::env::var("ANNEAL_SMOKE_CORPUS_ROOT") else {
            eprintln!("skipping external planning smoke; ANNEAL_SMOKE_CORPUS_ROOT is unset");
            return;
        };
        let root = std::path::Path::new(&root);
        let prelude = PreludeSet::standard()
            .program()
            .expect("checked-in prelude parses");
        let extension = load_project_extension(root, &[markdown_source_info()], &prelude)
            .expect("project extension loads");
        let (mut program, _warnings) = merge_program_layers(prelude, extension.program().clone());
        let query_program = parse_prelude_program(
            "<external-planning-smoke>",
            r#"
            ? diagnostic(code, severity, subject, file, line, evidence).
            ? frontier(h, energy).
            ? blocker(h, energy, source).
            ? flow(h, state).
            ? potential(h, energy).
            ? holding(h).
            ? advancing(h).
            ? drifting(h).
            ? recent_frontier(h, rank, recency) order by rank asc.
            ? ranked_anchor(h, rank, score, why) order by rank asc.
            ? at("snapshot:last") { *handle{id: h, status: status} }.
            "#,
        )
        .expect("external query battery parses");
        program.statements.extend(query_program.statements);

        let analyzed = analyze(program).expect("external query battery analyzes");
        let planned = plan(&analyzed).expect("external query battery plans");

        assert_eq!(planned.queries.len(), 11);
    }

    fn predicate_stage(planned: &ProgramPlan, predicate: &PredicateRef) -> Option<(usize, usize)> {
        let relation = planned.catalog.predicate_relation(predicate)?.id;
        planned
            .global
            .strata
            .iter()
            .enumerate()
            .find_map(|(stratum_index, stratum)| {
                stratum
                    .stages
                    .iter()
                    .enumerate()
                    .find_map(|(stage_index, stage)| {
                        stage
                            .rule_groups
                            .iter()
                            .any(|group_index| {
                                stratum
                                    .rule_groups
                                    .get(*group_index)
                                    .is_some_and(|group| group.head == Some(relation))
                            })
                            .then_some((stratum_index, stage_index))
                    })
            })
    }

    fn predicate_migration<'a>(
        planned: &'a ProgramPlan,
        predicate: &PredicateRef,
    ) -> Option<&'a StageMigration> {
        let (stratum_index, stage_index) = predicate_stage(planned, predicate)?;
        planned
            .global
            .strata
            .get(stratum_index)?
            .stages
            .get(stage_index)
            .map(|stage| &stage.migration)
    }

    fn predicate_aggregate<'a>(
        planned: &'a ProgramPlan,
        predicate: &PredicateRef,
    ) -> Option<&'a AggregatePlan> {
        let relation = planned.catalog.predicate_relation(predicate)?.id;
        planned.global.strata.iter().find_map(|stratum| {
            stratum.rule_groups.iter().find_map(|group| {
                (group.head == Some(relation)).then(|| {
                    group.body.atoms.iter().find_map(|atom| match atom {
                        AtomPlan::Aggregate(aggregate) => Some(aggregate.as_ref()),
                        _ => None,
                    })
                })?
            })
        })
    }
}
