use std::collections::{BTreeSet, HashMap};
use std::io::Write;

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
