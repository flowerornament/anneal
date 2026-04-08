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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::Handle;

    // -------------------------------------------------------------------
    // add_node
    // -------------------------------------------------------------------

    #[test]
    fn add_node_returns_sequential_ids() {
        let mut g = DiGraph::new();
        let n0 = g.add_node(Handle::test_file("a.md", None));
        let n1 = g.add_node(Handle::test_file("b.md", None));
        let n2 = g.add_node(Handle::test_file("c.md", None));

        assert_eq!(n0.index(), 0);
        assert_eq!(n1.index(), 1);
        assert_eq!(n2.index(), 2);
        assert_eq!(g.node_count(), 3);
    }

    // -------------------------------------------------------------------
    // add_edge — dual adjacency
    // -------------------------------------------------------------------

    #[test]
    fn add_edge_creates_forward_and_reverse_adjacency() {
        let mut g = DiGraph::new();
        let a = g.add_node(Handle::test_file("a.md", None));
        let b = g.add_node(Handle::test_file("b.md", None));

        g.add_edge(a, b, EdgeKind::DependsOn);

        assert_eq!(g.outgoing(a).len(), 1);
        assert_eq!(g.outgoing(a)[0].target, b);
        assert_eq!(g.outgoing(a)[0].kind, EdgeKind::DependsOn);

        assert_eq!(g.incoming(b).len(), 1);
        assert_eq!(g.incoming(b)[0].source, a);
        assert_eq!(g.incoming(b)[0].kind, EdgeKind::DependsOn);

        // No reverse on source, no forward on target.
        assert!(g.incoming(a).is_empty());
        assert!(g.outgoing(b).is_empty());
    }

    #[test]
    fn edge_count_reflects_all_edges() {
        let mut g = DiGraph::new();
        let a = g.add_node(Handle::test_file("a.md", None));
        let b = g.add_node(Handle::test_file("b.md", None));
        let c = g.add_node(Handle::test_file("c.md", None));

        g.add_edge(a, b, EdgeKind::Cites);
        g.add_edge(a, c, EdgeKind::DependsOn);
        g.add_edge(b, c, EdgeKind::Supersedes);

        assert_eq!(g.edge_count(), 3);
    }

    // -------------------------------------------------------------------
    // edges_by_kind
    // -------------------------------------------------------------------

    #[test]
    fn edges_by_kind_filters_correctly() {
        let mut g = DiGraph::new();
        let a = g.add_node(Handle::test_file("a.md", None));
        let b = g.add_node(Handle::test_file("b.md", None));
        let c = g.add_node(Handle::test_file("c.md", None));

        g.add_edge(a, b, EdgeKind::Cites);
        g.add_edge(a, c, EdgeKind::DependsOn);
        g.add_edge(a, b, EdgeKind::DependsOn);

        let depends: Vec<_> = g.edges_by_kind(a, EdgeKind::DependsOn).collect();
        assert_eq!(depends.len(), 2);

        let cites: Vec<_> = g.edges_by_kind(a, EdgeKind::Cites).collect();
        assert_eq!(cites.len(), 1);
        assert_eq!(cites[0].target, b);

        let verifies: Vec<_> = g.edges_by_kind(a, EdgeKind::Verifies).collect();
        assert!(verifies.is_empty());
    }

    // -------------------------------------------------------------------
    // EdgeKind::from_name
    // -------------------------------------------------------------------

    #[test]
    fn edge_kind_from_name_round_trip_well_known() {
        let cases = [
            ("Cites", EdgeKind::Cites),
            ("DependsOn", EdgeKind::DependsOn),
            ("Supersedes", EdgeKind::Supersedes),
            ("Verifies", EdgeKind::Verifies),
            ("Discharges", EdgeKind::Discharges),
        ];
        for (name, expected) in &cases {
            let parsed = EdgeKind::from_name(name);
            assert_eq!(&parsed, expected, "from_name({name}) mismatch");
            assert_eq!(parsed.as_str(), *name, "round-trip as_str({name}) mismatch");
        }
    }

    #[test]
    fn edge_kind_from_name_case_insensitive() {
        assert_eq!(EdgeKind::from_name("cites"), EdgeKind::Cites);
        assert_eq!(EdgeKind::from_name("CITES"), EdgeKind::Cites);
        assert_eq!(EdgeKind::from_name("dependson"), EdgeKind::DependsOn);
        assert_eq!(EdgeKind::from_name("DEPENDSON"), EdgeKind::DependsOn);
        assert_eq!(EdgeKind::from_name("depends_on"), EdgeKind::DependsOn);
        assert_eq!(EdgeKind::from_name("DEPENDS_ON"), EdgeKind::DependsOn);
        assert_eq!(EdgeKind::from_name("supersedes"), EdgeKind::Supersedes);
        assert_eq!(EdgeKind::from_name("verifies"), EdgeKind::Verifies);
        assert_eq!(EdgeKind::from_name("discharges"), EdgeKind::Discharges);
    }

    #[test]
    fn edge_kind_from_name_custom() {
        let kind = EdgeKind::from_name("Implements");
        assert_eq!(kind, EdgeKind::Custom("Implements".to_string()));
        assert_eq!(kind.as_str(), "Implements");
    }

    #[test]
    fn edge_kind_from_name_custom_preserves_case() {
        let kind = EdgeKind::from_name("MyCustomEdge");
        assert_eq!(kind, EdgeKind::Custom("MyCustomEdge".to_string()));
        assert_eq!(kind.as_str(), "MyCustomEdge");
    }
}
