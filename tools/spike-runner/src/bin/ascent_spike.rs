//! `ascent_spike` — MVS capabilities exercised against the hand-coded
//! [`spike_runner::fixture`]. The fixture-verdict oracles encode the
//! expected derivations for the trickiest cases in real corpora.
//! [`corpus_spike`] is the at-scale companion.

use spike_runner::capability::{emit, Verdict};
use spike_runner::fixture::ids::{EXEC, JIT_SPEC, OQ_22, OQ_99, V14, V15, V16, V17};
use spike_runner::program::{
    diagnostics_derived, mvs1_rows, mvs2_rows, mvs3_rows, mvs4_rows, mvs5a_rows, mvs5b_rows,
    mvs6_rows, mvs8_upstream_rows, push_edges, push_handles, push_linear_namespaces,
    push_pipeline_ordering, AdvancedRow, AreaCountRow, AscentProgram, Blocker, BlockerRow,
    ChainRow, HandleRow, OpenOqRow, PressureRow, UpstreamStep, UpstreamWithProvenance,
};
use spike_runner::types::HandleId;
use spike_runner::{EDGES, HANDLES, LINEAR_NAMESPACES, PENDING_EDGES, SNAPSHOTS};
use std::io::{self, BufWriter, Write};

fn load_fixture() -> AscentProgram {
    let mut prog = AscentProgram::default();
    push_handles(&mut prog, HANDLES);
    push_edges(&mut prog, EDGES);
    push_linear_namespaces(&mut prog, LINEAR_NAMESPACES);
    push_pipeline_ordering(&mut prog);
    prog.pending_edge.reserve(PENDING_EDGES.len());
    for p in PENDING_EDGES {
        prog.pending_edge.push((p.from, p.target, p.kind, p.file, p.line));
    }
    prog.snapshot_handle.reserve(SNAPSHOTS.len());
    for s in SNAPSHOTS {
        prog.snapshot_handle.push((s.id, s.handle, s.status));
    }
    prog.run();
    prog
}

// ---------------------------------------------------------------------------
// Fixture-specific verdicts — encode the expected derivations for the
// hand-coded fixture so the spike harness has a real pass/fail signal.
// ---------------------------------------------------------------------------

fn mvs1_verify(rows: &[HandleRow]) -> Verdict {
    if rows.len() == HANDLES.len() {
        Verdict::Pass
    } else {
        Verdict::Fail("row count != fixture HANDLES.len()")
    }
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

fn mvs5a_verify(rows: &[PressureRow]) -> Verdict {
    // Oracle: OQ-22 is depended on by v17 (settled) and jit-spec (draft);
    // only v17 counts → pressure 1.
    match rows.iter().find(|r| r.q == OQ_22) {
        Some(r) if r.n == 1 => Verdict::Pass,
        Some(_) => Verdict::Fail("OQ-22 pressure != 1"),
        None    => Verdict::Fail("OQ-22 missing from oq_pressure"),
    }
}

fn mvs5b_verify(rows: &[AreaCountRow]) -> Verdict {
    // Oracle: formal-model=2, compiler=2, research-log=1.
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

fn mvs6_verify(rows: &[AdvancedRow]) -> Verdict {
    // Oracle: JIT_SPEC advanced Raw → Draft; V17 advanced Current →
    // Authoritative. EXEC unchanged. JIT_STALE became terminal.
    let has = |id: HandleId| rows.iter().any(|r| r.h == id);
    if !has(JIT_SPEC)  { return Verdict::Fail("JIT_SPEC missing — Raw → Draft should advance") }
    if !has(V17)       { return Verdict::Fail("V17 missing — Current → Authoritative should advance") }
    if has(EXEC)       { return Verdict::Fail("EXEC leaked — Current → Current is not advancing") }
    if rows.len() != 2 { return Verdict::Fail("recently_advanced size != 2") }
    Verdict::Pass
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

    let rows = mvs3_rows(&prog, V17);
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
        "recently_advanced(h) := handle(h, _, current, _, _, _, _), \
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
    if all_pass { Ok(()) } else { std::process::exit(1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spike_runner::fixture::ids::{JIT_STALE, OQ_77, OQ_88};
    use spike_runner::types::{DiagnosticCode, FilePath};
    use std::sync::OnceLock;

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
        assert!(mvs3_verify(&mvs3_rows(prog(), V17)).is_pass());
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
        assert!(!mvs6_rows(prog()).iter().any(|r| r.h == JIT_STALE));
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
            .any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_77));
    }
    #[test] fn derived_w001_fires_for_stale_dep() {
        assert!(diagnostics_derived(prog()).iter().any(|d|
            d.code == DiagnosticCode::W001 && d.handle == JIT_SPEC));
    }

    #[test] fn mvs8_upstream_chain_reconstructs_exec_to_oq22() {
        assert!(mvs8_upstream_verify(&mvs8_upstream_rows(prog())).is_pass());
    }
    #[test] fn mvs8_direct_upstream_facts_reconstruct_as_single_direct_step() {
        let rows = mvs8_upstream_rows(prog());
        let direct = rows.iter().find(|r| r.h == JIT_SPEC && r.anc == OQ_22)
            .expect("upstream(jit-spec, OQ-22) missing");
        assert_eq!(direct.chain, vec![UpstreamStep::Direct]);
    }

    #[test] fn upstream_via_covers_every_upstream_fact() {
        let p = prog();
        let via: std::collections::HashSet<(HandleId, HandleId)> =
            p.upstream_via.iter().map(|(h, anc, _)| (*h, *anc)).collect();
        for &(h, anc) in &p.upstream {
            assert!(via.contains(&(h, anc)),
                "upstream({h:?}, {anc:?}) has no upstream_via row");
        }
    }

    #[test] fn mvs2_blocker_carries_broken_ref_bindings() {
        let rows = mvs2_rows(prog());
        let broken = rows.iter().find(|r| matches!(r.blocker, Blocker::BrokenRef { .. }))
            .expect("no broken_ref blocker");
        assert!(matches!(broken.blocker,
            Blocker::BrokenRef { file: FilePath("compiler/jit-spec.md"), line: 51 }));
    }

    #[test] fn mvs2_blocker_carries_stale_dep_target() {
        let rows = mvs2_rows(prog());
        let stale = rows.iter().find(|r| matches!(r.blocker, Blocker::StaleDep { .. }))
            .expect("no stale_dep blocker");
        assert!(matches!(stale.blocker, Blocker::StaleDep { target: t } if t == JIT_STALE));
    }
}
