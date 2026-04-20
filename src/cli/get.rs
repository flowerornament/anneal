use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::graph::DiGraph;
use crate::handle::NodeId;
use crate::output::{Glyph, Line, OutputStyle, Printer, Tone};

use super::{DetailLevel, OutputMeta, SnippetIndex, dedup_edges, lookup_handle};

// ---------------------------------------------------------------------------
// Batch get (multi-arg lookup — see spec "Batch handle lookup")
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub(crate) struct BatchGetRow {
    pub(crate) handle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) not_found: bool,
}

#[derive(Serialize)]
pub(crate) struct BatchGetOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) rows: Vec<BatchGetRow>,
}

/// Projection mode for batch get output. Mutually exclusive; replaces the
/// earlier `{status_only, context}` flag pair which allowed invalid
/// combinations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BatchGetMode {
    /// Identity + status + kind.
    Default,
    /// Identity + status only.
    StatusOnly,
    /// Identity + status + `purpose:`/`note:` summary.
    Context,
}

pub(crate) fn cmd_batch_get(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    snippets: SnippetIndex<'_>,
    handles: &[String],
    mode: BatchGetMode,
) -> BatchGetOutput {
    let rows: Vec<BatchGetRow> = handles
        .iter()
        .map(|requested| match lookup_handle(node_index, requested) {
            Some(node_id) => {
                let handle = graph.node(node_id);
                let summary = (mode == BatchGetMode::Context)
                    .then(|| snippets.summary_for(handle).map(str::to_string))
                    .flatten();
                let (kind, file) = match mode {
                    BatchGetMode::StatusOnly => (None, None),
                    _ => (
                        Some(handle.kind.as_str().to_string()),
                        handle.file_path.as_ref().map(ToString::to_string),
                    ),
                };
                BatchGetRow {
                    handle: handle.id.clone(),
                    kind,
                    status: handle.status.clone(),
                    file,
                    summary,
                    not_found: false,
                }
            }
            None => BatchGetRow {
                handle: requested.clone(),
                kind: None,
                status: None,
                file: None,
                summary: None,
                not_found: true,
            },
        })
        .collect();

    BatchGetOutput {
        meta: OutputMeta::full(),
        rows,
    }
}

impl BatchGetOutput {
    pub(crate) fn print_human(
        &self,
        w: &mut dyn Write,
        style: OutputStyle,
        mode: BatchGetMode,
    ) -> std::io::Result<()> {
        let mut p = Printer::new(w, style);
        let handle_width = self.rows.iter().map(|r| r.handle.len()).max().unwrap_or(0);
        for row in &self.rows {
            if row.not_found {
                let pad = handle_width.saturating_sub(row.handle.len()) + 2;
                p.line(
                    &Line::new()
                        .path(row.handle.clone())
                        .pad(pad)
                        .toned(Tone::Error, "(not found)"),
                )?;
                continue;
            }
            let pad = handle_width.saturating_sub(row.handle.len()) + 2;
            let status = row.status.as_deref().unwrap_or("—");
            let mut line = Line::new()
                .path(row.handle.clone())
                .pad(pad)
                .toned(Tone::Default, format!("{status:<10}"))
                .text("  ");
            line = match mode {
                BatchGetMode::StatusOnly => line,
                BatchGetMode::Context => line.dim(row.summary.clone().unwrap_or_default()),
                BatchGetMode::Default => line.dim(row.kind.clone().unwrap_or_else(|| "?".into())),
            };
            p.line(&line)?;
        }
        Ok(())
    }

    pub(crate) fn has_missing(&self) -> bool {
        self.rows.iter().any(|r| r.not_found)
    }
}

// ---------------------------------------------------------------------------
// Get command (CLI-02)
// ---------------------------------------------------------------------------

/// Direction of an edge relative to the focus handle. Serializes as
/// `"incoming"` / `"outgoing"` to preserve the JSON wire format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EdgeDirection {
    Incoming,
    Outgoing,
}

/// Summary of a single edge for display.
#[derive(Clone, Serialize)]
pub(crate) struct EdgeSummary {
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) direction: EdgeDirection,
}

pub(crate) struct GetData {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
    pub(crate) outgoing_edges: Vec<EdgeSummary>,
    pub(crate) incoming_edges: Vec<EdgeSummary>,
    pub(crate) snippet: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct EdgeCounts {
    pub(crate) incoming: usize,
    pub(crate) outgoing: usize,
}

#[derive(Serialize)]
pub(crate) struct GetJsonOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) id: String,
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snippet: Option<String>,
    pub(crate) edge_counts: EdgeCounts,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) sample_incoming: Vec<EdgeSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) sample_outgoing: Vec<EdgeSummary>,
    pub(crate) truncated_edges: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) incoming_edges: Option<Vec<EdgeSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) outgoing_edges: Option<Vec<EdgeSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) briefing: Option<String>,
}

pub(crate) struct GetHumanOutput {
    pub(crate) data: GetData,
    pub(crate) limit_edges: usize,
    pub(crate) context: bool,
}

impl GetHumanOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write, style: OutputStyle) -> std::io::Result<()> {
        let mut p = Printer::new(w, style);
        if self.context {
            render_get_context(&mut p, &self.data, self.limit_edges)
        } else {
            render_get_summary(&mut p, &self.data, self.limit_edges)
        }
    }
}

impl GetJsonOutput {
    fn summary(data: &GetData, limit_edges: usize) -> Self {
        let sample_incoming: Vec<EdgeSummary> = data
            .incoming_edges
            .iter()
            .take(limit_edges)
            .cloned()
            .collect();
        let sample_outgoing: Vec<EdgeSummary> = data
            .outgoing_edges
            .iter()
            .take(limit_edges)
            .cloned()
            .collect();
        let truncated_edges =
            data.incoming_edges.len() > limit_edges || data.outgoing_edges.len() > limit_edges;

        Self {
            meta: OutputMeta::new(
                DetailLevel::Summary,
                truncated_edges,
                Some(sample_incoming.len() + sample_outgoing.len()),
                Some(data.incoming_edges.len() + data.outgoing_edges.len()),
                vec![
                    format!("--limit-edges {limit_edges}"),
                    "--refs".to_string(),
                    "--trace".to_string(),
                    "--full".to_string(),
                ],
            ),
            id: data.id.clone(),
            kind: data.kind.clone(),
            status: data.status.clone(),
            file: data.file.clone(),
            snippet: data.snippet.clone(),
            edge_counts: EdgeCounts {
                incoming: data.incoming_edges.len(),
                outgoing: data.outgoing_edges.len(),
            },
            sample_incoming,
            sample_outgoing,
            truncated_edges,
            incoming_edges: None,
            outgoing_edges: None,
            briefing: None,
        }
    }

    fn refs(data: &GetData, limit_edges: usize, full: bool) -> Self {
        let incoming = if full {
            data.incoming_edges.clone()
        } else {
            data.incoming_edges
                .iter()
                .take(limit_edges)
                .cloned()
                .collect()
        };
        let outgoing = if full {
            data.outgoing_edges.clone()
        } else {
            data.outgoing_edges
                .iter()
                .take(limit_edges)
                .cloned()
                .collect()
        };
        let truncated_edges = !full
            && (incoming.len() < data.incoming_edges.len()
                || outgoing.len() < data.outgoing_edges.len());

        Self {
            meta: OutputMeta::new(
                if full {
                    DetailLevel::Full
                } else {
                    DetailLevel::Sample
                },
                truncated_edges,
                Some(incoming.len() + outgoing.len()),
                Some(data.incoming_edges.len() + data.outgoing_edges.len()),
                if full {
                    Vec::new()
                } else {
                    vec![
                        format!("--limit-edges {}", limit_edges * 2),
                        "--full".to_string(),
                    ]
                },
            ),
            id: data.id.clone(),
            kind: data.kind.clone(),
            status: data.status.clone(),
            file: data.file.clone(),
            snippet: data.snippet.clone(),
            edge_counts: EdgeCounts {
                incoming: data.incoming_edges.len(),
                outgoing: data.outgoing_edges.len(),
            },
            sample_incoming: Vec::new(),
            sample_outgoing: Vec::new(),
            truncated_edges,
            incoming_edges: Some(incoming),
            outgoing_edges: Some(outgoing),
            briefing: None,
        }
    }

    fn context(data: &GetData, limit_edges: usize) -> Self {
        let mut output = Self::summary(data, limit_edges);
        output.meta.detail = DetailLevel::Summary;
        output.briefing = Some(build_get_briefing(data, limit_edges));
        output.sample_incoming.clear();
        output.sample_outgoing.clear();
        output
    }
}

/// Compact identity + KV + edge sample view. Used by the default
/// single-handle `anneal get` path.
fn render_get_summary<W: Write>(
    p: &mut Printer<W>,
    data: &GetData,
    limit_edges: usize,
) -> std::io::Result<()> {
    render_get_header(p, data)?;
    render_get_kv(p, data)?;

    if !data.outgoing_edges.is_empty() {
        p.blank()?;
        let shown = data.outgoing_edges.len().min(limit_edges);
        p.heading("Outgoing", Some(data.outgoing_edges.len()))?;
        render_edge_group(p, &data.outgoing_edges, shown, EdgeDirection::Outgoing)?;
    }
    if !data.incoming_edges.is_empty() {
        p.blank()?;
        let shown = data.incoming_edges.len().min(limit_edges);
        p.heading("Incoming", Some(data.incoming_edges.len()))?;
        render_edge_group(p, &data.incoming_edges, shown, EdgeDirection::Incoming)?;
    }
    Ok(())
}

/// Briefing-oriented expansion — `anneal get --context`.
fn render_get_context<W: Write>(
    p: &mut Printer<W>,
    data: &GetData,
    limit_edges: usize,
) -> std::io::Result<()> {
    render_get_header(p, data)?;
    render_get_kv(p, data)?;

    if let Some(snippet) = &data.snippet {
        p.blank()?;
        p.heading("Context", None)?;
        p.line_at(4, &Line::new().dim(snippet.clone()))?;
    }

    p.blank()?;
    p.heading("Refs", None)?;
    if data.outgoing_edges.is_empty() {
        p.line_at(4, &Line::new().dim("Outgoing: none"))?;
    } else {
        let shown = data.outgoing_edges.len().min(limit_edges);
        p.line_at(
            4,
            &Line::new()
                .heading("Outgoing")
                .text(" ")
                .dim(format!("({shown} of {})", data.outgoing_edges.len())),
        )?;
        render_edge_group_at(p, 6, &data.outgoing_edges, shown, EdgeDirection::Outgoing)?;
    }
    if data.incoming_edges.is_empty() {
        p.line_at(4, &Line::new().dim("Incoming: none"))?;
    } else {
        let shown = data.incoming_edges.len().min(limit_edges);
        p.line_at(
            4,
            &Line::new()
                .heading("Incoming")
                .text(" ")
                .dim(format!("({shown} of {})", data.incoming_edges.len())),
        )?;
        render_edge_group_at(p, 6, &data.incoming_edges, shown, EdgeDirection::Incoming)?;
    }

    p.blank()?;
    let mut hints = vec![
        (
            format!("anneal get {} --refs", data.id),
            "bounded adjacency",
        ),
        (format!("anneal get {} --trace", data.id), "full adjacency"),
    ];
    if data.outgoing_edges.len() > limit_edges || data.incoming_edges.len() > limit_edges {
        hints.push((format!("anneal get {} --full", data.id), "expand"));
    }
    let hint_rows: Vec<(&str, &str)> = hints.iter().map(|(c, d)| (c.as_str(), *d)).collect();
    p.hints(&hint_rows)?;
    Ok(())
}

fn render_get_header<W: Write>(p: &mut Printer<W>, data: &GetData) -> std::io::Result<()> {
    p.line(
        &Line::new()
            .heading(data.id.clone())
            .text("  ")
            .dim(format!("({})", data.kind)),
    )
}

fn render_get_kv<W: Write>(p: &mut Printer<W>, data: &GetData) -> std::io::Result<()> {
    let mut rows: Vec<(&str, Line)> = Vec::new();
    if let Some(status) = &data.status {
        rows.push(("Status", Line::new().text(status.clone())));
    }
    if let Some(file) = &data.file {
        rows.push(("File", Line::new().path(file.clone())));
    }
    if let Some(snippet) = &data.snippet {
        rows.push(("Snippet", Line::new().dim(snippet.clone())));
    }
    if rows.is_empty() {
        return Ok(());
    }
    p.kv_block(&rows)
}

fn render_edge_group<W: Write>(
    p: &mut Printer<W>,
    edges: &[EdgeSummary],
    shown: usize,
    direction: EdgeDirection,
) -> std::io::Result<()> {
    render_edge_group_at(p, 4, edges, shown, direction)
}

fn render_edge_group_at<W: Write>(
    p: &mut Printer<W>,
    col: usize,
    edges: &[EdgeSummary],
    shown: usize,
    direction: EdgeDirection,
) -> std::io::Result<()> {
    let style = p.style();
    let glyph = match direction {
        EdgeDirection::Incoming => style.glyph(Glyph::ArrowIn),
        EdgeDirection::Outgoing => style.glyph(Glyph::Arrow),
    };
    let kind_width = edges.iter().map(|e| e.kind.len()).max().unwrap_or(0);
    for edge in edges.iter().take(shown) {
        let kind_pad = kind_width.saturating_sub(edge.kind.len());
        p.line_at(
            col,
            &Line::new()
                .toned(Tone::Info, edge.kind.clone())
                .pad(kind_pad)
                .dim(format!(" {glyph} "))
                .path(edge.target.clone()),
        )?;
    }
    if edges.len() > shown {
        let remaining = edges.len() - shown;
        p.line_at(col, &Line::new().dim(format!("… {remaining} more")))?;
    }
    Ok(())
}

fn build_get_briefing(data: &GetData, limit_edges: usize) -> String {
    let mut parts = vec![format!("{} is a {}", data.id, data.kind)];
    if let Some(status) = &data.status {
        parts.push(format!("status {status}"));
    }
    if let Some(file) = &data.file {
        parts.push(format!("defined in {file}"));
    }
    if let Some(snippet) = &data.snippet {
        parts.push(format!("snippet: {snippet}"));
    }

    let incoming = data
        .incoming_edges
        .iter()
        .take(limit_edges)
        .map(|edge| format!("{} from {}", edge.kind, edge.target))
        .collect::<Vec<_>>();
    let outgoing = data
        .outgoing_edges
        .iter()
        .take(limit_edges)
        .map(|edge| format!("{} to {}", edge.kind, edge.target))
        .collect::<Vec<_>>();

    if !outgoing.is_empty() {
        parts.push(format!(
            "outgoing refs (showing {} of {}): {}",
            outgoing.len(),
            data.outgoing_edges.len(),
            outgoing.join(", ")
        ));
    }
    if !incoming.is_empty() {
        parts.push(format!(
            "incoming refs (showing {} of {}): {}",
            incoming.len(),
            data.incoming_edges.len(),
            incoming.join(", ")
        ));
    }

    parts.join(". ")
}

/// Resolve a handle by identity string and build output.
///
/// Looks up the handle by exact match first, then tries case-insensitive
/// match against label identities.
pub(crate) fn cmd_get(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    snippets: SnippetIndex<'_>,
    handle: &str,
) -> Option<GetData> {
    let node_id = lookup_handle(node_index, handle)?;

    let h = graph.node(node_id);
    let file = h.file_path.as_ref().map(ToString::to_string);

    let outgoing_edges = dedup_edges(
        graph.outgoing(node_id),
        |e| e.target,
        EdgeDirection::Outgoing,
        graph,
    );
    let incoming_edges = dedup_edges(
        graph.incoming(node_id),
        |e| e.source,
        EdgeDirection::Incoming,
        graph,
    );
    let snippet = snippets.summary_for(h).map(str::to_string);

    Some(GetData {
        id: h.id.clone(),
        kind: h.kind.as_str().to_string(),
        status: h.status.clone(),
        file,
        outgoing_edges,
        incoming_edges,
        snippet,
    })
}

pub(crate) enum GetJsonMode {
    Summary,
    Refs,
    Trace,
    Context,
}

pub(crate) struct GetJsonOptions {
    pub(crate) mode: GetJsonMode,
    pub(crate) limit_edges: usize,
}

pub(crate) fn build_get_json_output(data: &GetData, options: &GetJsonOptions) -> GetJsonOutput {
    match options.mode {
        GetJsonMode::Context => GetJsonOutput::context(data, options.limit_edges),
        GetJsonMode::Trace => GetJsonOutput::refs(data, options.limit_edges, true),
        GetJsonMode::Refs => GetJsonOutput::refs(data, options.limit_edges, false),
        GetJsonMode::Summary => GetJsonOutput::summary(data, options.limit_edges),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::cli::test_helpers::*;
    use crate::graph::EdgeKind;
    use crate::handle::Handle;

    use super::*;

    #[test]
    fn lookup_handle_normalizes_zero_padded_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(Handle::test_label("OQ", 1, None));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "OQ-01"),
            Some(label)
        );
    }

    #[test]
    fn lookup_handle_normalizes_zero_padded_compound_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(Handle::test_label("KB-D", 1, None));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "KB-D01"),
            Some(label)
        );
    }

    #[test]
    fn lookup_handle_accepts_hyphenated_zero_padded_compound_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(Handle::test_label("KB-D", 1, None));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "KB-D-01"),
            Some(label)
        );
    }

    #[test]
    fn cmd_get_uses_precomputed_snippets() {
        let mut graph = crate::graph::DiGraph::new();
        let file_node = graph.add_node(Handle::test_file("guide.md", Some("draft")));
        let label_node = graph.add_node(Handle::test_label("OQ", 64, None));

        let node_index = HashMap::from([
            ("guide.md".to_string(), file_node),
            ("OQ-64".to_string(), label_node),
        ]);
        let file_snippets = HashMap::from([(
            "guide.md".to_string(),
            "First paragraph line. Still same paragraph.".to_string(),
        )]);
        let label_snippets =
            HashMap::from([("OQ-64".to_string(), "Details: See OQ-64 here.".to_string())]);

        let snippets = SnippetIndex {
            files: &file_snippets,
            labels: &label_snippets,
        };

        let file_output = cmd_get(&graph, &node_index, snippets, "guide.md").expect("file output");
        assert_eq!(
            file_output.snippet.as_deref(),
            Some("First paragraph line. Still same paragraph.")
        );

        let label_output = cmd_get(&graph, &node_index, snippets, "OQ-64").expect("label output");
        assert_eq!(
            label_output.snippet.as_deref(),
            Some("Details: See OQ-64 here.")
        );
    }

    #[test]
    fn get_json_summary_caps_edges() {
        let mut graph = crate::graph::DiGraph::new();
        let center = graph.add_node(Handle::test_file("center.md", None));
        for idx in 0..15 {
            let source = graph.add_node(Handle::test_file(&format!("source-{idx}.md"), None));
            let target = graph.add_node(Handle::test_file(&format!("target-{idx}.md"), None));
            graph.add_edge(source, center, EdgeKind::Cites);
            graph.add_edge(center, target, EdgeKind::DependsOn);
        }

        let node_index = test_node_index(&graph);
        let empty_files = HashMap::new();
        let empty_labels = HashMap::new();
        let data = cmd_get(
            &graph,
            &node_index,
            SnippetIndex {
                files: &empty_files,
                labels: &empty_labels,
            },
            "center.md",
        )
        .expect("get output");
        let json = build_get_json_output(
            &data,
            &GetJsonOptions {
                mode: GetJsonMode::Summary,
                limit_edges: 10,
            },
        );

        assert!(json.truncated_edges);
        assert_eq!(json.edge_counts.incoming, 15);
        assert_eq!(json.edge_counts.outgoing, 15);
        assert_eq!(json.sample_incoming.len(), 10);
        assert_eq!(json.sample_outgoing.len(), 10);
        assert!(json.incoming_edges.is_none());
    }

    #[test]
    fn get_context_human_is_multiline_and_scannable() {
        let output = GetHumanOutput {
            data: GetData {
                id: "LABELS.md".into(),
                kind: "file".into(),
                status: Some("living".into()),
                file: Some("LABELS.md".into()),
                outgoing_edges: vec![
                    EdgeSummary {
                        kind: "Cites".into(),
                        target: "OQ-1".into(),
                        direction: EdgeDirection::Outgoing,
                    },
                    EdgeSummary {
                        kind: "Cites".into(),
                        target: "OQ-2".into(),
                        direction: EdgeDirection::Outgoing,
                    },
                ],
                incoming_edges: vec![EdgeSummary {
                    kind: "DependsOn".into(),
                    target: "synthesis.md".into(),
                    direction: EdgeDirection::Incoming,
                }],
                snippet: Some("Label index for the corpus.".into()),
            },
            limit_edges: 1,
            context: true,
        };

        let mut buf = Vec::new();
        output
            .print_human(&mut buf, plain_style())
            .expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");

        assert!(text.contains("Label index for the corpus."));
        assert!(text.contains("Refs"));
        assert!(text.contains("Outgoing"));
        assert!(text.contains("Cites"));
        assert!(text.contains("OQ-1"));
        assert!(text.contains("Incoming"));
        assert!(text.contains("anneal get LABELS.md --refs"));
        assert!(
            !text.contains("LABELS.md is a file."),
            "context output should no longer be a single stitched sentence: {text}"
        );
    }

    // ---------------------------------------------------------------
    // cmd_batch_get tests
    // ---------------------------------------------------------------

    #[test]
    fn batch_get_returns_row_per_handle_including_missing() {
        let mut graph = crate::graph::DiGraph::new();
        graph.add_node(Handle::test_file("a.md", Some("draft")));
        graph.add_node(Handle::test_file("b.md", Some("active")));

        let node_index = test_node_index(&graph);
        let empty = HashMap::new();
        let snippets = SnippetIndex {
            files: &empty,
            labels: &empty,
        };
        let handles = vec![
            "a.md".to_string(),
            "b.md".to_string(),
            "missing.md".to_string(),
        ];
        let output = cmd_batch_get(
            &graph,
            &node_index,
            snippets,
            &handles,
            BatchGetMode::Default,
        );

        assert_eq!(output.rows.len(), 3);
        assert_eq!(output.rows[0].handle, "a.md");
        assert_eq!(output.rows[0].status.as_deref(), Some("draft"));
        assert!(!output.rows[0].not_found);
        assert_eq!(output.rows[2].handle, "missing.md");
        assert!(output.rows[2].not_found);
        assert!(output.has_missing());
    }

    #[test]
    fn batch_get_status_only_drops_kind_and_file() {
        let mut graph = crate::graph::DiGraph::new();
        graph.add_node(Handle::test_file("a.md", Some("draft")));

        let node_index = test_node_index(&graph);
        let empty = HashMap::new();
        let snippets = SnippetIndex {
            files: &empty,
            labels: &empty,
        };
        let output = cmd_batch_get(
            &graph,
            &node_index,
            snippets,
            std::slice::from_ref(&"a.md".to_string()),
            BatchGetMode::StatusOnly,
        );

        assert_eq!(output.rows[0].status.as_deref(), Some("draft"));
        assert!(output.rows[0].kind.is_none());
        assert!(output.rows[0].file.is_none());
    }

    #[test]
    fn batch_get_human_aligns_columns() {
        let mut graph = crate::graph::DiGraph::new();
        graph.add_node(Handle::test_file("short.md", Some("draft")));
        graph.add_node(Handle::test_file("longer-name.md", Some("active")));

        let node_index = test_node_index(&graph);
        let empty = HashMap::new();
        let snippets = SnippetIndex {
            files: &empty,
            labels: &empty,
        };
        let handles = vec!["short.md".to_string(), "longer-name.md".to_string()];
        let mode = BatchGetMode::StatusOnly;
        let output = cmd_batch_get(&graph, &node_index, snippets, &handles, mode);
        let mut buf = Vec::new();
        output
            .print_human(&mut buf, plain_style(), mode)
            .expect("print");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("short.md       "));
        assert!(text.contains("longer-name.md"));
    }

    fn plain_style() -> crate::output::OutputStyle {
        crate::output::OutputStyle::plain()
    }
}
