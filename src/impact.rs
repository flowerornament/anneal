use std::collections::{HashMap, HashSet, VecDeque};

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

#[derive(Debug, Serialize)]
pub(crate) struct ImpactPathResult {
    pub(crate) direct: Vec<ImpactPathEntry>,
    pub(crate) indirect: Vec<ImpactPathEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ImpactPathEntry {
    pub(crate) target: NodeId,
    pub(crate) path: Vec<ImpactPathHop>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactPathHop {
    pub(crate) source: NodeId,
    pub(crate) edge_kind: EdgeKind,
    pub(crate) target: NodeId,
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

/// Compute the same impact set as `compute_impact`, but retain one canonical
/// shortest explanation path from each affected handle back to `start`.
pub(crate) fn compute_impact_paths(graph: &DiGraph, start: NodeId) -> ImpactPathResult {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut depths: HashMap<NodeId, u32> = HashMap::new();
    let mut predecessor: HashMap<NodeId, ImpactPathHop> = HashMap::new();

    visited.insert(start);
    depths.insert(start, 0);
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        let mut incoming: Vec<ImpactPathHop> = graph
            .incoming(current)
            .iter()
            .filter(|edge| {
                matches!(
                    edge.kind,
                    EdgeKind::DependsOn | EdgeKind::Supersedes | EdgeKind::Verifies
                )
            })
            .map(|edge| ImpactPathHop {
                source: edge.source,
                edge_kind: edge.kind.clone(),
                target: current,
            })
            .collect();
        incoming.sort_by(|a, b| {
            graph
                .node(a.source)
                .id
                .cmp(&graph.node(b.source).id)
                .then_with(|| a.edge_kind.as_str().cmp(b.edge_kind.as_str()))
                .then_with(|| graph.node(a.target).id.cmp(&graph.node(b.target).id))
        });

        let depth = depths[&current];
        for hop in incoming {
            if visited.insert(hop.source) {
                depths.insert(hop.source, depth + 1);
                let source = hop.source;
                predecessor.insert(source, hop);
                queue.push_back(source);
            }
        }
    }

    let mut direct = Vec::new();
    let mut indirect = Vec::new();
    let mut nodes: Vec<(NodeId, u32)> = depths
        .into_iter()
        .filter(|(node_id, _)| *node_id != start)
        .collect();
    nodes.sort_by(|(left, left_depth), (right, right_depth)| {
        left_depth
            .cmp(right_depth)
            .then_with(|| graph.node(*left).id.cmp(&graph.node(*right).id))
    });

    for (node_id, depth) in nodes {
        let mut path = Vec::new();
        let mut current = node_id;
        while let Some(hop) = predecessor.get(&current).cloned() {
            current = hop.target;
            path.push(hop);
        }
        let entry = ImpactPathEntry {
            target: node_id,
            path,
        };
        if depth == 1 {
            direct.push(entry);
        } else {
            indirect.push(entry);
        }
    }

    ImpactPathResult { direct, indirect }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DiGraph;
    use crate::handle::Handle;

    fn make_file_handle(id: &str) -> Handle {
        Handle::test_file(id, None)
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

    #[test]
    fn impact_paths_capture_direct_and_indirect_chains() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("a.md"));
        let b = graph.add_node(make_file_handle("b.md"));
        let c = graph.add_node(make_file_handle("c.md"));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);

        let result = compute_impact_paths(&graph, c);
        assert_eq!(result.direct.len(), 1);
        assert_eq!(result.indirect.len(), 1);
        assert_eq!(result.direct[0].target, b);
        assert_eq!(result.direct[0].path.len(), 1);
        assert_eq!(result.direct[0].path[0].source, b);
        assert_eq!(result.direct[0].path[0].target, c);
        assert_eq!(result.indirect[0].target, a);
        assert_eq!(result.indirect[0].path.len(), 2);
        assert_eq!(result.indirect[0].path[0].source, a);
        assert_eq!(result.indirect[0].path[0].target, b);
        assert_eq!(result.indirect[0].path[1].source, b);
        assert_eq!(result.indirect[0].path[1].target, c);
    }
}
