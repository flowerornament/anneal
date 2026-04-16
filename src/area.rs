use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::checks::{Diagnostic, DiagnosticCode, Severity};
use crate::config::AreasConfig;
use crate::graph::DiGraph;
use crate::handle::HandleKind;
use crate::lattice::Lattice;

// ---------------------------------------------------------------------------
// Area health model
// ---------------------------------------------------------------------------

/// Health grade for an area, from best to worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) enum AreaGrade {
    A,
    B,
    C,
    D,
}

impl AreaGrade {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        }
    }
}

/// Health profile for a single area (directory-level grouping).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct AreaHealth {
    pub(crate) name: String,
    pub(crate) files: usize,
    pub(crate) handles: usize,
    pub(crate) labels: usize,
    pub(crate) sections: usize,
    pub(crate) active: usize,
    pub(crate) terminal: usize,
    /// Average edges per handle.
    pub(crate) connectivity: f64,
    /// Edges reaching other areas.
    pub(crate) cross_links: usize,
    /// Namespaces present in this area.
    pub(crate) namespaces: Vec<String>,
    /// Diagnostic counts by severity.
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) suggestions: usize,
    /// S001 orphaned label count.
    pub(crate) orphans: usize,
    /// Composite grade.
    pub(crate) grade: AreaGrade,
    /// Human-readable signal summary.
    pub(crate) signal: String,
}

// ---------------------------------------------------------------------------
// Area computation
// ---------------------------------------------------------------------------

/// Extract the area name from a file path (first path component, or "(root)").
pub(crate) fn area_of(file: &str) -> &str {
    if let Some(pos) = file.find('/') {
        &file[..pos]
    } else {
        "(root)"
    }
}

/// Compute area health profiles from the graph, lattice, and diagnostics.
pub(crate) fn compute_areas(
    graph: &DiGraph,
    lattice: &Lattice,
    diagnostics: &[Diagnostic],
    config: &AreasConfig,
) -> Vec<AreaHealth> {
    let mut stats: HashMap<String, AreaStats> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        let file_str = match &handle.kind {
            HandleKind::File(path) => path.as_str(),
            _ => match &handle.file_path {
                Some(fp) => fp.as_str(),
                None => continue,
            },
        };

        let area_name = area_of(file_str);
        let s = stats.entry(area_name.to_string()).or_default();

        s.handles += 1;
        if matches!(handle.kind, HandleKind::File(_)) {
            s.files.insert(file_str.to_string());
        }
        if matches!(handle.kind, HandleKind::Label { .. }) {
            s.labels += 1;
        }
        if matches!(handle.kind, HandleKind::Section { .. }) {
            s.sections += 1;
        }

        if handle.is_terminal(lattice) {
            s.terminal += 1;
        } else if handle.status.is_some() {
            s.active += 1;
        }

        if let HandleKind::Label { ref prefix, .. } = handle.kind {
            s.namespaces.insert(prefix.clone());
        }

        s.total_edges += graph.incoming(node_id).len();

        // Count outgoing edges and cross-area links in a single pass
        for edge in graph.outgoing(node_id) {
            s.total_edges += 1;
            let target = graph.node(edge.target);
            let target_file = target
                .file_path
                .as_deref()
                .map_or("", camino::Utf8Path::as_str);
            if area_of(target_file) != area_name {
                s.cross_links += 1;
            }
        }
    }

    for diag in diagnostics {
        let area_name = diag.file.as_deref().map_or("(root)", area_of).to_string();
        let s = stats.entry(area_name).or_default();

        match diag.severity {
            Severity::Error => s.errors += 1,
            Severity::Warning => s.warnings += 1,
            Severity::Suggestion => {
                s.suggestions += 1;
                if diag.code == DiagnosticCode::S001 {
                    s.orphans += 1;
                }
            }
            Severity::Info => {}
        }
    }

    let mut areas: Vec<AreaHealth> = stats
        .into_iter()
        .map(|(name, s)| {
            let file_count = s.files.len();
            #[allow(clippy::cast_precision_loss)]
            let connectivity = if s.handles > 0 {
                s.total_edges as f64 / s.handles as f64
            } else {
                0.0
            };

            let (grade, signal) = compute_grade(&s, file_count, connectivity, config);

            let mut namespaces: Vec<String> = s.namespaces.into_iter().collect();
            namespaces.sort();

            AreaHealth {
                name,
                files: file_count,
                handles: s.handles,
                labels: s.labels,
                sections: s.sections,
                active: s.active,
                terminal: s.terminal,
                connectivity,
                cross_links: s.cross_links,
                namespaces,
                errors: s.errors,
                warnings: s.warnings,
                suggestions: s.suggestions,
                orphans: s.orphans,
                grade,
                signal,
            }
        })
        .collect();

    areas.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    areas
}

// ---------------------------------------------------------------------------
// Internal accumulator
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AreaStats {
    files: HashSet<String>,
    handles: usize,
    labels: usize,
    sections: usize,
    active: usize,
    terminal: usize,
    namespaces: HashSet<String>,
    total_edges: usize,
    cross_links: usize,
    errors: usize,
    warnings: usize,
    suggestions: usize,
    orphans: usize,
}

// Internal grading constants — not user-configurable because the values
// are ratios (edges/handle) with no intuitive meaning to config authors.
const SPARSE_CONNECTIVITY: f64 = 0.2;
const DECAY_CONNECTIVITY: f64 = 0.3;
const MIN_FILES_FOR_SIGNALS: usize = 3;

/// Compute grade and signal text from area stats.
fn compute_grade(
    s: &AreaStats,
    file_count: usize,
    connectivity: f64,
    config: &AreasConfig,
) -> (AreaGrade, String) {
    let mut signals = Vec::new();
    let large_enough = file_count > MIN_FILES_FOR_SIGNALS;

    if s.errors > 0 {
        signals.push(format!("{} broken", s.errors));
    }
    if s.cross_links == 0 && large_enough {
        signals.push("island".to_string());
    } else if connectivity < SPARSE_CONNECTIVITY && large_enough {
        signals.push("sparse".to_string());
    }
    if s.active == 0 && large_enough {
        signals.push("no active files".to_string());
    }
    if s.orphans > 0 {
        signals.push(format!("{} orphans", s.orphans));
    }

    let has_errors = s.errors > 0;
    let is_island = s.cross_links == 0 && large_enough;
    let is_sparse = connectivity < SPARSE_CONNECTIVITY && large_enough;
    let no_active = s.active == 0 && large_enough;
    let high_orphans = s.orphans >= config.orphan_threshold;
    let decaying = large_enough && connectivity < DECAY_CONNECTIVITY;

    let grade = if has_errors && (is_island || decaying) {
        AreaGrade::D
    } else if has_errors {
        AreaGrade::C
    } else if is_island || is_sparse || no_active || high_orphans {
        AreaGrade::B
    } else {
        AreaGrade::A
    };

    if signals.is_empty() {
        signals.push("healthy".to_string());
    }

    (grade, signals.join(", "))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{DiGraph, EdgeKind};
    use crate::handle::Handle;
    use crate::lattice::Lattice;

    #[test]
    fn area_of_extracts_directory() {
        assert_eq!(area_of("compiler/foo.md"), "compiler");
        assert_eq!(area_of("compiler/sub/bar.md"), "compiler");
        assert_eq!(area_of("README.md"), "(root)");
    }

    #[test]
    fn compute_areas_groups_by_directory() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        graph.add_node(Handle::test_file("compiler/b.md", Some("active")));
        graph.add_node(Handle::test_file("synthesis/c.md", Some("draft")));
        graph.add_node(Handle::test_file("README.md", None));

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &[], &AreasConfig::default());

        assert_eq!(areas.len(), 3);
        let compiler = areas.iter().find(|a| a.name == "compiler").unwrap();
        assert_eq!(compiler.files, 2);
        assert_eq!(compiler.active, 2);
        let synthesis = areas.iter().find(|a| a.name == "synthesis").unwrap();
        assert_eq!(synthesis.files, 1);
        let root = areas.iter().find(|a| a.name == "(root)").unwrap();
        assert_eq!(root.files, 1);
    }

    #[test]
    fn active_terminal_counted_correctly() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("docs/a.md", Some("draft")));
        graph.add_node(Handle::test_file("docs/b.md", Some("archived")));
        graph.add_node(Handle::test_file("docs/c.md", None));

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &[], &AreasConfig::default());
        let docs = areas.iter().find(|a| a.name == "docs").unwrap();
        assert_eq!(docs.active, 1, "only draft is active");
        assert_eq!(docs.terminal, 1, "archived is terminal");
    }

    #[test]
    fn grade_a_for_healthy_area() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("docs/a.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("docs/b.md", Some("active")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &[], &AreasConfig::default());
        let docs = areas.iter().find(|a| a.name == "docs").unwrap();
        assert_eq!(docs.grade, AreaGrade::A);
        assert_eq!(docs.signal, "healthy");
    }

    #[test]
    fn grade_c_for_area_with_errors() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("impl/a.md", Some("draft")));

        let diags = vec![Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::E001,
            message: "broken ref".to_string(),
            file: Some("impl/a.md".to_string()),
            line: None,
            evidence: None,
        }];

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &diags, &AreasConfig::default());
        let imp = areas.iter().find(|a| a.name == "impl").unwrap();
        assert_eq!(imp.grade, AreaGrade::C);
        assert!(imp.signal.contains("broken"));
    }

    #[test]
    fn cross_links_counted_correctly() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("synthesis/b.md", Some("draft")));
        graph.add_edge(a, b, EdgeKind::Cites);

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &[], &AreasConfig::default());
        let compiler = areas.iter().find(|a| a.name == "compiler").unwrap();
        assert_eq!(
            compiler.cross_links, 1,
            "compiler should have 1 cross-link to synthesis"
        );
        let synthesis = areas.iter().find(|a| a.name == "synthesis").unwrap();
        assert_eq!(
            synthesis.cross_links, 0,
            "synthesis has no outgoing cross-links"
        );
    }

    #[test]
    fn orphan_count_from_diagnostics() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));

        let diags = vec![
            Diagnostic {
                severity: Severity::Suggestion,
                code: DiagnosticCode::S001,
                message: "orphaned".to_string(),
                file: Some("compiler/a.md".to_string()),
                line: None,
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Suggestion,
                code: DiagnosticCode::S001,
                message: "orphaned".to_string(),
                file: Some("compiler/a.md".to_string()),
                line: None,
                evidence: None,
            },
        ];

        let lattice = Lattice::test_new(&["draft", "active"], &["archived"]);
        let areas = compute_areas(&graph, &lattice, &diags, &AreasConfig::default());
        let compiler = areas.iter().find(|a| a.name == "compiler").unwrap();
        assert_eq!(compiler.orphans, 2);
    }
}
