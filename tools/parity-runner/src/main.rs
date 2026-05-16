//! Parity runner for anneal v2.0 migration gates.
//!
//! The harness can run v1.1 against itself for deterministic baselines, or run
//! v1.1 JSON surfaces against anneal-md fact-backed output and prelude queries.

use anneal_cli::{ReadCommand, SearchCommand};
use anneal_core::runtime::eval::NumberValue;
use anneal_core::runtime::prelude::standard_prelude_program;
use anneal_core::runtime::{
    Database, EvalOptions, Evaluator, QueryOutput, Row, Value as RuntimeValue, analyze,
    parse_program,
};
use anneal_core::{
    ActorContext, CancellationToken, ConfigFact, ConfigFacts, CorpusId, EdgeFact, FactBatch,
    FactBatchMode, FactIdentity, FactStore, Generation, HandleFact, MetaFact, NativeId, OriginUri,
    Revision, Source, SourceContext, SourceName, load_runtime_configs,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail};
use camino::Utf8PathBuf;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CORPUS: &str = "large-corpus";
const DEFAULT_LEFT_BIN: &str = "anneal";
const DEFAULT_RIGHT_BIN: &str = "anneal";
const DEFAULT_SAMPLE_SIZE: usize = 50;

#[derive(Debug)]
struct Config {
    corpus: String,
    left_bin: String,
    right_bin: String,
    right_mode: RightMode,
    sample_size: usize,
    report: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            corpus: DEFAULT_CORPUS.to_string(),
            left_bin: DEFAULT_LEFT_BIN.to_string(),
            right_bin: DEFAULT_RIGHT_BIN.to_string(),
            right_mode: RightMode::Cli,
            sample_size: DEFAULT_SAMPLE_SIZE,
            report: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RightMode {
    Cli,
    AnnealMdFacts,
}

impl RightMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "cli" => Ok(Self::Cli),
            "anneal-md" | "anneal-md-facts" => Ok(Self::AnnealMdFacts),
            other => bail!("unknown --right-mode {other:?}; expected cli or anneal-md"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::AnnealMdFacts => "anneal-md",
        }
    }
}

#[derive(Debug)]
struct CommandResult {
    stdout: JsonValue,
    stderr: String,
}

#[derive(Debug)]
struct FactRight {
    root: Utf8PathBuf,
    batch: FactBatch,
    configs: Vec<ConfigFact>,
}

#[derive(Debug)]
enum RightRunner {
    Cli { bin: String },
    AnnealMdFacts(Box<FactRight>),
}

impl RightRunner {
    fn build(config: &Config, corpus_root: &Path) -> Result<Self> {
        match config.right_mode {
            RightMode::Cli => Ok(Self::Cli {
                bin: config.right_bin.clone(),
            }),
            RightMode::AnnealMdFacts => Ok(Self::AnnealMdFacts(Box::new(extract_anneal_md(
                corpus_root,
            )?))),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Cli { bin } => bin.clone(),
            Self::AnnealMdFacts(_) => "anneal-md facts".to_string(),
        }
    }

    const fn is_fact_backed(&self) -> bool {
        matches!(self, Self::AnnealMdFacts(_))
    }

    fn run(&self, corpus_root: &Path, args: &[&str]) -> Result<CommandResult> {
        match self {
            Self::Cli { bin } => run_anneal(bin, corpus_root, args),
            Self::AnnealMdFacts(facts) => facts.run(args),
        }
    }
}

impl FactRight {
    fn run(&self, args: &[&str]) -> Result<CommandResult> {
        let stdout = match args {
            ["health", "--json"] => {
                anneal_legacy::v2_adapter::health_json_from_facts(self.root.as_path(), &self.batch)?
            }
            ["check", "--scope=active", "--json"] => {
                anneal_legacy::v2_adapter::check_json_from_facts(self.root.as_path(), &self.batch)?
            }
            ["get", handle, "--refs", "--json"] => {
                anneal_legacy::v2_adapter::get_refs_json_from_facts(
                    self.root.as_path(),
                    &self.batch,
                    handle,
                )?
            }
            _ => bail!(
                "anneal-md fact runner does not implement {}",
                command_string(args)
            ),
        };
        Ok(CommandResult {
            stdout,
            stderr: String::new(),
        })
    }

    fn eval_query(&self, query: &str) -> Result<QueryOutput> {
        let mut store = FactStore::default();
        store
            .merge(self.batch.clone())
            .context("failed to merge anneal-md facts into runtime store")?;
        if !self.configs.is_empty() {
            store
                .replace_configs(&self.batch.corpus, self.configs.clone())
                .context("failed to merge runtime config facts")?;
        }
        let database = Database::from_store(&store);
        let program = parse_program("parity-phase4", query)?;
        let analyzed = analyze(program)?;
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .context("phase4 parity query did not contain a query")?;
        let mut evaluator = Evaluator::with_options(analyzed, database, EvalOptions::default());
        evaluator
            .run_fixpoint()
            .context("phase4 parity fixpoint failed")?;
        evaluator
            .eval_query(&query)
            .context("phase4 parity query failed")
    }

    fn eval_prelude_query(&self, query: &str) -> Result<QueryOutput> {
        let mut store = FactStore::default();
        store
            .merge(self.batch.clone())
            .context("failed to merge anneal-md facts into runtime store")?;
        if !self.configs.is_empty() {
            store
                .replace_configs(&self.batch.corpus, self.configs.clone())
                .context("failed to merge runtime config facts")?;
        }
        let database = Database::from_store(&store);
        let mut program = standard_prelude_program().context("standard prelude did not parse")?;
        let query = parse_program("parity-phase6", query)?;
        program.statements.extend(query.statements);
        let analyzed = analyze(program)?;
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .context("phase6 parity query did not contain a query")?;
        let mut evaluator = Evaluator::with_options(analyzed, database, EvalOptions::default());
        evaluator
            .run_fixpoint()
            .context("phase6 parity fixpoint failed")?;
        evaluator
            .eval_query(&query)
            .context("phase6 parity query failed")
    }
}

#[derive(Debug)]
struct CaseResult {
    id: &'static str,
    label: &'static str,
    command: String,
    passed: bool,
    detail: String,
    left_stderr: String,
    right_stderr: String,
}

fn main() -> Result<()> {
    let config = parse_args(env::args().skip(1))?;
    let repo = repo_root()?;
    let corpus_root = resolve_corpus(&repo, &config.corpus)?;
    let report_path = config.report.clone().unwrap_or_else(|| {
        repo.join(".fixtures").join(format!(
            "parity-2026-05-13-{}.json",
            sanitize(&config.corpus)
        ))
    });

    println!("corpus: {}", corpus_root.display());
    println!("left:   {}", config.left_bin);
    let right_runner = RightRunner::build(&config, &corpus_root)?;
    println!("right:  {}", right_runner.label());

    let mut cases = Vec::new();
    cases.push(run_case(
        "PD-1",
        "health",
        &config.left_bin,
        &right_runner,
        &corpus_root,
        &["health", "--json"],
    )?);
    cases.push(run_case(
        "PD-2",
        "check",
        &config.left_bin,
        &right_runner,
        &corpus_root,
        &["check", "--scope=active", "--json"],
    )?);

    let sample_handles = sample_handles(&config.left_bin, &corpus_root, config.sample_size)?;
    cases.push(run_pd3(
        &config.left_bin,
        &right_runner,
        &corpus_root,
        &sample_handles,
    )?);
    if let RightRunner::AnnealMdFacts(facts) = &right_runner {
        cases.extend(run_phase4_cases(facts, &config.left_bin, &corpus_root)?);
    }

    for case in &cases {
        let status = if case.passed { "ok" } else { "fail" };
        println!(
            "running {:<4} {:<12} ... {status:<4} ({})",
            case.id, case.label, case.detail
        );
    }

    let failures = cases.iter().filter(|case| !case.passed).count();
    let passes = cases.len() - failures;
    println!("result: {failures} fail, {passes} pass");
    println!("report: {}", report_path.display());

    write_report(&report_path, &config, &corpus_root, &sample_handles, &cases)?;

    if failures == 0 {
        Ok(())
    } else {
        bail!("parity run failed: {failures} failing case(s)")
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config> {
    let mut config = Config::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let (key, value) = if let Some((k, v)) = arg.split_once('=') {
            (k.to_string(), Some(v.to_string()))
        } else {
            (arg, None)
        };
        match key.as_str() {
            "--corpus" => config.corpus = value_or_next("--corpus", value, &mut iter)?,
            "--left-bin" => config.left_bin = value_or_next("--left-bin", value, &mut iter)?,
            "--right-bin" => config.right_bin = value_or_next("--right-bin", value, &mut iter)?,
            "--right-mode" => {
                config.right_mode =
                    RightMode::parse(&value_or_next("--right-mode", value, &mut iter)?)?;
            }
            "--sample-size" => {
                config.sample_size = value_or_next("--sample-size", value, &mut iter)?
                    .parse()
                    .context("--sample-size must be a positive integer")?;
            }
            "--report" => {
                config.report = Some(PathBuf::from(value_or_next("--report", value, &mut iter)?));
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?}; pass --help for usage"),
        }
    }
    if config.sample_size == 0 {
        bail!("--sample-size must be greater than zero");
    }
    Ok(config)
}

fn value_or_next(
    flag: &'static str,
    value: Option<String>,
    iter: &mut impl Iterator<Item = String>,
) -> Result<String> {
    value.map_or_else(
        || {
            iter.next()
                .with_context(|| format!("{flag} requires a value"))
        },
        Ok,
    )
}

fn print_help() {
    println!(
        "Usage: parity-runner [--corpus large-corpus|PATH] [--left-bin anneal] \\
         [--right-bin anneal] [--right-mode cli|anneal-md] \\
         [--sample-size 50] [--report PATH]"
    );
}

fn repo_root() -> Result<PathBuf> {
    let mut dir = env::current_dir().context("failed to read current directory")?;
    loop {
        if dir.join(".git").exists() && dir.join(".fixtures").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not locate repo root with .git and .fixtures");
        }
    }
}

fn resolve_corpus(repo: &Path, corpus: &str) -> Result<PathBuf> {
    let path = if corpus == "large-corpus" {
        repo.join(".fixtures/sample-corpus")
    } else {
        PathBuf::from(corpus)
    };
    if path.join("anneal.toml").exists() {
        Ok(path)
    } else {
        bail!(
            "corpus root {} does not contain anneal.toml",
            path.display()
        )
    }
}

fn run_case(
    id: &'static str,
    label: &'static str,
    left_bin: &str,
    right: &RightRunner,
    corpus_root: &Path,
    args: &[&str],
) -> Result<CaseResult> {
    let (left, right) = if right.is_fact_backed() {
        let right_result = right.run(corpus_root, args)?;
        let left_result = run_anneal(left_bin, corpus_root, args)?;
        (left_result, right_result)
    } else {
        let left_result = run_anneal(left_bin, corpus_root, args)?;
        let right_result = right.run(corpus_root, args)?;
        (left_result, right_result)
    };
    let json_matches = left.stdout == right.stdout;
    let stderr_matches = left.stderr == right.stderr;
    let passed = json_matches && stderr_matches;
    let detail = if !json_matches {
        first_json_diff(&left.stdout, &right.stdout)
    } else if !stderr_matches {
        format!(
            "stderr differs: left={} bytes right={} bytes",
            left.stderr.len(),
            right.stderr.len()
        )
    } else {
        "identical-to-spec".to_string()
    };
    Ok(CaseResult {
        id,
        label,
        command: command_string(args),
        passed,
        detail,
        left_stderr: left.stderr,
        right_stderr: right.stderr,
    })
}

fn run_pd3(
    left_bin: &str,
    right: &RightRunner,
    corpus_root: &Path,
    sample_handles: &[String],
) -> Result<CaseResult> {
    let mut failures = Vec::new();
    for handle in sample_handles {
        let args = ["get", handle.as_str(), "--refs", "--json"];
        let left = run_anneal(left_bin, corpus_root, &args)?;
        let right = right.run(corpus_root, &args)?;
        if left.stdout != right.stdout || left.stderr != right.stderr {
            failures.push(json!({
                "handle": handle,
                "stdout_diff": (left.stdout != right.stdout)
                    .then(|| first_json_diff(&left.stdout, &right.stdout)),
                "stderr_diff": (left.stderr != right.stderr).then(|| json!({
                    "left_bytes": left.stderr.len(),
                    "right_bytes": right.stderr.len(),
                })),
            }));
        }
    }
    let passed = failures.is_empty();
    let detail = if passed {
        format!("identical-to-spec: {} handles", sample_handles.len())
    } else {
        format!("{} handle mismatch(es)", failures.len())
    };
    Ok(CaseResult {
        id: "PD-3",
        label: "get x50",
        command: "anneal --root <corpus> get <handle> --refs --json".to_string(),
        passed,
        detail,
        left_stderr: String::new(),
        right_stderr: serde_json::to_string(&failures)?,
    })
}

fn run_phase4_cases(
    right: &FactRight,
    left_bin: &str,
    corpus_root: &Path,
) -> Result<Vec<CaseResult>> {
    let search = run_pd4_search(right)?;
    let read = run_pd5_read(right)?;
    let diagnostics = run_pd6_diagnostics(right, left_bin, corpus_root)?;
    let catalog = run_pd6_catalog_fixture()?;
    Ok(vec![search, read, diagnostics, catalog])
}

fn run_pd4_search(right: &FactRight) -> Result<CaseResult> {
    const EXPECTED_HANDLE: &str = "reviews/2026-04-28-formal-model-v17-conformance-audit.md";
    let command = "anneal search \"v17 conformance audit\" --limit=3".to_string();
    let query = SearchCommand::new("v17 conformance audit")
        .with_limit(3)
        .datalog();
    let output = right.eval_query(&query)?;
    let expected = output.rows.iter().find(|row| {
        row_string(row, "h") == Some(EXPECTED_HANDLE)
            && row_f64(row, "score").is_some_and(|score| score > 0.7)
            && row_string(row, "reason").is_some_and(search_reason_allowed)
            && row_bool(row, "low_confidence") == Some(false)
    });
    let passed = expected.is_some();
    let detail = if passed {
        format!("top-3 includes {EXPECTED_HANDLE}")
    } else {
        format!(
            "top-3 missing expected audit handle; rows={}",
            serde_json::to_string(&output.rows)?
        )
    };
    Ok(CaseResult {
        id: "PD-4",
        label: "search",
        command,
        passed,
        detail,
        left_stderr: String::new(),
        right_stderr: String::new(),
    })
}

fn run_pd5_read(right: &FactRight) -> Result<CaseResult> {
    const EXPECTED_HANDLE: &str = "reviews/2026-04-28-formal-model-v17-conformance-audit.md";
    let command = format!("anneal read {EXPECTED_HANDLE} --budget=4000");
    let query = ReadCommand::new(EXPECTED_HANDLE)
        .with_budget(4_000)
        .datalog();
    let output = right.eval_query(&query)?;
    let first_text = output
        .rows
        .first()
        .and_then(|row| row_string(row, "text"))
        .unwrap_or_default();
    let passed = first_text.contains("## Method") || first_text.contains("## Summary");
    let detail = if passed {
        "first span includes Method or Summary".to_string()
    } else {
        format!(
            "first span missing Method/Summary; rows={}",
            serde_json::to_string(&output.rows)?
        )
    };
    Ok(CaseResult {
        id: "PD-5",
        label: "read",
        command,
        passed,
        detail,
        left_stderr: String::new(),
        right_stderr: String::new(),
    })
}

fn run_pd6_diagnostics(
    right: &FactRight,
    left_bin: &str,
    corpus_root: &Path,
) -> Result<CaseResult> {
    let command = "anneal --root <corpus> check --scope=all --json --full".to_string();
    let left = run_anneal(
        left_bin,
        corpus_root,
        &["check", "--scope=all", "--json", "--full"],
    )?;
    let expected = check_relation_rows(&left.stdout)?;
    let output = right.eval_prelude_query(
        r"
        ? diagnostic(code, severity, subject, file, line, evidence).
        ",
    )?;
    let actual = diagnostic_relation_rows(&output.rows)?;
    diagnostic_case_result(DiagnosticCase {
        id: "PD-6",
        label: "checks.dl",
        command,
        success_prefix: "diagnostic relation rows match",
        failure_prefix: "diagnostic relation rows differ",
        expected_label: "left",
        actual_label: "right",
        expected,
        actual,
        raw_actual_rows: output.rows,
    })
}

fn run_pd6_catalog_fixture() -> Result<CaseResult> {
    let right = load_checks_catalog_fixture()?;
    let output = right.eval_prelude_query(
        r"
        ? diagnostic(code, severity, subject, file, line, evidence).
        ",
    )?;
    let actual = diagnostic_relation_rows(&output.rows)?;
    let expected = right.expected_diagnostics()?;
    diagnostic_case_result(DiagnosticCase {
        id: "PD-6b",
        label: "checks catalog",
        command: "anneal prelude diagnostic(...) over .fixtures/checks-catalog".to_string(),
        success_prefix: "catalog diagnostic relation rows match",
        failure_prefix: "catalog diagnostic relation rows differ",
        expected_label: "expected",
        actual_label: "actual",
        expected,
        actual,
        raw_actual_rows: output.rows,
    })
}

type RowMultiset = BTreeMap<String, usize>;

struct DiagnosticCase {
    id: &'static str,
    label: &'static str,
    command: String,
    success_prefix: &'static str,
    failure_prefix: &'static str,
    expected_label: &'static str,
    actual_label: &'static str,
    expected: RowMultiset,
    actual: RowMultiset,
    raw_actual_rows: Vec<Row>,
}

fn diagnostic_case_result(case: DiagnosticCase) -> Result<CaseResult> {
    let passed = case.actual == case.expected;
    let detail = if passed {
        format!("{}: {}", case.success_prefix, format_counts(&case.actual)?)
    } else {
        format!(
            "{}: {}",
            case.failure_prefix,
            first_multiset_diff(&case.expected, &case.actual)
        )
    };
    Ok(CaseResult {
        id: case.id,
        label: case.label,
        command: case.command,
        passed,
        detail,
        left_stderr: String::new(),
        right_stderr: if passed {
            String::new()
        } else {
            serde_json::to_string(&json!({
                format!("{}_counts", case.expected_label): format_counts(&case.expected)?,
                format!("{}_counts", case.actual_label): format_counts(&case.actual)?,
                format!("{}_rows", case.expected_label): multiset_rows(&case.expected),
                format!("{}_rows", case.actual_label): multiset_rows(&case.actual),
                format!("raw_{}_rows", case.actual_label): case.raw_actual_rows,
            }))?
        },
    })
}

impl FactRight {
    fn expected_diagnostics(&self) -> Result<RowMultiset> {
        let expected = self.root.join("expected-diagnostics.json");
        let rows: Vec<ExpectedDiagnostic> = serde_json::from_slice(
            &fs::read(&expected).with_context(|| format!("failed to read {expected}"))?,
        )
        .with_context(|| format!("failed to parse {expected}"))?;
        let mut out = BTreeMap::new();
        for row in rows {
            insert_multiset(&mut out, row.canonical()?);
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
struct ExpectedDiagnostic {
    code: String,
    severity: String,
    subject: JsonValue,
    file: JsonValue,
    line: JsonValue,
    evidence: JsonValue,
}

impl ExpectedDiagnostic {
    fn canonical(&self) -> Result<String> {
        canonical_diagnostic_row(
            &self.code,
            &self.severity,
            &self.subject,
            &self.file,
            &self.line,
            &self.evidence,
        )
    }
}

fn check_relation_rows(output: &JsonValue) -> Result<RowMultiset> {
    let diagnostics = output
        .get("diagnostics")
        .and_then(JsonValue::as_array)
        .context("check JSON missing diagnostics array; use --full for PD-6")?;
    let mut rows = BTreeMap::new();
    for diagnostic in diagnostics {
        let code = diagnostic
            .get("code")
            .and_then(JsonValue::as_str)
            .context("diagnostic missing code")?;
        let severity = diagnostic
            .get("severity")
            .and_then(JsonValue::as_str)
            .context("diagnostic missing severity")?;
        let message = diagnostic
            .get("message")
            .and_then(JsonValue::as_str)
            .context("diagnostic missing message")?;
        let corpus_scoped = is_corpus_scoped_diagnostic(code);
        // CR-D69/CR-D50: these are corpus- or status-scoped relation rows.
        // v1.x JSON may carry representative display files, so PD-6 compares
        // the semantic diagnostic tuple rather than the legacy display anchor.
        let file = if corpus_scoped {
            JsonValue::Null
        } else {
            diagnostic.get("file").cloned().unwrap_or(JsonValue::Null)
        };
        let line = if corpus_scoped {
            JsonValue::Null
        } else {
            diagnostic.get("line").cloned().unwrap_or(JsonValue::Null)
        };
        let evidence = diagnostic.get("evidence").unwrap_or(&JsonValue::Null);
        let subject = left_diagnostic_subject(code, message, &file, evidence)?;
        let evidence = left_diagnostic_evidence(code, message, evidence)?;
        insert_multiset(
            &mut rows,
            canonical_diagnostic_row(code, severity, &subject, &file, &line, &evidence)?,
        );
    }
    Ok(rows)
}

fn is_corpus_scoped_diagnostic(code: &str) -> bool {
    matches!(code, "I001" | "S003" | "S005")
}

fn diagnostic_relation_rows(rows: &[Row]) -> Result<RowMultiset> {
    let mut out = BTreeMap::new();
    for row in rows {
        let code = row_string(row, "code").context("diagnostic row missing string code")?;
        let severity =
            row_string(row, "severity").context("diagnostic row missing string severity")?;
        let subject = row_json(row, "subject").context("diagnostic row missing subject")?;
        let file = row_json(row, "file").context("diagnostic row missing file")?;
        let line = row_json(row, "line").context("diagnostic row missing line")?;
        let evidence = row
            .fields
            .get("evidence")
            .map(|value| runtime_diagnostic_evidence(code, value))
            .context("diagnostic row missing evidence")??;
        insert_multiset(
            &mut out,
            canonical_diagnostic_row(code, severity, &subject, &file, &line, &evidence)?,
        );
    }
    Ok(out)
}

fn insert_multiset(rows: &mut RowMultiset, row: String) {
    *rows.entry(row).or_default() += 1;
}

fn canonical_diagnostic_row(
    code: &str,
    severity: &str,
    subject: &JsonValue,
    file: &JsonValue,
    line: &JsonValue,
    evidence: &JsonValue,
) -> Result<String> {
    serde_json::to_string(&json!({
        "code": code,
        "severity": severity,
        "subject": subject,
        "file": file,
        "line": line,
        "evidence": evidence,
    }))
    .context("failed to serialize canonical diagnostic row")
}

fn left_diagnostic_subject(
    code: &str,
    message: &str,
    file: &JsonValue,
    evidence: &JsonValue,
) -> Result<JsonValue> {
    let subject = match code {
        "I001" => "corpus".to_string(),
        "E001" | "W003" | "W004" => file.as_str().unwrap_or_default().to_string(),
        "W001" => parse_between(message, "stale dependency: ", " (active)")
            .context("W001 message missing source subject")?
            .to_string(),
        "W002" => parse_until_status(message, "confidence gap: ")
            .context("W002 message missing source subject")?,
        "E002" => parse_between(message, "undischarged obligation: ", " has no")
            .context("E002 message missing obligation subject")?
            .to_string(),
        "I002" => parse_between(message, "multiple discharges: ", " discharged")
            .context("I002 message missing obligation subject")?
            .to_string(),
        "S001" => evidence_string(evidence, "handle").context("S001 evidence missing handle")?,
        "S002" | "S004" => {
            evidence_string(evidence, "prefix").context("namespace evidence missing prefix")?
        }
        "S003" => evidence_string(evidence, "status").context("S003 evidence missing status")?,
        "S005" => {
            evidence_string(evidence, "left_prefix").context("S005 evidence missing left_prefix")?
        }
        other => bail!("unknown diagnostic code in PD-6 left output: {other}"),
    };
    Ok(JsonValue::String(subject))
}

fn left_diagnostic_evidence(code: &str, message: &str, evidence: &JsonValue) -> Result<JsonValue> {
    match code {
        "I001" => Ok(json!(["section_refs", parse_leading_usize(message)?])),
        "E001" => Ok(json!([
            "broken_ref",
            evidence_string(evidence, "target").context("E001 evidence missing target")?
        ])),
        "E002" => Ok(JsonValue::String("undischarged".to_string())),
        "I002" => Ok(json!([
            "multiple_discharges",
            parse_between(message, " discharged ", " times")
                .context("I002 message missing discharge count")?
                .parse::<usize>()
                .context("I002 discharge count was not a number")?
        ])),
        "W001" => Ok(json!([
            "stale_ref",
            evidence_string(evidence, "source_status").context("W001 evidence missing source")?,
            evidence_string(evidence, "target_status").context("W001 evidence missing target")?
        ])),
        "W002" => Ok(json!([
            "confidence_gap",
            evidence_string(evidence, "source_status").context("W002 evidence missing source")?,
            evidence_usize(evidence, "source_level")
                .context("W002 evidence missing source level")?,
            evidence_string(evidence, "target_status").context("W002 evidence missing target")?,
            evidence_usize(evidence, "target_level")
                .context("W002 evidence missing target level")?
        ])),
        "W003" => Ok(JsonValue::Null),
        "W004" => Ok(json!([
            "implausible_ref",
            evidence_string(evidence, "value").context("W004 evidence missing value")?,
            evidence_string(evidence, "reason").context("W004 evidence missing reason")?,
            evidence.get("line").cloned().unwrap_or(JsonValue::Null)
        ])),
        "S001" => Ok(json!([
            "orphaned_handle",
            evidence_string(evidence, "handle").context("S001 evidence missing handle")?
        ])),
        "S002" => Ok(json!([
            "candidate_namespace",
            evidence_string(evidence, "prefix").context("S002 evidence missing prefix")?,
            evidence_usize(evidence, "count").context("S002 evidence missing count")?
        ])),
        "S003" => Ok(json!([
            "pipeline_stall",
            evidence_string(evidence, "status").context("S003 evidence missing status")?,
            evidence_usize(evidence, "count").context("S003 evidence missing count")?,
            evidence
                .get("next_status")
                .cloned()
                .unwrap_or(JsonValue::Null),
            evidence
                .get("based_on_history")
                .and_then(JsonValue::as_bool)
                .context("S003 evidence missing history flag")?
        ])),
        "S004" => Ok(json!([
            "abandoned_namespace",
            evidence_string(evidence, "prefix").context("S004 evidence missing prefix")?,
            evidence_usize(evidence, "member_count")
                .context("S004 evidence missing member count")?,
            evidence_usize(evidence, "terminal_members")
                .context("S004 evidence missing terminal count")?,
            evidence_usize(evidence, "stale_members")
                .context("S004 evidence missing stale count")?
        ])),
        "S005" => Ok(json!([
            "concern_group_candidate",
            evidence_string(evidence, "left_prefix")
                .context("S005 evidence missing left prefix")?,
            evidence_string(evidence, "right_prefix")
                .context("S005 evidence missing right prefix")?,
            evidence_usize(evidence, "file_count").context("S005 evidence missing file count")?
        ])),
        other => bail!("unknown diagnostic code in PD-6 left evidence: {other}"),
    }
}

fn runtime_diagnostic_evidence(code: &str, value: &RuntimeValue) -> Result<JsonValue> {
    let value = runtime_value_to_json(value);
    if code != "W004" {
        return Ok(value);
    }
    let Some(items) = value.as_array() else {
        return Ok(value);
    };
    let [JsonValue::String(kind), JsonValue::String(payload)] = items.as_slice() else {
        return Ok(value);
    };
    if kind != "implausible_ref" {
        return Ok(value);
    }
    let parsed: JsonValue =
        serde_json::from_str(payload).context("W004 md.implausible_ref payload was not JSON")?;
    Ok(json!([
        "implausible_ref",
        parsed.get("value").cloned().unwrap_or(JsonValue::Null),
        parsed.get("reason").cloned().unwrap_or(JsonValue::Null),
        parsed.get("line").cloned().unwrap_or(JsonValue::Null)
    ]))
}

fn runtime_value_to_json(value: &RuntimeValue) -> JsonValue {
    match value {
        RuntimeValue::String(value) => JsonValue::String(value.clone()),
        RuntimeValue::Number(NumberValue::Int(value)) => json!(value),
        RuntimeValue::Number(NumberValue::Float(value)) => {
            serde_json::Number::from_f64(*value).map_or(JsonValue::Null, JsonValue::Number)
        }
        RuntimeValue::Bool(value) => JsonValue::Bool(*value),
        RuntimeValue::Null => JsonValue::Null,
        RuntimeValue::List(items) => {
            JsonValue::Array(items.iter().map(runtime_value_to_json).collect())
        }
    }
}

fn row_json(row: &Row, field: &str) -> Option<JsonValue> {
    row.fields.get(field).map(runtime_value_to_json)
}

fn evidence_string(evidence: &JsonValue, field: &str) -> Option<String> {
    evidence
        .get(field)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn evidence_usize(evidence: &JsonValue, field: &str) -> Option<usize> {
    evidence
        .get(field)
        .and_then(JsonValue::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn parse_leading_usize(message: &str) -> Result<usize> {
    message
        .split_whitespace()
        .next()
        .context("message is empty")?
        .parse()
        .context("leading diagnostic count was not a number")
}

fn parse_between<'a>(message: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    message
        .strip_prefix(prefix)
        .and_then(|rest| rest.split_once(suffix))
        .map(|(value, _)| value)
}

fn parse_until_status(message: &str, prefix: &str) -> Option<String> {
    let rest = message.strip_prefix(prefix)?;
    let (subject, _) = rest.split_once(" (")?;
    Some(subject.to_string())
}

fn format_counts(rows: &RowMultiset) -> Result<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (row, count) in rows {
        let value: JsonValue = serde_json::from_str(row).context("canonical row was not JSON")?;
        let code = value
            .get("code")
            .and_then(JsonValue::as_str)
            .context("canonical row missing code")?;
        *counts.entry(code.to_string()).or_default() += count;
    }
    Ok(counts
        .iter()
        .map(|(code, count)| format!("{code}={count}"))
        .collect::<Vec<_>>()
        .join(","))
}

fn first_multiset_diff(left: &RowMultiset, right: &RowMultiset) -> String {
    let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
    for key in keys {
        let left_count = left.get(key.as_str()).copied().unwrap_or_default();
        let right_count = right.get(key.as_str()).copied().unwrap_or_default();
        if left_count != right_count {
            return format!("row={key} left_count={left_count} right_count={right_count}");
        }
    }
    "row multisets differ but no differing row was found".to_string()
}

fn multiset_rows(rows: &RowMultiset) -> Vec<JsonValue> {
    let mut out = Vec::new();
    for (row, count) in rows {
        let value = serde_json::from_str(row).unwrap_or_else(|_| JsonValue::String(row.clone()));
        for _ in 0..*count {
            out.push(value.clone());
        }
    }
    out
}

fn search_reason_allowed(reason: &str) -> bool {
    matches!(
        reason,
        "identifier-substring"
            | "title-substring"
            | "frontmatter-key-match"
            | "frontmatter-value-match"
    )
}

fn row_string<'a>(row: &'a Row, field: &str) -> Option<&'a str> {
    match row.fields.get(field) {
        Some(RuntimeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn row_bool(row: &Row, field: &str) -> Option<bool> {
    match row.fields.get(field) {
        Some(RuntimeValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn row_f64(row: &Row, field: &str) -> Option<f64> {
    match row.fields.get(field) {
        Some(RuntimeValue::Number(NumberValue::Int(value))) => value.to_string().parse().ok(),
        Some(RuntimeValue::Number(NumberValue::Float(value))) => Some(*value),
        _ => None,
    }
}

fn extract_anneal_md(corpus_root: &Path) -> Result<FactRight> {
    let root = Utf8PathBuf::from_path_buf(corpus_root.to_path_buf())
        .map_err(|path| anyhow!("corpus path is not valid UTF-8: {}", path.display()))?;
    let roots = vec![root.clone()];
    let corpus = CorpusId::from("parity");
    let config_facts = ConfigFacts::new(vec![
        ("md.file_extension".to_string(), ".md".to_string()),
        ("md.scan_root".to_string(), ".".to_string()),
    ]);
    let context = SourceContext {
        corpus: corpus.clone(),
        roots: roots.as_slice(),
        config_facts: &config_facts,
        time_ref: None,
        previous_generation: Some(Generation::new(0)),
        actor: ActorContext::anonymous_cli(),
        cancellation: CancellationToken::new(),
    };
    let batch = MarkdownSource
        .extract(&context)
        .map_err(|err| anyhow!("anneal-md extraction failed: {err}"))?;
    let configs = load_runtime_configs(&root, &corpus)?;
    Ok(FactRight {
        root,
        batch,
        configs,
    })
}

#[derive(Debug, Deserialize)]
struct ChecksCatalogFixture {
    handles: Vec<FixtureHandle>,
    #[serde(default)]
    edges: Vec<FixtureEdge>,
    #[serde(default)]
    meta: Vec<FixtureMeta>,
    #[serde(default)]
    configs: Vec<FixtureConfig>,
}

#[derive(Debug, Deserialize)]
struct FixtureHandle {
    id: String,
    kind: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    namespace: String,
    file: String,
    #[serde(default = "default_line")]
    line: u32,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    area: String,
    #[serde(default)]
    summary: String,
}

#[derive(Debug, Deserialize)]
struct FixtureEdge {
    from: String,
    to: String,
    kind: String,
    file: String,
    #[serde(default = "default_line")]
    line: u32,
}

#[derive(Debug, Deserialize)]
struct FixtureMeta {
    handle: String,
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct FixtureConfig {
    key: String,
    value: String,
    #[serde(default)]
    ordinal: Option<u32>,
}

fn default_line() -> u32 {
    1
}

fn load_checks_catalog_fixture() -> Result<FactRight> {
    let repo = repo_root()?;
    let root =
        Utf8PathBuf::from_path_buf(repo.join(".fixtures/checks-catalog")).map_err(|path| {
            anyhow!(
                "checks catalog fixture path is not valid UTF-8: {}",
                path.display()
            )
        })?;
    let facts_path = root.join("facts.json");
    let fixture: ChecksCatalogFixture = serde_json::from_slice(
        &fs::read(&facts_path).with_context(|| format!("failed to read {facts_path}"))?,
    )
    .with_context(|| format!("failed to parse {facts_path}"))?;

    let corpus = CorpusId::from("checks-catalog");
    let source = SourceName::from("checks-catalog");
    let generation = Generation::initial();
    let mut batch = FactBatch::new(
        corpus.clone(),
        source.clone(),
        FactBatchMode::FullSnapshot,
        generation,
    );
    let scope = FixtureFactScope {
        corpus: &corpus,
        source: &source,
        generation,
    };

    batch.handles = fixture
        .handles
        .into_iter()
        .map(|handle| HandleFact {
            identity: fixture_identity(&scope, &format!("handle:{}", handle.id)),
            id: handle.id,
            kind: handle.kind,
            status: handle.status,
            namespace: handle.namespace,
            file: handle.file,
            line: handle.line,
            date: handle.date,
            area: handle.area,
            summary: handle.summary,
        })
        .collect();
    batch.edges = fixture
        .edges
        .into_iter()
        .map(|edge| EdgeFact {
            identity: fixture_identity(
                &scope,
                &format!("edge:{}:{}:{}", edge.from, edge.kind, edge.to),
            ),
            from: edge.from,
            to: edge.to,
            kind: edge.kind,
            file: edge.file,
            line: edge.line,
        })
        .collect();
    batch.meta = fixture
        .meta
        .into_iter()
        .map(|meta| MetaFact {
            identity: fixture_identity(
                &scope,
                &format!("meta:{}:{}:{}", meta.handle, meta.key, meta.value),
            ),
            handle: meta.handle,
            key: meta.key,
            value: meta.value,
        })
        .collect();
    let configs = fixture
        .configs
        .into_iter()
        .map(|config| ConfigFact {
            corpus: corpus.clone(),
            key: config.key,
            value: config.value,
            ordinal: config.ordinal,
        })
        .collect();

    Ok(FactRight {
        root,
        batch,
        configs,
    })
}

struct FixtureFactScope<'a> {
    corpus: &'a CorpusId,
    source: &'a SourceName,
    generation: Generation,
}

fn fixture_identity(scope: &FixtureFactScope<'_>, native_id: &str) -> FactIdentity {
    FactIdentity::new(
        scope.corpus.clone(),
        scope.source.clone(),
        NativeId::from(native_id),
        OriginUri::from(format!("fixture://checks-catalog/{native_id}")),
        Revision::from("checks-catalog"),
        scope.generation,
    )
}

fn run_anneal(bin: &str, corpus_root: &Path, args: &[&str]) -> Result<CommandResult> {
    let sandbox = isolated_command_sandbox(args);
    let state_home = sandbox.join("state");
    let config_home = sandbox.join("config");
    fs::create_dir_all(&state_home)
        .with_context(|| format!("failed to create {}", state_home.display()))?;
    fs::create_dir_all(&config_home)
        .with_context(|| format!("failed to create {}", config_home.display()))?;

    let output_result = Command::new(bin)
        .arg("--root")
        .arg(corpus_root)
        .args(args)
        .env("XDG_STATE_HOME", &state_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .output();
    let _ = fs::remove_dir_all(&sandbox);
    let output =
        output_result.with_context(|| format!("failed to spawn {}", command_string(args)))?;
    if !output.status.success() {
        bail!(
            "{} exited {}: {}",
            command_string(args),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout: JsonValue = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse JSON from {}", command_string(args)))?;
    Ok(CommandResult { stdout, stderr })
}

fn isolated_command_sandbox(args: &[&str]) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!(
        "anneal-parity-{}-{}-{nanos}",
        std::process::id(),
        sanitize(&args.join("-"))
    ))
}

fn sample_handles(bin: &str, corpus_root: &Path, sample_size: usize) -> Result<Vec<String>> {
    let out = run_anneal(bin, corpus_root, &["query", "handles", "--json", "--full"])?;
    let items = out
        .stdout
        .get("items")
        .and_then(JsonValue::as_array)
        .context("query handles output missing items array")?;
    let ids = items
        .iter()
        .filter_map(|item| item.get("id").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if ids.len() < sample_size {
        bail!("only {} handles available; need {sample_size}", ids.len());
    }
    Ok(ids.into_iter().take(sample_size).collect())
}

fn first_json_diff(left: &JsonValue, right: &JsonValue) -> String {
    let left = serde_json::to_string_pretty(left).expect("serializing JSON value cannot fail");
    let right = serde_json::to_string_pretty(right).expect("serializing JSON value cannot fail");
    for (index, (l, r)) in left.lines().zip(right.lines()).enumerate() {
        if l != r {
            return format!("first diff at line {}: left={l:?} right={r:?}", index + 1);
        }
    }
    if left.len() == right.len() {
        "values differ but no line diff found".to_string()
    } else {
        format!(
            "different lengths: left={} bytes right={} bytes",
            left.len(),
            right.len()
        )
    }
}

fn command_string(args: &[&str]) -> String {
    format!("anneal --root <corpus> {}", args.join(" "))
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn write_report(
    path: &Path,
    config: &Config,
    corpus_root: &Path,
    sample_handles: &[String],
    cases: &[CaseResult],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report dir {}", parent.display()))?;
    }
    let report = json!({
        "schema": "anneal.parity-runner.v0",
        "corpus": config.corpus,
        "corpus_root": corpus_root,
        "left_bin": config.left_bin,
        "right_bin": config.right_bin,
        "right_mode": config.right_mode.as_str(),
        "sample_size": config.sample_size,
        "sample_handles": sample_handles,
        "cases": cases.iter().map(|case| json!({
            "id": case.id,
            "label": case.label,
            "command": case.command,
            "passed": case.passed,
            "detail": case.detail,
            "left_stderr": case.left_stderr,
            "right_stderr": case.right_stderr,
        })).collect::<Vec<_>>(),
        "summary": {
            "passes": cases.iter().filter(|case| case.passed).count(),
            "failures": cases.iter().filter(|case| !case.passed).count(),
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_json_diff_preserves_array_order() {
        let left = json!({ "items": [1, 2] });
        let right = json!({ "items": [2, 1] });
        let diff = first_json_diff(&left, &right);
        assert!(diff.contains("left=\"    1,\" right=\"    2,\""));
    }

    #[test]
    fn sanitize_replaces_path_separators() {
        assert_eq!(sanitize("../large-corpus fixture"), "---large-corpus-fixture");
    }
}
