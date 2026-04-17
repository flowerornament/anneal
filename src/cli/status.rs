use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::checks::{DiagnosticCode, Severity};
use crate::graph::DiGraph;
use crate::handle::HandleKind;
use crate::lattice::Lattice;

use super::{DetailLevel, OutputMeta, plural};

// ---------------------------------------------------------------------------
// Status command (CLI-04, KB-C4, spec section 12.4)
// ---------------------------------------------------------------------------

/// A single pipeline level with handle count.
#[derive(Clone, Serialize)]
pub(crate) struct PipelineLevel {
    pub(crate) level: String,
    pub(crate) count: usize,
}

/// Obligation summary for status dashboard.
#[derive(Clone, Serialize)]
pub(crate) struct ObligationSummary {
    pub(crate) discharged: usize,
    pub(crate) total: usize,
    pub(crate) mooted: usize,
}

/// Diagnostic counts for status dashboard.
#[derive(Clone, Serialize)]
pub(crate) struct DiagnosticSummary {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
}

/// Convergence signal for status dashboard output.
#[derive(Clone, Serialize)]
pub(crate) struct ConvergenceSummaryOutput {
    pub(crate) signal: String,
    pub(crate) detail: String,
}

/// Output of `anneal status`: single-screen dashboard for arriving agents.
///
/// Matches spec section 12.4 / KB-C4. Shows file/handle/edge counts,
/// active/frozen partition, pipeline histogram or flat lattice counts (D-11),
/// obligation summary, diagnostic counts, convergence signal, and suggestions.
#[derive(Serialize)]
pub(crate) struct StatusOutput {
    pub(crate) files: usize,
    pub(crate) handles: usize,
    pub(crate) edges: usize,
    pub(crate) active_handles: usize,
    pub(crate) frozen_handles: usize,
    pub(crate) pipeline: Option<Vec<PipelineLevel>>,
    pub(crate) states: HashMap<String, usize>,
    pub(crate) obligations: ObligationSummary,
    pub(crate) diagnostics: DiagnosticSummary,
    pub(crate) convergence: Option<ConvergenceSummaryOutput>,
    pub(crate) suggestion_total: usize,
    pub(crate) suggestion_breakdown: Vec<SuggestionCount>,
}

#[derive(Serialize)]
pub(crate) struct StatusCompactOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) files: usize,
    pub(crate) handles: usize,
    pub(crate) edges: usize,
    pub(crate) active_handles: usize,
    pub(crate) frozen_handles: usize,
    pub(crate) pipeline: Option<Vec<PipelineLevel>>,
    pub(crate) states: HashMap<String, usize>,
    pub(crate) obligations: ObligationSummary,
    pub(crate) diagnostics: DiagnosticSummary,
    pub(crate) convergence: Option<ConvergenceSummaryOutput>,
    pub(crate) suggestion_total: usize,
}

/// A single suggestion type with its count, for the status breakdown.
#[derive(Serialize)]
pub(crate) struct SuggestionCount {
    pub(crate) code: String,
    pub(crate) label: String,
    pub(crate) count: usize,
}

impl StatusOutput {
    /// Print dashboard without verbose expansion (used by tests).
    #[cfg(test)]
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        self.print_human_inner(w, false, None, None)
    }

    /// Print dashboard with optional verbose pipeline expansion.
    pub(crate) fn print_human_with_options(
        &self,
        w: &mut dyn Write,
        verbose: bool,
        graph: &DiGraph,
        lattice: &Lattice,
    ) -> std::io::Result<()> {
        self.print_human_inner(w, verbose, Some(graph), Some(lattice))
    }

    fn print_human_inner(
        &self,
        w: &mut dyn Write,
        verbose: bool,
        graph: Option<&DiGraph>,
        lattice: Option<&Lattice>,
    ) -> std::io::Result<()> {
        use crate::style::S;

        // -- Graph --
        writeln!(
            w,
            " {}  {}",
            S.label.apply_to("corpus"),
            fmt_counts(&[
                (self.files, "file"),
                (self.handles, "handle"),
                (self.edges, "edge"),
            ])
        )?;
        writeln!(
            w,
            "         {} active, {} terminal",
            self.active_handles, self.frozen_handles,
        )?;

        // Pipeline histogram (D-11)
        if let Some(ref pipeline) = self.pipeline {
            let parts: Vec<String> = pipeline
                .iter()
                .map(|p| format!("{} {}", S.bold.apply_to(p.count), p.level))
                .collect();
            writeln!(
                w,
                "    {}  {}",
                S.label.apply_to("pipeline"),
                parts.join(" → ")
            )?;

            // Verbose: list files at each pipeline level (single graph pass)
            if verbose && let (Some(graph), Some(lattice)) = (graph, lattice) {
                // Collect all files grouped by status in one pass
                let mut by_status: HashMap<&str, Vec<&str>> = HashMap::new();
                for (_, h) in graph.nodes() {
                    if let HandleKind::File(ref path) = h.kind
                        && let Some(ref status) = h.status
                        && !lattice.terminal.contains(status)
                    {
                        by_status
                            .entry(status.as_str())
                            .or_default()
                            .push(path.as_str());
                    }
                }
                for level in pipeline {
                    let Some(files) = by_status.get_mut(level.level.as_str()) else {
                        continue;
                    };
                    files.sort_unstable();
                    writeln!(
                        w,
                        "              {} {}:",
                        S.bold.apply_to(&level.level),
                        S.dim.apply_to(format_args!("({})", files.len())),
                    )?;
                    for f in files.iter() {
                        writeln!(w, "                {f}")?;
                    }
                }
            }
        }

        // -- Health --
        writeln!(w)?;
        let health_color = if self.diagnostics.errors > 0 {
            &S.error
        } else if self.diagnostics.warnings > 0 {
            &S.warning
        } else {
            &S.green
        };
        let outstanding = self.outstanding_obligations();
        write!(
            w,
            " {}  {} error{}, {} warning{}",
            S.label.apply_to("health"),
            health_color.apply_to(self.diagnostics.errors),
            plural(self.diagnostics.errors),
            self.diagnostics.warnings,
            plural(self.diagnostics.warnings),
        )?;
        if self.obligations.total > 0 {
            write!(
                w,
                ", {}/{} obligations discharged",
                self.obligations.discharged, self.obligations.total,
            )?;
            if self.obligations.mooted > 0 {
                write!(w, " ({} mooted)", self.obligations.mooted)?;
            }
            if outstanding > 0 {
                write!(w, " — {outstanding} outstanding")?;
            }
        }
        writeln!(w)?;

        // -- Convergence --
        writeln!(w)?;
        if let Some(ref conv) = self.convergence {
            let signal_style = match conv.signal.as_str() {
                "advancing" => &S.green,
                "drifting" => &S.warning,
                _ => &S.dim,
            };
            writeln!(
                w,
                " {}  {} {}",
                S.label.apply_to("convergence"),
                signal_style.apply_to(&conv.signal),
                S.dim.apply_to(format_args!("({})", conv.detail)),
            )?;
        } else {
            writeln!(
                w,
                " {}  {}",
                S.label.apply_to("convergence"),
                S.dim.apply_to("(no history yet)"),
            )?;
        }

        // -- Suggestions --
        let active: Vec<&SuggestionCount> = self
            .suggestion_breakdown
            .iter()
            .filter(|s| s.count > 0)
            .collect();
        if !active.is_empty() {
            writeln!(w)?;
            writeln!(
                w,
                " {}  {}",
                S.label.apply_to("suggestions"),
                self.suggestion_total,
            )?;
            for s in &active {
                writeln!(
                    w,
                    "   {:>5}  {} {}",
                    s.count,
                    S.suggestion.apply_to(&s.code),
                    S.dim.apply_to(&s.label),
                )?;
            }
        }

        let has_hints = self.diagnostics.errors > 0 || outstanding > 0 || self.suggestion_total > 0;
        if has_hints {
            writeln!(w)?;
            writeln!(w, " {}", S.label.apply_to("next"))?;
            if self.diagnostics.errors > 0 {
                writeln!(
                    w,
                    "   {} for detailed diagnostics",
                    S.dim.apply_to("anneal check"),
                )?;
            }
            if outstanding > 0 {
                writeln!(
                    w,
                    "   {} to inspect a specific obligation",
                    S.dim.apply_to("anneal explain obligation <handle>"),
                )?;
            }
            if self.suggestion_total > 0 {
                writeln!(
                    w,
                    "   {} for ranked maintenance tasks",
                    S.dim.apply_to("anneal garden"),
                )?;
            }
        }

        Ok(())
    }

    fn outstanding_obligations(&self) -> usize {
        self.obligations
            .total
            .saturating_sub(self.obligations.discharged)
            .saturating_sub(self.obligations.mooted)
    }

    /// Set convergence after construction (caller computes from snapshot history).
    pub(crate) fn with_convergence(mut self, summary: Option<ConvergenceSummaryOutput>) -> Self {
        self.convergence = summary;
        self
    }

    pub(crate) fn compact_json(&self) -> StatusCompactOutput {
        StatusCompactOutput {
            meta: OutputMeta::new(
                DetailLevel::Summary,
                false,
                None,
                None,
                vec!["status --json".to_string()],
            ),
            files: self.files,
            handles: self.handles,
            edges: self.edges,
            active_handles: self.active_handles,
            frozen_handles: self.frozen_handles,
            pipeline: self.pipeline.clone(),
            states: self.states.clone(),
            obligations: ObligationSummary {
                discharged: self.obligations.discharged,
                total: self.obligations.total,
                mooted: self.obligations.mooted,
            },
            diagnostics: DiagnosticSummary {
                errors: self.diagnostics.errors,
                warnings: self.diagnostics.warnings,
            },
            convergence: self.convergence.clone(),
            suggestion_total: self.suggestion_total,
        }
    }
}

/// Format counts like "262 files, 9882 handles, 6974 edges".
fn fmt_counts(items: &[(usize, &str)]) -> String {
    items
        .iter()
        .map(|(n, label)| format!("{n} {label}{}", plural(*n)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the status dashboard from the graph, lattice, config, and diagnostics.
///
/// Counts files, handles, edges, active/frozen partition, pipeline levels,
/// obligations (linear namespaces), diagnostics, and suggestions.
/// Convergence is set to `None` here; the caller in main.rs computes it
/// from snapshot history via `with_convergence`.
///
/// Derives counts from the pre-built snapshot to avoid a redundant graph traversal.
/// The only extra traversal is counting File handles (not tracked in snapshots).
pub(crate) fn cmd_status(
    graph: &DiGraph,
    lattice: &Lattice,
    snap: &crate::snapshot::Snapshot,
    diagnostics_list: &[crate::checks::Diagnostic],
    area: Option<&crate::area::AreaFilter>,
    temporal: Option<&crate::area::TemporalFilter>,
) -> StatusOutput {
    // When scoped to an area or temporal window, compute counts from
    // the graph directly instead of using the corpus-wide snapshot.
    let scoped = area.is_some() || temporal.is_some();
    let (files, handles, edges, active_handles, frozen_handles, states) = if scoped {
        let mut files = 0usize;
        let mut handles = 0usize;
        let mut active = 0usize;
        let mut terminal = 0usize;
        let mut edge_count = 0usize;
        let mut states: HashMap<String, usize> = HashMap::new();

        for (node_id, h) in graph.nodes() {
            if area.is_some_and(|af| !af.matches_handle(h)) {
                continue;
            }
            if temporal.is_some_and(|tf| !tf.matches_handle(h)) {
                continue;
            }
            handles += 1;
            if matches!(h.kind, HandleKind::File(_)) {
                files += 1;
            }
            if let Some(ref s) = h.status {
                *states.entry(s.clone()).or_insert(0) += 1;
                if lattice.terminal.contains(s) {
                    terminal += 1;
                } else {
                    active += 1;
                }
            }
            edge_count += graph.outgoing(node_id).len();
        }

        (files, handles, edge_count, active, terminal, states)
    } else {
        let files = graph
            .nodes()
            .filter(|(_, h)| matches!(h.kind, HandleKind::File(_)))
            .count();
        (
            files,
            snap.handles.total,
            snap.edges.total,
            snap.handles.active,
            snap.handles.frozen,
            snap.states.clone(),
        )
    };

    // Pipeline histogram from states + lattice ordering
    let pipeline = if lattice.ordering.is_empty() {
        None
    } else {
        Some(
            lattice
                .ordering
                .iter()
                .map(|level| PipelineLevel {
                    level: level.clone(),
                    count: states.get(level).copied().unwrap_or(0),
                })
                .collect(),
        )
    };

    // Suggestion breakdown by code
    let mut code_counts: HashMap<DiagnosticCode, usize> = HashMap::new();
    for d in diagnostics_list {
        if d.severity == Severity::Suggestion {
            *code_counts.entry(d.code).or_insert(0) += 1;
        }
    }
    let suggestion_total: usize = code_counts.values().sum();

    let suggestion_labels: &[(DiagnosticCode, &str)] = &[
        (DiagnosticCode::S001, "orphaned handles"),
        (DiagnosticCode::S002, "candidate namespaces"),
        (DiagnosticCode::S003, "pipeline stalls"),
        (DiagnosticCode::S004, "abandoned namespaces"),
        (DiagnosticCode::S005, "concern group candidates"),
    ];
    let suggestion_breakdown: Vec<SuggestionCount> = suggestion_labels
        .iter()
        .map(|&(code, label)| SuggestionCount {
            code: code.to_string(),
            label: label.to_string(),
            count: code_counts.get(&code).copied().unwrap_or(0),
        })
        .collect();

    StatusOutput {
        files,
        handles,
        edges,
        active_handles,
        frozen_handles,
        pipeline,
        states,
        obligations: ObligationSummary {
            discharged: snap.obligations.discharged,
            total: snap.obligations.outstanding
                + snap.obligations.discharged
                + snap.obligations.mooted,
            mooted: snap.obligations.mooted,
        },
        diagnostics: DiagnosticSummary {
            errors: snap.diagnostics.errors,
            warnings: snap.diagnostics.warnings,
        },
        convergence: None,
        suggestion_total,
        suggestion_breakdown,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::AnnealConfig;
    use crate::graph::DiGraph;
    use crate::handle::Handle;

    use super::*;

    fn make_status_output_basic() -> StatusOutput {
        StatusOutput {
            files: 265,
            handles: 487,
            edges: 2031,
            active_handles: 142,
            frozen_handles: 345,
            pipeline: None,
            states: HashMap::new(),
            obligations: ObligationSummary {
                discharged: 6,
                total: 20,
                mooted: 12,
            },
            diagnostics: DiagnosticSummary {
                errors: 0,
                warnings: 3,
            },
            convergence: None,
            suggestion_total: 2,
            suggestion_breakdown: vec![SuggestionCount {
                code: "S001".into(),
                label: "orphaned handles".into(),
                count: 2,
            }],
        }
    }

    /// Helper: render a StatusOutput to string for assertion (ANSI stripped).
    fn render_status(output: &StatusOutput) -> String {
        let mut buf = Vec::new();
        output.print_human(&mut buf).expect("print_human");
        let raw = String::from_utf8(buf).expect("utf8");
        console::strip_ansi_codes(&raw).to_string()
    }

    #[test]
    fn status_print_human_scanned_line() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("265 files"),
            "Expected file count, got: {text}"
        );
        assert!(
            text.contains("487 handles"),
            "Expected handle count, got: {text}"
        );
        assert!(
            text.contains("2031 edges"),
            "Expected edge count, got: {text}"
        );
    }

    #[test]
    fn status_print_human_active_terminal_line() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("142 active") && text.contains("345 terminal"),
            "Expected active/terminal counts, got: {text}"
        );
    }

    #[test]
    fn status_print_human_pipeline_histogram() {
        let mut output = make_status_output_basic();
        output.pipeline = Some(vec![
            PipelineLevel {
                level: "raw".to_string(),
                count: 12,
            },
            PipelineLevel {
                level: "digested".to_string(),
                count: 8,
            },
            PipelineLevel {
                level: "formal".to_string(),
                count: 6,
            },
        ]);
        let text = render_status(&output);
        assert!(
            text.contains("pipeline"),
            "Expected pipeline section, got: {text}"
        );
        assert!(
            text.contains("12 raw"),
            "Expected '12 raw' in pipeline, got: {text}"
        );
    }

    #[test]
    fn status_print_human_flat_lattice_omits_pipeline() {
        let text = render_status(&make_status_output_basic());
        // pipeline is None => no pipeline line, active/frozen on the corpus line
        assert!(
            !text.contains("pipeline"),
            "Flat lattice should not show pipeline, got: {text}"
        );
    }

    #[test]
    fn status_print_human_obligations_line() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("6/20 obligations discharged"),
            "Expected obligations line, got: {text}"
        );
    }

    #[test]
    fn status_print_human_diagnostics_line() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("0 errors") && text.contains("3 warnings"),
            "Expected diagnostics counts, got: {text}"
        );
    }

    #[test]
    fn status_print_human_convergence_no_history() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("no history"),
            "Expected no history message, got: {text}"
        );
    }

    #[test]
    fn status_print_human_convergence_with_signal() {
        let mut output = make_status_output_basic();
        output.convergence = Some(ConvergenceSummaryOutput {
            signal: "advancing".to_string(),
            detail: "resolution +10, creation +5".to_string(),
        });
        let text = render_status(&output);
        assert!(
            text.contains("advancing"),
            "Expected advancing signal, got: {text}"
        );
        assert!(
            text.contains("resolution +10"),
            "Expected convergence detail, got: {text}"
        );
    }

    #[test]
    fn status_print_human_suggestions_breakdown() {
        let text = render_status(&make_status_output_basic());
        assert!(
            text.contains("suggestions"),
            "Expected suggestions section, got: {text}"
        );
        assert!(
            text.contains("S001"),
            "Expected S001 in breakdown, got: {text}"
        );
        assert!(
            text.contains("orphaned handles"),
            "Expected S001 label, got: {text}"
        );
    }

    #[test]
    fn status_cmd_status_basic_counts() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("doc1.md", None));
        graph.add_node(Handle::test_file("doc2.md", None));
        graph.add_node(Handle::test_label("OQ", 1, None));

        let lattice = Lattice::test_empty();
        let config = AnnealConfig::default();
        let snap = crate::snapshot::build_snapshot(&graph, &lattice, &config, &[]);

        let output = cmd_status(&graph, &lattice, &snap, &[], None, None);

        assert_eq!(output.files, 2);
        assert_eq!(output.handles, 3);
        assert_eq!(output.edges, 0);
    }

    #[test]
    fn status_cmd_status_counts_active_frozen() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("doc1.md", Some("draft")));
        graph.add_node(Handle::test_file("doc2.md", Some("archived")));
        graph.add_node(Handle::test_file("doc3.md", None));

        let lattice = Lattice::test_new(&[], &["archived"]);
        let config = AnnealConfig::default();
        let snap = crate::snapshot::build_snapshot(&graph, &lattice, &config, &[]);

        let output = cmd_status(&graph, &lattice, &snap, &[], None, None);

        // doc1.md (draft, not terminal) + doc3.md (no status) = 2 active
        assert_eq!(output.active_handles, 2);
        // doc2.md (archived, terminal) = 1 frozen
        assert_eq!(output.frozen_handles, 1);
    }

    #[test]
    fn status_compact_json_keeps_core_counts() {
        let compact = make_status_output_basic().compact_json();
        assert_eq!(compact.files, 265);
        assert_eq!(compact.handles, 487);
        assert_eq!(compact.suggestion_total, 2);
        assert!(matches!(compact.meta.detail, DetailLevel::Summary));
    }
}
