use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::checks::{Diagnostic, DiagnosticCode, Severity};
use crate::graph::DiGraph;
use crate::handle::HandleKind;

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

/// Compute area health profiles from the graph and diagnostics.
pub(crate) fn compute_areas(graph: &DiGraph, diagnostics: &[Diagnostic]) -> Vec<AreaHealth> {
    // Accumulate per-area stats
    let mut stats: HashMap<String, AreaStats> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        let file = match &handle.kind {
            HandleKind::File(path) => path.as_str().to_string(),
            _ => {
                // Non-file handles: use file_path if available
                if let Some(fp) = &handle.file_path {
                    fp.as_str().to_string()
                } else {
                    continue;
                }
            }
        };

        let area_name = area_of(&file).to_string();
        let s = stats.entry(area_name).or_default();

        s.handles += 1;
        if matches!(handle.kind, HandleKind::File(_)) {
            s.files.insert(file.clone());
        }
        if matches!(handle.kind, HandleKind::Label { .. }) {
            s.labels += 1;
        }
        if matches!(handle.kind, HandleKind::Section { .. }) {
            s.sections += 1;
        }

        if handle.status.is_some() {
            s.has_status += 1;
        }

        if let Some(ns) = match &handle.kind {
            HandleKind::Label { prefix, .. } => Some(prefix.as_str()),
            _ => None,
        } {
            s.namespaces.insert(ns.to_string());
        }

        // Edge counts
        let incoming = graph.incoming(node_id).len();
        let outgoing = graph.outgoing(node_id).len();
        s.total_edges += incoming + outgoing;

        // Cross-area edges (outgoing only to avoid double-counting)
        for edge in graph.outgoing(node_id) {
            let target = graph.node(edge.target);
            let target_file = target
                .file_path
                .as_deref()
                .map_or("", camino::Utf8Path::as_str);
            let target_area = area_of(target_file);
            if target_area != area_of(&file) {
                s.cross_links += 1;
            }
        }
    }

    // Accumulate diagnostics per area
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

    // Compute grades and build output
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

            let (grade, signal) = compute_grade(&s, file_count, connectivity);

            let mut namespaces: Vec<String> = s.namespaces.into_iter().collect();
            namespaces.sort();

            AreaHealth {
                name,
                files: file_count,
                handles: s.handles,
                labels: s.labels,
                sections: s.sections,
                active: s.has_status, // approximation until lattice is available
                terminal: 0,          // requires lattice integration
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

    // Sort by file count descending (most content first)
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
    has_status: usize,
    namespaces: HashSet<String>,
    total_edges: usize,
    cross_links: usize,
    errors: usize,
    warnings: usize,
    suggestions: usize,
    orphans: usize,
}

/// Compute grade and signal text from area stats.
fn compute_grade(s: &AreaStats, file_count: usize, connectivity: f64) -> (AreaGrade, String) {
    let mut signals = Vec::new();
    let mut grade = AreaGrade::A;

    // Errors are the strongest signal
    if s.errors > 0 {
        grade = AreaGrade::C;
        signals.push(format!("{} broken", s.errors));

        // Errors + low connectivity = structural decay
        if connectivity < 0.3 && file_count > 3 {
            grade = AreaGrade::D;
        }
    }

    // Low connectivity
    if s.cross_links == 0 && file_count > 3 {
        if grade > AreaGrade::B {
            grade = AreaGrade::B;
        }
        signals.push("island".to_string());
    } else if connectivity < 0.2 && file_count > 3 {
        if grade > AreaGrade::B {
            grade = AreaGrade::B;
        }
        signals.push("sparse".to_string());
    }

    // No active metadata
    if s.has_status == 0 && file_count > 3 {
        if grade > AreaGrade::B {
            grade = AreaGrade::B;
        }
        signals.push("no active files".to_string());
    }

    // Orphaned labels
    if s.orphans > 0 {
        signals.push(format!("{} orphans", s.orphans));
    }

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
    use crate::handle::{Handle, HandleKind, HandleMetadata};
    use camino::Utf8PathBuf;

    fn make_file(graph: &mut DiGraph, path: &str, status: Option<&str>) -> crate::handle::NodeId {
        graph.add_node(Handle {
            id: path.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(path)),
            status: status.map(String::from),
            file_path: Some(Utf8PathBuf::from(path)),
            date: None,
            metadata: HandleMetadata::default(),
        })
    }

    #[test]
    fn area_of_extracts_directory() {
        assert_eq!(area_of("compiler/foo.md"), "compiler");
        assert_eq!(area_of("compiler/sub/bar.md"), "compiler");
        assert_eq!(area_of("README.md"), "(root)");
    }

    #[test]
    fn compute_areas_groups_by_directory() {
        let mut graph = DiGraph::new();
        make_file(&mut graph, "compiler/a.md", Some("draft"));
        make_file(&mut graph, "compiler/b.md", Some("active"));
        make_file(&mut graph, "synthesis/c.md", Some("draft"));
        make_file(&mut graph, "README.md", None);

        let areas = compute_areas(&graph, &[]);

        assert_eq!(areas.len(), 3);
        let compiler = areas.iter().find(|a| a.name == "compiler").unwrap();
        assert_eq!(compiler.files, 2);
        let synthesis = areas.iter().find(|a| a.name == "synthesis").unwrap();
        assert_eq!(synthesis.files, 1);
        let root = areas.iter().find(|a| a.name == "(root)").unwrap();
        assert_eq!(root.files, 1);
    }

    #[test]
    fn grade_a_for_healthy_area() {
        let mut graph = DiGraph::new();
        let a = make_file(&mut graph, "docs/a.md", Some("draft"));
        let b = make_file(&mut graph, "docs/b.md", Some("active"));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let areas = compute_areas(&graph, &[]);
        let docs = areas.iter().find(|a| a.name == "docs").unwrap();
        assert_eq!(docs.grade, AreaGrade::A);
        assert_eq!(docs.signal, "healthy");
    }

    #[test]
    fn grade_c_for_area_with_errors() {
        let mut graph = DiGraph::new();
        make_file(&mut graph, "impl/a.md", Some("draft"));

        let diags = vec![Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::E001,
            message: "broken ref".to_string(),
            file: Some("impl/a.md".to_string()),
            line: None,
            evidence: None,
        }];

        let areas = compute_areas(&graph, &diags);
        let imp = areas.iter().find(|a| a.name == "impl").unwrap();
        assert_eq!(imp.grade, AreaGrade::C);
        assert!(imp.signal.contains("broken"));
    }

    #[test]
    fn cross_links_counted_correctly() {
        let mut graph = DiGraph::new();
        let a = make_file(&mut graph, "compiler/a.md", Some("draft"));
        let b = make_file(&mut graph, "synthesis/b.md", Some("draft"));
        graph.add_edge(a, b, EdgeKind::Cites);

        let areas = compute_areas(&graph, &[]);
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
        make_file(&mut graph, "compiler/a.md", Some("draft"));

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

        let areas = compute_areas(&graph, &diags);
        let compiler = areas.iter().find(|a| a.name == "compiler").unwrap();
        assert_eq!(compiler.orphans, 2);
    }
}
