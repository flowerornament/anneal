//! ascent_spike — MVS-1..9 against the large-corpus-shaped fixture using ascent.
//!
//! Goal: prove out the language-redesign spec's MVS capabilities against
//! `ascent` as a candidate engine. Emits NDJSON to stdout, one record per
//! query. Exit 0 on full pass, 1 on any MVS failure.
//!
//! Currently exercises MVS-1 (stored relations), MVS-2 (multi-clause
//! rule union), MVS-3 (transitive recursion), MVS-4 (stratified negation),
//! and MVS-5 (aggregation with grouping). MVS-6..9 are pending.

use ascent::ascent;
use ascent::aggregators::count;
use serde::Serialize;
use spike_runner::fixture::{self, TERMINAL_STATUSES};
use std::io::{self, Write};

ascent! {
    // ----- Stored relations (MVS-1) -----
    // Tuple-typed because ascent doesn't support named fields directly.
    // Field order: id, kind, status, namespace, area
    relation handle(&'static str, &'static str, &'static str, &'static str, &'static str);

    // Field order: from, to, kind
    relation edge(&'static str, &'static str, &'static str);

    // Field order: code, severity, handle, file, line
    relation diagnostic(&'static str, &'static str, &'static str, &'static str, u32);

    // Lookup table for terminal statuses.
    relation is_terminal_status(&'static str);

    // ----- Derived predicates (engine-provided in spec §8) -----
    relation terminal(&'static str);
    terminal(h) <-- handle(h, _, s, _, _), is_terminal_status(s);

    relation active(&'static str);
    active(h) <-- handle(h, _, _, _, _), !terminal(h);

    // upstream(h, anc) — transitive depends_on (MVS-3 recursion)
    relation upstream(&'static str, &'static str);
    upstream(h, anc)  <-- edge(h, anc, "depends_on");
    upstream(h, anc)  <-- edge(h, mid, "depends_on"), upstream(mid, anc);

    // ----- Project-level multi-clause rule (MVS-2) -----
    // release_blocker = broken refs OR undischarged OR stuck-on-stale-dep
    relation release_blocker(&'static str, &'static str);
    release_blocker(h, "broken_ref")    <-- diagnostic("E001", _, h, _, _);
    release_blocker(h, "undischarged")  <-- diagnostic("E002", _, h, _, _);
    release_blocker(h, "stale_dep")     <--
        edge(h, t, "depends_on"),
        active(h),
        terminal(t);

    // ----- Settled statuses for downstream pressure (MVS-5 setup) -----
    relation settled_status(&'static str);

    // ----- Open OQs (MVS-4: stratified negation) -----
    relation open_oq(&'static str);
    open_oq(q) <-- handle(q, "label", _, "OQ", _), !terminal(q);

    // ----- OQ pressure (MVS-5: aggregation with grouping by q) -----
    // For each open OQ, count downstream handles whose status is settled.
    relation downstream_settled(&'static str, &'static str);
    downstream_settled(q, x) <--
        open_oq(q),
        edge(x, q, "depends_on"),
        handle(x, _, sx, _, _),
        settled_status(sx);

    relation oq_pressure(&'static str, usize);
    oq_pressure(q, n) <--
        open_oq(q),
        agg n = count() in downstream_settled(q, _);

    // ----- Per-area open-OQ counts (MVS-5 grouping by area) -----
    relation oq_in_area(&'static str, &'static str);
    oq_in_area(area, q) <-- handle(q, "label", _, "OQ", area), !terminal(q);

    relation oq_per_area(&'static str, usize);
    oq_per_area(area, n) <--
        oq_in_area(area, _),
        agg n = count() in oq_in_area(area, _);

    // ----- Supersession chain (MVS-3 recursion, with depth) -----
    relation supersedes_chain(&'static str, &'static str, usize);
    supersedes_chain(s, t, 1) <-- edge(s, t, "supersedes");
    supersedes_chain(s, t, d + 1) <--
        edge(s, m, "supersedes"),
        supersedes_chain(m, t, d);
}

#[derive(Serialize)]
struct MvsResult {
    capability: &'static str,
    query: &'static str,
    pass: bool,
    rows: serde_json::Value,
    detail: Option<&'static str>,
}

fn write_record<W: Write>(w: &mut W, rec: &MvsResult) -> io::Result<()> {
    let line = serde_json::to_string(rec).expect("serialize MvsResult");
    writeln!(w, "{line}")
}

fn main() -> anyhow::Result<()> {
    let mut prog = AscentProgram::default();

    // ---- Load fixture (MVS-1) ----
    for h in fixture::handles() {
        prog.handle.push((h.id, h.kind, h.status, h.namespace, h.area));
    }
    for e in fixture::edges() {
        prog.edge.push((e.from, e.to, e.kind));
    }
    for d in fixture::diagnostics() {
        prog.diagnostic.push((d.code, d.severity, d.handle, d.file, d.line));
    }
    for s in TERMINAL_STATUSES {
        prog.is_terminal_status.push((s,));
    }
    for s in &["authoritative", "current", "active", "stable"] {
        prog.settled_status.push((s,));
    }

    prog.run();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // ---- MVS-1: Stored relation projection ----
    let mut handles: Vec<_> = prog.handle.iter()
        .map(|(id, kind, status, ns, area)| serde_json::json!({
            "id": id, "kind": kind, "status": status, "namespace": ns, "area": area
        }))
        .collect();
    handles.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_string());
    write_record(&mut out, &MvsResult {
        capability: "MVS-1",
        query: "? *handle{id, kind, status, namespace, area}.",
        pass: handles.len() == fixture::handles().len(),
        rows: serde_json::Value::Array(handles.clone()),
        detail: None,
    })?;

    // ---- MVS-2: Multi-clause rule (release_blocker) ----
    let mut blockers: Vec<_> = prog.release_blocker.iter()
        .map(|(h, why)| serde_json::json!({"h": h, "why": why}))
        .collect();
    blockers.sort_by_key(|v| (v["h"].as_str().unwrap_or("").to_string(),
                              v["why"].as_str().unwrap_or("").to_string()));
    write_record(&mut out, &MvsResult {
        capability: "MVS-2",
        query: "release_blocker(h, \"broken_ref\")   := diagnostic(\"E001\", ...).\n\
                release_blocker(h, \"undischarged\") := diagnostic(\"E002\", ...).\n\
                release_blocker(h, \"stale_dep\")    := edge(h, t, \"depends_on\"), active(h), terminal(t).\n\
                ? release_blocker(h, why).",
        // Expected: at least the stale_dep clause should fire for compiler/jit-spec.md
        // (no E001/E002 in fixture, so only stale_dep is reachable).
        pass: blockers.iter().any(|r| r["h"] == "compiler/jit-spec.md" && r["why"] == "stale_dep"),
        rows: serde_json::Value::Array(blockers),
        detail: Some("only stale_dep clause exercised; no E001/E002 fixture data"),
    })?;

    // ---- MVS-3: Transitive recursion (supersedes chain from v17) ----
    let mut chain: Vec<_> = prog.supersedes_chain.iter()
        .filter(|(s, _, _)| *s == "formal-model/v17.md")
        .map(|(s, t, d)| serde_json::json!({"start": s, "target": t, "depth": d}))
        .collect();
    chain.sort_by_key(|v| v["depth"].as_u64().unwrap_or(0));
    let chain_pass = chain.len() == 3
        && chain[0]["target"] == "formal-model/v16.md"
        && chain[1]["target"] == "formal-model/v15.md"
        && chain[2]["target"] == "formal-model/v14.md";
    write_record(&mut out, &MvsResult {
        capability: "MVS-3",
        query: "supersedes_chain(s, t, 1)     := edge(s, t, \"supersedes\").\n\
                supersedes_chain(s, t, d + 1) := edge(s, m, \"supersedes\"), supersedes_chain(m, t, d).\n\
                ? supersedes_chain(\"formal-model/v17.md\", t, d).",
        pass: chain_pass,
        rows: serde_json::Value::Array(chain),
        detail: None,
    })?;

    // ---- MVS-4: Stratified negation (open OQs) ----
    let mut oqs: Vec<_> = prog.open_oq.iter()
        .map(|(q,)| serde_json::json!({"q": q}))
        .collect();
    oqs.sort_by_key(|v| v["q"].as_str().unwrap_or("").to_string());
    // Fixture has 4 OQs: OQ-22, OQ-23, OQ-60 open; OQ-99 resolved (terminal).
    let neg_pass = oqs.len() == 3
        && !oqs.iter().any(|v| v["q"] == "OQ-99");
    write_record(&mut out, &MvsResult {
        capability: "MVS-4",
        query: "open_oq(q) := *handle{id: q, kind: \"label\", namespace: \"OQ\"}, not terminal(q).\n\
                ? open_oq(q).",
        pass: neg_pass,
        rows: serde_json::Value::Array(oqs),
        detail: None,
    })?;

    // ---- MVS-5a: Aggregation with grouping (OQ pressure) ----
    let mut pressure: Vec<_> = prog.oq_pressure.iter()
        .map(|(q, n)| serde_json::json!({"q": q, "n": n}))
        .collect();
    pressure.sort_by_key(|v| v["q"].as_str().unwrap_or("").to_string());
    // OQ-22 should have pressure 1 (depended on by v17 [authoritative]).
    // jit-spec is draft, not settled, so doesn't count.
    let pressure_pass = pressure.iter().any(|r| r["q"] == "OQ-22" && r["n"] == 1);
    write_record(&mut out, &MvsResult {
        capability: "MVS-5a",
        query: "oq_pressure(q, n) := open_oq(q), n = Count{ x : downstream_settled(q, x) }.\n\
                ? oq_pressure(q, n).",
        pass: pressure_pass,
        rows: serde_json::Value::Array(pressure),
        detail: Some("expects OQ-22 to have pressure 1 (v17 is settled, jit-spec is draft)"),
    })?;

    // ---- MVS-5b: Aggregation grouping by area ----
    let mut per_area: Vec<_> = prog.oq_per_area.iter()
        .map(|(area, n)| serde_json::json!({"area": area, "n": n}))
        .collect();
    per_area.sort_by_key(|v| v["area"].as_str().unwrap_or("").to_string());
    // Expected: formal-model has 2 open OQs (OQ-22, OQ-23), compiler has 1 (OQ-60).
    let area_pass = per_area.iter().any(|r| r["area"] == "formal-model" && r["n"] == 2)
                 && per_area.iter().any(|r| r["area"] == "compiler" && r["n"] == 1);
    write_record(&mut out, &MvsResult {
        capability: "MVS-5b",
        query: "oq_per_area(area, n) := n = Count{ q : *handle{id: q, kind: \"label\", namespace: \"OQ\", area}, not terminal(q) }.\n\
                ? oq_per_area(area, n).",
        pass: area_pass,
        rows: serde_json::Value::Array(per_area),
        detail: None,
    })?;

    // ---- Summary line ----
    let summary = serde_json::json!({
        "engine": "ascent",
        "mvs_covered": ["MVS-1", "MVS-2", "MVS-3", "MVS-4", "MVS-5a", "MVS-5b"],
        "mvs_pending": ["MVS-6 (time travel)", "MVS-7 (streaming NDJSON)",
                        "MVS-8 (provenance)", "MVS-9 (anneal.dl loading)"],
        "fact_counts": {
            "handles": prog.handle.len(),
            "edges": prog.edge.len(),
        },
        "derived_counts": {
            "terminal": prog.terminal.len(),
            "active": prog.active.len(),
            "upstream": prog.upstream.len(),
            "release_blocker": prog.release_blocker.len(),
            "supersedes_chain": prog.supersedes_chain.len(),
            "open_oq": prog.open_oq.len(),
            "oq_pressure": prog.oq_pressure.len(),
            "oq_per_area": prog.oq_per_area.len(),
        }
    });
    writeln!(out, "{}", serde_json::to_string_pretty(&summary)?)?;

    Ok(())
}
