use std::cmp::Ordering;
use std::collections::btree_set;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::slice;

use serde::Serialize;

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactIdentity, HandleFact, MetaFact,
    SnapshotFact, SpanFact,
};
use crate::ids::Generation;
use crate::runtime::analysis::{AnalyzedProgram, AnalyzedQuery};
use crate::runtime::ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, ComparisonOp, Expr,
    FieldPattern, Head, Ident, Literal, NegatedAtom, NumberLiteral, PredicateRef, Rule, StoredAtom,
    Term,
};
use crate::store::FactStore;

pub type Binding = BTreeMap<Ident, Value>;
type DeltaMap = BTreeMap<PredicateRef, DerivedRelation>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Tuple(pub Vec<Value>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueryOutput {
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum Value {
    String(String),
    Number(NumberValue),
    Bool(bool),
    Null,
    List(Vec<Value>),
}

impl Value {
    fn kind_rank(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Number(_) => 2,
            Self::String(_) => 3,
            Self::List(_) => 4,
        }
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::List(a), Self::List(b)) => a.cmp(b),
            _ => self.kind_rank().cmp(&other.kind_rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum NumberValue {
    Int(i64),
    Float(f64),
}

impl Eq for NumberValue {}

impl Ord for NumberValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.total_cmp(b),
            (Self::Int(_), Self::Float(_)) => Ordering::Less,
            (Self::Float(_), Self::Int(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NumberValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for NumberValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Int(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Self::Float(value) => {
                1_u8.hash(state);
                value.to_bits().hash(state);
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Database {
    stored: BTreeMap<Ident, StoredRelation>,
    derived: BTreeMap<PredicateRef, DerivedRelation>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database")
            .field(
                "stored",
                &self
                    .stored
                    .iter()
                    .map(|(relation, rows)| (relation.to_string(), rows.len()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .field(
                "derived",
                &self
                    .derived
                    .iter()
                    .map(|(predicate, tuples)| (predicate.display_name(), tuples.len()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .finish()
    }
}

impl Database {
    pub fn from_store(store: &FactStore) -> Self {
        let mut db = Self::default();
        db.insert_named_rows("handle", store.handles().iter().map(handle_row));
        db.insert_named_rows("edge", store.edges().iter().map(edge_row));
        db.insert_named_rows("meta", store.meta().iter().map(meta_row));
        db.insert_named_rows("content", store.content().iter().map(content_row));
        db.insert_named_rows("span", store.spans().iter().map(span_row));
        db.insert_named_rows("concern", store.concerns().iter().map(concern_row));
        db.insert_named_rows("config", store.configs().iter().map(config_row));
        db.insert_named_rows("snapshot", store.snapshots().iter().map(snapshot_row));
        db.insert_named_rows(
            "generation",
            store.generations().iter().map(|row| {
                named_row([
                    ("corpus", Value::String(row.corpus.to_string())),
                    ("source", Value::String(row.source.to_string())),
                    ("current", generation_value(row.current)),
                ])
            }),
        );
        db
    }

    pub fn insert_stored_rows(
        &mut self,
        relation: impl Into<String>,
        rows: impl IntoIterator<Item = NamedRow>,
    ) {
        self.insert_named_rows(&relation.into(), rows);
    }

    pub fn derived(&self, predicate: &PredicateRef) -> Option<&BTreeSet<Tuple>> {
        self.derived.get(predicate).map(DerivedRelation::tuples)
    }

    fn ensure_derived(&mut self, predicates: impl IntoIterator<Item = PredicateRef>) {
        for predicate in predicates {
            self.derived.entry(predicate).or_default();
        }
    }

    fn insert_named_rows(&mut self, relation: &str, rows: impl IntoIterator<Item = NamedRow>) {
        self.stored
            .entry(Ident::new_unchecked(relation))
            .or_insert_with(|| StoredRelation::new(Ident::new_unchecked(relation)))
            .extend(rows);
    }
}

pub type NamedRow = BTreeMap<Ident, Value>;

#[derive(Clone, Debug)]
struct StoredRelation {
    relation: Ident,
    rows: Vec<NamedRow>,
    indexes: BTreeMap<Ident, BTreeMap<Value, Vec<usize>>>,
}

impl StoredRelation {
    fn new(relation: Ident) -> Self {
        Self {
            relation,
            rows: Vec::new(),
            indexes: BTreeMap::new(),
        }
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    fn extend(&mut self, rows: impl IntoIterator<Item = NamedRow>) {
        for row in rows {
            self.push(row);
        }
    }

    fn push(&mut self, row: NamedRow) {
        let idx = self.rows.len();
        for (field, value) in &row {
            if !should_index_stored_field(&self.relation, field) {
                continue;
            }
            self.indexes
                .entry(field.clone())
                .or_default()
                .entry(value.clone())
                .or_default()
                .push(idx);
        }
        self.rows.push(row);
    }

    fn candidate_rows(&self, constraints: &[(Ident, Value)]) -> RowCandidates<'_> {
        let mut best = None;
        for (field, value) in constraints {
            if !should_index_stored_field(&self.relation, field) {
                continue;
            }
            let Some(values) = self.indexes.get(field) else {
                return RowCandidates::Empty;
            };
            let Some(indices) = values.get(value) else {
                return RowCandidates::Empty;
            };
            if best.is_none_or(|current: &Vec<usize>| indices.len() < current.len()) {
                best = Some(indices);
            }
        }

        best.map_or_else(
            || RowCandidates::All(self.rows.iter()),
            |indices| RowCandidates::Indexed {
                rows: &self.rows,
                indices: indices.iter(),
            },
        )
    }
}

enum RowCandidates<'a> {
    All(slice::Iter<'a, NamedRow>),
    Indexed {
        rows: &'a [NamedRow],
        indices: slice::Iter<'a, usize>,
    },
    Empty,
}

impl<'a> Iterator for RowCandidates<'a> {
    type Item = &'a NamedRow;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::All(rows) => rows.next(),
            Self::Indexed { rows, indices } => indices.next().map(|idx| &rows[*idx]),
            Self::Empty => None,
        }
    }
}

fn should_index_stored_field(relation: &Ident, field: &Ident) -> bool {
    !matches!(
        (relation.as_str(), field.as_str()),
        ("content", "text")
            | ("span" | "handle", "summary")
            | ("meta" | "config" | "snapshot", "value")
    )
}

#[derive(Clone, Debug, Default)]
struct DerivedRelation {
    tuples: BTreeSet<Tuple>,
    indexes: Vec<BTreeMap<Value, Vec<Tuple>>>,
}

impl DerivedRelation {
    fn len(&self) -> usize {
        self.tuples.len()
    }

    fn tuples(&self) -> &BTreeSet<Tuple> {
        &self.tuples
    }

    fn insert(&mut self, tuple: &Tuple) -> bool {
        if !self.tuples.insert(tuple.clone()) {
            return false;
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

    fn candidate_tuples(&self, constraints: &[(usize, Value)]) -> TupleCandidates<'_> {
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

enum TupleCandidates<'a> {
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

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("unknown stored relation '*{relation}'")]
    UnknownStoredRelation { relation: Ident },
    #[error("unknown derived predicate '{predicate}'")]
    UnknownDerivedPredicate { predicate: PredicateRef },
    #[error("unbound variable '{variable}'")]
    UnboundVariable { variable: Ident },
    #[error("unsupported aggregate '{function:?}'")]
    UnsupportedAggregate { function: AggregateFunction },
    #[error("unsupported time reference '{reference}'")]
    UnsupportedTimeRef { reference: String },
    #[error("unsupported expression")]
    UnsupportedExpression,
    #[error("division by zero")]
    DivisionByZero,
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct Evaluator {
    program: AnalyzedProgram,
    database: Database,
    facts_seeded: bool,
}

impl Evaluator {
    pub fn new(program: AnalyzedProgram, mut database: Database) -> Self {
        database.ensure_derived(program.predicates().cloned());
        Self {
            program,
            database,
            facts_seeded: false,
        }
    }

    pub fn run_fixpoint(&mut self) -> Result<(), EvalError> {
        self.seed_facts()?;
        let strata = self.program.strata().to_vec();
        for stratum in strata {
            let rules = self
                .program
                .rules()
                .filter(|rule| stratum.predicates.contains(&rule.head.predicate))
                .cloned()
                .collect::<Vec<_>>();
            run_rule_group(&mut self.database, &rules)?;
        }
        Ok(())
    }

    pub fn eval_query(&self, query: &AnalyzedQuery) -> Result<QueryOutput, EvalError> {
        let query_ast = query.query();
        if query_ast.local_rules.is_empty() {
            let bindings = eval_body(&query_ast.body, vec![Binding::new()], &self.database)?;
            return Ok(QueryOutput {
                rows: bindings.into_iter().map(binding_to_row).collect(),
            });
        }

        let mut database = self.database.clone();
        database.ensure_derived(query.local_predicates().cloned());
        for stratum in query.local_strata() {
            let rules = query_ast
                .local_rules
                .iter()
                .filter(|rule| stratum.predicates.contains(&rule.head.predicate))
                .cloned()
                .collect::<Vec<_>>();
            run_rule_group(&mut database, &rules)?;
        }
        let bindings = eval_body(&query_ast.body, vec![Binding::new()], &database)?;
        let rows = bindings.into_iter().map(binding_to_row).collect();
        Ok(QueryOutput { rows })
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
            self.database
                .derived
                .entry(fact.predicate.clone())
                .or_default()
                .insert(&tuple);
        }
        self.facts_seeded = true;
        Ok(())
    }
}

fn run_rule_group(database: &mut Database, rules: &[Rule]) -> Result<(), EvalError> {
    let stratum_predicates = rules
        .iter()
        .map(|rule| rule.head.predicate.clone())
        .collect::<BTreeSet<_>>();
    database.ensure_derived(stratum_predicates.iter().cloned());

    let mut delta = DeltaMap::new();
    for rule in rules {
        let tuples = eval_rule(rule, database)?;
        insert_new_tuples(database, &rule.head.predicate, tuples, &mut delta);
    }

    while !delta.is_empty() {
        let previous_delta = delta;
        delta = DeltaMap::new();
        for rule in rules {
            for atom_index in recursive_atom_indexes(&rule.body, &stratum_predicates) {
                let tuples = eval_rule_with_delta(rule, database, &previous_delta, atom_index)?;
                insert_new_tuples(database, &rule.head.predicate, tuples, &mut delta);
            }
        }
    }
    Ok(())
}

fn eval_rule(rule: &Rule, database: &Database) -> Result<Vec<Tuple>, EvalError> {
    let bindings = eval_body(&rule.body, vec![Binding::new()], database)?;
    bindings
        .into_iter()
        .map(|binding| project_head(&rule.head, &binding))
        .collect()
}

fn eval_rule_with_delta(
    rule: &Rule,
    database: &Database,
    delta: &DeltaMap,
    atom_index: usize,
) -> Result<Vec<Tuple>, EvalError> {
    let bindings = eval_body_with_delta(
        &rule.body,
        vec![Binding::new()],
        database,
        Some(DeltaView { delta, atom_index }),
    )?;
    bindings
        .into_iter()
        .map(|binding| project_head(&rule.head, &binding))
        .collect()
}

fn insert_new_tuples(
    database: &mut Database,
    predicate: &PredicateRef,
    tuples: Vec<Tuple>,
    delta: &mut DeltaMap,
) -> bool {
    let relation = database.derived.entry(predicate.clone()).or_default();
    let mut changed = false;
    for tuple in tuples {
        if relation.insert(&tuple) {
            delta.entry(predicate.clone()).or_default().insert(&tuple);
            changed = true;
        }
    }
    changed
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

#[derive(Clone, Copy)]
struct DeltaView<'a> {
    delta: &'a DeltaMap,
    atom_index: usize,
}

fn eval_body(
    body: &Body,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    eval_body_with_delta(body, bindings, database, None)
}

fn eval_body_with_delta(
    body: &Body,
    mut bindings: Vec<Binding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
) -> Result<Vec<Binding>, EvalError> {
    for (atom_index, atom) in body.atoms.iter().enumerate() {
        let atom_delta = delta.filter(|view| view.atom_index == atom_index);
        bindings = eval_atom(atom, bindings, database, atom_delta)?;
    }
    Ok(bindings)
}

fn eval_atom(
    atom: &Atom,
    bindings: Vec<Binding>,
    database: &Database,
    delta: Option<DeltaView<'_>>,
) -> Result<Vec<Binding>, EvalError> {
    match atom {
        Atom::Stored(stored) => eval_stored(stored, bindings, database),
        Atom::Derived(derived) => {
            if let Some(view) = delta {
                eval_derived_from_delta(derived, bindings, view.delta)
            } else {
                eval_derived(derived, bindings, database)
            }
        }
        Atom::Comparison(comparison) => eval_comparison(comparison, bindings),
        Atom::Aggregation(aggregate) => eval_aggregate(aggregate, bindings, database),
        Atom::Negation(negated) => eval_negation(negated, bindings, database),
        Atom::TimeBlock(time_block) => Err(EvalError::UnsupportedTimeRef {
            reference: time_block.reference.clone(),
        }),
    }
}

fn eval_stored(
    atom: &StoredAtom,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    let relation =
        database
            .stored
            .get(&atom.relation)
            .ok_or_else(|| EvalError::UnknownStoredRelation {
                relation: atom.relation.clone(),
            })?;
    let mut out = Vec::new();
    for binding in bindings {
        let constraints = stored_constraints(&atom.fields, &binding)?;
        for row in relation.candidate_rows(&constraints) {
            if let Some(next) = unify_stored_fields(&atom.fields, row, &binding)? {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn eval_derived(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    let relation = database.derived.get(&atom.predicate).ok_or_else(|| {
        EvalError::UnknownDerivedPredicate {
            predicate: atom.predicate.clone(),
        }
    })?;
    eval_derived_from_relation(atom, bindings, relation)
}

fn eval_derived_from_delta(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    delta: &DeltaMap,
) -> Result<Vec<Binding>, EvalError> {
    let Some(relation) = delta.get(&atom.predicate) else {
        return Ok(Vec::new());
    };
    eval_derived_from_relation(atom, bindings, relation)
}

fn eval_derived_from_relation(
    atom: &crate::runtime::ast::DerivedAtom,
    bindings: Vec<Binding>,
    relation: &DerivedRelation,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let constraints = call_constraints(&atom.args, &binding)?;
        for tuple in relation.candidate_tuples(&constraints) {
            if tuple.0.len() != atom.args.len() {
                continue;
            }
            if let Some(next) = unify_call_args(&atom.args, tuple, &binding)? {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn eval_comparison(
    comparison: &Comparison,
    bindings: Vec<Binding>,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let left = eval_expr(&comparison.left, &binding)?;
        let right = eval_expr(&comparison.right, &binding)?;
        if compare(&left, comparison.op, &right)? {
            out.push(binding);
        }
    }
    Ok(out)
}

fn eval_negation(
    negated: &NegatedAtom,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    for binding in bindings {
        let atom = match negated {
            NegatedAtom::Stored(stored) => Atom::Stored(stored.clone()),
            NegatedAtom::Derived(derived) => Atom::Derived(derived.clone()),
        };
        let matches = eval_atom(&atom, vec![binding.clone()], database, None)?;
        if matches.is_empty() {
            out.push(binding);
        }
    }
    Ok(out)
}

fn eval_aggregate(
    aggregate: &Aggregate,
    bindings: Vec<Binding>,
    database: &Database,
) -> Result<Vec<Binding>, EvalError> {
    if aggregate.function != AggregateFunction::Count {
        return Err(EvalError::UnsupportedAggregate {
            function: aggregate.function,
        });
    }
    let Expr::Var(result_var) = &aggregate.result else {
        return Err(EvalError::UnsupportedExpression);
    };
    let Expr::Var(value_var) = &aggregate.value else {
        return Err(EvalError::UnsupportedExpression);
    };

    let mut out = Vec::new();
    for binding in bindings {
        let inner = eval_body(&aggregate.body, vec![binding.clone()], database)?;
        let mut groups: BTreeMap<Binding, BTreeSet<Value>> = BTreeMap::new();
        for mut row in inner {
            let Some(value) = row.remove(value_var) else {
                return Err(EvalError::UnboundVariable {
                    variable: value_var.clone(),
                });
            };
            row.remove(result_var);
            groups.entry(row).or_default().insert(value);
        }
        for (mut group, values) in groups {
            group.insert(
                result_var.clone(),
                Value::Number(NumberValue::Int(
                    i64::try_from(values.len()).unwrap_or(i64::MAX),
                )),
            );
            out.push(group);
        }
    }
    Ok(out)
}

fn stored_constraints(
    fields: &[FieldPattern],
    binding: &Binding,
) -> Result<Vec<(Ident, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for field in fields {
        if let Some(value) = bound_value_for_term(&field.term, binding)? {
            constraints.push((field.field.clone(), value));
        }
    }
    Ok(constraints)
}

fn call_constraints(args: &[CallArg], binding: &Binding) -> Result<Vec<(usize, Value)>, EvalError> {
    let mut constraints = Vec::new();
    for (idx, arg) in args.iter().enumerate() {
        if let Some(value) = bound_value_for_expr(arg.expr(), binding)? {
            constraints.push((idx, value));
        }
    }
    Ok(constraints)
}

fn bound_value_for_term(term: &Term, binding: &Binding) -> Result<Option<Value>, EvalError> {
    match term {
        Term::Wildcard => Ok(None),
        Term::Expr(expr) => bound_value_for_expr(expr, binding),
    }
}

fn bound_value_for_expr(expr: &Expr, binding: &Binding) -> Result<Option<Value>, EvalError> {
    match expr {
        Expr::Var(var) => Ok(binding.get(var).cloned()),
        _ if expr_is_bound(expr, binding) => eval_expr(expr, binding).map(Some),
        _ => Ok(None),
    }
}

fn expr_is_bound(expr: &Expr, binding: &Binding) -> bool {
    let mut vars = BTreeSet::new();
    expr.variables(&mut vars);
    vars.iter().all(|var| binding.contains_key(var))
}

fn unify_stored_fields(
    fields: &[FieldPattern],
    row: &NamedRow,
    binding: &Binding,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    for field in fields {
        let Some(value) = row.get(&field.field) else {
            return Ok(None);
        };
        if !unify_term(&field.term, value, binding, &mut next)? {
            return Ok(None);
        }
    }
    Ok(Some(next.unwrap_or_else(|| binding.clone())))
}

fn unify_call_args(
    args: &[CallArg],
    tuple: &Tuple,
    binding: &Binding,
) -> Result<Option<Binding>, EvalError> {
    let mut next = None;
    for (arg, value) in args.iter().zip(&tuple.0) {
        match arg {
            CallArg::Positional(expr) => {
                if !unify_expr(expr, value, binding, &mut next)? {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(next.unwrap_or_else(|| binding.clone())))
}

fn unify_term(
    term: &Term,
    value: &Value,
    binding: &Binding,
    next: &mut Option<Binding>,
) -> Result<bool, EvalError> {
    match term {
        Term::Wildcard => Ok(true),
        Term::Expr(expr) => unify_expr(expr, value, binding, next),
    }
}

fn unify_expr(
    expr: &Expr,
    value: &Value,
    binding: &Binding,
    next: &mut Option<Binding>,
) -> Result<bool, EvalError> {
    match expr {
        Expr::Var(var) => {
            if let Some(existing) = active_binding(binding, next.as_ref()).get(var) {
                Ok(existing == value)
            } else {
                writable_binding(binding, next).insert(var.clone(), value.clone());
                Ok(true)
            }
        }
        _ => Ok(eval_expr(expr, active_binding(binding, next.as_ref()))? == *value),
    }
}

fn active_binding<'a>(binding: &'a Binding, next: Option<&'a Binding>) -> &'a Binding {
    next.map_or(binding, |next| next)
}

fn writable_binding<'a>(binding: &Binding, next: &'a mut Option<Binding>) -> &'a mut Binding {
    next.get_or_insert_with(|| binding.clone())
}

fn project_head(head: &Head, binding: &Binding) -> Result<Tuple, EvalError> {
    let mut values = Vec::with_capacity(head.terms.len());
    for term in &head.terms {
        match term {
            Term::Wildcard => values.push(Value::Null),
            Term::Expr(expr) => values.push(eval_expr(expr, binding)?),
        }
    }
    Ok(Tuple(values))
}

fn project_fact_head(head: &Head) -> Result<Tuple, EvalError> {
    project_head(head, &Binding::new())
}

fn eval_expr(expr: &Expr, binding: &Binding) -> Result<Value, EvalError> {
    match expr {
        Expr::Var(var) => binding
            .get(var)
            .cloned()
            .ok_or_else(|| EvalError::UnboundVariable {
                variable: var.clone(),
            }),
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::Binary { left, op, right } => eval_binary(left, *op, right, binding),
        Expr::Tuple(items) => items
            .iter()
            .map(|item| eval_expr(item, binding))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Expr::FunctionCall { .. } => Err(EvalError::UnsupportedExpression),
    }
}

fn eval_binary(
    left: &Expr,
    op: crate::runtime::ast::ArithmeticOp,
    right: &Expr,
    binding: &Binding,
) -> Result<Value, EvalError> {
    let left = eval_expr(left, binding)?;
    let right = eval_expr(right, binding)?;
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

fn value_from_literal(literal: &Literal) -> Value {
    match literal {
        Literal::String(value) => Value::String(value.clone()),
        Literal::Number(NumberLiteral::Int(value)) => Value::Number(NumberValue::Int(*value)),
        Literal::Number(NumberLiteral::Float(value)) => Value::Number(NumberValue::Float(*value)),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Null => Value::Null,
        Literal::List(items) => Value::List(items.iter().map(value_from_literal).collect()),
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
        ComparisonOp::Matches => return Err(EvalError::UnsupportedExpression),
    };
    Ok(result)
}

fn binding_to_row(binding: Binding) -> Row {
    Row {
        fields: binding
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    }
}

fn named_row(entries: impl IntoIterator<Item = (&'static str, Value)>) -> NamedRow {
    entries
        .into_iter()
        .map(|(key, value)| (Ident::new_unchecked(key), value))
        .collect()
}

fn source_fact_row(
    identity: &FactIdentity,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) -> NamedRow {
    let mut row = identity_row(identity);
    row.extend(named_row(entries));
    row
}

fn identity_row(identity: &FactIdentity) -> NamedRow {
    named_row([
        ("corpus", Value::String(identity.corpus.to_string())),
        ("source", Value::String(identity.source.to_string())),
        ("native_id", Value::String(identity.native_id.to_string())),
        ("origin_uri", Value::String(identity.origin_uri.to_string())),
        ("revision", Value::String(identity.revision.to_string())),
        ("generation", generation_value(identity.generation)),
    ])
}

fn opt_string(value: Option<&String>) -> Value {
    value.cloned().map_or(Value::Null, Value::String)
}

fn handle_row(fact: &HandleFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("id", Value::String(fact.id.clone())),
            ("kind", Value::String(fact.kind.clone())),
            ("status", opt_string(fact.status.as_ref())),
            ("namespace", Value::String(fact.namespace.clone())),
            ("file", Value::String(fact.file.clone())),
            (
                "line",
                Value::Number(NumberValue::Int(i64::from(fact.line))),
            ),
            ("date", opt_string(fact.date.as_ref())),
            ("area", Value::String(fact.area.clone())),
            ("summary", Value::String(fact.summary.clone())),
        ],
    )
}

fn edge_row(fact: &EdgeFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("from", Value::String(fact.from.clone())),
            ("to", Value::String(fact.to.clone())),
            ("kind", Value::String(fact.kind.clone())),
            ("file", Value::String(fact.file.clone())),
            (
                "line",
                Value::Number(NumberValue::Int(i64::from(fact.line))),
            ),
        ],
    )
}

fn meta_row(fact: &MetaFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("handle", Value::String(fact.handle.clone())),
            ("key", Value::String(fact.key.clone())),
            ("value", Value::String(fact.value.clone())),
        ],
    )
}

fn content_row(fact: &ContentFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("handle", Value::String(fact.handle.clone())),
            ("span_id", Value::String(fact.span_id.clone())),
            (
                "lines",
                Value::Number(NumberValue::Int(i64::from(fact.lines))),
            ),
            ("text", Value::String(fact.text.clone())),
            (
                "tokens",
                Value::Number(NumberValue::Int(i64::from(fact.tokens))),
            ),
        ],
    )
}

fn span_row(fact: &SpanFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("id", Value::String(fact.id.clone())),
            ("handle", Value::String(fact.handle.clone())),
            (
                "start_line",
                Value::Number(NumberValue::Int(i64::from(fact.start_line))),
            ),
            (
                "end_line",
                Value::Number(NumberValue::Int(i64::from(fact.end_line))),
            ),
            ("summary", Value::String(fact.summary.clone())),
        ],
    )
}

fn concern_row(fact: &ConcernFact) -> NamedRow {
    source_fact_row(
        &fact.identity,
        [
            ("name", Value::String(fact.name.clone())),
            ("member", Value::String(fact.member.clone())),
        ],
    )
}

fn config_row(fact: &ConfigFact) -> NamedRow {
    named_row([
        ("corpus", Value::String(fact.corpus.to_string())),
        ("key", Value::String(fact.key.clone())),
        ("value", Value::String(fact.value.clone())),
    ])
}

fn snapshot_row(fact: &SnapshotFact) -> NamedRow {
    named_row([
        ("corpus", Value::String(fact.corpus.to_string())),
        ("at", Value::String(fact.at.clone())),
        ("id", Value::String(fact.id.clone())),
        ("key", Value::String(fact.key.clone())),
        ("value", Value::String(fact.value.clone())),
    ])
}

fn generation_value(generation: Generation) -> Value {
    Value::Number(NumberValue::Int(
        i64::try_from(generation.get()).unwrap_or(i64::MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{FactBatch, FactBatchMode, FactIdentity};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::runtime::{analyze, parse_program};

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
        HandleFact {
            identity: identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: Some(status.to_string()),
            namespace: namespace.to_string(),
            file: format!("{area}/{id}.md"),
            line: 1,
            date: None,
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
        }
    }

    fn chain_store(edge_count: usize) -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.edges = (0..edge_count)
            .map(|idx| edge(&format!("n{idx}"), &format!("n{}", idx + 1), "DependsOn"))
            .collect();
        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merge");
        store
    }

    fn fixture_store() -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("fixture"),
            FactBatchMode::FullSnapshot,
            Generation::initial(),
        );
        batch.handles = vec![
            handle("v17", "file", "current", "", "formal-model"),
            handle("v16", "file", "superseded", "", "formal-model"),
            handle("jit", "file", "draft", "", "compiler"),
            handle("OQ-22", "label", "open", "OQ", "formal-model"),
            handle("OQ-99", "label", "resolved", "OQ", "compiler"),
        ];
        batch.edges = vec![
            edge("v17", "v16", "Supersedes"),
            edge("jit", "OQ-22", "DependsOn"),
            edge("v17", "OQ-22", "DependsOn"),
        ];
        let mut store = FactStore::default();
        store.merge(batch).expect("fixture merge");
        store
    }

    #[test]
    fn stored_relation_uses_bound_field_candidates() {
        let database = Database::from_store(&fixture_store());
        let relation = database
            .stored
            .get(&Ident::new_unchecked("handle"))
            .expect("handle relation");
        let candidates = relation
            .candidate_rows(&[(Ident::new_unchecked("id"), Value::String("v17".to_string()))])
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].get(&Ident::new_unchecked("id")),
            Some(&Value::String("v17".to_string()))
        );
    }

    #[test]
    fn fixed_point_evaluates_recursion_negation_and_count() {
        let program = parse_program(
            "fixture",
            r#"
            terminal(h) := *handle{id: h, status: "resolved"}.
            terminal(h) := *handle{id: h, status: "superseded"}.
            open_oq(h) := *handle{id: h, kind: "label", namespace: "OQ"}, not terminal(h).
            upstream(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            upstream(h, anc) := *edge{from: h, to: mid, kind: "DependsOn"}, upstream(mid, anc).
            oq_per_area(area, n) := n = Count{ h : open_oq(h), *handle{id: h, area} }.
            ? open_oq(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let open_oq = PredicateRef::new(Ident::new_unchecked("open_oq"));
        let rows = evaluator.database().derived(&open_oq).expect("open_oq");
        assert_eq!(rows.len(), 1);
        assert!(rows.contains(&Tuple(vec![Value::String("OQ-22".to_string())])));

        let oq_per_area = PredicateRef::new(Ident::new_unchecked("oq_per_area"));
        let counts = evaluator.database().derived(&oq_per_area).expect("counts");
        assert!(counts.contains(&Tuple(vec![
            Value::String("formal-model".to_string()),
            Value::Number(NumberValue::Int(1)),
        ])));

        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
    }

    #[test]
    fn derived_relation_uses_bound_position_candidates() {
        let program = parse_program(
            "fixture",
            r#"
            upstream(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            ? upstream("v17", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let relation = evaluator
            .database
            .derived
            .get(&PredicateRef::new(Ident::new_unchecked("upstream")))
            .expect("upstream relation");
        let candidates = relation
            .candidate_tuples(&[(0, Value::String("v17".to_string()))])
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn stored_index_preserves_same_atom_expression_unification() {
        let program =
            parse_program("fixture", r"? *pair{n: x, next: x + 1}.").expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut database = Database::default();
        database.insert_stored_rows(
            "pair",
            [
                named_row([
                    ("n", Value::Number(NumberValue::Int(1))),
                    ("next", Value::Number(NumberValue::Int(2))),
                ]),
                named_row([
                    ("n", Value::Number(NumberValue::Int(1))),
                    ("next", Value::Number(NumberValue::Int(3))),
                ]),
            ],
        );
        let evaluator = Evaluator::new(analyzed, database);
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("x"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn derived_index_preserves_same_atom_expression_unification() {
        let program = parse_program("fixture", r"seed(1, 2). seed(1, 3). ? seed(x, x + 1).")
            .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("x"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn semi_naive_recursion_handles_chain_closure() {
        let program = parse_program(
            "fixture",
            r#"
            upstream(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            upstream(h, anc) := upstream(h, mid), *edge{from: mid, to: anc, kind: "DependsOn"}.
            ? upstream("n0", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&chain_store(256)));
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 256);
        assert!(output.rows.iter().any(|row| {
            row.fields
                .get("anc")
                .is_some_and(|value| value == &Value::String("n256".to_string()))
        }));
    }

    #[test]
    fn facts_are_seeded_as_derived_tuples() {
        let program =
            parse_program("fixture", r#"seed("alpha"). ? seed(value)."#).expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("value"),
            Some(&Value::String("alpha".to_string()))
        );
    }

    #[test]
    fn positive_recursion_is_not_rule_order_dependent() {
        let program = parse_program(
            "fixture",
            r#"
            upstream(h, anc) := *edge{from: h, to: mid, kind: "DependsOn"}, upstream(mid, anc).
            upstream(h, anc) := *edge{from: h, to: anc, kind: "DependsOn"}.
            ? upstream("v17", anc).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        evaluator.run_fixpoint().expect("fixpoint");
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].fields.get("anc"),
            Some(&Value::String("OQ-22".to_string()))
        );
    }

    #[test]
    fn query_local_rules_execute() {
        let program = parse_program(
            "fixture",
            r#"
            ?
              where local_oq(h) := *handle{id: h, namespace: "OQ"}.
              local_oq(h).
            "#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 2);
    }

    #[test]
    fn source_identity_fields_are_queryable_on_source_facts() {
        let program = parse_program(
            "fixture",
            r#"? *handle{id: "v17", corpus, source, native_id, origin_uri, revision, generation}."#,
        )
        .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().cloned().expect("query exists");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        assert_eq!(output.rows.len(), 1);
        let row = &output.rows[0].fields;
        assert_eq!(row.get("corpus"), Some(&Value::String("test".to_string())));
        assert_eq!(
            row.get("source"),
            Some(&Value::String("fixture".to_string()))
        );
        assert_eq!(
            row.get("native_id"),
            Some(&Value::String("v17".to_string()))
        );
        assert_eq!(
            row.get("origin_uri"),
            Some(&Value::String("fixture://v17".to_string()))
        );
        assert_eq!(row.get("revision"), Some(&Value::String("rev".to_string())));
        assert_eq!(
            row.get("generation"),
            Some(&Value::Number(NumberValue::Int(1)))
        );
    }

    #[test]
    fn query_rows_are_deterministic_by_variable_name() {
        let program = parse_program("fixture", r"? *handle{id: h, area}.").expect("parse");
        let analyzed = analyze(program).expect("analyze");
        let query = analyzed.queries().next().cloned().expect("query");
        let evaluator = Evaluator::new(analyzed, Database::from_store(&fixture_store()));
        let output = evaluator.eval_query(&query).expect("query");
        let first = output.rows.first().expect("row");
        let keys = first.fields.keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys, vec!["area", "h"]);
    }
}
