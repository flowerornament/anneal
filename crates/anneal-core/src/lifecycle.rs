//! Lifecycle-status helpers shared by runtime and adapters.

pub(crate) const TERMINAL_STATUS_HEURISTICS: &[&str] = &[
    "superseded",
    "archived",
    "historical",
    "prior",
    "retired",
    "deprecated",
    "obsolete",
    "withdrawn",
    "cancelled",
    "canceled",
    "closed",
    "resolved",
    "done",
    "completed",
    "incorporated",
    "digested",
];

pub(crate) const CANONICAL_PIPELINE_ORDERING: &[&str] = &[
    "raw",
    "draft",
    "research",
    "plan",
    "current",
    "active",
    "stable",
    "authoritative",
];

pub(crate) const CANONICAL_SETTLED_STATUSES: &[&str] =
    &["authoritative", "current", "active", "stable", "living"];

pub fn is_terminal_status(status: &str) -> bool {
    let lower = status.to_lowercase();
    TERMINAL_STATUS_HEURISTICS
        .iter()
        .any(|heuristic| lower.contains(heuristic))
}

pub(crate) fn is_canonical_settled_status(status: &str) -> bool {
    CANONICAL_SETTLED_STATUSES.contains(&status)
}

pub(crate) fn canonical_pipeline_position(status: &str) -> Option<i64> {
    CANONICAL_PIPELINE_ORDERING
        .iter()
        .position(|candidate| candidate == &status)
        .map(|idx| i64::try_from(idx).unwrap_or(i64::MAX))
}
