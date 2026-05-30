//! Benchmark the Phase 0 dynamic-IR skeleton against the frozen sample
//! fixture. The gate is warm prelude evaluation under 50ms.
//!
//! Usage:
//!   `cargo run --release --bin dynamic_ir_bench -- [<root>] [--iterations N]`

use serde::Serialize;
use spike_runner::dynamic_ir::{BENCH_PRELUDE, Evaluator, Program};
use spike_runner::loader::{LoadError, load_via_anneal};
use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_ITERATIONS: usize = 20;
const WARM_GATE_MS: f64 = 50.0;

#[derive(Serialize)]
struct BenchReport {
    corpus_root: String,
    facts: FactCounts,
    rules: usize,
    iterations: usize,
    timings_ms: Timings,
    relation_counts: BTreeMap<&'static str, usize>,
    gate: Gate,
}

#[derive(Serialize)]
struct FactCounts {
    handles: usize,
    edges: usize,
}

#[derive(Serialize)]
struct Timings {
    load: f64,
    parse: f64,
    first_eval: f64,
    warm_min: f64,
    warm_avg: f64,
    warm_last: f64,
}

#[derive(Serialize)]
struct Gate {
    target_warm_avg_ms: f64,
    passed: bool,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn corpus_root() -> PathBuf {
    if let Ok(p) = std::env::var("DYNAMIC_IR_CORPUS_ROOT") {
        return PathBuf::from(p);
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--iterations" {
            let _ = args.next();
        } else if !arg.starts_with("--") {
            return PathBuf::from(arg);
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.fixtures/sample-corpus")
}

fn iterations() -> Result<usize, String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--iterations" {
            let Some(value) = args.next() else {
                return Err("--iterations requires a value".to_string());
            };
            return value
                .parse::<usize>()
                .map_err(|_| "--iterations must be a positive integer".to_string())
                .and_then(|n| {
                    if n == 0 {
                        Err("--iterations must be greater than zero".to_string())
                    } else {
                        Ok(n)
                    }
                });
        }
    }
    Ok(DEFAULT_ITERATIONS)
}

fn run() -> Result<BenchReport, LoadError> {
    let root = corpus_root();
    let iterations =
        iterations().map_err(|e| LoadError::Io(io::Error::new(io::ErrorKind::InvalidInput, e)))?;

    let t = Instant::now();
    let corpus = load_via_anneal(&root)?;
    let load = ms(t.elapsed());

    let t = Instant::now();
    let program = Program::parse(BENCH_PRELUDE)
        .map_err(|e| LoadError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
    let parse = ms(t.elapsed());
    let rules = program.rules().len();
    let evaluator = Evaluator::new(program);

    let t = Instant::now();
    let first = evaluator.eval(&corpus);
    let first_eval = ms(t.elapsed());

    let mut samples = Vec::with_capacity(iterations);
    let mut last = first.clone();
    for _ in 0..iterations {
        let t = Instant::now();
        last = evaluator.eval(&corpus);
        samples.push(ms(t.elapsed()));
    }

    assert_eq!(
        first.relation_counts, last.relation_counts,
        "warm evaluation changed relation counts"
    );

    let warm_min = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let sample_count = u32::try_from(samples.len()).expect("iteration count fits in u32");
    let warm_avg = samples.iter().sum::<f64>() / f64::from(sample_count);
    let warm_last = samples.last().copied().unwrap_or(0.0);

    Ok(BenchReport {
        corpus_root: root.display().to_string(),
        facts: FactCounts {
            handles: corpus.handles.len(),
            edges: corpus.edges.len(),
        },
        rules,
        iterations,
        timings_ms: Timings {
            load,
            parse,
            first_eval,
            warm_min,
            warm_avg,
            warm_last,
        },
        relation_counts: last.relation_counts,
        gate: Gate {
            target_warm_avg_ms: WARM_GATE_MS,
            passed: warm_avg < WARM_GATE_MS,
        },
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            let passed = report.gate.passed;
            let stdout = io::stdout();
            let mut out = BufWriter::new(stdout.lock());
            if let Err(e) = serde_json::to_writer_pretty(&mut out, &report) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            if let Err(e) = out.write_all(b"\n").and_then(|()| out.flush()) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            if passed {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "error: dynamic IR warm avg {:.3}ms exceeds {:.3}ms gate",
                    report.timings_ms.warm_avg, WARM_GATE_MS
                );
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
