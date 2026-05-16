use std::collections::HashSet;

use serde::Serialize;

use crate::graph::DiGraph;
use crate::handle::HandleKind;
use crate::lattice::Lattice;
use crate::output::{Line, Printer, Render};
use crate::resolve::ResolveStats;

// ---------------------------------------------------------------------------
// Graph summary (moved from main.rs)
// ---------------------------------------------------------------------------

/// Output of bare `anneal` (no subcommand): graph summary.
#[derive(Serialize)]
pub(crate) struct GraphSummary {
    pub(crate) root: String,
    pub(crate) handles: usize,
    pub(crate) files: usize,
    pub(crate) labels: usize,
    pub(crate) sections: usize,
    pub(crate) versions_count: usize,
    pub(crate) edges: usize,
    pub(crate) namespaces: Vec<String>,
    pub(crate) versions: usize,
    pub(crate) labels_resolved: usize,
    pub(crate) labels_skipped: usize,
    pub(crate) pending_edges_resolved: usize,
    pub(crate) pending_edges_unresolved: usize,
    pub(crate) lattice_kind: crate::lattice::LatticeKind,
    pub(crate) observed_statuses: usize,
    pub(crate) active_statuses: usize,
    pub(crate) terminal_statuses: usize,
}

impl Render for GraphSummary {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        p.heading("anneal", None)?;
        p.caption("knowledge graph built")?;
        p.blank()?;

        let mut rows: Vec<(&str, Line)> = vec![
            ("Root", Line::new().path(self.root.clone())),
            (
                "Handles",
                Line::new().count(self.handles).text("  ").dim(format!(
                    "{} files, {} labels, {} sections, {} versions",
                    self.files, self.labels, self.sections, self.versions_count
                )),
            ),
            ("Edges", Line::new().count(self.edges)),
            (
                "Namespaces",
                Line::new()
                    .count(self.namespaces.len())
                    .text("  ")
                    .dim(self.namespaces.join(", ")),
            ),
            (
                "Labels",
                Line::new()
                    .count(self.labels_resolved)
                    .text(" resolved, ")
                    .count(self.labels_skipped)
                    .text(" skipped"),
            ),
            ("Versions", Line::new().count(self.versions)),
            (
                "Edges resolved",
                Line::new()
                    .count(self.pending_edges_resolved)
                    .text(" resolved, ")
                    .count(self.pending_edges_unresolved)
                    .text(" unresolved"),
            ),
            (
                "Lattice",
                Line::new().text(format!("{:?}", self.lattice_kind).to_lowercase()),
            ),
        ];
        if self.lattice_kind == crate::lattice::LatticeKind::Confidence {
            rows.push((
                "Statuses",
                Line::new()
                    .count(self.observed_statuses)
                    .text(" observed, ")
                    .count(self.active_statuses)
                    .text(" active, ")
                    .count(self.terminal_statuses)
                    .text(" terminal"),
            ));
        }
        p.kv_block(&rows)
    }
}

/// Build a `GraphSummary` from pipeline results.
pub(crate) fn build_summary(
    root: &str,
    graph: &DiGraph,
    stats: &ResolveStats,
    lattice: &Lattice,
) -> GraphSummary {
    let (mut files, mut labels, mut sections, mut versions_count) =
        (0usize, 0usize, 0usize, 0usize);
    for (_, h) in graph.nodes() {
        match h.kind {
            HandleKind::File(_) => files += 1,
            HandleKind::Label { .. } => labels += 1,
            HandleKind::Section { .. } => sections += 1,
            HandleKind::Version { .. } => versions_count += 1,
            HandleKind::External { .. } => {}
        }
    }
    GraphSummary {
        root: root.to_string(),
        handles: graph.node_count(),
        files,
        labels,
        sections,
        versions_count,
        edges: graph.edge_count(),
        namespaces: sorted_namespace_names(&stats.namespaces),
        versions: stats.versions_resolved,
        labels_resolved: stats.labels_resolved,
        labels_skipped: stats.labels_skipped,
        pending_edges_resolved: stats.pending_edges_resolved,
        pending_edges_unresolved: stats.pending_edges_unresolved,
        lattice_kind: if lattice.kind == crate::lattice::LatticeKind::Confidence {
            crate::lattice::LatticeKind::Confidence
        } else {
            crate::lattice::LatticeKind::Existence
        },
        observed_statuses: lattice.observed_statuses.len(),
        active_statuses: lattice.active.len(),
        terminal_statuses: lattice.terminal.len(),
    }
}

/// Sort a set of namespace names into a deterministic order.
pub(crate) fn sorted_namespace_names(ns: &HashSet<String>) -> Vec<String> {
    let mut list: Vec<String> = ns.iter().cloned().collect();
    list.sort_unstable();
    list
}
