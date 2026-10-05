//! Synthetic code-population benchmark for anneal-u865.
//!
//! Run with `cargo run --release -p anneal-core --example measure_code_population -- 10000`.
//! Pass `old-code` or `tuple-code` as the second argument for paired identity costs.
//! Measure peak RSS externally with `/usr/bin/time -l` (macOS) or `time -v` (Linux).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anneal_core::runtime::{Database, Evaluator, NumberValue, Value, analyze, parse_program};
use anneal_core::{
    CorpusId, EdgeFact, FactBatch, FactBatchMode, FactIdentity, FactStore, Generation, HandleFact,
    HandleId, NativeId, OriginUri, Revision, SourceName,
};
use serde_json::Value as JsonValue;

#[cfg(feature = "dhat")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const HANDLES: usize = 1_024;
const PHASES: [&str; 4] = ["parse", "lower", "plan", "emit"];
const REACH_QUERY: &str = r#"
  reachable("h0000").
  reachable(next) := reachable(current), *edge{from: current, to: next, kind: "call"}.
  ? reachable("h1023").
"#;
const PHASE_QUERY: &str = r#"
  phase_rank("parse", 0).
  phase_rank("lower", 1).
  phase_rank("plan", 2).
  phase_rank("emit", 3).
  phase_total(phase, rank, count) :=
    phase_rank(phase, rank),
    count = Count{ edge_id : *handle{id: source, status: phase}, *edge{native_id: edge_id, from: source, kind: "resolves"} }.
  ? phase_total(phase, rank, count).
"#;

fn identity(native_id: String) -> FactIdentity {
    FactIdentity::new(
        CorpusId::from("synthetic"),
        SourceName::from("code-population"),
        NativeId::from(native_id),
        OriginUri::from("synthetic://code-population"),
        Revision::from("r1"),
        Generation::initial(),
    )
}

fn handle_name(index: usize) -> String {
    format!("h{index:04}")
}

fn generate(edge_count: usize, mode: &str) -> FactBatch {
    let mut occurrences = HashMap::<String, usize>::new();
    let mut batch = FactBatch::new(
        CorpusId::from("synthetic"),
        SourceName::from("code-population"),
        FactBatchMode::FullSnapshot,
        Generation::initial(),
    );
    batch.handles.reserve(HANDLES);
    batch.edges.reserve(edge_count);
    for index in 0..HANDLES {
        let name = handle_name(index);
        batch.handles.push(HandleFact {
            identity: identity(format!("handle-{index}")),
            id: HandleId::new(name).expect("nonempty synthetic handle"),
            kind: "function".to_string(),
            status: Some(PHASES[index % PHASES.len()].to_string()),
            namespace: String::new(),
            file: "synthetic.rs".to_string(),
            line: 1,
            date: None,
            area: String::new(),
            summary: String::new(),
        });
    }
    let calls = edge_count / 8;
    let resolves = edge_count - calls;
    for index in 0..edge_count {
        let (source, target, kind) = if index < calls {
            let (source, target) = if index < HANDLES / 2 {
                (0, index + 1)
            } else if index < HANDLES - 1 {
                (1, index + 1)
            } else {
                (index % HANDLES, (index / HANDLES + 1) % HANDLES)
            };
            (source, target, "call")
        } else {
            let offset = index - calls;
            (offset % HANDLES, offset / HANDLES % HANDLES, "resolves")
        };
        let from = handle_name(source);
        let to = handle_name(target);
        let native_id = match mode {
            "old-code" => format!("{from}::edge::{index}::{kind}::{to}::1"),
            "tuple-code" => {
                // Same tuple, local sequence and JSON wrapping as CodeFactIds.
                use std::fmt::Write;
                let fields = (&from, kind, &to, "synthetic.rs", 1, "", "");
                let mut key = anneal_core::encode_native_id("edge", &fields)
                    .expect("primitive identity components");
                let occurrence = occurrences.entry(key.clone()).or_default();
                key.insert("edge:".len(), '[');
                write!(&mut key, ",{occurrence}]").expect("String write");
                *occurrence += 1;
                key
            }
            _ => format!("edge-{index}"),
        };
        batch.edges.push(EdgeFact {
            identity: identity(native_id),
            from: HandleId::new(from).expect("nonempty source"),
            to: HandleId::new(to).expect("nonempty target"),
            kind: kind.to_string(),
            file: "synthetic.rs".to_string(),
            line: 1,
            assertion_date: None,
            assertion_revision: None,
        });
    }
    assert_eq!(batch.edges.len(), calls + resolves);
    batch
}

fn real_identity(native_id: String, basis: &str) -> FactIdentity {
    FactIdentity::new(
        CorpusId::from("murail-lsif"),
        SourceName::from("lsif"),
        NativeId::from(native_id),
        OriginUri::from("lsif://murail"),
        Revision::from(basis.to_string()),
        Generation::initial(),
    )
}

fn real_handle(batch: &mut FactBatch, seen: &mut HashSet<String>, name: &str, basis: &str) {
    if !seen.insert(name.to_string()) {
        return;
    }
    let phase = name
        .bytes()
        .fold(0_usize, |sum, byte| sum + usize::from(byte))
        % PHASES.len();
    batch.handles.push(HandleFact {
        identity: real_identity(format!("handle-{name}"), basis),
        id: HandleId::new(name).expect("nonempty LSIF handle"),
        kind: "code".to_string(),
        status: Some(PHASES[phase].to_string()),
        namespace: String::new(),
        file: "murail.lsif".to_string(),
        line: 1,
        date: None,
        area: String::new(),
        summary: String::new(),
    });
}

fn required_str<'a>(row: &'a JsonValue, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    row.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing string {key} in {row}").into())
}

fn required_u64(row: &JsonValue, key: &str) -> Result<u64, Box<dyn std::error::Error>> {
    row.get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| format!("missing integer {key} in {row}").into())
}

struct ReadRows {
    count: usize,
    first_edge: Option<(String, String)>,
}

struct RealBatch {
    batch: FactBatch,
    entry: String,
    target: String,
    resolves: usize,
    refs: usize,
}

fn read_real_file(
    path: &Path,
    relation: &str,
    batch: &mut FactBatch,
    seen: &mut HashSet<String>,
    basis: &mut Option<String>,
) -> Result<ReadRows, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let header: JsonValue = serde_json::from_str(&lines.next().ok_or("empty NDJSON")??)?;
    if required_str(&header, "schema")? != "architecture-lens.rows.v1"
        || required_str(&header, "relation")? != relation
    {
        return Err(format!("unexpected {relation} schema in {}", path.display()).into());
    }
    let mut count = 0;
    let mut first = None;
    let mut fanout = std::collections::HashMap::<String, (usize, String)>::new();
    for line in lines {
        let row: JsonValue = serde_json::from_str(&line?)?;
        let row_basis = required_str(&row, "basis")?;
        if let Some(expected) = basis {
            if expected != row_basis {
                return Err(format!("mixed LSIF basis: {expected} and {row_basis}").into());
            }
        } else {
            *basis = Some(row_basis.to_string());
        }
        let (from, to, file, line) = if relation == "resolves" {
            let site_file = required_str(&row, "site_file")?;
            let site_line = required_u64(&row, "site_line")?;
            let from = format!("site:{site_file}:{site_line}");
            let to = format!("symbol:{}", required_str(&row, "def_symbol")?);
            (from, to, site_file, site_line)
        } else {
            let ref_file = required_str(&row, "ref_file")?;
            let ref_line = required_u64(&row, "ref_line")?;
            let from = format!("symbol:{}", required_str(&row, "def_symbol")?);
            let to = format!("site:{ref_file}:{ref_line}");
            (from, to, ref_file, ref_line)
        };
        real_handle(batch, seen, &from, row_basis);
        real_handle(batch, seen, &to, row_basis);
        if first.is_none() {
            first = Some((from.clone(), to.clone()));
        }
        if relation == "refs" {
            let entry = fanout.entry(from.clone()).or_insert((0, to.clone()));
            entry.0 += 1;
        }
        batch.edges.push(EdgeFact {
            identity: real_identity(format!("{relation}-{count}"), row_basis),
            from: HandleId::new(from)?,
            to: HandleId::new(to)?,
            kind: relation.to_string(),
            file: file.to_string(),
            line: u32::try_from(line + 1)?,
            assertion_date: None,
            assertion_revision: None,
        });
        count += 1;
    }
    if relation == "refs" {
        first = fanout
            .into_iter()
            .max_by(|left, right| left.1.0.cmp(&right.1.0).then_with(|| right.0.cmp(&left.0)))
            .map(|(from, (_, to))| (from, to));
    }
    Ok(ReadRows {
        count,
        first_edge: first,
    })
}

fn generate_real(directory: &Path) -> Result<RealBatch, Box<dyn std::error::Error>> {
    let mut batch = FactBatch::new(
        CorpusId::from("murail-lsif"),
        SourceName::from("lsif"),
        FactBatchMode::FullSnapshot,
        Generation::initial(),
    );
    let mut seen = HashSet::new();
    let mut basis = None;
    let resolves = read_real_file(
        &directory.join("resolves.ndjson"),
        "resolves",
        &mut batch,
        &mut seen,
        &mut basis,
    )?;
    let refs = read_real_file(
        &directory.join("refs.ndjson"),
        "refs",
        &mut batch,
        &mut seen,
        &mut basis,
    )?;
    let (entry, target) = refs.first_edge.ok_or("refs NDJSON contains no rows")?;
    Ok(RealBatch {
        batch,
        entry,
        target,
        resolves: resolves.count,
        refs: refs.count,
    })
}

fn query(
    database: Database,
    source: &str,
) -> Result<(Duration, usize, i64), Box<dyn std::error::Error>> {
    let program = parse_program("code-population-benchmark", source)?;
    let analyzed = analyze(program)?;
    let query = analyzed
        .queries()
        .next()
        .ok_or("benchmark query missing")?
        .clone();
    let start = Instant::now();
    let mut evaluator = Evaluator::new(analyzed, database);
    evaluator.run_fixpoint_for_query(&query)?;
    let output = evaluator.eval_query(&query)?;
    let elapsed = start.elapsed();
    let count_sum = output
        .rows
        .iter()
        .map(|row| match row.fields.get("count") {
            Some(Value::Number(NumberValue::Int(value))) => Ok(*value),
            None => Ok(0),
            other => Err(format!("unexpected count value: {other:?}")),
        })
        .sum::<Result<i64, _>>()?;
    Ok((elapsed, output.rows.len(), count_sum))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "dhat")]
    let _profiler = dhat::Profiler::new_heap();

    let extraction_start = Instant::now();
    let argument = std::env::args()
        .nth(1)
        .ok_or("pass edge count or --real DIR")?;
    let mode = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "ordinal".to_string());
    if argument != "--real" && !["ordinal", "old-code", "tuple-code"].contains(&mode.as_str()) {
        return Err("identity mode must be ordinal, old-code or tuple-code".into());
    }
    let (batch, reach_source, calls, resolves, refs) = if argument == "--real" {
        let directory = std::env::args().nth(2).ok_or("pass NDJSON directory")?;
        let real = generate_real(Path::new(&directory))?;
        let entry = serde_json::to_string(&real.entry)?;
        let target = serde_json::to_string(&real.target)?;
        let reach = format!(
            "reachable({entry}). reachable(next) := reachable(current), *edge{{from: current, to: next, kind: \"resolves\"}}. reachable(next) := reachable(current), *edge{{from: current, to: next, kind: \"refs\"}}. ? reachable({target}), count = Count{{ x : reachable(x) }}."
        );
        (real.batch, reach, 0, real.resolves, real.refs)
    } else {
        let edge_count: usize = argument.parse()?;
        if edge_count < 10_000 || !edge_count.is_multiple_of(8) {
            return Err("edge count must be at least 10000 and divisible by 8".into());
        }
        (
            generate(edge_count, &mode),
            REACH_QUERY.to_string(),
            edge_count / 8,
            edge_count - edge_count / 8,
            0,
        )
    };
    let id_bytes: usize = batch
        .edges
        .iter()
        .map(|row| row.identity.native_id.as_str().len())
        .sum();
    let mut store = FactStore::default();
    store.merge(batch)?;
    let database = Database::from_store(&store);
    let extraction = extraction_start.elapsed();
    let (reach, reach_rows, reach_count) = query(database.clone(), &reach_source)?;
    let (phase, phase_rows, phase_count_sum) = query(database, PHASE_QUERY)?;
    let expected_resolves = i64::try_from(resolves)?;
    if reach_rows != 1
        || (refs > 0 && reach_count <= 1)
        || phase_rows != PHASES.len()
        || phase_count_sum != expected_resolves
    {
        return Err(format!("unexpected query result: reach={reach_rows}, reach_count={reach_count}, phase={phase_rows}, resolves={phase_count_sum}").into());
    }
    println!(
        "identity_mode={mode} edge_id_bytes={id_bytes} edges={} calls={calls} resolves={resolves} refs={refs} handles={} extraction_s={:.3} reach_s={:.3} reach_rows={reach_rows} reach_count={reach_count} phase_s={:.3} phase_rows={phase_rows} phase_count_sum={phase_count_sum}",
        calls + resolves + refs,
        store.handles().len(),
        extraction.as_secs_f64(),
        reach.as_secs_f64(),
        phase.as_secs_f64(),
    );
    Ok(())
}
