use std::collections::BTreeSet;
use std::io::Write;

use anyhow::Context;
use camino::Utf8Path;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Diff command (CLI-08, KB-C8, KB-D19)
// ---------------------------------------------------------------------------

/// Delta in handle counts between two snapshots.
#[derive(Serialize)]
pub(crate) struct HandleDelta {
    pub(crate) created: i64,
    pub(crate) active_delta: i64,
    pub(crate) frozen_delta: i64,
}

/// Change in a single convergence state's count.
#[derive(Serialize)]
pub(crate) struct StateChange {
    pub(crate) state: String,
    pub(crate) previous_count: usize,
    pub(crate) current_count: usize,
    pub(crate) delta: i64,
}

/// Delta in obligation counts.
#[derive(Serialize)]
#[allow(clippy::struct_field_names)]
pub(crate) struct ObligationDelta {
    pub(crate) outstanding_delta: i64,
    pub(crate) discharged_delta: i64,
    pub(crate) mooted_delta: i64,
}

/// Delta in edge counts.
#[derive(Serialize)]
pub(crate) struct EdgeDelta {
    pub(crate) total_delta: i64,
}

/// Delta in namespace statistics.
#[derive(Serialize)]
pub(crate) struct NamespaceDelta {
    pub(crate) prefix: String,
    pub(crate) total_delta: i64,
    pub(crate) open_delta: i64,
    pub(crate) resolved_delta: i64,
}

/// Output of `anneal diff`: graph-level changes since a reference point.
#[derive(Serialize)]
pub(crate) struct DiffOutput {
    pub(crate) reference: String,
    pub(crate) has_history: bool,
    pub(crate) handle_delta: HandleDelta,
    pub(crate) state_changes: Vec<StateChange>,
    pub(crate) obligation_delta: ObligationDelta,
    pub(crate) edge_delta: EdgeDelta,
    pub(crate) namespace_deltas: Vec<NamespaceDelta>,
}

impl DiffOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        if !self.has_history {
            writeln!(w, "No snapshot history yet.")?;
            writeln!(
                w,
                "Run `anneal status` to create the first snapshot, then run it again later."
            )?;
            writeln!(
                w,
                "`anneal diff` compares the current state against a previous snapshot."
            )?;
            return Ok(());
        }
        writeln!(w, "Since {}:", self.reference)?;
        writeln!(
            w,
            "  Handles: {:+} ({:+} active, {:+} frozen)",
            self.handle_delta.created,
            self.handle_delta.active_delta,
            self.handle_delta.frozen_delta
        )?;
        if !self.state_changes.is_empty() {
            for sc in &self.state_changes {
                writeln!(
                    w,
                    "  State: {}: {} -> {} ({:+})",
                    sc.state, sc.previous_count, sc.current_count, sc.delta
                )?;
            }
        }
        writeln!(
            w,
            "  Obligations: {:+} outstanding, {:+} discharged, {:+} mooted",
            self.obligation_delta.outstanding_delta,
            self.obligation_delta.discharged_delta,
            self.obligation_delta.mooted_delta
        )?;
        writeln!(w, "  Edges: {:+}", self.edge_delta.total_delta)?;
        for nd in &self.namespace_deltas {
            writeln!(
                w,
                "  Namespace {}: {:+} total ({:+} open, {:+} resolved)",
                nd.prefix, nd.total_delta, nd.open_delta, nd.resolved_delta
            )?;
        }
        Ok(())
    }
}

/// Compute the diff between two snapshots.
#[allow(clippy::cast_possible_wrap)]
fn diff_snapshots(
    current: &crate::snapshot::Snapshot,
    previous: &crate::snapshot::Snapshot,
    reference: &str,
) -> DiffOutput {
    let handle_delta = HandleDelta {
        created: current.handles.total as i64 - previous.handles.total as i64,
        active_delta: current.handles.active as i64 - previous.handles.active as i64,
        frozen_delta: current.handles.frozen as i64 - previous.handles.frozen as i64,
    };

    // State changes: union of all state keys, include only non-zero deltas
    let mut all_states: BTreeSet<String> = current.states.keys().cloned().collect();
    all_states.extend(previous.states.keys().cloned());

    let state_changes: Vec<StateChange> = all_states
        .into_iter()
        .filter_map(|state| {
            let curr = current.states.get(&state).copied().unwrap_or(0);
            let prev = previous.states.get(&state).copied().unwrap_or(0);
            let delta = curr as i64 - prev as i64;
            if delta != 0 {
                Some(StateChange {
                    state,
                    previous_count: prev,
                    current_count: curr,
                    delta,
                })
            } else {
                None
            }
        })
        .collect();

    let obligation_delta = ObligationDelta {
        outstanding_delta: current.obligations.outstanding as i64
            - previous.obligations.outstanding as i64,
        discharged_delta: current.obligations.discharged as i64
            - previous.obligations.discharged as i64,
        mooted_delta: current.obligations.mooted as i64 - previous.obligations.mooted as i64,
    };

    let edge_delta = EdgeDelta {
        total_delta: current.edges.total as i64 - previous.edges.total as i64,
    };

    // Namespace deltas: union of namespace keys, include only non-zero deltas
    let mut all_ns: BTreeSet<String> = current.namespaces.keys().cloned().collect();
    all_ns.extend(previous.namespaces.keys().cloned());

    let namespace_deltas: Vec<NamespaceDelta> = all_ns
        .into_iter()
        .filter_map(|prefix| {
            let curr = current.namespaces.get(&prefix);
            let prev = previous.namespaces.get(&prefix);
            let total_delta =
                curr.map_or(0, |s| s.total as i64) - prev.map_or(0, |s| s.total as i64);
            let open_delta = curr.map_or(0, |s| s.open as i64) - prev.map_or(0, |s| s.open as i64);
            let resolved_delta =
                curr.map_or(0, |s| s.resolved as i64) - prev.map_or(0, |s| s.resolved as i64);

            if total_delta != 0 || open_delta != 0 || resolved_delta != 0 {
                Some(NamespaceDelta {
                    prefix,
                    total_delta,
                    open_delta,
                    resolved_delta,
                })
            } else {
                None
            }
        })
        .collect();

    DiffOutput {
        reference: reference.to_string(),
        has_history: true,
        handle_delta,
        state_changes,
        obligation_delta,
        edge_delta,
        namespace_deltas,
    }
}

/// Find the snapshot closest to `days` days ago in the history.
fn find_snapshot_by_days(
    history: &[crate::snapshot::Snapshot],
    days: u32,
) -> Option<&crate::snapshot::Snapshot> {
    if history.is_empty() {
        return None;
    }

    let target = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
    let target_ts = target.timestamp();

    history.iter().min_by_key(|s| {
        chrono::DateTime::parse_from_rfc3339(&s.timestamp)
            .map(|dt| (dt.timestamp() - target_ts).unsigned_abs())
            .unwrap_or(u64::MAX)
    })
}

/// Reconstruct a snapshot from files at a git ref by extracting the tree
/// into a temp directory and running the full anneal pipeline on it.
fn build_graph_at_git_ref(
    root: &Utf8Path,
    git_ref: &str,
) -> anyhow::Result<crate::snapshot::Snapshot> {
    use std::process::Command as ProcessCommand;

    let temp_dir = std::env::temp_dir().join(format!(
        "anneal-diff-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    ));
    std::fs::create_dir_all(&temp_dir)?;

    let cmd = format!(
        "git -C {} archive {} | tar -x -C {}",
        shell_escape(root.as_str()),
        shell_escape(git_ref),
        shell_escape(&temp_dir.to_string_lossy()),
    );

    let output = ProcessCommand::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .context("failed to run git archive")?;

    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_dir);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git archive failed: {stderr}");
    }

    let temp_root = camino::Utf8PathBuf::try_from(temp_dir.clone())
        .context("temp dir path is not valid UTF-8")?;

    let result = (|| -> anyhow::Result<crate::snapshot::Snapshot> {
        let cfg = crate::config::load_config(temp_root.as_std_path())?;
        let mut build_result = crate::parse::build_graph(&temp_root, &cfg)?;
        let stats = crate::resolve::resolve_all(
            &mut build_result.graph,
            &build_result.label_candidates,
            &build_result.pending_edges,
            &cfg,
            &temp_root,
            &build_result.filename_index,
        );
        let _ = stats; // stats used by resolve side effects
        let lattice = crate::lattice::infer_lattice(
            build_result.observed_statuses,
            &cfg,
            &build_result.terminal_by_directory,
        );
        let node_index = crate::resolve::build_node_index(&build_result.graph);
        let (unresolved_owned, section_ref_count, section_ref_file) =
            crate::analysis::collect_unresolved_owned(
                &build_result.pending_edges,
                &node_index,
                &build_result.graph,
            );
        let cascade_candidates = std::collections::HashMap::new();
        let check_input = crate::checks::CheckInput {
            graph: &build_result.graph,
            lattice: &lattice,
            config: &cfg,
            unresolved_edges: &unresolved_owned,
            section_ref_count,
            section_ref_file: section_ref_file.as_deref(),
            implausible_refs: &build_result.implausible_refs,
            cascade_candidates: &cascade_candidates,
            previous_snapshot: None,
        };
        let all_diagnostics = crate::checks::run_checks(&check_input);
        Ok(crate::snapshot::build_snapshot(
            &build_result.graph,
            &lattice,
            &cfg,
            &all_diagnostics,
        ))
    })();

    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}

/// Escape a string for shell usage (simple quoting).
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Compute graph-level diff output.
///
/// Three modes:
/// 1. `git_ref` — reconstruct graph at that ref and diff structurally
/// 2. `days` — find closest snapshot to N days ago in history
/// 3. Default — diff against the most recent snapshot only
pub(crate) fn cmd_diff(
    root: &Utf8Path,
    state: &crate::config::ResolvedStateConfig,
    current_snapshot: &crate::snapshot::Snapshot,
    days: Option<u32>,
    git_ref: Option<&str>,
) -> anyhow::Result<DiffOutput> {
    if let Some(git_ref) = git_ref {
        let previous = build_graph_at_git_ref(root, git_ref)?;
        return Ok(diff_snapshots(current_snapshot, &previous, git_ref));
    }

    if let Some(days) = days {
        let history = crate::snapshot::read_all_snapshots(root, state);
        if let Some(previous) = find_snapshot_by_days(&history, days) {
            return Ok(diff_snapshots(
                current_snapshot,
                previous,
                &format!("{days} days ago"),
            ));
        }
    } else if let Some(previous) = crate::snapshot::read_latest_snapshot(root, state).as_ref() {
        return Ok(diff_snapshots(current_snapshot, previous, "last snapshot"));
    }

    // No history available
    Ok(DiffOutput {
        reference: String::new(),
        has_history: false,
        handle_delta: HandleDelta {
            created: 0,
            active_delta: 0,
            frozen_delta: 0,
        },
        state_changes: Vec::new(),
        obligation_delta: ObligationDelta {
            outstanding_delta: 0,
            discharged_delta: 0,
            mooted_delta: 0,
        },
        edge_delta: EdgeDelta { total_delta: 0 },
        namespace_deltas: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use camino::Utf8Path;

    use crate::cli::test_helpers::*;
    use crate::snapshot::NamespaceStats;

    use super::*;

    #[test]
    fn diff_detects_new_handles() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        current.handles.total = 110;
        current.handles.active = 68;
        current.handles.frozen = 42;

        let output = diff_snapshots(&current, &previous, "test");

        assert_eq!(output.handle_delta.created, 10);
        assert_eq!(output.handle_delta.active_delta, 8);
        assert_eq!(output.handle_delta.frozen_delta, 2);
        assert!(output.has_history);
    }

    #[test]
    fn diff_detects_state_changes() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        // Increase draft, decrease archived
        current.states.insert("draft".to_string(), 35);
        current.states.insert("archived".to_string(), 35);

        let output = diff_snapshots(&current, &previous, "test");

        assert!(!output.state_changes.is_empty());
        let draft_change = output
            .state_changes
            .iter()
            .find(|sc| sc.state == "draft")
            .expect("draft state change");
        assert_eq!(draft_change.delta, 5);
        assert_eq!(draft_change.previous_count, 30);
        assert_eq!(draft_change.current_count, 35);

        let archived_change = output
            .state_changes
            .iter()
            .find(|sc| sc.state == "archived")
            .expect("archived state change");
        assert_eq!(archived_change.delta, -5);
    }

    #[test]
    fn diff_detects_obligation_deltas() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        current.obligations.outstanding = 3;
        current.obligations.discharged = 12;

        let output = diff_snapshots(&current, &previous, "test");

        assert_eq!(output.obligation_delta.outstanding_delta, -2);
        assert_eq!(output.obligation_delta.discharged_delta, 2);
        assert_eq!(output.obligation_delta.mooted_delta, 0);
    }

    #[test]
    fn diff_detects_edge_count_changes() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        current.edges.total = 215;

        let output = diff_snapshots(&current, &previous, "test");

        assert_eq!(output.edge_delta.total_delta, 15);
    }

    #[test]
    fn diff_detects_namespace_deltas() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        // Add more OQ labels
        current.namespaces.insert(
            "OQ".to_string(),
            NamespaceStats {
                total: 72,
                open: 46,
                resolved: 20,
                deferred: 6,
            },
        );
        // Add a new namespace
        current.namespaces.insert(
            "FM".to_string(),
            NamespaceStats {
                total: 10,
                open: 7,
                resolved: 3,
                deferred: 0,
            },
        );

        let output = diff_snapshots(&current, &previous, "test");

        assert!(!output.namespace_deltas.is_empty());
        let oq = output
            .namespace_deltas
            .iter()
            .find(|d| d.prefix == "OQ")
            .expect("OQ delta");
        assert_eq!(oq.total_delta, 3);
        assert_eq!(oq.open_delta, 2);
        assert_eq!(oq.resolved_delta, 1);

        let fm = output
            .namespace_deltas
            .iter()
            .find(|d| d.prefix == "FM")
            .expect("FM delta");
        assert_eq!(fm.total_delta, 10);
    }

    #[test]
    fn diff_print_human_includes_since() {
        let previous = make_snapshot_base();
        let mut current = make_snapshot_base();
        current.handles.total = 105;
        current.handles.active = 63;
        current.handles.frozen = 42;

        let output = diff_snapshots(&current, &previous, "last snapshot");

        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");

        assert!(
            text.contains("Since last snapshot:"),
            "Expected 'Since last snapshot:' in output, got: {text}"
        );
        assert!(text.contains("Handles:"), "Missing Handles line");
        assert!(text.contains("Obligations:"), "Missing Obligations line");
        assert!(text.contains("Edges:"), "Missing Edges line");
    }

    #[test]
    fn diff_no_history_produces_message() {
        let current = make_snapshot_base();
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let output = cmd_diff(root, &repo_state(), &current, None, None).expect("cmd_diff");

        assert!(!output.has_history);
        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(
            text.contains("No snapshot history yet"),
            "Expected no-history message, got: {text}"
        );
    }

    #[test]
    fn diff_default_compares_against_most_recent_snapshot() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let mut oldest = make_snapshot_base();
        oldest.handles.total = 60;
        oldest.handles.active = 40;
        oldest.handles.frozen = 20;

        let mut middle = make_snapshot_base();
        middle.handles.total = 80;
        middle.handles.active = 48;
        middle.handles.frozen = 32;

        let mut latest = make_snapshot_base();
        latest.handles.total = 95;
        latest.handles.active = 59;
        latest.handles.frozen = 36;

        let state = repo_state();
        crate::snapshot::append_snapshot(root, &state, &oldest).expect("append oldest");
        crate::snapshot::append_snapshot(root, &state, &middle).expect("append middle");
        crate::snapshot::append_snapshot(root, &state, &latest).expect("append latest");

        let mut current = make_snapshot_base();
        current.handles.total = 100;
        current.handles.active = 63;
        current.handles.frozen = 37;

        let output = cmd_diff(root, &state, &current, None, None).expect("cmd_diff");

        assert!(output.has_history);
        assert_eq!(output.reference, "last snapshot");
        assert_eq!(output.handle_delta.created, 5);
        assert_eq!(output.handle_delta.active_delta, 4);
        assert_eq!(output.handle_delta.frozen_delta, 1);
    }
}
