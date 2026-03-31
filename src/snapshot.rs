use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};

use camino::Utf8Path;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::checks::{Diagnostic, Severity};
use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::HandleKind;
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
    }
}

// ---------------------------------------------------------------------------
// JSONL I/O (per spec section 15.2, decisions D-01, D-02)
// ---------------------------------------------------------------------------

/// Append a snapshot as a single JSON line to `.anneal/history.jsonl`.
///
/// Creates the `.anneal/` directory if it does not exist (D-02).
/// Uses `O_APPEND` for practically atomic writes (D-01).
pub(crate) fn append_snapshot(root: &Utf8Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    let anneal_dir = root.join(".anneal");
    fs::create_dir_all(anneal_dir.as_std_path())?;

    let history_path = anneal_dir.join("history.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path.as_std_path())?;

    let mut buf = serde_json::to_vec(snapshot)?;
    buf.push(b'\n');
    file.write_all(&buf)?;

    Ok(())
}

/// Read all snapshots from `.anneal/history.jsonl`.
///
/// Returns empty Vec if file is missing (CONVERGE-05).
/// Skips unparseable lines with a stderr warning (handles truncated writes).
pub(crate) fn read_history(root: &Utf8Path) -> Vec<Snapshot> {
    let history_path = root.join(".anneal/history.jsonl");

    let Ok(file) = fs::File::open(history_path.as_std_path()) else {
        return Vec::new();
    };

    let reader = BufReader::new(file);
    let mut snapshots = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Snapshot>(&line) {
            Ok(s) => snapshots.push(s),
            Err(e) => {
                eprintln!("warning: skipping unparseable snapshot line: {e}");
            }
        }
    }

    snapshots
}

// ---------------------------------------------------------------------------
// Convergence summary computation (per spec section 10.1 / KB-D18)
// ---------------------------------------------------------------------------

/// Compute convergence summary from two snapshots (D-05).
///
/// Compares frozen handle counts (resolution), total handles (creation),
/// and outstanding obligations to determine signal.
pub(crate) fn compute_convergence_summary(
    current: &Snapshot,
    previous: &Snapshot,
) -> ConvergenceSummary {
    #[allow(clippy::cast_possible_wrap)]
    let resolution_gain = current.handles.frozen as i64 - previous.handles.frozen as i64;
    #[allow(clippy::cast_possible_wrap)]
    let creation_gain = current.handles.total as i64 - previous.handles.total as i64;
    #[allow(clippy::cast_possible_wrap)]
    let obligations_delta =
        current.obligations.outstanding as i64 - previous.obligations.outstanding as i64;

    if resolution_gain > creation_gain && obligations_delta <= 0 {
        ConvergenceSummary {
            signal: ConvergenceSignal::Advancing,
            detail: format!(
                "resolution +{resolution_gain}, creation +{creation_gain}, obligations {obligations_delta}"
            ),
        }
    } else if creation_gain > resolution_gain || obligations_delta > 0 {
        ConvergenceSummary {
            signal: ConvergenceSignal::Drifting,
            detail: format!(
                "resolution +{resolution_gain}, creation +{creation_gain}, obligations +{obligations_delta}"
            ),
        }
    } else {
        ConvergenceSummary {
            signal: ConvergenceSignal::Holding,
            detail: format!(
                "resolution +{resolution_gain}, creation +{creation_gain}, obligations {obligations_delta}"
            ),
        }
    }
}

/// Get the latest convergence summary by comparing current snapshot against
/// the most recent entry in history (D-06).
///
/// Returns `None` if no previous snapshot exists (first run).
pub(crate) fn latest_summary(root: &Utf8Path, current: &Snapshot) -> Option<ConvergenceSummary> {
    let history = read_history(root);
    let previous = history.last()?;
    Some(compute_convergence_summary(current, previous))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnnealConfig, HandlesConfig};
    use crate::graph::DiGraph;
    use crate::handle::{Handle, HandleKind, HandleMetadata};
    use crate::lattice::{Lattice, LatticeKind};

    fn make_lattice(active: &[&str], terminal: &[&str]) -> Lattice {
        Lattice {
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

    fn make_file_handle(id: &str, status: Option<&str>) -> Handle {
        Handle::test_file(id, status)
    }

    fn make_label_handle(prefix: &str, number: u32, status: Option<&str>) -> Handle {
        Handle::test_label(prefix, number, status)
    }

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
        append_snapshot(root, &snapshot).expect("append");

        let history_path = root.join(".anneal/history.jsonl");
        assert!(history_path.exists(), "history.jsonl should exist");

        let content = std::fs::read_to_string(history_path.as_std_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "Should have exactly one line");

        // Parse back
        let parsed: Snapshot = serde_json::from_str(lines[0]).expect("parse");
        assert_eq!(parsed.handles.total, 10);
    }

    // -----------------------------------------------------------------------
    // Test 3: read_history returns empty Vec when file missing (CONVERGE-05)
    // -----------------------------------------------------------------------

    #[test]
    fn read_history_returns_empty_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let history = read_history(root);
        assert!(
            history.is_empty(),
            "Should return empty Vec for missing file"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: read_history skips unparseable lines (CONVERGE-05)
    // -----------------------------------------------------------------------

    #[test]
    fn read_history_skips_unparseable_lines() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let anneal_dir = root.join(".anneal");
        fs::create_dir_all(anneal_dir.as_std_path()).expect("mkdir");

        let snapshot = make_snapshot(10, 5, 5, 0);
        let valid_line = serde_json::to_string(&snapshot).expect("serialize");

        let history_path = anneal_dir.join("history.jsonl");
        let content = format!("{valid_line}\nthis is garbage\n{valid_line}\n");
        fs::write(history_path.as_std_path(), content).expect("write");

        let history = read_history(root);
        assert_eq!(
            history.len(),
            2,
            "Should have 2 valid snapshots, skipping garbage"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: read_history returns snapshots in file order
    // -----------------------------------------------------------------------

    #[test]
    fn read_history_returns_snapshots_in_file_order() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let s1 = make_snapshot(10, 5, 5, 0);
        let s2 = make_snapshot(20, 10, 10, 0);
        let s3 = make_snapshot(30, 15, 15, 0);

        append_snapshot(root, &s1).expect("append1");
        append_snapshot(root, &s2).expect("append2");
        append_snapshot(root, &s3).expect("append3");

        let history = read_history(root);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].handles.total, 10);
        assert_eq!(history[1].handles.total, 20);
        assert_eq!(history[2].handles.total, 30);
    }

    // -----------------------------------------------------------------------
    // Test 6: compute_convergence_summary returns None when no previous (D-06)
    // -----------------------------------------------------------------------

    #[test]
    fn latest_summary_returns_none_when_no_history() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let snapshot = make_snapshot(10, 5, 5, 0);
        let result = latest_summary(root, &snapshot);
        assert!(
            result.is_none(),
            "Should return None when no history exists"
        );
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
        let _f1 = graph.add_node(make_file_handle("doc1.md", Some("draft")));
        let _f2 = graph.add_node(make_file_handle("doc2.md", Some("archived")));
        // 2 label handles in OQ namespace
        let oq1 = graph.add_node(make_label_handle("OQ", 1, None));
        let _oq2 = graph.add_node(make_label_handle("OQ", 2, Some("archived")));
        // 1 label in linear namespace OBL with a discharge
        let obl1 = graph.add_node(make_label_handle("OBL", 1, None));
        let discharger = graph.add_node(make_file_handle("proof.md", Some("draft")));
        graph.add_edge(discharger, obl1, crate::graph::EdgeKind::Discharges);
        // OQ-1 has no discharge (not linear, doesn't matter)
        // Add a Cites edge for edge count
        graph.add_edge(discharger, oq1, crate::graph::EdgeKind::Cites);

        let lattice = make_lattice(&["draft"], &["archived"]);
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
                code: "E001",
                message: "test error".to_string(),
                file: None,
                line: None,
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Warning,
                code: "W001",
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
        let obligation = graph.add_node(make_label_handle("OBL", 1, None));
        let discharger = graph.add_node(make_file_handle("proof.md", Some("draft")));
        graph.add_edge(discharger, obligation, crate::graph::EdgeKind::Discharges);
        graph.add_node(Handle {
            id: "https://example.com/spec".to_string(),
            kind: HandleKind::External {
                url: "https://example.com/spec".to_string(),
            },
            status: None,
            file_path: Some(camino::Utf8PathBuf::from("proof.md")),
            metadata: HandleMetadata::default(),
        });

        let lattice = make_lattice(&["draft"], &["archived"]);
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
