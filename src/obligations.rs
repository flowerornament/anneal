use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{HandleKind, resolved_file};
use crate::lattice::Lattice;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObligationDisposition {
    Outstanding,
    Discharged,
    MultiDischarged,
    Mooted,
}

impl ObligationDisposition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Outstanding => "outstanding",
            Self::Discharged => "discharged",
            Self::MultiDischarged => "multi_discharged",
            Self::Mooted => "mooted",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ObligationEntry {
    pub(crate) handle: String,
    pub(crate) namespace: String,
    pub(crate) disposition: ObligationDisposition,
    pub(crate) discharge_count: usize,
    pub(crate) file: Option<String>,
    pub(crate) dischargers: Vec<String>,
}

pub(crate) fn collect_obligation_summaries(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    include_mooted: bool,
) -> Vec<ObligationEntry> {
    collect_obligations_with(graph, lattice, config, include_mooted, false)
}

fn collect_obligations_with(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    include_mooted: bool,
    include_dischargers: bool,
) -> Vec<ObligationEntry> {
    let linear_namespaces = config.handles.linear_set();
    graph
        .nodes()
        .filter_map(|(node_id, handle)| {
            obligation_entry(
                graph,
                lattice,
                &linear_namespaces,
                node_id,
                handle,
                include_mooted,
                include_dischargers,
            )
        })
        .collect()
}

pub(crate) fn lookup_obligation(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    node_id: crate::handle::NodeId,
) -> Option<ObligationEntry> {
    let linear_namespaces = config.handles.linear_set();
    obligation_entry(
        graph,
        lattice,
        &linear_namespaces,
        node_id,
        graph.node(node_id),
        true,
        true,
    )
}

pub(crate) fn obligation_disposition(
    terminal: bool,
    discharge_count: usize,
) -> ObligationDisposition {
    if terminal {
        ObligationDisposition::Mooted
    } else if discharge_count == 0 {
        ObligationDisposition::Outstanding
    } else if discharge_count == 1 {
        ObligationDisposition::Discharged
    } else {
        ObligationDisposition::MultiDischarged
    }
}

fn obligation_entry(
    graph: &DiGraph,
    lattice: &Lattice,
    linear_namespaces: &std::collections::HashSet<&str>,
    node_id: crate::handle::NodeId,
    handle: &crate::handle::Handle,
    include_mooted: bool,
    include_dischargers: bool,
) -> Option<ObligationEntry> {
    let HandleKind::Label { prefix, .. } = &handle.kind else {
        return None;
    };
    if !linear_namespaces.contains(prefix.as_str()) {
        return None;
    }

    let mut dischargers = Vec::new();
    let mut discharge_count = 0;
    for edge in graph.incoming(node_id) {
        if edge.kind != EdgeKind::Discharges {
            continue;
        }
        discharge_count += 1;
        if include_dischargers {
            dischargers.push(graph.node(edge.source).id.clone());
        }
    }
    let disposition = obligation_disposition(handle.is_terminal(lattice), discharge_count);
    if !include_mooted && disposition == ObligationDisposition::Mooted {
        return None;
    }

    Some(ObligationEntry {
        handle: handle.id.clone(),
        namespace: prefix.clone(),
        disposition,
        discharge_count,
        file: resolved_file(handle, graph).map(ToString::to_string),
        dischargers,
    })
}
