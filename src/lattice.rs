use std::collections::HashSet;

use serde::Serialize;

use crate::config::{AnnealConfig, FreshnessConfig};

// ---------------------------------------------------------------------------
// Convergence state
// ---------------------------------------------------------------------------

/// The convergence state of a single handle (KB-D7, KB-D8).
///
/// Two lattice levels:
/// - **Existence lattice** (zero-config): only `Exists` and `Missing`.
/// - **Confidence lattice** (when status values are present): `Active(status)`
///   and `Terminal(status)` classify handles by their refinement stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ConvergenceState {
    /// Two-element lattice: the handle exists in the corpus.
    Exists,
    /// Two-element lattice: the handle is referenced but not found.
    Missing,
    /// Confidence lattice: the handle has an active (in-progress) status value.
    Active(String),
    /// Confidence lattice: the handle has a terminal (settled) status value.
    Terminal(String),
}

// ---------------------------------------------------------------------------
// Lattice kind
// ---------------------------------------------------------------------------

/// Whether the corpus uses simple existence checking or full confidence lattice.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub enum LatticeKind {
    /// Zero-config baseline: {exists, missing} only (LATTICE-01, KB-D8).
    Existence,
    /// Has frontmatter status values, enabling active/terminal partition (LATTICE-02, KB-D9).
    Confidence,
}

// ---------------------------------------------------------------------------
// Lattice
// ---------------------------------------------------------------------------

/// The computed convergence lattice for a corpus.
///
/// Holds the observed status values, the active/terminal partition, and the
/// optional pipeline ordering. Inferred from frontmatter and config.
#[derive(Debug, Serialize)]
pub struct Lattice {
    /// All distinct status values observed across the corpus.
    pub observed_statuses: HashSet<String>,
    /// Status values classified as active (in-progress, convergence states).
    pub active: HashSet<String>,
    /// Status values classified as terminal (settled, fixed points).
    pub terminal: HashSet<String>,
    /// Optional pipeline ordering for flow analysis. Empty if flat set.
    pub ordering: Vec<String>,
    /// Whether this is an existence or confidence lattice.
    pub kind: LatticeKind,
}

// ---------------------------------------------------------------------------
// Lattice inference
// ---------------------------------------------------------------------------

/// Infer the convergence lattice from observed status values and config (LATTICE-02).
///
/// - If no status values are observed, returns an existence lattice (zero-config baseline).
/// - Otherwise, returns a confidence lattice with active/terminal partition.
///
/// The `terminal_by_directory` parameter provides statuses that were observed only in
/// terminal directories (archive/, prior/, history/) -- these are classified as
/// terminal unless config overrides them.
pub fn infer_lattice(
    observed_statuses: &HashSet<String>,
    config: &AnnealConfig,
    terminal_by_directory: &HashSet<String>,
) -> Lattice {
    if observed_statuses.is_empty() {
        return Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: LatticeKind::Existence,
        };
    }

    let mut active = HashSet::new();
    let mut terminal = HashSet::new();

    // Config overrides take priority
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

    // Classify remaining statuses
    for status in observed_statuses {
        if active.contains(status) || terminal.contains(status) {
            continue; // already classified by config
        }

        // Directory convention: statuses found only in terminal directories
        if terminal_by_directory.contains(status) {
            terminal.insert(status.clone());
        } else {
            // Default: unrecognized statuses are active
            active.insert(status.clone());
        }
    }

    Lattice {
        observed_statuses: observed_statuses.clone(),
        active,
        terminal,
        ordering: config.convergence.ordering.clone(),
        kind: LatticeKind::Confidence,
    }
}

// ---------------------------------------------------------------------------
// Status classification
// ---------------------------------------------------------------------------

/// Classify a status value into a `ConvergenceState` using the inferred lattice (LATTICE-03).
///
/// - If the status is in the active set -> `Active(status)`
/// - If the status is in the terminal set -> `Terminal(status)`
/// - Default (unrecognized) -> `Active(status)` (conservative: treat as in-progress)
pub fn classify_status(status: &str, lattice: &Lattice) -> ConvergenceState {
    if lattice.terminal.contains(status) {
        ConvergenceState::Terminal(status.to_string())
    } else {
        // Active set or unrecognized: treat as active
        ConvergenceState::Active(status.to_string())
    }
}

// ---------------------------------------------------------------------------
// Freshness
// ---------------------------------------------------------------------------

/// The freshness level of a handle based on its age.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FreshnessLevel {
    /// Within the warn threshold -- no issues.
    Fresh,
    /// Between warn and error thresholds -- approaching staleness.
    Warn,
    /// Beyond the error threshold -- stale.
    Stale,
}

/// Freshness computation result for a handle (LATTICE-04, KB-D11).
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    /// Days since the handle was last updated.
    pub days: i64,
    /// Classification based on configured thresholds.
    pub level: FreshnessLevel,
}

/// Compute freshness from the `updated:` frontmatter field or file mtime (KB-D11).
///
/// Prefers `updated` if present, falls back to `mtime`. If neither is available,
/// returns `Fresh` with 0 days (can't compute -- assume OK).
pub fn compute_freshness(
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

// ---------------------------------------------------------------------------
// State ordering
// ---------------------------------------------------------------------------

/// Look up the ordering level of a status value in the lattice pipeline (KB-D9).
///
/// Returns the index of `status` in `lattice.ordering`, or `None` if the status
/// is not in the ordering or the ordering is empty. Used by Phase 2's CHECK-03
/// (confidence gap: source state > target state).
pub fn state_level(status: &str, lattice: &Lattice) -> Option<usize> {
    if lattice.ordering.is_empty() {
        return None;
    }

    lattice.ordering.iter().position(|s| s == status)
}

// ---------------------------------------------------------------------------
// Convention adoption
// ---------------------------------------------------------------------------

/// Compute the frontmatter adoption rate for a set of files (KB-D12).
///
/// Returns the fraction of files that have frontmatter. Phase 2 uses this for
/// CHECK-05: warn about missing frontmatter only when >50% of siblings have it.
pub fn frontmatter_adoption_rate(total_files: usize, files_with_frontmatter: usize) -> f64 {
    if total_files == 0 {
        return 0.0;
    }

    #[allow(clippy::cast_precision_loss)] // file counts will never exceed 2^52
    {
        files_with_frontmatter as f64 / total_files as f64
    }
}
