use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use anyhow::Context;
use camino::Utf8Path;
use serde::Serialize;

use crate::area::AreaGrade;
use crate::output::{Line, Printer, Render, TableHeader, Tone, Toned};
use crate::snapshot::AreaSnapshot;

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

impl Render for DiffOutput {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        if !self.has_history {
            p.heading("No snapshot history yet", None)?;
            p.blank()?;
            p.hints(&[
                ("anneal status", "create the first snapshot"),
                ("anneal status", "run again later to compare"),
            ])?;
            return Ok(());
        }

        p.heading("Diff", None)?;
        p.caption(&format!("since {}", self.reference))?;
        p.blank()?;

        let rows = &[
            (
                "Handles",
                signed_summary(&[
                    (self.handle_delta.created, "created"),
                    (self.handle_delta.active_delta, "active"),
                    (self.handle_delta.frozen_delta, "terminal"),
                ]),
            ),
            (
                "Obligations",
                signed_summary(&[
                    (self.obligation_delta.outstanding_delta, "outstanding"),
                    (self.obligation_delta.discharged_delta, "discharged"),
                    (self.obligation_delta.mooted_delta, "mooted"),
                ]),
            ),
            (
                "Edges",
                signed_summary(&[(self.edge_delta.total_delta, "total")]),
            ),
        ];
        p.kv_block(rows)?;

        if !self.state_changes.is_empty() {
            p.blank()?;
            p.heading("State changes", Some(self.state_changes.len()))?;
            for sc in &self.state_changes {
                p.line_at(
                    4,
                    &Line::new()
                        .toned(Tone::Heading, sc.state.clone())
                        .text("  ")
                        .count(sc.previous_count)
                        .dim(" → ")
                        .count(sc.current_count)
                        .dim(format!("  ({delta:+})", delta = sc.delta)),
                )?;
            }
        }

        if !self.namespace_deltas.is_empty() {
            p.blank()?;
            p.heading("Namespaces", Some(self.namespace_deltas.len()))?;
            for nd in &self.namespace_deltas {
                p.line_at(
                    4,
                    &Line::new()
                        .toned(Tone::Heading, nd.prefix.clone())
                        .text("  ")
                        .toned(delta_tone(nd.total_delta), format!("{:+}", nd.total_delta))
                        .text(" total, ")
                        .toned(delta_tone(nd.open_delta), format!("{:+}", nd.open_delta))
                        .text(" open, ")
                        .toned(
                            delta_tone(-nd.resolved_delta),
                            format!("{:+}", nd.resolved_delta),
                        )
                        .text(" resolved"),
                )?;
            }
        }

        Ok(())
    }
}

/// Render a comma-separated list of `+N label` pairs, coloring the sign
/// by polarity (positive = success-ish for growth; callers interpret
/// semantics like "more terminal is improving").
fn signed_summary(parts: &[(i64, &str)]) -> Line {
    let mut line = Line::new();
    for (i, (delta, label)) in parts.iter().enumerate() {
        if i > 0 {
            line = line.dim(", ");
        }
        line = line
            .toned(delta_tone(*delta), format!("{delta:+}"))
            .text(format!(" {label}"));
    }
    line
}

fn delta_tone(delta: i64) -> Tone {
    match delta {
        0 => Tone::Dim,
        d if d > 0 => Tone::Success,
        _ => Tone::Warning,
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

/// Resolved reference point for a diff, shared by `cmd_diff` and
/// `cmd_diff_by_area`. Three modes: git_ref reconstructs the graph at that
/// ref; days finds the closest historical snapshot; default takes the latest
/// snapshot. Returns `None` when no history exists.
fn resolve_previous_snapshot(
    root: &Utf8Path,
    state: &crate::config::ResolvedStateConfig,
    days: Option<u32>,
    git_ref: Option<&str>,
) -> anyhow::Result<Option<(String, crate::snapshot::Snapshot)>> {
    if let Some(git_ref) = git_ref {
        let previous = build_graph_at_git_ref(root, git_ref)?;
        return Ok(Some((git_ref.to_string(), previous)));
    }
    if let Some(days) = days {
        let history = crate::snapshot::read_all_snapshots(root, state);
        if let Some(previous) = find_snapshot_by_days(&history, days) {
            return Ok(Some((format!("{days} days ago"), previous.clone())));
        }
        return Ok(None);
    }
    Ok(
        crate::snapshot::read_latest_snapshot(root, state)
            .map(|s| ("last snapshot".to_string(), s)),
    )
}

/// Compute graph-level diff output.
pub(crate) fn cmd_diff(
    root: &Utf8Path,
    state: &crate::config::ResolvedStateConfig,
    current_snapshot: &crate::snapshot::Snapshot,
    days: Option<u32>,
    git_ref: Option<&str>,
) -> anyhow::Result<DiffOutput> {
    if let Some((reference, previous)) = resolve_previous_snapshot(root, state, days, git_ref)? {
        return Ok(diff_snapshots(current_snapshot, &previous, &reference));
    }

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

// ---------------------------------------------------------------------------
// diff --by-area: per-area convergence deltas
// ---------------------------------------------------------------------------

/// Trend direction for a single area between two snapshots.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AreaTrend {
    Improving,
    Holding,
    Degrading,
    New,
    Removed,
}

impl std::fmt::Display for AreaTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Improving => "improving",
            Self::Holding => "holding",
            Self::Degrading => "degrading",
            Self::New => "new",
            Self::Removed => "removed",
        })
    }
}

impl Toned for AreaTrend {
    fn tone(&self) -> Tone {
        match self {
            Self::Improving => Tone::Success,
            Self::Holding | Self::Removed => Tone::Dim,
            Self::Degrading => Tone::Warning,
            Self::New => Tone::Callout,
        }
    }
}

/// Per-area delta between two snapshots.
#[derive(Clone, Serialize)]
pub(crate) struct AreaDelta {
    pub(crate) name: String,
    pub(crate) grade: AreaGrade,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) previous_grade: Option<AreaGrade>,
    pub(crate) errors_delta: i64,
    pub(crate) orphans_delta: i64,
    pub(crate) connectivity_delta: f64,
    pub(crate) cross_links_delta: i64,
    pub(crate) trend: AreaTrend,
}

/// Output of `anneal diff --by-area`.
#[derive(Serialize)]
pub(crate) struct DiffByAreaOutput {
    pub(crate) reference: String,
    pub(crate) has_history: bool,
    pub(crate) areas: Vec<AreaDelta>,
}

impl Render for DiffByAreaOutput {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        if self.has_history {
            p.heading("Diff by area", Some(self.areas.len()))?;
            p.caption(&format!("since {}", self.reference))?;
        } else {
            p.heading("Area snapshot", Some(self.areas.len()))?;
            p.caption("no snapshot history yet — current state only")?;
        }
        p.blank()?;

        let headers = &[
            TableHeader::text("Area"),
            TableHeader::text("Grade"),
            TableHeader::numeric("Δ Err"),
            TableHeader::numeric("Δ Orphans"),
            TableHeader::numeric("Δ Conn"),
            TableHeader::text("Trend"),
        ];
        let rows: Vec<Vec<Line>> = self
            .areas
            .iter()
            .map(|area| {
                let grade_cell = match area.previous_grade {
                    Some(prev) if prev != area.grade => Line::new()
                        .toned(prev.tone(), format!("[{prev}]"))
                        .dim(" → ")
                        .toned(area.grade.tone(), format!("[{}]", area.grade)),
                    _ => Line::new().toned(area.grade.tone(), format!("[{}]", area.grade)),
                };
                vec![
                    Line::new().path(format!("{}/", area.name)),
                    grade_cell,
                    signed_cell(area.errors_delta, /* lower-is-better */ true),
                    signed_cell(area.orphans_delta, true),
                    signed_cell_float(area.connectivity_delta),
                    Line::new().toned(area.trend.tone(), area.trend.to_string()),
                ]
            })
            .collect();
        p.table(headers, &rows)?;
        Ok(())
    }
}

fn signed_cell(delta: i64, lower_is_better: bool) -> Line {
    let tone = match (delta, lower_is_better) {
        (0, _) => Tone::Dim,
        (d, true) if d > 0 => Tone::Warning,
        (d, true) => {
            let _ = d;
            Tone::Success
        }
        (d, false) if d > 0 => Tone::Success,
        _ => Tone::Warning,
    };
    Line::new().toned(tone, format!("{delta:+}"))
}

fn signed_cell_float(delta: f64) -> Line {
    let tone = if delta.abs() < 0.05 {
        Tone::Dim
    } else if delta > 0.0 {
        Tone::Success
    } else {
        Tone::Warning
    };
    Line::new().toned(tone, format!("{delta:+.1}"))
}

/// Classify trend from deltas using the principle: errors and orphans matter
/// more than connectivity. A new error dominates a connectivity boost.
fn classify_trend(
    errors_delta: i64,
    orphans_delta: i64,
    connectivity_delta: f64,
    grade_changed: bool,
) -> AreaTrend {
    if errors_delta > 0 || orphans_delta > 0 {
        return AreaTrend::Degrading;
    }
    if errors_delta < 0 || orphans_delta < 0 {
        return AreaTrend::Improving;
    }
    if connectivity_delta < -0.05 {
        return AreaTrend::Degrading;
    }
    if connectivity_delta > 0.05 {
        return AreaTrend::Improving;
    }
    if grade_changed {
        AreaTrend::Degrading
    } else {
        AreaTrend::Holding
    }
}

#[allow(clippy::cast_possible_wrap)]
pub(crate) fn compute_area_deltas(
    current: &HashMap<String, AreaSnapshot>,
    previous: &HashMap<String, AreaSnapshot>,
) -> Vec<AreaDelta> {
    let mut names: BTreeSet<&str> = current
        .keys()
        .chain(previous.keys())
        .map(String::as_str)
        .collect();
    let mut out = Vec::with_capacity(names.len());
    while let Some(name) = names.pop_first() {
        let curr = current.get(name);
        let prev = previous.get(name);
        match (curr, prev) {
            (Some(c), Some(p)) => {
                let errors_delta = c.errors as i64 - p.errors as i64;
                let orphans_delta = c.orphans as i64 - p.orphans as i64;
                let connectivity_delta = c.connectivity - p.connectivity;
                let cross_links_delta = c.cross_links as i64 - p.cross_links as i64;
                let grade_changed = c.grade != p.grade;
                let trend = classify_trend(
                    errors_delta,
                    orphans_delta,
                    connectivity_delta,
                    grade_changed,
                );
                out.push(AreaDelta {
                    name: name.to_string(),
                    grade: c.grade,
                    previous_grade: grade_changed.then_some(p.grade),
                    errors_delta,
                    orphans_delta,
                    connectivity_delta,
                    cross_links_delta,
                    trend,
                });
            }
            (Some(c), None) => {
                out.push(AreaDelta {
                    name: name.to_string(),
                    grade: c.grade,
                    previous_grade: None,
                    errors_delta: c.errors as i64,
                    orphans_delta: c.orphans as i64,
                    connectivity_delta: c.connectivity,
                    cross_links_delta: c.cross_links as i64,
                    trend: AreaTrend::New,
                });
            }
            (None, Some(p)) => {
                out.push(AreaDelta {
                    name: name.to_string(),
                    grade: p.grade,
                    previous_grade: None,
                    errors_delta: -(p.errors as i64),
                    orphans_delta: -(p.orphans as i64),
                    connectivity_delta: -p.connectivity,
                    cross_links_delta: -(p.cross_links as i64),
                    trend: AreaTrend::Removed,
                });
            }
            (None, None) => {}
        }
    }
    out
}

fn current_only_area_view(current: &HashMap<String, AreaSnapshot>) -> Vec<AreaDelta> {
    let mut out: Vec<AreaDelta> = current
        .iter()
        .map(|(name, snap)| AreaDelta {
            name: name.clone(),
            grade: snap.grade,
            previous_grade: None,
            errors_delta: 0,
            orphans_delta: 0,
            connectivity_delta: 0.0,
            cross_links_delta: 0,
            trend: AreaTrend::Holding,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Compute `anneal diff --by-area` output. Same three reference modes as
/// `cmd_diff`; falls back to a current-state-only view when no history exists.
pub(crate) fn cmd_diff_by_area(
    root: &Utf8Path,
    state: &crate::config::ResolvedStateConfig,
    current_snapshot: &crate::snapshot::Snapshot,
    days: Option<u32>,
    git_ref: Option<&str>,
) -> anyhow::Result<DiffByAreaOutput> {
    if let Some((reference, previous)) = resolve_previous_snapshot(root, state, days, git_ref)? {
        return Ok(DiffByAreaOutput {
            reference,
            has_history: true,
            areas: compute_area_deltas(&current_snapshot.areas, &previous.areas),
        });
    }
    Ok(DiffByAreaOutput {
        reference: String::new(),
        has_history: false,
        areas: current_only_area_view(&current_snapshot.areas),
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
        let mut p = Printer::new(&mut buf, plain_style());
        output.render(&mut p).expect("render");
        let text = String::from_utf8(buf).expect("utf8");

        assert!(
            text.contains("since last snapshot"),
            "Expected 'since last snapshot' caption, got: {text}"
        );
        assert!(text.contains("Handles"), "Missing Handles line");
        assert!(text.contains("Obligations"), "Missing Obligations line");
        assert!(text.contains("Edges"), "Missing Edges line");
    }

    #[test]
    fn diff_no_history_produces_message() {
        let current = make_snapshot_base();
        let tmp = tempfile::tempdir().expect("tmpdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");

        let output = cmd_diff(root, &repo_state(), &current, None, None).expect("cmd_diff");

        assert!(!output.has_history);
        let mut buf = Vec::new();
        let mut p = Printer::new(&mut buf, plain_style());
        output.render(&mut p).expect("render");
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

    // -----------------------------------------------------------------------
    // diff --by-area tests
    // -----------------------------------------------------------------------

    fn area_snap(
        grade: AreaGrade,
        errors: usize,
        orphans: usize,
        connectivity: f64,
    ) -> AreaSnapshot {
        AreaSnapshot {
            files: 10,
            handles: 30,
            errors,
            orphans,
            cross_links: 5,
            connectivity,
            grade,
        }
    }

    #[test]
    fn by_area_classifies_degrading_on_new_errors() {
        let mut previous = HashMap::new();
        previous.insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 0, 0.5));
        let mut current = HashMap::new();
        current.insert("compiler".to_string(), area_snap(AreaGrade::C, 2, 0, 0.5));

        let deltas = compute_area_deltas(&current, &previous);
        let compiler = deltas.iter().find(|d| d.name == "compiler").expect("found");
        assert_eq!(compiler.errors_delta, 2);
        assert_eq!(compiler.previous_grade, Some(AreaGrade::B));
        assert!(matches!(compiler.trend, AreaTrend::Degrading));
    }

    #[test]
    fn by_area_classifies_improving_on_fewer_orphans() {
        let mut previous = HashMap::new();
        previous.insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 9, 0.4));
        let mut current = HashMap::new();
        current.insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 3, 0.4));

        let deltas = compute_area_deltas(&current, &previous);
        assert_eq!(deltas[0].orphans_delta, -6);
        assert!(matches!(deltas[0].trend, AreaTrend::Improving));
    }

    #[test]
    fn by_area_classifies_new_and_removed() {
        let mut previous = HashMap::new();
        previous.insert("gone".to_string(), area_snap(AreaGrade::B, 0, 0, 0.4));
        let mut current = HashMap::new();
        current.insert("fresh".to_string(), area_snap(AreaGrade::A, 0, 0, 1.0));

        let deltas = compute_area_deltas(&current, &previous);
        let fresh = deltas.iter().find(|d| d.name == "fresh").expect("fresh");
        assert!(matches!(fresh.trend, AreaTrend::New));
        let gone = deltas.iter().find(|d| d.name == "gone").expect("gone");
        assert!(matches!(gone.trend, AreaTrend::Removed));
    }

    #[test]
    fn by_area_holding_when_nothing_changed() {
        let mut previous = HashMap::new();
        previous.insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 0, 0.4));
        let mut current = HashMap::new();
        current.insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 0, 0.4));
        let deltas = compute_area_deltas(&current, &previous);
        assert!(matches!(deltas[0].trend, AreaTrend::Holding));
        assert!(deltas[0].previous_grade.is_none());
    }

    #[test]
    fn by_area_falls_back_to_current_view_without_history() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8");
        let mut current = make_snapshot_base();
        current
            .areas
            .insert("compiler".to_string(), area_snap(AreaGrade::B, 0, 2, 0.5));

        let output = cmd_diff_by_area(root, &repo_state(), &current, None, None).expect("diff");
        assert!(!output.has_history);
        assert_eq!(output.areas.len(), 1);
        assert_eq!(output.areas[0].name, "compiler");
        assert!(matches!(output.areas[0].trend, AreaTrend::Holding));
    }

    #[test]
    fn by_area_human_no_history_message() {
        let output = DiffByAreaOutput {
            reference: String::new(),
            has_history: false,
            areas: vec![AreaDelta {
                name: "compiler".to_string(),
                grade: AreaGrade::B,
                previous_grade: None,
                errors_delta: 0,
                orphans_delta: 0,
                connectivity_delta: 0.0,
                cross_links_delta: 0,
                trend: AreaTrend::Holding,
            }],
        };
        let mut buf = Vec::new();
        let mut p = Printer::new(&mut buf, plain_style());
        output.render(&mut p).expect("render");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("no snapshot history"));
        assert!(text.contains("compiler"));
    }

    fn plain_style() -> crate::output::OutputStyle {
        crate::output::OutputStyle::plain()
    }
}
