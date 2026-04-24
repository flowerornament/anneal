use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::area::area_of_handle;
use crate::config::AnnealConfig;
use crate::graph::DiGraph;
use crate::handle::{Handle, HandleKind, NodeId};
use crate::lattice::Lattice;
use crate::output::{Line, Printer, Render, Tone};

use super::{DetailLevel, OutputMeta, emit_expand_hints, lookup_handle};

// ---------------------------------------------------------------------------
// Map command (CLI-05, KB-C5)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct KindCount {
    pub(crate) kind: String,
    pub(crate) count: usize,
}

#[derive(Serialize)]
pub(crate) struct NamespaceCount {
    pub(crate) namespace: String,
    pub(crate) count: usize,
}

#[derive(Clone, Serialize)]
pub(crate) struct MapNodeEntry {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct MapEdgeEntry {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) kind: String,
}

/// Structured data for the Printer-based `--around` render path. Not
/// serialized: users who want machine-readable neighborhood data should
/// pass `--nodes`/`--edges`/`--json` to get `node_list`/`edge_list`.
pub(crate) struct AroundSummary {
    focus_id: String,
    depth: u32,
    node_count: usize,
    edge_count: usize,
    files: Vec<AroundFile>,
    files_total: usize,
    label_total: usize,
    namespaces: Vec<AroundNamespace>,
    namespaces_total: usize,
    sections: usize,
    versions: usize,
    externals: usize,
    focus_outgoing: Vec<AroundEdge>,
    focus_outgoing_total: usize,
    focus_incoming: Vec<AroundEdge>,
    focus_incoming_total: usize,
    other_edges: Vec<AroundFullEdge>,
    other_edges_total: usize,
    /// True when the neighborhood rendered the full detail view (below
    /// the hub threshold) rather than the hub summary.
    full_view: bool,
}

struct AroundFile {
    id: String,
    status: Option<String>,
}

struct AroundNamespace {
    prefix: String,
    total: usize,
    sample: Vec<String>,
}

struct AroundEdge {
    kind: String,
    other: String,
}

struct AroundFullEdge {
    source: String,
    target: String,
    kind: String,
}

#[derive(Serialize)]
pub(crate) struct MapOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) format: crate::MapRender,
    pub(crate) nodes: usize,
    pub(crate) edges: usize,
    pub(crate) by_kind: Vec<KindCount>,
    pub(crate) top_namespaces: Vec<NamespaceCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node_list: Option<Vec<MapNodeEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) edge_list: Option<Vec<MapEdgeEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rendered_content: Option<String>,
    #[serde(skip)]
    pub(crate) around_summary: Option<AroundSummary>,
}

impl Render for MapOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        if let Some(summary) = &self.around_summary {
            render_around(p, summary)?;
            return emit_expand_hints(p, &self.meta.expand);
        }
        if let Some(content) = &self.rendered_content {
            // Rendered content (text/dot format) — pass through verbatim so
            // `dot -Tpng` still works, but frame expansion hint via Printer.
            for line in content.lines() {
                p.raw_line(line)?;
            }
            return emit_expand_hints(p, &self.meta.expand);
        }

        p.heading("Graph summary", None)?;
        p.tally(&[(self.nodes, "nodes"), (self.edges, "edges")])?;

        if !self.by_kind.is_empty() {
            p.blank()?;
            p.heading("By kind", Some(self.by_kind.len()))?;
            let width = max_count_width(self.by_kind.iter().map(|c| c.count));
            for count in &self.by_kind {
                render_count_row(p, count.count, &count.kind, width)?;
            }
        }
        if !self.top_namespaces.is_empty() {
            p.blank()?;
            p.heading("Top namespaces", Some(self.top_namespaces.len()))?;
            let width = max_count_width(self.top_namespaces.iter().map(|n| n.count));
            for ns in &self.top_namespaces {
                render_count_row(p, ns.count, &ns.namespace, width)?;
            }
        }
        emit_expand_hints(p, &self.meta.expand)
    }
}

fn render_count_row(
    p: &mut Printer,
    count: usize,
    label: &str,
    count_width: usize,
) -> std::io::Result<()> {
    let rendered = format_count(count);
    let pad = count_width.saturating_sub(rendered.len());
    p.line_at(
        4,
        &Line::new()
            .pad(pad)
            .count(count)
            .text("  ")
            .dim(label.to_string()),
    )
}

/// Format a count the way `Line::count` will — thousands-separated —
/// so we can compute an accurate column width before layout.
fn format_count(n: usize) -> String {
    let abs = n.to_string();
    let grouped: Vec<&str> = abs
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).expect("ascii digits"))
        .collect();
    grouped.join(",")
}

fn max_count_width(counts: impl IntoIterator<Item = usize>) -> usize {
    counts
        .into_iter()
        .map(|c| format_count(c).len())
        .max()
        .unwrap_or(0)
}

/// Maximum number of edges to display in map text rendering.
const MAP_EDGE_DISPLAY_LIMIT: usize = 50;
/// Edge count above which a focused neighborhood is treated as a hub summary.
const MAP_HUB_EDGE_THRESHOLD: usize = 50;
/// Label count above which a focused neighborhood is treated as a hub summary.
const MAP_HUB_LABEL_THRESHOLD: usize = 40;
/// Maximum namespaces to show in a hub summary.
const MAP_HUB_NAMESPACE_DISPLAY_LIMIT: usize = 8;
/// Maximum labels to sample per namespace in a hub summary.
const MAP_HUB_LABEL_SAMPLE_LIMIT: usize = 5;
/// Maximum focus-handle edges to sample per direction in a hub summary.
const MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT: usize = 10;
/// Maximum non-focus neighborhood edges to sample in a hub summary.
const MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT: usize = 12;
/// Maximum files to show in a hub summary.
const MAP_HUB_FILE_DISPLAY_LIMIT: usize = 8;

struct TextRenderOutput {
    content: String,
    truncated: bool,
}

/// BFS from `start`, optionally directional and bounded to an area.
///
/// When `area` is set, out-of-area nodes are included as boundary leaves but
/// never expanded — the "area-local handles plus boundary" semantics.
pub(super) fn around_subgraph(
    graph: &DiGraph,
    start: NodeId,
    depth: u32,
    direction: TraversalDirection,
    area: Option<&crate::area::AreaFilter>,
) -> HashSet<NodeId> {
    let follow_outgoing = !matches!(direction, TraversalDirection::Downstream);
    let follow_incoming = !matches!(direction, TraversalDirection::Upstream);

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back((start, 0u32));

    while let Some((current, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }
        if let Some(af) = area
            && current != start
            && !af.matches_handle(graph.node(current))
        {
            continue;
        }
        if follow_outgoing {
            for edge in graph.outgoing(current) {
                if visited.insert(edge.target) {
                    queue.push_back((edge.target, d + 1));
                }
            }
        }
        if follow_incoming {
            for edge in graph.incoming(current) {
                if visited.insert(edge.source) {
                    queue.push_back((edge.source, d + 1));
                }
            }
        }
    }
    visited
}

/// Extract the subgraph of `NodeId`s to render, based on filters.
///
/// - `around`: directional or undirected BFS from this handle to `depth` hops.
/// - `concern`: filter to handles matching concern group patterns from config.
/// - `area`: filter to handles in this area, plus one-hop boundary nodes.
/// - None of the above: all nodes where status is NOT terminal (active graph, D-12).
fn extract_subgraph(opts: &MapOptions<'_>) -> HashSet<NodeId> {
    let graph = opts.graph;
    let node_index = opts.node_index;
    let lattice = opts.lattice;
    let depth = opts.depth;
    if let Some(handle_str) = opts.around {
        let Some(start) = lookup_handle(node_index, handle_str) else {
            return HashSet::new();
        };
        around_subgraph(graph, start, depth, opts.direction, opts.area)
    } else if let Some(concern_name) = opts.concern {
        // Concern group: match patterns from config
        let patterns = opts.config.concerns.get(concern_name);
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
    } else if let Some(af) = opts.area {
        // Area: handles in the area, plus one-hop cross-edge boundary nodes
        let mut matched = HashSet::new();
        for (node_id, handle) in graph.nodes() {
            if af.matches_handle(handle) {
                matched.insert(node_id);
            }
        }
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

/// Collect unique edges within the subgraph (both endpoints in the node set),
/// deduplicated by (source, target, kind). Returned in sorted order.
fn subgraph_edges<'a>(
    graph: &'a DiGraph,
    nodes: &HashSet<NodeId>,
) -> BTreeSet<(NodeId, NodeId, &'a str)> {
    let mut seen = BTreeSet::new();
    for &node_id in nodes {
        for edge in graph.outgoing(node_id) {
            if nodes.contains(&edge.target) {
                seen.insert((edge.source, edge.target, edge.kind.as_str()));
            }
        }
    }
    seen
}

fn format_handle_with_status(handle: &Handle) -> String {
    let status_str = handle
        .status
        .as_deref()
        .map_or(String::new(), |status| format!(" [{status}]"));
    format!("{}{}", handle.id, status_str)
}

fn format_edge_line(graph: &DiGraph, source: NodeId, target: NodeId, kind: &str) -> String {
    format!(
        "  {} -{}-> {}",
        graph.node(source).id,
        kind,
        graph.node(target).id
    )
}

/// Render the subgraph as grouped text (D-12, D-14).
///
/// Groups handles by kind, then by namespace for Labels. Edges are listed
/// separately with deduplication and an optional display limit.
fn render_text_full(
    graph: &DiGraph,
    nodes: &HashSet<NodeId>,
    edge_display_limit: Option<usize>,
) -> TextRenderOutput {
    use std::fmt::Write as FmtWrite;
    let mut out = String::new();

    // Collect handles by kind
    let mut files: Vec<(NodeId, &Handle)> = Vec::new();
    let mut labels_by_ns: HashMap<&str, Vec<(NodeId, &Handle)>> = HashMap::new();
    let mut sections: Vec<(NodeId, &Handle)> = Vec::new();
    let mut versions: Vec<(NodeId, &Handle)> = Vec::new();
    let mut externals: Vec<(NodeId, &Handle)> = Vec::new();

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
            HandleKind::External { .. } => externals.push((node_id, h)),
        }
    }

    // Sort each group for deterministic output
    files.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    sections.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    versions.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    externals.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    // Files
    if !files.is_empty() {
        let _ = writeln!(out, "Files ({}):", files.len());
        for (_, h) in &files {
            let _ = writeln!(out, "  {}", format_handle_with_status(h));
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
                let _ = writeln!(out, "    {}", format_handle_with_status(h));
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
            let _ = writeln!(out, "  {}", format_handle_with_status(h));
        }
        let _ = writeln!(out);
    }

    // External URLs
    if !externals.is_empty() {
        let _ = writeln!(out, "External URLs ({}):", externals.len());
        for (_, h) in &externals {
            let _ = writeln!(out, "  {}", h.id);
        }
        let _ = writeln!(out);
    }

    // Edges within the subgraph
    let edge_lines: Vec<String> = subgraph_edges(graph, nodes)
        .iter()
        .map(|&(src, tgt, kind)| format_edge_line(graph, src, tgt, kind))
        .collect();

    let mut truncated = false;
    if !edge_lines.is_empty() {
        let total = edge_lines.len();
        let _ = writeln!(out, "Edges ({total}):");
        let display_limit = edge_display_limit.unwrap_or(total);
        for line in edge_lines.iter().take(display_limit) {
            let _ = writeln!(out, "{line}");
        }
        if total > display_limit {
            truncated = true;
            let _ = writeln!(out, "  ... and {} more", total - display_limit);
        }
        let _ = writeln!(out);
    }

    TextRenderOutput {
        content: out,
        truncated,
    }
}

fn render_text_hub_summary(
    graph: &DiGraph,
    nodes: &HashSet<NodeId>,
    focus: NodeId,
    depth: u32,
) -> TextRenderOutput {
    use std::fmt::Write as FmtWrite;

    let mut out = String::new();
    let focus_handle = graph.node(focus);
    let edge_set = subgraph_edges(graph, nodes);
    let edge_count = edge_set.len();

    let mut files: Vec<&Handle> = Vec::new();
    let mut labels_by_ns: HashMap<String, Vec<&Handle>> = HashMap::new();
    let mut sections_count = 0usize;
    let mut versions_count = 0usize;
    let mut externals_count = 0usize;

    for &node_id in nodes {
        let handle = graph.node(node_id);
        match &handle.kind {
            HandleKind::File(_) => files.push(handle),
            HandleKind::Label { prefix, .. } => {
                labels_by_ns.entry(prefix.clone()).or_default().push(handle);
            }
            HandleKind::Section { .. } => sections_count += 1,
            HandleKind::Version { .. } => versions_count += 1,
            HandleKind::External { .. } => externals_count += 1,
        }
    }

    files.sort_by(|a, b| a.id.cmp(&b.id));
    let label_count: usize = labels_by_ns.values().map(Vec::len).sum();

    let _ = writeln!(
        out,
        "Neighborhood around {} (depth {}):",
        focus_handle.id, depth
    );
    let _ = writeln!(out, "  {} nodes, {} edges", nodes.len(), edge_count);
    let _ = writeln!(
        out,
        "  {} files, {} labels across {} namespaces",
        files.len(),
        label_count,
        labels_by_ns.len()
    );

    let mut other_handle_counts = Vec::new();
    if sections_count > 0 {
        other_handle_counts.push(format!("{sections_count} sections"));
    }
    if versions_count > 0 {
        other_handle_counts.push(format!("{versions_count} versions"));
    }
    if externals_count > 0 {
        other_handle_counts.push(format!("{externals_count} external URLs"));
    }
    if !other_handle_counts.is_empty() {
        let _ = writeln!(out, "  Other handles: {}", other_handle_counts.join(", "));
    }
    let _ = writeln!(out);

    if !files.is_empty() {
        let shown = files.len().min(MAP_HUB_FILE_DISPLAY_LIMIT);
        let _ = writeln!(out, "Files (showing {shown} of {}):", files.len());
        for handle in files.iter().take(MAP_HUB_FILE_DISPLAY_LIMIT) {
            let _ = writeln!(out, "  {}", format_handle_with_status(handle));
        }
        if files.len() > MAP_HUB_FILE_DISPLAY_LIMIT {
            let _ = writeln!(
                out,
                "  ... and {} more files",
                files.len() - MAP_HUB_FILE_DISPLAY_LIMIT
            );
        }
        let _ = writeln!(out);
    }

    if !labels_by_ns.is_empty() {
        let mut namespaces: Vec<(String, Vec<&Handle>)> = labels_by_ns.into_iter().collect();
        namespaces.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        let total_namespaces = namespaces.len();
        let shown = total_namespaces.min(MAP_HUB_NAMESPACE_DISPLAY_LIMIT);
        let _ = writeln!(out, "Namespaces (showing {shown} of {total_namespaces}):");
        for (namespace, mut handles) in namespaces.into_iter().take(MAP_HUB_NAMESPACE_DISPLAY_LIMIT)
        {
            handles.sort_by(|a, b| a.id.cmp(&b.id));
            let sample: Vec<&str> = handles
                .iter()
                .take(MAP_HUB_LABEL_SAMPLE_LIMIT)
                .map(|handle| handle.id.as_str())
                .collect();
            let suffix = if handles.len() > sample.len() {
                format!(", ... and {} more", handles.len() - sample.len())
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  {namespace} ({}): {}{suffix}",
                handles.len(),
                sample.join(", ")
            );
        }
        if shown < total_namespaces {
            let _ = writeln!(
                out,
                "  ... and {} more namespaces",
                total_namespaces - shown
            );
        }
        let _ = writeln!(out);
    }

    let mut outgoing_edges: Vec<(String, String)> = edge_set
        .iter()
        .filter(|(source, _, _)| *source == focus)
        .map(|(_, target, kind)| (kind.to_string(), graph.node(*target).id.clone()))
        .collect();
    outgoing_edges.sort();

    let mut incoming_edges: Vec<(String, String)> = edge_set
        .iter()
        .filter(|(_, target, _)| *target == focus)
        .map(|(source, _, kind)| (kind.to_string(), graph.node(*source).id.clone()))
        .collect();
    incoming_edges.sort();

    let _ = writeln!(out, "Focus edges for {}:", focus_handle.id);
    if outgoing_edges.is_empty() {
        let _ = writeln!(out, "  Outgoing: none");
    } else {
        let shown = outgoing_edges.len().min(MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT);
        let _ = writeln!(
            out,
            "  Outgoing (showing {shown} of {}):",
            outgoing_edges.len()
        );
        for (kind, target) in outgoing_edges.iter().take(MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT) {
            let _ = writeln!(out, "    {kind} -> {target}");
        }
        if outgoing_edges.len() > MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT {
            let _ = writeln!(
                out,
                "    ... and {} more",
                outgoing_edges.len() - MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT
            );
        }
    }
    if incoming_edges.is_empty() {
        let _ = writeln!(out, "  Incoming: none");
    } else {
        let shown = incoming_edges.len().min(MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT);
        let _ = writeln!(
            out,
            "  Incoming (showing {shown} of {}):",
            incoming_edges.len()
        );
        for (kind, source) in incoming_edges.iter().take(MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT) {
            let _ = writeln!(out, "    {kind} <- {source}");
        }
        if incoming_edges.len() > MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT {
            let _ = writeln!(
                out,
                "    ... and {} more",
                incoming_edges.len() - MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT
            );
        }
    }
    let _ = writeln!(out);

    let other_edges: Vec<String> = edge_set
        .iter()
        .filter(|(source, target, _)| *source != focus && *target != focus)
        .map(|&(source, target, kind)| format_edge_line(graph, source, target, kind))
        .collect();
    if !other_edges.is_empty() {
        let shown = other_edges.len().min(MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT);
        let _ = writeln!(
            out,
            "Other neighborhood edges (showing {shown} of {}):",
            other_edges.len()
        );
        for line in other_edges.iter().take(MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT) {
            let _ = writeln!(out, "{line}");
        }
        if other_edges.len() > MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT {
            let _ = writeln!(
                out,
                "  ... and {} more",
                other_edges.len() - MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT
            );
        }
        let _ = writeln!(out);
    }

    TextRenderOutput {
        content: out,
        truncated: true,
    }
}

fn render_text(
    graph: &DiGraph,
    nodes: &HashSet<NodeId>,
    focus: Option<NodeId>,
    depth: u32,
    full: bool,
) -> TextRenderOutput {
    if full {
        return render_text_full(graph, nodes, None);
    }

    let edge_count = subgraph_edges(graph, nodes).len();
    let label_count = nodes
        .iter()
        .filter(|node_id| matches!(graph.node(**node_id).kind, HandleKind::Label { .. }))
        .count();

    if let Some(focus) = focus
        && (edge_count > MAP_HUB_EDGE_THRESHOLD || label_count > MAP_HUB_LABEL_THRESHOLD)
    {
        return render_text_hub_summary(graph, nodes, focus, depth);
    }

    render_text_full(graph, nodes, Some(MAP_EDGE_DISPLAY_LIMIT))
}

/// Build the structured neighborhood summary used by the Printer path.
/// Mirrors `render_text_hub_summary` / `render_text_full` data selection
/// but returns typed fields so `render_around` can style them.
fn build_around_summary<'g>(
    graph: &'g DiGraph,
    nodes: &HashSet<NodeId>,
    edge_set: &BTreeSet<(NodeId, NodeId, &'g str)>,
    focus: NodeId,
    depth: u32,
) -> AroundSummary {
    let focus_id = graph.node(focus).id.clone();
    let edge_count = edge_set.len();

    let label_count = nodes
        .iter()
        .filter(|node_id| matches!(graph.node(**node_id).kind, HandleKind::Label { .. }))
        .count();
    let full_view = edge_count <= MAP_HUB_EDGE_THRESHOLD && label_count <= MAP_HUB_LABEL_THRESHOLD;

    let mut files: Vec<&Handle> = Vec::new();
    let mut labels_by_ns: HashMap<String, Vec<&Handle>> = HashMap::new();
    let mut sections = 0usize;
    let mut versions = 0usize;
    let mut externals = 0usize;

    for &node_id in nodes {
        let handle = graph.node(node_id);
        match &handle.kind {
            HandleKind::File(_) => files.push(handle),
            HandleKind::Label { prefix, .. } => {
                labels_by_ns.entry(prefix.clone()).or_default().push(handle);
            }
            HandleKind::Section { .. } => sections += 1,
            HandleKind::Version { .. } => versions += 1,
            HandleKind::External { .. } => externals += 1,
        }
    }

    files.sort_by(|a, b| a.id.cmp(&b.id));
    let files_total = files.len();
    let file_limit = if full_view {
        files_total
    } else {
        MAP_HUB_FILE_DISPLAY_LIMIT
    };
    let around_files: Vec<AroundFile> = files
        .iter()
        .take(file_limit)
        .map(|h| AroundFile {
            id: h.id.clone(),
            status: h.status.clone(),
        })
        .collect();

    let label_total: usize = labels_by_ns.values().map(Vec::len).sum();
    let mut namespaces_vec: Vec<(String, Vec<&Handle>)> = labels_by_ns.into_iter().collect();
    namespaces_vec.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    let namespaces_total = namespaces_vec.len();
    let ns_limit = if full_view {
        namespaces_total
    } else {
        MAP_HUB_NAMESPACE_DISPLAY_LIMIT
    };
    let namespaces: Vec<AroundNamespace> = namespaces_vec
        .into_iter()
        .take(ns_limit)
        .map(|(prefix, mut handles)| {
            handles.sort_by(|a, b| a.id.cmp(&b.id));
            let sample_limit = if full_view {
                handles.len()
            } else {
                MAP_HUB_LABEL_SAMPLE_LIMIT
            };
            let total = handles.len();
            let sample = handles
                .iter()
                .take(sample_limit)
                .map(|h| h.id.clone())
                .collect();
            AroundNamespace {
                prefix,
                total,
                sample,
            }
        })
        .collect();

    let mut outgoing: Vec<AroundEdge> = edge_set
        .iter()
        .filter(|(source, _, _)| *source == focus)
        .map(|(_, target, kind)| AroundEdge {
            kind: (*kind).to_string(),
            other: graph.node(*target).id.clone(),
        })
        .collect();
    outgoing.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.other.cmp(&b.other)));

    let mut incoming: Vec<AroundEdge> = edge_set
        .iter()
        .filter(|(_, target, _)| *target == focus)
        .map(|(source, _, kind)| AroundEdge {
            kind: (*kind).to_string(),
            other: graph.node(*source).id.clone(),
        })
        .collect();
    incoming.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.other.cmp(&b.other)));

    let outgoing_total = outgoing.len();
    let incoming_total = incoming.len();
    let edge_display_limit = if full_view {
        outgoing_total.max(incoming_total)
    } else {
        MAP_HUB_FOCUS_EDGE_DISPLAY_LIMIT
    };
    outgoing.truncate(edge_display_limit);
    incoming.truncate(edge_display_limit);

    let other_all: Vec<AroundFullEdge> = edge_set
        .iter()
        .filter(|(source, target, _)| *source != focus && *target != focus)
        .map(|&(source, target, kind)| AroundFullEdge {
            source: graph.node(source).id.clone(),
            target: graph.node(target).id.clone(),
            kind: kind.to_string(),
        })
        .collect();
    let other_edges_total = other_all.len();
    let other_limit = if full_view {
        other_edges_total
    } else {
        MAP_HUB_NEIGHBOR_EDGE_DISPLAY_LIMIT
    };
    let other_edges: Vec<AroundFullEdge> = other_all.into_iter().take(other_limit).collect();

    AroundSummary {
        focus_id,
        depth,
        node_count: nodes.len(),
        edge_count,
        files: around_files,
        files_total,
        label_total,
        namespaces,
        namespaces_total,
        sections,
        versions,
        externals,
        focus_outgoing: outgoing,
        focus_outgoing_total: outgoing_total,
        focus_incoming: incoming,
        focus_incoming_total: incoming_total,
        other_edges,
        other_edges_total,
        full_view,
    }
}

fn render_around(p: &mut Printer, s: &AroundSummary) -> std::io::Result<()> {
    p.line(
        &Line::new()
            .heading("Neighborhood")
            .text("  ")
            .path(s.focus_id.clone())
            .dim(format!("  depth {depth}", depth = s.depth)),
    )?;
    p.blank()?;

    let mut meta = Line::new()
        .count(s.node_count)
        .text(" nodes, ")
        .count(s.edge_count)
        .text(" edges, ")
        .count(s.files_total)
        .text(" files");
    if s.label_total > 0 {
        meta = meta
            .text(", ")
            .count(s.label_total)
            .text(" labels across ")
            .count(s.namespaces_total)
            .text(" namespaces");
    }
    if s.sections > 0 {
        meta = meta.text(", ").count(s.sections).text(" sections");
    }
    if s.versions > 0 {
        meta = meta.text(", ").count(s.versions).text(" versions");
    }
    if s.externals > 0 {
        meta = meta.text(", ").count(s.externals).text(" external URLs");
    }
    p.line_at(2, &meta)?;

    if !s.files.is_empty() {
        p.blank()?;
        p.heading("Files", Some(s.files_total))?;
        if s.files_total > s.files.len() {
            p.caption(&format!("showing {} of {}", s.files.len(), s.files_total))?;
        }
        for file in &s.files {
            let mut row = Line::new().path(file.id.clone());
            if let Some(status) = &file.status {
                row = row.dim(format!("  [{status}]"));
            }
            p.line_at(4, &row)?;
        }
        if s.files_total > s.files.len() {
            let more = s.files_total - s.files.len();
            p.line_at(4, &Line::new().dim(format!("… {more} more")))?;
        }
    }

    if !s.namespaces.is_empty() {
        p.blank()?;
        p.heading("Namespaces", Some(s.namespaces_total))?;
        if s.namespaces_total > s.namespaces.len() {
            p.caption(&format!(
                "showing {} of {}",
                s.namespaces.len(),
                s.namespaces_total
            ))?;
        }
        for ns in &s.namespaces {
            let more = ns.total - ns.sample.len();
            let mut row = Line::new()
                .toned(Tone::Heading, ns.prefix.clone())
                .dim(format!(" ({})", ns.total))
                .text("  ")
                .text(ns.sample.join(", "));
            if more > 0 {
                row = row.dim(format!(", … {more} more"));
            }
            p.line_at(4, &row)?;
        }
        if s.namespaces_total > s.namespaces.len() {
            let more = s.namespaces_total - s.namespaces.len();
            p.line_at(4, &Line::new().dim(format!("… {more} more namespaces")))?;
        }
    }

    p.blank()?;
    p.heading(
        "Focus edges",
        Some(s.focus_outgoing_total + s.focus_incoming_total),
    )?;
    render_focus_edges(
        p,
        "Outgoing",
        &s.focus_outgoing,
        s.focus_outgoing_total,
        "→",
    )?;
    render_focus_edges(
        p,
        "Incoming",
        &s.focus_incoming,
        s.focus_incoming_total,
        "←",
    )?;

    if !s.other_edges.is_empty() {
        p.blank()?;
        p.heading("Other neighborhood edges", Some(s.other_edges_total))?;
        if s.other_edges_total > s.other_edges.len() {
            p.caption(&format!(
                "showing {} of {}",
                s.other_edges.len(),
                s.other_edges_total
            ))?;
        }
        for edge in &s.other_edges {
            p.line_at(
                4,
                &Line::new()
                    .path(edge.source.clone())
                    .dim(format!(" -{}-> ", edge.kind))
                    .path(edge.target.clone()),
            )?;
        }
        if s.other_edges_total > s.other_edges.len() {
            let more = s.other_edges_total - s.other_edges.len();
            p.line_at(4, &Line::new().dim(format!("… {more} more")))?;
        }
    }

    Ok(())
}

fn render_focus_edges(
    p: &mut Printer,
    label: &str,
    edges: &[AroundEdge],
    total: usize,
    arrow: &str,
) -> std::io::Result<()> {
    if total == 0 {
        p.line_at(4, &Line::new().toned(Tone::Heading, label).dim(": none"))?;
        return Ok(());
    }
    let caption = if total > edges.len() {
        format!("{label} (showing {} of {total})", edges.len())
    } else {
        format!("{label} ({total})")
    };
    p.line_at(4, &Line::new().toned(Tone::Heading, caption))?;
    for edge in edges {
        p.line_at(
            6,
            &Line::new()
                .dim(format!("{} {arrow} ", edge.kind))
                .path(edge.other.clone()),
        )?;
    }
    if total > edges.len() {
        let more = total - edges.len();
        p.line_at(6, &Line::new().dim(format!("… {more} more")))?;
    }
    Ok(())
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
    let mut node_list: Vec<(NodeId, &Handle)> =
        nodes.iter().map(|&id| (id, graph.node(id))).collect();
    node_list.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    for (_, h) in &node_list {
        let shape = match &h.kind {
            HandleKind::File(_) => "note",
            HandleKind::Label { .. } => "box",
            HandleKind::Section { .. } => "ellipse",
            HandleKind::Version { .. } => "diamond",
            HandleKind::External { .. } => "oval",
        };
        let status_label = h
            .status
            .as_deref()
            .map_or(String::new(), |s| format!("\\n[{s}]"));
        let id_escaped = dot_escape(&h.id);
        let is_terminal = h.is_terminal(lattice);
        let color_attr = if is_terminal {
            ", style=filled, fillcolor=grey"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  \"{id_escaped}\" [shape={shape}, label=\"{id_escaped}{status_label}\"{color_attr}];",
        );
    }

    let _ = writeln!(out);

    // Edges
    for (src_id, tgt_id, kind) in subgraph_edges(graph, nodes) {
        let src = dot_escape(&graph.node(src_id).id);
        let tgt = dot_escape(&graph.node(tgt_id).id);
        let _ = writeln!(out, "  \"{src}\" -> \"{tgt}\" [label=\"{kind}\"];");
    }

    let _ = writeln!(out, "}}");
    out
}

/// Direction of a `map --around` walk.
///
/// `Both` is the historic undirected neighborhood; `Upstream` follows outgoing
/// edges only (what the handle builds on); `Downstream` follows incoming edges
/// only (what depends on the handle).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TraversalDirection {
    #[default]
    Both,
    Upstream,
    Downstream,
}

/// Options for the `anneal map` command.
pub(crate) struct MapOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) node_index: &'a HashMap<String, NodeId>,
    pub(crate) lattice: &'a Lattice,
    pub(crate) config: &'a AnnealConfig,
    pub(crate) concern: Option<&'a str>,
    pub(crate) around: Option<&'a str>,
    pub(crate) direction: TraversalDirection,
    pub(crate) area: Option<&'a crate::area::AreaFilter>,
    pub(crate) temporal: Option<&'a crate::area::TemporalFilter>,
    pub(crate) depth: u32,
    pub(crate) render: crate::MapRender,
    pub(crate) include_nodes: bool,
    pub(crate) include_edges: bool,
    pub(crate) full: bool,
    pub(crate) limit_nodes: usize,
    pub(crate) limit_edges: usize,
}

fn map_kind_counts(graph: &DiGraph, nodes: &HashSet<NodeId>) -> Vec<KindCount> {
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for node_id in nodes {
        *counts
            .entry(graph.node(*node_id).kind.as_str())
            .or_insert(0) += 1;
    }

    let mut counts: Vec<KindCount> = counts
        .into_iter()
        .map(|(kind, count)| KindCount {
            kind: kind.to_string(),
            count,
        })
        .collect();
    counts.sort_by(|a, b| a.kind.cmp(&b.kind));
    counts
}

fn map_top_namespaces(graph: &DiGraph, nodes: &HashSet<NodeId>) -> Vec<NamespaceCount> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for node_id in nodes {
        if let HandleKind::Label { prefix, .. } = &graph.node(*node_id).kind {
            *counts.entry(prefix.clone()).or_insert(0) += 1;
        }
    }

    let mut namespaces: Vec<NamespaceCount> = counts
        .into_iter()
        .map(|(namespace, count)| NamespaceCount { namespace, count })
        .collect();
    namespaces.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.namespace.cmp(&b.namespace))
    });
    namespaces.truncate(10);
    namespaces
}

fn build_map_node_entries(
    graph: &DiGraph,
    nodes: &HashSet<NodeId>,
    full: bool,
    limit_nodes: usize,
) -> (Vec<MapNodeEntry>, bool) {
    let mut list: Vec<MapNodeEntry> = nodes
        .iter()
        .map(|node_id| {
            let handle = graph.node(*node_id);
            MapNodeEntry {
                id: handle.id.clone(),
                kind: handle.kind.as_str().to_string(),
                status: handle.status.clone(),
                file: handle.file_path.as_ref().map(ToString::to_string),
            }
        })
        .collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    let truncated = !full && list.len() > limit_nodes;
    if truncated {
        list.truncate(limit_nodes);
    }
    (list, truncated)
}

fn build_map_edge_entries(
    graph: &DiGraph,
    nodes: &HashSet<NodeId>,
    full: bool,
    limit_edges: usize,
) -> (Vec<MapEdgeEntry>, bool) {
    let mut list: Vec<MapEdgeEntry> = subgraph_edges(graph, nodes)
        .into_iter()
        .map(|(source, target, kind)| MapEdgeEntry {
            source: graph.node(source).id.clone(),
            target: graph.node(target).id.clone(),
            kind: kind.to_string(),
        })
        .collect();
    let truncated = !full && list.len() > limit_edges;
    if truncated {
        list.truncate(limit_edges);
    }
    (list, truncated)
}

/// Render or summarize the knowledge graph (CLI-05, KB-C5).
pub(crate) fn cmd_map(opts: &MapOptions<'_>) -> MapOutput {
    let mut nodes = extract_subgraph(opts);
    // Apply temporal filter as a post-processing step so it composes
    // with area, concern, and around focuses.
    if let Some(tf) = opts.temporal {
        nodes.retain(|&nid| tf.matches_handle(opts.graph.node(nid)));
    }
    let edge_set = subgraph_edges(opts.graph, &nodes);
    let edge_count = edge_set.len();
    let by_kind = map_kind_counts(opts.graph, &nodes);
    let top_namespaces = map_top_namespaces(opts.graph, &nodes);

    let (node_list, nodes_truncated) = if opts.include_nodes {
        let (list, truncated) =
            build_map_node_entries(opts.graph, &nodes, opts.full, opts.limit_nodes);
        (Some(list), truncated)
    } else {
        (None, false)
    };

    let (edge_list, edges_truncated) = if opts.include_edges {
        let (list, truncated) =
            build_map_edge_entries(opts.graph, &nodes, opts.full, opts.limit_edges);
        (Some(list), truncated)
    } else {
        (None, false)
    };

    let mut rendered_truncated = false;
    let around_focus = opts
        .around
        .and_then(|handle| lookup_handle(opts.node_index, handle));
    let mut around_summary: Option<AroundSummary> = None;
    let rendered_content = match opts.render {
        crate::MapRender::Summary => None,
        crate::MapRender::Dot => Some(render_dot(opts.graph, &nodes, opts.lattice)),
        crate::MapRender::Text => {
            let rendered = render_text(opts.graph, &nodes, around_focus, opts.depth, opts.full);
            rendered_truncated = rendered.truncated;
            Some(rendered.content)
        }
        crate::MapRender::Around => {
            if let Some(focus) = around_focus {
                let summary =
                    build_around_summary(opts.graph, &nodes, &edge_set, focus, opts.depth);
                if !summary.full_view {
                    rendered_truncated = true;
                }
                around_summary = Some(summary);
            }
            None
        }
    };

    let expand = if opts.full {
        Vec::new()
    } else if opts.around.is_some() || opts.concern.is_some() {
        vec![
            "--nodes".to_string(),
            "--edges".to_string(),
            "--render text --full".to_string(),
        ]
    } else {
        vec![
            "--around <handle>".to_string(),
            "--nodes".to_string(),
            "--edges".to_string(),
            "--render text --full".to_string(),
        ]
    };

    MapOutput {
        meta: OutputMeta::new(
            if opts.full {
                DetailLevel::Full
            } else if opts.render == crate::MapRender::Summary {
                DetailLevel::Summary
            } else {
                DetailLevel::Sample
            },
            nodes_truncated || edges_truncated || rendered_truncated,
            node_list
                .as_ref()
                .map(Vec::len)
                .or(edge_list.as_ref().map(Vec::len)),
            node_list
                .as_ref()
                .map(|_| nodes.len())
                .or(edge_list.as_ref().map(|_| edge_count)),
            expand,
        ),
        format: opts.render,
        nodes: nodes.len(),
        edges: edge_count,
        by_kind,
        top_namespaces,
        node_list,
        edge_list,
        rendered_content,
        around_summary,
    }
}

// ---------------------------------------------------------------------------
// map --by-area: area-level topology
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub(crate) struct AreaNode {
    pub(crate) name: String,
    pub(crate) files: usize,
    pub(crate) handles: usize,
}

/// A cross-area directed edge with aggregated count.
#[derive(Clone, Serialize)]
pub(crate) struct AreaEdge {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) count: usize,
}

/// Output of `anneal map --by-area`.
#[derive(Serialize)]
pub(crate) struct MapByAreaOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) format: crate::MapRender,
    pub(crate) areas: Vec<AreaNode>,
    pub(crate) edges: Vec<AreaEdge>,
    pub(crate) islands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rendered_content: Option<String>,
}

impl Render for MapByAreaOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        if let Some(content) = &self.rendered_content {
            for line in content.lines() {
                p.raw_line(line)?;
            }
            return Ok(());
        }

        p.heading("Cross-area edges", Some(self.edges.len()))?;
        if self.edges.is_empty() {
            p.line_at(4, &Line::new().dim("(none)"))?;
        } else {
            let src_width = self
                .edges
                .iter()
                .map(|e| console::measure_text_width(&e.source))
                .max()
                .unwrap_or(0);
            // Right-align the count inside the `—N→` arrow so rows with
            // 1-digit and 3-digit counts have the same arrow column.
            let count_width = self
                .edges
                .iter()
                .map(|e| e.count.to_string().len())
                .max()
                .unwrap_or(0);
            for edge in &self.edges {
                let src_pad =
                    src_width.saturating_sub(console::measure_text_width(&edge.source)) + 2;
                let count_str = edge.count.to_string();
                let count_pad = count_width.saturating_sub(count_str.len());
                p.line_at(
                    4,
                    &Line::new()
                        .path(edge.source.clone())
                        .pad(src_pad)
                        .dim("—")
                        .pad(count_pad)
                        .toned(Tone::Number, count_str)
                        .dim("→ ")
                        .path(edge.target.clone()),
                )?;
            }
        }

        if !self.islands.is_empty() {
            p.blank()?;
            p.heading("Islands", Some(self.islands.len()))?;
            p.caption("areas with zero cross-links")?;
            for island in &self.islands {
                p.line_at(4, &Line::new().path(island.clone()))?;
            }
        }
        Ok(())
    }
}

/// Options for `anneal map --by-area`.
pub(crate) struct MapByAreaOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) render: crate::MapRender,
    pub(crate) min_edges: usize,
    pub(crate) area: Option<&'a crate::area::AreaFilter>,
    pub(crate) include_terminal: bool,
    pub(crate) lattice: &'a Lattice,
}

/// Compute the area-level topology graph.
pub(crate) fn cmd_map_by_area(opts: &MapByAreaOptions<'_>) -> MapByAreaOutput {
    let mut area_files: HashMap<String, HashSet<String>> = HashMap::new();
    let mut area_handles: HashMap<String, usize> = HashMap::new();
    let mut area_edges: HashMap<(String, String), usize> = HashMap::new();

    for (node_id, handle) in opts.graph.nodes() {
        let Some(source_area) = area_of_handle(handle) else {
            continue;
        };

        if !opts.include_terminal && handle.is_terminal(opts.lattice) {
            continue;
        }

        *area_handles.entry(source_area.to_string()).or_insert(0) += 1;
        if let HandleKind::File(path) = &handle.kind {
            area_files
                .entry(source_area.to_string())
                .or_default()
                .insert(path.as_str().to_string());
        }

        for edge in opts.graph.outgoing(node_id) {
            let target = opts.graph.node(edge.target);
            if !opts.include_terminal && target.is_terminal(opts.lattice) {
                continue;
            }
            let Some(target_area) = area_of_handle(target) else {
                continue;
            };
            if target_area == source_area {
                continue;
            }
            *area_edges
                .entry((source_area.to_string(), target_area.to_string()))
                .or_insert(0) += 1;
        }
    }

    let mut areas: Vec<AreaNode> = area_handles
        .iter()
        .map(|(name, handles)| AreaNode {
            name: name.clone(),
            files: area_files.get(name).map_or(0, HashSet::len),
            handles: *handles,
        })
        .collect();
    areas.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));

    // Islands come from the full cross-edge set so --min-edges doesn't
    // promote heavily-linked areas into islands.
    let connected: HashSet<&str> = area_edges
        .keys()
        .flat_map(|(source, target)| [source.as_str(), target.as_str()])
        .collect();
    let mut islands: Vec<String> = areas
        .iter()
        .filter(|a| !connected.contains(a.name.as_str()))
        .map(|a| a.name.clone())
        .collect();
    islands.sort();

    let mut edges: Vec<AreaEdge> = area_edges
        .into_iter()
        .filter(|(_, count)| *count >= opts.min_edges)
        .map(|((source, target), count)| AreaEdge {
            source,
            target,
            count,
        })
        .collect();
    edges.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
    });

    if let Some(af) = opts.area {
        let name = af.name();
        edges.retain(|e| e.source == name || e.target == name);
    }

    let rendered_content = match opts.render {
        crate::MapRender::Dot => Some(render_by_area_dot(&areas, &edges)),
        crate::MapRender::Text | crate::MapRender::Summary | crate::MapRender::Around => None,
    };

    MapByAreaOutput {
        meta: OutputMeta::new(
            DetailLevel::Full,
            false,
            Some(edges.len()),
            Some(edges.len()),
            Vec::new(),
        ),
        format: opts.render,
        areas,
        edges,
        islands,
        rendered_content,
    }
}

fn render_by_area_dot(areas: &[AreaNode], edges: &[AreaEdge]) -> String {
    use std::fmt::Write as FmtWrite;
    let mut out = String::new();
    let _ = writeln!(out, "digraph anneal_areas {{");
    let _ = writeln!(out, "  rankdir=LR;");
    let _ = writeln!(
        out,
        "  node [shape=box, fontname=\"Helvetica\", fontsize=10];"
    );
    let _ = writeln!(out);
    for area in areas {
        let name = dot_escape(&area.name);
        let _ = writeln!(
            out,
            "  \"{name}\" [label=\"{name}\\n{} files\"];",
            area.files
        );
    }
    let _ = writeln!(out);
    for edge in edges {
        let src = dot_escape(&edge.source);
        let tgt = dot_escape(&edge.target);
        let _ = writeln!(out, "  \"{src}\" -> \"{tgt}\" [label=\"{}\"];", edge.count);
    }
    let _ = writeln!(out, "}}");
    out
}

#[cfg(test)]
mod tests {
    use crate::cli::test_helpers::*;
    use crate::config::AnnealConfig;
    use crate::graph::EdgeKind;
    use crate::handle::Handle;

    use super::*;

    #[test]
    fn map_text_renders_all_active_handles_grouped_by_kind() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("doc.md", None));
        graph.add_node(Handle::test_label("OQ", 1, None));

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("Files (1):")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("doc.md")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("Labels:")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("OQ (1):")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("OQ-1")
        );
        assert_eq!(output.nodes, 2);
    }

    #[test]
    fn map_excludes_terminal_handles_by_default() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("active.md", Some("draft")));
        graph.add_node(Handle::test_file("settled.md", Some("done")));

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_new(&[], &["done"]);
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        // File handles are always included per D-12 ("Include all File handles regardless of status")
        // But terminal labels/sections/versions ARE excluded
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("active.md")
        );
        // Files always included for structure
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("settled.md")
        );
    }

    #[test]
    fn map_text_groups_labels_by_namespace() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_label("OQ", 1, None));
        graph.add_node(Handle::test_label("OQ", 64, None));
        graph.add_node(Handle::test_label("FM", 1, None));

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("OQ (2):")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("FM (1):")
        );
    }

    #[test]
    fn map_dot_starts_with_digraph() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("a.md", None));

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Dot,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .starts_with("digraph anneal {")
        );
        assert_eq!(output.format, crate::MapRender::Dot);
    }

    #[test]
    fn map_dot_contains_edge_format() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("a.md", None));
        let b = graph.add_node(Handle::test_file("b.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Dot,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("\"a.md\" -> \"b.md\"")
        );
    }

    #[test]
    fn map_around_extracts_bfs_neighborhood() {
        // a -> b -> c -> d
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("a.md", None));
        let b = graph.add_node(Handle::test_file("b.md", None));
        let c = graph.add_node(Handle::test_file("c.md", None));
        let d = graph.add_node(Handle::test_file("d.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);
        graph.add_edge(c, d, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        // Depth 1 from b: should include a (reverse), b, c (forward)
        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("b.md"),
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 1,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("a.md")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("b.md")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("c.md")
        );
        assert!(
            !output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("d.md"),
            "d.md should be beyond depth 1"
        );
        assert_eq!(output.nodes, 3);
    }

    #[test]
    fn map_around_upstream_follows_only_outgoing_edges() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("a.md", None));
        let b = graph.add_node(Handle::test_file("b.md", None));
        let c = graph.add_node(Handle::test_file("c.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("a.md"),
            direction: TraversalDirection::Upstream,
            area: None,
            temporal: None,
            depth: 5,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        let content = output.rendered_content.as_deref().expect("rendered");
        assert!(content.contains("a.md"));
        assert!(content.contains("b.md"));
        assert!(content.contains("c.md"));
        assert_eq!(output.nodes, 3);
    }

    #[test]
    fn map_around_downstream_follows_only_incoming_edges() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("a.md", None));
        let b = graph.add_node(Handle::test_file("b.md", None));
        let c = graph.add_node(Handle::test_file("c.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("c.md"),
            direction: TraversalDirection::Downstream,
            area: None,
            temporal: None,
            depth: 5,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        let content = output.rendered_content.as_deref().expect("rendered");
        assert!(content.contains("c.md"));
        assert!(content.contains("b.md"));
        assert!(content.contains("a.md"));
        assert_eq!(output.nodes, 3);
    }

    #[test]
    fn map_around_with_area_includes_boundary_but_does_not_expand() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("compiler/a.md", None));
        let b = graph.add_node(Handle::test_file("compiler/b.md", None));
        let x = graph.add_node(Handle::test_file("other/x.md", None));
        let y = graph.add_node(Handle::test_file("other/y.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(x, a, EdgeKind::DependsOn);
        graph.add_edge(y, x, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();
        let area = crate::area::AreaFilter::new("compiler");

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("compiler/a.md"),
            direction: TraversalDirection::Both,
            area: Some(&area),
            temporal: None,
            depth: 5,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        let content = output.rendered_content.as_deref().expect("rendered");
        assert!(content.contains("compiler/a.md"));
        assert!(content.contains("compiler/b.md"));
        assert!(content.contains("other/x.md"));
        assert!(!content.contains("other/y.md"));
    }

    #[test]
    fn map_around_depth_0_returns_just_handle() {
        let mut graph = DiGraph::new();
        let node_a = graph.add_node(Handle::test_file("a.md", None));
        let node_b = graph.add_node(Handle::test_file("b.md", None));
        graph.add_edge(node_a, node_b, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &node_index,
            lattice: &lattice,
            config: &config,
            concern: None,
            around: Some("a.md"),
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 0,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert_eq!(output.nodes, 1);
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("a.md")
        );
        assert!(
            !output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("b.md")
        );
    }

    #[test]
    fn map_concern_filters_to_matching_handles() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_label("OQ", 1, None));
        graph.add_node(Handle::test_label("OQ", 2, None));
        graph.add_node(Handle::test_label("FM", 1, None));
        graph.add_node(Handle::test_file("unrelated.md", None));

        let node_index = test_node_index(&graph);
        let lattice = Lattice::test_empty();
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
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 2,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("OQ-1")
        );
        assert!(
            output
                .rendered_content
                .as_deref()
                .expect("rendered content")
                .contains("OQ-2")
        );
        // FM-1 may or may not be included (only if connected to OQ handles)
    }

    #[test]
    fn map_summary_omits_rendered_content() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("doc.md", None));
        graph.add_node(Handle::test_label("OQ", 1, None));

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &test_node_index(&graph),
            lattice: &Lattice::test_empty(),
            config: &AnnealConfig::default(),
            concern: None,
            around: None,
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 1,
            render: crate::MapRender::Summary,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert_eq!(output.format, crate::MapRender::Summary);
        assert!(output.rendered_content.is_none());
        assert!(!output.by_kind.is_empty());
    }

    #[test]
    fn map_text_full_shows_all_edges() {
        let mut graph = DiGraph::new();
        let center = graph.add_node(Handle::test_file("center.md", None));
        for number in 1..=60 {
            let target = graph.add_node(Handle::test_label("OQ", number, None));
            graph.add_edge(center, target, EdgeKind::Cites);
        }

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &test_node_index(&graph),
            lattice: &Lattice::test_empty(),
            config: &AnnealConfig::default(),
            concern: None,
            around: Some("center.md"),
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 1,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: true,
            limit_nodes: 100,
            limit_edges: 250,
        });

        let text = output.rendered_content.expect("rendered content");
        assert!(text.contains("Edges (60):"));
        assert!(text.contains("center.md -Cites-> OQ-60"));
        assert!(
            !text.contains("... and 10 more"),
            "full text rendering should not keep the old edge cap: {text}"
        );
    }

    #[test]
    fn map_around_hub_uses_hub_summary() {
        let mut graph = DiGraph::new();
        let hub = graph.add_node(Handle::test_file("LABELS.md", Some("living")));
        let synthesis = graph.add_node(Handle::test_file("synthesis.md", Some("historical")));
        graph.add_edge(synthesis, hub, EdgeKind::DependsOn);

        for number in 1..=30 {
            let label = graph.add_node(Handle::test_label("OQ", number, None));
            graph.add_edge(hub, label, EdgeKind::Cites);
            if number <= 4 {
                graph.add_edge(synthesis, label, EdgeKind::Cites);
            }
        }
        for number in 1..=20 {
            let label = graph.add_node(Handle::test_label("FM", number, None));
            graph.add_edge(hub, label, EdgeKind::DependsOn);
        }

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &test_node_index(&graph),
            lattice: &Lattice::test_empty(),
            config: &AnnealConfig::default(),
            concern: None,
            around: Some("LABELS.md"),
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 1,
            render: crate::MapRender::Text,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        let (writer, buf) = crate::output::test_support::SharedBuf::new();
        let mut p = Printer::new(writer, plain_style());
        output.render(&mut p).expect("render");
        let text = String::from_utf8(buf.borrow().clone()).expect("utf8");

        assert!(text.contains("Neighborhood around LABELS.md (depth 1):"));
        assert!(text.contains("Namespaces (showing 2 of 2):"));
        assert!(text.contains("OQ (30):"));
        assert!(text.contains("FM (20):"));
        assert!(text.contains("Focus edges for LABELS.md:"));
        assert!(text.contains("Other neighborhood edges (showing"));
        assert!(text.contains("--nodes"));
        assert!(text.contains("--edges"));
        assert!(text.contains("--render text --full"));
        assert!(
            !text.contains("OQ-30"),
            "hub summary should sample namespace members instead of dumping them all: {text}"
        );
    }

    #[test]
    fn map_render_around_uses_printer_path() {
        let mut graph = DiGraph::new();
        let hub = graph.add_node(Handle::test_file("LABELS.md", Some("living")));
        let other = graph.add_node(Handle::test_file("cite.md", Some("draft")));
        graph.add_edge(other, hub, EdgeKind::DependsOn);
        for number in 1..=6 {
            let label = graph.add_node(Handle::test_label("OQ", number, None));
            graph.add_edge(hub, label, EdgeKind::Cites);
        }

        let output = cmd_map(&MapOptions {
            graph: &graph,
            node_index: &test_node_index(&graph),
            lattice: &Lattice::test_empty(),
            config: &AnnealConfig::default(),
            concern: None,
            around: Some("LABELS.md"),
            direction: TraversalDirection::Both,
            area: None,
            temporal: None,
            depth: 1,
            render: crate::MapRender::Around,
            include_nodes: false,
            include_edges: false,
            full: false,
            limit_nodes: 100,
            limit_edges: 250,
        });

        assert!(output.rendered_content.is_none());
        assert!(output.around_summary.is_some());
        let summary = output.around_summary.as_ref().expect("around summary");
        assert_eq!(summary.focus_id, "LABELS.md");
        assert_eq!(summary.focus_outgoing_total, 6);
        assert_eq!(summary.focus_incoming_total, 1);

        let (writer, buf) = crate::output::test_support::SharedBuf::new();
        let mut p = Printer::new(writer, plain_style());
        output.render(&mut p).expect("render");
        let text = String::from_utf8(buf.borrow().clone()).expect("utf8");

        assert!(text.contains("Neighborhood"));
        assert!(text.contains("LABELS.md"));
        assert!(text.contains("depth 1"));
        assert!(text.contains("Focus edges"));
        assert!(text.contains("Outgoing"));
        assert!(text.contains("Incoming"));
        assert!(text.contains("Cites → OQ-1"));
        assert!(text.contains("DependsOn ← cite.md"));
        assert!(
            !text.contains(':'),
            "headings should not keep trailing colons (R2): {text}"
        );
    }

    // ---------------------------------------------------------------
    // map --by-area tests
    // ---------------------------------------------------------------

    fn by_area_graph() -> DiGraph {
        let mut graph = DiGraph::new();
        let ca = graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let cb = graph.add_node(Handle::test_file("compiler/b.md", Some("draft")));
        let sa = graph.add_node(Handle::test_file("synthesis/a.md", Some("draft")));
        let sb = graph.add_node(Handle::test_file("synthesis/b.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/a.md", Some("archived")));
        graph.add_edge(ca, sa, EdgeKind::Cites);
        graph.add_edge(cb, sa, EdgeKind::Cites);
        graph.add_edge(cb, sb, EdgeKind::DependsOn);
        graph.add_edge(sa, ca, EdgeKind::Cites);
        graph
    }

    #[test]
    fn map_by_area_aggregates_cross_area_edges() {
        let graph = by_area_graph();
        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 1,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });

        let compiler_to_synthesis = output
            .edges
            .iter()
            .find(|e| e.source == "compiler" && e.target == "synthesis")
            .expect("compiler -> synthesis edge");
        assert_eq!(compiler_to_synthesis.count, 3);

        let synthesis_to_compiler = output
            .edges
            .iter()
            .find(|e| e.source == "synthesis" && e.target == "compiler")
            .expect("synthesis -> compiler edge");
        assert_eq!(synthesis_to_compiler.count, 1);
    }

    #[test]
    fn map_by_area_detects_islands() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/a.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/b.md", Some("draft")));
        let lattice = Lattice::test_new(&["draft"], &[]);

        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 1,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });

        assert!(output.islands.contains(&"archive".to_string()));
        assert!(output.islands.contains(&"compiler".to_string()));
    }

    #[test]
    fn map_by_area_min_edges_filters_but_keeps_islands_stable() {
        let graph = by_area_graph();
        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 2,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });

        // Only compiler->synthesis (count 3) survives
        assert_eq!(output.edges.len(), 1);
        // Synthesis still "connected" via full edge set; not an island
        assert!(!output.islands.iter().any(|i| i == "synthesis"));
    }

    #[test]
    fn map_by_area_human_format_uses_count_arrow() {
        let graph = by_area_graph();
        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 1,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });

        let (writer, buf) = crate::output::test_support::SharedBuf::new();
        let mut p = Printer::new(writer, plain_style());
        output.render(&mut p).expect("render");
        let text = String::from_utf8(buf.borrow().clone()).expect("utf8");
        assert!(
            text.contains("compiler") && text.contains("synthesis"),
            "expected compiler -> synthesis edge, got: {text}"
        );
    }

    fn plain_style() -> crate::output::OutputStyle {
        crate::output::OutputStyle::plain()
    }

    #[test]
    fn map_by_area_dot_render() {
        let graph = by_area_graph();
        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Dot,
            min_edges: 1,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });

        let content = output.rendered_content.expect("dot content");
        assert!(content.starts_with("digraph anneal_areas {"));
        assert!(content.contains("\"compiler\" -> \"synthesis\""));
    }

    #[test]
    fn map_by_area_excludes_terminal_by_default() {
        let mut graph = DiGraph::new();
        let compiler_file = graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let archived_file = graph.add_node(Handle::test_file("archive/a.md", Some("archived")));
        graph.add_edge(compiler_file, archived_file, EdgeKind::Cites);
        let lattice = Lattice::test_new(&["draft"], &["archived"]);

        let output = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 1,
            area: None,
            include_terminal: false,
            lattice: &lattice,
        });
        // archive file is terminal -> edge excluded
        assert!(output.edges.is_empty());

        let output_all = cmd_map_by_area(&MapByAreaOptions {
            graph: &graph,
            render: crate::MapRender::Text,
            min_edges: 1,
            area: None,
            include_terminal: true,
            lattice: &lattice,
        });
        assert_eq!(output_all.edges.len(), 1);
    }
}
