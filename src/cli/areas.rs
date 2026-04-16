use std::io::Write;

use serde::Serialize;

use crate::area::{AreaGrade, AreaHealth, compute_areas};
use crate::checks::Diagnostic;
use crate::config::AreasConfig;
use crate::graph::DiGraph;
use crate::lattice::Lattice;
use crate::style::S;

/// Sort order for areas output.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub(crate) enum AreaSort {
    /// Sort by file count descending (default).
    #[default]
    Files,
    /// Sort by grade (worst first).
    Grade,
    /// Sort by connectivity descending.
    Conn,
    /// Sort alphabetically by name.
    Name,
}

/// Output of the `anneal areas` command.
#[derive(Serialize)]
pub(crate) struct AreasOutput {
    areas: Vec<AreaHealth>,
}

impl AreasOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(
            w,
            "{:<20} {:>5} {:>5} {:>6} {:>5}  Signal",
            "Area", "Files", "Conn", "Cross", "Grade"
        )?;
        writeln!(w, "{}", "─".repeat(72))?;

        for area in &self.areas {
            let grade_str = format_grade(area.grade);
            let conn_str = format!("{:.1}", area.connectivity);
            writeln!(
                w,
                "{:<20} {:>5} {:>5} {:>6} {:>5}  {}",
                format!("{}/", area.name),
                area.files,
                conn_str,
                area.cross_links,
                grade_str,
                area.signal,
            )?;
        }
        Ok(())
    }
}

fn format_grade(grade: AreaGrade) -> String {
    let s = format!("[{}]", grade.as_str());
    match grade {
        AreaGrade::A => S.green.apply_to(s).to_string(),
        AreaGrade::B => S.dim.apply_to(s).to_string(),
        AreaGrade::C => S.warning.apply_to(s).to_string(),
        AreaGrade::D => S.error.apply_to(s).to_string(),
    }
}

pub(crate) fn cmd_areas(
    graph: &DiGraph,
    lattice: &Lattice,
    diagnostics: &[Diagnostic],
    config: &AreasConfig,
    sort: AreaSort,
    include_terminal: bool,
) -> AreasOutput {
    let mut areas = compute_areas(graph, lattice, diagnostics, config);

    if !include_terminal {
        areas.retain(|a| a.active > 0 || a.errors > 0 || a.files > 0);
    }

    match sort {
        AreaSort::Files => {} // already sorted by files desc from compute_areas
        AreaSort::Grade => areas.sort_by(|a, b| b.grade.cmp(&a.grade).then(a.name.cmp(&b.name))),
        AreaSort::Conn => areas.sort_by(|a, b| {
            b.connectivity
                .partial_cmp(&a.connectivity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.name.cmp(&b.name))
        }),
        AreaSort::Name => areas.sort_by(|a, b| a.name.cmp(&b.name)),
    }

    AreasOutput { areas }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{DiGraph, EdgeKind};
    use crate::handle::Handle;
    use crate::lattice::Lattice;

    #[test]
    fn cmd_areas_produces_output_for_multi_area_graph() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("synthesis/b.md", Some("draft")));
        graph.add_node(Handle::test_file("README.md", None));
        graph.add_edge(a, b, EdgeKind::Cites);

        let lattice = Lattice::test_new(&["draft"], &["archived"]);
        let output = cmd_areas(
            &graph,
            &lattice,
            &[],
            &AreasConfig::default(),
            AreaSort::Files,
            false,
        );

        assert_eq!(output.areas.len(), 3);
        assert_eq!(output.areas[0].name, "(root)");
        assert_eq!(output.areas[1].name, "compiler");
        assert_eq!(output.areas[2].name, "synthesis");
    }

    #[test]
    fn cmd_areas_sort_by_name() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("z-area/a.md", Some("draft")));
        graph.add_node(Handle::test_file("a-area/b.md", Some("draft")));

        let lattice = Lattice::test_new(&["draft"], &[]);
        let output = cmd_areas(
            &graph,
            &lattice,
            &[],
            &AreasConfig::default(),
            AreaSort::Name,
            false,
        );

        assert_eq!(output.areas[0].name, "a-area");
        assert_eq!(output.areas[1].name, "z-area");
    }

    #[test]
    fn cmd_areas_human_output_contains_header() {
        let graph = DiGraph::new();
        let lattice = Lattice::test_empty();
        let output = cmd_areas(
            &graph,
            &lattice,
            &[],
            &AreasConfig::default(),
            AreaSort::Files,
            false,
        );

        let mut buf = Vec::new();
        output.print_human(&mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("Area"), "header should contain 'Area'");
        assert!(text.contains("Grade"), "header should contain 'Grade'");
    }
}
