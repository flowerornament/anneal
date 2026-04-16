use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::graph::{DiGraph, EdgeKind};
use crate::handle::NodeId;

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
