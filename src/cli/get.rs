use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::graph::DiGraph;
use crate::handle::{HandleKind, NodeId};

use super::{DetailLevel, OutputMeta, dedup_edges, lookup_handle};

// ---------------------------------------------------------------------------
// Get command (CLI-02)
// ---------------------------------------------------------------------------

/// Summary of a single edge for display.
#[derive(Clone, Serialize)]
pub(crate) struct EdgeSummary {
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) direction: String,
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
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        if self.context {
            return print_get_context_human(&self.data, self.limit_edges, w);
        }

        let output = GetJsonOutput::summary(&self.data, self.limit_edges);
        output.print_human(w)
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

    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "{} ({})", self.id, self.kind)?;
        if let Some(ref status) = self.status {
            writeln!(w, "  Status: {status}")?;
        }
        if let Some(ref file) = self.file {
            writeln!(w, "  File: {file}")?;
        }
        if let Some(ref snippet) = self.snippet {
            writeln!(w, "  Snippet: {snippet}")?;
        }
        let outgoing = self
            .outgoing_edges
            .as_ref()
            .unwrap_or(&self.sample_outgoing);
        if !outgoing.is_empty() {
            writeln!(w, "  Outgoing:")?;
            let total = self.edge_counts.outgoing;
            for edge in outgoing {
                writeln!(w, "    {} -> {}", edge.kind, edge.target)?;
            }
            if total > outgoing.len() {
                writeln!(
                    w,
                    "    ... and {} more outgoing edges ({total} unique)",
                    total - outgoing.len()
                )?;
            }
        }
        let incoming = self
            .incoming_edges
            .as_ref()
            .unwrap_or(&self.sample_incoming);
        if !incoming.is_empty() {
            writeln!(w, "  Incoming:")?;
            let total = self.edge_counts.incoming;
            for edge in incoming {
                writeln!(w, "    {} <- {}", edge.kind, edge.target)?;
            }
            if total > incoming.len() {
                writeln!(
                    w,
                    "    ... and {} more incoming edges ({total} unique)",
                    total - incoming.len()
                )?;
            }
        }
        Ok(())
    }
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

fn print_get_context_human(
    data: &GetData,
    limit_edges: usize,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(w, "{} ({})", data.id, data.kind)?;
    if let Some(status) = &data.status {
        writeln!(w, "  Status: {status}")?;
    }
    if let Some(file) = &data.file {
        writeln!(w, "  File: {file}")?;
    }

    if let Some(snippet) = &data.snippet {
        writeln!(w)?;
        writeln!(w, "Context:")?;
        writeln!(w, "  {snippet}")?;
    }

    writeln!(w)?;
    writeln!(w, "Refs:")?;

    if data.outgoing_edges.is_empty() {
        writeln!(w, "  Outgoing: none")?;
    } else {
        let shown = data.outgoing_edges.len().min(limit_edges);
        writeln!(
            w,
            "  Outgoing (showing {shown} of {}):",
            data.outgoing_edges.len()
        )?;
        for edge in data.outgoing_edges.iter().take(limit_edges) {
            writeln!(w, "    {} -> {}", edge.kind, edge.target)?;
        }
        if data.outgoing_edges.len() > limit_edges {
            writeln!(
                w,
                "    ... and {} more",
                data.outgoing_edges.len() - limit_edges
            )?;
        }
    }

    if data.incoming_edges.is_empty() {
        writeln!(w, "  Incoming: none")?;
    } else {
        let shown = data.incoming_edges.len().min(limit_edges);
        writeln!(
            w,
            "  Incoming (showing {shown} of {}):",
            data.incoming_edges.len()
        )?;
        for edge in data.incoming_edges.iter().take(limit_edges) {
            writeln!(w, "    {} <- {}", edge.kind, edge.target)?;
        }
        if data.incoming_edges.len() > limit_edges {
            writeln!(
                w,
                "    ... and {} more",
                data.incoming_edges.len() - limit_edges
            )?;
        }
    }

    let mut expand = vec![
        format!(
            "anneal get {} --refs --limit-edges {}",
            data.id,
            limit_edges * 2
        ),
        format!("anneal get {} --trace", data.id),
    ];
    if data.outgoing_edges.len() > limit_edges || data.incoming_edges.len() > limit_edges {
        expand.push(format!("anneal get {} --full", data.id));
    }
    writeln!(w)?;
    writeln!(w, "Expand with: {}", expand.join(", "))
}

/// Resolve a handle by identity string and build output.
///
/// Looks up the handle by exact match first, then tries case-insensitive
/// match against label identities.
pub(crate) fn cmd_get(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    file_snippets: &HashMap<String, String>,
    label_snippets: &HashMap<String, String>,
    handle: &str,
) -> Option<GetData> {
    let node_id = lookup_handle(node_index, handle)?;

    let h = graph.node(node_id);
    let file = h.file_path.as_ref().map(ToString::to_string);

    let outgoing_edges = dedup_edges(graph.outgoing(node_id), |e| e.target, "outgoing", graph);
    let incoming_edges = dedup_edges(graph.incoming(node_id), |e| e.source, "incoming", graph);
    let snippet = match &h.kind {
        HandleKind::File(path) => file_snippets.get(path.as_str()).cloned(),
        HandleKind::Label { .. } => label_snippets.get(&h.id).cloned(),
        _ => None,
    };

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
    use crate::handle::HandleKind;

    use super::*;

    #[test]
    fn lookup_handle_normalizes_zero_padded_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(make_label_handle("OQ", 1));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "OQ-01"),
            Some(label)
        );
    }

    #[test]
    fn lookup_handle_normalizes_zero_padded_compound_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(make_label_handle("KB-D", 1));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "KB-D01"),
            Some(label)
        );
    }

    #[test]
    fn lookup_handle_accepts_hyphenated_zero_padded_compound_labels() {
        let mut graph = crate::graph::DiGraph::new();
        let label = graph.add_node(make_label_handle("KB-D", 1));
        let node_index = test_node_index(&graph);

        assert_eq!(
            super::super::lookup_handle(&node_index, "KB-D-01"),
            Some(label)
        );
    }

    #[test]
    fn cmd_get_uses_precomputed_snippets() {
        let mut graph = crate::graph::DiGraph::new();
        let file_node = graph.add_node(crate::handle::Handle::test_file("guide.md", Some("draft")));
        let label_node = graph.add_node(crate::handle::Handle {
            id: "OQ-64".to_string(),
            kind: HandleKind::Label {
                prefix: "OQ".to_string(),
                number: 64,
            },
            status: None,
            file_path: Some("guide.md".into()),
            metadata: crate::handle::HandleMetadata::default(),
        });

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

        let file_output = cmd_get(
            &graph,
            &node_index,
            &file_snippets,
            &label_snippets,
            "guide.md",
        )
        .expect("file output");
        assert_eq!(
            file_output.snippet.as_deref(),
            Some("First paragraph line. Still same paragraph.")
        );

        let label_output = cmd_get(
            &graph,
            &node_index,
            &file_snippets,
            &label_snippets,
            "OQ-64",
        )
        .expect("label output");
        assert_eq!(
            label_output.snippet.as_deref(),
            Some("Details: See OQ-64 here.")
        );
    }

    #[test]
    fn get_json_summary_caps_edges() {
        let mut graph = crate::graph::DiGraph::new();
        let center = graph.add_node(make_file_handle("center.md"));
        for idx in 0..15 {
            let source = graph.add_node(make_file_handle(&format!("source-{idx}.md")));
            let target = graph.add_node(make_file_handle(&format!("target-{idx}.md")));
            graph.add_edge(source, center, EdgeKind::Cites);
            graph.add_edge(center, target, EdgeKind::DependsOn);
        }

        let node_index = test_node_index(&graph);
        let data = cmd_get(
            &graph,
            &node_index,
            &HashMap::new(),
            &HashMap::new(),
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
                        direction: "outgoing".into(),
                    },
                    EdgeSummary {
                        kind: "Cites".into(),
                        target: "OQ-2".into(),
                        direction: "outgoing".into(),
                    },
                ],
                incoming_edges: vec![EdgeSummary {
                    kind: "DependsOn".into(),
                    target: "synthesis.md".into(),
                    direction: "incoming".into(),
                }],
                snippet: Some("Label index for the corpus.".into()),
            },
            limit_edges: 1,
            context: true,
        };

        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let text = String::from_utf8(buf).expect("utf8");

        assert!(text.contains("Context:\n  Label index for the corpus."));
        assert!(text.contains("Refs:\n  Outgoing (showing 1 of 2):"));
        assert!(text.contains("    Cites -> OQ-1"));
        assert!(text.contains("  Incoming (showing 1 of 1):"));
        assert!(text.contains("Expand with: anneal get LABELS.md --refs --limit-edges 2"));
        assert!(
            !text.contains("LABELS.md is a file."),
            "context output should no longer be a single stitched sentence: {text}"
        );
    }
}
