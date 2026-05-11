//! Hand-built large-corpus-shaped fixture for engine-spike testing.
//!
//! Models the trickiest cases from real corpora in a 13-handle subset:
//! a supersession chain, an open obligation in a linear namespace,
//! a discharged obligation, a stale dependency (active → terminal),
//! and a broken reference. The data is `const` and zero-cost.

use crate::types::{Area, EdgeKind, FilePath, HandleId, HandleKind, IsoDate, Namespace, Status};

// ---------------------------------------------------------------------------
// Domain shapes
// ---------------------------------------------------------------------------

/// Stored relation row for `*handle{id, kind, status, namespace, file, line, date, area}`.
#[derive(Copy, Clone, Debug)]
pub struct Handle {
    pub id: HandleId,
    pub kind: HandleKind,
    pub status: Status,
    pub namespace: Namespace,
    pub file: FilePath,
    pub area: Area,
    pub date: Option<IsoDate>,
}

/// Stored relation row for `*edge{from, to, kind, file, line}`.
#[derive(Copy, Clone, Debug)]
pub struct Edge {
    pub from: HandleId,
    pub to: HandleId,
    pub kind: EdgeKind,
    pub file: FilePath,
    pub line: u32,
}

/// A pending-edge fact whose target is unresolved (broken reference).
/// Used to derive `E001` in the ascent program rather than precomputing.
#[derive(Copy, Clone, Debug)]
pub struct PendingEdge {
    pub from: HandleId,
    pub target: HandleId,
    pub kind: EdgeKind,
    pub file: FilePath,
    pub line: u32,
}

// ---------------------------------------------------------------------------
// Const fixture data
// ---------------------------------------------------------------------------

const V17: HandleId = HandleId("formal-model/v17.md");
const V16: HandleId = HandleId("formal-model/v16.md");
const V15: HandleId = HandleId("formal-model/v15.md");
const V14: HandleId = HandleId("formal-model/v14.md");
const JIT_SPEC: HandleId = HandleId("compiler/jit-spec.md");
const JIT_STALE: HandleId = HandleId("compiler/jit-stale.md");
const EXEC: HandleId = HandleId("compiler/exec.md");
const RESEARCH: HandleId = HandleId("research-log/2026-04-jit.md");
const DISCHARGE_NOTE: HandleId = HandleId("synthesis/2026-04-discharge.md");

const OQ_22: HandleId = HandleId("OQ-22"); // open, depended on by settled v17
const OQ_23: HandleId = HandleId("OQ-23"); // open, depended on by settled v17
const OQ_60: HandleId = HandleId("OQ-60"); // open, in compiler area
const OQ_99: HandleId = HandleId("OQ-99"); // resolved → terminal
const OQ_77: HandleId = HandleId("OQ-77"); // discharged by DISCHARGE_NOTE
const OQ_88: HandleId = HandleId("OQ-88"); // open, undischarged → E002

const NS_OQ: Namespace = Namespace("OQ");

const FORMAL: Area = Area("formal-model");
const COMPILER: Area = Area("compiler");
const RESEARCH_LOG: Area = Area("research-log");
const SYNTHESIS: Area = Area("synthesis");

pub const HANDLES: &[Handle] = &[
    // formal-model supersession chain (v17 latest authoritative)
    Handle { id: V17, kind: HandleKind::File, status: Status::Authoritative,
             namespace: Namespace::NONE, file: FilePath("formal-model/v17.md"),
             area: FORMAL, date: Some("2026-03-25") },
    Handle { id: V16, kind: HandleKind::File, status: Status::Superseded,
             namespace: Namespace::NONE, file: FilePath("formal-model/v16.md"),
             area: FORMAL, date: Some("2026-03-10") },
    Handle { id: V15, kind: HandleKind::File, status: Status::Superseded,
             namespace: Namespace::NONE, file: FilePath("formal-model/v15.md"),
             area: FORMAL, date: Some("2026-02-15") },
    Handle { id: V14, kind: HandleKind::File, status: Status::Superseded,
             namespace: Namespace::NONE, file: FilePath("formal-model/v14.md"),
             area: FORMAL, date: Some("2026-02-01") },
    // compiler area: a draft spec with a stale dep, a stale file, an active downstream
    Handle { id: JIT_SPEC, kind: HandleKind::File, status: Status::Draft,
             namespace: Namespace::NONE, file: FilePath("compiler/jit-spec.md"),
             area: COMPILER, date: Some("2026-04-10") },
    Handle { id: JIT_STALE, kind: HandleKind::File, status: Status::Superseded,
             namespace: Namespace::NONE, file: FilePath("compiler/jit-stale.md"),
             area: COMPILER, date: Some("2026-02-20") },
    Handle { id: EXEC, kind: HandleKind::File, status: Status::Current,
             namespace: Namespace::NONE, file: FilePath("compiler/exec.md"),
             area: COMPILER, date: Some("2026-04-22") },
    // research-log
    Handle { id: RESEARCH, kind: HandleKind::File, status: Status::Research,
             namespace: Namespace::NONE, file: FilePath("research-log/2026-04-jit.md"),
             area: RESEARCH_LOG, date: Some("2026-04-29") },
    // synthesis: discharges an OQ
    Handle { id: DISCHARGE_NOTE, kind: HandleKind::File, status: Status::Current,
             namespace: Namespace::NONE, file: FilePath("synthesis/2026-04-discharge.md"),
             area: SYNTHESIS, date: Some("2026-04-15") },
    // OQ labels: open / resolved / discharged / undischarged
    Handle { id: OQ_22, kind: HandleKind::Label, status: Status::Open,
             namespace: NS_OQ, file: FilePath("formal-model/v17.md"),
             area: FORMAL, date: None },
    Handle { id: OQ_23, kind: HandleKind::Label, status: Status::Open,
             namespace: NS_OQ, file: FilePath("formal-model/v17.md"),
             area: FORMAL, date: None },
    Handle { id: OQ_60, kind: HandleKind::Label, status: Status::Open,
             namespace: NS_OQ, file: FilePath("compiler/jit-spec.md"),
             area: COMPILER, date: None },
    Handle { id: OQ_77, kind: HandleKind::Label, status: Status::Open,
             namespace: NS_OQ, file: FilePath("research-log/2026-04-jit.md"),
             area: RESEARCH_LOG, date: None },
    Handle { id: OQ_88, kind: HandleKind::Label, status: Status::Open,
             namespace: NS_OQ, file: FilePath("compiler/jit-spec.md"),
             area: COMPILER, date: None },
    Handle { id: OQ_99, kind: HandleKind::Label, status: Status::Resolved,
             namespace: NS_OQ, file: FilePath("formal-model/v16.md"),
             area: FORMAL, date: None },
];

pub const EDGES: &[Edge] = &[
    // v17 depends on three OQs
    Edge { from: V17, to: OQ_22, kind: EdgeKind::DependsOn,
           file: FilePath("formal-model/v17.md"), line: 14 },
    Edge { from: V17, to: OQ_23, kind: EdgeKind::DependsOn,
           file: FilePath("formal-model/v17.md"), line: 14 },
    Edge { from: V17, to: OQ_60, kind: EdgeKind::DependsOn,
           file: FilePath("formal-model/v17.md"), line: 18 },
    // Supersession chain v17 → v16 → v15 → v14
    Edge { from: V17, to: V16, kind: EdgeKind::Supersedes,
           file: FilePath("formal-model/v17.md"), line: 6 },
    Edge { from: V16, to: V15, kind: EdgeKind::Supersedes,
           file: FilePath("formal-model/v16.md"), line: 6 },
    Edge { from: V15, to: V14, kind: EdgeKind::Supersedes,
           file: FilePath("formal-model/v15.md"), line: 6 },
    // jit-spec depends on OQ-22 (settled downstream) and on a stale (terminal) file
    Edge { from: JIT_SPEC, to: OQ_22, kind: EdgeKind::DependsOn,
           file: FilePath("compiler/jit-spec.md"), line: 22 },
    Edge { from: JIT_SPEC, to: JIT_STALE, kind: EdgeKind::DependsOn,
           file: FilePath("compiler/jit-spec.md"), line: 30 },
    // exec depends on jit-spec
    Edge { from: EXEC, to: JIT_SPEC, kind: EdgeKind::DependsOn,
           file: FilePath("compiler/exec.md"), line: 8 },
    // Research cites v17
    Edge { from: RESEARCH, to: V17, kind: EdgeKind::Cites,
           file: FilePath("research-log/2026-04-jit.md"), line: 3 },
    // Discharge: DISCHARGE_NOTE discharges OQ-77
    Edge { from: DISCHARGE_NOTE, to: OQ_77, kind: EdgeKind::Discharges,
           file: FilePath("synthesis/2026-04-discharge.md"), line: 12 },
    // Verifies edge — JIT_SPEC verifies OQ-22 (exercises EdgeKind::Verifies)
    Edge { from: JIT_SPEC, to: OQ_22, kind: EdgeKind::Verifies,
           file: FilePath("compiler/jit-spec.md"), line: 44 },
];

/// One pending edge that points at a non-existent handle — drives the
/// `E001` (broken reference) derivation in the ascent program.
pub const PENDING_EDGES: &[PendingEdge] = &[
    PendingEdge {
        from: JIT_SPEC,
        target: HandleId("OQ-9999"),
        kind: EdgeKind::DependsOn,
        file: FilePath("compiler/jit-spec.md"),
        line: 51,
    },
];

/// Namespaces declared linear — labels in these namespaces are obligations
/// that must be discharged. Drives `E002` derivation.
pub const LINEAR_NAMESPACES: &[Namespace] = &[NS_OQ];

// ---------------------------------------------------------------------------
// Tests — fixture invariants
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn handle_ids_are_unique() {
        let mut seen = HashSet::new();
        for h in HANDLES {
            assert!(seen.insert(h.id), "duplicate handle id: {:?}", h.id);
        }
    }

    #[test]
    fn edge_endpoints_exist_in_handles() {
        let ids: HashSet<HandleId> = HANDLES.iter().map(|h| h.id).collect();
        for e in EDGES {
            assert!(ids.contains(&e.from), "edge.from missing handle: {:?}", e.from);
            assert!(ids.contains(&e.to),   "edge.to   missing handle: {:?}", e.to);
        }
    }

    #[test]
    fn pending_edge_targets_are_genuinely_missing() {
        // If a pending edge's target exists, it's not actually pending.
        let ids: HashSet<HandleId> = HANDLES.iter().map(|h| h.id).collect();
        for p in PENDING_EDGES {
            assert!(!ids.contains(&p.target),
                "pending edge target {:?} actually exists — would not fire E001", p.target);
        }
    }

    #[test]
    fn fixture_exercises_required_capabilities() {
        // Sanity: at least one of each shape the MVS tests rely on.
        assert!(HANDLES.iter().any(|h| h.kind == HandleKind::Label && h.namespace == NS_OQ));
        assert!(HANDLES.iter().any(|h| h.status.is_terminal()));
        assert!(HANDLES.iter().any(|h| h.status == Status::Authoritative));
        assert!(EDGES.iter().any(|e| e.kind == EdgeKind::Supersedes));
        assert!(EDGES.iter().any(|e| e.kind == EdgeKind::Discharges));
        assert!(!PENDING_EDGES.is_empty());
    }
}
