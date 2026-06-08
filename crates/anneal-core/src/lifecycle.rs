//! Lifecycle-status helpers shared by runtime and adapters.

const TERMINAL_STATUS_HEURISTICS: &[&str] = &[
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

pub fn is_terminal_status(status: &str) -> bool {
    let lower = status.to_lowercase();
    TERMINAL_STATUS_HEURISTICS
        .iter()
        .any(|heuristic| lower.contains(heuristic))
}
