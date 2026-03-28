use serde::Serialize;

use crate::handle::{Handle, NodeId};

/// The five kinds of directed edge per KB-D5.
///
/// Edge kind determines what consistency checks apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EdgeKind {
    /// Source mentions target (informational; no consistency check).
    Cites,
    /// Source builds on target (consistency check: source state <= target state).
    DependsOn,
    /// Source replaces target (target becomes terminal).
    Supersedes,
    /// Source proves or checks target.
    Verifies,
    /// Source consumes target (for linear handles).
    Discharges,
}

/// A typed directed edge in the knowledge graph.
#[derive(Clone, Debug, Serialize)]
pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: EdgeKind,
}

/// Hand-rolled directed graph with dual adjacency lists per spec section 15.3.
///
/// Arena-indexed: `NodeId(u32)` indices into `nodes`. Forward and reverse
/// adjacency lists provide O(1) traversal in both directions. Ephemeral --
/// recomputed from files on every invocation (KB-P1, GRAPH-06).
pub struct DiGraph {
    nodes: Vec<Handle>,
    /// `fwd[src]` = outgoing edges from `src`.
    fwd: Vec<Vec<Edge>>,
    /// `rev[dst]` = incoming edges to `dst`.
    rev: Vec<Vec<Edge>>,
}

impl DiGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            fwd: Vec::new(),
            rev: Vec::new(),
        }
    }

    /// Add a handle to the graph and return its arena index.
    pub fn add_node(&mut self, handle: Handle) -> NodeId {
        let id =
            NodeId::new(u32::try_from(self.nodes.len()).expect("graph exceeds u32::MAX nodes"));
        self.nodes.push(handle);
        self.fwd.push(Vec::new());
        self.rev.push(Vec::new());
        id
    }

    /// Add a typed directed edge between two nodes.
    ///
    /// Appends the edge to both `fwd[source]` and `rev[target]` for
    /// O(1) forward and reverse traversal.
    pub fn add_edge(&mut self, source: NodeId, target: NodeId, kind: EdgeKind) {
        let edge = Edge {
            source,
            target,
            kind,
        };
        self.fwd[source.index()].push(edge.clone());
        self.rev[target.index()].push(edge);
    }

    /// Look up a node by its arena index.
    pub fn node(&self, id: NodeId) -> &Handle {
        &self.nodes[id.index()]
    }

    /// Mutably look up a node by its arena index.
    pub fn node_mut(&mut self, id: NodeId) -> &mut Handle {
        &mut self.nodes[id.index()]
    }

    /// Total number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Total number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.fwd.iter().map(Vec::len).sum()
    }

    /// Outgoing edges from a node (forward adjacency).
    pub fn outgoing(&self, id: NodeId) -> &[Edge] {
        &self.fwd[id.index()]
    }

    /// Incoming edges to a node (reverse adjacency).
    pub fn incoming(&self, id: NodeId) -> &[Edge] {
        &self.rev[id.index()]
    }

    /// Outgoing edges from a node filtered by kind.
    ///
    /// Typed traversal as first-class API rather than post-hoc filtering
    /// (per spec section 15.3).
    pub fn edges_by_kind(&self, id: NodeId, kind: EdgeKind) -> Vec<&Edge> {
        self.fwd[id.index()]
            .iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    /// Iterate over all nodes with their IDs.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Handle)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, h)| (NodeId::new(u32::try_from(i).expect("index fits u32")), h))
    }
}

impl Default for DiGraph {
    fn default() -> Self {
        Self::new()
    }
}
