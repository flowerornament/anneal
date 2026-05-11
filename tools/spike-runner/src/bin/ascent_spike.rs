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
// Relax only the lints the macro trips; our hand-written code stays
// under the workspace policy.
#![allow(
    clippy::no_effect_underscore_binding,
    clippy::explicit_auto_deref,
    clippy::redundant_pub_crate,
    clippy::needless_pass_by_value,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
)]

use ascent::ascent;
use ascent::aggregators::count;
use serde::Serialize;
use spike_runner::capability::{emit, Verdict};
use spike_runner::types::{
    Area, DiagnosticCode, EdgeKind, FilePath, HandleId, HandleKind, IsoDate, Namespace, Severity,
    Status,
};
use spike_runner::{EDGES, HANDLES, LINEAR_NAMESPACES, PENDING_EDGES};
use std::io::{self, BufWriter, Write};

// ---------------------------------------------------------------------------
// Provenance enums — capture which rule clause fired plus its join bindings.
// Stored inside `_via` companion relations alongside the head's key columns.
// ---------------------------------------------------------------------------

/// Which clause of `release_blocker/2` fired, plus the bindings unique to
/// that clause. Captures everything a reader needs to reconstruct *why*
/// a handle is a blocker without re-running the query.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
enum BlockerRule {
    /// `E001` fired at this `file`:`line`.
    BrokenRef { file: FilePath, line: u32 },
    /// `E002` fired (no useful join bindings beyond the head).
    Undischarged,
    /// Active handle's `depends_on` edge lands on this terminal `target`.
    StaleDep { target: HandleId },
}

/// One step in an `upstream/2` derivation chain. A `Direct` step means
/// the head fact comes from a single edge; a `Transitive` step means a
/// `mid` handle was reached via `depends_on` and the recursion produced
/// the rest of the path.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
enum UpstreamStep {
    Direct,
    Transitive { mid: HandleId },
}

// ---------------------------------------------------------------------------
// Ascent program — typed tuples mean swapped fields fail at compile time
// ---------------------------------------------------------------------------

ascent! {
    pub struct AscentProgram;

    // Stored relations populated from the fixture.
    relation handle(HandleId, HandleKind, Status, Namespace, FilePath, Area, Option<IsoDate>);
    relation edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation pending_edge(HandleId, HandleId, EdgeKind, FilePath, u32);
    relation linear_namespace(Namespace);

    // ---- Engine-derived primitives (§8) ----
    relation terminal(HandleId);
    terminal(h) <-- handle(h, _, s, _, _, _, _), if s.is_terminal();

    relation active(HandleId);
    active(h) <-- handle(h, _, s, _, _, _, _), if s.is_active();

    relation settled(HandleId);
    settled(h) <-- handle(h, _, s, _, _, _, _), if s.is_settled();

    // upstream(h, anc) — transitive depends_on closure (MVS-3)
    relation upstream(HandleId, HandleId);
    upstream(h, anc) <-- edge(h, anc, EdgeKind::DependsOn, _, _);
    upstream(h, anc) <-- edge(h, mid, EdgeKind::DependsOn, _, _), upstream(mid, anc);

    // Supersession chain with explicit depth (MVS-3)
    relation supersedes_chain(HandleId, HandleId, usize);
    supersedes_chain(s, t, 1) <-- edge(s, t, EdgeKind::Supersedes, _, _);
    supersedes_chain(s, t, d + 1) <--
        edge(s, mid, EdgeKind::Supersedes, _, _),
        supersedes_chain(mid, t, d);

    // ---- Obligation lifecycle ----
    relation obligation(HandleId);
    obligation(h) <--
        handle(h, HandleKind::Label, _, ns, _, _, _),
        linear_namespace(ns);

    relation discharged(HandleId);
    discharged(h) <-- edge(_, h, EdgeKind::Discharges, _, _);

    relation undischarged(HandleId);
    undischarged(h) <-- obligation(h), !discharged(h), !terminal(h);

    // ---- Diagnostic-deriving rules (anneal's checks moved to the rule layer) ----
    relation diagnostic(DiagnosticCode, Severity, HandleId, FilePath, u32);

    // E001: pending edge whose target has no corresponding handle.
    diagnostic(DiagnosticCode::E001, Severity::Error, src, file, line) <--
        pending_edge(src, target, _, file, line),
        !handle(target, _, _, _, _, _, _);

    // E002: open obligation in a linear namespace with no discharges-in edge.
    diagnostic(DiagnosticCode::E002, Severity::Error, h, file, line) <--
        undischarged(h),
        handle(h, _, _, _, file, _, _),
        let line = 1u32;

    // W001: active handle depends on terminal handle.
    diagnostic(DiagnosticCode::W001, Severity::Warning, src, file, line) <--
        edge(src, target, EdgeKind::DependsOn, file, line),
        active(src),
        terminal(target);

    // ---- Project predicates (MVS-2: multi-clause rule union) ----
    relation release_blocker(HandleId, &'static str);
    release_blocker(h, "broken_ref")    <-- diagnostic(DiagnosticCode::E001, _, h, _, _);
    release_blocker(h, "undischarged")  <-- diagnostic(DiagnosticCode::E002, _, h, _, _);
    release_blocker(h, "stale_dep")     <--
        edge(h, t, EdgeKind::DependsOn, _, _),
        active(h),
        terminal(t);

    // ---- Provenance instrumentation (MVS-8) ----
    // ascent doesn't expose derivation trails natively. The companion-
    // relation pattern: each rule head gets a parallel `_via` relation
    // whose extra columns capture which clause fired and the join
    // bindings. Reconstruction walks these companions on the host side.
    relation release_blocker_via(HandleId, &'static str, BlockerRule);
    release_blocker_via(h, "broken_ref",   BlockerRule::BrokenRef { file: *file, line: *line })
        <-- diagnostic(DiagnosticCode::E001, _, h, file, line);
    release_blocker_via(h, "undischarged", BlockerRule::Undischarged)
        <-- diagnostic(DiagnosticCode::E002, _, h, _, _);
    release_blocker_via(h, "stale_dep",    BlockerRule::StaleDep { target: *t })
        <-- edge(h, t, EdgeKind::DependsOn, _, _),
            active(h),
            terminal(t);

    // upstream_via captures recursion-step provenance: every fact in
    // `upstream` traces to either a base edge or a (mid, recurse) join.
    relation upstream_via(HandleId, HandleId, UpstreamStep);
    upstream_via(h, anc, UpstreamStep::Direct)
        <-- edge(h, anc, EdgeKind::DependsOn, _, _);
    upstream_via(h, anc, UpstreamStep::Transitive { mid: *mid })
        <-- edge(h, mid, EdgeKind::DependsOn, _, _), upstream(mid, anc);

    // Open OQs (MVS-4 stratified negation visible at the rule level).
    relation open_oq(HandleId);
    open_oq(q) <-- handle(q, HandleKind::Label, _, Namespace("OQ"), _, _, _), !terminal(q);

    // Downstream settled pressure on each open OQ (MVS-5 setup).
    relation downstream_settled(HandleId, HandleId);
    downstream_settled(q, x) <--
        open_oq(q),
        edge(x, q, EdgeKind::DependsOn, _, _),
        settled(x);

    relation oq_pressure(HandleId, usize);
    oq_pressure(q, n) <--
        open_oq(q),
        agg n = count() in downstream_settled(q, _);

    // Per-area open OQ counts (MVS-5 grouping by Area).
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
    prog.run();
    prog
}

// ---------------------------------------------------------------------------
// Row shapes — one struct per capability, derived `Serialize + Ord`
// ---------------------------------------------------------------------------

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct HandleRow {
    id: HandleId,
    kind: HandleKind,
    status: Status,
    namespace: Namespace,
    area: Area,
}

#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct BlockerRow { h: HandleId, why: &'static str }

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

/// MVS-8 row: a `release_blocker` fact with its `_derivation`.
#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct BlockerWithProvenance {
    h: HandleId,
    why: &'static str,
    #[serde(rename = "_derivation")]
    derivation: BlockerRule,
}

/// MVS-8 row: an `upstream` fact with a reconstructed derivation chain.
#[derive(Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct UpstreamWithProvenance {
    h: HandleId,
    anc: HandleId,
    #[serde(rename = "_derivation")]
    chain: Vec<UpstreamStep>,
}

// ---------------------------------------------------------------------------
// Per-MVS verify functions — pure oracles, usable from tests and the binary
// ---------------------------------------------------------------------------

fn mvs1_rows(prog: &AscentProgram) -> Vec<HandleRow> {
    let mut rows: Vec<HandleRow> = prog.handle.iter()
        .map(|(id, kind, status, ns, _file, area, _date)| HandleRow {
            id: *id, kind: *kind, status: *status, namespace: *ns, area: *area,
        })
        .collect();
    rows.sort();
    rows
}

fn mvs1_verify(rows: &[HandleRow]) -> Verdict {
    if rows.len() == HANDLES.len() {
        Verdict::Pass
    } else {
        Verdict::Fail("row count != fixture HANDLES.len()")
    }
}

fn mvs2_rows(prog: &AscentProgram) -> Vec<BlockerRow> {
    let mut rows: Vec<BlockerRow> = prog.release_blocker.iter()
        .map(|(h, why)| BlockerRow { h: *h, why: *why })
        .collect();
    rows.sort();
    rows
}

fn mvs2_verify(rows: &[BlockerRow]) -> Verdict {
    // All three clauses should fire:
    let has_broken      = rows.iter().any(|r| r.why == "broken_ref");
    let has_undischarged = rows.iter().any(|r| r.why == "undischarged");
    let has_stale       = rows.iter().any(|r| r.why == "stale_dep");
    match (has_broken, has_undischarged, has_stale) {
        (true, true, true) => Verdict::Pass,
        (false, _, _) => Verdict::Fail("missing broken_ref clause output"),
        (_, false, _) => Verdict::Fail("missing undischarged clause output"),
        (_, _, false) => Verdict::Fail("missing stale_dep clause output"),
    }
}

fn mvs3_rows(prog: &AscentProgram) -> Vec<ChainRow> {
    let v17 = HandleId("formal-model/v17.md");
    let mut rows: Vec<ChainRow> = prog.supersedes_chain.iter()
        .filter(|(s, _, _)| *s == v17)
        .map(|(s, t, d)| ChainRow { depth: *d, start: *s, target: *t })
        .collect();
    rows.sort();
    rows
}

fn mvs3_verify(rows: &[ChainRow]) -> Verdict {
    let expected = [
        (1, "formal-model/v16.md"),
        (2, "formal-model/v15.md"),
        (3, "formal-model/v14.md"),
    ];
    if rows.len() != expected.len() {
        return Verdict::Fail("supersedes_chain depth count mismatch");
    }
    for (row, (d, t)) in rows.iter().zip(expected) {
        if row.depth != d || row.target.0 != t {
            return Verdict::Fail("supersedes_chain entry mismatch");
        }
    }
    Verdict::Pass
}

fn mvs4_rows(prog: &AscentProgram) -> Vec<OpenOqRow> {
    let mut rows: Vec<OpenOqRow> = prog.open_oq.iter()
        .map(|(q,)| OpenOqRow { q: *q })
        .collect();
    rows.sort();
    rows
}

fn mvs4_verify(rows: &[OpenOqRow]) -> Verdict {
    // Fixture: OQ-22, OQ-23, OQ-60, OQ-77, OQ-88 open; OQ-99 resolved (terminal).
    let expected = ["OQ-22", "OQ-23", "OQ-60", "OQ-77", "OQ-88"];
    if rows.len() != expected.len() {
        return Verdict::Fail("open_oq count != expected");
    }
    if rows.iter().any(|r| r.q.0 == "OQ-99") {
        return Verdict::Fail("OQ-99 (resolved) leaked into open_oq");
    }
    Verdict::Pass
}

fn mvs5a_rows(prog: &AscentProgram) -> Vec<PressureRow> {
    let mut rows: Vec<PressureRow> = prog.oq_pressure.iter()
        .map(|(q, n)| PressureRow { q: *q, n: *n })
        .collect();
    rows.sort();
    rows
}

fn mvs5a_verify(rows: &[PressureRow]) -> Verdict {
    // OQ-22 has v17 (authoritative=settled) and jit-spec (draft, not settled)
    // depending on it → pressure 1.
    let oq22 = rows.iter().find(|r| r.q.0 == "OQ-22");
    match oq22 {
        Some(r) if r.n == 1 => Verdict::Pass,
        Some(_) => Verdict::Fail("OQ-22 pressure != 1"),
        None    => Verdict::Fail("OQ-22 missing from oq_pressure"),
    }
}

fn mvs5b_rows(prog: &AscentProgram) -> Vec<AreaCountRow> {
    let mut rows: Vec<AreaCountRow> = prog.oq_per_area.iter()
        .map(|(area, n)| AreaCountRow { area: *area, n: *n })
        .collect();
    rows.sort();
    rows
}

fn mvs5b_verify(rows: &[AreaCountRow]) -> Verdict {
    // formal-model: OQ-22, OQ-23 = 2; compiler: OQ-60, OQ-88 = 2; research-log: OQ-77 = 1
    let counts: std::collections::BTreeMap<&str, usize> = rows.iter()
        .map(|r| (r.area.0, r.n))
        .collect();
    let mut errs = vec![];
    if counts.get("formal-model").copied() != Some(2) { errs.push("formal-model"); }
    if counts.get("compiler").copied() != Some(2) { errs.push("compiler"); }
    if counts.get("research-log").copied() != Some(1) { errs.push("research-log"); }
    if errs.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail("oq_per_area area-count mismatch")
    }
}

// MVS-8 — provenance via companion relations
fn mvs8_blocker_rows(prog: &AscentProgram) -> Vec<BlockerWithProvenance> {
    let mut rows: Vec<BlockerWithProvenance> = prog.release_blocker_via.iter()
        .map(|(h, why, rule)| BlockerWithProvenance {
            h: *h, why: *why, derivation: *rule,
        })
        .collect();
    rows.sort();
    rows
}

fn mvs8_blocker_verify(rows: &[BlockerWithProvenance]) -> Verdict {
    let has_broken      = rows.iter().any(|r| matches!(r.derivation, BlockerRule::BrokenRef { .. }));
    let has_undischarged = rows.iter().any(|r| matches!(r.derivation, BlockerRule::Undischarged));
    let has_stale       = rows.iter().any(|r| matches!(r.derivation, BlockerRule::StaleDep { .. }));
    match (has_broken, has_undischarged, has_stale) {
        (true, true, true) => Verdict::Pass,
        _ => Verdict::Fail("companion relation did not surface all three rule clauses"),
    }
}

/// Walk `upstream_via` to reconstruct the rule chain from `h` to `anc`.
/// Returns the steps in order; the last entry is always `Direct`.
fn reconstruct_upstream_chain(
    prog: &AscentProgram,
    h: HandleId,
    anc: HandleId,
) -> Vec<UpstreamStep> {
    let mut chain = Vec::new();
    let mut current = h;
    while let Some(&(_, _, step)) = prog.upstream_via.iter()
        .find(|(start, end, _)| *start == current && *end == anc)
    {
        chain.push(step);
        match step {
            UpstreamStep::Direct => break,
            UpstreamStep::Transitive { mid } => current = mid,
        }
    }
    chain
}

fn mvs8_upstream_rows(prog: &AscentProgram) -> Vec<UpstreamWithProvenance> {
    let mut rows: Vec<UpstreamWithProvenance> = prog.upstream.iter()
        .map(|(h, anc)| UpstreamWithProvenance {
            h: *h,
            anc: *anc,
            chain: reconstruct_upstream_chain(prog, *h, *anc),
        })
        .collect();
    rows.sort();
    rows
}

fn mvs8_upstream_verify(rows: &[UpstreamWithProvenance]) -> Verdict {
    // exec → jit-spec → OQ-22 should reconstruct as Transitive(jit-spec) then Direct.
    let exec = HandleId("compiler/exec.md");
    let oq22 = HandleId("OQ-22");
    let jit  = HandleId("compiler/jit-spec.md");
    let exec_oq22 = rows.iter().find(|r| r.h == exec && r.anc == oq22);
    match exec_oq22 {
        None => Verdict::Fail("upstream(exec, OQ-22) missing"),
        Some(r) if r.chain == [UpstreamStep::Transitive { mid: jit }, UpstreamStep::Direct] => Verdict::Pass,
        Some(_) => Verdict::Fail("upstream(exec, OQ-22) chain != [Transitive(jit-spec), Direct]"),
    }
}

fn diagnostics_derived(prog: &AscentProgram) -> Vec<DiagnosticRow> {
    let mut rows: Vec<DiagnosticRow> = prog.diagnostic.iter()
        .map(|(code, sev, h, file, line)| DiagnosticRow {
            code: *code, severity: *sev, handle: *h, file: *file, line: *line,
        })
        .collect();
    rows.sort();
    rows
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
        "release_blocker(h, \"broken_ref\")   := diagnostic(E001, ...).\n\
         release_blocker(h, \"undischarged\") := diagnostic(E002, ...).\n\
         release_blocker(h, \"stale_dep\")    := edge(h, t, depends_on, _, _), active(h), terminal(t).",
        &rows, v, Some("all three clauses now exercised (fixture grew)"))?;

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

    let rows = mvs8_blocker_rows(&prog);
    let v = mvs8_blocker_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-8a",
        "release_blocker_via(h, why, BlockerRule) — companion relation per clause.\n\
         output: each release_blocker row carries `_derivation` describing which\n\
         rule clause fired and the join bindings (FilePath/line for broken_ref,\n\
         target HandleId for stale_dep).",
        &rows, v, Some("provenance via companion relation; ascent has no built-in trail"))?;

    let rows = mvs8_upstream_rows(&prog);
    let v = mvs8_upstream_verify(&rows);
    record(v.is_pass());
    emit(&mut out, "MVS-8b",
        "upstream_via(h, anc, UpstreamStep) — recursive companion.\n\
         host-side reconstruct_upstream_chain walks the companion to produce\n\
         an ordered chain ending in Direct.",
        &rows, v, Some("recursive provenance reconstruction"))?;

    // Bonus: emit the derived diagnostics under their own banner so a
    // reader can see what the rule-layer checks produced.
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

// ---------------------------------------------------------------------------
// Tests — each MVS capability has a #[test] proving its expectations.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn prog() -> AscentProgram { load_fixture() }

    #[test] fn mvs1_loads_all_fixture_handles() {
        assert!(mvs1_verify(&mvs1_rows(&prog())).is_pass());
    }
    #[test] fn mvs2_all_three_release_blocker_clauses_fire() {
        assert!(mvs2_verify(&mvs2_rows(&prog())).is_pass());
    }
    #[test] fn mvs3_supersedes_chain_recurses_to_depth_3() {
        assert!(mvs3_verify(&mvs3_rows(&prog())).is_pass());
    }
    #[test] fn mvs4_stratified_negation_excludes_terminal_oqs() {
        assert!(mvs4_verify(&mvs4_rows(&prog())).is_pass());
    }
    #[test] fn mvs5a_oq_pressure_aggregation_groups_by_handle() {
        assert!(mvs5a_verify(&mvs5a_rows(&prog())).is_pass());
    }
    #[test] fn mvs5b_oq_per_area_aggregation_groups_by_area() {
        assert!(mvs5b_verify(&mvs5b_rows(&prog())).is_pass());
    }

    #[test] fn derived_e001_fires_for_pending_edge_with_missing_target() {
        let diag = diagnostics_derived(&prog());
        assert!(diag.iter().any(|d| d.code == DiagnosticCode::E001),
            "expected at least one E001 from pending_edge → missing handle");
    }

    #[test] fn derived_e002_fires_for_undischarged_obligation() {
        let diag = diagnostics_derived(&prog());
        let oq88_e002 = diag.iter().any(|d| d.code == DiagnosticCode::E002 && d.handle.0 == "OQ-88");
        let oq22_e002 = diag.iter().any(|d| d.code == DiagnosticCode::E002 && d.handle.0 == "OQ-22");
        assert!(oq88_e002, "OQ-88 should produce E002 (undischarged)");
        assert!(oq22_e002, "OQ-22 should produce E002 (undischarged)");
    }

    #[test] fn derived_e002_does_not_fire_for_discharged_obligation() {
        let diag = diagnostics_derived(&prog());
        assert!(!diag.iter().any(|d| d.code == DiagnosticCode::E002 && d.handle.0 == "OQ-77"),
            "OQ-77 is discharged → no E002");
    }

    #[test] fn mvs8_blocker_provenance_surfaces_all_three_clauses() {
        assert!(mvs8_blocker_verify(&mvs8_blocker_rows(&prog())).is_pass());
    }

    #[test] fn mvs8_upstream_chain_reconstructs_exec_to_oq22() {
        assert!(mvs8_upstream_verify(&mvs8_upstream_rows(&prog())).is_pass());
    }

    #[test] fn mvs8_direct_upstream_facts_reconstruct_as_single_direct_step() {
        // jit-spec → OQ-22 is a one-hop edge; the chain should be [Direct].
        let rows = mvs8_upstream_rows(&prog());
        let direct = rows.iter().find(|r|
            r.h == HandleId("compiler/jit-spec.md") && r.anc == HandleId("OQ-22"));
        let direct = direct.expect("upstream(jit-spec, OQ-22) missing");
        assert_eq!(direct.chain, vec![UpstreamStep::Direct]);
    }

    #[test] fn mvs8_blocker_rule_records_file_and_line_for_broken_ref() {
        let rows = mvs8_blocker_rows(&prog());
        let broken = rows.iter().find(|r| r.why == "broken_ref")
            .expect("missing broken_ref provenance row");
        // E001 fires on the pending_edge file:line, which is jit-spec.md:51.
        assert!(matches!(broken.derivation,
            BlockerRule::BrokenRef { file: FilePath("compiler/jit-spec.md"), line: 51 }),
            "expected BrokenRef(jit-spec.md, 51), got {:?}", broken.derivation);
    }

    #[test] fn mvs8_blocker_rule_records_terminal_target_for_stale_dep() {
        let rows = mvs8_blocker_rows(&prog());
        let stale = rows.iter().find(|r| r.why == "stale_dep")
            .expect("missing stale_dep provenance row");
        assert!(matches!(stale.derivation,
            BlockerRule::StaleDep { target: HandleId("compiler/jit-stale.md") }),
            "expected StaleDep(jit-stale.md), got {:?}", stale.derivation);
    }

    #[test] fn derived_w001_fires_for_stale_dep() {
        let diag = diagnostics_derived(&prog());
        let stale = diag.iter().any(|d|
            d.code == DiagnosticCode::W001
            && d.handle.0 == "compiler/jit-spec.md");
        assert!(stale, "W001 should fire for jit-spec → jit-stale (terminal)");
    }
}
