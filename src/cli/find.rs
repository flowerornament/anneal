use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::graph::DiGraph;
use crate::handle::HandleKind;
use crate::lattice::Lattice;

use super::{DetailLevel, OutputMeta};

// ---------------------------------------------------------------------------
// Find command (CLI-03)
// ---------------------------------------------------------------------------

/// A single match from a find query.
#[derive(Clone, Serialize)]
pub(crate) struct FindMatch {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FindFacetValue {
    pub(crate) value: String,
    pub(crate) count: usize,
}

#[derive(Serialize)]
pub(crate) struct FindFacets {
    pub(crate) kind: Vec<FindFacetValue>,
    pub(crate) status: Vec<FindFacetValue>,
}

/// Output of `anneal find <query>`: matching handles.
#[derive(Serialize)]
pub(crate) struct FindOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    pub(crate) query: String,
    pub(crate) matches: Vec<FindMatch>,
    pub(crate) total: usize,
    pub(crate) returned: usize,
    pub(crate) offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) facets: Option<FindFacets>,
}

impl FindOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        if self.meta.truncated || self.offset > 0 {
            writeln!(
                w,
                "Showing {} of {} matches for \"{}\" (offset {}):",
                self.returned, self.total, self.query, self.offset
            )?;
        } else {
            writeln!(w, "Found {} matches for \"{}\":", self.total, self.query)?;
        }
        for m in &self.matches {
            let status_str = m
                .status
                .as_deref()
                .map_or(String::new(), |s| format!(" status: {s}"));
            let file_str = m.file.as_deref().unwrap_or("");
            writeln!(w, "  {} ({}){status_str}  {file_str}", m.id, m.kind)?;
        }
        if self.meta.truncated && !self.meta.expand.is_empty() {
            writeln!(w)?;
            writeln!(w, "More available: {}", self.meta.expand.join(", "))?;
        }
        Ok(())
    }
}

/// Filter options for the find command.
#[derive(Default)]
pub(crate) struct FindFilters<'a> {
    pub(crate) namespace: Option<&'a str>,
    pub(crate) status: Option<&'a str>,
    pub(crate) kind: Option<&'a str>,
    pub(crate) include_all: bool,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: usize,
    pub(crate) full: bool,
    pub(crate) no_facets: bool,
    pub(crate) area: Option<&'a crate::area::AreaFilter>,
}

/// Search handle identities with case-insensitive substring matching.
pub(crate) fn cmd_find(
    graph: &DiGraph,
    lattice: &Lattice,
    query: &str,
    filters: &FindFilters<'_>,
) -> anyhow::Result<FindOutput> {
    let lower_query = query.to_lowercase();

    let has_narrowing_filter = filters.namespace.is_some()
        || filters.status.is_some()
        || filters.kind.is_some()
        || filters.area.is_some();
    if lower_query.is_empty() && !filters.full && !has_narrowing_filter {
        anyhow::bail!("empty query requires a narrowing filter or --full");
    }

    let all_matches: Vec<FindMatch> = graph
        .nodes()
        .filter(|(_, h)| {
            // Substring match on handle identity
            if !lower_query.is_empty() && !h.id.to_lowercase().contains(&lower_query) {
                return false;
            }

            if let Some(kf) = filters.kind
                && h.kind.as_str() != kf
            {
                return false;
            }

            if let Some(ns) = filters.namespace {
                match &h.kind {
                    HandleKind::Label { prefix, .. } => {
                        if prefix != ns {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }

            if let Some(sf) = filters.status {
                match &h.status {
                    Some(s) if s == sf => {}
                    _ => return false,
                }
            }

            if let Some(af) = filters.area
                && !af.matches_handle(h)
            {
                return false;
            }

            // Exclude terminal handles unless user explicitly filtered by status
            if !filters.include_all
                && filters.status.is_none()
                && let Some(ref s) = h.status
                && lattice.terminal.contains(s)
            {
                return false;
            }

            true
        })
        .map(|(_, h)| FindMatch {
            id: h.id.clone(),
            kind: h.kind.as_str().to_string(),
            status: h.status.clone(),
            file: h.file_path.as_ref().map(ToString::to_string),
        })
        .collect();

    let mut sorted_matches = all_matches;
    sorted_matches.sort_by(|a, b| a.id.cmp(&b.id));
    let total = sorted_matches.len();
    let offset = filters.offset.min(total);
    let limit = if filters.full {
        total.saturating_sub(offset)
    } else {
        filters.limit.unwrap_or(25)
    };
    let facets = if filters.no_facets {
        None
    } else {
        Some(build_find_facets(&sorted_matches))
    };
    let returned_matches: Vec<FindMatch> = sorted_matches
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect();
    let returned = returned_matches.len();

    Ok(FindOutput {
        meta: OutputMeta::new(
            if filters.full {
                DetailLevel::Full
            } else {
                DetailLevel::Sample
            },
            !filters.full && offset + returned < total,
            Some(returned),
            Some(total),
            if filters.full || offset + returned >= total {
                Vec::new()
            } else {
                vec![
                    format!("--limit {}", limit.saturating_mul(2).max(25)),
                    format!("--offset {}", offset + returned),
                    "--full".to_string(),
                ]
            },
        ),
        query: query.to_string(),
        matches: returned_matches,
        total,
        returned,
        offset,
        facets,
    })
}

fn build_find_facets(matches: &[FindMatch]) -> FindFacets {
    let mut kind_counts: HashMap<String, usize> = HashMap::new();
    let mut status_counts: HashMap<String, usize> = HashMap::new();
    for entry in matches {
        *kind_counts.entry(entry.kind.clone()).or_insert(0) += 1;
        if let Some(status) = &entry.status {
            *status_counts.entry(status.clone()).or_insert(0) += 1;
        }
    }

    let mut kind: Vec<FindFacetValue> = kind_counts
        .into_iter()
        .map(|(value, count)| FindFacetValue { value, count })
        .collect();
    kind.sort_by(|a, b| a.value.cmp(&b.value));

    let mut status: Vec<FindFacetValue> = status_counts
        .into_iter()
        .map(|(value, count)| FindFacetValue { value, count })
        .collect();
    status.sort_by(|a, b| a.value.cmp(&b.value));

    FindFacets { kind, status }
}

#[cfg(test)]
mod tests {
    use crate::handle::Handle;

    use super::*;

    #[test]
    fn find_default_limit_truncates_results() {
        let mut graph = DiGraph::new();
        for number in 1..=30 {
            graph.add_node(Handle::test_label("OQ", number, Some("draft")));
        }

        let output = cmd_find(
            &graph,
            &Lattice::test_empty(),
            "",
            &FindFilters {
                status: Some("draft"),
                ..FindFilters::default()
            },
        )
        .expect("find output");

        assert_eq!(output.total, 30);
        assert_eq!(output.returned, 25);
        assert!(output.meta.truncated);
    }

    #[test]
    fn find_empty_query_requires_scope_or_full() {
        let graph = DiGraph::new();
        match cmd_find(&graph, &Lattice::test_empty(), "", &FindFilters::default()) {
            Ok(_) => panic!("empty query should fail"),
            Err(err) => assert!(err.to_string().contains("empty query")),
        }
    }
}
