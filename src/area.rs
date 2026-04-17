use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use serde::Serialize;

use crate::checks::{Diagnostic, DiagnosticCode, Severity};
use crate::config::AreasConfig;
use crate::graph::DiGraph;
use crate::handle::{Handle, HandleKind};
use crate::lattice::Lattice;

// ---------------------------------------------------------------------------
// Area health model
// ---------------------------------------------------------------------------

/// Health grade for an area, from best to worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
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

    /// Whether this grade indicates an area that needs attention.
    pub(crate) fn is_degraded(self) -> bool {
        matches!(self, Self::C | Self::D)
    }
}

impl std::fmt::Display for AreaGrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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

/// Extract the area name for a handle. Files use their path; other kinds
/// inherit via `file_path`. Returns `None` for handles without a source file.
pub(crate) fn area_of_handle(handle: &Handle) -> Option<&str> {
    match &handle.kind {
        HandleKind::File(path) => Some(area_of(path.as_str())),
        _ => handle.file_path.as_deref().map(|p| area_of(p.as_str())),
    }
}

/// Extract the area name for a diagnostic, treating unspecified locations as
/// corpus-root (`"(root)"`). Mirrors the convention used by `compute_areas`.
pub(crate) fn area_of_diagnostic(diag: &Diagnostic) -> &str {
    diag.file.as_deref().map_or("(root)", area_of)
}

/// A filter that scopes commands to a single area (directory or concern group).
pub(crate) struct AreaFilter {
    name: String,
}

impl AreaFilter {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    /// Whether a handle belongs to this area by file path.
    pub(crate) fn matches_handle(&self, handle: &Handle) -> bool {
        handle
            .file_path
            .as_deref()
            .is_some_and(|fp| area_of(fp.as_str()) == self.name)
    }

    /// Whether a file path belongs to this area.
    pub(crate) fn matches_file(&self, path: &str) -> bool {
        area_of(path) == self.name
    }

    #[allow(dead_code)] // used by orient/garden commands (planned)
    pub(crate) fn name(&self) -> &str {
        &self.name
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
        let Some(area_name) = area_of_handle(handle) else {
            continue;
        };
        let s = stats.entry(area_name.to_string()).or_default();

        s.handles += 1;
        if let HandleKind::File(path) = &handle.kind {
            s.files.insert(path.as_str().to_string());
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
            if area_of_handle(target).is_some_and(|t| t != area_name) {
                s.cross_links += 1;
            }
        }
    }

    for diag in diagnostics {
        let area_name = area_of_diagnostic(diag).to_string();
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
// Temporal filter
// ---------------------------------------------------------------------------

/// A filter that scopes commands to handles within a time window.
///
/// Built once from the graph after parsing, then passed to each command.
/// Non-file handles inherit their parent file's date via the pre-built
/// `file_set` of qualifying file paths, so no per-handle graph lookup is
/// needed at filter time.
pub(crate) struct TemporalFilter {
    file_set: HashSet<String>,
}

impl TemporalFilter {
    /// Build the filter by scanning file handles whose date falls within
    /// the window (>= `cutoff`).
    pub(crate) fn new(cutoff: NaiveDate, graph: &DiGraph) -> Self {
        let file_set = graph
            .nodes()
            .filter(|(_, h)| {
                matches!(h.kind, HandleKind::File(_)) && h.date.is_some_and(|d| d >= cutoff)
            })
            .filter_map(|(_, h)| h.file_path.as_ref().map(ToString::to_string))
            .collect();
        Self { file_set }
    }

    /// Whether a handle's parent file is within the temporal window.
    pub(crate) fn matches_handle(&self, handle: &Handle) -> bool {
        handle
            .file_path
            .as_ref()
            .is_some_and(|fp| self.file_set.contains(fp.as_str()))
    }

    /// Whether a file path is within the temporal window.
    pub(crate) fn matches_file(&self, path: &str) -> bool {
        self.file_set.contains(path)
    }
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
    fn area_filter_matches_handle_in_area() {
        let filter = AreaFilter::new("compiler");
        assert!(filter.matches_handle(&Handle::test_file("compiler/a.md", None)));
        assert!(!filter.matches_handle(&Handle::test_file("synthesis/b.md", None)));
    }

    #[test]
    fn area_filter_matches_root_area() {
        let filter = AreaFilter::new("(root)");
        assert!(filter.matches_handle(&Handle::test_file("README.md", None)));
        assert!(!filter.matches_handle(&Handle::test_file("compiler/a.md", None)));
    }

    #[test]
    fn area_filter_matches_file_path() {
        let filter = AreaFilter::new("compiler");
        assert!(filter.matches_file("compiler/a.md"));
        assert!(filter.matches_file("compiler/sub/b.md"));
        assert!(!filter.matches_file("synthesis/c.md"));
        assert!(!filter.matches_file("README.md"));
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

    // -----------------------------------------------------------------------
    // TemporalFilter tests
    // -----------------------------------------------------------------------

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn temporal_filter_matches_recent_file() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file_with_date(
            "docs/a.md",
            Some("draft"),
            date(2026, 4, 10),
        ));

        let tf = TemporalFilter::new(date(2026, 4, 5), &graph);
        assert!(tf.matches_file("docs/a.md"));
    }

    #[test]
    fn temporal_filter_excludes_old_file() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file_with_date(
            "docs/old.md",
            Some("draft"),
            date(2026, 1, 1),
        ));

        let tf = TemporalFilter::new(date(2026, 4, 5), &graph);
        assert!(!tf.matches_file("docs/old.md"));
    }

    #[test]
    fn temporal_filter_excludes_undated_file() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("docs/no-date.md", Some("draft")));

        let tf = TemporalFilter::new(date(2026, 4, 5), &graph);
        assert!(!tf.matches_file("docs/no-date.md"));
    }

    #[test]
    fn temporal_filter_section_inherits_file_date() {
        let mut graph = DiGraph::new();
        let file_id = graph.add_node(Handle::test_file_with_date(
            "docs/a.md",
            Some("draft"),
            date(2026, 4, 10),
        ));
        graph.add_node(Handle::section(
            file_id,
            "Introduction".to_string(),
            camino::Utf8PathBuf::from("docs/a.md"),
        ));

        let tf = TemporalFilter::new(date(2026, 4, 5), &graph);
        // The section handle has file_path "docs/a.md" which is in the set.
        let section = graph
            .nodes()
            .find(|(_, h)| matches!(h.kind, HandleKind::Section { .. }))
            .unwrap()
            .1;
        assert!(tf.matches_handle(section));
    }
}
