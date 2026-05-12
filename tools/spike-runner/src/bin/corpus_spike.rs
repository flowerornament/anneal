//! `corpus_spike` — run the spike's ascent program against a real corpus
//! loaded via [`spike_runner::loader`]. Times the load, fixpoint, and
//! per-capability row construction to produce numbers against the
//! SP-R1 perf gate.
//!
//! Usage:
//!   `cargo run --release --bin corpus_spike -- [<root>]`
//!
//! Default root: `/path/to/large-corpus/.design`. Set `SPIKE_CORPUS_ROOT` to
//! override without flags.

use spike_runner::loader::{load_via_anneal, LoadError};
use spike_runner::program::{
    diagnostics_derived, mvs1_rows, mvs2_rows, mvs3_rows, mvs4_rows, mvs5a_rows, mvs5b_rows,
    mvs6_rows, mvs8_upstream_rows, AscentProgram,
};
use spike_runner::types::{HandleId, Namespace};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[derive(serde::Serialize)]
struct ScaleReport {
    corpus_root: String,
    facts: FactCounts,
    timings_ms: Timings,
    derived: DerivedCounts,
}

#[derive(serde::Serialize)]
struct FactCounts {
    handles: usize,
    edges: usize,
}

#[derive(serde::Serialize)]
struct Timings {
    load: f64,
    fill_program: f64,
    fixpoint: f64,
    mvs1: f64,
    mvs2: f64,
    mvs3: f64,
    mvs4: f64,
    mvs5a: f64,
    mvs5b: f64,
    mvs6: f64,
    mvs8_upstream: f64,
    diagnostics: f64,
    total: f64,
}

#[derive(serde::Serialize)]
struct DerivedCounts {
    terminal: usize,
    active: usize,
    settled: usize,
    upstream: usize,
    supersedes_chain: usize,
    obligation: usize,
    discharged: usize,
    undischarged: usize,
    diagnostic: usize,
    release_blocker: usize,
    open_oq: usize,
    recently_advanced: usize,
}

fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1000.0 }

fn corpus_root() -> PathBuf {
    if let Ok(p) = std::env::var("SPIKE_CORPUS_ROOT") {
        return PathBuf::from(p);
    }
    if let Some(arg) = std::env::args().nth(1) {
        return PathBuf::from(arg);
    }
    let home = std::env::var("HOME").expect("HOME unset");
    PathBuf::from(home).join("code/large-corpus/.design")
}

fn fill_program(prog: &mut AscentProgram, corpus: &spike_runner::loader::Corpus) {
    prog.handle.reserve(corpus.handles.len());
    for h in &corpus.handles {
        prog.handle.push((h.id, h.kind, h.status, h.namespace, h.file, h.area, h.date));
    }
    prog.edge.reserve(corpus.edges.len());
    for e in &corpus.edges {
        prog.edge.push((e.from, e.to, e.kind, e.file, e.line));
    }
    // OQ is a near-universal anneal/large-corpus/host-corpus linear namespace.
    prog.linear_namespace.push((Namespace("OQ"),));
    prog.pipeline_position_for.reserve(spike_runner::PIPELINE_ORDERING.len());
    for (i, s) in spike_runner::PIPELINE_ORDERING.iter().enumerate() {
        prog.pipeline_position_for.push((*s, i));
    }
}

fn run() -> Result<i32, LoadError> {
    let root = corpus_root();
    eprintln!("corpus: {}", root.display());

    let total_start = Instant::now();

    let t = Instant::now();
    let corpus = load_via_anneal(&root)?;
    let load_ms = ms(t.elapsed());

    let mut prog = AscentProgram::default();
    let t = Instant::now();
    fill_program(&mut prog, &corpus);
    let fill_ms = ms(t.elapsed());

    let t = Instant::now();
    prog.run();
    let fixpoint_ms = ms(t.elapsed());

    let t = Instant::now(); let rows_mvs1   = mvs1_rows(&prog);   let mvs1_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs2   = mvs2_rows(&prog);   let mvs2_ms = ms(t.elapsed());
    // Pick any supersedes source as MVS-3 root — gives non-empty output.
    let mvs3_root = prog.supersedes_chain.first().map_or(HandleId(""), |(s, _, _)| *s);
    let t = Instant::now(); let rows_mvs3   = mvs3_rows(&prog, mvs3_root); let mvs3_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs4   = mvs4_rows(&prog);   let mvs4_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs5a  = mvs5a_rows(&prog);  let mvs5a_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs5b  = mvs5b_rows(&prog);  let mvs5b_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs6   = mvs6_rows(&prog);   let mvs6_ms = ms(t.elapsed());
    let t = Instant::now(); let rows_mvs8u  = mvs8_upstream_rows(&prog); let mvs8_ms = ms(t.elapsed());
    let t = Instant::now(); let diagnostics = diagnostics_derived(&prog); let diag_ms = ms(t.elapsed());

    let total_ms = ms(total_start.elapsed());

    let report = ScaleReport {
        corpus_root: root.display().to_string(),
        facts: FactCounts {
            handles: corpus.handle_count(),
            edges: corpus.edge_count(),
        },
        timings_ms: Timings {
            load: load_ms,
            fill_program: fill_ms,
            fixpoint: fixpoint_ms,
            mvs1: mvs1_ms,
            mvs2: mvs2_ms,
            mvs3: mvs3_ms,
            mvs4: mvs4_ms,
            mvs5a: mvs5a_ms,
            mvs5b: mvs5b_ms,
            mvs6: mvs6_ms,
            mvs8_upstream: mvs8_ms,
            diagnostics: diag_ms,
            total: total_ms,
        },
        derived: DerivedCounts {
            terminal:          prog.terminal.len(),
            active:            prog.active.len(),
            settled:           prog.settled.len(),
            upstream:          prog.upstream.len(),
            supersedes_chain:  prog.supersedes_chain.len(),
            obligation:        prog.obligation.len(),
            discharged:        prog.discharged.len(),
            undischarged:      prog.undischarged.len(),
            diagnostic:        prog.diagnostic.len(),
            release_blocker:   prog.release_blocker.len(),
            open_oq:           prog.open_oq.len(),
            recently_advanced: prog.recently_advanced.len(),
        },
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, &report).expect("serialize report");
    out.write_all(b"\n")?;

    // Sample first 3 rows from each capability — proof the streaming
    // architecture works at scale without exhaustively dumping 13k rows.
    let mut sample = |label: &'static str, items: &[serde_json::Value]| {
        writeln!(out, "\n--- {label} sample (first 3 of {}) ---", items.len()).ok();
        for v in items.iter().take(3) {
            serde_json::to_writer(&mut out, v).ok();
            out.write_all(b"\n").ok();
        }
    };
    sample("MVS-1 handles",   &rows_mvs1.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("MVS-2 blockers",  &rows_mvs2.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample(
        if mvs3_root == HandleId("") { "MVS-3 supersedes (no supersedes edges)" }
        else { "MVS-3 supersedes (from first supersedes source)" },
        &rows_mvs3.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>(),
    );
    sample("MVS-4 open_oq",   &rows_mvs4.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("MVS-5a pressure", &rows_mvs5a.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("MVS-5b per-area", &rows_mvs5b.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("MVS-6 advanced",  &rows_mvs6.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("MVS-8 upstream",  &rows_mvs8u.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());
    sample("diagnostics",     &diagnostics.iter().map(|r| serde_json::to_value(r).unwrap()).collect::<Vec<_>>());

    out.flush()?;
    Ok(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(0)  => ExitCode::SUCCESS,
        Ok(_)  => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
