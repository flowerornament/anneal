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

use serde::Serialize;
use spike_runner::loader::{load_via_anneal, Corpus, LoadError};
use spike_runner::program::{
    diagnostics_derived, mvs1_rows, mvs2_rows, mvs3_rows, mvs4_rows, mvs5a_rows, mvs5b_rows,
    mvs6_rows, mvs8_upstream_rows, push_edges, push_handles, push_linear_namespaces,
    push_pipeline_ordering, AscentProgram,
};
use spike_runner::types::{HandleId, Namespace};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct ScaleReport {
    corpus_root: String,
    facts: FactCounts,
    timings_ms: Timings,
    derived: DerivedCounts,
}

#[derive(Serialize)]
struct FactCounts { handles: usize, edges: usize }

#[derive(Serialize)]
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

#[derive(Serialize)]
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
    if let Ok(p) = std::env::var("SPIKE_CORPUS_ROOT") { return PathBuf::from(p); }
    if let Some(arg) = std::env::args().nth(1) { return PathBuf::from(arg); }
    let home = std::env::var("HOME").expect("HOME unset");
    PathBuf::from(home).join("code/large-corpus/.design")
}

fn fill_program(prog: &mut AscentProgram, corpus: &Corpus) {
    push_handles(prog, &corpus.handles);
    push_edges(prog, &corpus.edges);
    // OQ is a near-universal anneal/large-corpus/host-corpus linear namespace; declaring
    // it surfaces undischarged-obligation diagnostics from any of the three.
    push_linear_namespaces(prog, &[Namespace("OQ")]);
    push_pipeline_ordering(prog);
}

fn write_sample<W: Write, T: Serialize>(
    out: &mut W,
    label: &str,
    items: &[T],
) -> io::Result<()> {
    writeln!(out, "\n--- {label} sample (first 3 of {}) ---", items.len())?;
    for item in items.iter().take(3) {
        serde_json::to_writer(&mut *out, item)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn run() -> Result<(), LoadError> {
    let root = corpus_root();
    eprintln!("corpus: {}", root.display());

    let total_start = Instant::now();

    let t = Instant::now();
    let corpus = load_via_anneal(&root)?;
    let load = ms(t.elapsed());

    let mut prog = AscentProgram::default();
    let t = Instant::now();
    fill_program(&mut prog, &corpus);
    let fill = ms(t.elapsed());

    let t = Instant::now();
    prog.run();
    let fixpoint = ms(t.elapsed());

    let t = Instant::now(); let mvs1   = mvs1_rows(&prog);   let mvs1_ms   = ms(t.elapsed());
    let t = Instant::now(); let mvs2   = mvs2_rows(&prog);   let mvs2_ms   = ms(t.elapsed());
    let mvs3_root: Option<HandleId> = prog.supersedes_chain.first().map(|(s, _, _)| *s);
    let t = Instant::now();
    let mvs3 = mvs3_root.map(|r| mvs3_rows(&prog, r)).unwrap_or_default();
    let mvs3_ms = ms(t.elapsed());
    let t = Instant::now(); let mvs4   = mvs4_rows(&prog);   let mvs4_ms   = ms(t.elapsed());
    let t = Instant::now(); let mvs5a  = mvs5a_rows(&prog);  let mvs5a_ms  = ms(t.elapsed());
    let t = Instant::now(); let mvs5b  = mvs5b_rows(&prog);  let mvs5b_ms  = ms(t.elapsed());
    let t = Instant::now(); let mvs6   = mvs6_rows(&prog);   let mvs6_ms   = ms(t.elapsed());
    let t = Instant::now(); let mvs8u  = mvs8_upstream_rows(&prog); let mvs8_ms = ms(t.elapsed());
    let t = Instant::now(); let diag   = diagnostics_derived(&prog); let diag_ms = ms(t.elapsed());

    let total = ms(total_start.elapsed());

    let report = ScaleReport {
        corpus_root: root.display().to_string(),
        facts: FactCounts {
            handles: corpus.handles.len(),
            edges: corpus.edges.len(),
        },
        timings_ms: Timings {
            load, fill_program: fill, fixpoint,
            mvs1: mvs1_ms, mvs2: mvs2_ms, mvs3: mvs3_ms, mvs4: mvs4_ms,
            mvs5a: mvs5a_ms, mvs5b: mvs5b_ms, mvs6: mvs6_ms,
            mvs8_upstream: mvs8_ms, diagnostics: diag_ms, total,
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
    serde_json::to_writer_pretty(&mut out, &report)?;
    out.write_all(b"\n")?;

    write_sample(&mut out, "MVS-1 handles",   &mvs1)?;
    write_sample(&mut out, "MVS-2 blockers",  &mvs2)?;
    let mvs3_label = if mvs3_root.is_some() {
        "MVS-3 supersedes (from first supersedes source)"
    } else {
        "MVS-3 supersedes (no supersedes edges in corpus)"
    };
    write_sample(&mut out, mvs3_label, &mvs3)?;
    write_sample(&mut out, "MVS-4 open_oq",   &mvs4)?;
    write_sample(&mut out, "MVS-5a pressure", &mvs5a)?;
    write_sample(&mut out, "MVS-5b per-area", &mvs5b)?;
    write_sample(&mut out, "MVS-6 advanced",  &mvs6)?;
    write_sample(&mut out, "MVS-8 upstream",  &mvs8u)?;
    write_sample(&mut out, "diagnostics",     &diag)?;

    out.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
