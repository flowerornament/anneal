use std::fmt;

use serde::{Serialize, Serializer};

use crate::handle::{Handle, NodeId};

/// Directed edge kinds. The five well-known kinds carry built-in diagnostic
/// semantics; `Custom` accepts any user-defined string (indexed in the graph,
/// queryable, but no built-in checks).
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// User-defined edge kind with no built-in diagnostic behavior.
    Custom(String),
}

impl EdgeKind {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Cites => "Cites",
            Self::DependsOn => "DependsOn",
            Self::Supersedes => "Supersedes",
            Self::Verifies => "Verifies",
            Self::Discharges => "Discharges",
            Self::Custom(s) => s,
        }
    }

    /// Parse an edge kind from its string name. Well-known names resolve to
    /// their variant (case-insensitive); everything else becomes `Custom`.
    pub(crate) fn from_name(s: &str) -> Self {
        if s.eq_ignore_ascii_case("cites") {
            Self::Cites
        } else if s.eq_ignore_ascii_case("dependson") || s.eq_ignore_ascii_case("depends_on") {
            Self::DependsOn
        } else if s.eq_ignore_ascii_case("supersedes") {
            Self::Supersedes
        } else if s.eq_ignore_ascii_case("verifies") {
            Self::Verifies
        } else if s.eq_ignore_ascii_case("discharges") {
            Self::Discharges
        } else {
            Self::Custom(s.to_string())
        }
    }
}

impl Serialize for EdgeKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Serialize)]
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
        self.rev[target.index()].push(edge.clone());
        self.fwd[source.index()].push(edge);
    }

    pub(crate) fn node(&self, id: NodeId) -> &Handle {
        &self.nodes[id.index()]
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn edge_count(&self) -> usize {
        self.fwd.iter().map(Vec::len).sum()
    }

    pub(crate) fn outgoing(&self, id: NodeId) -> &[Edge] {
        &self.fwd[id.index()]
    }

    pub(crate) fn incoming(&self, id: NodeId) -> &[Edge] {
        &self.rev[id.index()]
    }

    /// Outgoing edges filtered by kind. Typed traversal as first-class API
    /// (per spec section 15.3).
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
