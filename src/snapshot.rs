use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::checks::{Diagnostic, Severity};
use crate::config::{AnnealConfig, HistoryMode, ResolvedStateConfig};
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::HandleKind;
use crate::identity::fnv1a_64;
use crate::lattice::Lattice;

// ---------------------------------------------------------------------------
// Snapshot types (per spec section 10 / KB-D17, decision D-03)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HandleCounts {
    pub(crate) total: usize,
    pub(crate) active: usize,
    pub(crate) frozen: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EdgeCounts {
    pub(crate) total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ObligationCounts {
    pub(crate) outstanding: usize,
    pub(crate) discharged: usize,
    pub(crate) mooted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiagnosticCounts {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NamespaceStats {
    pub(crate) total: usize,
    pub(crate) open: usize,
    pub(crate) resolved: usize,
    pub(crate) deferred: usize,
}

/// Per-area summary captured in snapshots for `diff --by-area` trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AreaSnapshot {
    pub(crate) files: usize,
    pub(crate) handles: usize,
    pub(crate) errors: usize,
    pub(crate) orphans: usize,
    pub(crate) cross_links: usize,
    pub(crate) connectivity: f64,
    pub(crate) grade: crate::area::AreaGrade,
}

/// A point-in-time snapshot of the knowledge graph state (KB-D17).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) timestamp: String,
    pub(crate) handles: HandleCounts,
    pub(crate) edges: EdgeCounts,
    pub(crate) states: HashMap<String, usize>,
    pub(crate) obligations: ObligationCounts,
    pub(crate) diagnostics: DiagnosticCounts,
    pub(crate) namespaces: HashMap<String, NamespaceStats>,
    /// Per-area summary for trend analysis. Serde default so older snapshots
    /// without this field still parse.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) areas: HashMap<String, AreaSnapshot>,
}

// ---------------------------------------------------------------------------
// Convergence summary (per spec section 10.1 / KB-D18, decisions D-05, D-06)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub(crate) enum ConvergenceSignal {
    Advancing,
    Holding,
    Drifting,
}

impl std::fmt::Display for ConvergenceSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Advancing => f.write_str("advancing"),
            Self::Holding => f.write_str("holding"),
            Self::Drifting => f.write_str("drifting"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConvergenceSummary {
    pub(crate) signal: ConvergenceSignal,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConvergenceAnalysis {
    pub(crate) signal: ConvergenceSignal,
    pub(crate) detail: String,
    pub(crate) resolution_gain: i64,
    pub(crate) creation_gain: i64,
    pub(crate) obligations_delta: i64,
}

// ---------------------------------------------------------------------------
// Build snapshot from graph state
// ---------------------------------------------------------------------------

/// Build a `Snapshot` from the current graph, lattice, config, and diagnostics.
///
/// Iterates graph nodes to count handles by active/terminal status, groups by
/// status, counts obligations for linear namespaces, counts diagnostics by
/// severity, and computes per-namespace stats for Label handles.
pub(crate) fn build_snapshot(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    diagnostics: &[Diagnostic],
) -> Snapshot {
    let mut total = 0usize;
    let mut active = 0usize;
    let mut frozen = 0usize;
    let mut states: HashMap<String, usize> = HashMap::new();

    // Per-namespace tracking
    let mut ns_total: HashMap<String, usize> = HashMap::new();
    let mut ns_open: HashMap<String, usize> = HashMap::new();
    let mut ns_resolved: HashMap<String, usize> = HashMap::new();

    let linear_namespaces = config.handles.linear_set();

    let mut obligations_outstanding = 0usize;
    let mut obligations_discharged = 0usize;
    let mut obligations_mooted = 0usize;

    for (node_id, handle) in graph.nodes() {
        total += 1;

        if let Some(ref status) = handle.status {
            *states.entry(status.clone()).or_insert(0) += 1;
        }
        if handle.is_terminal(lattice) {
            frozen += 1;
        } else {
            active += 1;
        }

        // Namespace stats for Label handles
        if let HandleKind::Label { ref prefix, .. } = handle.kind {
            *ns_total.entry(prefix.clone()).or_insert(0) += 1;

            if handle.is_terminal(lattice) {
                *ns_resolved.entry(prefix.clone()).or_insert(0) += 1;
            } else {
                *ns_open.entry(prefix.clone()).or_insert(0) += 1;
            }

            // Obligation tracking for linear namespaces
            if linear_namespaces.contains(prefix.as_str()) {
                if handle.is_terminal(lattice) {
                    obligations_mooted += 1;
                } else {
                    let discharge_count = graph
                        .incoming(node_id)
                        .iter()
                        .filter(|e| e.kind == EdgeKind::Discharges)
                        .count();
                    if discharge_count > 0 {
                        obligations_discharged += 1;
                    } else {
                        obligations_outstanding += 1;
                    }
                }
            }
        }
    }

    // Build namespace stats map
    let mut namespaces = HashMap::new();
    for (prefix, count) in &ns_total {
        namespaces.insert(
            prefix.clone(),
            NamespaceStats {
                total: *count,
                open: ns_open.get(prefix).copied().unwrap_or(0),
                resolved: ns_resolved.get(prefix).copied().unwrap_or(0),
                deferred: 0, // not yet computed; placeholder for future freshness-based deferred tracking
            },
        );
    }

    // Count diagnostics by severity
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();

    let areas: HashMap<String, AreaSnapshot> =
        crate::area::compute_areas(graph, lattice, diagnostics, &config.areas)
            .into_iter()
            .map(|a| {
                (
                    a.name,
                    AreaSnapshot {
                        files: a.files,
                        handles: a.handles,
                        errors: a.errors,
                        orphans: a.orphans,
                        cross_links: a.cross_links,
                        connectivity: a.connectivity,
                        grade: a.grade,
                    },
                )
            })
            .collect();

    Snapshot {
        timestamp: Utc::now().to_rfc3339(),
        handles: HandleCounts {
            total,
            active,
            frozen,
        },
        edges: EdgeCounts {
            total: graph.edge_count(),
        },
        states,
        obligations: ObligationCounts {
            outstanding: obligations_outstanding,
            discharged: obligations_discharged,
            mooted: obligations_mooted,
        },
        diagnostics: DiagnosticCounts { errors, warnings },
        namespaces,
        areas,
    }
}

// ---------------------------------------------------------------------------
// JSONL I/O (per spec section 15.2, decisions D-01, D-02)
// ---------------------------------------------------------------------------

/// Append a snapshot as a single JSON line to the resolved history backend.
///
/// In `xdg` mode, writes to machine-local state outside the repo. In `repo`
/// mode, writes to `<root>/.anneal/history.jsonl`. In `off` mode, this is a
/// no-op. If legacy repo history exists on first write in `xdg` mode, it is
/// copied into XDG state once so convergence history continues without further
/// repo mutation.
pub(crate) fn append_snapshot(
    root: &Utf8Path,
    state: &ResolvedStateConfig,
    snapshot: &Snapshot,
) -> anyhow::Result<()> {
    let Some(history_path) = write_history_path(root, state) else {
        return Ok(());
    };

    if state.history_mode == HistoryMode::Xdg {
        maybe_seed_xdg_history_from_repo(root, &history_path)?;
    }

    if let Some(parent) = history_path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path.as_std_path())?;

    let mut buf = serde_json::to_vec(snapshot)?;
    buf.push(b'\n');
    file.write_all(&buf)?;

    Ok(())
}

fn parse_snapshot_line(line: &str) -> Option<Snapshot> {
    if line.trim().is_empty() {
        return None;
    }

    match serde_json::from_str::<Snapshot>(line) {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            eprintln!("warning: skipping unparseable snapshot line: {e}");
            None
        }
    }
}

/// Read the full snapshot history from the resolved history backend.
///
/// Prefer [`read_latest_snapshot`] unless the caller truly needs chronological
/// traversal (for example, selecting a snapshot from N days ago).
///
/// Returns empty Vec if file is missing (CONVERGE-05).
/// Skips unparseable lines with a stderr warning (handles truncated writes).
pub(crate) fn read_all_snapshots(root: &Utf8Path, state: &ResolvedStateConfig) -> Vec<Snapshot> {
    let Some(history_path) = read_history_path(root, state) else {
        return Vec::new();
    };

    let Ok(file) = fs::File::open(history_path.as_std_path()) else {
        return Vec::new();
    };

    let reader = BufReader::new(file);
    let mut snapshots = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if let Some(snapshot) = parse_snapshot_line(&line) {
            snapshots.push(snapshot);
        }
    }

    snapshots
}

/// Read only the most recent parseable snapshot from the resolved history
/// backend.
///
/// Returns `None` if the file is missing or contains no valid snapshots.
pub(crate) fn read_latest_snapshot(
    root: &Utf8Path,
    state: &ResolvedStateConfig,
) -> Option<Snapshot> {
    let history_path = read_history_path(root, state)?;

    let Ok(contents) = fs::read_to_string(history_path.as_std_path()) else {
        return None;
    };

    contents
        .rsplit('\n')
        .find(|line| !line.trim().is_empty())
        .and_then(parse_snapshot_line)
}

fn repo_history_path(root: &Utf8Path) -> Utf8PathBuf {
    root.join(".anneal/history.jsonl")
}

fn read_history_path(root: &Utf8Path, state: &ResolvedStateConfig) -> Option<Utf8PathBuf> {
    match state.history_mode {
        HistoryMode::Off => None,
        HistoryMode::Repo => Some(repo_history_path(root)),
        HistoryMode::Xdg => {
            let legacy = repo_history_path(root);
            if let Some(xdg_path) = xdg_history_path(root, state) {
                if xdg_path.exists() {
                    Some(xdg_path)
                } else if legacy.exists() {
                    Some(legacy)
                } else {
                    Some(xdg_path)
                }
            } else if legacy.exists() {
                Some(legacy)
            } else {
                None
            }
        }
    }
}

fn write_history_path(root: &Utf8Path, state: &ResolvedStateConfig) -> Option<Utf8PathBuf> {
    match state.history_mode {
        HistoryMode::Off => None,
        HistoryMode::Repo => Some(repo_history_path(root)),
        HistoryMode::Xdg => xdg_history_path(root, state),
    }
}

fn maybe_seed_xdg_history_from_repo(
    root: &Utf8Path,
    xdg_history_path: &Utf8Path,
) -> anyhow::Result<()> {
    if xdg_history_path.exists() {
        return Ok(());
    }

    let legacy = repo_history_path(root);
    if !legacy.exists() {
        return Ok(());
    }

    if let Some(parent) = xdg_history_path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    fs::copy(legacy.as_std_path(), xdg_history_path.as_std_path())?;
    Ok(())
}

fn xdg_history_path(root: &Utf8Path, state: &ResolvedStateConfig) -> Option<Utf8PathBuf> {
    let base = state
        .history_dir
        .clone()
        .or_else(default_state_dir)?
        .join("anneal/history");
    Some(base.join(root_history_key(root)).join("history.jsonl"))
}

fn default_state_dir() -> Option<Utf8PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        Utf8PathBuf::from_path_buf(dir.into()).ok()
    } else {
        std::env::var_os("HOME")
            .and_then(|home| Utf8PathBuf::from_path_buf(home.into()).ok())
            .map(|home| home.join(".local/state"))
    }
}

fn root_history_key(root: &Utf8Path) -> String {
    let identity = canonical_root_identity(root);
    format!("{:016x}", fnv1a_64(identity.as_bytes()))
}

fn canonical_root_identity(root: &Utf8Path) -> String {
    fs::canonicalize(root.as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .map_or_else(
            || {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| Utf8PathBuf::from_path_buf(cwd).ok())
                    .map_or_else(
                        || root.as_str().to_string(),
                        |cwd| cwd.join(root).to_string(),
                    )
            },
            |path| path.to_string(),
        )
}

// ---------------------------------------------------------------------------
// Convergence summary computation (per spec section 10.1 / KB-D18)
// ---------------------------------------------------------------------------

/// Compute convergence summary from two snapshots (D-05).
///
/// Compares frozen handle counts (resolution), total handles (creation),
/// and outstanding obligations to determine signal.
pub(crate) fn analyze_convergence(current: &Snapshot, previous: &Snapshot) -> ConvergenceAnalysis {
    #[allow(clippy::cast_possible_wrap)]
    let resolution_gain = current.handles.frozen as i64 - previous.handles.frozen as i64;
    #[allow(clippy::cast_possible_wrap)]
    let creation_gain = current.handles.total as i64 - previous.handles.total as i64;
    #[allow(clippy::cast_possible_wrap)]
    let obligations_delta =
        current.obligations.outstanding as i64 - previous.obligations.outstanding as i64;

    // `{:+}` always renders the sign, so `obligations_delta` reads
    // consistently (`+0`, `-2`, `+3`) regardless of convergence branch.
    let detail = format!(
        "resolution +{resolution_gain}, creation +{creation_gain}, obligations {obligations_delta:+}"
    );
    let signal = if resolution_gain > creation_gain && obligations_delta <= 0 {
        ConvergenceSignal::Advancing
    } else if creation_gain > resolution_gain || obligations_delta > 0 {
        ConvergenceSignal::Drifting
    } else {
        ConvergenceSignal::Holding
    };
    ConvergenceAnalysis {
        signal,
        detail,
        resolution_gain,
        creation_gain,
        obligations_delta,
    }
}

pub(crate) fn compute_convergence_summary(
    current: &Snapshot,
    previous: &Snapshot,
) -> ConvergenceSummary {
    let analysis = analyze_convergence(current, previous);
    ConvergenceSummary {
        signal: analysis.signal,
        detail: analysis.detail,
    }
}

/// Compute convergence summary against an already-loaded previous snapshot.
///
/// Returns `None` when there is no previous snapshot (first run).
pub(crate) fn summary_from_previous(
    current: &Snapshot,
    previous: Option<&Snapshot>,
) -> Option<ConvergenceSummary> {
    previous.map(|snapshot| compute_convergence_summary(current, snapshot))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::DiagnosticCode;
    use crate::config::{AnnealConfig, HandlesConfig};
    use crate::graph::DiGraph;
    use crate::handle::Handle;
    use crate::lattice::Lattice;

    fn make_snapshot(total: usize, active: usize, frozen: usize, outstanding: usize) -> Snapshot {
        Snapshot {
            timestamp: "2026-03-29T00:00:00Z".to_string(),
            handles: HandleCounts {
                total,
                active,
                frozen,
            },
            edges: EdgeCounts { total: 0 },
            states: HashMap::new(),
            obligations: ObligationCounts {
                outstanding,
                discharged: 0,
                mooted: 0,
            },
            diagnostics: DiagnosticCounts {
                errors: 0,
                warnings: 0,
            },
            namespaces: HashMap::new(),
            areas: HashMap::new(),
        }
    }

    fn repo_state() -> ResolvedStateConfig {
        ResolvedStateConfig {
            history_mode: HistoryMode::Repo,
            history_dir: None,
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: Snapshot serialization matches spec section 10 schema
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_serializes_to_json_matching_spec_schema() {
        let mut states = HashMap::new();
        states.insert("draft".to_string(), 5);
        states.insert("formal".to_string(), 3);

        let mut namespaces = HashMap::new();
        namespaces.insert(
            "OQ".to_string(),
            NamespaceStats {
                total: 69,
                open: 44,
                resolved: 19,
                deferred: 6,
            },
        );

        let snapshot = Snapshot {
            timestamp: "2026-03-27T14:30:00Z".to_string(),
            handles: HandleCounts {
                total: 487,
                active: 142,
                frozen: 345,
            },
            edges: EdgeCounts { total: 2031 },
            states,
            obligations: ObligationCounts {
                outstanding: 0,
                discharged: 18,
                mooted: 12,
            },
            diagnostics: DiagnosticCounts {
                errors: 0,
                warnings: 3,
            },
            namespaces,
            areas: HashMap::new(),
        };

        let json = serde_json::to_value(&snapshot).expect("serialize");

        // Verify top-level fields exist with correct structure
        assert_eq!(json["timestamp"], "2026-03-27T14:30:00Z");
        assert_eq!(json["handles"]["total"], 487);
        assert_eq!(json["handles"]["active"], 142);
        assert_eq!(json["handles"]["frozen"], 345);
        assert_eq!(json["edges"]["total"], 2031);
        assert_eq!(json["obligations"]["outstanding"], 0);
        assert_eq!(json["obligations"]["discharged"], 18);
        assert_eq!(json["obligations"]["mooted"], 12);
        assert_eq!(json["diagnostics"]["errors"], 0);
        assert_eq!(json["diagnostics"]["warnings"], 3);
        assert_eq!(json["namespaces"]["OQ"]["total"], 69);
        assert_eq!(json["namespaces"]["OQ"]["open"], 44);
        assert_eq!(json["namespaces"]["OQ"]["resolved"], 19);
        assert_eq!(json["namespaces"]["OQ"]["deferred"], 6);
    }

    // -----------------------------------------------------------------------
    // Test 2: append_snapshot creates dir and appends
    // -----------------------------------------------------------------------

    #[test]
    fn append_snapshot_creates_directory_and_appends() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let snapshot = make_snapshot(10, 5, 5, 0);
        append_snapshot(root, &repo_state(), &snapshot).expect("append");

        let history_path = repo_history_path(root);
        assert!(history_path.exists(), "history.jsonl should exist");

        let content = std::fs::read_to_string(history_path.as_std_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "Should have exactly one line");

        // Parse back
        let parsed: Snapshot = serde_json::from_str(lines[0]).expect("parse");
        assert_eq!(parsed.handles.total, 10);
    }

    // -----------------------------------------------------------------------
    // Test 3: read_all_snapshots returns empty Vec when file missing (CONVERGE-05)
    // -----------------------------------------------------------------------

    #[test]
    fn read_all_snapshots_returns_empty_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let history = read_all_snapshots(root, &repo_state());
        assert!(
            history.is_empty(),
            "Should return empty Vec for missing file"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: read_all_snapshots skips unparseable lines (CONVERGE-05)
    // -----------------------------------------------------------------------

    #[test]
    fn read_all_snapshots_skips_unparseable_lines() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let anneal_dir = root.join(".anneal");
        fs::create_dir_all(anneal_dir.as_std_path()).expect("mkdir");

        let snapshot = make_snapshot(10, 5, 5, 0);
        let valid_line = serde_json::to_string(&snapshot).expect("serialize");

        let history_path = anneal_dir.join("history.jsonl");
        let content = format!("{valid_line}\nthis is garbage\n{valid_line}\n");
        fs::write(history_path.as_std_path(), content).expect("write");

        let history = read_all_snapshots(root, &repo_state());
        assert_eq!(
            history.len(),
            2,
            "Should have 2 valid snapshots, skipping garbage"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: read_all_snapshots returns snapshots in file order
    // -----------------------------------------------------------------------

    #[test]
    fn read_all_snapshots_returns_snapshots_in_file_order() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let s1 = make_snapshot(10, 5, 5, 0);
        let s2 = make_snapshot(20, 10, 10, 0);
        let s3 = make_snapshot(30, 15, 15, 0);

        let state = repo_state();
        append_snapshot(root, &state, &s1).expect("append1");
        append_snapshot(root, &state, &s2).expect("append2");
        append_snapshot(root, &state, &s3).expect("append3");

        let history = read_all_snapshots(root, &state);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].handles.total, 10);
        assert_eq!(history[1].handles.total, 20);
        assert_eq!(history[2].handles.total, 30);
    }

    // -----------------------------------------------------------------------
    // Test 6: compute_convergence_summary returns None when no previous (D-06)
    // -----------------------------------------------------------------------

    #[test]
    fn read_latest_snapshot_returns_none_when_no_history() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let result = read_latest_snapshot(root, &repo_state());
        assert!(
            result.is_none(),
            "Should return None when no history exists"
        );
    }

    #[test]
    fn xdg_mode_seeds_from_legacy_repo_history_once() {
        let root_tmp = tempfile::tempdir().expect("root tmpdir");
        let state_tmp = tempfile::tempdir().expect("state tmpdir");
        let root = Utf8Path::from_path(root_tmp.path()).expect("utf8");

        let legacy = repo_history_path(root);
        fs::create_dir_all(
            legacy
                .parent()
                .expect("legacy history parent")
                .as_std_path(),
        )
        .expect("mkdir legacy");
        let previous = make_snapshot(10, 5, 5, 0);
        let legacy_line = serde_json::to_string(&previous).expect("serialize");
        fs::write(legacy.as_std_path(), format!("{legacy_line}\n")).expect("write legacy");

        let state = ResolvedStateConfig {
            history_mode: HistoryMode::Xdg,
            history_dir: Some(
                Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).expect("utf8 state dir"),
            ),
        };
        let current = make_snapshot(20, 10, 10, 0);
        append_snapshot(root, &state, &current).expect("append xdg");

        let history = read_all_snapshots(root, &state);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].handles.total, 10);
        assert_eq!(history[1].handles.total, 20);
    }

    // -----------------------------------------------------------------------
    // Test 7: Advancing when resolution increased and obligations decreased
    // -----------------------------------------------------------------------

    #[test]
    fn convergence_summary_advancing() {
        // Previous: 100 total, 60 active, 40 frozen, 5 outstanding
        let previous = make_snapshot(100, 60, 40, 5);
        // Current: 105 total, 55 active, 50 frozen, 3 outstanding
        // resolution_gain = 50 - 40 = 10, creation_gain = 105 - 100 = 5
        // obligations_delta = 3 - 5 = -2
        // resolution_gain(10) > creation_gain(5) && obligations_delta(-2) <= 0 => Advancing
        let current = make_snapshot(105, 55, 50, 3);

        let summary = compute_convergence_summary(&current, &previous);
        assert!(
            matches!(summary.signal, ConvergenceSignal::Advancing),
            "Expected Advancing, got {:?}",
            summary.signal
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: Drifting when creation outpaces resolution
    // -----------------------------------------------------------------------

    #[test]
    fn convergence_summary_drifting() {
        // Previous: 100 total, 60 active, 40 frozen, 2 outstanding
        let previous = make_snapshot(100, 60, 40, 2);
        // Current: 120 total, 78 active, 42 frozen, 5 outstanding
        // resolution_gain = 42 - 40 = 2, creation_gain = 120 - 100 = 20
        // obligations_delta = 5 - 2 = 3
        // creation_gain(20) > resolution_gain(2) => Drifting
        let current = make_snapshot(120, 78, 42, 5);

        let summary = compute_convergence_summary(&current, &previous);
        assert!(
            matches!(summary.signal, ConvergenceSignal::Drifting),
            "Expected Drifting, got {:?}",
            summary.signal
        );
    }

    // -----------------------------------------------------------------------
    // Test 9: Holding when deltas are balanced
    // -----------------------------------------------------------------------

    #[test]
    fn convergence_summary_holding() {
        // Previous: 100 total, 60 active, 40 frozen, 3 outstanding
        let previous = make_snapshot(100, 60, 40, 3);
        // Current: 105 total, 60 active, 45 frozen, 3 outstanding
        // resolution_gain = 45 - 40 = 5, creation_gain = 105 - 100 = 5
        // obligations_delta = 3 - 3 = 0
        // resolution_gain(5) == creation_gain(5) => Holding
        let current = make_snapshot(105, 60, 45, 3);

        let summary = compute_convergence_summary(&current, &previous);
        assert!(
            matches!(summary.signal, ConvergenceSignal::Holding),
            "Expected Holding, got {:?}",
            summary.signal
        );
    }

    // -----------------------------------------------------------------------
    // Test 10: build_snapshot computes correct counts
    // -----------------------------------------------------------------------

    #[test]
    fn build_snapshot_counts_handles_edges_states_obligations_diagnostics_namespaces() {
        let mut graph = DiGraph::new();
        // 2 file handles: one active, one terminal
        let _f1 = graph.add_node(Handle::test_file("doc1.md", Some("draft")));
        let _f2 = graph.add_node(Handle::test_file("doc2.md", Some("archived")));
        // 2 label handles in OQ namespace
        let oq1 = graph.add_node(Handle::test_label("OQ", 1, None));
        let _oq2 = graph.add_node(Handle::test_label("OQ", 2, Some("archived")));
        // 1 label in linear namespace OBL with a discharge
        let obl1 = graph.add_node(Handle::test_label("OBL", 1, None));
        let discharger = graph.add_node(Handle::test_file("proof.md", Some("draft")));
        graph.add_edge(discharger, obl1, crate::graph::EdgeKind::Discharges);
        // OQ-1 has no discharge (not linear, doesn't matter)
        // Add a Cites edge for edge count
        graph.add_edge(discharger, oq1, crate::graph::EdgeKind::Cites);

        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let diags = vec![
            Diagnostic {
                severity: Severity::Error,
                code: DiagnosticCode::E001,
                message: "test error".to_string(),
                file: None,
                line: None,
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Warning,
                code: DiagnosticCode::W001,
                message: "test warning".to_string(),
                file: None,
                line: None,
                evidence: None,
            },
        ];

        let snapshot = build_snapshot(&graph, &lattice, &config, &diags);

        // 6 total handles
        assert_eq!(snapshot.handles.total, 6);
        // Active: doc1.md(draft), OQ-1(no status), OBL-1(no status), proof.md(draft) = 4
        // Frozen: doc2.md(archived), OQ-2(archived) = 2
        assert_eq!(snapshot.handles.frozen, 2);
        assert_eq!(snapshot.handles.active, 4);

        // 2 edges
        assert_eq!(snapshot.edges.total, 2);

        // States: draft=2 (doc1, proof), archived=2 (doc2, OQ-2)
        assert_eq!(*snapshot.states.get("draft").unwrap_or(&0), 2);
        assert_eq!(*snapshot.states.get("archived").unwrap_or(&0), 2);

        // Obligations: OBL-1 has discharge => discharged=1, outstanding=0, mooted=0
        assert_eq!(snapshot.obligations.discharged, 1);
        assert_eq!(snapshot.obligations.outstanding, 0);
        assert_eq!(snapshot.obligations.mooted, 0);

        // Diagnostics
        assert_eq!(snapshot.diagnostics.errors, 1);
        assert_eq!(snapshot.diagnostics.warnings, 1);

        // Namespace stats: OQ has 2 total, 1 open (OQ-1 no status), 1 resolved (OQ-2 terminal)
        let oq_stats = snapshot.namespaces.get("OQ").expect("OQ namespace");
        assert_eq!(oq_stats.total, 2);
        assert_eq!(oq_stats.open, 1);
        assert_eq!(oq_stats.resolved, 1);

        // OBL has 1 total
        let obl_stats = snapshot.namespaces.get("OBL").expect("OBL namespace");
        assert_eq!(obl_stats.total, 1);
    }

    #[test]
    fn build_snapshot_ignores_external_handles_for_obligations_and_namespaces() {
        let mut graph = DiGraph::new();
        let obligation = graph.add_node(Handle::test_label("OBL", 1, None));
        let discharger = graph.add_node(Handle::test_file("proof.md", Some("draft")));
        graph.add_edge(discharger, obligation, crate::graph::EdgeKind::Discharges);
        graph.add_node(Handle::external(
            "https://example.com/spec".to_string(),
            Some(camino::Utf8PathBuf::from("proof.md")),
        ));

        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let snapshot = build_snapshot(&graph, &lattice, &config, &[]);

        assert_eq!(snapshot.obligations.discharged, 1);
        assert_eq!(snapshot.obligations.outstanding, 0);
        assert_eq!(snapshot.namespaces.len(), 1);
        assert!(snapshot.namespaces.contains_key("OBL"));
    }
}
