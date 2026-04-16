mod areas;
mod check;
mod diff;
mod find;
mod get;
mod impact;
mod init;
mod map;
mod obligations;
mod status;

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::graph::{DiGraph, Edge};
use crate::handle::{Handle, HandleKind, NodeId};
use crate::lattice::Lattice;
use crate::resolve::zero_padded_label_candidates;

// Re-export public API of the cli module — used by main.rs
pub(crate) use areas::{AreaSort, cmd_areas};
pub(crate) use check::{
    CheckFilters, CheckJsonOptions, apply_check_filters, build_check_json_output, cmd_check,
};
pub(crate) use diff::cmd_diff;
pub(crate) use find::{FindFilters, cmd_find};
pub(crate) use get::{GetHumanOutput, GetJsonMode, GetJsonOptions, build_get_json_output, cmd_get};
pub(crate) use impact::cmd_impact;
pub(crate) use init::cmd_init;
pub(crate) use map::{MapOptions, cmd_map};
pub(crate) use obligations::cmd_obligations;
pub(crate) use status::{ConvergenceSummaryOutput, cmd_status};
pub(crate) use summary::build_summary;

mod summary;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Bundle of precomputed body-text snippets used by `--context`-style output.
///
/// Snippets are extracted once during corpus scan and then looked up per handle
/// to serve as the fallback when `purpose:`/`note:` frontmatter is absent.
#[derive(Clone, Copy)]
pub(crate) struct SnippetIndex<'a> {
    pub(crate) files: &'a HashMap<String, String>,
    pub(crate) labels: &'a HashMap<String, String>,
}

impl<'a> SnippetIndex<'a> {
    pub(crate) fn lookup(&self, handle: &Handle) -> Option<&'a str> {
        match &handle.kind {
            HandleKind::File(path) => self.files.get(path.as_str()).map(String::as_str),
            HandleKind::Label { .. } => self.labels.get(&handle.id).map(String::as_str),
            _ => None,
        }
    }

    /// Preferred one-line summary for a handle: purpose → note → body snippet.
    pub(crate) fn summary_for(&self, handle: &'a Handle) -> Option<&'a str> {
        handle.summary(self.lookup(handle))
    }
}

/// Build the set of file paths that have terminal status.
pub(crate) fn terminal_file_set(graph: &DiGraph, lattice: &Lattice) -> HashSet<String> {
    graph
        .nodes()
        .filter_map(|(_, h)| {
            if matches!(h.kind, HandleKind::File(_)) && h.is_terminal(lattice) {
                h.file_path.as_ref().map(ToString::to_string)
            } else {
                None
            }
        })
        .collect()
}

/// Look up a handle by exact match, falling back to case-insensitive search.
pub(crate) fn lookup_handle(node_index: &HashMap<String, NodeId>, handle: &str) -> Option<NodeId> {
    node_index
        .get(handle)
        .copied()
        .or_else(|| lookup_canonical_label(node_index, handle))
        .or_else(|| {
            let lower = handle.to_lowercase();
            node_index
                .iter()
                .find(|(k, _)| k.to_lowercase() == lower)
                .map(|(_, &id)| id)
        })
}

fn lookup_canonical_label(node_index: &HashMap<String, NodeId>, handle: &str) -> Option<NodeId> {
    let [primary, alternate] = zero_padded_label_candidates(handle)?;
    node_index
        .get(&primary)
        .copied()
        .or_else(|| node_index.get(&alternate).copied())
}

/// Deduplicate edges by (kind, other_node) and build `EdgeSummary` list.
pub(super) fn dedup_edges(
    edges: &[Edge],
    other_node: impl Fn(&Edge) -> NodeId,
    direction: &str,
    graph: &DiGraph,
) -> Vec<get::EdgeSummary> {
    let mut seen = BTreeSet::new();
    edges
        .iter()
        .filter_map(|e| {
            let kind = e.kind.as_str().to_string();
            let target = graph.node(other_node(e)).id.clone();
            if seen.insert((kind.clone(), target.clone())) {
                Some(get::EdgeSummary {
                    kind,
                    target,
                    direction: direction.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// JSON helper (CLI-09)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) enum JsonStyle {
    Compact,
    Pretty,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetailLevel {
    Summary,
    Sample,
    Full,
}

#[derive(Serialize)]
pub(crate) struct OutputMeta {
    pub(crate) schema_version: u32,
    pub(crate) detail: DetailLevel,
    pub(crate) truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) total: Option<usize>,
    pub(crate) expand: Vec<String>,
}

impl OutputMeta {
    pub(crate) fn new(
        detail: DetailLevel,
        truncated: bool,
        returned: Option<usize>,
        total: Option<usize>,
        expand: Vec<String>,
    ) -> Self {
        Self {
            schema_version: 2,
            detail,
            truncated,
            returned,
            total,
            expand,
        }
    }

    pub(crate) fn full() -> Self {
        Self::new(DetailLevel::Full, false, None, None, Vec::new())
    }
}

#[derive(Serialize)]
pub(crate) struct JsonEnvelope<T: Serialize> {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    #[serde(flatten)]
    pub(crate) data: T,
}

impl<T: Serialize> JsonEnvelope<T> {
    pub(crate) fn new(meta: OutputMeta, data: T) -> Self {
        Self { meta, data }
    }
}

/// Serialize any output type to JSON and print to stdout.
///
/// Since `Serialize` is not object-safe, each command returns its own concrete
/// output struct rather than using trait objects (Pitfall 5).
pub(crate) fn print_json<T: Serialize>(output: &T, style: JsonStyle) -> anyhow::Result<()> {
    let json = match style {
        JsonStyle::Compact => serde_json::to_string(output)?,
        JsonStyle::Pretty => serde_json::to_string_pretty(output)?,
    };
    println!("{json}");
    Ok(())
}

/// Returns "s" for plural, "" for singular.
pub(super) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
pub(super) mod test_helpers {
    use std::collections::HashMap;

    use crate::checks::{Diagnostic, DiagnosticCode, Severity};
    use crate::graph::DiGraph;
    use crate::handle::NodeId;
    use crate::snapshot::{
        DiagnosticCounts, EdgeCounts, HandleCounts, NamespaceStats, ObligationCounts, Snapshot,
    };

    pub(crate) fn test_node_index(graph: &DiGraph) -> HashMap<String, NodeId> {
        crate::resolve::build_node_index(graph)
    }

    pub(crate) fn test_diag(code: DiagnosticCode, file: &str) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: format!("{code} from {file}"),
            file: Some(file.to_string()),
            line: Some(1),
            evidence: None,
        }
    }

    pub(crate) fn make_snapshot_base() -> Snapshot {
        Snapshot {
            timestamp: "2026-03-29T00:00:00Z".to_string(),
            handles: HandleCounts {
                total: 100,
                active: 60,
                frozen: 40,
            },
            edges: EdgeCounts { total: 200 },
            states: {
                let mut m = HashMap::new();
                m.insert("draft".to_string(), 30);
                m.insert("formal".to_string(), 20);
                m.insert("archived".to_string(), 40);
                m
            },
            obligations: ObligationCounts {
                outstanding: 5,
                discharged: 10,
                mooted: 3,
            },
            diagnostics: DiagnosticCounts {
                errors: 0,
                warnings: 0,
            },
            namespaces: {
                let mut m = HashMap::new();
                m.insert(
                    "OQ".to_string(),
                    NamespaceStats {
                        total: 69,
                        open: 44,
                        resolved: 19,
                        deferred: 6,
                    },
                );
                m
            },
        }
    }

    pub(crate) fn repo_state() -> crate::config::ResolvedStateConfig {
        crate::config::ResolvedStateConfig {
            history_mode: crate::config::HistoryMode::Repo,
            history_dir: None,
        }
    }
}
