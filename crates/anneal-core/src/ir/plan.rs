//! Planning-only IR for the Plan/IR middle-end.
//!
//! This module lowers an already-analyzed program into relation, field, and slot
//! ids. It intentionally does not execute the plan yet; the old evaluator remains
//! the runtime path while this artifact proves the compiler boundary.

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
    pub(crate) deltas: Vec<DeltaPlan>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleStagePlan {
    pub(crate) rule_groups: Vec<usize>,
    pub(crate) predicates: BTreeSet<PredicateRef>,
    pub(crate) authoritative_predicates: BTreeSet<PredicateRef>,
    pub(crate) authoritative_planned: bool,
    pub(crate) shadow_planned: bool,
    pub(crate) migration: StageMigration,
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

    fn interpreted(reasons: BTreeSet<UnsupportedReason>) -> Self {
        Self {
            mode: StageMigrationMode::Interpreted,
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
    Interpreted,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UnsupportedReason {
    TimeScope,
    UnsupportedAggregate(AggregateFunction),
    InvalidAggregateArgs(AggregateFunction),
    UnsupportedExpression,
    UnsupportedComparison(ComparisonOp),
    MissingProvenance,
    Recursive,
    PositiveCycle,
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
    pub(crate) predicate: RelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleGroupPlan {
    pub(crate) head: Option<RelationId>,
    pub(crate) head_terms: Vec<TermPlan>,
    pub(crate) body: RuleBodyPlan,
    pub(crate) slots: SlotLayout,
    pub(crate) provenance: Option<RuleProvenance>,
    pub(crate) shadow_planned: bool,
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
    pub(crate) args_valid: bool,
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
    #[error("unsupported expression in planning-only artifact")]
    UnsupportedExpression,
    #[error("positive dependency cycle inside planned stratum: {predicates:?}")]
    PositiveCycle { predicates: Vec<PredicateRef> },
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
            let mut deltas = Vec::new();
            for rule in &rules {
                let rule_group = plan_rule(rule, catalog, None)?;
                let rule_group_index = rule_groups.len();
                for atom_index in recursive_atom_indexes(&rule.body, &stratum_predicates) {
                    deltas.push(DeltaPlan {
                        rule_group: rule_group_index,
                        atom_index,
                        predicate: catalog
                            .predicate_relation(&rule.head.predicate)
                            .ok_or_else(|| PlanError::UnknownPredicate {
                                predicate: rule.head.predicate.clone(),
                            })?
                            .id,
                    });
                }
                rule_groups.push(rule_group);
            }
            let stages = plan_rule_stages(&rules, &rule_groups, &stratum_predicates, catalog)?;
            Ok(StratumPlan {
                recursive: !deltas.is_empty(),
                authoritative_planned: stages.iter().any(|stage| stage.authoritative_planned),
                rule_groups,
                stages,
                deltas,
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
            let stages = plan_rule_stages(&rules, &rule_groups, &stratum_predicates, catalog)?;
            Ok(StratumPlan {
                recursive: false,
                authoritative_planned: false,
                rule_groups,
                stages,
                deltas: Vec::new(),
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
    Ok(QueryPlan {
        plan: Plan {
            kind: PlanKind::Query,
            strata: {
                let mut all = strata;
                all.push(StratumPlan {
                    recursive: false,
                    authoritative_planned: false,
                    rule_groups: vec![RuleGroupPlan {
                        head: None,
                        head_terms: Vec::new(),
                        body,
                        slots,
                        provenance: None,
                        shadow_planned: false,
                    }],
                    stages: vec![RuleStagePlan {
                        rule_groups: vec![0],
                        predicates: BTreeSet::new(),
                        authoritative_predicates: BTreeSet::new(),
                        authoritative_planned: false,
                        shadow_planned: false,
                        migration: StageMigration::interpreted(BTreeSet::new()),
                    }],
                    deltas: Vec::new(),
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
        shadow_planned: predicate_is_shadow_planned_target(&rule.head.predicate),
    })
}

const SHADOW_PLANNED_PREDICATES: &[&str] = &["entropy", "incoming_edge"];

fn predicate_is_shadow_planned_target(predicate: &PredicateRef) -> bool {
    SHADOW_PLANNED_PREDICATES.contains(&predicate.display_name().as_str())
}

fn plan_rule_stages(
    rules: &[&Rule],
    rule_groups: &[RuleGroupPlan],
    stratum_predicates: &BTreeSet<PredicateRef>,
    catalog: &PlanCatalog,
) -> Result<Vec<RuleStagePlan>, PlanError> {
    let mut remaining = stratum_predicates.clone();
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

    let mut stages = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|predicate| {
                dependencies
                    .get(*predicate)
                    .is_none_or(|deps| deps.is_disjoint(&remaining))
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if ready.is_empty() {
            return Err(PlanError::PositiveCycle {
                predicates: remaining.into_iter().collect(),
            });
        }

        let rule_group_indexes = ready
            .iter()
            .filter_map(|predicate| groups_by_predicate.get(predicate))
            .flat_map(|groups| groups.iter().copied())
            .collect::<Vec<_>>();
        let migration = stage_migration(&rule_group_indexes, rule_groups, catalog);
        let authoritative_predicates = ready
            .iter()
            .filter(|predicate| {
                groups_by_predicate.contains_key(*predicate) && migration.is_planned()
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let authoritative_planned = !authoritative_predicates.is_empty();
        let shadow_planned = rule_group_indexes.iter().any(|index| {
            rule_groups
                .get(*index)
                .is_some_and(|group| group.shadow_planned)
        });
        stages.push(RuleStagePlan {
            rule_groups: rule_group_indexes,
            predicates: ready.clone(),
            authoritative_predicates,
            authoritative_planned,
            shadow_planned,
            migration,
        });
        for predicate in ready {
            remaining.remove(&predicate);
        }
    }
    Ok(stages)
}

fn stage_migration(
    rule_group_indexes: &[usize],
    rule_groups: &[RuleGroupPlan],
    catalog: &PlanCatalog,
) -> StageMigration {
    let mut reasons = BTreeSet::new();
    for index in rule_group_indexes {
        let Some(group) = rule_groups.get(*index) else {
            reasons.insert(UnsupportedReason::UnsupportedExpression);
            continue;
        };
        reasons.extend(planned_rule_group_unsupported_reasons(group, catalog));
    }
    if reasons.is_empty() {
        StageMigration::planned()
    } else {
        StageMigration::interpreted(reasons)
    }
}

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

pub(crate) fn aggregate_allowed_args(function: AggregateFunction) -> &'static [&'static str] {
    match function {
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::Avg
        | AggregateFunction::List
        | AggregateFunction::Set => &[],
        AggregateFunction::TopK => &["k", "key"],
        AggregateFunction::Rank => &["key", "rank"],
        AggregateFunction::TakeUntil => &["budget", "sum", "key"],
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

pub(crate) fn time_scoped_stored_relation_supported(relation: &Ident) -> bool {
    time_scoped_stored_relation_supported_name(relation.as_str())
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
            if !aggregate.args_valid {
                out.insert(UnsupportedReason::InvalidAggregateArgs(aggregate.function));
            }
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
    let args_valid = aggregate_args_valid(aggregate);
    let value = plan_expr(&aggregate.value, &inner_slots)?;
    let result = plan_expr(&aggregate.result, outer_slots).or_else(|_| {
        // Rank injects a synthetic variable inside the aggregate body before the
        // value/result expression is evaluated. Keep that slot explicit in the
        // aggregate node even though old eval still executes this phase.
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
        args_valid,
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
    Ok(args)
}

fn aggregate_args_valid(aggregate: &Aggregate) -> bool {
    let allowed = aggregate_allowed_args(aggregate.function);
    let mut seen = BTreeSet::new();
    for arg in &aggregate.args {
        let name = arg.name.as_str();
        if !allowed.contains(&name) || !seen.insert(name) {
            return false;
        }
    }
    match aggregate.function {
        AggregateFunction::TopK => seen.contains("k") && seen.contains("key"),
        AggregateFunction::Rank => {
            seen.contains("key")
                && aggregate_rank_var(aggregate).is_some()
                && aggregate.args.iter().any(|arg| arg.name.as_str() == "rank")
        }
        AggregateFunction::TakeUntil => {
            seen.contains("budget") && seen.contains("sum") && seen.contains("key")
        }
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::Avg
        | AggregateFunction::List
        | AggregateFunction::Set => true,
    }
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
    Ok(AtomPlan::TimeScope {
        reference: time_block.reference.clone(),
        inner: Box::new(plan_body(&time_block.body, catalog, slots, query_index)?),
        outer_slots: outer_slots.into_iter().collect(),
        provenance: TimeScopeProvenance {
            reference: time_block.reference.clone(),
            location: time_block.location.clone(),
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

fn recursive_atom_indexes(body: &Body, stratum_predicates: &BTreeSet<PredicateRef>) -> Vec<usize> {
    body.atoms
        .iter()
        .enumerate()
        .filter_map(|(idx, atom)| match atom {
            Atom::Derived(derived) if stratum_predicates.contains(&derived.predicate) => Some(idx),
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
    fn shadow_migration_target_is_a_plan_property() {
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
        assert!(
            incoming_rule.shadow_planned,
            "shadow migration target should be marked by the plan"
        );
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
                        && stage.shadow_planned
                        && stage
                            .authoritative_predicates
                            .iter()
                            .any(|predicate| predicate.display_name() == "entropy")
                        && stage
                            .predicates
                            .iter()
                            .any(|predicate| predicate.display_name() == "entropy")
                }),
            "entropy should be marked as a stage-level authoritative and shadow target"
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
    fn aggregate_argument_shape_is_part_of_migration_certificate() {
        let program = parse_prelude_program(
            "<aggregate-arg-certificate>",
            r#"
            amount("a", 1).
            invalid_unknown(h, score) :=
              (h, score) = TopK{ k: 1, bogus: 2, key: score : (h, score) : amount(h, score) }.
            invalid_duplicate(h, score) :=
              (h, score) = TopK{ k: 1, k: 2, key: score : (h, score) : amount(h, score) }.
            ? invalid_unknown(h, score).
            ? invalid_duplicate(h, score).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");

        for predicate in ["invalid_unknown", "invalid_duplicate"] {
            let predicate = PredicateRef::parse(predicate).expect("predicate parses");
            let migration = predicate_migration(&planned, &predicate).expect("predicate stage");
            assert_eq!(migration.mode, StageMigrationMode::Interpreted);
            assert!(
                migration
                    .reasons
                    .iter()
                    .any(|reason| matches!(reason, UnsupportedReason::InvalidAggregateArgs(_))),
                "invalid aggregate argument shape must keep the stage interpreted"
            );
        }
    }

    #[test]
    fn unsupported_time_scope_shape_stays_interpreted_by_certificate() {
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
            assert_eq!(migration.mode, StageMigrationMode::Interpreted);
            assert!(
                migration.reasons.contains(&UnsupportedReason::TimeScope),
                "unsupported time-scope shape must keep the stage interpreted"
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
}
