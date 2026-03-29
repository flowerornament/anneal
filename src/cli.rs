use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use anyhow::Context;
use camino::Utf8Path;
use serde::Serialize;

use crate::checks::{self, Diagnostic, Severity};
use crate::config::{
    AnnealConfig, ConvergenceConfig, Direction, FreshnessConfig, FrontmatterConfig,
    FrontmatterFieldMapping, HandlesConfig,
};
use crate::graph::{DiGraph, Edge};
use crate::handle::{HandleKind, NodeId};
use crate::impact;
use crate::lattice::Lattice;
use crate::parse::PendingEdge;
use crate::resolve::ResolveStats;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Look up a handle by exact match, falling back to case-insensitive search.
fn lookup_handle(node_index: &HashMap<String, NodeId>, handle: &str) -> Option<NodeId> {
    node_index.get(handle).copied().or_else(|| {
        let lower = handle.to_lowercase();
        node_index
            .iter()
            .find(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, &id)| id)
    })
}

/// Deduplicate edges by (kind, other_node) and build `EdgeSummary` list.
fn dedup_edges(
    edges: &[Edge],
    other_node: impl Fn(&Edge) -> NodeId,
    direction: &str,
    graph: &DiGraph,
) -> Vec<EdgeSummary> {
    let mut seen = BTreeSet::new();
    edges
        .iter()
        .filter_map(|e| {
            let kind = e.kind.as_str().to_string();
            let target = graph.node(other_node(e)).id.clone();
            if seen.insert((kind.clone(), target.clone())) {
                Some(EdgeSummary {
                    kind,
                    target,
                    direction: direction.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

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
    pub(crate) suggestions: usize,
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
            "{} errors, {} warnings, {} info, {} suggestions",
            self.errors, self.warnings, self.info, self.suggestions
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
    let suggestions = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Suggestion)
        .count();

    CheckOutput {
        diagnostics,
        errors,
        warnings,
        info,
        suggestions,
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

/// Maximum number of edges to display in human-readable output per direction.
const EDGE_DISPLAY_LIMIT: usize = 20;

/// Frontmatter keys that are metadata-only (not edge-producing references).
const METADATA_ONLY_KEYS: &[&str] = &["status", "updated", "title", "description", "tags", "date"];

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
            let total = self.outgoing_edges.len();
            for edge in self.outgoing_edges.iter().take(EDGE_DISPLAY_LIMIT) {
                writeln!(w, "    {} -> {}", edge.kind, edge.target)?;
            }
            if total > EDGE_DISPLAY_LIMIT {
                writeln!(
                    w,
                    "    ... and {} more outgoing edges ({total} unique)",
                    total - EDGE_DISPLAY_LIMIT
                )?;
            }
        }
        if !self.incoming_edges.is_empty() {
            writeln!(w, "  Incoming:")?;
            let total = self.incoming_edges.len();
            for edge in self.incoming_edges.iter().take(EDGE_DISPLAY_LIMIT) {
                writeln!(w, "    {} <- {}", edge.kind, edge.target)?;
            }
            if total > EDGE_DISPLAY_LIMIT {
                writeln!(
                    w,
                    "    ... and {} more incoming edges ({total} unique)",
                    total - EDGE_DISPLAY_LIMIT
                )?;
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
    let node_id = lookup_handle(node_index, handle)?;

    let h = graph.node(node_id);
    let file = h.file_path.as_ref().map(ToString::to_string);

    let outgoing_edges = dedup_edges(graph.outgoing(node_id), |e| e.target, "outgoing", graph);
    let incoming_edges = dedup_edges(graph.incoming(node_id), |e| e.source, "incoming", graph);

    Some(GetOutput {
        id: h.id.clone(),
        kind: h.kind.as_str().to_string(),
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

/// Filter options for the find command.
#[derive(Default)]
pub(crate) struct FindFilters<'a> {
    pub(crate) namespace: Option<&'a str>,
    pub(crate) status: Option<&'a str>,
    pub(crate) kind: Option<&'a str>,
    pub(crate) include_all: bool,
}

/// Search handle identities with case-insensitive substring matching.
pub(crate) fn cmd_find(
    graph: &DiGraph,
    lattice: &Lattice,
    query: &str,
    filters: &FindFilters<'_>,
) -> FindOutput {
    let lower_query = query.to_lowercase();

    let mut matches: Vec<FindMatch> = graph
        .nodes()
        .filter(|(_, h)| {
            // Substring match on handle identity
            if !h.id.to_lowercase().contains(&lower_query) {
                return false;
            }

            if let Some(kf) = filters.kind
                && h.kind.as_str() != kf
            {
                return false;
            }

            if let Some(ns) = filters.namespace {
                match &h.kind {
                    HandleKind::Label { prefix, .. } => {
                        if prefix != ns {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }

            if let Some(sf) = filters.status {
                match &h.status {
                    Some(s) if s == sf => {}
                    _ => return false,
                }
            }

            // Exclude terminal handles unless user explicitly filtered by status
            if !filters.include_all
                && filters.status.is_none()
                && let Some(ref s) = h.status
                && lattice.terminal.contains(s)
            {
                return false;
            }

            true
        })
        .map(|(_, h)| FindMatch {
            id: h.id.clone(),
            kind: h.kind.as_str().to_string(),
            status: h.status.clone(),
            file: h.file_path.as_ref().map(ToString::to_string),
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
// Impact command (CLI-07)
// ---------------------------------------------------------------------------

/// Output of `anneal impact <handle>`: affected handles.
#[derive(Serialize)]
pub(crate) struct ImpactOutput {
    pub(crate) handle: String,
    pub(crate) direct: Vec<String>,
    pub(crate) indirect: Vec<String>,
}

impl ImpactOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "Directly affected (depend on this):")?;
        if self.direct.is_empty() {
            writeln!(w, "  (none)")?;
        } else {
            for id in &self.direct {
                writeln!(w, "  {id}")?;
            }
        }
        writeln!(w, "Indirectly affected (depend on the above):")?;
        if self.indirect.is_empty() {
            writeln!(w, "  (none)")?;
        } else {
            for id in &self.indirect {
                writeln!(w, "  {id}")?;
            }
        }
        Ok(())
    }
}

/// Compute impact analysis for a handle.
pub(crate) fn cmd_impact(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    handle: &str,
) -> Option<ImpactOutput> {
    let node_id = lookup_handle(node_index, handle)?;

    let result = impact::compute_impact(graph, node_id);

    let direct: Vec<String> = result
        .direct
        .iter()
        .map(|&id| graph.node(id).id.clone())
        .collect();
    let indirect: Vec<String> = result
        .indirect
        .iter()
        .map(|&id| graph.node(id).id.clone())
        .collect();

    Some(ImpactOutput {
        handle: graph.node(node_id).id.clone(),
        direct,
        indirect,
    })
}

// ---------------------------------------------------------------------------
// Init command (CLI-06, CONFIG-04)
// ---------------------------------------------------------------------------

/// Output of `anneal init`: generated config.
#[derive(Serialize)]
pub(crate) struct InitOutput {
    pub(crate) config: AnnealConfig,
    pub(crate) written: bool,
    pub(crate) path: String,
}

impl InitOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let toml_str =
            toml::to_string_pretty(&self.config).unwrap_or_else(|e| format!("# error: {e}"));
        if self.written {
            writeln!(w, "Wrote config to {}", self.path)?;
            writeln!(w)?;
        } else {
            writeln!(w, "# anneal.toml (dry run -- not written)")?;
            writeln!(w)?;
        }
        write!(w, "{toml_str}")?;
        Ok(())
    }
}

/// Propose frontmatter field mapping based on field name heuristics (D-07).
/// Returns Some(mapping) only for field names that look like edge-producing references.
/// Scalar metadata fields (version, type, authors, etc.) return None.
fn propose_mapping(field_name: &str) -> Option<FrontmatterFieldMapping> {
    let lower = field_name.to_lowercase();
    match lower.as_str() {
        "affects" | "impacts" => Some(FrontmatterFieldMapping {
            edge_kind: "DependsOn".to_string(),
            direction: Direction::Inverse,
        }),
        "source" | "sources" | "based-on" | "builds-on" | "extends" | "parent" => {
            Some(FrontmatterFieldMapping {
                edge_kind: "DependsOn".to_string(),
                direction: Direction::Forward,
            })
        }
        "resolves" | "addresses" => Some(FrontmatterFieldMapping {
            edge_kind: "Discharges".to_string(),
            direction: Direction::Forward,
        }),
        "references" | "refs" | "related" | "see-also" | "cites" => Some(FrontmatterFieldMapping {
            edge_kind: "Cites".to_string(),
            direction: Direction::Forward,
        }),
        _ => None, // Scalar metadata — don't propose
    }
}

/// Generate an `AnnealConfig` from inferred structure.
///
/// Scans the lattice, resolve stats, and observed frontmatter keys to build
/// a config that represents the current corpus structure. The D-07 auto-
/// detection adds frontmatter field mappings for keys seen >= 3 times that
/// are not already in the default mapping.
pub(crate) fn cmd_init(
    root: &Utf8Path,
    lattice: &Lattice,
    stats: &ResolveStats,
    observed_frontmatter_keys: &HashMap<String, usize>,
    dry_run: bool,
) -> anyhow::Result<InitOutput> {
    // Build convergence section from lattice
    let mut active: Vec<String> = lattice.active.iter().cloned().collect();
    active.sort();
    let mut terminal: Vec<String> = lattice.terminal.iter().cloned().collect();
    terminal.sort();

    let convergence = ConvergenceConfig {
        active,
        terminal,
        ordering: lattice.ordering.clone(),
    };

    // Build handles section from namespaces
    let mut confirmed: Vec<String> = stats.namespaces.iter().cloned().collect();
    confirmed.sort();

    let handles = HandlesConfig {
        confirmed,
        rejected: Vec::new(),
        linear: Vec::new(),
    };

    // Build frontmatter section: start with defaults, add auto-detected fields
    let default_fm = FrontmatterConfig::default();
    let default_keys: std::collections::HashSet<String> =
        default_fm.fields.keys().cloned().collect();

    let mut fields = default_fm.fields;

    for (key, count) in observed_frontmatter_keys {
        if default_keys.contains(key) || METADATA_ONLY_KEYS.contains(&key.as_str()) {
            continue;
        }
        // Only propose fields seen in >= 3 files with edge-like names
        if *count >= 3
            && let Some(mapping) = propose_mapping(key)
        {
            fields.insert(key.clone(), mapping);
        }
    }

    let frontmatter = FrontmatterConfig { fields };

    let config = AnnealConfig {
        root: String::new(),
        exclude: Vec::new(),
        convergence,
        handles,
        freshness: FreshnessConfig::default(),
        frontmatter,
        concerns: HashMap::new(),
    };

    let config_path = root.join("anneal.toml");
    let path_str = config_path.to_string();

    let written = if dry_run {
        false
    } else {
        let toml_str = toml::to_string_pretty(&config)?;
        std::fs::write(&config_path, toml_str)?;
        true
    };

    Ok(InitOutput {
        config,
        written,
        path: path_str,
    })
}

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

impl GraphSummary {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "anneal: knowledge graph built")?;
        writeln!(w, "  root: {}", self.root)?;
        writeln!(w, "  handles: {}", self.handles)?;
        writeln!(
            w,
            "    {} files, {} labels, {} sections, {} versions",
            self.files, self.labels, self.sections, self.versions_count
        )?;
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
pub(crate) fn sorted_namespace_names(ns: &std::collections::HashSet<String>) -> Vec<String> {
    let mut list: Vec<String> = ns.iter().cloned().collect();
    list.sort_unstable();
    list
}

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
            writeln!(
                w,
                "No history available. Run anneal check or anneal status to create a baseline snapshot."
            )?;
            return Ok(());
        }
        writeln!(w, "Since {}:", self.reference)?;
        writeln!(
            w,
            "  Handles: {:+} ({:+} active, {:+} frozen)",
            self.handle_delta.created, self.handle_delta.active_delta, self.handle_delta.frozen_delta
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
fn diff_snapshots(current: &crate::snapshot::Snapshot, previous: &crate::snapshot::Snapshot, reference: &str) -> DiffOutput {
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
            let total_delta = curr.map_or(0, |s| s.total as i64)
                - prev.map_or(0, |s| s.total as i64);
            let open_delta = curr.map_or(0, |s| s.open as i64)
                - prev.map_or(0, |s| s.open as i64);
            let resolved_delta = curr.map_or(0, |s| s.resolved as i64)
                - prev.map_or(0, |s| s.resolved as i64);

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
fn find_snapshot_by_days(history: &[crate::snapshot::Snapshot], days: u32) -> Option<&crate::snapshot::Snapshot> {
    if history.is_empty() {
        return None;
    }

    let target = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
    let target_ts = target.timestamp();

    history
        .iter()
        .min_by_key(|s| {
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
        let (unresolved_refs, section_ref_count) =
            super::collect_unresolved(&build_result.pending_edges, &node_index);
        let unresolved_owned: Vec<crate::parse::PendingEdge> = unresolved_refs
            .iter()
            .map(|e| crate::parse::PendingEdge {
                source: e.source,
                target_identity: e.target_identity.clone(),
                kind: e.kind,
                inverse: e.inverse,
            })
            .collect();
        let all_diagnostics = crate::checks::run_checks(
            &build_result.graph,
            &lattice,
            &cfg,
            &unresolved_owned,
            section_ref_count,
        );
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
/// 3. Default — diff against last snapshot in history
pub(crate) fn cmd_diff(
    root: &Utf8Path,
    current_snapshot: &crate::snapshot::Snapshot,
    days: Option<u32>,
    git_ref: Option<&str>,
) -> anyhow::Result<DiffOutput> {
    if let Some(git_ref) = git_ref {
        let previous = build_graph_at_git_ref(root, git_ref)?;
        return Ok(diff_snapshots(current_snapshot, &previous, git_ref));
    }

    let history = crate::snapshot::read_history(root);

    if let Some(days) = days {
        if let Some(previous) = find_snapshot_by_days(&history, days) {
            return Ok(diff_snapshots(
                current_snapshot,
                previous,
                &format!("{days} days ago"),
            ));
        }
    } else if let Some(previous) = history.last() {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{
        DiagnosticCounts, EdgeCounts, HandleCounts, NamespaceStats, ObligationCounts, Snapshot,
    };

    fn make_snapshot_base() -> Snapshot {
        Snapshot {
            timestamp: "2026-03-29T00:00:00Z".to_string(),
            handles: HandleCounts {
                total: 100,
                active: 60,
                frozen: 40,
            },
            edges: EdgeCounts { total: 200 },
            states: {
                let mut m = HashMap::new();
                m.insert("draft".to_string(), 30);
                m.insert("formal".to_string(), 20);
                m.insert("archived".to_string(), 40);
                m
            },
            obligations: ObligationCounts {
                outstanding: 5,
                discharged: 10,
                mooted: 3,
            },
            diagnostics: DiagnosticCounts {
                errors: 0,
                warnings: 0,
            },
            namespaces: {
                let mut m = HashMap::new();
                m.insert(
                    "OQ".to_string(),
                    NamespaceStats {
                        total: 69,
                        open: 44,
                        resolved: 19,
                        deferred: 6,
                    },
                );
                m
            },
        }
    }

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

        let output = cmd_diff(root, &current, None, None).expect("cmd_diff");

        assert!(!output.has_history);
        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(
            text.contains("No history available"),
            "Expected no-history message, got: {text}"
        );
    }
}
