//! Parity runner skeleton for anneal v2.0 Phase 0.
//!
//! The first job is intentionally modest: run anneal v1.1 against itself on the
//! frozen large-corpus fixture for PD-1, PD-2, and PD-3. That proves the harness is
//! deterministic before v2.0 has any output to compare against.

use anneal_core::{
    ActorContext, CancellationToken, ConfigFacts, CorpusId, FactBatch, Generation, Source,
    SourceContext,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail};
use camino::Utf8PathBuf;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    stdout: Value,
    stderr: String,
}

#[derive(Debug)]
struct FactRight {
    root: Utf8PathBuf,
    batch: FactBatch,
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
            ["status", "--json"] => {
                anneal::v2_adapter::status_json_from_facts(self.root.as_path(), &self.batch)?
            }
            ["check", "--scope=active", "--json"] => {
                anneal::v2_adapter::check_json_from_facts(self.root.as_path(), &self.batch)?
            }
            ["get", handle, "--refs", "--json"] => anneal::v2_adapter::get_refs_json_from_facts(
                self.root.as_path(),
                &self.batch,
                handle,
            )?,
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
        "status",
        &config.left_bin,
        &right_runner,
        &corpus_root,
        &["status", "--json"],
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

fn extract_anneal_md(corpus_root: &Path) -> Result<FactRight> {
    let root = Utf8PathBuf::from_path_buf(corpus_root.to_path_buf())
        .map_err(|path| anyhow!("corpus path is not valid UTF-8: {}", path.display()))?;
    let roots = vec![root.clone()];
    let config_facts = ConfigFacts::new(vec![
        ("md.file_extension".to_string(), ".md".to_string()),
        ("md.scan_root".to_string(), ".".to_string()),
    ]);
    let context = SourceContext {
        corpus: CorpusId::from("parity"),
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
    Ok(FactRight { root, batch })
}

fn run_anneal(bin: &str, corpus_root: &Path, args: &[&str]) -> Result<CommandResult> {
    let output = Command::new(bin)
        .arg("--root")
        .arg(corpus_root)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn {}", command_string(args)))?;
    if !output.status.success() {
        bail!(
            "{} exited {}: {}",
            command_string(args),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout: Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse JSON from {}", command_string(args)))?;
    Ok(CommandResult { stdout, stderr })
}

fn sample_handles(bin: &str, corpus_root: &Path, sample_size: usize) -> Result<Vec<String>> {
    let out = run_anneal(bin, corpus_root, &["query", "handles", "--json", "--full"])?;
    let items = out
        .stdout
        .get("items")
        .and_then(Value::as_array)
        .context("query handles output missing items array")?;
    let ids = items
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if ids.len() < sample_size {
        bail!("only {} handles available; need {sample_size}", ids.len());
    }
    Ok(ids.into_iter().take(sample_size).collect())
}

fn first_json_diff(left: &Value, right: &Value) -> String {
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
