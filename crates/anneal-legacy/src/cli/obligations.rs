use std::collections::HashMap;

use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::HandleKind;
use crate::lattice::Lattice;
use crate::output::{Line, Printer, Render, Tone};

// ---------------------------------------------------------------------------
// Obligations command (UX-06)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ObligationNamespace {
    pub(crate) namespace: String,
    pub(crate) outstanding: Vec<String>,
    pub(crate) discharged: Vec<String>,
    pub(crate) mooted: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ObligationsOutput {
    pub(crate) total_outstanding: usize,
    pub(crate) total_discharged: usize,
    pub(crate) total_mooted: usize,
    pub(crate) namespaces: Vec<ObligationNamespace>,
}

type NamespaceBuckets = (Vec<String>, Vec<String>, Vec<String>);

impl Render for ObligationsOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        p.heading("Obligations", None)?;
        p.tally(&[
            (self.total_outstanding, "outstanding"),
            (self.total_discharged, "discharged"),
            (self.total_mooted, "mooted"),
        ])?;

        for ns in &self.namespaces {
            p.blank()?;
            p.heading(
                &ns.namespace,
                Some(ns.outstanding.len() + ns.discharged.len() + ns.mooted.len()),
            )?;
            p.tally(&[
                (ns.outstanding.len(), "outstanding"),
                (ns.discharged.len(), "discharged"),
                (ns.mooted.len(), "mooted"),
            ])?;
            // Aligned status tag so identities line up in a column.
            for id in &ns.outstanding {
                p.line_at(
                    4,
                    &Line::new()
                        .toned(Tone::Warning, "outstanding")
                        .text("  ")
                        .path(id.clone()),
                )?;
            }
            for id in &ns.discharged {
                p.line_at(
                    4,
                    &Line::new()
                        .toned(Tone::Success, "discharged ")
                        .text("  ")
                        .path(id.clone()),
                )?;
            }
            for id in &ns.mooted {
                p.line_at(
                    4,
                    &Line::new()
                        .toned(Tone::Dim, "mooted     ")
                        .text("  ")
                        .path(id.clone()),
                )?;
            }
        }
        Ok(())
    }
}

pub(crate) fn cmd_obligations(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
) -> ObligationsOutput {
    let linear_namespaces = config.handles.linear_set();
    let mut ns_data: HashMap<String, NamespaceBuckets> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        if let HandleKind::Label { ref prefix, .. } = handle.kind {
            if !linear_namespaces.contains(prefix.as_str()) {
                continue;
            }

            let entry = ns_data
                .entry(prefix.clone())
                .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new()));

            if handle.is_terminal(lattice) {
                entry.2.push(handle.id.clone());
            } else {
                let discharge_count = graph
                    .incoming(node_id)
                    .iter()
                    .filter(|edge| edge.kind == EdgeKind::Discharges)
                    .count();
                if discharge_count > 0 {
                    entry.1.push(handle.id.clone());
                } else {
                    entry.0.push(handle.id.clone());
                }
            }
        }
    }

    let mut namespaces: Vec<ObligationNamespace> = ns_data
        .into_iter()
        .map(
            |(namespace, (outstanding, discharged, mooted))| ObligationNamespace {
                namespace,
                outstanding,
                discharged,
                mooted,
            },
        )
        .collect();
    namespaces.sort_by(|a, b| a.namespace.cmp(&b.namespace));

    let total_outstanding = namespaces.iter().map(|n| n.outstanding.len()).sum();
    let total_discharged = namespaces.iter().map(|n| n.discharged.len()).sum();
    let total_mooted = namespaces.iter().map(|n| n.mooted.len()).sum();

    ObligationsOutput {
        total_outstanding,
        total_discharged,
        total_mooted,
        namespaces,
    }
}
