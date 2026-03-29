use std::collections::HashMap;

use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{HandleKind, NodeId};
use crate::lattice::{self, Lattice};
use crate::parse::PendingEdge;

/// Severity level for diagnostics, ordered so errors sort first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) enum Severity {
    Error = 0,
    Warning = 1,
    Info = 2,
}

/// A single diagnostic produced by a check rule (CHECK-06).
///
/// Each diagnostic has a severity, error code, human message, and optional
/// file location. Format matches spec section 12.1 compiler-style output.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

impl Diagnostic {
    /// Print in compiler-style format per spec section 12.1:
    /// ```text
    /// error[E001]: broken reference: OQ-99 not found
    ///   -> formal-model/v17.md
    /// ```
    #[allow(dead_code)] // Phase 2 Plan 03: CLI will call this
    pub(crate) fn print_human(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        };
        write!(w, "{prefix}[{}]: {}", self.code, self.message)?;
        if let Some(ref file) = self.file {
            write!(w, "\n  -> {file}")?;
            if let Some(line) = self.line {
                write!(w, ":{line}")?;
            }
        }
        writeln!(w)
    }
}

// ---------------------------------------------------------------------------
// CHECK-01: Existence (KB-R1)
// ---------------------------------------------------------------------------

/// Check existence: every edge target must resolve.
///
/// Per D-01: section references (target starting with "section:") get a single
/// I001 info summary, not per-reference errors. All other unresolved pending
/// edges produce E001 errors.
fn check_existence(
    graph: &DiGraph,
    unresolved_edges: &[PendingEdge],
    section_ref_count: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if section_ref_count > 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "I001",
            message: format!(
                "{section_ref_count} section references use section notation, \
                 not resolvable to heading slugs"
            ),
            file: None,
            line: None,
        });
    }

    for edge in unresolved_edges {
        if edge.target_identity.starts_with("section:") {
            continue;
        }
        let file = graph
            .node(edge.source)
            .file_path
            .as_ref()
            .map(ToString::to_string);
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: format!("broken reference: {} not found", edge.target_identity),
            file,
            line: None,
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-02: Staleness (KB-R2)
// ---------------------------------------------------------------------------

/// Check staleness: active handle referencing terminal handle.
///
/// For each outgoing edge, if source has an active status and target has a
/// terminal status, emit W001.
fn check_staleness(graph: &DiGraph, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (node_id, handle) in graph.nodes() {
        let Some(ref source_status) = handle.status else {
            continue;
        };
        if !lattice.active.contains(source_status) {
            continue;
        }

        for edge in graph.outgoing(node_id) {
            let target = graph.node(edge.target);
            let Some(ref target_status) = target.status else {
                continue;
            };
            if lattice.terminal.contains(target_status) {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "W001",
                    message: format!(
                        "stale reference: {} (active) references {} ({}, terminal)",
                        handle.id, target.id, target_status
                    ),
                    file: handle.file_path.as_ref().map(ToString::to_string),
                    line: None,
                });
            }
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-03: Confidence gap (KB-R3)
// ---------------------------------------------------------------------------

/// Check confidence gap: DependsOn edge where source state > target state.
///
/// Only applies when the lattice has a non-empty ordering and both handles
/// have statuses with known levels. Uses `state_level()` from lattice.rs.
fn check_confidence_gap(graph: &DiGraph, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if lattice.ordering.is_empty() {
        return diagnostics;
    }

    for (node_id, handle) in graph.nodes() {
        let Some(ref source_status) = handle.status else {
            continue;
        };
        let Some(source_level) = lattice::state_level(source_status, lattice) else {
            continue;
        };

        for edge in graph.edges_by_kind(node_id, EdgeKind::DependsOn) {
            let target = graph.node(edge.target);
            let Some(ref target_status) = target.status else {
                continue;
            };
            let Some(target_level) = lattice::state_level(target_status, lattice) else {
                continue;
            };

            if source_level > target_level {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "W002",
                    message: format!(
                        "confidence gap: {} ({}) depends on {} ({})",
                        handle.id, source_status, target.id, target_status
                    ),
                    file: handle.file_path.as_ref().map(ToString::to_string),
                    line: None,
                });
            }
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-04: Linearity (KB-R4)
// ---------------------------------------------------------------------------

/// Check linearity: linear handles must be discharged exactly once.
///
/// Builds a set of linear namespace prefixes from config. For each Label handle
/// in a linear namespace: count incoming Discharges edges. Skip if terminal
/// (mooted). Zero = E002. Multiple = I002.
fn check_linearity(graph: &DiGraph, config: &AnnealConfig, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let linear_namespaces: std::collections::HashSet<&str> =
        config.handles.linear.iter().map(String::as_str).collect();

    if linear_namespaces.is_empty() {
        return diagnostics;
    }

    for (node_id, handle) in graph.nodes() {
        let HandleKind::Label { ref prefix, .. } = handle.kind else {
            continue;
        };

        if !linear_namespaces.contains(prefix.as_str()) {
            continue;
        }

        // Mooted: terminal status means obligation is automatically discharged
        if let Some(ref status) = handle.status
            && lattice.terminal.contains(status)
        {
            continue;
        }

        let discharge_count = graph
            .incoming(node_id)
            .iter()
            .filter(|e| e.kind == EdgeKind::Discharges)
            .count();

        if discharge_count == 0 {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "E002",
                message: format!(
                    "undischarged obligation: {} has no Discharges edge",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
            });
        } else if discharge_count >= 2 {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "I002",
                message: format!(
                    "multiple discharges: {} discharged {discharge_count} times (affine)",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-05: Convention adoption (KB-R5)
// ---------------------------------------------------------------------------

/// Check convention adoption: warn about missing frontmatter when >50% of
/// siblings in the same directory have it.
///
/// Groups File handles by parent directory, computes adoption rate, and emits
/// W003 for files without frontmatter in high-adoption directories.
fn check_conventions(graph: &DiGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Group file handles by parent directory
    // Key: directory path, Value: (total_count, with_frontmatter_count, files_without_fm)
    let mut by_dir: HashMap<String, (usize, usize, Vec<NodeId>)> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        let HandleKind::File(ref path) = handle.kind else {
            continue;
        };

        let dir = path.parent().map_or_else(String::new, ToString::to_string);

        let entry = by_dir.entry(dir).or_insert((0, 0, Vec::new()));
        entry.0 += 1; // total
        if handle.status.is_some() {
            entry.1 += 1; // with frontmatter
        } else {
            entry.2.push(node_id); // missing frontmatter
        }
    }

    for (total, with_fm, missing_nodes) in by_dir.values() {
        if *total < 2 {
            continue;
        }

        let rate = lattice::frontmatter_adoption_rate(*total, *with_fm);
        if rate <= 0.5 {
            continue;
        }

        for &node_id in missing_nodes {
            let handle = graph.node(node_id);
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "W003",
                message: format!(
                    "missing frontmatter: {} has no status field ({with_fm}/{total} siblings have frontmatter)",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run all five check rules and return sorted diagnostics.
///
/// Diagnostics are sorted by severity: errors first, then warnings, then info.
#[allow(dead_code)] // Phase 2 Plan 03: CLI will call this
pub(crate) fn run_checks(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    unresolved_edges: &[PendingEdge],
    section_ref_count: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(check_existence(graph, unresolved_edges, section_ref_count));
    diagnostics.extend(check_staleness(graph, lattice));
    diagnostics.extend(check_confidence_gap(graph, lattice));
    diagnostics.extend(check_linearity(graph, config, lattice));
    diagnostics.extend(check_conventions(graph));
    diagnostics.sort_by_key(|d| d.severity);
    diagnostics
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnnealConfig, HandlesConfig};
    use crate::graph::DiGraph;
    use crate::handle::{Handle, HandleKind, HandleMetadata};
    use crate::lattice::Lattice;
    use crate::parse::PendingEdge;
    use camino::Utf8PathBuf;

    fn make_lattice(active: &[&str], terminal: &[&str], ordering: &[&str]) -> Lattice {
        Lattice {
            observed_statuses: active
                .iter()
                .chain(terminal.iter())
                .copied()
                .map(String::from)
                .collect(),
            active: active.iter().copied().map(String::from).collect(),
            terminal: terminal.iter().copied().map(String::from).collect(),
            ordering: ordering.iter().copied().map(String::from).collect(),
            kind: crate::lattice::LatticeKind::Confidence,
        }
    }

    fn make_file_handle(id: &str, status: Option<&str>) -> Handle {
        Handle {
            id: id.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(id)),
            status: status.map(String::from),
            file_path: Some(Utf8PathBuf::from(id)),
            metadata: HandleMetadata::default(),
        }
    }

    fn make_label_handle(prefix: &str, number: u32, status: Option<&str>) -> Handle {
        Handle {
            id: format!("{prefix}-{number}"),
            kind: HandleKind::Label {
                prefix: prefix.to_string(),
                number,
            },
            status: status.map(String::from),
            file_path: None,
            metadata: HandleMetadata::default(),
        }
    }

    // -----------------------------------------------------------------------
    // CHECK-01: Existence
    // -----------------------------------------------------------------------

    #[test]
    fn e001_for_unresolved_non_section_edge() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(make_file_handle("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "OQ-99".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
        }];

        let diags = check_existence(&graph, &unresolved, 0);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, "E001");
        assert!(diags[0].message.contains("OQ-99"));
    }

    #[test]
    fn i001_for_section_refs() {
        let graph = DiGraph::new();
        let unresolved: Vec<PendingEdge> = Vec::new();

        let diags = check_existence(&graph, &unresolved, 42);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, "I001");
        assert!(diags[0].message.contains("42"));
    }

    // -----------------------------------------------------------------------
    // CHECK-02: Staleness
    // -----------------------------------------------------------------------

    #[test]
    fn w001_active_references_terminal() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("active.md", Some("draft")));
        let b = graph.add_node(make_file_handle("terminal.md", Some("archived")));
        graph.add_edge(a, b, EdgeKind::Cites);

        let lattice = make_lattice(&["draft"], &["archived"], &[]);

        let diags = check_staleness(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W001");
        assert!(diags[0].message.contains("active.md"));
        assert!(diags[0].message.contains("terminal.md"));
    }

    // -----------------------------------------------------------------------
    // CHECK-03: Confidence gap
    // -----------------------------------------------------------------------

    #[test]
    fn w002_source_higher_than_target() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("formal.md", Some("formal")));
        let b = graph.add_node(make_file_handle("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // ordering: provisional(0) < draft(1) < formal(2)
        let lattice = make_lattice(
            &["provisional", "draft", "formal"],
            &[],
            &["provisional", "draft", "formal"],
        );

        let diags = check_confidence_gap(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W002");
        assert!(diags[0].message.contains("formal.md"));
        assert!(diags[0].message.contains("provisional.md"));
    }

    #[test]
    fn w002_not_produced_when_ordering_empty() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("formal.md", Some("formal")));
        let b = graph.add_node(make_file_handle("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // No ordering -- cannot determine levels
        let lattice = make_lattice(&["provisional", "formal"], &[], &[]);

        let diags = check_confidence_gap(&graph, &lattice);
        assert!(
            diags.is_empty(),
            "W002 should not be produced when lattice has no ordering"
        );
    }

    // -----------------------------------------------------------------------
    // CHECK-04: Linearity
    // -----------------------------------------------------------------------

    #[test]
    fn e002_undischarged_obligation() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(make_label_handle("OBL", 1, None));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, "E002");
        assert!(diags[0].message.contains("OBL-1"));
    }

    #[test]
    fn e002_not_produced_for_terminal_handle() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(make_label_handle("OBL", 1, Some("archived")));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &["archived"], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert!(
            diags.is_empty(),
            "E002 should not be produced for handles with terminal status (mooted)"
        );
    }

    #[test]
    fn i002_multiple_discharges() {
        let mut graph = DiGraph::new();
        let label = graph.add_node(make_label_handle("OBL", 1, None));
        let discharger1 = graph.add_node(make_file_handle("proof1.md", None));
        let discharger2 = graph.add_node(make_file_handle("proof2.md", None));
        graph.add_edge(discharger1, label, EdgeKind::Discharges);
        graph.add_edge(discharger2, label, EdgeKind::Discharges);

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, "I002");
        assert!(diags[0].message.contains("OBL-1"));
        assert!(diags[0].message.contains("2 times"));
    }

    // -----------------------------------------------------------------------
    // CHECK-05: Convention adoption
    // -----------------------------------------------------------------------

    #[test]
    fn w003_missing_frontmatter_above_threshold() {
        let mut graph = DiGraph::new();
        // 3 files in same dir: 2 have status, 1 does not -> 66% adoption
        let _a = graph.add_node(make_file_handle("dir/a.md", Some("draft")));
        let _b = graph.add_node(make_file_handle("dir/b.md", Some("final")));
        let _c = graph.add_node(make_file_handle("dir/c.md", None));

        let diags = check_conventions(&graph);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W003");
        assert!(diags[0].message.contains("dir/c.md"));
    }

    #[test]
    fn w003_not_produced_below_threshold() {
        let mut graph = DiGraph::new();
        // 3 files in same dir: 1 has status, 2 do not -> 33% adoption
        let _a = graph.add_node(make_file_handle("dir/a.md", Some("draft")));
        let _b = graph.add_node(make_file_handle("dir/b.md", None));
        let _c = graph.add_node(make_file_handle("dir/c.md", None));

        let diags = check_conventions(&graph);
        assert!(
            diags.is_empty(),
            "W003 should not be produced when adoption rate is <= 50%"
        );
    }

    // -----------------------------------------------------------------------
    // run_checks integration
    // -----------------------------------------------------------------------

    #[test]
    fn run_checks_sorts_by_severity() {
        let mut graph = DiGraph::new();
        // Create a scenario producing all three severities:
        // E001 from unresolved edge
        let source = graph.add_node(make_file_handle("doc.md", Some("draft")));
        // W001 from stale reference
        let terminal = graph.add_node(make_file_handle("old.md", Some("archived")));
        graph.add_edge(source, terminal, EdgeKind::Cites);

        let lattice = make_lattice(&["draft"], &["archived"], &[]);
        let config = AnnealConfig::default();

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "missing-ref".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
        }];

        let diags = run_checks(&graph, &lattice, &config, &unresolved, 5);

        // Should have: E001 (error), W001 (warning), I001 (info)
        assert!(
            diags.len() >= 3,
            "Expected at least 3 diagnostics, got {}",
            diags.len()
        );

        // Verify ordering: errors before warnings before info
        let mut last_severity = Severity::Error;
        for d in &diags {
            assert!(
                d.severity >= last_severity,
                "Diagnostics not sorted by severity: {:?} came after {:?}",
                d.severity,
                last_severity
            );
            last_severity = d.severity;
        }
    }
}
