use serde::Serialize;

use crate::handle::{Handle, NodeId};

/// The five kinds of directed edge per KB-D5.
///
/// Edge kind determines what consistency checks apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum EdgeKind {
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

impl EdgeKind {
    /// Parse an edge kind from its string name (case-insensitive match on variant names).
    pub(crate) fn from_name(s: &str) -> Option<Self> {
        match s {
            "Cites" | "cites" => Some(Self::Cites),
            "DependsOn" | "depends_on" => Some(Self::DependsOn),
            "Supersedes" | "supersedes" => Some(Self::Supersedes),
            "Verifies" | "verifies" => Some(Self::Verifies),
            "Discharges" | "discharges" => Some(Self::Discharges),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Edge {
    pub(crate) source: NodeId,
    pub(crate) target: NodeId,
    pub(crate) kind: EdgeKind,
}

/// Hand-rolled directed graph with dual adjacency lists per spec section 15.3.
///
/// Arena-indexed: `NodeId(u32)` indices into `nodes`. Forward and reverse
/// adjacency lists provide O(1) traversal in both directions. Ephemeral —
/// recomputed from files on every invocation (KB-P1, GRAPH-06).
pub(crate) struct DiGraph {
    nodes: Vec<Handle>,
    /// `fwd[src]` = outgoing edges from `src`.
    fwd: Vec<Vec<Edge>>,
    /// `rev[dst]` = incoming edges to `dst`.
    rev: Vec<Vec<Edge>>,
}

impl DiGraph {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            fwd: Vec::new(),
            rev: Vec::new(),
        }
    }

    pub(crate) fn add_node(&mut self, handle: Handle) -> NodeId {
        let id =
            NodeId::new(u32::try_from(self.nodes.len()).expect("graph exceeds u32::MAX nodes"));
        self.nodes.push(handle);
        self.fwd.push(Vec::new());
        self.rev.push(Vec::new());
        id
    }

    /// Appends the edge to both `fwd[source]` and `rev[target]` for
    /// O(1) forward and reverse traversal.
    pub(crate) fn add_edge(&mut self, source: NodeId, target: NodeId, kind: EdgeKind) {
        let edge = Edge {
            source,
            target,
            kind,
        };
        self.fwd[source.index()].push(edge);
        self.rev[target.index()].push(edge);
    }

    pub(crate) fn node(&self, id: NodeId) -> &Handle {
        &self.nodes[id.index()]
    }

    // Phase 2: used by check mutations and future handle updates
    #[allow(dead_code)]
    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Handle {
        &mut self.nodes[id.index()]
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn edge_count(&self) -> usize {
        self.fwd.iter().map(Vec::len).sum()
    }

    // Phase 2: CHECK rules, impact analysis
    #[allow(dead_code)]
    pub(crate) fn outgoing(&self, id: NodeId) -> &[Edge] {
        &self.fwd[id.index()]
    }

    // Phase 2: impact analysis (reverse traversal)
    #[allow(dead_code)]
    pub(crate) fn incoming(&self, id: NodeId) -> &[Edge] {
        &self.rev[id.index()]
    }

    /// Outgoing edges filtered by kind. Typed traversal as first-class API
    /// (per spec section 15.3).
    // Phase 2: CHECK-03 confidence gap
    #[allow(dead_code)]
    pub(crate) fn edges_by_kind(&self, id: NodeId, kind: EdgeKind) -> impl Iterator<Item = &Edge> {
        self.fwd[id.index()].iter().filter(move |e| e.kind == kind)
    }

    pub(crate) fn nodes(&self) -> impl Iterator<Item = (NodeId, &Handle)> {
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
