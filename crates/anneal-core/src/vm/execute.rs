//! Planned executor over the tuple VM.

use std::cmp::Ordering;
use std::collections::btree_set;
use std::collections::{BTreeMap, BTreeSet};
use std::slice;
use std::sync::Arc;

use regex::Regex;

use crate::ir::ids::{ListId, SymbolId};
use crate::ir::interner::Interner;
use crate::ir::plan::{
    AggregateArgPlanError, AggregatePlan, AtomPlan, CallArgPlan, ColumnPatternPlan, ComparePlan,
    ExprPlan, LiteralPlan, OrderKeyPlan, OutputPlan, PlanCatalog, PlanRelationKind, ProgramPlan,
    QueryPlan, RuleBodyPlan, RuleGroupPlan, TermPlan, UnsupportedTimeScope,
    UnsupportedTimeScopeAtom,
};
use crate::runtime::ast::{
    AggregateFunction, ComparisonOp, Ident, NumberLiteral, OrderDirection, PredicateRef,
};
use crate::runtime::eval::{
    Database, EvalError, EvalOptions, ExplainOptions, NumberValue, QueryOutput, QueryWarning, Row,
    Tuple, Value,
};
use crate::runtime::primitives::PrimitivePredicate;
use crate::vm::frame::PlannedFrame;
use crate::vm::provenance::{DerivationNode, DerivationRef, derivation_ref};
use crate::vm::value::{ListArena, PhysicalValue};

const MAX_AGGREGATE_DERIVATION_CHILDREN: usize = 32;

pub(crate) type DeltaMap = BTreeMap<PredicateRef, DerivedRelation>;

#[derive(Clone, Debug, Default)]
pub(crate) struct DerivedRelation {
    tuples: BTreeSet<Tuple>,
    derivations: BTreeMap<Tuple, DerivationRef>,
    indexes: Vec<BTreeMap<Value, Vec<Tuple>>>,
}

impl DerivedRelation {
    pub(crate) fn len(&self) -> usize {
        self.tuples.len()
    }

    pub(crate) fn tuples(&self) -> &BTreeSet<Tuple> {
        &self.tuples
    }

    pub(crate) fn insert_with_derivation(
        &mut self,
        tuple: &Tuple,
        derivation: Option<DerivationRef>,
    ) -> bool {
        if !self.tuples.insert(tuple.clone()) {
            return false;
        }
        if let Some(derivation) = derivation {
            self.derivations.insert(tuple.clone(), derivation);
        }
        if self.indexes.len() < tuple.0.len() {
            self.indexes.resize_with(tuple.0.len(), BTreeMap::new);
        }
        for (idx, value) in tuple.0.iter().enumerate() {
            self.indexes[idx]
                .entry(value.clone())
                .or_default()
                .push(tuple.clone());
        }
        true
    }

    pub(crate) fn derivation(&self, tuple: &Tuple) -> Option<DerivationRef> {
        self.derivations.get(tuple).map(Arc::clone)
    }

    pub(crate) fn candidate_tuples(&self, constraints: &[(usize, Value)]) -> TupleCandidates<'_> {
        let mut best = None;
        for (idx, value) in constraints {
            let Some(values) = self.indexes.get(*idx) else {
                return TupleCandidates::Empty;
            };
            let Some(tuples) = values.get(value) else {
                return TupleCandidates::Empty;
            };
            if best.is_none_or(|current: &Vec<Tuple>| tuples.len() < current.len()) {
                best = Some(tuples);
            }
        }

        best.map_or_else(
            || TupleCandidates::All(self.tuples.iter()),
            |tuples| TupleCandidates::Indexed(tuples.iter()),
        )
    }
}

pub(crate) enum TupleCandidates<'a> {
    All(btree_set::Iter<'a, Tuple>),
    Indexed(slice::Iter<'a, Tuple>),
    Empty,
}

impl<'a> Iterator for TupleCandidates<'a> {
    type Item = &'a Tuple;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(tuples) => tuples.next(),
            Self::Indexed(tuples) => tuples.next(),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DerivedTuple {
    pub(crate) tuple: Tuple,
    pub(crate) derivation: Option<DerivationRef>,
}

pub(crate) fn clone_derivation_refs(steps: &[DerivationRef]) -> Vec<DerivationNode> {
    steps.iter().map(|step| step.as_ref().clone()).collect()
}

// Local arenas model appending to a fresh clone, so their IDs are base-length offsets.
#[derive(Debug)]
struct PlannedValueEnv<'a> {
    base_interner: &'a Interner,
    local_interner: Interner,
    base_lists: &'a ListArena,
    local_lists: ListArena,
}

impl<'a> PlannedValueEnv<'a> {
    fn from_database(database: &'a Database) -> Self {
        Self::new(database.tuple_interner(), database.tuple_lists())
    }

    fn new(base_interner: &'a Interner, base_lists: &'a ListArena) -> Self {
        Self {
            base_interner,
            local_interner: Interner::default(),
            base_lists,
            local_lists: ListArena::default(),
        }
    }

    fn physical_from_logical(&mut self, value: &Value) -> PhysicalValue {
        match value {
            Value::String(value) => PhysicalValue::Sym(self.intern(value)),
            Value::Number(value) => PhysicalValue::Number(*value),
            Value::Bool(value) => PhysicalValue::Bool(*value),
            Value::Null => PhysicalValue::Null,
            Value::List(values) => {
                let values = values
                    .iter()
                    .map(|value| self.physical_from_logical(value))
                    .collect::<Vec<_>>();
                PhysicalValue::List(self.push_list(values))
            }
        }
    }

    fn logical(&self, value: PhysicalValue) -> Result<Value, EvalError> {
        self.logical_value(value)
            .ok_or(EvalError::UnsupportedExpression)
    }

    fn logical_value(&self, value: PhysicalValue) -> Option<Value> {
        match value {
            PhysicalValue::Sym(symbol) => self
                .resolve(symbol)
                .map(|text| Value::String(text.to_owned())),
            PhysicalValue::Number(value) => Some(Value::Number(value)),
            PhysicalValue::Bool(value) => Some(Value::Bool(value)),
            PhysicalValue::Null => Some(Value::Null),
            PhysicalValue::List(list) => {
                let values = self
                    .list(list)?
                    .iter()
                    .copied()
                    .map(|value| self.logical_value(value))
                    .collect::<Option<Vec<_>>>()?;
                Some(Value::List(values))
            }
        }
    }

    fn intern(&mut self, text: &str) -> SymbolId {
        if let Some(symbol) = self.base_interner.lookup(text) {
            return symbol;
        }
        let local = self.local_interner.intern(text);
        SymbolId::from_index(self.base_interner.len() + local.index())
    }

    fn resolve(&self, symbol: SymbolId) -> Option<&str> {
        if symbol.index() < self.base_interner.len() {
            return self.base_interner.resolve(symbol);
        }
        let local = SymbolId::from_index(symbol.index() - self.base_interner.len());
        self.local_interner.resolve(local)
    }

    fn push_list(&mut self, values: Vec<PhysicalValue>) -> ListId {
        let local = self.local_lists.push(values);
        ListId::from_index(self.base_lists.len() + local.index())
    }

    fn list(&self, list: ListId) -> Option<&[PhysicalValue]> {
        if list.index() < self.base_lists.len() {
            return self.base_lists.get(list);
        }
        let local = ListId::from_index(list.index() - self.base_lists.len());
        self.local_lists.get(local)
    }
}

struct PlannedEvalCtx<'ctx, 'db> {
    catalog: &'ctx PlanCatalog,
    database: &'ctx Database,
    warnings: &'ctx mut Vec<QueryWarning>,
    options: &'ctx EvalOptions,
    env: &'ctx mut PlannedValueEnv<'db>,
    delta: Option<PlannedDeltaView<'ctx>>,
}

impl PlannedEvalCtx<'_, '_> {
    fn with_delta_for_atom<T>(
        &mut self,
        atom_index: usize,
        f: impl FnOnce(&mut Self) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        let previous_delta = self.delta;
        self.delta = previous_delta.filter(|view| view.atom_index == atom_index);
        let result = f(self);
        self.delta = previous_delta;
        result
    }
}

pub(crate) fn eval_planned_rule_group(
    planned: &RuleGroupPlan,
    catalog: &PlanCatalog,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<DerivedTuple>, EvalError> {
    eval_planned_rule_group_inner(planned, catalog, database, None, warnings, options)
}

pub(crate) fn eval_planned_rule_group_with_delta(
    planned: &RuleGroupPlan,
    catalog: &PlanCatalog,
    database: &Database,
    delta: PlannedDeltaView<'_>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<DerivedTuple>, EvalError> {
    eval_planned_rule_group_inner(planned, catalog, database, Some(delta), warnings, options)
}

fn eval_planned_rule_group_inner(
    planned: &RuleGroupPlan,
    catalog: &PlanCatalog,
    database: &Database,
    delta: Option<PlannedDeltaView<'_>>,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<Vec<DerivedTuple>, EvalError> {
    let mut env = PlannedValueEnv::from_database(database);
    let bindings = {
        let mut ctx = PlannedEvalCtx {
            catalog,
            database,
            warnings,
            options,
            env: &mut env,
            delta,
        };
        eval_planned_body(
            &planned.body,
            vec![PlannedFrame::empty(planned.slots.vars().len())],
            &mut ctx,
        )?
    };
    bindings
        .iter()
        .map(|binding| {
            let tuple = project_planned_head(&planned.head_terms, binding, &mut env)?;
            let derivation = if options.explain().is_enabled() {
                let provenance = planned
                    .provenance
                    .as_ref()
                    .ok_or(EvalError::UnsupportedExpression)?;
                Some(derivation_ref(
                    DerivationNode::planned_rule(
                        provenance,
                        &tuple.0,
                        clone_derivation_refs(&binding.steps),
                    )
                    .bounded(options.explain()),
                ))
            } else {
                None
            };
            Ok(DerivedTuple { tuple, derivation })
        })
        .collect()
}

pub(crate) fn eval_planned_query_output(
    query_plan: Option<&QueryPlan>,
    planned: &ProgramPlan,
    database: &Database,
    warnings: &[QueryWarning],
    options: &EvalOptions,
) -> Result<QueryOutput, EvalError> {
    let query_plan = query_plan.ok_or(EvalError::UnsupportedExpression)?;
    let mut planned_warnings = warnings.to_vec();
    eval_planned_query(
        query_plan,
        &planned.catalog,
        database,
        &mut planned_warnings,
        options,
    )
}

fn eval_planned_query(
    query: &QueryPlan,
    catalog: &PlanCatalog,
    database: &Database,
    warnings: &mut Vec<QueryWarning>,
    options: &EvalOptions,
) -> Result<QueryOutput, EvalError> {
    let output_group = query
        .output_group()
        .ok_or(EvalError::UnsupportedExpression)?;
    if let Some(output_stage) = query.output_stage()
        && !output_stage.authoritative_planned
    {
        if let Some(error) = planned_body_time_scope_error(&output_group.body) {
            return Err(error);
        }
        return Err(EvalError::PlannedExecutorUnsupported {
            predicate: query_output_predicate(),
            reasons: format!("{:?}", output_stage.migration.reasons),
        });
    }
    let mut env = PlannedValueEnv::from_database(database);
    let mut bindings = {
        let mut ctx = PlannedEvalCtx {
            catalog,
            database,
            warnings,
            options,
            env: &mut env,
            delta: None,
        };
        eval_planned_body(
            &output_group.body,
            vec![PlannedFrame::empty(output_group.slots.vars().len())],
            &mut ctx,
        )?
    };
    if options.explain().is_enabled() {
        ensure_no_reserved_planned_explain_fields(&query.plan.output)?;
    }
    sort_planned_bindings_for_query(&query.plan.output.ordering, &mut bindings, &mut env)?;
    let rows = planned_bindings_to_rows(&query.plan.output, bindings, &mut env, options.explain())?;
    Ok(QueryOutput {
        rows,
        warnings: std::mem::take(warnings),
    })
}

fn eval_planned_body(
    body: &RuleBodyPlan,
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let mut bindings = bindings;
    for atom_index in &body.execution_atoms {
        if bindings.is_empty() {
            break;
        }
        let atom = body
            .atoms
            .get(*atom_index)
            .ok_or(EvalError::UnsupportedExpression)?;
        bindings =
            ctx.with_delta_for_atom(*atom_index, |ctx| eval_planned_atom(atom, bindings, ctx))?;
    }
    Ok(bindings)
}

fn eval_planned_atom(
    atom: &AtomPlan,
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    match atom {
        AtomPlan::Scan { relation, patterns } => {
            eval_planned_scan(*relation, patterns, bindings, ctx)
        }
        AtomPlan::PrimitiveCall {
            predicate,
            primitive,
            args,
            ..
        } => eval_planned_primitive(predicate, *primitive, args, bindings, ctx),
        AtomPlan::Filter { comparison } => eval_planned_filter(comparison, bindings, ctx),
        AtomPlan::Aggregate(aggregate) => eval_planned_aggregate(aggregate, bindings, ctx),
        AtomPlan::Negation {
            inner, provenance, ..
        } => {
            let mut out = Vec::new();
            let trace = ctx.options.explain().is_enabled();
            for binding in bindings {
                let matches = eval_planned_body(inner, vec![binding.clone()], ctx)?;
                if matches.is_empty() {
                    out.push(binding.push_step(trace, || {
                        derivation_ref(DerivationNode::planned_negation(provenance))
                    }));
                }
            }
            Ok(out)
        }
        AtomPlan::TimeScope {
            reference,
            inner,
            provenance,
            ..
        } => eval_planned_time_scope(reference, inner, provenance, bindings, ctx),
    }
}

fn eval_planned_scan(
    relation: crate::ir::ids::RelationId,
    patterns: &[ColumnPatternPlan],
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let relation_info = ctx
        .catalog
        .relation(relation)
        .ok_or(EvalError::UnsupportedExpression)?;
    match relation_info.kind {
        PlanRelationKind::Stored => {
            if ctx.delta.is_some() {
                return Err(EvalError::UnsupportedExpression);
            }
            eval_planned_stored_scan(relation, &relation_info.name, patterns, bindings, ctx)
        }
        PlanRelationKind::Derived => {
            let predicate = PredicateRef::parse(&relation_info.name)
                .map_err(|_| EvalError::UnsupportedExpression)?;
            match ctx.delta {
                Some(view) if view.delta_relation == relation => {
                    let Some(relation) = view.delta.get(&predicate) else {
                        return Ok(Vec::new());
                    };
                    eval_planned_derived_scan(&predicate, relation, patterns, bindings, ctx)
                }
                Some(_) => Err(EvalError::UnsupportedExpression),
                None => {
                    let relation = ctx.database.derived_relation(&predicate).ok_or_else(|| {
                        EvalError::UnknownDerivedPredicate {
                            predicate: predicate.clone(),
                        }
                    })?;
                    eval_planned_derived_scan(&predicate, relation, patterns, bindings, ctx)
                }
            }
        }
        PlanRelationKind::Primitive { .. } => Err(EvalError::UnsupportedExpression),
    }
}

fn eval_planned_stored_scan(
    relation: crate::ir::ids::RelationId,
    relation_name: &str,
    patterns: &[ColumnPatternPlan],
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let mut out = Vec::new();
    let trace = ctx.options.explain().is_enabled();
    let relation_ident = Ident::new_unchecked(relation_name.to_string());
    let Some(store) = ctx.database.physical_tuple_store(&relation_ident, relation) else {
        return Ok(Vec::new());
    };
    for binding in bindings {
        let constraints = planned_column_constraints(patterns, &binding, ctx.env)?;
        for row in store.candidate_rows(&constraints) {
            if !ctx
                .database
                .stored_tuple_row_visible(&relation_ident, row, ctx.options)
            {
                continue;
            }
            let Some(tuple) = store.row(row) else {
                continue;
            };
            let mut next = binding.clone();
            let mut matched = true;
            for pattern in patterns {
                let Some(value) = tuple.get(pattern.field) else {
                    matched = false;
                    break;
                };
                if !unify_planned_term(&pattern.term, value, &mut next, ctx.env)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                let step = if trace {
                    Some(ctx.database.stored_tuple_derivation(&relation_ident, row)?)
                } else {
                    None
                };
                out.push(next.push_step(trace, || step.expect("trace step exists")));
            }
        }
    }
    Ok(out)
}

fn eval_planned_time_scope(
    reference: &str,
    inner: &RuleBodyPlan,
    provenance: &crate::ir::plan::TimeScopeProvenance,
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    if let Some(unsupported) = &provenance.unsupported {
        return Err(unsupported_time_scope_error(unsupported));
    }
    let (scoped, scoped_warnings) = ctx.database.scoped_to_time_ref(reference)?;
    push_warnings(ctx.warnings, scoped_warnings);
    let trace = ctx.options.explain().is_enabled();
    let mut out = Vec::new();
    for binding in bindings {
        let outer_steps = trace.then(|| binding.steps.clone());
        let children = {
            let mut scoped_ctx = PlannedEvalCtx {
                catalog: ctx.catalog,
                database: &scoped,
                warnings: &mut *ctx.warnings,
                options: ctx.options,
                env: &mut *ctx.env,
                delta: None,
            };
            eval_planned_body(inner, vec![binding.with_values_only()], &mut scoped_ctx)?
        };
        out.extend(children.into_iter().map(|child| {
            if trace {
                let mut steps = outer_steps.clone().unwrap_or_default();
                steps.push(derivation_ref(DerivationNode::time_block(
                    &provenance.reference,
                    provenance.location.clone(),
                    clone_derivation_refs(&child.steps),
                )));
                PlannedFrame {
                    slots: child.slots,
                    steps,
                }
            } else {
                PlannedFrame {
                    slots: child.slots,
                    steps: binding.steps.clone(),
                }
            }
        }));
    }
    Ok(out)
}

fn planned_body_time_scope_error(body: &RuleBodyPlan) -> Option<EvalError> {
    body.atoms.iter().find_map(planned_atom_time_scope_error)
}

fn planned_atom_time_scope_error(atom: &AtomPlan) -> Option<EvalError> {
    match atom {
        AtomPlan::TimeScope {
            inner, provenance, ..
        } => provenance
            .unsupported
            .as_ref()
            .map(unsupported_time_scope_error)
            .or_else(|| planned_body_time_scope_error(inner)),
        AtomPlan::Aggregate(aggregate) => planned_body_time_scope_error(&aggregate.inner),
        AtomPlan::Negation { inner, .. } => planned_body_time_scope_error(inner),
        AtomPlan::Scan { .. } | AtomPlan::Filter { .. } | AtomPlan::PrimitiveCall { .. } => None,
    }
}

fn unsupported_time_scope_error(unsupported: &UnsupportedTimeScope) -> EvalError {
    match &unsupported.atom {
        UnsupportedTimeScopeAtom::StoredRelation { relation } => {
            EvalError::UnsupportedTimeScopedStoredRelation {
                reference: unsupported.reference.clone(),
                relation: relation.clone(),
            }
        }
        UnsupportedTimeScopeAtom::DerivedPredicate { predicate } => {
            EvalError::UnsupportedTimeScopedDerivedPredicate {
                reference: unsupported.reference.clone(),
                predicate: predicate.clone(),
            }
        }
        UnsupportedTimeScopeAtom::Primitive { predicate } => {
            EvalError::UnsupportedTimeScopedPrimitive {
                reference: unsupported.reference.clone(),
                predicate: predicate.clone(),
            }
        }
        UnsupportedTimeScopeAtom::UnknownRelation => EvalError::UnsupportedExpression,
    }
}

fn eval_planned_derived_scan(
    predicate: &PredicateRef,
    relation: &DerivedRelation,
    patterns: &[ColumnPatternPlan],
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let mut out = Vec::new();
    let trace = ctx.options.explain().is_enabled();
    for binding in bindings {
        let constraints = planned_tuple_constraints(patterns, &binding, ctx.env)?;
        for tuple in relation.candidate_tuples(&constraints) {
            let mut next = binding.clone();
            let mut matched = true;
            for pattern in patterns {
                let Some(value) = tuple.0.get(pattern.field.index()) else {
                    matched = false;
                    break;
                };
                let physical = ctx.env.physical_from_logical(value);
                if !unify_planned_term(&pattern.term, physical, &mut next, ctx.env)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                out.push(next.push_step(trace, || {
                    relation.derivation(tuple).unwrap_or_else(|| {
                        derivation_ref(DerivationNode::fact(predicate, &tuple.0))
                    })
                }));
            }
        }
    }
    Ok(out)
}

fn eval_planned_primitive(
    predicate: &PredicateRef,
    primitive: PrimitivePredicate,
    args: &[CallArgPlan],
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let mut out = Vec::new();
    let mut regex_cache = BTreeMap::<String, Regex>::new();
    let trace = ctx.options.explain().is_enabled();
    for binding in bindings {
        let constraints = planned_call_constraints(args, &binding, ctx.env)?;
        let tuples = ctx.database.primitive_tuples(
            primitive,
            &constraints,
            ctx.options,
            &mut regex_cache,
        )?;
        for tuple in tuples {
            let mut next = binding.clone();
            let mut matched = true;
            for arg in args {
                let Some(value) = tuple.0.get(arg.position) else {
                    matched = false;
                    break;
                };
                let physical = ctx.env.physical_from_logical(value);
                if !unify_planned_term(&arg.term, physical, &mut next, ctx.env)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                out.push(next.push_step(trace, || {
                    derivation_ref(DerivationNode::primitive(predicate, &tuple.0))
                }));
            }
        }
    }
    Ok(out)
}

fn eval_planned_filter(
    comparison: &ComparePlan,
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let mut out = Vec::new();
    let trace = ctx.options.explain().is_enabled();
    for binding in bindings {
        let left = eval_planned_expr_logical(&comparison.left, &binding, ctx.env)?;
        let right = eval_planned_expr_logical(&comparison.right, &binding, ctx.env)?;
        if compare(&left, comparison.op, &right)? {
            out.push(binding.push_step(trace, || {
                derivation_ref(DerivationNode::planned_comparison(&comparison.provenance))
            }));
        }
    }
    Ok(out)
}

fn eval_planned_aggregate(
    aggregate: &AggregatePlan,
    bindings: Vec<PlannedFrame>,
    ctx: &mut PlannedEvalCtx<'_, '_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    report_planned_aggregate_arg_error(aggregate)?;

    let mut out = Vec::new();
    let trace = ctx.options.explain().is_enabled();
    for binding in bindings {
        let mut inner_seed = PlannedFrame::empty(aggregate.inner_slots.vars().len());
        for (outer, inner) in &aggregate.outer_to_inner_slots {
            if let Some(value) = binding.get(*outer) {
                inner_seed.set(*inner, value);
            }
        }
        let inner_rows = eval_planned_body(&aggregate.inner, vec![inner_seed], ctx)?;
        if inner_rows.is_empty() {
            if aggregate.function == AggregateFunction::Count
                && let Some(binding) = unify_planned_expr(
                    &aggregate.result,
                    PhysicalValue::Number(NumberValue::Int(0)),
                    &binding,
                    ctx.env,
                )?
            {
                out.push(binding.push_step(trace, || {
                    derivation_ref(DerivationNode::planned_aggregate(
                        &aggregate.provenance,
                        Vec::new(),
                    ))
                }));
            }
            continue;
        }
        let aggregate_steps = trace.then(|| aggregate_derivation_steps_planned(&inner_rows));
        match aggregate.function {
            AggregateFunction::TopK => {
                out.extend(eval_planned_top_k(
                    aggregate,
                    &binding,
                    &inner_rows,
                    aggregate_steps.as_deref().unwrap_or_default(),
                    trace,
                    ctx.env,
                )?);
            }
            AggregateFunction::Rank => {
                out.extend(eval_planned_rank(
                    aggregate,
                    &binding,
                    inner_rows,
                    aggregate_steps.as_deref().unwrap_or_default(),
                    trace,
                    ctx.env,
                )?);
            }
            AggregateFunction::Count
            | AggregateFunction::Sum
            | AggregateFunction::Min
            | AggregateFunction::Max
            | AggregateFunction::Avg
            | AggregateFunction::List
            | AggregateFunction::Set => {
                if let Some(binding) = eval_planned_scalar_aggregate(
                    aggregate,
                    &binding,
                    &inner_rows,
                    aggregate_steps.as_deref().unwrap_or_default(),
                    trace,
                    ctx.env,
                )? {
                    out.push(binding);
                }
            }
            AggregateFunction::TakeUntil => {
                out.extend(eval_planned_take_until(
                    aggregate,
                    &binding,
                    &inner_rows,
                    aggregate_steps.as_deref().unwrap_or_default(),
                    trace,
                    ctx.env,
                )?);
            }
        }
    }
    Ok(out)
}

fn report_planned_aggregate_arg_error(aggregate: &AggregatePlan) -> Result<(), EvalError> {
    match aggregate.args.error {
        Some(AggregateArgPlanError::Unknown) => Err(EvalError::InvalidAggregateArg {
            function: aggregate.function,
            argument: "unknown",
        }),
        Some(AggregateArgPlanError::Duplicate) => Err(EvalError::InvalidAggregateArg {
            function: aggregate.function,
            argument: "duplicate",
        }),
        Some(AggregateArgPlanError::Missing(argument)) => Err(EvalError::MissingAggregateArg {
            function: aggregate.function,
            argument: argument.as_str(),
        }),
        None => Ok(()),
    }
}

fn eval_planned_scalar_aggregate(
    aggregate: &AggregatePlan,
    base: &PlannedFrame,
    rows: &[PlannedFrame],
    aggregate_steps: &[DerivationRef],
    trace: bool,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Option<PlannedFrame>, EvalError> {
    let values = rows
        .iter()
        .map(|row| eval_planned_expr_logical(&aggregate.value, row, env))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(value) = scalar_aggregate_value(aggregate.function, &values)? else {
        return Ok(None);
    };
    let value = env.physical_from_logical(&value);
    Ok(
        unify_planned_expr(&aggregate.result, value, base, env)?.map(|binding| {
            binding.push_step(trace, || {
                derivation_ref(DerivationNode::planned_aggregate(
                    &aggregate.provenance,
                    clone_derivation_refs(aggregate_steps),
                ))
            })
        }),
    )
}

fn eval_planned_top_k(
    aggregate: &AggregatePlan,
    base: &PlannedFrame,
    rows: &[PlannedFrame],
    aggregate_steps: &[DerivationRef],
    trace: bool,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let k = aggregate
        .args
        .k
        .as_ref()
        .expect("planner requires TopK k argument");
    let k = planned_non_negative_int(AggregateFunction::TopK, "k", k, base, env)?;
    if k == 0 {
        return Ok(Vec::new());
    }
    let key = aggregate
        .args
        .key
        .as_ref()
        .expect("planner requires TopK key argument");
    let limit = usize::try_from(k).unwrap_or(usize::MAX);
    let mut candidates = Vec::<(Value, PhysicalValue)>::new();
    for row in rows {
        let candidate = (
            eval_planned_expr_logical(key, row, env)?,
            eval_planned_expr(&aggregate.value, row, env)?,
        );
        let insert_at = candidates
            .binary_search_by(|existing| {
                existing.0.cmp(&candidate.0).reverse().then_with(|| {
                    env.logical(existing.1)
                        .unwrap_or(Value::Null)
                        .cmp(&env.logical(candidate.1).unwrap_or(Value::Null))
                })
            })
            .unwrap_or_else(|idx| idx);
        if insert_at < limit {
            candidates.insert(insert_at, candidate);
            if candidates.len() > limit {
                candidates.pop();
            }
        }
    }
    let mut out = Vec::new();
    for (_, value) in candidates {
        if let Some(binding) = unify_planned_expr(&aggregate.result, value, base, env)? {
            out.push(binding.push_step(trace, || {
                derivation_ref(DerivationNode::planned_aggregate(
                    &aggregate.provenance,
                    clone_derivation_refs(aggregate_steps),
                ))
            }));
        }
    }
    Ok(out)
}

fn eval_planned_rank(
    aggregate: &AggregatePlan,
    base: &PlannedFrame,
    rows: Vec<PlannedFrame>,
    aggregate_steps: &[DerivationRef],
    trace: bool,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let key = aggregate
        .args
        .key
        .as_ref()
        .expect("planner requires Rank key argument");
    let rank_slot = aggregate
        .args
        .synthetic_rank_slot
        .expect("planner requires Rank rank argument");
    let mut rows = rows
        .into_iter()
        .map(|row| {
            let key = eval_planned_expr_logical(key, &row, env)?;
            Ok((key, row))
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    rows.sort_by(|(left_key, left), (right_key, right)| {
        right_key
            .cmp(left_key)
            .then_with(|| planned_frame_logical_cmp(left, right, env))
    });
    let mut out = Vec::new();
    let mut current_rank = 0_i64;
    let mut previous_key = None;
    for (key_value, mut row) in rows {
        if previous_key.as_ref() != Some(&key_value) {
            current_rank += 1;
            previous_key = Some(key_value);
        }
        row.overwrite(
            rank_slot,
            PhysicalValue::Number(NumberValue::Int(current_rank)),
        );
        let value = eval_planned_expr(&aggregate.value, &row, env)?;
        if let Some(binding) = unify_planned_expr(&aggregate.result, value, base, env)? {
            out.push(binding.push_step(trace, || {
                derivation_ref(DerivationNode::planned_aggregate(
                    &aggregate.provenance,
                    clone_derivation_refs(aggregate_steps),
                ))
            }));
        }
    }
    Ok(out)
}

struct PlannedTakeUntilCandidate {
    key: Value,
    value: Value,
    cost: i64,
}

fn eval_planned_take_until(
    aggregate: &AggregatePlan,
    base: &PlannedFrame,
    rows: &[PlannedFrame],
    aggregate_steps: &[DerivationRef],
    trace: bool,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<PlannedFrame>, EvalError> {
    let budget = aggregate
        .args
        .budget
        .as_ref()
        .expect("planner requires TakeUntil budget argument");
    let budget =
        planned_non_negative_int(AggregateFunction::TakeUntil, "budget", budget, base, env)?;
    let sum = aggregate
        .args
        .sum
        .as_ref()
        .expect("planner requires TakeUntil sum argument");
    let key = aggregate
        .args
        .key
        .as_ref()
        .expect("planner requires TakeUntil key argument");
    let mut candidates = rows
        .iter()
        .map(|row| {
            let Value::Number(NumberValue::Int(cost)) = eval_planned_expr_logical(sum, row, env)?
            else {
                return Err(EvalError::InvalidAggregateArg {
                    function: AggregateFunction::TakeUntil,
                    argument: "sum",
                });
            };
            if cost < 0 {
                return Err(EvalError::InvalidAggregateArg {
                    function: AggregateFunction::TakeUntil,
                    argument: "sum",
                });
            }
            Ok(PlannedTakeUntilCandidate {
                key: eval_planned_expr_logical(key, row, env)?,
                value: eval_planned_expr_logical(&aggregate.value, row, env)?,
                cost,
            })
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    candidates.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.value.cmp(&right.value))
    });

    let mut out = Vec::new();
    let mut used = 0_i64;
    for candidate in candidates {
        let next = used.saturating_add(candidate.cost);
        if next > budget {
            break;
        }
        used = next;
        let value = env.physical_from_logical(&candidate.value);
        if let Some(binding) = unify_planned_expr(&aggregate.result, value, base, env)? {
            out.push(binding.push_step(trace, || {
                derivation_ref(DerivationNode::planned_aggregate(
                    &aggregate.provenance,
                    clone_derivation_refs(aggregate_steps),
                ))
            }));
        }
    }
    Ok(out)
}

fn aggregate_derivation_steps_planned(rows: &[PlannedFrame]) -> Vec<DerivationRef> {
    collect_aggregate_derivation_steps(rows.iter().flat_map(|row| row.steps.iter()))
}

fn planned_column_constraints(
    patterns: &[ColumnPatternPlan],
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<(crate::ir::ids::FieldId, PhysicalValue)>, EvalError> {
    let mut constraints = Vec::new();
    for pattern in patterns {
        if let Some(value) = planned_constraint_value_for_term(&pattern.term, binding, env)? {
            constraints.push((pattern.field, value));
        }
    }
    Ok(constraints)
}

fn planned_tuple_constraints(
    patterns: &[ColumnPatternPlan],
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<(usize, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for pattern in patterns {
        if let Some(value) = planned_constraint_value_for_term(&pattern.term, binding, env)? {
            constraints.push((pattern.field.index(), env.logical(value)?));
        }
    }
    Ok(constraints)
}

fn planned_call_constraints(
    args: &[CallArgPlan],
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Vec<(usize, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for arg in args {
        if let Some(value) = planned_constraint_value_for_term(&arg.term, binding, env)? {
            constraints.push((arg.position, env.logical(value)?));
        }
    }
    Ok(constraints)
}

fn planned_constraint_value_for_term(
    term: &TermPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Option<PhysicalValue>, EvalError> {
    match term {
        TermPlan::Wildcard => Ok(None),
        TermPlan::Expr(ExprPlan::Slot(slot)) => Ok(binding.get(*slot)),
        TermPlan::Expr(ExprPlan::Literal(literal)) => Ok(Some(physical_from_literal(literal, env))),
        TermPlan::Expr(expr) => match eval_planned_expr(expr, binding, env) {
            Ok(value) => Ok(Some(value)),
            Err(EvalError::UnboundVariable { .. }) => Ok(None),
            Err(error) => Err(error),
        },
    }
}

fn unify_planned_term(
    term: &TermPlan,
    value: PhysicalValue,
    binding: &mut PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<bool, EvalError> {
    match term {
        TermPlan::Wildcard => Ok(true),
        TermPlan::Expr(expr) => unify_planned_expr_in_place(expr, value, binding, env),
    }
}

fn unify_planned_expr(
    expr: &ExprPlan,
    value: PhysicalValue,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Option<PlannedFrame>, EvalError> {
    let mut next = binding.clone();
    if unify_planned_expr_in_place(expr, value, &mut next, env)? {
        Ok(Some(next))
    } else {
        Ok(None)
    }
}

fn unify_planned_expr_in_place(
    expr: &ExprPlan,
    value: PhysicalValue,
    binding: &mut PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<bool, EvalError> {
    match expr {
        ExprPlan::Slot(slot) => Ok(binding.set(*slot, value)),
        ExprPlan::Tuple(items) => {
            let PhysicalValue::List(list) = value else {
                return Ok(false);
            };
            let Some(values) = env.list(list).map(<[PhysicalValue]>::to_vec) else {
                return Ok(false);
            };
            if values.len() != items.len() {
                return Ok(false);
            }
            for (item, value) in items.iter().zip(values) {
                if !unify_planned_expr_in_place(item, value, binding, env)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(eval_planned_expr(expr, binding, env)? == value),
    }
}

fn eval_planned_expr(
    expr: &ExprPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<PhysicalValue, EvalError> {
    match expr {
        ExprPlan::Slot(slot) => binding.get(*slot).ok_or(EvalError::UnboundVariable {
            variable: Ident::new_unchecked(format!("slot{}", slot.index())),
        }),
        ExprPlan::Literal(literal) => Ok(physical_from_literal(literal, env)),
        ExprPlan::Binary { left, op, right } => {
            let left = eval_planned_expr_logical(left, binding, env)?;
            let right = eval_planned_expr_logical(right, binding, env)?;
            let value = eval_planned_binary_values(left, *op, right)?;
            Ok(env.physical_from_logical(&value))
        }
        ExprPlan::Tuple(items) => {
            let values = items
                .iter()
                .map(|item| eval_planned_expr(item, binding, env))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(PhysicalValue::List(env.push_list(values)))
        }
    }
}

fn eval_planned_expr_logical(
    expr: &ExprPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Value, EvalError> {
    let physical = eval_planned_expr(expr, binding, env)?;
    env.logical(physical)
}

fn physical_from_literal(literal: &LiteralPlan, env: &mut PlannedValueEnv<'_>) -> PhysicalValue {
    match literal {
        LiteralPlan::String(value) => PhysicalValue::Sym(env.intern(value)),
        LiteralPlan::Number(NumberLiteral::Int(value)) => {
            PhysicalValue::Number(NumberValue::Int(*value))
        }
        LiteralPlan::Number(NumberLiteral::Float(value)) => {
            PhysicalValue::Number(NumberValue::Float(*value))
        }
        LiteralPlan::Bool(value) => PhysicalValue::Bool(*value),
        LiteralPlan::Null => PhysicalValue::Null,
        LiteralPlan::List(values) => {
            let values = values
                .iter()
                .map(|value| physical_from_literal(value, env))
                .collect::<Vec<_>>();
            PhysicalValue::List(env.push_list(values))
        }
    }
}

fn eval_planned_binary_values(
    left: Value,
    op: crate::runtime::ast::ArithmeticOp,
    right: Value,
) -> Result<Value, EvalError> {
    let (Value::Number(left), Value::Number(right)) = (left, right) else {
        return Err(EvalError::UnsupportedExpression);
    };
    let (NumberValue::Int(left), NumberValue::Int(right)) = (left, right) else {
        return Err(EvalError::UnsupportedExpression);
    };
    let value = match op {
        crate::runtime::ast::ArithmeticOp::Add => left + right,
        crate::runtime::ast::ArithmeticOp::Sub => left - right,
        crate::runtime::ast::ArithmeticOp::Mul => left * right,
        crate::runtime::ast::ArithmeticOp::Div => {
            if right == 0 {
                return Err(EvalError::DivisionByZero);
            }
            left / right
        }
        crate::runtime::ast::ArithmeticOp::Rem => {
            if right == 0 {
                return Err(EvalError::DivisionByZero);
            }
            left % right
        }
    };
    Ok(Value::Number(NumberValue::Int(value)))
}

fn planned_non_negative_int(
    function: AggregateFunction,
    argument: &'static str,
    expr: &ExprPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<i64, EvalError> {
    let Value::Number(NumberValue::Int(value)) = eval_planned_expr_logical(expr, binding, env)?
    else {
        return Err(EvalError::InvalidAggregateArg { function, argument });
    };
    if value < 0 {
        return Err(EvalError::InvalidAggregateArg { function, argument });
    }
    Ok(value)
}

fn planned_frame_logical_cmp(
    left: &PlannedFrame,
    right: &PlannedFrame,
    env: &PlannedValueEnv<'_>,
) -> Ordering {
    left.slots
        .iter()
        .zip(&right.slots)
        .map(|(left, right)| {
            let left = left.and_then(|value| env.logical(value).ok());
            let right = right.and_then(|value| env.logical(value).ok());
            left.cmp(&right)
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn sort_planned_bindings_for_query(
    ordering: &[OrderKeyPlan],
    bindings: &mut Vec<PlannedFrame>,
    env: &mut PlannedValueEnv<'_>,
) -> Result<(), EvalError> {
    if ordering.is_empty() {
        return Ok(());
    }
    let mut keyed = std::mem::take(bindings)
        .into_iter()
        .enumerate()
        .map(|(index, binding)| {
            let keys = eval_planned_order_keys(ordering, &binding, env)?;
            Ok((index, keys, binding))
        })
        .collect::<Result<Vec<_>, EvalError>>()?;
    keyed.sort_by(|left, right| compare_ordered_planned_query_rows(ordering, left, right));
    bindings.extend(keyed.into_iter().map(|(_, _, binding)| binding));
    Ok(())
}

fn eval_planned_order_keys(
    ordering: &[OrderKeyPlan],
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<QueryOrderKeys, EvalError> {
    if let [key] = ordering {
        return eval_planned_expr_logical(&key.expr, binding, env).map(QueryOrderKeys::One);
    }
    ordering
        .iter()
        .map(|key| eval_planned_expr_logical(&key.expr, binding, env))
        .collect::<Result<Vec<_>, _>>()
        .map(QueryOrderKeys::Many)
}

fn compare_ordered_planned_query_rows<T>(
    ordering: &[OrderKeyPlan],
    left: &(usize, QueryOrderKeys, T),
    right: &(usize, QueryOrderKeys, T),
) -> Ordering {
    for (index, key) in ordering.iter().enumerate() {
        let (left_key, right_key) = order_key_values(index, &left.1, &right.1);
        let comparison = match key.direction {
            OrderDirection::Asc => left_key.cmp(right_key),
            OrderDirection::Desc => right_key.cmp(left_key),
        };
        if comparison != Ordering::Equal {
            return comparison;
        }
    }
    left.0.cmp(&right.0)
}

fn ensure_no_reserved_planned_explain_fields(output: &OutputPlan) -> Result<(), EvalError> {
    if output
        .projection
        .iter()
        .any(|(name, _)| name.as_str() == "_derivation")
    {
        return Err(EvalError::ReservedExplainField {
            field: "_derivation",
        });
    }
    Ok(())
}

fn query_output_predicate() -> PredicateRef {
    PredicateRef::new(Ident::new_unchecked("query_output"))
}

fn planned_bindings_to_rows(
    output: &OutputPlan,
    bindings: Vec<PlannedFrame>,
    env: &mut PlannedValueEnv<'_>,
    options: &ExplainOptions,
) -> Result<Vec<Row>, EvalError> {
    bindings
        .into_iter()
        .enumerate()
        .map(|(index, binding)| {
            planned_binding_to_row(output, &binding, env, options, options.explains_row(index))
        })
        .collect()
}

fn planned_binding_to_row(
    output: &OutputPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
    options: &ExplainOptions,
    include_derivation: bool,
) -> Result<Row, EvalError> {
    let fields = planned_projected_fields(output, binding, env)?;
    Ok(Row {
        fields,
        derivation: include_derivation
            .then(|| DerivationNode::query(clone_derivation_refs(&binding.steps)).bounded(options)),
    })
}

fn planned_projected_fields(
    output: &OutputPlan,
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<BTreeMap<String, Value>, EvalError> {
    output
        .projection
        .iter()
        .map(|(name, expr)| {
            Ok((
                name.to_string(),
                eval_planned_expr_logical(expr, binding, env)?,
            ))
        })
        .collect()
}

fn project_planned_head(
    terms: &[TermPlan],
    binding: &PlannedFrame,
    env: &mut PlannedValueEnv<'_>,
) -> Result<Tuple, EvalError> {
    terms
        .iter()
        .map(|term| match term {
            TermPlan::Wildcard => Ok(Value::Null),
            TermPlan::Expr(ExprPlan::Slot(slot)) => binding
                .get(*slot)
                .ok_or(EvalError::UnboundVariable {
                    variable: Ident::new_unchecked(format!("slot{}", slot.index())),
                })
                .and_then(|value| env.logical(value)),
            TermPlan::Expr(expr) => eval_planned_expr_logical(expr, binding, env),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Tuple)
}

#[derive(Clone, Copy)]
pub(crate) struct PlannedDeltaView<'a> {
    pub(crate) delta: &'a DeltaMap,
    pub(crate) atom_index: usize,
    pub(crate) delta_relation: crate::ir::ids::RelationId,
}

fn push_warnings(out: &mut Vec<QueryWarning>, warnings: Vec<QueryWarning>) {
    for warning in warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
}

fn collect_aggregate_derivation_steps<'a>(
    steps: impl Iterator<Item = &'a DerivationRef>,
) -> Vec<DerivationRef> {
    let mut out = Vec::new();
    let mut omitted = 0_usize;
    for step in steps {
        if out.len() < MAX_AGGREGATE_DERIVATION_CHILDREN {
            out.push(Arc::clone(step));
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        out.push(derivation_ref(DerivationNode::evidence_truncated(omitted)));
    }
    out
}

fn scalar_aggregate_value(
    function: AggregateFunction,
    values: &[Value],
) -> Result<Option<Value>, EvalError> {
    if values.is_empty() && function != AggregateFunction::Count {
        return Ok(None);
    }
    match function {
        AggregateFunction::Count => Ok(Some(Value::Number(NumberValue::Int(
            i64::try_from(distinct_aggregate_values(values).len()).unwrap_or(i64::MAX),
        )))),
        AggregateFunction::Sum => numeric_sum(values).map(Some),
        AggregateFunction::Min => Ok(values.iter().min().cloned()),
        AggregateFunction::Max => Ok(values.iter().max().cloned()),
        AggregateFunction::Avg => numeric_avg(values).map(Some),
        AggregateFunction::List | AggregateFunction::Set => Ok(Some(Value::List(
            distinct_aggregate_values(values).into_iter().collect(),
        ))),
        AggregateFunction::TopK | AggregateFunction::Rank | AggregateFunction::TakeUntil => {
            Err(EvalError::UnsupportedAggregate { function })
        }
    }
}

fn distinct_aggregate_values(values: &[Value]) -> BTreeSet<Value> {
    values.iter().cloned().collect()
}

fn numeric_sum(values: &[Value]) -> Result<Value, EvalError> {
    let mut int_sum = 0_i64;
    let mut float_sum = 0.0_f64;
    let mut has_float = false;
    for value in values {
        match numeric_value(value)? {
            NumberValue::Int(value) if !has_float => {
                int_sum = int_sum.saturating_add(value);
            }
            NumberValue::Int(value) => {
                float_sum += i64_to_f64(value);
            }
            NumberValue::Float(value) => {
                if !has_float {
                    float_sum = i64_to_f64(int_sum);
                    has_float = true;
                }
                float_sum += value;
            }
        }
    }
    if has_float {
        Ok(Value::Number(NumberValue::Float(float_sum)))
    } else {
        Ok(Value::Number(NumberValue::Int(int_sum)))
    }
}

fn numeric_avg(values: &[Value]) -> Result<Value, EvalError> {
    let mut total = 0.0_f64;
    for value in values {
        match numeric_value(value)? {
            NumberValue::Int(value) => total += i64_to_f64(value),
            NumberValue::Float(value) => total += value,
        }
    }
    Ok(Value::Number(NumberValue::Float(
        total / usize_to_f64(values.len()),
    )))
}

fn numeric_value(value: &Value) -> Result<NumberValue, EvalError> {
    let Value::Number(value) = value else {
        return Err(EvalError::UnsupportedExpression);
    };
    Ok(*value)
}

fn i64_to_f64(value: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

fn usize_to_f64(value: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f64
    }
}

enum QueryOrderKeys {
    One(Value),
    Many(Vec<Value>),
}

fn order_key_values<'a>(
    index: usize,
    left: &'a QueryOrderKeys,
    right: &'a QueryOrderKeys,
) -> (&'a Value, &'a Value) {
    match (left, right) {
        (QueryOrderKeys::One(left), QueryOrderKeys::One(right)) => {
            debug_assert_eq!(index, 0);
            (left, right)
        }
        (QueryOrderKeys::Many(left), QueryOrderKeys::Many(right)) => (&left[index], &right[index]),
        (QueryOrderKeys::One(_), QueryOrderKeys::Many(_))
        | (QueryOrderKeys::Many(_), QueryOrderKeys::One(_)) => {
            unreachable!("order key arity is fixed for a query")
        }
    }
}

fn compare(left: &Value, op: ComparisonOp, right: &Value) -> Result<bool, EvalError> {
    let result = match op {
        ComparisonOp::Eq => left == right,
        ComparisonOp::Ne => left != right,
        ComparisonOp::Lt => left < right,
        ComparisonOp::Gt => left > right,
        ComparisonOp::Le => left <= right,
        ComparisonOp::Ge => left >= right,
        ComparisonOp::In => match right {
            Value::List(items) => items.contains(left),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::Contains => match (left, right) {
            (Value::String(haystack), Value::String(needle)) => haystack.contains(needle),
            (Value::List(items), needle) => items.contains(needle),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::StartsWith => match (left, right) {
            (Value::String(value), Value::String(prefix)) => value.starts_with(prefix),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::EndsWith => match (left, right) {
            (Value::String(value), Value::String(suffix)) => value.ends_with(suffix),
            _ => return Err(EvalError::UnsupportedExpression),
        },
        ComparisonOp::Matches => unreachable!("planner excludes unsupported comparison"),
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::facts::{
        EdgeFact, FactBatch, FactBatchMode, FactIdentity, HandleFact, SnapshotFact,
    };
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::ir::plan::{plan, planned_rule_group_executable};
    use crate::runtime::{Evaluator, analyze, parse_program};
    use crate::store::FactStore;
    use crate::vm::provenance::DerivationKind;

    #[test]
    fn planned_value_env_overlays_preserve_fresh_clone_ids_without_mutating_base() {
        let mut base_interner = Interner::default();
        let mut base_lists = ListArena::default();
        let base_value = Value::List(vec![s("base"), Value::List(vec![s("nested-base")])]);
        let base_physical =
            PhysicalValue::from_logical(&base_value, &mut base_interner, &mut base_lists);
        let base_symbol = base_interner.lookup("base").expect("base symbol exists");
        let base_symbol_count = base_interner.len();
        let base_list_count = base_lists.len();
        let local_value = Value::List(vec![
            s("base"),
            s("local"),
            Value::List(vec![s("nested-local"), s("base")]),
        ]);

        let first = {
            let mut env = PlannedValueEnv::new(&base_interner, &base_lists);
            assert_eq!(env.intern("base"), base_symbol, "base lookup wins");
            let local_symbol = env.intern("local");
            assert_eq!(local_symbol.index(), base_symbol_count);
            assert_eq!(
                env.logical(base_physical)
                    .expect("base list resolves through the overlay"),
                base_value.clone()
            );
            let physical = env.physical_from_logical(&local_value);
            let PhysicalValue::List(outer_list) = physical else {
                panic!("local value is a list");
            };
            assert_eq!(outer_list.index(), base_list_count + 1);
            assert_eq!(
                env.logical(physical)
                    .expect("local nested list resolves through the overlay"),
                local_value.clone()
            );
            (local_symbol, physical)
        };
        assert_eq!(base_interner.len(), base_symbol_count);
        assert_eq!(base_lists.len(), base_list_count);

        let second = {
            let mut env = PlannedValueEnv::new(&base_interner, &base_lists);
            assert_eq!(env.intern("base"), base_symbol, "base lookup wins");
            let local_symbol = env.intern("local");
            let physical = env.physical_from_logical(&local_value);
            assert_eq!(
                env.logical(physical)
                    .expect("second local nested list resolves through the overlay"),
                local_value
            );
            (local_symbol, physical)
        };

        assert_eq!(
            second, first,
            "fresh rule-group overlays assign identical ids"
        );
        assert_eq!(base_interner.len(), base_symbol_count);
        assert_eq!(base_lists.len(), base_list_count);
    }

    fn identity(native_id: &str) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            NativeId::from(native_id),
            OriginUri::from(format!("fixture://{native_id}")),
            Revision::from("rev"),
            Generation::initial(),
        )
    }

    fn handle(id: &str, kind: &str, status: &str, namespace: &str, area: &str) -> HandleFact {
        handle_with_options(id, kind, Some(status), namespace, area, None)
    }

    fn handle_with_options(
        id: &str,
        kind: &str,
        status: Option<&str>,
        namespace: &str,
        area: &str,
        date: Option<&str>,
    ) -> HandleFact {
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(str::to_string),
            namespace: namespace.to_string(),
            file: format!("{area}/{id}.md"),
            line: 1,
            date: date.map(str::to_string),
            area: area.to_string(),
            summary: String::new(),
        }
    }

    fn edge(from: &str, to: &str, kind: &str) -> EdgeFact {
        EdgeFact {
            identity: identity(&format!("{from}->{to}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: "fixture.md".to_string(),
            line: 1,
            assertion_date: None,
            assertion_revision: None,
        }
    }

    fn snapshot_fact(snapshot: &str, at: &str, id: &str, key: &str, value: &str) -> SnapshotFact {
        SnapshotFact {
            corpus: CorpusId::from("test"),
            snapshot: snapshot.to_string(),
            at: at.to_string(),
            id: id.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn time_travel_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("draft.md", "file", "current", "", "core"),
            handle("plan.md", "file", "plan", "", "core"),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("time travel fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "draft.md", "status", "draft"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "plan.md", "status", "plan"),
                ],
            )
            .expect("replace snapshots");
        Database::from_store(&store)
    }

    fn time_travel_metric_database() -> Database {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![handle_with_options(
            "draft.md",
            "file",
            Some("current"),
            "",
            "core",
            Some("2026-05-01"),
        )];
        let mut store = FactStore::default();
        store
            .merge(batch)
            .expect("time travel metric fixture merge");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_fact("s1", "2026-05-01T00:00:00Z", "draft.md", "status", "raw"),
                    snapshot_fact("s2", "2026-05-10T00:00:00Z", "draft.md", "status", "draft"),
                ],
            )
            .expect("replace snapshots");
        Database::from_store(&store)
    }

    fn s(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn n(value: i64) -> Value {
        Value::Number(NumberValue::Int(value))
    }

    fn derivation_contains(node: &DerivationNode, kind: DerivationKind) -> bool {
        node.kind() == kind
            || node
                .children()
                .iter()
                .any(|child| derivation_contains(child, kind))
    }

    fn planned_rule<'a>(
        planned: &'a ProgramPlan,
        predicate: &str,
    ) -> (&'a RuleGroupPlan, &'a PlanCatalog) {
        let relation = planned
            .catalog
            .predicate_relation(&PredicateRef::parse(predicate).expect("predicate parses"))
            .expect("planned relation")
            .id;
        let rule = planned
            .global
            .strata
            .iter()
            .flat_map(|stratum| &stratum.rule_groups)
            .find(|rule| rule.head == Some(relation))
            .expect("planned rule");
        (rule, &planned.catalog)
    }

    fn derived_tuple_set(database: &Database, predicate: &str) -> BTreeSet<Tuple> {
        database
            .derived(&PredicateRef::parse(predicate).expect("predicate parses"))
            .expect("derived relation")
            .iter()
            .cloned()
            .collect()
    }

    fn planned_tuple_set(
        rule: &RuleGroupPlan,
        catalog: &PlanCatalog,
        database: &Database,
    ) -> BTreeSet<Tuple> {
        let mut warnings = Vec::new();
        eval_planned_rule_group(
            rule,
            catalog,
            database,
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect("planned rule evaluates")
        .into_iter()
        .map(|row| row.tuple)
        .collect()
    }

    #[test]
    fn planned_executor_evaluates_slots_from_catalog_decisions() {
        let input = r#"
        active(h) := *handle{id: h, status: "draft"}.
        candidate(h, score) := *handle{id: h}, active(h), in_degree(h, score).
        ranked(h, rank) :=
          (h, rank) = Rank{ key: score, rank: rank :
            (h, rank) : candidate(h, score)
          }.
        "#;
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");

        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("a.md", "file", "draft", "", "core"),
            handle("b.md", "file", "draft", "", "core"),
            handle("c.md", "file", "stable", "", "core"),
        ];
        batch.edges = vec![
            edge("x.md", "a.md", "DependsOn"),
            edge("y.md", "a.md", "DependsOn"),
            edge("z.md", "b.md", "DependsOn"),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merges");
        let database = Database::from_store(&store);
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("fixpoint evaluates");

        let (candidate, catalog) = planned_rule(&planned, "candidate");
        assert_eq!(
            planned_tuple_set(candidate, catalog, evaluator.database()),
            derived_tuple_set(evaluator.database(), "candidate"),
            "stored scan + graph primitive + soft-primitive override should match candidate rows"
        );
        let (ranked, catalog) = planned_rule(&planned, "ranked");
        assert_eq!(
            planned_tuple_set(ranked, catalog, evaluator.database()),
            derived_tuple_set(evaluator.database(), "ranked"),
            "rank aggregate should match ranked rows"
        );

        let candidate_atoms = &candidate.body.atoms;
        assert!(candidate_atoms.iter().any(|atom| {
            matches!(atom, AtomPlan::Scan { relation, .. }
                if catalog.relation(*relation).is_some_and(|relation| relation.kind == PlanRelationKind::Stored))
        }));
        assert!(candidate_atoms.iter().any(|atom| {
            matches!(atom, AtomPlan::Scan { relation, .. }
                if catalog.relation(*relation).is_some_and(|relation| relation.name == "active" && relation.kind == PlanRelationKind::Derived))
        }));
        assert!(candidate_atoms.iter().any(|atom| {
            matches!(
                atom,
                AtomPlan::PrimitiveCall {
                    primitive: PrimitivePredicate::InDegree,
                    ..
                }
            )
        }));
    }

    #[test]
    fn planned_rank_overwrites_inner_rank_binding() {
        let input = r#"
        seed_rank(99).
        candidate("a.md", 10).
        candidate("b.md", 3).
        ranked(h, rank) :=
          (h, rank) = Rank{ key: score, rank: rank :
            (h, rank) : candidate(h, score), seed_rank(rank)
          }.
        "#;
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint evaluates");

        let (ranked, catalog) = planned_rule(&planned, "ranked");
        assert_eq!(
            planned_tuple_set(ranked, catalog, evaluator.database()),
            derived_tuple_set(evaluator.database(), "ranked"),
            "planned Rank must overwrite the synthetic rank slot before projecting the aggregate value"
        );
    }

    #[test]
    fn planned_time_scope_uses_overlay_and_preserves_warning_and_provenance() {
        let input = r#"
            prior_status(h, prior) :=
              at("snapshot:last") { *handle{id: h, status: prior} }.
        "#;
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "prior_status");
        assert!(planned_rule_group_executable(rule, catalog));

        let mut warnings = Vec::new();
        let rows = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_database(),
            &mut warnings,
            &EvalOptions::default().with_explain_all(),
        )
        .expect("planned TimeScope evaluates");

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "partial_history");
        assert_eq!(warnings[0].reference.as_deref(), Some("snapshot:last"));
        assert_eq!(
            rows.iter()
                .map(|row| row.tuple.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                Tuple(vec![s("draft.md"), s("draft")]),
                Tuple(vec![s("plan.md"), s("plan")]),
            ])
        );
        let derivation = rows[0].derivation.as_ref().expect("row has derivation");
        assert!(derivation_contains(derivation, DerivationKind::TimeBlock));
        assert!(derivation_contains(derivation, DerivationKind::Stored));
    }

    #[test]
    fn planned_time_scope_uses_scoped_graph_primitives() {
        let input = r#"
            historical_freshness(h, days) :=
              at("snapshot:s2") { freshness(h, days) }.
        "#;
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "historical_freshness");

        let mut warnings = Vec::new();
        let rows = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_metric_database(),
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect("planned TimeScope evaluates");

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "partial_history");
        assert_eq!(
            rows.iter()
                .map(|row| row.tuple.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([Tuple(vec![s("draft.md"), n(9)])])
        );
    }

    #[test]
    fn planned_time_scope_rejects_unsupported_stored_relations() {
        let input = r#"
            historical_edge(to) :=
              at("snapshot:last") { *edge{from: "draft.md", to, kind} }.
        "#;
        let program = parse_program("inline", input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "historical_edge");

        let mut warnings = Vec::new();
        let err = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_database(),
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect_err("planned TimeScope rejects unsupported stored relation");

        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedStoredRelation { relation, .. }
                if relation.as_str() == "edge"
        ));
    }

    #[test]
    fn planned_time_scope_rejects_unsupported_primitives_and_derived_predicates() {
        let primitive_input = r#"
            historical_upstream(h) :=
              at("snapshot:last") { upstream("draft.md", h) }.
        "#;
        let program = parse_program("inline", primitive_input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "historical_upstream");
        let mut warnings = Vec::new();
        let err = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_database(),
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect_err("planned TimeScope rejects unsupported primitive");
        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedPrimitive { predicate, .. }
                if predicate.display_name() == "upstream"
        ));

        let derived_input = r#"
            historical_current(h) := *handle{id: h, status: "current"}.
            scoped(h) := at("snapshot:last") { historical_current(h) }.
        "#;
        let program = parse_program("inline", derived_input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "scoped");
        let mut warnings = Vec::new();
        let err = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_database(),
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect_err("planned TimeScope rejects derived predicate");
        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedDerivedPredicate { .. }
        ));

        let nested_input = r#"
            nested_bad(to) :=
              at("snapshot:outer") {
                at("snapshot:inner") { *edge{from: "draft.md", to, kind} }
              }.
        "#;
        let program = parse_program("inline", nested_input).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let planned = plan(&analyzed).expect("program plans");
        let (rule, catalog) = planned_rule(&planned, "nested_bad");
        let mut warnings = Vec::new();
        let err = eval_planned_rule_group(
            rule,
            catalog,
            &time_travel_database(),
            &mut warnings,
            &EvalOptions::default(),
        )
        .expect_err("planned nested TimeScope preserves unsupported inner reference");
        assert!(matches!(
            err,
            EvalError::UnsupportedTimeScopedStoredRelation {
                reference,
                relation,
            } if reference == "snapshot:inner" && relation.as_str() == "edge"
        ));
    }
}
