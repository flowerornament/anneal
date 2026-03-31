use std::collections::HashSet;

use serde::Serialize;

use crate::config::{AnnealConfig, FreshnessConfig};

/// The convergence state of a single handle (KB-D7, KB-D8).
///
/// Two lattice levels:
/// - **Existence lattice** (zero-config): only `Exists` and `Missing`.
/// - **Confidence lattice** (when status values are present): `Active(status)`
///   and `Terminal(status)` classify handles by their refinement stage.
// Phase 2: CHECK rules classify handles via this enum
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum ConvergenceState {
    Exists,
    Missing,
    Active(String),
    Terminal(String),
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
        if terminal_by_directory.contains(status) || crate::parse::is_terminal_by_heuristic(status)
        {
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

/// Classify a status value using the inferred lattice (LATTICE-03).
// Phase 2: CHECK-03 confidence gap
#[allow(dead_code)]
pub(crate) fn classify_status(status: &str, lattice: &Lattice) -> ConvergenceState {
    if lattice.terminal.contains(status) {
        ConvergenceState::Terminal(status.to_string())
    } else {
        ConvergenceState::Active(status.to_string())
    }
}

/// Freshness level based on age thresholds.
// Phase 2: CHECK-02 staleness
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum FreshnessLevel {
    Fresh,
    Warn,
    Stale,
}

/// Freshness computation result for a handle (LATTICE-04, KB-D11).
// Phase 2: CHECK-02 staleness
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Freshness {
    pub(crate) days: i64,
    pub(crate) level: FreshnessLevel,
}

/// Compute freshness from the `updated:` frontmatter field or file mtime (KB-D11).
///
/// Prefers `updated` if present, falls back to `mtime`. If neither is available,
/// returns `Fresh` with 0 days.
// Phase 2: CHECK-02 staleness
#[allow(dead_code)]
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
/// Phase 2 uses this for CHECK-05: warn about missing frontmatter only
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
