//! `ascent_spike` — MVS capabilities exercised against the `ascent` Datalog
//! crate.
//!
//! Loads the large-corpus-shaped fixture, runs an ascent program that derives
//! anneal's diagnostic and convergence predicates (E001, E002, W001,
//! `release_blocker`, `supersedes_chain`, OQ pressure), and streams
//! per-row NDJSON to stdout via [`spike_runner::capability::emit`].
//!
//! Each MVS capability has both a row-producing function and a verify
//! function. The verify functions are pure oracles consumed by both the
//! binary's main loop and the `#[test]` suite.

// The ascent! macro expands `_` placeholders to internal `_xN` bindings
// and uses auto-deref patterns that don't survive strict pedantic clippy.
#![allow(clippy::no_effect_underscore_binding, clippy::explicit_auto_deref)]

use ascent::ascent;
use ascent::aggregators::count;
use serde::Serialize;
use spike_runner::capability::{emit, Verdict};
use spike_runner::types::{
    Area, DiagnosticCode, EdgeKind, FilePath, HandleId, HandleKind, IsoDate, Namespace, Severity,
    Status,
};
use spike_runner::fixture::ids::{EXEC, JIT_SPEC, OQ_22, OQ_99, V14, V15, V16, V17};
use spike_runner::{
    EDGES, HANDLES, LINEAR_NAMESPACES, PENDING_EDGES, PIPELINE_ORDERING, SNAPSHOTS,
};
use spike_runner::types::SnapshotId;
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

/// A release-blocker fact, carrying both the head identity and the
/// provenance bindings that triggered it. Replaces the dual
/// `release_blocker/2` + `release_blocker_via/3` companion relations
/// the earlier spike used — one typed relation, one type.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Blocker {
    BrokenRef { file: FilePath, line: u32 },
    Undischarged,
    StaleDep { target: HandleId },
}

/// One step in an `upstream/2` derivation chain. `Direct` bottoms out at
/// a base edge; `Transitive` carries the mid-handle and points to a
/// further step in the chain.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
enum UpstreamStep {
    Direct,
    Transitive { mid: HandleId },
}

ascent! {
    pub struct AscentProgram;

    relation handle(HandleId, HandleKind, Status, Namespace, FilePath, Area, Option<IsoDate>);
    relation edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation pending_edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation linear_namespace(Namespace);

    // MVS-6 time-travel primitives: snapshot_handle is the relational
    // form of the spec's `at(<ref>) { *handle{...} }` block; the engine
    // joins on this rather than re-evaluating the program at a time
    // point. pipeline_position_for is the spec's §8 derived predicate
    // loaded as a lookup table at fixpoint entry.
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

    // MVS-6: handle advanced in the pipeline since the last snapshot.
    // Mirrors spec §16 recently_advanced/1 — joins current handle status
    // and snapshot_handle status, requires pipeline position to increase.
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

    // Diagnostics derived from rules, not pre-baked in fixture data —
    // this is the §17/LR-D8 vision: checks are Horn clauses.
    relation diagnostic(DiagnosticCode, Severity, HandleId, FilePath, u32);

    diagnostic(DiagnosticCode::E001, Severity::Error, src, file, line) <--
        pending_edge(src, target, _, file, line),
        !handle(target, _, _, _, _, _, _);

    // E002 surfaces file/line from the obligation's declaring file; line
    // pinned to 1 because the obligation is a frontmatter-level fact, not
    // a body reference.
    diagnostic(DiagnosticCode::E002, Severity::Error, h, file, line) <--
        undischarged(h),
        handle(h, _, _, _, file, _, _),
        let line = 1u32;

    diagnostic(DiagnosticCode::W001, Severity::Warning, src, file, line) <--
        edge(src, target, EdgeKind::DependsOn, file, line),
        active(src),
        terminal(target);

    // release_blocker carries provenance directly in its `Blocker` value:
    // the discriminant names the clause that fired, the payload carries
    // the join bindings. This is the spike's answer to "ascent has no
    // built-in derivation trail" — bake provenance into the head's value
    // column rather than maintain a parallel `_via` companion.
    relation release_blocker(HandleId, Blocker);
    release_blocker(h, Blocker::BrokenRef { file: *file, line: *line })
        <-- diagnostic(DiagnosticCode::E001, _, h, file, line);
    release_blocker(h, Blocker::Undischarged)
        <-- diagnostic(DiagnosticCode::E002, _, h, _, _);
    release_blocker(h, Blocker::StaleDep { target: *t })
        <-- edge(h, t, EdgeKind::DependsOn, _, _),
            active(h),
            terminal(t);

    // upstream_via is the recursive case: provenance can't fit in a single
    // value column because chains have unbounded length. Keep the companion
    // relation here and reconstruct chains host-side via upstream_chain_map.
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
// Fixture loading
// ---------------------------------------------------------------------------

fn load_fixture() -> AscentProgram {
    let mut prog = AscentProgram::default();
    prog.handle.reserve(HANDLES.len());
    for h in HANDLES {
        prog.handle.push((h.id, h.kind, h.status, h.namespace, h.file, h.area, h.date));
    }
    prog.edge.reserve(EDGES.len());
    for e in EDGES {
        prog.edge.push((e.from, e.to, e.kind, e.file, e.line));
    }
    prog.pending_edge.reserve(PENDING_EDGES.len());
    for p in PENDING_EDGES {
        prog.pending_edge.push((p.from, p.target, p.kind, p.file, p.line));
    }
    prog.linear_namespace.reserve(LINEAR_NAMESPACES.len());
    for ns in LINEAR_NAMESPACES {
        prog.linear_namespace.push((*ns,));
    }
    prog.snapshot_handle.reserve(SNAPSHOTS.len());
    for s in SNAPSHOTS {
        prog.snapshot_handle.push((s.id, s.handle, s.status));
    }
    prog.pipeline_position_for.reserve(PIPELINE_ORDERING.len());
    for (i, s) in PIPELINE_ORDERING.iter().enumerate() {
        prog.pipeline_position_for.push((*s, i));
    }
    prog.run();
    prog
}

fn sorted<T: Ord>(iter: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut v: Vec<T> = iter.into_iter().collect();
    v.sort();
    v
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct HandleRow {
    id: HandleId,
    kind: HandleKind,
    status: Status,
    namespace: Namespace,
    area: Area,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct BlockerRow {
    h: HandleId,
    #[serde(flatten)]
    blocker: Blocker,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct ChainRow { depth: usize, start: HandleId, target: HandleId }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct OpenOqRow { q: HandleId }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct PressureRow { q: HandleId, n: usize }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct AreaCountRow { area: Area, n: usize }

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct DiagnosticRow {
    code: DiagnosticCode,
    severity: Severity,
    handle: HandleId,
    file: FilePath,
    line: u32,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct UpstreamWithProvenance {
    h: HandleId,
    anc: HandleId,
    #[serde(rename = "_derivation")]
    chain: Vec<UpstreamStep>,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct AdvancedRow { h: HandleId }

fn mvs1_rows(prog: &AscentProgram) -> Vec<HandleRow> {
    sorted(prog.handle.iter().map(|(id, kind, status, ns, _file, area, _date)| HandleRow {
        id: *id, kind: *kind, status: *status, namespace: *ns, area: *area,
    }))
}

fn mvs1_verify(rows: &[HandleRow]) -> Verdict {
    if rows.len() == HANDLES.len() {
        Verdict::Pass
    } else {
        Verdict::Fail("row count != fixture HANDLES.len()")
    }
}

fn mvs2_rows(prog: &AscentProgram) -> Vec<BlockerRow> {
    sorted(prog.release_blocker.iter().map(|(h, b)| BlockerRow { h: *h, blocker: *b }))
}

fn mvs2_verify(rows: &[BlockerRow]) -> Verdict {
    let broken = rows.iter().any(|r| matches!(r.blocker, Blocker::BrokenRef { .. }));
    let undischarged = rows.iter().any(|r| matches!(r.blocker, Blocker::Undischarged));
    let stale = rows.iter().any(|r| matches!(r.blocker, Blocker::StaleDep { .. }));
    match (broken, undischarged, stale) {
        (true, true, true) => Verdict::Pass,
        (false, _, _) => Verdict::Fail("missing broken_ref clause output"),
        (_, false, _) => Verdict::Fail("missing undischarged clause output"),
        (_, _, false) => Verdict::Fail("missing stale_dep clause output"),
    }
}

fn mvs3_rows(prog: &AscentProgram) -> Vec<ChainRow> {
    sorted(
        prog.supersedes_chain.iter()
            .filter(|(s, _, _)| *s == V17)
            .map(|(s, t, d)| ChainRow { depth: *d, start: *s, target: *t })
    )
}

fn mvs3_verify(rows: &[ChainRow]) -> Verdict {
    let expected = [(1, V16), (2, V15), (3, V14)];
    if rows.len() != expected.len() {
        return Verdict::Fail("supersedes_chain depth count mismatch");
    }
    for (row, (d, t)) in rows.iter().zip(expected) {
        if row.depth != d || row.target != t {
            return Verdict::Fail("supersedes_chain entry mismatch");
        }
    }
    Verdict::Pass
}

fn mvs4_rows(prog: &AscentProgram) -> Vec<OpenOqRow> {
    sorted(prog.open_oq.iter().map(|(q,)| OpenOqRow { q: *q }))
}

fn mvs4_verify(rows: &[OpenOqRow]) -> Verdict {
    // Oracle: fixture has OQ-22/23/60/77/88 open; OQ-99 resolved → terminal.
    if rows.len() != 5 {
        return Verdict::Fail("open_oq count != 5");
    }
    if rows.iter().any(|r| r.q == OQ_99) {
        return Verdict::Fail("OQ-99 (resolved) leaked into open_oq");
    }
    Verdict::Pass
}

fn mvs5a_rows(prog: &AscentProgram) -> Vec<PressureRow> {
    sorted(prog.oq_pressure.iter().map(|(q, n)| PressureRow { q: *q, n: *n }))
}

fn mvs5a_verify(rows: &[PressureRow]) -> Verdict {
    // Oracle: OQ-22 is depended on by v17 (settled) and jit-spec (draft);
    // only v17 counts → pressure 1.
    match rows.iter().find(|r| r.q == OQ_22) {
        Some(r) if r.n == 1 => Verdict::Pass,
        Some(_) => Verdict::Fail("OQ-22 pressure != 1"),
        None    => Verdict::Fail("OQ-22 missing from oq_pressure"),
    }
}

fn mvs5b_rows(prog: &AscentProgram) -> Vec<AreaCountRow> {
    sorted(prog.oq_per_area.iter().map(|(area, n)| AreaCountRow { area: *area, n: *n }))
}

fn mvs5b_verify(rows: &[AreaCountRow]) -> Verdict {
    // Oracle: formal-model=2 (OQ-22, OQ-23), compiler=2 (OQ-60, OQ-88),
    // research-log=1 (OQ-77).
    let count = |area: &str| rows.iter().find(|r| r.area.0 == area).map(|r| r.n);
    if count("formal-model") == Some(2)
        && count("compiler") == Some(2)
        && count("research-log") == Some(1)
    {
        Verdict::Pass
    } else {
        Verdict::Fail("oq_per_area area-count mismatch")
    }
}

/// Build a `(from, to) → step` index over `upstream_via` once per call;
/// repeated chain walks reuse it rather than rescanning. Without this,
/// `mvs8_upstream_rows` is `O(|upstream|² · chain_depth)`.
fn upstream_chain_map(prog: &AscentProgram) -> HashMap<(HandleId, HandleId), UpstreamStep> {
    let mut m = HashMap::with_capacity(prog.upstream_via.len());
    for &(h, anc, step) in &prog.upstream_via {
        m.entry((h, anc)).or_insert(step);
    }
    m
}

fn reconstruct_upstream_chain(
    map: &HashMap<(HandleId, HandleId), UpstreamStep>,
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

fn mvs8_upstream_rows(prog: &AscentProgram) -> Vec<UpstreamWithProvenance> {
    let map = upstream_chain_map(prog);
    sorted(prog.upstream.iter().map(|(h, anc)| UpstreamWithProvenance {
        h: *h,
        anc: *anc,
        chain: reconstruct_upstream_chain(&map, *h, *anc),
    }))
}

fn mvs8_upstream_verify(rows: &[UpstreamWithProvenance]) -> Verdict {
    // Oracle: exec → jit-spec → OQ-22 reconstructs as [Transitive(jit-spec), Direct].
    match rows.iter().find(|r| r.h == EXEC && r.anc == OQ_22) {
        None => Verdict::Fail("upstream(exec, OQ-22) missing"),
        Some(r) if r.chain == [UpstreamStep::Transitive { mid: JIT_SPEC }, UpstreamStep::Direct]
            => Verdict::Pass,
        Some(_) => Verdict::Fail("upstream(exec, OQ-22) chain != [Transitive(jit-spec), Direct]"),
    }
}

fn mvs6_rows(prog: &AscentProgram) -> Vec<AdvancedRow> {
    sorted(prog.recently_advanced.iter().map(|(h,)| AdvancedRow { h: *h }))
}

fn mvs6_verify(rows: &[AdvancedRow]) -> Verdict {
    // Oracle: JIT_SPEC advanced Raw → Draft; V17 advanced Current →
    // Authoritative. EXEC and OQ_22 didn't change. JIT_STALE became
    // terminal (Draft → Superseded) — no pipeline position now, so it
    // cannot fire recently_advanced by the rule's definition.
    let has = |id: HandleId| rows.iter().any(|r| r.h == id);
    if !has(JIT_SPEC)  { return Verdict::Fail("JIT_SPEC missing — Raw → Draft should advance") }
    if !has(V17)       { return Verdict::Fail("V17 missing — Current → Authoritative should advance") }
    if has(EXEC)       { return Verdict::Fail("EXEC leaked — Current → Current is not advancing") }
    if rows.len() != 2 { return Verdict::Fail("recently_advanced size != 2") }
    Verdict::Pass
}

fn diagnostics_derived(prog: &AscentProgram) -> Vec<DiagnosticRow> {
    sorted(prog.diagnostic.iter().map(|(code, sev, h, file, line)| DiagnosticRow {
        code: *code, severity: *sev, handle: *h, file: *file, line: *line,
    }))
}

// ---------------------------------------------------------------------------
// main — run program, emit NDJSON, exit nonzero on any MVS failure
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let prog = load_fixture();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut all_pass = true;
    let mut record = |pass: bool| { all_pass &= pass; };

    let rows = mvs1_rows(&prog);
    let v = mvs1_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-1",
        "? *handle{id, kind, status, namespace, area}.",
        &rows, v, None)?;

    let rows = mvs2_rows(&prog);
    let v = mvs2_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-2",
        "release_blocker(h, Blocker::BrokenRef{file, line}) := diagnostic(E001, ...).\n\
         release_blocker(h, Blocker::Undischarged)          := diagnostic(E002, ...).\n\
         release_blocker(h, Blocker::StaleDep{target: t})   := edge(h, t, depends_on, _, _), active(h), terminal(t).",
        &rows, v, Some("provenance lives in the head value; serialized via #[serde(flatten)]"))?;

    let rows = mvs3_rows(&prog);
    let v = mvs3_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-3",
        "supersedes_chain(s, t, 1)     := edge(s, t, supersedes, _, _).\n\
         supersedes_chain(s, t, d + 1) := edge(s, m, supersedes, _, _), supersedes_chain(m, t, d).",
        &rows, v, None)?;

    let rows = mvs4_rows(&prog);
    let v = mvs4_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-4",
        "open_oq(q) := *handle{id: q, kind: Label, namespace: \"OQ\"}, not terminal(q).",
        &rows, v, None)?;

    let rows = mvs5a_rows(&prog);
    let v = mvs5a_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-5a",
        "oq_pressure(q, n) := open_oq(q), n = Count{ x : downstream_settled(q, x) }.",
        &rows, v, None)?;

    let rows = mvs5b_rows(&prog);
    let v = mvs5b_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-5b",
        "oq_per_area(area, n) := n = Count{ q : *handle{kind: Label, namespace: \"OQ\", area}, not terminal(q) }.",
        &rows, v, None)?;

    let rows = mvs6_rows(&prog);
    let v = mvs6_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-6",
        "recently_advanced(h) := \
         handle(h, _, current, _, _, _, _), \
         at(\"snapshot:last\") { *handle{id: h, status: prior} }, \
         pipeline_position_for(current, n_now), \
         pipeline_position_for(prior, n_then), \
         n_now > n_then.",
        &rows, v, Some("at() block expressed as join on snapshot_handle relation"))?;

    let rows = mvs8_upstream_rows(&prog);
    let v = mvs8_upstream_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-8",
        "upstream_via(h, anc, UpstreamStep) — recursive companion.\n\
         host-side reconstruct_upstream_chain walks the companion to produce\n\
         an ordered chain ending in Direct. release_blocker carries its own\n\
         provenance directly in the head's Blocker value (see MVS-2 output).",
        &rows, v, Some("recursive provenance reconstruction; non-recursive provenance lives in the head"))?;

    let diag = diagnostics_derived(&prog);
    emit(&mut out, "DERIVED-DIAGNOSTICS",
        "? diagnostic(code, severity, handle, file, line).",
        &diag, Verdict::Pass, Some("emitted by E001/E002/W001 rules"))?;

    out.flush()?;
    if all_pass {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spike_runner::fixture::ids::{JIT_STALE, OQ_77, OQ_88};
    use std::sync::OnceLock;

    // Shared fixpoint — every test reads, none mutate, ascent's fixpoint
    // amortizes across the whole test binary.
    fn prog() -> &'static AscentProgram {
        static P: OnceLock<AscentProgram> = OnceLock::new();
        P.get_or_init(load_fixture)
    }

    #[test] fn mvs1_loads_all_fixture_handles() {
        assert!(mvs1_verify(&mvs1_rows(prog())).is_pass());
    }
    #[test] fn mvs2_all_three_release_blocker_clauses_fire() {
        assert!(mvs2_verify(&mvs2_rows(prog())).is_pass());
    }
    #[test] fn mvs3_supersedes_chain_recurses_to_depth_3() {
        assert!(mvs3_verify(&mvs3_rows(prog())).is_pass());
    }
    #[test] fn mvs4_stratified_negation_excludes_terminal_oqs() {
        assert!(mvs4_verify(&mvs4_rows(prog())).is_pass());
    }
    #[test] fn mvs5a_oq_pressure_aggregation_groups_by_handle() {
        assert!(mvs5a_verify(&mvs5a_rows(prog())).is_pass());
    }
    #[test] fn mvs5b_oq_per_area_aggregation_groups_by_area() {
        assert!(mvs5b_verify(&mvs5b_rows(prog())).is_pass());
    }
    #[test] fn mvs6_recently_advanced_picks_up_pipeline_forward_moves() {
        assert!(mvs6_verify(&mvs6_rows(prog())).is_pass());
    }
    #[test] fn mvs6_terminal_transitions_do_not_count_as_advancing() {
        // JIT_STALE went Draft → Superseded. Superseded has no pipeline
        // position, so it can't appear in recently_advanced.
        let rows = mvs6_rows(prog());
        assert!(!rows.iter().any(|r| r.h == JIT_STALE),
            "JIT_STALE became terminal — must not appear in recently_advanced");
    }

    #[test] fn derived_e001_fires_for_pending_edge_with_missing_target() {
        assert!(diagnostics_derived(prog()).iter().any(|d| d.code == DiagnosticCode::E001));
    }

    #[test] fn derived_e002_fires_for_undischarged_obligation() {
        let diag = diagnostics_derived(prog());
        assert!(diag.iter().any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_88));
        assert!(diag.iter().any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_22));
    }

    #[test] fn derived_e002_does_not_fire_for_discharged_obligation() {
        assert!(!diagnostics_derived(prog()).iter()
            .any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_77),
            "OQ-77 is discharged → no E002");
    }

    #[test] fn derived_w001_fires_for_stale_dep() {
        assert!(diagnostics_derived(prog()).iter().any(|d|
            d.code == DiagnosticCode::W001 && d.handle == JIT_SPEC));
    }

    #[test] fn mvs8_upstream_chain_reconstructs_exec_to_oq22() {
        assert!(mvs8_upstream_verify(&mvs8_upstream_rows(prog())).is_pass());
    }

    #[test] fn mvs8_direct_upstream_facts_reconstruct_as_single_direct_step() {
        // Oracle: jit-spec → OQ-22 is a one-hop edge; chain = [Direct].
        let rows = mvs8_upstream_rows(prog());
        let direct = rows.iter().find(|r| r.h == JIT_SPEC && r.anc == OQ_22)
            .expect("upstream(jit-spec, OQ-22) missing");
        assert_eq!(direct.chain, vec![UpstreamStep::Direct]);
    }

    #[test] fn mvs2_blocker_carries_broken_ref_bindings() {
        let rows = mvs2_rows(prog());
        // Oracle: E001 fires from pending_edge at jit-spec.md:51.
        let broken = rows.iter().find(|r| matches!(r.blocker, Blocker::BrokenRef { .. }))
            .expect("no broken_ref blocker");
        assert!(matches!(broken.blocker,
            Blocker::BrokenRef { file: FilePath("compiler/jit-spec.md"), line: 51 }));
    }

    #[test] fn upstream_via_covers_every_upstream_fact() {
        // Companion-relation invariant: every (h, anc) in upstream/2 must
        // have at least one matching (h, anc, _) in upstream_via/3.
        // Catches the day someone adds an upstream rule without a matching
        // upstream_via clause.
        let p = prog();
        let via: std::collections::HashSet<(HandleId, HandleId)> =
            p.upstream_via.iter().map(|(h, anc, _)| (*h, *anc)).collect();
        for &(h, anc) in &p.upstream {
            assert!(via.contains(&(h, anc)),
                "upstream({h:?}, {anc:?}) has no upstream_via row");
        }
    }

    #[test] fn mvs2_blocker_carries_stale_dep_target() {
        let rows = mvs2_rows(prog());
        let stale = rows.iter().find(|r| matches!(r.blocker, Blocker::StaleDep { .. }))
            .expect("no stale_dep blocker");
        assert!(matches!(stale.blocker, Blocker::StaleDep { target: t } if t == JIT_STALE));
    }
}
