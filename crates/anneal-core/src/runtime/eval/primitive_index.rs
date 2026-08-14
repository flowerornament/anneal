//! Precomputed state serving engine primitive predicates.
//!
//! Graph, lifecycle, pipeline, snapshot, content, and time predicates share
//! one tuple-derived representation here. The parent evaluator can construct
//! it, apply runtime context, scope it, and query it; representation state and
//! all other accessors remain private to this module.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::config_schema::{RuntimeConfigKey, runtime_config_key_for_config_key};
use crate::facts::{
    CONFIG_RELATION_NAME as CONFIG_RELATION, CONTENT_RELATION_NAME as CONTENT_RELATION,
    EDGE_RELATION_NAME as EDGE_RELATION, HANDLE_RELATION_NAME as HANDLE_RELATION,
    SNAPSHOT_RELATION_NAME as SNAPSHOT_RELATION,
};
use crate::ids::HandleId;
use crate::impact::ImpactTraversalPolicy;
use crate::ir::ids::RowId;
use crate::lifecycle::{
    CANONICAL_PIPELINE_ORDERING, CANONICAL_SETTLED_STATUSES, TERMINAL_STATUS_HEURISTICS,
    canonical_pipeline_position, is_canonical_settled_status, is_terminal_status,
};
use crate::repository::RepositoryContext;
use crate::runtime::primitives::PrimitivePredicate;
use crate::time::{current_days_since_epoch, iso_days_since_epoch, snapshot_days_since_epoch};
use crate::vm::store::{TupleDb, TupleRow};
use crate::vm::view::SnapshotSelection;

use super::{
    AT_FIELD, ArgConstraint, CITES_EDGE_KIND, DATE_FIELD, DISCHARGES_EDGE_KIND, DepthLimit,
    FILE_FIELD, FROM_FIELD, HANDLE_FIELD, ID_FIELD, KEY_FIELD, KIND_FIELD, LABEL_KIND,
    LINEAR_NAMESPACE_RELATION, NAMESPACE_FIELD, ORDINAL_FIELD, STATUS_FIELD, TO_FIELD,
    TOKENS_FIELD, Tuple, VALUE_FIELD, Value, depth_limit, i64_constraint, int_value,
    string_constraint, string_value, tuple_matches_constraints,
};

const PRIMITIVE_INDEX_CONFIG_KEYS: &[RuntimeConfigKey] = &[
    RuntimeConfigKey::ConvergenceActive,
    RuntimeConfigKey::ConvergenceTerminal,
    RuntimeConfigKey::ConvergenceSettled,
    RuntimeConfigKey::ConvergenceOrdering,
    RuntimeConfigKey::HandlesLinear,
];

#[derive(Clone, Debug, Default)]
pub(super) struct PrimitiveIndex {
    nodes: BTreeSet<HandleId>,
    handles: BTreeMap<HandleId, HandleState>,
    outgoing: BTreeMap<HandleId, BTreeSet<HandleId>>,
    incoming: BTreeMap<HandleId, BTreeSet<HandleId>>,
    outgoing_edges: BTreeMap<HandleId, BTreeSet<(String, HandleId)>>,
    incoming_edges: BTreeMap<HandleId, BTreeSet<(String, HandleId)>>,
    impact_traverse: ImpactTraversalPolicy,
    out_edge_count: BTreeMap<HandleId, usize>,
    in_edge_count: BTreeMap<HandleId, usize>,
    cite_count: BTreeMap<HandleId, usize>,
    discharge_count: BTreeMap<HandleId, usize>,
    content_tokens: BTreeMap<HandleId, usize>,
    active_statuses: BTreeSet<String>,
    terminal_statuses: BTreeSet<String>,
    settled_statuses: BTreeSet<String>,
    pipeline_positions: BTreeMap<String, i64>,
    linear_namespaces: BTreeSet<String>,
    status_snapshots: BTreeMap<HandleId, Vec<SnapshotStatus>>,
    git_mtimes: BTreeMap<String, String>,
    repository: Option<RepositoryContext>,
    evaluation_day: Option<i64>,
}

pub(super) enum PrimitiveIndexContext {
    GitMtimes(BTreeMap<String, String>),
    Repository(RepositoryContext),
    EvaluationDay(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HandleState {
    kind: String,
    status: Option<String>,
    namespace: String,
    file: String,
    date: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotStatus {
    day: i64,
    sort_at: String,
    status: String,
}

fn freshness_days(state: &HandleState, today: Option<i64>) -> i64 {
    let (Some(date), Some(today)) = (state.date, today) else {
        return 0;
    };
    today.saturating_sub(date).max(0)
}

impl PrimitiveIndex {
    pub(super) fn from_tuples(tuples: &TupleDb) -> Self {
        let mut index = Self::default();
        tuples.for_each_relation_row(|relation, row| index.insert_tuple_row(relation, row));
        index
    }

    pub(super) fn apply_context(&mut self, context: PrimitiveIndexContext) {
        match context {
            PrimitiveIndexContext::GitMtimes(mtimes) => self.git_mtimes = mtimes,
            PrimitiveIndexContext::Repository(repository) => self.repository = Some(repository),
            PrimitiveIndexContext::EvaluationDay(day) => self.evaluation_day = Some(day),
        }
    }

    fn insert_tuple_row(&mut self, relation: &str, row: TupleRow<'_>) {
        match relation {
            HANDLE_RELATION => {
                if let Some(id) = row.string(ID_FIELD) {
                    self.insert_handle(
                        id,
                        HandleState {
                            kind: row.string(KIND_FIELD).unwrap_or_default().to_owned(),
                            status: row.string(STATUS_FIELD).map(str::to_owned),
                            namespace: row.string(NAMESPACE_FIELD).unwrap_or_default().to_owned(),
                            file: row.string(FILE_FIELD).unwrap_or_default().to_owned(),
                            date: row.string(DATE_FIELD).and_then(iso_days_since_epoch),
                        },
                    );
                }
            }
            EDGE_RELATION => {
                let (Some(from), Some(to)) = (row.string(FROM_FIELD), row.string(TO_FIELD)) else {
                    return;
                };
                self.insert_edge(from, to, row.string(KIND_FIELD));
            }
            CONFIG_RELATION => self.insert_config_tuple(row),
            CONTENT_RELATION => {
                let (Some(handle), Some(tokens)) =
                    (row.string(HANDLE_FIELD), row.i64(TOKENS_FIELD))
                else {
                    return;
                };
                self.insert_content_tokens(handle, tokens);
            }
            SNAPSHOT_RELATION => {
                let (Some(id), Some(key), Some(status), Some(at)) = (
                    row.string(ID_FIELD),
                    row.string(KEY_FIELD),
                    row.string(VALUE_FIELD),
                    row.string(AT_FIELD),
                ) else {
                    return;
                };
                let Some(day) = snapshot_days_since_epoch(at) else {
                    return;
                };
                if key == STATUS_FIELD {
                    self.insert_status_snapshot(
                        id,
                        SnapshotStatus {
                            day,
                            sort_at: at.to_owned(),
                            status: status.to_owned(),
                        },
                    );
                }
            }
            LINEAR_NAMESPACE_RELATION => {
                if let Some(namespace) = row.string(NAMESPACE_FIELD) {
                    self.linear_namespaces.insert(namespace.to_owned());
                }
            }
            _ => {}
        }
    }

    fn insert_handle(&mut self, id: &str, state: HandleState) {
        let Ok(id) = HandleId::new(id) else {
            return;
        };
        self.nodes.insert(id.clone());
        self.handles.insert(id, state);
    }

    fn insert_edge(&mut self, from: &str, to: &str, kind: Option<&str>) {
        let (Ok(from), Ok(to)) = (HandleId::new(from), HandleId::new(to)) else {
            return;
        };
        self.nodes.insert(from.clone());
        self.nodes.insert(to.clone());
        self.outgoing
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        self.incoming
            .entry(to.clone())
            .or_default()
            .insert(from.clone());
        if let Some(kind) = kind {
            self.outgoing_edges
                .entry(from.clone())
                .or_default()
                .insert((kind.to_owned(), to.clone()));
            self.incoming_edges
                .entry(to.clone())
                .or_default()
                .insert((kind.to_owned(), from.clone()));
        }
        *self.out_edge_count.entry(from).or_default() += 1;
        *self.in_edge_count.entry(to.clone()).or_default() += 1;
        if kind == Some(CITES_EDGE_KIND) {
            *self.cite_count.entry(to).or_default() += 1;
        } else if kind == Some(DISCHARGES_EDGE_KIND) {
            *self.discharge_count.entry(to).or_default() += 1;
        }
    }

    fn insert_content_tokens(&mut self, handle: &str, tokens: i64) {
        let Ok(handle) = HandleId::new(handle) else {
            return;
        };
        *self.content_tokens.entry(handle).or_default() += usize::try_from(tokens).unwrap_or(0);
    }

    fn insert_status_snapshot(&mut self, handle: &str, snapshot: SnapshotStatus) {
        let Ok(handle) = HandleId::new(handle) else {
            return;
        };
        let snapshots = self.status_snapshots.entry(handle).or_default();
        let idx = snapshots
            .binary_search_by(|probe| {
                probe
                    .day
                    .cmp(&snapshot.day)
                    .then_with(|| probe.sort_at.cmp(&snapshot.sort_at))
                    .then_with(|| probe.status.cmp(&snapshot.status))
            })
            .unwrap_or_else(|idx| idx);
        snapshots.insert(idx, snapshot);
    }

    pub(super) fn scoped_to_snapshot_tuples(
        &self,
        tuples: &TupleDb,
        selection: &SnapshotSelection,
    ) -> Self {
        let mut graph = self.clone();
        graph.evaluation_day = Some(selection.day);
        graph.apply_snapshot_tuple_rows(tuples, &selection.tuple_rows);
        graph
    }

    fn apply_snapshot_tuple_rows(&mut self, tuples: &TupleDb, snapshot_rows: &[RowId]) {
        for row in snapshot_rows {
            let Some(row) = tuples.tuple_row(SNAPSHOT_RELATION, *row) else {
                continue;
            };
            let (Some(id), Some(key), Some(value)) = (
                row.string(ID_FIELD),
                row.string(KEY_FIELD),
                row.string(VALUE_FIELD),
            ) else {
                continue;
            };
            let Some(state) = self.handles.get_mut(id) else {
                continue;
            };
            match key {
                KIND_FIELD => value.clone_into(&mut state.kind),
                STATUS_FIELD => state.status = Some(value.to_owned()),
                NAMESPACE_FIELD => value.clone_into(&mut state.namespace),
                DATE_FIELD => state.date = iso_days_since_epoch(value),
                _ => {}
            }
        }
    }

    fn insert_config_tuple(&mut self, row: TupleRow<'_>) {
        let (Some(key), Some(value)) = (row.string(KEY_FIELD), row.string(VALUE_FIELD)) else {
            return;
        };
        self.insert_config_values(key, value, row.i64(ORDINAL_FIELD));
    }

    fn insert_config_values(&mut self, key: &str, value: &str, ordinal: Option<i64>) {
        let Some(config_key) = runtime_config_key_for_config_key(key) else {
            self.impact_traverse.insert_config(key, value);
            return;
        };
        if !PRIMITIVE_INDEX_CONFIG_KEYS.contains(&config_key) {
            self.impact_traverse.insert_config(key, value);
            return;
        }
        match config_key {
            RuntimeConfigKey::ConvergenceActive => {
                self.active_statuses.insert(value.to_owned());
            }
            RuntimeConfigKey::ConvergenceTerminal => {
                self.terminal_statuses.insert(value.to_owned());
            }
            RuntimeConfigKey::ConvergenceSettled => {
                self.settled_statuses.insert(value.to_owned());
            }
            RuntimeConfigKey::ConvergenceOrdering => {
                let position = ordinal.unwrap_or_else(|| {
                    i64::try_from(self.pipeline_positions.len()).unwrap_or(i64::MAX)
                });
                self.pipeline_positions
                    .entry(value.to_owned())
                    .and_modify(|existing| *existing = (*existing).min(position))
                    .or_insert(position);
            }
            RuntimeConfigKey::HandlesLinear => {
                self.linear_namespaces.insert(value.to_owned());
            }
            _ => unreachable!("graph-index config key set and match arms stay aligned"),
        }
    }

    pub(super) fn tuples(
        &self,
        primitive: PrimitivePredicate,
        constraints: &[(usize, Value)],
    ) -> Vec<Tuple> {
        match primitive {
            PrimitivePredicate::Upstream => {
                self.directional_pairs(constraints, Direction::Outgoing, Direction::Incoming)
            }
            PrimitivePredicate::Downstream => {
                self.directional_pairs(constraints, Direction::Incoming, Direction::Outgoing)
            }
            PrimitivePredicate::Impact => self.impact_tuples(constraints),
            PrimitivePredicate::Neighborhood => self.neighborhood_tuples(constraints),
            PrimitivePredicate::Terminal => self.lifecycle_tuples(constraints, Self::is_terminal),
            PrimitivePredicate::Active => self.lifecycle_tuples(constraints, Self::is_active),
            PrimitivePredicate::Settled => self.lifecycle_tuples(constraints, Self::is_settled),
            PrimitivePredicate::LifecycleStatusClassification => {
                self.lifecycle_status_classification_tuples(constraints)
            }
            PrimitivePredicate::PipelinePosition => self.pipeline_position_tuples(constraints),
            PrimitivePredicate::PipelinePositionFor => {
                self.pipeline_position_for_tuples(constraints)
            }
            PrimitivePredicate::Obligation => {
                self.lifecycle_tuples(constraints, Self::is_obligation)
            }
            PrimitivePredicate::Discharged => {
                self.lifecycle_tuples(constraints, Self::is_discharged)
            }
            PrimitivePredicate::Undischarged => {
                self.lifecycle_tuples(constraints, Self::is_undischarged)
            }
            PrimitivePredicate::CiteCount => self.count_tuples(constraints, &self.cite_count),
            PrimitivePredicate::InDegree => self.count_tuples(constraints, &self.in_edge_count),
            PrimitivePredicate::OutDegree => self.count_tuples(constraints, &self.out_edge_count),
            PrimitivePredicate::DischargeCount => {
                self.handle_count_tuples(constraints, &self.discharge_count)
            }
            PrimitivePredicate::Freshness => self.freshness_tuples(constraints),
            PrimitivePredicate::Flux => self.flux_tuples(constraints),
            PrimitivePredicate::GitMtime => self.git_mtime_tuples(constraints),
            PrimitivePredicate::ChangedWithin => self.changed_within_tuples(constraints),
            PrimitivePredicate::RepositoryOperationCapability => {
                self.repository_operation_capability_tuples(constraints)
            }
            PrimitivePredicate::TokenEstimate => {
                self.handle_count_tuples(constraints, &self.content_tokens)
            }
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
            | PrimitivePredicate::Sources => Vec::new(),
        }
    }

    fn directional_pairs(
        &self,
        constraints: &[(usize, Value)],
        from_direction: Direction,
        to_direction: Direction,
    ) -> Vec<Tuple> {
        let left = string_constraint(constraints, 0);
        let right = string_constraint(constraints, 1);
        match (left, right) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .reachable_from(start, from_direction, None)
                .into_iter()
                .map(|step| Tuple(vec![string_value(start), string_value(&step.node)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .reachable_from(end, to_direction, None)
                .into_iter()
                .map(|step| Tuple(vec![string_value(&step.node), string_value(end)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.reachable_from(start.as_str(), from_direction, None)
                        .into_iter()
                        .map(move |step| Tuple(vec![string_value(start), string_value(&step.node)]))
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn impact_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let root = string_constraint(constraints, 0);
        let impacted = string_constraint(constraints, 1);
        let depth = i64_constraint(constraints, 2);
        let max_depth = match depth_limit(depth) {
            DepthLimit::Unbounded => None,
            DepthLimit::Max(value) => Some(value),
            DepthLimit::Impossible => return Vec::new(),
        };
        match (root, impacted) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .impact_reachable_from(start, Direction::Incoming, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(start),
                        string_value(&step.node),
                        int_value(step.depth),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .impact_reachable_from(end, Direction::Outgoing, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(&step.node),
                        string_value(end),
                        int_value(step.depth),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.impact_reachable_from(start.as_str(), Direction::Incoming, max_depth)
                        .into_iter()
                        .map(move |step| {
                            Tuple(vec![
                                string_value(start),
                                string_value(&step.node),
                                int_value(step.depth),
                            ])
                        })
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn neighborhood_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let root = string_constraint(constraints, 0);
        let depth = i64_constraint(constraints, 1);
        let member = string_constraint(constraints, 2);
        let max_depth = match depth_limit(depth) {
            DepthLimit::Unbounded => None,
            DepthLimit::Max(value) => Some(value),
            DepthLimit::Impossible => return Vec::new(),
        };
        match (root, member) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(start), _) => self
                .neighborhood_from(start, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(start),
                        int_value(step.depth),
                        string_value(&step.node),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Exact(end)) => self
                .neighborhood_from(end, max_depth)
                .into_iter()
                .map(|step| {
                    Tuple(vec![
                        string_value(&step.node),
                        int_value(step.depth),
                        string_value(end),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, ArgConstraint::Any) => self
                .nodes
                .iter()
                .flat_map(|start| {
                    self.neighborhood_from(start.as_str(), max_depth)
                        .into_iter()
                        .map(move |step| {
                            Tuple(vec![
                                string_value(start),
                                int_value(step.depth),
                                string_value(&step.node),
                            ])
                        })
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn lifecycle_tuples(
        &self,
        constraints: &[(usize, Value)],
        predicate: fn(&Self, &HandleId, &HandleState) -> bool,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(id) => self
                .handles
                .get_key_value(id)
                .filter(|(id, state)| predicate(self, id, state))
                .map(|_| vec![Tuple(vec![string_value(id)])])
                .unwrap_or_default(),
            ArgConstraint::Any => self
                .handles
                .iter()
                .filter(|(id, state)| predicate(self, id, state))
                .map(|(id, _)| Tuple(vec![string_value(id)]))
                .collect(),
        }
    }

    fn pipeline_position_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let position = i64_constraint(constraints, 1);
        match (handle, position) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(id), _) => self
                .handles
                .get(id)
                .and_then(|state| state.status.as_deref())
                .and_then(|status| self.pipeline_position(status))
                .map(|position| Tuple(vec![string_value(id), int_value(position)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .filter_map(|(id, state)| {
                    state
                        .status
                        .as_deref()
                        .and_then(|status| self.pipeline_position(status))
                        .map(|position| Tuple(vec![string_value(id), int_value(position)]))
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn lifecycle_status_classification_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        match string_constraint(constraints, 0) {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(status) => self
                .lifecycle_status_classifications(status)
                .into_iter()
                .map(|(classification, origin)| {
                    Tuple(vec![
                        string_value(status),
                        string_value(classification),
                        string_value(origin),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            ArgConstraint::Any => self
                .lifecycle_status_candidates()
                .into_iter()
                .flat_map(|status| {
                    self.lifecycle_status_classifications(&status)
                        .into_iter()
                        .map(move |(classification, origin)| {
                            Tuple(vec![
                                string_value(&status),
                                string_value(classification),
                                string_value(origin),
                            ])
                        })
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn lifecycle_status_candidates(&self) -> BTreeSet<String> {
        self.handles
            .values()
            .filter_map(|state| state.status.clone())
            .chain(self.active_statuses.iter().cloned())
            .chain(self.terminal_statuses.iter().cloned())
            .chain(self.settled_statuses.iter().cloned())
            .chain(self.pipeline_positions.keys().cloned())
            .chain(
                CANONICAL_PIPELINE_ORDERING
                    .iter()
                    .chain(CANONICAL_SETTLED_STATUSES)
                    .chain(TERMINAL_STATUS_HEURISTICS)
                    .map(|status| (*status).to_owned()),
            )
            .collect()
    }

    fn lifecycle_status_classifications(
        &self,
        status: &str,
    ) -> BTreeSet<(&'static str, &'static str)> {
        let mut classifications = BTreeSet::new();

        if let Some(origin) = self.terminal_status_origin(status) {
            classifications.insert(("terminal", origin));
        } else if self.active_statuses.contains(status) {
            classifications.insert(("active", "project"));
        }

        if let Some(origin) = self.settled_status_origin(status) {
            classifications.insert(("settled", origin));
        }

        if let Some(origin) = self.pipeline_status_origin(status) {
            classifications.insert(("pipeline", origin));
        }

        classifications
    }

    fn pipeline_position_for_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let status = string_constraint(constraints, 0);
        let position = i64_constraint(constraints, 1);
        match (status, position) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(status), _) => self
                .pipeline_position(status)
                .map(|position| Tuple(vec![string_value(status), int_value(position)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .pipeline_ordering()
                .into_iter()
                .map(|(status, position)| Tuple(vec![string_value(status), int_value(position)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn handle_count_tuples(
        &self,
        constraints: &[(usize, Value)],
        counts: &BTreeMap<HandleId, usize>,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let count = i64_constraint(constraints, 1);
        match (handle, count) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) if self.handles.contains_key(handle) => {
                let count = i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX);
                vec![Tuple(vec![string_value(handle), int_value(count)])]
                    .into_iter()
                    .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                    .collect()
            }
            (ArgConstraint::Exact(_), _) => Vec::new(),
            (ArgConstraint::Any, _) => self
                .handles
                .keys()
                .map(|handle| {
                    let count =
                        i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX);
                    Tuple(vec![string_value(handle), int_value(count)])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn freshness_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = i64_constraint(constraints, 1);
        let today = self.evaluation_day.or_else(current_days_since_epoch);
        match (handle, days) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) => self
                .handles
                .get(handle)
                .map(|state| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(freshness_days(state, today)),
                    ])
                })
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .map(|(handle, state)| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(freshness_days(state, today)),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn flux_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = match i64_constraint(constraints, 1) {
            ArgConstraint::Exact(days) if days >= 0 => days,
            ArgConstraint::Any | ArgConstraint::Exact(_) | ArgConstraint::Impossible => {
                return Vec::new();
            }
        };
        let delta = i64_constraint(constraints, 2);
        let today = self.evaluation_day.or_else(current_days_since_epoch);
        match (handle, delta) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) => self
                .handles
                .get_key_value(handle)
                .map(|(handle_id, state)| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(days),
                        int_value(self.flux_delta(handle_id, state, days, today)),
                    ])
                })
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .handles
                .iter()
                .map(|(handle, state)| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(days),
                        int_value(self.flux_delta(handle, state, days, today)),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn git_mtime_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let file = string_constraint(constraints, 0);
        let instant = string_constraint(constraints, 1);
        match (file, instant) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(file), _) => self
                .git_mtimes
                .get(file)
                .map(|mtime| Tuple(vec![string_value(file), string_value(mtime)]))
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            (ArgConstraint::Any, _) => self
                .git_mtimes
                .iter()
                .map(|(file, instant)| Tuple(vec![string_value(file), string_value(instant)]))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn repository_operation_capability_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        self.repository
            .iter()
            .flat_map(RepositoryContext::capability_rows)
            .map(|(operation, availability, provider, reason)| {
                Tuple(vec![
                    string_value(operation),
                    string_value(availability),
                    string_value(provider),
                    string_value(reason),
                ])
            })
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .collect()
    }

    fn changed_within_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let days = match i64_constraint(constraints, 1) {
            ArgConstraint::Exact(days) if days >= 0 => days,
            ArgConstraint::Any | ArgConstraint::Exact(_) | ArgConstraint::Impossible => {
                return Vec::new();
            }
        };
        let Some(today) = self.evaluation_day.or_else(current_days_since_epoch) else {
            return Vec::new();
        };
        let cutoff = today.saturating_sub(days);
        match handle {
            ArgConstraint::Impossible => Vec::new(),
            ArgConstraint::Exact(handle) => self
                .changed_within_tuple_for(handle, days, cutoff)
                .into_iter()
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
            ArgConstraint::Any => self
                .handles
                .keys()
                .filter_map(|handle| self.changed_within_tuple_for(handle.as_str(), days, cutoff))
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn changed_within_tuple_for(&self, handle: &str, days: i64, cutoff: i64) -> Option<Tuple> {
        let state = self.handles.get(handle)?;
        let instant = self.git_mtimes.get(&state.file)?;
        let mtime_day = snapshot_days_since_epoch(instant)?;
        (mtime_day >= cutoff).then(|| Tuple(vec![string_value(handle), int_value(days)]))
    }

    fn count_tuples(
        &self,
        constraints: &[(usize, Value)],
        counts: &BTreeMap<HandleId, usize>,
    ) -> Vec<Tuple> {
        let handle = string_constraint(constraints, 0);
        let count = i64_constraint(constraints, 1);
        match (handle, count) {
            (ArgConstraint::Impossible, _) | (_, ArgConstraint::Impossible) => Vec::new(),
            (ArgConstraint::Exact(handle), _) if self.nodes.contains(handle) => vec![Tuple(vec![
                string_value(handle),
                int_value(i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX)),
            ])]
            .into_iter()
            .filter(|tuple| tuple_matches_constraints(tuple, constraints))
            .collect(),
            (ArgConstraint::Exact(_), _) => Vec::new(),
            (ArgConstraint::Any, _) => self
                .nodes
                .iter()
                .map(|handle| {
                    Tuple(vec![
                        string_value(handle),
                        int_value(
                            i64::try_from(*counts.get(handle).unwrap_or(&0)).unwrap_or(i64::MAX),
                        ),
                    ])
                })
                .filter(|tuple| tuple_matches_constraints(tuple, constraints))
                .collect(),
        }
    }

    fn is_terminal(&self, _handle: &HandleId, state: &HandleState) -> bool {
        let Some(status) = state.status.as_deref() else {
            return false;
        };
        self.terminal_status_origin(status).is_some()
    }

    fn terminal_status_origin(&self, status: &str) -> Option<&'static str> {
        if self.terminal_statuses.contains(status) {
            return Some("project");
        }
        if self.active_statuses.contains(status) {
            return None;
        }
        is_terminal_status(status).then_some("builtin")
    }

    fn is_active(&self, handle: &HandleId, state: &HandleState) -> bool {
        !self.is_terminal(handle, state)
    }

    fn is_settled(&self, _handle: &HandleId, state: &HandleState) -> bool {
        let Some(status) = state.status.as_deref() else {
            return false;
        };
        self.settled_status_origin(status).is_some()
    }

    fn settled_status_origin(&self, status: &str) -> Option<&'static str> {
        if self.settled_statuses.contains(status) {
            return Some("project");
        }
        is_canonical_settled_status(status).then_some("builtin")
    }

    fn pipeline_status_origin(&self, status: &str) -> Option<&'static str> {
        if self.pipeline_positions.contains_key(status) {
            return Some("project");
        }
        (self.pipeline_positions.is_empty() && canonical_pipeline_position(status).is_some())
            .then_some("builtin")
    }

    fn is_obligation(&self, _handle: &HandleId, state: &HandleState) -> bool {
        state.kind == LABEL_KIND && self.linear_namespaces.contains(&state.namespace)
    }

    fn is_discharged(&self, handle: &HandleId, _state: &HandleState) -> bool {
        self.discharge_count
            .get(handle)
            .copied()
            .unwrap_or_default()
            > 0
    }

    fn is_undischarged(&self, handle: &HandleId, state: &HandleState) -> bool {
        self.is_obligation(handle, state)
            && !self.is_discharged(handle, state)
            && !self.is_terminal(handle, state)
    }

    fn pipeline_position(&self, status: &str) -> Option<i64> {
        self.pipeline_positions.get(status).copied().or_else(|| {
            self.pipeline_positions
                .is_empty()
                .then(|| canonical_pipeline_position(status))
                .flatten()
        })
    }

    fn pipeline_ordering(&self) -> Vec<(&str, i64)> {
        if self.pipeline_positions.is_empty() {
            return CANONICAL_PIPELINE_ORDERING
                .iter()
                .enumerate()
                .map(|(idx, status)| (*status, i64::try_from(idx).unwrap_or(i64::MAX)))
                .collect();
        }
        let mut ordering = self
            .pipeline_positions
            .iter()
            .map(|(status, position)| (status.as_str(), *position))
            .collect::<Vec<_>>();
        ordering.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(right.0)));
        ordering
    }

    fn flux_delta(
        &self,
        handle: &HandleId,
        state: &HandleState,
        days: i64,
        today: Option<i64>,
    ) -> i64 {
        let Some(today) = today else {
            return 0;
        };
        let start = today.saturating_sub(days);
        let mut statuses = self
            .status_snapshots
            .get(handle)
            .into_iter()
            .flat_map(|snapshots| snapshots.iter())
            .filter(|snapshot| snapshot.day >= start && snapshot.day <= today)
            .map(|snapshot| (snapshot.day, snapshot.status.as_str()))
            .collect::<Vec<_>>();
        if let Some(status) = state.status.as_deref() {
            statuses.push((today, status));
        }
        i64::try_from(
            statuses
                .windows(2)
                .filter(|pair| pair[0].1 != pair[1].1)
                .count(),
        )
        .unwrap_or(i64::MAX)
    }

    fn reachable_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        self.walk_from(start, direction, false, max_depth)
    }

    fn impact_reachable_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        self.walk_impact_from(start, direction, max_depth)
    }

    fn neighborhood_from(&self, start: &str, max_depth: Option<i64>) -> Vec<GraphStep> {
        if !self.nodes.contains(start) {
            return Vec::new();
        }
        self.walk_from(start, Direction::Undirected, true, max_depth)
    }

    fn walk_from(
        &self,
        start: &str,
        direction: Direction,
        include_start: bool,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        let Ok(start) = HandleId::new(start) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if include_start {
            out.push(GraphStep {
                node: start.clone(),
                depth: 0,
            });
        }
        let mut seen = BTreeSet::from([start.clone()]);
        let mut queue = VecDeque::from([(start, 0_i64)]);
        while let Some((node, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            self.visit_neighbors(&node, direction, |next| {
                if !seen.insert(next.clone()) {
                    return;
                }
                let next_depth = depth + 1;
                out.push(GraphStep {
                    node: next.clone(),
                    depth: next_depth,
                });
                queue.push_back((next.clone(), next_depth));
            });
        }
        out
    }

    fn walk_impact_from(
        &self,
        start: &str,
        direction: Direction,
        max_depth: Option<i64>,
    ) -> Vec<GraphStep> {
        let Ok(start) = HandleId::new(start) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([start.clone()]);
        let mut queue = VecDeque::from([(start, 0_i64)]);
        while let Some((node, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            self.visit_impact_neighbors(&node, direction, |next| {
                if !seen.insert(next.clone()) {
                    return;
                }
                let next_depth = depth + 1;
                out.push(GraphStep {
                    node: next.clone(),
                    depth: next_depth,
                });
                queue.push_back((next.clone(), next_depth));
            });
        }
        out
    }

    fn visit_neighbors(
        &self,
        node: &HandleId,
        direction: Direction,
        mut visit: impl FnMut(&HandleId),
    ) {
        match direction {
            Direction::Outgoing => {
                if let Some(outgoing) = self.outgoing.get(node) {
                    for next in outgoing {
                        visit(next);
                    }
                }
            }
            Direction::Incoming => {
                if let Some(incoming) = self.incoming.get(node) {
                    for next in incoming {
                        visit(next);
                    }
                }
            }
            Direction::Undirected => {
                if let Some(incoming) = self.incoming.get(node) {
                    for next in incoming {
                        visit(next);
                    }
                }
                if let Some(outgoing) = self.outgoing.get(node) {
                    for next in outgoing {
                        visit(next);
                    }
                }
            }
        }
    }

    fn visit_impact_neighbors(
        &self,
        node: &HandleId,
        direction: Direction,
        mut visit: impl FnMut(&HandleId),
    ) {
        match direction {
            Direction::Outgoing => {
                self.visit_impact_edges(self.outgoing_edges.get(node), &mut visit);
            }
            Direction::Incoming => {
                self.visit_impact_edges(self.incoming_edges.get(node), &mut visit);
            }
            Direction::Undirected => {
                self.visit_impact_edges(self.incoming_edges.get(node), &mut visit);
                self.visit_impact_edges(self.outgoing_edges.get(node), &mut visit);
            }
        }
    }

    fn visit_impact_edges(
        &self,
        edges: Option<&BTreeSet<(String, HandleId)>>,
        visit: &mut impl FnMut(&HandleId),
    ) {
        let Some(edges) = edges else {
            return;
        };
        for (kind, next) in edges {
            if self.impact_traverse.traverses(kind) {
                visit(next);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GraphStep {
    node: HandleId,
    depth: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_inputs_are_declared_in_the_runtime_schema() {
        for key in PRIMITIVE_INDEX_CONFIG_KEYS {
            let declaration = crate::config_schema::runtime_config_declaration_by_key(*key)
                .expect("every primitive-index config input is supported project vocabulary");
            assert_eq!(
                runtime_config_key_for_config_key(&declaration.config_key()),
                Some(*key)
            );
        }
    }
}
