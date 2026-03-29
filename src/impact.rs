use std::collections::{HashSet, VecDeque};

use serde::Serialize;

use crate::graph::{DiGraph, EdgeKind};
use crate::handle::NodeId;

/// Result of impact analysis: handles affected if the starting handle changes.
#[derive(Debug, Serialize)]
pub(crate) struct ImpactResult {
    /// Handles directly depending on the target (depth 1).
    pub(crate) direct: Vec<NodeId>,
    /// Handles indirectly depending on the target (depth > 1).
    pub(crate) indirect: Vec<NodeId>,
}

/// Compute the impact set by reverse BFS from a starting handle (KB-D16).
///
/// Traverses reverse DependsOn, Supersedes, and Verifies edges. Uses a
/// visited set for cycle detection (IMPACT-02). Distinguishes direct
/// (depth=1) from indirect (depth>1) affected handles (IMPACT-03).
pub(crate) fn compute_impact(graph: &DiGraph, start: NodeId) -> ImpactResult {
    let mut visited = HashSet::new();
    let mut direct = Vec::new();
    let mut indirect = Vec::new();
    let mut queue = VecDeque::new();

    visited.insert(start);
    queue.push_back((start, 0u32));

    while let Some((current, depth)) = queue.pop_front() {
        for edge in graph.incoming(current) {
            if !matches!(
                edge.kind,
                EdgeKind::DependsOn | EdgeKind::Supersedes | EdgeKind::Verifies
            ) {
                continue;
            }
            if visited.insert(edge.source) {
                if depth == 0 {
                    direct.push(edge.source);
                } else {
                    indirect.push(edge.source);
                }
                queue.push_back((edge.source, depth + 1));
            }
        }
    }

    ImpactResult { direct, indirect }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DiGraph;
    use crate::handle::{Handle, HandleKind, HandleMetadata};
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

    #[test]
    fn simple_chain_direct_and_indirect() {
        // A -DependsOn-> B -DependsOn-> C
        // impact(C) = direct: [B], indirect: [A]
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        let c = graph.add_node(make_file_handle("c.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);

        let result = compute_impact(&graph, c);
        assert_eq!(result.direct, vec![b]);
        assert_eq!(result.indirect, vec![a]);
    }

    #[test]
    fn cycle_detection_terminates() {
        // A -DependsOn-> B -DependsOn-> A (cycle)
        // impact(A) should terminate with direct: [B]
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, a, EdgeKind::DependsOn);

        let result = compute_impact(&graph, a);
        assert_eq!(result.direct, vec![b]);
        assert!(result.indirect.is_empty());
    }

    #[test]
    fn cites_edges_not_traversed() {
        // A -Cites-> B
        // impact(B) should be empty (Cites is not traversed)
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(a, b, EdgeKind::Cites);

        let result = compute_impact(&graph, b);
        assert!(result.direct.is_empty());
        assert!(result.indirect.is_empty());
    }

    #[test]
    fn discharges_edges_not_traversed() {
        // A -Discharges-> B
        // impact(B) should be empty (Discharges is not traversed)
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(a, b, EdgeKind::Discharges);

        let result = compute_impact(&graph, b);
        assert!(result.direct.is_empty());
        assert!(result.indirect.is_empty());
    }

    #[test]
    fn no_incoming_edges_returns_empty() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));

        let result = compute_impact(&graph, a);
        assert!(result.direct.is_empty());
        assert!(result.indirect.is_empty());
    }

    #[test]
    fn empty_graph_node_returns_empty() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));

        let result = compute_impact(&graph, a);
        assert!(result.direct.is_empty());
        assert!(result.indirect.is_empty());
    }

    #[test]
    fn start_node_not_in_results() {
        // A -DependsOn-> B
        // impact(B) should not include B itself
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let result = compute_impact(&graph, b);
        assert!(!result.direct.contains(&b));
        assert!(!result.indirect.contains(&b));
    }

    #[test]
    fn supersedes_and_verifies_traversed() {
        // A -Supersedes-> C, B -Verifies-> C
        // impact(C) = direct: [A, B]
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        let c = graph.add_node(make_file_handle("c.md"));
        graph.add_edge(a, c, EdgeKind::Supersedes);
        graph.add_edge(b, c, EdgeKind::Verifies);

        let result = compute_impact(&graph, c);
        assert_eq!(result.direct.len(), 2);
        assert!(result.direct.contains(&a));
        assert!(result.direct.contains(&b));
        assert!(result.indirect.is_empty());
    }
}
