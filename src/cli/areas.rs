use serde::Serialize;

use crate::area::{AreaHealth, compute_areas};
use crate::checks::Diagnostic;
use crate::config::AreasConfig;
use crate::graph::DiGraph;
use crate::lattice::Lattice;
use crate::output::{Line, Printer, Render, TableHeader, Toned};

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

impl Render for AreasOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        p.heading("Areas", Some(self.areas.len()))?;
        p.blank()?;

        let headers = &[
            TableHeader::text("Area"),
            TableHeader::numeric("Files"),
            TableHeader::numeric("Conn"),
            TableHeader::numeric("Cross"),
            TableHeader::text("Grade"),
            TableHeader::text("Signal"),
        ];
        let rows: Vec<Vec<Line>> = self
            .areas
            .iter()
            .map(|a| {
                vec![
                    Line::new().path(format!("{}/", a.name)),
                    Line::new().count(a.files),
                    Line::new().float(a.connectivity, 1),
                    Line::new().count(a.cross_links),
                    Line::new().toned(a.grade.tone(), format!("[{}]", a.grade)),
                    Line::new().dim(a.signal.clone()),
                ]
            })
            .collect();
        p.table(headers, &rows)?;

        let degraded = self.areas.iter().filter(|a| a.grade.is_degraded()).count();
        if degraded > 0 {
            p.blank()?;
            p.hints(&[(
                "anneal garden",
                &format!(
                    "ranked tasks for the {degraded} degraded area{}",
                    if degraded == 1 { "" } else { "s" }
                ),
            )])?;
        }
        Ok(())
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
    use crate::output::OutputStyle;

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

        let (writer, buf) = crate::output::test_support::SharedBuf::new();
        let mut p = Printer::new(writer, OutputStyle::plain());
        output.render(&mut p).unwrap();
        let text = String::from_utf8(buf.borrow().clone()).unwrap();
        assert!(text.contains("Area"), "header should contain 'Area'");
        assert!(text.contains("Grade"), "header should contain 'Grade'");
    }
}
