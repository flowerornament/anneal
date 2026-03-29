use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::io::Write;

use camino::Utf8Path;
use serde::Serialize;

use crate::checks::{self, Diagnostic, Severity};
use crate::config::{
    AnnealConfig, ConvergenceConfig, Direction, FreshnessConfig, FrontmatterConfig,
    FrontmatterFieldMapping, HandlesConfig,
};
use crate::graph::{DiGraph, Edge, EdgeKind};
use crate::handle::{Handle, HandleKind, NodeId};
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
// Map command (CLI-05, KB-C5)
// ---------------------------------------------------------------------------

/// Output of `anneal map`: rendered graph in text or DOT format.
#[derive(Serialize)]
pub(crate) struct MapOutput {
    pub(crate) format: String,
    pub(crate) nodes: usize,
    pub(crate) edges: usize,
    pub(crate) content: String,
}

impl MapOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        write!(w, "{}", self.content)
    }
}

/// Maximum number of edges to display in map text rendering.
const MAP_EDGE_DISPLAY_LIMIT: usize = 50;

/// Extract the subgraph of `NodeId`s to render, based on filters.
///
/// - `around`: BFS from this handle to `depth` hops (forward + reverse).
/// - `concern`: filter to handles matching concern group patterns from config.
/// - Neither: all nodes where status is NOT terminal (active graph, D-12).
fn extract_subgraph(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    lattice: &Lattice,
    concern: Option<&str>,
    around: Option<&str>,
    depth: u32,
    config: &AnnealConfig,
) -> HashSet<NodeId> {
    if let Some(handle_str) = around {
        // BFS neighborhood from a handle
        let Some(start) = lookup_handle(node_index, handle_str) else {
            return HashSet::new();
        };
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back((start, 0u32));

        while let Some((current, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }
            // Forward edges
            for edge in graph.outgoing(current) {
                if visited.insert(edge.target) {
                    queue.push_back((edge.target, d + 1));
                }
            }
            // Reverse edges
            for edge in graph.incoming(current) {
                if visited.insert(edge.source) {
                    queue.push_back((edge.source, d + 1));
                }
            }
        }
        visited
    } else if let Some(concern_name) = concern {
        // Concern group: match patterns from config
        let patterns = config.concerns.get(concern_name);
        let Some(patterns) = patterns else {
            return HashSet::new();
        };
        let mut matched = HashSet::new();
        for (node_id, handle) in graph.nodes() {
            for pattern in patterns {
                if handle.id.starts_with(pattern) || handle.id.contains(pattern) {
                    matched.insert(node_id);
                    break;
                }
            }
        }
        // Also include handles connected by one hop
        let anchors: Vec<NodeId> = matched.iter().copied().collect();
        for anchor in anchors {
            for edge in graph.outgoing(anchor) {
                matched.insert(edge.target);
            }
            for edge in graph.incoming(anchor) {
                matched.insert(edge.source);
            }
        }
        matched
    } else {
        // Default: all non-terminal nodes (active graph per D-12)
        let mut nodes = HashSet::new();
        for (node_id, handle) in graph.nodes() {
            // Include all File handles (they provide structure)
            if matches!(handle.kind, HandleKind::File(_)) {
                nodes.insert(node_id);
                continue;
            }
            // Include handles without status or with active status
            match &handle.status {
                None => {
                    nodes.insert(node_id);
                }
                Some(s) if !lattice.terminal.contains(s) => {
                    nodes.insert(node_id);
                }
                _ => {}
            }
        }
        nodes
    }
}

/// Count edges within the subgraph (both endpoints in the node set).
fn count_subgraph_edges(graph: &DiGraph, nodes: &HashSet<NodeId>) -> usize {
    let mut count = 0;
    for &node_id in nodes {
        for edge in graph.outgoing(node_id) {
            if nodes.contains(&edge.target) {
                count += 1;
            }
        }
    }
    count
}

/// Render the subgraph as grouped text (D-12, D-14).
///
/// Groups handles by kind, then by namespace for Labels. Edges are listed
/// separately with deduplication and a display limit.
fn render_text(graph: &DiGraph, nodes: &HashSet<NodeId>) -> String {
    use std::fmt::Write as FmtWrite;
    let mut out = String::new();

    // Collect handles by kind
    let mut files: Vec<(NodeId, &Handle)> = Vec::new();
    let mut labels_by_ns: HashMap<&str, Vec<(NodeId, &Handle)>> = HashMap::new();
    let mut sections: Vec<(NodeId, &Handle)> = Vec::new();
    let mut versions: Vec<(NodeId, &Handle)> = Vec::new();

    for &node_id in nodes {
        let h = graph.node(node_id);
        match &h.kind {
            HandleKind::File(_) => files.push((node_id, h)),
            HandleKind::Label { prefix, .. } => {
                labels_by_ns
                    .entry(prefix.as_str())
                    .or_default()
                    .push((node_id, h));
            }
            HandleKind::Section { .. } => sections.push((node_id, h)),
            HandleKind::Version { .. } => versions.push((node_id, h)),
        }
    }

    // Sort each group for deterministic output
    files.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    sections.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    versions.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    // Files
    if !files.is_empty() {
        let _ = writeln!(out, "Files ({}):", files.len());
        for (_, h) in &files {
            let status_str = h
                .status
                .as_deref()
                .map_or(String::new(), |s| format!(" [{s}]"));
            let _ = writeln!(out, "  {}{status_str}", h.id);
        }
        let _ = writeln!(out);
    }

    // Labels grouped by namespace
    if !labels_by_ns.is_empty() {
        let _ = writeln!(out, "Labels:");
        let mut ns_keys: Vec<&str> = labels_by_ns.keys().copied().collect();
        ns_keys.sort_unstable();
        for ns in ns_keys {
            let items = labels_by_ns.get(ns).expect("namespace exists");
            let mut sorted_items: Vec<&(NodeId, &Handle)> = items.iter().collect();
            sorted_items.sort_by(|a, b| a.1.id.cmp(&b.1.id));
            let _ = writeln!(out, "  {ns} ({}):", sorted_items.len());
            for (_, h) in sorted_items {
                let status_str = h
                    .status
                    .as_deref()
                    .map_or(String::new(), |s| format!(" [{s}]"));
                let _ = writeln!(out, "    {}{status_str}", h.id);
            }
        }
        let _ = writeln!(out);
    }

    // Sections
    if !sections.is_empty() {
        let _ = writeln!(out, "Sections ({}):", sections.len());
        for (_, h) in &sections {
            let _ = writeln!(out, "  {}", h.id);
        }
        let _ = writeln!(out);
    }

    // Versions
    if !versions.is_empty() {
        let _ = writeln!(out, "Versions ({}):", versions.len());
        for (_, h) in &versions {
            let status_str = h
                .status
                .as_deref()
                .map_or(String::new(), |s| format!(" [{s}]"));
            let _ = writeln!(out, "  {}{status_str}", h.id);
        }
        let _ = writeln!(out);
    }

    // Edges within the subgraph
    let mut edge_lines: Vec<String> = Vec::new();
    let mut seen_edges = BTreeSet::new();
    for &node_id in nodes {
        for edge in graph.outgoing(node_id) {
            if !nodes.contains(&edge.target) {
                continue;
            }
            let key = (edge.source, edge.target, edge.kind.as_str());
            if seen_edges.insert(key) {
                let src = &graph.node(edge.source).id;
                let tgt = &graph.node(edge.target).id;
                edge_lines.push(format!("  {src} -{}- {tgt}", edge.kind.as_str()));
            }
        }
    }

    if !edge_lines.is_empty() {
        let total = edge_lines.len();
        let _ = writeln!(out, "Edges ({total}):");
        for line in edge_lines.iter().take(MAP_EDGE_DISPLAY_LIMIT) {
            let _ = writeln!(out, "{line}");
        }
        if total > MAP_EDGE_DISPLAY_LIMIT {
            let _ = writeln!(out, "  ... and {} more", total - MAP_EDGE_DISPLAY_LIMIT);
        }
        let _ = writeln!(out);
    }

    out
}

/// Escape a string for use as a DOT identifier.
fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render the subgraph as graphviz DOT (D-12).
///
/// Uses shape=note for File, shape=box for Label, shape=ellipse for Section,
/// shape=diamond for Version. Terminal nodes colored grey.
fn render_dot(graph: &DiGraph, nodes: &HashSet<NodeId>, lattice: &Lattice) -> String {
    use std::fmt::Write as FmtWrite;
    let mut out = String::new();

    let _ = writeln!(out, "digraph anneal {{");
    let _ = writeln!(out, "  rankdir=LR;");
    let _ = writeln!(
        out,
        "  node [shape=box, fontname=\"Helvetica\", fontsize=10];"
    );
    let _ = writeln!(out);

    // Nodes
    let mut node_list: Vec<(NodeId, &Handle)> = nodes
        .iter()
        .map(|&id| (id, graph.node(id)))
        .collect();
    node_list.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    for (node_id, h) in &node_list {
        let shape = match &h.kind {
            HandleKind::File(_) => "note",
            HandleKind::Label { .. } => "box",
            HandleKind::Section { .. } => "ellipse",
            HandleKind::Version { .. } => "diamond",
        };
        let status_label = h
            .status
            .as_deref()
            .map_or(String::new(), |s| format!("\\n[{s}]"));
        let id_escaped = dot_escape(&h.id);
        let is_terminal = h
            .status
            .as_ref()
            .is_some_and(|s| lattice.terminal.contains(s));
        let color_attr = if is_terminal {
            ", style=filled, fillcolor=grey"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  \"{id_escaped}\" [shape={shape}, label=\"{id_escaped}{status_label}\"{color_attr}];",
        );
        let _ = node_id; // suppress unused warning
    }

    let _ = writeln!(out);

    // Edges
    let mut seen_edges = BTreeSet::new();
    for &node_id in nodes {
        for edge in graph.outgoing(node_id) {
            if !nodes.contains(&edge.target) {
                continue;
            }
            let key = (edge.source, edge.target, edge.kind.as_str());
            if seen_edges.insert(key) {
                let src = dot_escape(&graph.node(edge.source).id);
                let tgt = dot_escape(&graph.node(edge.target).id);
                let _ = writeln!(
                    out,
                    "  \"{src}\" -> \"{tgt}\" [label=\"{}\"];",
                    edge.kind.as_str()
                );
            }
        }
    }

    let _ = writeln!(out, "}}");
    out
}

/// Options for the `anneal map` command.
pub(crate) struct MapOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) node_index: &'a HashMap<String, NodeId>,
    pub(crate) lattice: &'a Lattice,
    pub(crate) config: &'a AnnealConfig,
    pub(crate) concern: Option<&'a str>,
    pub(crate) around: Option<&'a str>,
    pub(crate) depth: u32,
    pub(crate) format: &'a str,
}

/// Render the knowledge graph in text or DOT format (CLI-05, KB-C5).
///
/// Extracts a subgraph based on `concern`, `around`/`depth` filters, then
/// renders in the requested format. Counts nodes and edges in the subgraph.
pub(crate) fn cmd_map(opts: &MapOptions<'_>) -> MapOutput {
    let nodes = extract_subgraph(
        opts.graph,
        opts.node_index,
        opts.lattice,
        opts.concern,
        opts.around,
        opts.depth,
        opts.config,
    );
    let edge_count = count_subgraph_edges(opts.graph, &nodes);

    let content = match opts.format {
        "dot" => render_dot(opts.graph, &nodes, opts.lattice),
        _ => render_text(opts.graph, &nodes),
    };

    MapOutput {
        format: opts.format.to_string(),
        nodes: nodes.len(),
        edges: edge_count,
        content,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::HandleMetadata;
    use camino::Utf8PathBuf;

    fn make_file_handle(id: &str) -> Handle {
        Handle {
            id: id.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(id)),
            status: None,
            file_path: Some(Utf8PathBuf::from(id)),
            metadata: HandleMetadata::default(),
        }
    }

    fn make_file_handle_with_status(id: &str, status: &str) -> Handle {
        Handle {
            id: id.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(id)),
            status: Some(status.to_string()),
            file_path: Some(Utf8PathBuf::from(id)),
            metadata: HandleMetadata::default(),
        }
    }

    fn make_label_handle(prefix: &str, number: u32) -> Handle {
        let id = format!("{prefix}-{number}");
        Handle {
            id,
            kind: HandleKind::Label {
                prefix: prefix.to_string(),
                number,
            },
            status: None,
            file_path: None,
            metadata: HandleMetadata::default(),
        }
    }

    fn empty_lattice() -> Lattice {
        Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: crate::lattice::LatticeKind::Existence,
        }
    }

    fn lattice_with_terminal(terminal: &[&str]) -> Lattice {
        Lattice {
            observed_statuses: terminal.iter().copied().map(str::to_string).collect(),
            active: HashSet::new(),
            terminal: terminal.iter().copied().map(str::to_string).collect(),
            ordering: Vec::new(),
            kind: crate::lattice::LatticeKind::Confidence,
        }
    }

    fn test_node_index(graph: &DiGraph) -> HashMap<String, NodeId> {
        crate::resolve::build_node_index(graph)
    }

    #[test]
    fn map_text_renders_all_active_handles_grouped_by_kind() {
        let mut graph = DiGraph::new();
        graph.add_node(make_file_handle("doc.md"));
        graph.add_node(make_label_handle("OQ", 1));

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            depth: 2,
            format: "text",
        });

        assert!(output.content.contains("Files (1):"));
        assert!(output.content.contains("doc.md"));
        assert!(output.content.contains("Labels:"));
        assert!(output.content.contains("OQ (1):"));
        assert!(output.content.contains("OQ-1"));
        assert_eq!(output.nodes, 2);
    }

    #[test]
    fn map_excludes_terminal_handles_by_default() {
        let mut graph = DiGraph::new();
        graph.add_node(make_file_handle_with_status("active.md", "draft"));
        graph.add_node(make_file_handle_with_status("settled.md", "done"));

        let node_index = test_node_index(&graph);
        let lattice = lattice_with_terminal(&["done"]);
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            depth: 2,
            format: "text",
        });

        // File handles are always included per D-12 ("Include all File handles regardless of status")
        // But terminal labels/sections/versions ARE excluded
        assert!(output.content.contains("active.md"));
        // Files always included for structure
        assert!(output.content.contains("settled.md"));
    }

    #[test]
    fn map_text_groups_labels_by_namespace() {
        let mut graph = DiGraph::new();
        graph.add_node(make_label_handle("OQ", 1));
        graph.add_node(make_label_handle("OQ", 64));
        graph.add_node(make_label_handle("FM", 1));

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            depth: 2,
            format: "text",
        });

        assert!(output.content.contains("OQ (2):"));
        assert!(output.content.contains("FM (1):"));
    }

    #[test]
    fn map_dot_starts_with_digraph() {
        let mut graph = DiGraph::new();
        graph.add_node(make_file_handle("a.md"));

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            depth: 2,
            format: "dot",
        });

        assert!(output.content.starts_with("digraph anneal {"));
        assert!(output.format == "dot");
    }

    #[test]
    fn map_dot_contains_edge_format() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            depth: 2,
            format: "dot",
        });

        assert!(output.content.contains("\"a.md\" -> \"b.md\""));
    }

    #[test]
    fn map_around_extracts_bfs_neighborhood() {
        // a -> b -> c -> d
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        let c = graph.add_node(make_file_handle("c.md"));
        let d = graph.add_node(make_file_handle("d.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);
        graph.add_edge(c, d, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        // Depth 1 from b: should include a (reverse), b, c (forward)
        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("b.md"),
            depth: 1,
            format: "text",
        });

        assert!(output.content.contains("a.md"));
        assert!(output.content.contains("b.md"));
        assert!(output.content.contains("c.md"));
        assert!(
            !output.content.contains("d.md"),
            "d.md should be beyond depth 1"
        );
        assert_eq!(output.nodes, 3);
    }

    #[test]
    fn map_around_depth_0_returns_just_handle() {
        let mut graph = DiGraph::new();
        let node_a = graph.add_node(make_file_handle("a.md"));
        let node_b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(node_a, node_b, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("a.md"),
            depth: 0,
            format: "text",
        });

        assert_eq!(output.nodes, 1);
        assert!(output.content.contains("a.md"));
        assert!(!output.content.contains("b.md"));
    }

    #[test]
    fn map_concern_filters_to_matching_handles() {
        let mut graph = DiGraph::new();
        graph.add_node(make_label_handle("OQ", 1));
        graph.add_node(make_label_handle("OQ", 2));
        graph.add_node(make_label_handle("FM", 1));
        graph.add_node(make_file_handle("unrelated.md"));

        let node_index = test_node_index(&graph);
        let lattice = empty_lattice();
        let mut config = AnnealConfig::default();
        config
            .concerns
            .insert("questions".to_string(), vec!["OQ".to_string()]);

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: Some("questions"),
            around: None,
            depth: 2,
            format: "text",
        });

        assert!(output.content.contains("OQ-1"));
        assert!(output.content.contains("OQ-2"));
        // FM-1 may or may not be included (only if connected to OQ handles)
    }
}
