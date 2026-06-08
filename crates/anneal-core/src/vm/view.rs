//! Tuple-level view overlays for scoped evaluation.

use std::collections::BTreeMap;

use crate::ir::ids::RowId;
use crate::runtime::ast::Ident;
use crate::time::{relative_days_reference, snapshot_days_since_epoch};
use crate::vm::store::RelationStore;

#[derive(Clone, Debug, Default)]
pub(crate) struct TupleOverlay {
    relations: BTreeMap<Ident, RelationStore>,
}

impl TupleOverlay {
    pub(crate) fn relation(&self, relation: &Ident) -> Option<&RelationStore> {
        self.relations.get(relation)
    }

    pub(crate) fn insert(&mut self, relation: Ident, store: RelationStore) {
        self.relations.insert(relation, store);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotSelection {
    pub(crate) snapshot: String,
    pub(crate) day: i64,
    pub(crate) tuple_rows: Vec<RowId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotCandidate {
    pub(crate) snapshot: String,
    pub(crate) day: i64,
    pub(crate) sort_at: String,
    pub(crate) tuple_rows: Vec<RowId>,
}

pub(crate) enum SnapshotReference {
    Last,
    Snapshot(String),
    Day(i64),
}

pub(crate) fn snapshot_reference(reference: &str) -> Option<SnapshotReference> {
    if reference == "snapshot:last" {
        return Some(SnapshotReference::Last);
    }
    if let Some(snapshot) = reference.strip_prefix("snapshot:") {
        return (!snapshot.is_empty()).then(|| SnapshotReference::Snapshot(snapshot.to_string()));
    }
    if let Some(day) = snapshot_days_since_epoch(reference) {
        return Some(SnapshotReference::Day(day));
    }
    relative_days_reference(reference).map(SnapshotReference::Day)
}

pub(crate) fn latest_snapshot_candidate(
    candidates: impl Iterator<Item = SnapshotCandidate>,
) -> Option<SnapshotSelection> {
    candidates
        .max_by(|left, right| {
            left.day
                .cmp(&right.day)
                .then_with(|| left.sort_at.cmp(&right.sort_at))
                .then_with(|| left.snapshot.cmp(&right.snapshot))
        })
        .map(SnapshotSelection::from)
}

pub(crate) fn nearest_snapshot_candidate(
    candidates: impl Iterator<Item = SnapshotCandidate>,
    target_day: i64,
) -> Option<SnapshotSelection> {
    candidates
        .min_by(|left, right| {
            let left_distance = left.day.abs_diff(target_day);
            let right_distance = right.day.abs_diff(target_day);
            left_distance
                .cmp(&right_distance)
                .then_with(|| right.day.cmp(&left.day))
                .then_with(|| right.sort_at.cmp(&left.sort_at))
                .then_with(|| right.snapshot.cmp(&left.snapshot))
        })
        .map(SnapshotSelection::from)
}

impl From<SnapshotCandidate> for SnapshotSelection {
    fn from(candidate: SnapshotCandidate) -> Self {
        Self {
            snapshot: candidate.snapshot,
            day: candidate.day,
            tuple_rows: candidate.tuple_rows,
        }
    }
}

pub(crate) fn handle_snapshot_patch_field(key: &str) -> Option<&'static str> {
    match key {
        "kind" => Some("kind"),
        "status" => Some("status"),
        "namespace" => Some("namespace"),
        "file" => Some("file"),
        "date" => Some("date"),
        "area" => Some("area"),
        "summary" => Some("summary"),
        _ => None,
    }
}
