use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::checks::{self, Diagnostic, Severity};
use crate::graph::DiGraph;
use crate::handle::{HandleKind, NodeId};
use crate::lattice::Lattice;
use crate::parse::PendingEdge;
use crate::resolve::ResolveStats;

// ---------------------------------------------------------------------------
// JSON helper (CLI-09)
// ---------------------------------------------------------------------------

/// Serialize any output type to pretty-printed JSON and print to stdout.
///
/// Since `Serialize` is not object-safe, each command returns its own concrete
/// output struct rather than using trait objects (Pitfall 5).
pub(crate) fn print_json<T: Serialize>(output: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(output)?;
    println!("{json}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Check command (CLI-01)
// ---------------------------------------------------------------------------

/// Output of `anneal check`: diagnostics with summary counts.
#[derive(Serialize)]
pub(crate) struct CheckOutput {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) info: usize,
}

impl CheckOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for diag in &self.diagnostics {
            diag.print_human(w)?;
        }
        if !self.diagnostics.is_empty() {
            writeln!(w)?;
        }
        writeln!(
            w,
            "{} errors, {} warnings, {} info",
            self.errors, self.warnings, self.info
        )
    }
}

/// Run all check rules and produce output, optionally filtering to errors only.
pub(crate) fn cmd_check(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &crate::config::AnnealConfig,
    unresolved_edges: &[PendingEdge],
    section_ref_count: usize,
    errors_only: bool,
) -> CheckOutput {
    let mut diagnostics =
        checks::run_checks(graph, lattice, config, unresolved_edges, section_ref_count);

    if errors_only {
        diagnostics.retain(|d| d.severity == Severity::Error);
    }

    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    let info = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .count();

    CheckOutput {
        diagnostics,
        errors,
        warnings,
        info,
    }
}

// ---------------------------------------------------------------------------
// Get command (CLI-02)
// ---------------------------------------------------------------------------

/// Summary of a single edge for display.
#[derive(Serialize)]
pub(crate) struct EdgeSummary {
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) direction: String,
}

/// Output of `anneal get <handle>`: resolved handle with context.
#[derive(Serialize)]
pub(crate) struct GetOutput {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
    pub(crate) outgoing_edges: Vec<EdgeSummary>,
    pub(crate) incoming_edges: Vec<EdgeSummary>,
}

impl GetOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "{} ({})", self.id, self.kind)?;
        if let Some(ref status) = self.status {
            writeln!(w, "  Status: {status}")?;
        }
        if let Some(ref file) = self.file {
            writeln!(w, "  File: {file}")?;
        }
        if !self.outgoing_edges.is_empty() {
            writeln!(w, "  Outgoing:")?;
            for edge in &self.outgoing_edges {
                writeln!(w, "    {} -> {}", edge.kind, edge.target)?;
            }
        }
        if !self.incoming_edges.is_empty() {
            writeln!(w, "  Incoming:")?;
            for edge in &self.incoming_edges {
                writeln!(w, "    {} <- {}", edge.kind, edge.target)?;
            }
        }
        Ok(())
    }
}

/// Resolve a handle by identity string and build output.
///
/// Looks up the handle by exact match first, then tries case-insensitive
/// match against label identities.
pub(crate) fn cmd_get(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    handle: &str,
) -> Option<GetOutput> {
    // Exact match first
    let node_id = if let Some(&id) = node_index.get(handle) {
        id
    } else {
        // Case-insensitive label match
        let lower = handle.to_lowercase();
        let found = node_index.iter().find(|(k, _)| k.to_lowercase() == lower);
        found.map(|(_, &id)| id)?
    };

    let h = graph.node(node_id);

    let kind_str = match &h.kind {
        HandleKind::File(_) => "file",
        HandleKind::Section { .. } => "section",
        HandleKind::Label { .. } => "label",
        HandleKind::Version { .. } => "version",
    };

    let file = h.file_path.as_ref().map(ToString::to_string);

    let outgoing_edges: Vec<EdgeSummary> = graph
        .outgoing(node_id)
        .iter()
        .map(|e| EdgeSummary {
            kind: format!("{:?}", e.kind),
            target: graph.node(e.target).id.clone(),
            direction: "outgoing".to_string(),
        })
        .collect();

    let incoming_edges: Vec<EdgeSummary> = graph
        .incoming(node_id)
        .iter()
        .map(|e| EdgeSummary {
            kind: format!("{:?}", e.kind),
            target: graph.node(e.source).id.clone(),
            direction: "incoming".to_string(),
        })
        .collect();

    Some(GetOutput {
        id: h.id.clone(),
        kind: kind_str.to_string(),
        status: h.status.clone(),
        file,
        outgoing_edges,
        incoming_edges,
    })
}

// ---------------------------------------------------------------------------
// Find command (CLI-03)
// ---------------------------------------------------------------------------

/// A single match from a find query.
#[derive(Serialize)]
pub(crate) struct FindMatch {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
}

/// Output of `anneal find <query>`: matching handles.
#[derive(Serialize)]
pub(crate) struct FindOutput {
    pub(crate) query: String,
    pub(crate) matches: Vec<FindMatch>,
    pub(crate) total: usize,
}

impl FindOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Found {} matches for \"{}\":", self.total, self.query)?;
        for m in &self.matches {
            let status_str = m
                .status
                .as_deref()
                .map_or(String::new(), |s| format!(" status: {s}"));
            let file_str = m.file.as_deref().unwrap_or("");
            writeln!(w, "  {} ({}){status_str}  {file_str}", m.id, m.kind)?;
        }
        Ok(())
    }
}

/// Search handle identities with case-insensitive substring matching.
///
/// Filters: `--namespace` (label prefix), `--status` (exact status),
/// `--all` (include terminal handles; default excludes them).
pub(crate) fn cmd_find(
    graph: &DiGraph,
    lattice: &Lattice,
    query: &str,
    namespace: Option<&str>,
    status_filter: Option<&str>,
    include_all: bool,
) -> FindOutput {
    let lower_query = query.to_lowercase();

    let mut matches: Vec<FindMatch> = graph
        .nodes()
        .filter(|(_, h)| {
            // Substring match on handle identity
            if !h.id.to_lowercase().contains(&lower_query) {
                return false;
            }

            // Namespace filter: label prefix must match
            if let Some(ns) = namespace {
                match &h.kind {
                    HandleKind::Label { prefix, .. } => {
                        if prefix != ns {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }

            // Status filter: exact match
            if let Some(sf) = status_filter {
                match &h.status {
                    Some(s) if s == sf => {}
                    _ => return false,
                }
            }

            // By default, exclude terminal handles
            if !include_all
                && let Some(ref s) = h.status
                && lattice.terminal.contains(s)
            {
                return false;
            }

            true
        })
        .map(|(_, h)| {
            let kind_str = match &h.kind {
                HandleKind::File(_) => "file",
                HandleKind::Section { .. } => "section",
                HandleKind::Label { .. } => "label",
                HandleKind::Version { .. } => "version",
            };
            FindMatch {
                id: h.id.clone(),
                kind: kind_str.to_string(),
                status: h.status.clone(),
                file: h.file_path.as_ref().map(ToString::to_string),
            }
        })
        .collect();

    matches.sort_by(|a, b| a.id.cmp(&b.id));
    let total = matches.len();

    FindOutput {
        query: query.to_string(),
        matches,
        total,
    }
}

// ---------------------------------------------------------------------------
// Graph summary (moved from main.rs)
// ---------------------------------------------------------------------------

/// Output of bare `anneal` (no subcommand): graph summary.
#[derive(Serialize)]
pub(crate) struct GraphSummary {
    pub(crate) root: String,
    pub(crate) files: usize,
    pub(crate) handles: usize,
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

impl GraphSummary {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "anneal: knowledge graph built")?;
        writeln!(w, "  root: {}", self.root)?;
        writeln!(w, "  files: {}", self.files)?;
        writeln!(w, "  handles: {}", self.handles)?;
        writeln!(w, "  edges: {}", self.edges)?;
        writeln!(
            w,
            "  namespaces: {} ({})",
            self.namespaces.len(),
            self.namespaces.join(", ")
        )?;
        writeln!(
            w,
            "  labels resolved: {}, skipped: {}",
            self.labels_resolved, self.labels_skipped
        )?;
        writeln!(w, "  versions resolved: {}", self.versions)?;
        writeln!(
            w,
            "  pending edges resolved: {}, unresolved: {}",
            self.pending_edges_resolved, self.pending_edges_unresolved
        )?;
        writeln!(w, "  lattice: {:?}", self.lattice_kind)?;

        if self.lattice_kind == crate::lattice::LatticeKind::Confidence {
            writeln!(
                w,
                "  statuses: {} observed ({} active, {} terminal)",
                self.observed_statuses, self.active_statuses, self.terminal_statuses
            )?;
        }
        Ok(())
    }
}

/// Build a `GraphSummary` from pipeline results.
pub(crate) fn build_summary(
    root: &str,
    file_count: usize,
    graph: &DiGraph,
    stats: &ResolveStats,
    lattice: &Lattice,
) -> GraphSummary {
    GraphSummary {
        root: root.to_string(),
        files: file_count,
        handles: graph.node_count(),
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
pub(crate) fn sorted_namespace_names(ns: &std::collections::HashSet<String>) -> Vec<String> {
    let mut list: Vec<String> = ns.iter().cloned().collect();
    list.sort_unstable();
    list
}
