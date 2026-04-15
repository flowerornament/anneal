use std::collections::HashSet;

use serde::Serialize;

use crate::config::{AnnealConfig, FreshnessConfig};

/// Status names that heuristically indicate terminal state (UX-03).
pub(crate) const TERMINAL_STATUS_HEURISTICS: &[&str] = &[
    "superseded",
    "archived",
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
];

/// Check if a status name matches terminal heuristics (case-insensitive substring match).
pub(crate) fn is_terminal_by_heuristic(status: &str) -> bool {
    let lower = status.to_lowercase();
    TERMINAL_STATUS_HEURISTICS
        .iter()
        .any(|heuristic| lower.contains(heuristic))
}

/// Whether the corpus uses simple existence checking or full confidence lattice.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) enum LatticeKind {
    /// Zero-config baseline: {exists, missing} only (LATTICE-01).
    Existence,
    /// Has frontmatter status values, enabling active/terminal partition (LATTICE-02).
    Confidence,
}

/// The computed convergence lattice for a corpus.
///
/// Holds the observed status values, the active/terminal partition, and the
/// optional pipeline ordering. Inferred from frontmatter and config.
#[derive(Debug, Serialize)]
pub(crate) struct Lattice {
    pub(crate) observed_statuses: HashSet<String>,
    pub(crate) active: HashSet<String>,
    pub(crate) terminal: HashSet<String>,
    /// Optional pipeline ordering for flow analysis. Empty if flat set.
    pub(crate) ordering: Vec<String>,
    pub(crate) kind: LatticeKind,
}

/// Infer the convergence lattice from observed status values and config (LATTICE-02).
///
/// The `terminal_by_directory` parameter provides statuses observed only in
/// terminal directories (archive/, prior/, history/) — classified as terminal
/// unless config overrides them.
pub(crate) fn infer_lattice(
    observed_statuses: HashSet<String>,
    config: &AnnealConfig,
    terminal_by_directory: &HashSet<String>,
) -> Lattice {
    if observed_statuses.is_empty() {
        return Lattice {
            observed_statuses,
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: LatticeKind::Existence,
        };
    }

    let mut active = HashSet::new();
    let mut terminal = HashSet::new();

    for s in &config.convergence.active {
        if observed_statuses.contains(s) {
            active.insert(s.clone());
        }
    }
    for s in &config.convergence.terminal {
        if observed_statuses.contains(s) {
            terminal.insert(s.clone());
        }
    }

    for status in &observed_statuses {
        if active.contains(status) || terminal.contains(status) {
            continue;
        }
        if terminal_by_directory.contains(status) || is_terminal_by_heuristic(status) {
            terminal.insert(status.clone());
        } else {
            active.insert(status.clone());
        }
    }

    Lattice {
        observed_statuses,
        active,
        terminal,
        ordering: config.convergence.ordering.clone(),
        kind: LatticeKind::Confidence,
    }
}

/// Freshness level based on age thresholds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum FreshnessLevel {
    Fresh,
    Warn,
    Stale,
}

/// Freshness computation result for a handle (LATTICE-04, KB-D11).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Freshness {
    pub(crate) days: i64,
    pub(crate) level: FreshnessLevel,
}

/// Compute freshness from the `updated:` frontmatter field or file mtime (KB-D11).
///
/// Prefers `updated` if present, falls back to `mtime`. If neither is available,
/// returns `Fresh` with 0 days.
pub(crate) fn compute_freshness(
    updated: Option<chrono::NaiveDate>,
    mtime: Option<chrono::NaiveDate>,
    config: &FreshnessConfig,
) -> Freshness {
    let date = updated.or(mtime);

    let Some(date) = date else {
        return Freshness {
            days: 0,
            level: FreshnessLevel::Fresh,
        };
    };

    let today = chrono::Local::now().date_naive();
    let days = (today - date).num_days();

    let level = if days < i64::from(config.warn) {
        FreshnessLevel::Fresh
    } else if days < i64::from(config.error) {
        FreshnessLevel::Warn
    } else {
        FreshnessLevel::Stale
    };

    Freshness { days, level }
}

/// Look up the ordering level of a status in the lattice pipeline (KB-D9).
///
/// Returns the index in `lattice.ordering`, or `None` if absent.
pub(crate) fn state_level(status: &str, lattice: &Lattice) -> Option<usize> {
    if lattice.ordering.is_empty() {
        return None;
    }
    lattice.ordering.iter().position(|s| s == status)
}

/// Fraction of files that have frontmatter (KB-D12).
///
/// Used by CHECK-05 (W003): warn about missing frontmatter only
/// when >50% of siblings have it.
pub(crate) fn frontmatter_adoption_rate(total_files: usize, files_with_frontmatter: usize) -> f64 {
    if total_files == 0 {
        return 0.0;
    }

    #[allow(clippy::cast_precision_loss)]
    {
        files_with_frontmatter as f64 / total_files as f64
    }
}

#[cfg(test)]
impl Lattice {
    /// Empty lattice with no statuses (existence mode).
    pub(crate) fn test_empty() -> Self {
        Self {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: LatticeKind::Existence,
        }
    }

    /// Lattice with explicit active and terminal sets.
    pub(crate) fn test_new(active: &[&str], terminal: &[&str]) -> Self {
        Self {
            observed_statuses: active
                .iter()
                .chain(terminal.iter())
                .copied()
                .map(String::from)
                .collect(),
            active: active.iter().copied().map(String::from).collect(),
            terminal: terminal.iter().copied().map(String::from).collect(),
            ordering: Vec::new(),
            kind: LatticeKind::Confidence,
        }
    }

    /// Lattice with active, terminal, and pipeline ordering.
    pub(crate) fn test_with_ordering(
        active: &[&str],
        terminal: &[&str],
        ordering: &[&str],
    ) -> Self {
        let mut l = Self::test_new(active, terminal);
        l.ordering = ordering.iter().copied().map(String::from).collect();
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AnnealConfig;

    // -------------------------------------------------------------------
    // infer_lattice
    // -------------------------------------------------------------------

    #[test]
    fn infer_lattice_empty_statuses_returns_existence_kind() {
        let config = AnnealConfig::default();
        let terminal_dirs = HashSet::new();
        let lattice = infer_lattice(HashSet::new(), &config, &terminal_dirs);

        assert_eq!(lattice.kind, LatticeKind::Existence);
        assert!(lattice.active.is_empty());
        assert!(lattice.terminal.is_empty());
        assert!(lattice.ordering.is_empty());
        assert!(lattice.observed_statuses.is_empty());
    }

    #[test]
    fn infer_lattice_with_config_active_override() {
        let mut config = AnnealConfig::default();
        config.convergence.active = vec!["wip".to_string()];
        config.convergence.terminal = vec!["done".to_string()];

        let observed: HashSet<String> = ["wip", "done", "draft"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let terminal_dirs = HashSet::new();

        let lattice = infer_lattice(observed, &config, &terminal_dirs);

        assert_eq!(lattice.kind, LatticeKind::Confidence);
        assert!(lattice.active.contains("wip"));
        assert!(lattice.terminal.contains("done"));
        // "draft" is not in config active/terminal, nor in terminal_dirs,
        // nor in heuristic terminals, so it falls to active.
        assert!(lattice.active.contains("draft"));
    }

    #[test]
    fn infer_lattice_heuristic_terminal_wins_for_unclassified() {
        let config = AnnealConfig::default();
        let observed: HashSet<String> = ["archived", "active"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let terminal_dirs = HashSet::new();

        let lattice = infer_lattice(observed, &config, &terminal_dirs);

        assert!(
            lattice.terminal.contains("archived"),
            "archived should be terminal by heuristic"
        );
        assert!(
            lattice.active.contains("active"),
            "active should fall to active set"
        );
    }

    #[test]
    fn infer_lattice_terminal_by_directory_classifies_status() {
        let config = AnnealConfig::default();
        let observed: HashSet<String> = ["custom-done"].iter().map(|s| (*s).to_string()).collect();
        let mut terminal_dirs = HashSet::new();
        terminal_dirs.insert("custom-done".to_string());

        let lattice = infer_lattice(observed, &config, &terminal_dirs);

        assert!(
            lattice.terminal.contains("custom-done"),
            "directory-terminal status should be in terminal set"
        );
    }

    // -------------------------------------------------------------------
    // is_terminal_by_heuristic
    // -------------------------------------------------------------------

    #[test]
    fn is_terminal_by_heuristic_matches_known_statuses() {
        assert!(is_terminal_by_heuristic("archived"));
        assert!(is_terminal_by_heuristic("superseded"));
        assert!(is_terminal_by_heuristic("deprecated"));
        assert!(is_terminal_by_heuristic("done"));
        assert!(is_terminal_by_heuristic("completed"));
        assert!(is_terminal_by_heuristic("withdrawn"));
        assert!(is_terminal_by_heuristic("cancelled"));
        assert!(is_terminal_by_heuristic("canceled"));
        assert!(is_terminal_by_heuristic("resolved"));
    }

    #[test]
    fn is_terminal_by_heuristic_case_insensitive() {
        assert!(is_terminal_by_heuristic("Archived"));
        assert!(is_terminal_by_heuristic("SUPERSEDED"));
        assert!(is_terminal_by_heuristic("Done"));
    }

    #[test]
    fn is_terminal_by_heuristic_rejects_non_terminal() {
        assert!(!is_terminal_by_heuristic("draft"));
        assert!(!is_terminal_by_heuristic("active"));
        assert!(!is_terminal_by_heuristic("wip"));
        assert!(!is_terminal_by_heuristic("review"));
        assert!(!is_terminal_by_heuristic(""));
    }

    // -------------------------------------------------------------------
    // state_level
    // -------------------------------------------------------------------

    #[test]
    fn state_level_returns_position_in_ordering() {
        let lattice = Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: vec![
                "draft".to_string(),
                "review".to_string(),
                "final".to_string(),
            ],
            kind: LatticeKind::Confidence,
        };

        assert_eq!(state_level("draft", &lattice), Some(0));
        assert_eq!(state_level("review", &lattice), Some(1));
        assert_eq!(state_level("final", &lattice), Some(2));
    }

    #[test]
    fn state_level_returns_none_for_unknown_status() {
        let lattice = Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: vec!["draft".to_string(), "review".to_string()],
            kind: LatticeKind::Confidence,
        };

        assert_eq!(state_level("nonexistent", &lattice), None);
    }

    #[test]
    fn state_level_returns_none_when_ordering_empty() {
        let lattice = Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: LatticeKind::Existence,
        };

        assert_eq!(state_level("draft", &lattice), None);
    }

    // -------------------------------------------------------------------
    // frontmatter_adoption_rate
    // -------------------------------------------------------------------

    #[test]
    fn frontmatter_adoption_rate_zero_files() {
        let rate = frontmatter_adoption_rate(0, 0);
        assert!((rate - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn frontmatter_adoption_rate_some_files() {
        let rate = frontmatter_adoption_rate(10, 7);
        assert!((rate - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn frontmatter_adoption_rate_all_files() {
        let rate = frontmatter_adoption_rate(5, 5);
        assert!((rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn frontmatter_adoption_rate_no_frontmatter() {
        let rate = frontmatter_adoption_rate(8, 0);
        assert!((rate - 0.0).abs() < f64::EPSILON);
    }
}
