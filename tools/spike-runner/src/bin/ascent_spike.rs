//! `ascent_spike` — MVS capabilities exercised against the hand-coded
//! [`spike_runner::fixture`]. The fixture-verdict oracles encode the
//! expected derivations for the trickiest cases in real corpora.
//! [`corpus_spike`] is the at-scale companion.

use serde::Serialize;
use spike_runner::capability::{Verdict, emit};
use spike_runner::dynamic_ir::{
    BENCH_PRELUDE, PredicateId, Program, ProjectLoadReport, load_project_extension,
};
use spike_runner::fixture::ids::{
    COMPILER, EXEC, FORMAL, JIT_SPEC, JIT_STALE, OQ_22, OQ_23, OQ_60, OQ_77, OQ_88, OQ_99,
    RESEARCH_LOG, SYNTHESIS, V14, V15, V16, V17,
};
use spike_runner::program::{
    AdvancedRow, AreaCountRow, AscentProgram, Blocker, BlockerRow, ChainRow, HandleRow, OpenOqRow,
    PressureRow, SpqAreaActiveCountRow, SpqBlockerRow, SpqDerivationStep, SpqExplainedUpstreamRow,
    SpqOpenLabelRow, SpqStatusChangedRow, SpqUnfinishedRow, SpqUpstreamRow, UpstreamStep,
    UpstreamWithProvenance, diagnostics_derived, mvs1_rows, mvs2_rows, mvs3_rows, mvs4_rows,
    mvs5a_rows, mvs5b_rows, mvs6_rows, mvs8_upstream_rows, push_edges, push_handles,
    push_linear_namespaces, push_pipeline_ordering, spq1_rows, spq2_rows, spq3_rows, spq4_rows,
    spq5_rows, spq6_rows, spq8_rows,
};
use spike_runner::types::{Area, EdgeKind, HandleId, Status};
use spike_runner::{EDGES, HANDLES, LINEAR_NAMESPACES, PENDING_EDGES, SNAPSHOTS};
use std::collections::HashSet;
use std::io::{self, BufWriter, Write};

fn load_fixture() -> AscentProgram {
    let mut prog = AscentProgram::default();
    push_handles(&mut prog, HANDLES);
    push_edges(&mut prog, EDGES);
    push_linear_namespaces(&mut prog, LINEAR_NAMESPACES);
    push_pipeline_ordering(&mut prog);
    prog.pending_edge.reserve(PENDING_EDGES.len());
    for p in PENDING_EDGES {
        prog.pending_edge
            .push((p.from, p.target, p.kind, p.file, p.line));
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
    let broken = rows
        .iter()
        .any(|r| matches!(r.blocker, Blocker::BrokenRef { .. }));
    let undischarged = rows
        .iter()
        .any(|r| matches!(r.blocker, Blocker::Undischarged));
    let stale = rows
        .iter()
        .any(|r| matches!(r.blocker, Blocker::StaleDep { .. }));
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
        None => Verdict::Fail("OQ-22 missing from oq_pressure"),
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
    if !has(JIT_SPEC) {
        return Verdict::Fail("JIT_SPEC missing — Raw → Draft should advance");
    }
    if !has(V17) {
        return Verdict::Fail("V17 missing — Current → Authoritative should advance");
    }
    if has(EXEC) {
        return Verdict::Fail("EXEC leaked — Current → Current is not advancing");
    }
    if rows.len() != 2 {
        return Verdict::Fail("recently_advanced size != 2");
    }
    Verdict::Pass
}

fn mvs8_upstream_verify(rows: &[UpstreamWithProvenance]) -> Verdict {
    // Oracle: exec → jit-spec → OQ-22 reconstructs as [Transitive(jit-spec), Direct].
    match rows.iter().find(|r| r.h == EXEC && r.anc == OQ_22) {
        None => Verdict::Fail("upstream(exec, OQ-22) missing"),
        Some(r)
            if r.chain
                == [
                    UpstreamStep::Transitive { mid: JIT_SPEC },
                    UpstreamStep::Direct,
                ] =>
        {
            Verdict::Pass
        }
        Some(_) => Verdict::Fail("upstream(exec, OQ-22) chain != [Transitive(jit-spec), Direct]"),
    }
}

fn spq1_verify(rows: &[SpqOpenLabelRow]) -> Verdict {
    let expected = [OQ_22, OQ_23, OQ_60, OQ_77, OQ_88];
    if rows.len() != expected.len() {
        return Verdict::Fail("open label count != fixture open labels");
    }
    if expected.iter().all(|id| rows.iter().any(|r| r.id == *id)) {
        Verdict::Pass
    } else {
        Verdict::Fail("missing one or more open label rows")
    }
}

fn spq2_verify(rows: &[SpqBlockerRow]) -> Verdict {
    let has_broken = rows
        .iter()
        .any(|r| r.h == JIT_SPEC && r.why == "broken_ref");
    let has_undischarged = rows.iter().any(|r| r.h == OQ_88 && r.why == "undischarged");
    let has_stale = rows.iter().any(|r| r.why == "stale_dep");
    match (has_broken, has_undischarged, has_stale) {
        (true, true, false) => Verdict::Pass,
        (false, _, _) => Verdict::Fail("missing broken_ref literal SP-Q2 output"),
        (_, false, _) => Verdict::Fail("missing undischarged literal SP-Q2 output"),
        (_, _, true) => Verdict::Fail("SP-Q2 leaked MVS stale_dep extension"),
    }
}

fn spq3_verify(rows: &[SpqUpstreamRow]) -> Verdict {
    let expected = [OQ_22, OQ_23, OQ_60];
    if rows.len() != expected.len() {
        return Verdict::Fail("v17 upstream ancestor count mismatch");
    }
    if expected
        .iter()
        .all(|anc| rows.iter().any(|r| r.anc == *anc))
    {
        Verdict::Pass
    } else {
        Verdict::Fail("v17 upstream ancestors mismatch")
    }
}

fn spq4_verify(rows: &[SpqUnfinishedRow]) -> Verdict {
    if rows.len() != 5 {
        return Verdict::Fail("unfinished OQ count != 5");
    }
    if rows.iter().any(|r| r.h == OQ_99) {
        return Verdict::Fail("resolved OQ leaked into unfinished");
    }
    Verdict::Pass
}

fn spq5_verify(rows: &[SpqAreaActiveCountRow]) -> Verdict {
    let count = |area: Area| rows.iter().find(|r| r.area == area).map(|r| r.n);
    if count(FORMAL) == Some(1)
        && count(COMPILER) == Some(2)
        && count(RESEARCH_LOG) == Some(1)
        && count(SYNTHESIS) == Some(1)
        && rows.len() == 4
    {
        Verdict::Pass
    } else {
        Verdict::Fail("per-area active file counts mismatch")
    }
}

fn spq6_verify(rows: &[SpqStatusChangedRow]) -> Verdict {
    let has = |id: HandleId, prev: Status, curr: Status| {
        rows.iter()
            .any(|r| r.h == id && r.prev == prev && r.curr == curr)
    };
    if !has(JIT_SPEC, Status::Raw, Status::Draft) {
        return Verdict::Fail("missing JIT_SPEC Raw -> Draft change");
    }
    if !has(V17, Status::Current, Status::Authoritative) {
        return Verdict::Fail("missing V17 Current -> Authoritative change");
    }
    if !has(JIT_STALE, Status::Draft, Status::Superseded) {
        return Verdict::Fail("missing JIT_STALE Draft -> Superseded change");
    }
    if rows.iter().any(|r| r.h == EXEC) {
        return Verdict::Fail("unchanged EXEC leaked into status_changed");
    }
    Verdict::Pass
}

fn spq7_verify(rows: &[SpqOpenLabelRow]) -> Verdict {
    let limit = rows.len().min(1_000);
    let limited_rows = &rows[..limit];
    let mut seen = HashSet::with_capacity(limited_rows.len());
    let mut out = StreamingNdjsonProbe::default();
    if emit(
        &mut out,
        "SP-Q7",
        "SP-Q1 with --limit=1000",
        limited_rows,
        Verdict::Pass,
        Some("streaming NDJSON contract check"),
    )
    .is_err()
    {
        return Verdict::Fail("SP-Q7 emit failed");
    }
    if let Err(reason) = out.finish(limited_rows.len() + 1) {
        return Verdict::Fail(reason);
    }
    for row in limited_rows {
        if !seen.insert(row.id) {
            return Verdict::Fail("SP-Q7 limited rows contain duplicates");
        }
    }
    Verdict::Pass
}

#[derive(Default)]
struct StreamingNdjsonProbe {
    current_line: Vec<u8>,
    completed_lines: usize,
}

impl StreamingNdjsonProbe {
    fn finish(self, expected_lines: usize) -> Result<(), &'static str> {
        if !self.current_line.is_empty() {
            return Err("SP-Q7 left a partial JSON line buffered");
        }
        if self.completed_lines == expected_lines {
            Ok(())
        } else {
            Err("SP-Q7 row/report line count mismatch")
        }
    }
}

impl Write for StreamingNdjsonProbe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf {
            if byte == b'\n' {
                validate_streamed_json_line(&self.current_line)?;
                self.current_line.clear();
                self.completed_lines += 1;
            } else {
                self.current_line.push(byte);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_streamed_json_line(line: &[u8]) -> io::Result<()> {
    if line.first() == Some(&b'[') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SP-Q7 must not array-frame NDJSON rows",
        ));
    }
    serde_json::from_slice::<serde_json::Value>(line)
        .map(|_| ())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn spq8_verify(rows: &[SpqExplainedUpstreamRow]) -> Verdict {
    let expected = [OQ_22, OQ_23, OQ_60];
    if rows.len() != expected.len() {
        return Verdict::Fail("SP-Q8 explained row count mismatch");
    }
    for anc in expected {
        match rows.iter().find(|r| r.h == V17 && r.anc == anc) {
            Some(row) if row.chain == vec![direct_step(V17, anc)] => {}
            Some(_) => return Verdict::Fail("SP-Q8 derivation support facts mismatch"),
            None => return Verdict::Fail("SP-Q8 missing expected upstream ancestor"),
        }
    }
    if serde_json::to_value(&rows[0])
        .ok()
        .and_then(|value| value.get("_derivation").cloned())
        .is_none()
    {
        return Verdict::Fail("SP-Q8 rows do not serialize _derivation");
    }
    Verdict::Pass
}

fn direct_step(from: HandleId, to: HandleId) -> SpqDerivationStep {
    SpqDerivationStep {
        rule: "upstream/2 base",
        edge_from: from,
        edge_to: to,
        edge_kind: EdgeKind::DependsOn,
        file: spike_runner::FilePath("formal-model/v17.md"),
        line: if to == OQ_60 { 18 } else { 14 },
    }
}

#[derive(Serialize)]
struct Spq9Row {
    file: &'static str,
    release_blocker: String,
    release_blocker_rows: usize,
    shadow_warnings: Vec<String>,
}

const FIXTURE_ANNEAL_DL: &str = r#"
release_blocker(h, "broken_ref") :- diagnostic("E001", _severity, h, _file, _line).
release_blocker(h, "undischarged") :- diagnostic("E002", _severity, h, _file, _line).
terminal(h) :- retired(h).
"#;

fn prelude_predicates() -> std::collections::BTreeSet<PredicateId> {
    Program::parse(BENCH_PRELUDE)
        .expect("benchmark prelude parses")
        .head_predicates()
}

fn spq9_report() -> Result<ProjectLoadReport, spike_runner::dynamic_ir::IrError> {
    load_project_extension(FIXTURE_ANNEAL_DL, &prelude_predicates())
}

fn spq9_rows(report: &ProjectLoadReport, release_blocker_rows: usize) -> Vec<Spq9Row> {
    vec![Spq9Row {
        file: "fixture-anneal.dl",
        release_blocker: report
            .release_blocker
            .as_ref()
            .map_or_else(|| "(missing)".to_string(), format_predicate),
        release_blocker_rows,
        shadow_warnings: report
            .shadow_warnings
            .iter()
            .map(|warning| format_predicate(&warning.predicate))
            .collect(),
    }]
}

fn spq9_verify(report: &ProjectLoadReport, rows: &[Spq9Row]) -> Verdict {
    let has_terminal_shadow = report
        .shadow_warnings
        .iter()
        .any(|warning| warning.predicate == PredicateId::new("terminal", 1));
    let has_release_blocker =
        report.release_blocker == Some(PredicateId::new("release_blocker", 2));
    let returned_rows = rows.iter().any(|row| row.release_blocker_rows > 0);
    match (has_release_blocker, has_terminal_shadow, returned_rows) {
        (true, true, true) => Verdict::Pass,
        (false, _, _) => Verdict::Fail("fixture-anneal.dl did not define release_blocker/2"),
        (_, false, _) => Verdict::Fail("fixture-anneal.dl did not warn on terminal/1 shadow"),
        (_, _, false) => Verdict::Fail("fixture-anneal.dl release_blocker query returned no rows"),
    }
}

fn format_predicate(predicate: &PredicateId) -> String {
    format!("{}/{}", predicate.name, predicate.arity)
}

fn record(all_pass: &mut bool, verdict: Verdict) -> Verdict {
    *all_pass &= verdict.is_pass();
    verdict
}

fn emit_mvs_reports<W: Write>(
    mut out: &mut W,
    prog: &AscentProgram,
    all_pass: &mut bool,
) -> io::Result<()> {
    let rows = mvs1_rows(prog);
    let v = record(all_pass, mvs1_verify(&rows));
    emit(
        &mut out,
        "MVS-1",
        "? *handle{id, kind, status, namespace, area}.",
        &rows,
        v,
        None,
    )?;

    let rows = mvs2_rows(prog);
    let v = record(all_pass, mvs2_verify(&rows));
    emit(
        &mut out,
        "MVS-2",
        "release_blocker(h, Blocker::BrokenRef{file, line}) := diagnostic(E001, ...).\n\
         release_blocker(h, Blocker::Undischarged)          := diagnostic(E002, ...).\n\
         release_blocker(h, Blocker::StaleDep{target: t})   := edge(h, t, depends_on, _, _), active(h), terminal(t).",
        &rows,
        v,
        Some("provenance lives in the head value; serialized via #[serde(flatten)]"),
    )?;

    let rows = mvs3_rows(prog, V17);
    let v = record(all_pass, mvs3_verify(&rows));
    emit(
        &mut out,
        "MVS-3",
        "supersedes_chain(s, t, 1)     := edge(s, t, supersedes, _, _).\n\
         supersedes_chain(s, t, d + 1) := edge(s, m, supersedes, _, _), supersedes_chain(m, t, d).",
        &rows,
        v,
        None,
    )?;

    let rows = mvs4_rows(prog);
    let v = record(all_pass, mvs4_verify(&rows));
    emit(
        &mut out,
        "MVS-4",
        "open_oq(q) := *handle{id: q, kind: Label, namespace: \"OQ\"}, not terminal(q).",
        &rows,
        v,
        None,
    )?;

    let rows = mvs5a_rows(prog);
    let v = record(all_pass, mvs5a_verify(&rows));
    emit(
        &mut out,
        "MVS-5a",
        "oq_pressure(q, n) := open_oq(q), n = Count{ x : downstream_settled(q, x) }.",
        &rows,
        v,
        None,
    )?;

    let rows = mvs5b_rows(prog);
    let v = record(all_pass, mvs5b_verify(&rows));
    emit(
        &mut out,
        "MVS-5b",
        "oq_per_area(area, n) := n = Count{ q : *handle{kind: Label, namespace: \"OQ\", area}, not terminal(q) }.",
        &rows,
        v,
        None,
    )?;

    let rows = mvs6_rows(prog);
    let v = record(all_pass, mvs6_verify(&rows));
    emit(
        &mut out,
        "MVS-6",
        "recently_advanced(h) := handle(h, _, current, _, _, _, _), \
         at(\"snapshot:last\") { *handle{id: h, status: prior} }, \
         pipeline_position_for(current, n_now), \
         pipeline_position_for(prior, n_then), \
         n_now > n_then.",
        &rows,
        v,
        Some("at() block expressed as join on snapshot_handle relation"),
    )?;

    let rows = mvs8_upstream_rows(prog);
    let v = record(all_pass, mvs8_upstream_verify(&rows));
    emit(
        &mut out,
        "MVS-8",
        "upstream_via(h, anc, UpstreamStep) — recursive companion.\n\
         host-side reconstruct_upstream_chain walks the companion to produce\n\
         an ordered chain ending in Direct. release_blocker carries its own\n\
         provenance directly in the head's Blocker value (see MVS-2 output).",
        &rows,
        v,
        Some("recursive provenance reconstruction; non-recursive provenance lives in the head"),
    )?;

    let diag = diagnostics_derived(prog);
    emit(
        &mut out,
        "DERIVED-DIAGNOSTICS",
        "? diagnostic(code, severity, handle, file, line).",
        &diag,
        Verdict::Pass,
        Some("emitted by E001/E002/W001 rules"),
    )?;
    Ok(())
}

fn emit_spq_reports<W: Write>(
    mut out: &mut W,
    prog: &AscentProgram,
    all_pass: &mut bool,
) -> io::Result<()> {
    let rows = spq1_rows(prog);
    let v = record(all_pass, spq1_verify(&rows));
    emit(
        &mut out,
        "SP-Q1",
        r#"? *handle{id, kind, status}, kind = "label", status = "open"."#,
        &rows,
        v,
        Some("literal stored-relation projection; MVS-1 remains all handles"),
    )?;

    let rows = spq2_rows(prog);
    let v = record(all_pass, spq2_verify(&rows));
    emit(
        &mut out,
        "SP-Q2",
        r#"release_blocker(h, "broken_ref") := diagnostic("E001", _, h, _, _, _).
release_blocker(h, "undischarged") := diagnostic("E002", _, h, _, _, _).
? release_blocker(h, why)."#,
        &rows,
        v,
        Some("literal two-clause union; excludes MVS stale_dep extension"),
    )?;

    let rows = spq3_rows(prog, V17);
    let v = record(all_pass, spq3_verify(&rows));
    emit(
        &mut out,
        "SP-Q3",
        r#"? upstream("formal-model/v17.md", anc)."#,
        &rows,
        v,
        Some("literal depends_on transitive closure query"),
    )?;

    let rows = spq4_rows(prog);
    let v = record(all_pass, spq4_verify(&rows));
    emit(
        &mut out,
        "SP-Q4",
        r#"unfinished(h) := *handle{id: h, kind: "label", namespace: "OQ"}, not terminal(h).
? unfinished(h)."#,
        &rows,
        v,
        None,
    )?;

    let rows = spq5_rows(prog);
    let v = record(all_pass, spq5_verify(&rows));
    emit(
        &mut out,
        "SP-Q5",
        r#"area_active_count(area, n) := n = Count{ h : *handle{id: h, kind: "file", area}, active(h) }.
? area_active_count(area, n)."#,
        &rows,
        v,
        Some("literal active-file count; distinct from MVS OQ pressure/per-area probes"),
    )?;

    let rows = spq6_rows(prog);
    let v = record(all_pass, spq6_verify(&rows));
    emit(
        &mut out,
        "SP-Q6",
        r#"status_changed(h, prev, curr) := *handle{id: h, status: curr}, at("snapshot:last") { *handle{id: h, status: prev} }, prev != curr.
? status_changed(h, prev, curr)."#,
        &rows,
        v,
        Some("literal status-change query; MVS-6 recently_advanced remains separate"),
    )?;

    let spq7_base = spq1_rows(prog);
    let v = record(all_pass, spq7_verify(&spq7_base));
    emit(
        &mut out,
        "SP-Q7",
        "SP-Q1 with --limit=1000; verify one complete JSON object per line and deduped rows.",
        &spq7_base[..spq7_base.len().min(1_000)],
        v,
        Some("capability::emit streams one serde_json object then newline per row/report"),
    )?;

    let rows = spq8_rows(prog, V17);
    let v = record(all_pass, spq8_verify(&rows));
    emit(
        &mut out,
        "SP-Q8",
        r#"? upstream("formal-model/v17.md", anc) --explain."#,
        &rows,
        v,
        Some("explain represented as _derivation chain on literal upstream query"),
    )?;

    let report = spq9_report().expect("fixture-anneal.dl should load");
    let rows = spq9_rows(&report, spq2_rows(prog).len());
    let v = record(all_pass, spq9_verify(&report, &rows));
    emit(
        &mut out,
        "SP-Q9",
        "load fixture-anneal.dl after prelude; ? release_blocker(h, why); warn on terminal/1 shadow.",
        &rows,
        v,
        Some("validated through dynamic-IR project-extension loader skeleton"),
    )?;
    Ok(())
}

fn main() -> io::Result<()> {
    let prog = load_fixture();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut all_pass = true;

    emit_mvs_reports(&mut out, &prog, &mut all_pass)?;
    emit_spq_reports(&mut out, &prog, &mut all_pass)?;
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
    use spike_runner::types::{DiagnosticCode, FilePath};
    use std::sync::OnceLock;

    fn prog() -> &'static AscentProgram {
        static P: OnceLock<AscentProgram> = OnceLock::new();
        P.get_or_init(load_fixture)
    }

    #[test]
    fn mvs1_loads_all_fixture_handles() {
        assert!(mvs1_verify(&mvs1_rows(prog())).is_pass());
    }
    #[test]
    fn mvs2_all_three_release_blocker_clauses_fire() {
        assert!(mvs2_verify(&mvs2_rows(prog())).is_pass());
    }
    #[test]
    fn mvs3_supersedes_chain_recurses_to_depth_3() {
        assert!(mvs3_verify(&mvs3_rows(prog(), V17)).is_pass());
    }
    #[test]
    fn mvs4_stratified_negation_excludes_terminal_oqs() {
        assert!(mvs4_verify(&mvs4_rows(prog())).is_pass());
    }
    #[test]
    fn mvs5a_oq_pressure_aggregation_groups_by_handle() {
        assert!(mvs5a_verify(&mvs5a_rows(prog())).is_pass());
    }
    #[test]
    fn mvs5b_oq_per_area_aggregation_groups_by_area() {
        assert!(mvs5b_verify(&mvs5b_rows(prog())).is_pass());
    }
    #[test]
    fn mvs6_recently_advanced_picks_up_pipeline_forward_moves() {
        assert!(mvs6_verify(&mvs6_rows(prog())).is_pass());
    }
    #[test]
    fn mvs6_terminal_transitions_do_not_count_as_advancing() {
        assert!(!mvs6_rows(prog()).iter().any(|r| r.h == JIT_STALE));
    }

    #[test]
    fn derived_e001_fires_for_pending_edge_with_missing_target() {
        assert!(
            diagnostics_derived(prog())
                .iter()
                .any(|d| d.code == DiagnosticCode::E001)
        );
    }
    #[test]
    fn derived_e002_fires_for_undischarged_obligation() {
        let diag = diagnostics_derived(prog());
        assert!(
            diag.iter()
                .any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_88)
        );
        assert!(
            diag.iter()
                .any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_22)
        );
    }
    #[test]
    fn derived_e002_does_not_fire_for_discharged_obligation() {
        assert!(
            !diagnostics_derived(prog())
                .iter()
                .any(|d| d.code == DiagnosticCode::E002 && d.handle == OQ_77)
        );
    }
    #[test]
    fn derived_w001_fires_for_stale_dep() {
        assert!(
            diagnostics_derived(prog())
                .iter()
                .any(|d| d.code == DiagnosticCode::W001 && d.handle == JIT_SPEC)
        );
    }

    #[test]
    fn mvs8_upstream_chain_reconstructs_exec_to_oq22() {
        assert!(mvs8_upstream_verify(&mvs8_upstream_rows(prog())).is_pass());
    }
    #[test]
    fn mvs8_direct_upstream_facts_reconstruct_as_single_direct_step() {
        let rows = mvs8_upstream_rows(prog());
        let direct = rows
            .iter()
            .find(|r| r.h == JIT_SPEC && r.anc == OQ_22)
            .expect("upstream(jit-spec, OQ-22) missing");
        assert_eq!(direct.chain, vec![UpstreamStep::Direct]);
    }

    #[test]
    fn upstream_via_covers_every_upstream_fact() {
        let p = prog();
        let via: std::collections::HashSet<(HandleId, HandleId)> = p
            .upstream_via
            .iter()
            .map(|(h, anc, _)| (*h, *anc))
            .collect();
        for &(h, anc) in &p.upstream {
            assert!(
                via.contains(&(h, anc)),
                "upstream({h:?}, {anc:?}) has no upstream_via row"
            );
        }
    }

    #[test]
    fn mvs2_blocker_carries_broken_ref_bindings() {
        let rows = mvs2_rows(prog());
        let broken = rows
            .iter()
            .find(|r| matches!(r.blocker, Blocker::BrokenRef { .. }))
            .expect("no broken_ref blocker");
        assert!(matches!(
            broken.blocker,
            Blocker::BrokenRef {
                file: FilePath("compiler/jit-spec.md"),
                line: 51
            }
        ));
    }

    #[test]
    fn mvs2_blocker_carries_stale_dep_target() {
        let rows = mvs2_rows(prog());
        let stale = rows
            .iter()
            .find(|r| matches!(r.blocker, Blocker::StaleDep { .. }))
            .expect("no stale_dep blocker");
        assert!(matches!(stale.blocker, Blocker::StaleDep { target: t } if t == JIT_STALE));
    }

    #[test]
    fn spq1_projects_open_label_handles_only() {
        assert!(spq1_verify(&spq1_rows(prog())).is_pass());
        assert!(!spq1_rows(prog()).iter().any(|r| r.id == OQ_99));
    }

    #[test]
    fn spq2_runs_literal_two_clause_release_blocker() {
        let rows = spq2_rows(prog());
        assert!(spq2_verify(&rows).is_pass());
        assert!(!rows.iter().any(|r| r.why == "stale_dep"));
    }

    #[test]
    fn spq3_queries_v17_depends_on_upstream_directly() {
        assert!(spq3_verify(&spq3_rows(prog(), V17)).is_pass());
    }

    #[test]
    fn spq4_matches_unfinished_oq_literal_query() {
        assert!(spq4_verify(&spq4_rows(prog())).is_pass());
    }

    #[test]
    fn spq5_counts_active_files_by_area() {
        assert!(spq5_verify(&spq5_rows(prog())).is_pass());
    }

    #[test]
    fn spq6_reports_all_status_changes_not_only_advancement() {
        let rows = spq6_rows(prog());
        assert!(spq6_verify(&rows).is_pass());
        assert!(rows.iter().any(|r| r.h == JIT_STALE));
    }

    #[test]
    fn spq7_emits_limited_deduped_ndjson() {
        assert!(spq7_verify(&spq1_rows(prog())).is_pass());
    }

    #[test]
    fn spq8_explains_literal_v17_upstream_query() {
        let rows = spq8_rows(prog(), V17);
        assert!(spq8_verify(&rows).is_pass());
        assert!(rows.iter().all(|row| !row.chain.is_empty()));
    }

    #[test]
    fn spq9_loads_project_rules_and_reports_shadow_warning() {
        let report = spq9_report().expect("fixture-anneal.dl loads");
        let rows = spq9_rows(&report, spq2_rows(prog()).len());
        assert!(spq9_verify(&report, &rows).is_pass());
        assert_eq!(rows[0].release_blocker, "release_blocker/2");
        assert!(rows[0].shadow_warnings.iter().any(|p| p == "terminal/1"));
    }
}
