use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::graph::{DiGraph, EdgeKind};
use crate::handle::NodeId;
use crate::output::{Line, OutputStyle, Printer};

use super::lookup_handle;

// ---------------------------------------------------------------------------
// Impact command (CLI-07)
// ---------------------------------------------------------------------------

/// Output of `anneal impact <handle>`: affected handles.
#[derive(Serialize)]
pub(crate) struct ImpactOutput {
    pub(crate) handle: String,
    pub(crate) direct_count: usize,
    pub(crate) indirect_count: usize,
    pub(crate) direct: Vec<String>,
    pub(crate) indirect: Vec<String>,
}

impl ImpactOutput {
    /// Filter direct/indirect results to handles in the given area.
    pub(crate) fn retain_area(
        &mut self,
        af: &crate::area::AreaFilter,
        node_index: &HashMap<String, NodeId>,
        graph: &DiGraph,
    ) {
        let keep = |id: &String| {
            node_index
                .get(id)
                .is_some_and(|&nid| af.matches_handle(graph.node(nid)))
        };
        self.direct.retain(keep);
        self.indirect.retain(keep);
        self.direct_count = self.direct.len();
        self.indirect_count = self.indirect.len();
    }

    /// Filter direct/indirect results to handles in the temporal window.
    pub(crate) fn retain_temporal(
        &mut self,
        tf: &crate::area::TemporalFilter,
        node_index: &HashMap<String, NodeId>,
        graph: &DiGraph,
    ) {
        let keep = |id: &String| {
            node_index
                .get(id)
                .is_some_and(|&nid| tf.matches_handle(graph.node(nid)))
        };
        self.direct.retain(keep);
        self.indirect.retain(keep);
        self.direct_count = self.direct.len();
        self.indirect_count = self.indirect.len();
    }

    pub(crate) fn print_human(&self, w: &mut dyn Write, style: OutputStyle) -> std::io::Result<()> {
        let mut p = Printer::new(w, style);
        self.render(&mut p)
    }

    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.heading("Impact", None)?;
        p.caption(&format!("what depends on {}", self.handle))?;
        p.blank()?;

        render_section(p, "Direct", &self.direct)?;
        p.blank()?;
        render_section(p, "Indirect", &self.indirect)?;
        Ok(())
    }
}

fn render_section<W: Write>(
    p: &mut Printer<W>,
    title: &str,
    items: &[String],
) -> std::io::Result<()> {
    p.heading(title, Some(items.len()))?;
    if items.is_empty() {
        p.line_at(4, &Line::new().dim("(none)"))?;
    } else {
        for id in items {
            p.bullet(&Line::new().path(id.clone()))?;
        }
    }
    Ok(())
}

/// Compute impact analysis for a handle.
pub(crate) fn cmd_impact(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    handle: &str,
    traverse_set: &[EdgeKind],
) -> Option<ImpactOutput> {
    let node_id = lookup_handle(node_index, handle)?;

    let result = crate::impact::compute_impact(graph, node_id, traverse_set);

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
        direct_count: direct.len(),
        indirect_count: indirect.len(),
        direct,
        indirect,
    })
}
