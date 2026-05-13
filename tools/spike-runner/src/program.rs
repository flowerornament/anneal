//! Shared ascent program — the engine-spike's MVS coverage as a typed
//! Datalog program over `crate::types`. Two binaries drive it: one with
//! the hand-coded [`crate::fixture`], one with corpus data from
//! [`crate::loader`]. Row mappings and verdict oracles live alongside
//! the rules so the spike harness is one cohesive unit.

// ascent! generates `_xN` placeholder bindings and auto-deref on Copy
// fields in rule bodies. These trip pedantic clippy in macro-expanded
// code; we suppress only the lints the macro produces, not our own.
#![allow(clippy::no_effect_underscore_binding, clippy::explicit_auto_deref)]

use crate::fixture::{Edge, Handle};
use crate::types::{
    Area, DiagnosticCode, EdgeKind, FilePath, HandleId, HandleKind, IsoDate, Namespace, Severity,
    SnapshotId, Status, PIPELINE_ORDERING,
};
use ascent::ascent;
use ascent::aggregators::count;
use serde::Serialize;
use std::collections::HashMap;

/// A release-blocker fact carrying both head identity and the provenance
/// bindings that triggered it. Discriminant names the clause; payload
/// holds the join bindings.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Blocker {
    BrokenRef { file: FilePath, line: u32 },
    Undischarged,
    StaleDep { target: HandleId },
}

/// One step in an `upstream/2` derivation chain. `Direct` bottoms out at
/// a base edge; `Transitive` carries the mid-handle.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum UpstreamStep {
    Direct,
    Transitive { mid: HandleId },
}

ascent! {
    pub struct AscentProgram;

    relation handle(HandleId, HandleKind, Status, Namespace, FilePath, Area, Option<IsoDate>);
    relation edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation pending_edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation linear_namespace(Namespace);

    // MVS-6: relational form of the spec's `at(<ref>) { *handle{...} }`.
    relation snapshot_handle(SnapshotId, HandleId, Status);
    relation pipeline_position_for(Status, usize);

    // Engine-derived primitives per spec §8.
    relation terminal(HandleId);
    terminal(h) <-- handle(h, _, s, _, _, _, _), if s.is_terminal();

    relation active(HandleId);
    active(h) <-- handle(h, _, s, _, _, _, _), if s.is_active();

    relation settled(HandleId);
    settled(h) <-- handle(h, _, s, _, _, _, _), if s.is_settled();

    relation upstream(HandleId, HandleId);
    upstream(h, anc) <-- edge(h, anc, EdgeKind::DependsOn, _, _);
    upstream(h, anc) <-- edge(h, mid, EdgeKind::DependsOn, _, _), upstream(mid, anc);

    relation recently_advanced(HandleId);
    recently_advanced(h) <--
        handle(h, _, current_status, _, _, _, _),
        snapshot_handle(SnapshotId("snapshot:last"), h, prior_status),
        pipeline_position_for(current_status, n_current),
        pipeline_position_for(prior_status, n_prior),
        if n_current > n_prior;

    relation supersedes_chain(HandleId, HandleId, usize);
    supersedes_chain(s, t, 1) <-- edge(s, t, EdgeKind::Supersedes, _, _);
    supersedes_chain(s, t, d + 1) <--
        edge(s, mid, EdgeKind::Supersedes, _, _),
        supersedes_chain(mid, t, d);

    relation obligation(HandleId);
    obligation(h) <--
        handle(h, HandleKind::Label, _, ns, _, _, _),
        linear_namespace(ns);

    relation discharged(HandleId);
    discharged(h) <-- edge(_, h, EdgeKind::Discharges, _, _);

    relation undischarged(HandleId);
    undischarged(h) <-- obligation(h), !discharged(h), !terminal(h);

    // Diagnostics derived from rules, not pre-baked in fixture data.
    relation diagnostic(DiagnosticCode, Severity, HandleId, FilePath, u32);

    diagnostic(DiagnosticCode::E001, Severity::Error, src, file, line) <--
        pending_edge(src, target, _, file, line),
        !handle(target, _, _, _, _, _, _);

    diagnostic(DiagnosticCode::E002, Severity::Error, h, file, line) <--
        undischarged(h),
        handle(h, _, _, _, file, _, _),
        let line = 1u32;

    diagnostic(DiagnosticCode::W001, Severity::Warning, src, file, line) <--
        edge(src, target, EdgeKind::DependsOn, file, line),
        active(src),
        terminal(target);

    // release_blocker carries provenance in its head value (see Blocker).
    relation release_blocker(HandleId, Blocker);
    release_blocker(h, Blocker::BrokenRef { file: *file, line: *line })
        <-- diagnostic(DiagnosticCode::E001, _, h, file, line);
    release_blocker(h, Blocker::Undischarged)
        <-- diagnostic(DiagnosticCode::E002, _, h, _, _);
    release_blocker(h, Blocker::StaleDep { target: *t })
        <-- edge(h, t, EdgeKind::DependsOn, _, _),
            active(h),
            terminal(t);

    // upstream_via — recursive provenance can't fit in a value column
    // because chains have unbounded length. Reconstruct host-side via
    // `upstream_chain_map`.
    relation upstream_via(HandleId, HandleId, UpstreamStep);
    upstream_via(h, anc, UpstreamStep::Direct)
        <-- edge(h, anc, EdgeKind::DependsOn, _, _);
    upstream_via(h, anc, UpstreamStep::Transitive { mid: *mid })
        <-- edge(h, mid, EdgeKind::DependsOn, _, _), upstream(mid, anc);

    relation open_oq(HandleId);
    open_oq(q) <-- handle(q, HandleKind::Label, _, Namespace("OQ"), _, _, _), !terminal(q);

    relation downstream_settled(HandleId, HandleId);
    downstream_settled(q, x) <--
        open_oq(q),
        edge(x, q, EdgeKind::DependsOn, _, _),
        settled(x);

    relation oq_pressure(HandleId, usize);
    oq_pressure(q, n) <--
        open_oq(q),
        agg n = count() in downstream_settled(q, _);

    relation oq_in_area(Area, HandleId);
    oq_in_area(area, q) <--
        handle(q, HandleKind::Label, _, Namespace("OQ"), _, area, _),
        !terminal(q);

    relation oq_per_area(Area, usize);
    oq_per_area(area, n) <--
        oq_in_area(area, _),
        agg n = count() in oq_in_area(area, _);
}

// ---------------------------------------------------------------------------
// Row types — one struct per capability output, derived Ord + Serialize
// ---------------------------------------------------------------------------

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct HandleRow {
    pub id: HandleId,
    pub kind: HandleKind,
    pub status: Status,
    pub namespace: Namespace,
    pub area: Area,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct BlockerRow {
    pub h: HandleId,
    #[serde(flatten)]
    pub blocker: Blocker,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChainRow { pub depth: usize, pub start: HandleId, pub target: HandleId }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct OpenOqRow { pub q: HandleId }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct PressureRow { pub q: HandleId, pub n: usize }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct AreaCountRow { pub area: Area, pub n: usize }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct DiagnosticRow {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub handle: HandleId,
    pub file: FilePath,
    pub line: u32,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct UpstreamWithProvenance {
    pub h: HandleId,
    pub anc: HandleId,
    #[serde(rename = "_derivation")]
    pub chain: Vec<UpstreamStep>,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdvancedRow { pub h: HandleId }

// ---------------------------------------------------------------------------
// Row producers — pure functions over the program's derived relations
// ---------------------------------------------------------------------------

fn sorted<T: Ord>(iter: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut v: Vec<T> = iter.into_iter().collect();
    v.sort();
    v
}

pub fn mvs1_rows(prog: &AscentProgram) -> Vec<HandleRow> {
    sorted(prog.handle.iter().map(|(id, kind, status, ns, _file, area, _date)| HandleRow {
        id: *id, kind: *kind, status: *status, namespace: *ns, area: *area,
    }))
}

pub fn mvs2_rows(prog: &AscentProgram) -> Vec<BlockerRow> {
    sorted(prog.release_blocker.iter().map(|(h, b)| BlockerRow { h: *h, blocker: *b }))
}

pub fn mvs3_rows(prog: &AscentProgram, root: HandleId) -> Vec<ChainRow> {
    sorted(
        prog.supersedes_chain.iter()
            .filter(|(s, _, _)| *s == root)
            .map(|(s, t, d)| ChainRow { depth: *d, start: *s, target: *t })
    )
}

pub fn mvs4_rows(prog: &AscentProgram) -> Vec<OpenOqRow> {
    sorted(prog.open_oq.iter().map(|(q,)| OpenOqRow { q: *q }))
}

pub fn mvs5a_rows(prog: &AscentProgram) -> Vec<PressureRow> {
    sorted(prog.oq_pressure.iter().map(|(q, n)| PressureRow { q: *q, n: *n }))
}

pub fn mvs5b_rows(prog: &AscentProgram) -> Vec<AreaCountRow> {
    sorted(prog.oq_per_area.iter().map(|(area, n)| AreaCountRow { area: *area, n: *n }))
}

pub fn mvs6_rows(prog: &AscentProgram) -> Vec<AdvancedRow> {
    sorted(prog.recently_advanced.iter().map(|(h,)| AdvancedRow { h: *h }))
}

/// Build a `(from, to) → step` index over `upstream_via` once per call;
/// repeated chain walks reuse it rather than rescanning. Without this,
/// `mvs8_upstream_rows` is `O(|upstream|² · chain_depth)`.
pub fn upstream_chain_map(
    prog: &AscentProgram,
) -> HashMap<(HandleId, HandleId), UpstreamStep> {
    let mut m = HashMap::with_capacity(prog.upstream_via.len());
    for &(h, anc, step) in &prog.upstream_via {
        m.entry((h, anc)).or_insert(step);
    }
    m
}

pub fn reconstruct_upstream_chain<S: std::hash::BuildHasher>(
    map: &HashMap<(HandleId, HandleId), UpstreamStep, S>,
    h: HandleId,
    anc: HandleId,
) -> Vec<UpstreamStep> {
    let mut chain = Vec::new();
    let mut current = h;
    while let Some(&step) = map.get(&(current, anc)) {
        chain.push(step);
        match step {
            UpstreamStep::Direct => break,
            UpstreamStep::Transitive { mid } => current = mid,
        }
    }
    chain
}

pub fn mvs8_upstream_rows(prog: &AscentProgram) -> Vec<UpstreamWithProvenance> {
    let map = upstream_chain_map(prog);
    sorted(prog.upstream.iter().map(|(h, anc)| UpstreamWithProvenance {
        h: *h,
        anc: *anc,
        chain: reconstruct_upstream_chain(&map, *h, *anc),
    }))
}

pub fn diagnostics_derived(prog: &AscentProgram) -> Vec<DiagnosticRow> {
    sorted(prog.diagnostic.iter().map(|(code, sev, h, file, line)| DiagnosticRow {
        code: *code, severity: *sev, handle: *h, file: *file, line: *line,
    }))
}

// ---------------------------------------------------------------------------
// Fact loaders — shared between the fixture-driven and corpus-driven
// binaries. The column order is encoded once here so a `relation handle(...)`
// schema change breaks one place rather than every caller.
// ---------------------------------------------------------------------------

pub fn push_handles(prog: &mut AscentProgram, handles: &[Handle]) {
    prog.handle.reserve(handles.len());
    for h in handles {
        prog.handle.push((h.id, h.kind, h.status, h.namespace, h.file, h.area, h.date));
    }
}

pub fn push_edges(prog: &mut AscentProgram, edges: &[Edge]) {
    prog.edge.reserve(edges.len());
    for e in edges {
        prog.edge.push((e.from, e.to, e.kind, e.file, e.line));
    }
}

pub fn push_pipeline_ordering(prog: &mut AscentProgram) {
    prog.pipeline_position_for.reserve(PIPELINE_ORDERING.len());
    for (i, s) in PIPELINE_ORDERING.iter().enumerate() {
        prog.pipeline_position_for.push((*s, i));
    }
}

pub fn push_linear_namespaces(prog: &mut AscentProgram, namespaces: &[Namespace]) {
    prog.linear_namespace.reserve(namespaces.len());
    for ns in namespaces {
        prog.linear_namespace.push((*ns,));
    }
}
