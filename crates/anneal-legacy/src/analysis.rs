use std::collections::HashMap;

use crate::checks;
use crate::config;
use crate::handle::NodeId;
use crate::lattice;
use crate::parse;
use crate::snapshot;

pub(crate) struct AnalysisArtifacts {
    pub(crate) previous_snapshot: Option<snapshot::Snapshot>,
    pub(crate) diagnostics: Vec<checks::Diagnostic>,
}

pub(crate) struct AnalysisContext<'a> {
    pub(crate) root: &'a camino::Utf8Path,
    pub(crate) graph: &'a crate::graph::DiGraph,
    pub(crate) lattice: &'a lattice::Lattice,
    pub(crate) config: &'a config::AnnealConfig,
    pub(crate) state_config: &'a config::ResolvedStateConfig,
    pub(crate) result: &'a parse::BuildResult,
    pub(crate) node_index: &'a HashMap<String, NodeId>,
    pub(crate) cascade_candidates: &'a HashMap<String, Vec<String>>,
}

/// Clone only unresolved pending edges for check/query analysis.
///
/// Section refs are counted separately for the I001 summary diagnostic.
/// `section_ref_file` is the file path of the first section-ref source, used
/// as a representative location for the I001 diagnostic.
pub(crate) fn collect_unresolved_owned(
    pending: &[parse::PendingEdge],
    node_index: &HashMap<String, NodeId>,
    graph: &crate::graph::DiGraph,
) -> (Vec<parse::PendingEdge>, usize, Option<String>) {
    let mut unresolved = Vec::new();
    let mut section_ref_count: usize = 0;
    let mut section_ref_file: Option<String> = None;

    for edge in pending {
        if node_index.contains_key(&edge.target_identity) {
            continue;
        }
        if edge.target_identity.starts_with("section:") {
            section_ref_count += 1;
            if section_ref_file.is_none() {
                section_ref_file = graph
                    .node(edge.source)
                    .file_path
                    .as_ref()
                    .map(ToString::to_string);
            }
        } else {
            unresolved.push(edge.clone());
        }
    }

    (unresolved, section_ref_count, section_ref_file)
}

pub(crate) fn build_analysis_artifacts(context: &AnalysisContext<'_>) -> AnalysisArtifacts {
    build_analysis_artifacts_with_selection(context, checks::DiagnosticSelection::all())
}

pub(crate) fn build_analysis_artifacts_with_selection(
    context: &AnalysisContext<'_>,
    selection: checks::DiagnosticSelection,
) -> AnalysisArtifacts {
    let (unresolved_owned, section_ref_count, section_ref_file) = if selection.existence {
        collect_unresolved_owned(
            context.result.pending_edges.as_slice(),
            context.node_index,
            context.graph,
        )
    } else {
        (Vec::new(), 0, None)
    };
    let previous_snapshot = selection
        .includes_suggestions()
        .then(|| snapshot::read_latest_snapshot(context.root, context.state_config))
        .flatten();

    let check_input = checks::CheckInput {
        graph: context.graph,
        lattice: context.lattice,
        config: context.config,
        unresolved_edges: &unresolved_owned,
        section_ref_count,
        section_ref_file: section_ref_file.as_deref(),
        implausible_refs: context.result.implausible_refs.as_slice(),
        cascade_candidates: context.cascade_candidates,
        previous_snapshot: previous_snapshot.as_ref(),
    };
    let mut diagnostics = checks::run_checks_with_selection(&check_input, selection);
    checks::apply_suppressions(&mut diagnostics, &context.config.suppress);

    AnalysisArtifacts {
        previous_snapshot,
        diagnostics,
    }
}

pub(crate) fn matches_scoped_file(path: &str, file_filter: &str) -> bool {
    let normalized = path.strip_prefix("./").unwrap_or(path);
    normalized == file_filter || normalized.ends_with(&format!("/{file_filter}"))
}

pub(crate) fn retain_diagnostics_for_file(
    diagnostics: &mut Vec<checks::Diagnostic>,
    root: &str,
    file: &str,
) {
    let normalized = file.strip_prefix("./").unwrap_or(file);
    let normalized = normalized
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(normalized);

    diagnostics.retain(|d| {
        d.file
            .as_ref()
            .is_some_and(|diag_file| matches_scoped_file(diag_file, normalized))
    });
}
