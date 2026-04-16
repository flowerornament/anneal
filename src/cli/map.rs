use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::io::Write;

use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::DiGraph;
use crate::handle::{Handle, HandleKind, NodeId};
use crate::lattice::Lattice;

use super::{DetailLevel, OutputMeta, lookup_handle};

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

#[derive(Serialize)]
pub(crate) struct MapOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) format: String,
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
}

impl MapOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        if let Some(content) = &self.rendered_content {
            write!(w, "{content}")?;
            if !self.meta.expand.is_empty() {
                writeln!(w)?;
                writeln!(w, "Expand with: {}", self.meta.expand.join(", "))?;
            }
            return Ok(());
        }

        writeln!(
            w,
            "Graph summary: {} nodes, {} edges",
            self.nodes, self.edges
        )?;
        if !self.by_kind.is_empty() {
            writeln!(w, "By kind:")?;
            for count in &self.by_kind {
                writeln!(w, "  {} {}", count.count, count.kind)?;
            }
        }
        if !self.top_namespaces.is_empty() {
            writeln!(w, "Top namespaces:")?;
            for ns in &self.top_namespaces {
                writeln!(w, "  {} {}", ns.count, ns.namespace)?;
            }
        }
        if !self.meta.expand.is_empty() {
            writeln!(w)?;
            writeln!(w, "Expand with: {}", self.meta.expand.join(", "))?;
        }
        Ok(())
    }
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

/// Extract the subgraph of `NodeId`s to render, based on filters.
///
/// - `around`: BFS from this handle to `depth` hops (forward + reverse).
/// - `concern`: filter to handles matching concern group patterns from config.
/// - `area`: filter to handles in this area, plus one-hop boundary nodes.
/// - None of the above: all nodes where status is NOT terminal (active graph, D-12).
fn extract_subgraph(opts: &MapOptions<'_>) -> HashSet<NodeId> {
    let graph = opts.graph;
    let node_index = opts.node_index;
    let lattice = opts.lattice;
    let depth = opts.depth;
    if let Some(handle_str) = opts.around {
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

/// Options for the `anneal map` command.
pub(crate) struct MapOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) node_index: &'a HashMap<String, NodeId>,
    pub(crate) lattice: &'a Lattice,
    pub(crate) config: &'a AnnealConfig,
    pub(crate) concern: Option<&'a str>,
    pub(crate) around: Option<&'a str>,
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
    let edge_count = subgraph_edges(opts.graph, &nodes).len();
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
    let rendered_content = match opts.render {
        crate::MapRender::Summary => None,
        crate::MapRender::Dot => Some(render_dot(opts.graph, &nodes, opts.lattice)),
        crate::MapRender::Text => {
            let rendered = render_text(
                opts.graph,
                &nodes,
                opts.around
                    .and_then(|handle| lookup_handle(opts.node_index, handle)),
                opts.depth,
                opts.full,
            );
            rendered_truncated = rendered.truncated;
            Some(rendered.content)
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
        format: match opts.render {
            crate::MapRender::Summary => "summary",
            crate::MapRender::Text => "text",
            crate::MapRender::Dot => "dot",
        }
        .to_string(),
        nodes: nodes.len(),
        edges: edge_count,
        by_kind,
        top_namespaces,
        node_list,
        edge_list,
        rendered_content,
    }
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
        assert!(output.format == "dot");
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

        assert_eq!(output.format, "summary");
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

        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");

        assert!(text.contains("Neighborhood around LABELS.md (depth 1):"));
        assert!(text.contains("Namespaces (showing 2 of 2):"));
        assert!(text.contains("OQ (30):"));
        assert!(text.contains("FM (20):"));
        assert!(text.contains("Focus edges for LABELS.md:"));
        assert!(text.contains("Other neighborhood edges (showing"));
        assert!(text.contains("Expand with: --nodes, --edges, --render text --full"));
        assert!(
            !text.contains("OQ-30"),
            "hub summary should sample namespace members instead of dumping them all: {text}"
        );
    }
}
