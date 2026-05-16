use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;

use anneal_core::runtime::prelude::{LoadedPrelude, PreludeError, datalog_string_literal};
use anneal_core::runtime::{
    Database, EvalOptions, Evaluator, Program, QueryOutput, analyze, parse_program, write_ndjson,
};
use anneal_core::{
    ActorContext, CancellationToken, ConfigEntry, ConfigFacts, CorpusId, FactStore, Generation,
    Source, SourceContext, SourceInfo, VerbLayer, VerbRegistry, load_project_extension,
    load_runtime_configs_if_present, merge_program_layers,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail};
use camino::Utf8PathBuf;
use serde::Serialize;

use crate::{
    ContextCommand, DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT, DescribeCommand, ReadCommand,
    SearchCommand, SourcesCommand,
};

const DEFAULT_CORPUS: &str = "cli";

pub fn should_handle_args(args: &[OsString]) -> bool {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        if matches!(arg, "-e" | "--eval") {
            return true;
        }
        if arg == "--root" {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--root=") || is_ignored_global_flag(arg) {
            continue;
        }
        return V2Command::recognizes_first_arg(arg);
    }
    false
}

pub fn main_entry() -> Result<()> {
    run_args(std::env::args_os().collect())
}

pub fn run_args(args: Vec<OsString>) -> Result<()> {
    let invocation = Invocation::parse(args)?;
    let session = RuntimeSession::load(&invocation.root)?;
    let output = session.run(invocation.command)?;
    output.write(io::stdout().lock())
}

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    root: Utf8PathBuf,
    command: V2Command,
}

impl Invocation {
    fn parse(args: Vec<OsString>) -> Result<Self> {
        let mut root = None;
        let mut rest = Vec::new();
        let mut iter = args.into_iter().skip(1);
        while let Some(arg) = iter.next() {
            let arg = arg
                .into_string()
                .map_err(|arg| anyhow!("argument is not valid UTF-8: {}", arg.to_string_lossy()))?;
            if arg == "--root" {
                let value = iter
                    .next()
                    .context("--root requires a path")?
                    .into_string()
                    .map_err(|arg| {
                        anyhow!("--root path is not valid UTF-8: {}", arg.to_string_lossy())
                    })?;
                root = Some(Utf8PathBuf::from(value));
            } else if let Some(value) = arg.strip_prefix("--root=") {
                root = Some(Utf8PathBuf::from(value));
            } else if !is_ignored_global_flag(&arg) {
                rest.push(arg);
            }
        }
        Ok(Self {
            root: root.unwrap_or_else(default_root),
            command: V2Command::parse(&rest)?,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum V2Command {
    Dashboard,
    Context {
        goal: String,
        budget: i64,
        hits: usize,
        depth: i64,
        include_low_confidence: bool,
    },
    Search {
        query: String,
        limit: usize,
        include_low_confidence: bool,
    },
    Read {
        handle: String,
        budget: i64,
    },
    Handle {
        handle: String,
    },
    Work,
    Blocked {
        handle: String,
    },
    Broken,
    Trend,
    Describe {
        name: String,
    },
    Sources,
    Schema,
    Verbs,
    Eval {
        query: String,
        explain: ExplainMode,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ExplainMode {
    #[default]
    Disabled,
    DefaultDepth,
    Depth(usize),
}

impl V2Command {
    fn parse(args: &[String]) -> Result<Self> {
        let Some((command, rest)) = args.split_first() else {
            bail!("missing v2 command");
        };
        match command.as_str() {
            "anneal" => Ok(Self::Dashboard),
            "context" => parse_context(rest),
            "search" => parse_search(rest),
            "read" => parse_read(rest),
            "H" => Ok(Self::Handle {
                handle: required_positional(rest, "H requires a handle")?.to_string(),
            }),
            "work" => Ok(Self::Work),
            "blocked" => Ok(Self::Blocked {
                handle: required_positional(rest, "blocked requires a handle")?.to_string(),
            }),
            "broken" => Ok(Self::Broken),
            "trend" => Ok(Self::Trend),
            "describe" => Ok(Self::Describe {
                name: rest.first().map_or("runtime", String::as_str).to_string(),
            }),
            "sources" => Ok(Self::Sources),
            "schema" => Ok(Self::Schema),
            "verbs" => Ok(Self::Verbs),
            "-e" | "--eval" | "eval" => parse_eval(rest),
            other => bail!("unknown v2 command {other:?}"),
        }
    }

    fn recognizes_first_arg(arg: &str) -> bool {
        matches!(
            arg,
            "anneal"
                | "context"
                | "search"
                | "read"
                | "H"
                | "work"
                | "blocked"
                | "broken"
                | "trend"
                | "describe"
                | "sources"
                | "schema"
                | "verbs"
                | "eval"
        )
    }
}

struct RuntimeSession {
    program: Program,
    store: FactStore,
    registry: VerbRegistry,
    actor: ActorContext,
    sources: Vec<SourceInfo>,
}

impl RuntimeSession {
    fn load(root: &camino::Utf8Path) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let corpus = CorpusId::from(DEFAULT_CORPUS);
        let source = MarkdownSource;
        let sources = vec![source.describe()];
        let loaded_prelude = LoadedPrelude::load_active().map_err(prelude_error)?;
        let mut program = loaded_prelude.program().clone();
        let mut discovery = default_markdown_config();
        let project = if root.join(anneal_core::PROJECT_RULE_FILE).is_file() {
            let extension = load_project_extension(root.as_std_path(), &sources, &program)?;
            discovery.extend(extension.discovery().entries().iter().cloned());
            Some(extension.program().clone())
        } else {
            None
        };
        if let Some(project) = &project {
            let (merged, warnings) = merge_program_layers(program, project.clone());
            for warning in warnings {
                eprintln!(
                    "warning: {}:{}: '{}' overrides prelude ({} clauses)",
                    warning.location.source_name,
                    warning.location.line,
                    warning.predicate,
                    warning.replaced_clauses
                );
            }
            program = merged;
        }

        let config_facts = ConfigFacts::from_entries(discovery);
        let roots = vec![root.to_path_buf()];
        let context = SourceContext {
            corpus: corpus.clone(),
            roots: roots.as_slice(),
            config_facts: &config_facts,
            time_ref: None,
            previous_generation: Some(Generation::new(0)),
            actor: actor.clone(),
            cancellation: CancellationToken::new(),
        };
        let batch = source
            .extract(&context)
            .map_err(|err| anyhow!("markdown extraction failed: {err}"))?;
        let mut store = FactStore::default();
        store
            .merge(batch)
            .context("failed to merge markdown facts")?;
        let configs = load_runtime_configs_if_present(root, &corpus)?;
        if !configs.is_empty() {
            store
                .replace_configs(&corpus, configs)
                .context("failed to merge runtime config facts")?;
        }
        let registry = match &project {
            Some(project) => VerbRegistry::from_layers(&[
                (VerbLayer::Prelude, loaded_prelude.program()),
                (VerbLayer::Project, project),
            ])?,
            None => VerbRegistry::from_layers(&[(VerbLayer::Prelude, loaded_prelude.program())])?,
        };

        Ok(Self {
            program,
            store,
            registry,
            actor,
            sources,
        })
    }

    fn run(&self, command: V2Command) -> Result<CommandOutput> {
        match command {
            V2Command::Dashboard => self.run_verb("anneal"),
            V2Command::Context {
                goal,
                budget,
                hits,
                depth,
                include_low_confidence,
            } => {
                let command = ContextCommand::new(goal)
                    .with_budget(budget)
                    .with_hits(hits)
                    .with_neighborhood_depth(depth)
                    .include_low_confidence(include_low_confidence);
                let output = self.eval(command.datalog().as_str(), ExplainMode::Disabled)?;
                Ok(CommandOutput::One(serde_json::to_value(
                    command.group_rows(&output.rows)?,
                )?))
            }
            V2Command::Search {
                query,
                limit,
                include_low_confidence,
            } => {
                let query = SearchCommand::new(query)
                    .with_limit(limit)
                    .include_low_confidence(include_low_confidence)
                    .datalog();
                self.run_query(&query, ExplainMode::Disabled)
            }
            V2Command::Read { handle, budget } => {
                let query = ReadCommand::new(handle).with_budget(budget).datalog();
                self.run_query(&query, ExplainMode::Disabled)
            }
            V2Command::Handle { handle } => {
                self.run_query(&handle_query(&handle), ExplainMode::Disabled)
            }
            V2Command::Work => self.run_verb("work"),
            V2Command::Blocked { handle } => {
                self.run_query(&blocked_query(&handle), ExplainMode::Disabled)
            }
            V2Command::Broken => self.run_verb("broken"),
            V2Command::Trend => self.run_verb("trend"),
            V2Command::Describe { name } => {
                let query = DescribeCommand::new(name).datalog();
                self.run_query(&query, ExplainMode::Disabled)
            }
            V2Command::Sources => self.run_query(SourcesCommand.datalog(), ExplainMode::Disabled),
            V2Command::Schema => self.run_verb("schema"),
            V2Command::Verbs => self.run_verb("verbs"),
            V2Command::Eval { query, explain } => self.run_query(&query, explain),
        }
    }

    fn run_verb(&self, name: &str) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        self.run_query(plan.query_source(), ExplainMode::Disabled)
    }

    fn run_query(&self, query: &str, explain: ExplainMode) -> Result<CommandOutput> {
        let output = self.eval(query, explain)?;
        Ok(CommandOutput::Rows(output.rows))
    }

    fn eval(&self, query_source: &str, explain: ExplainMode) -> Result<QueryOutput> {
        let mut program = self.program.clone();
        let query_program = parse_program("cli-query", query_source)
            .with_context(|| format!("failed to parse query {query_source:?}"))?;
        program.statements.extend(query_program.statements);
        let analyzed = analyze(program).context("query failed static analysis")?;
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .context("query source did not contain a query")?;
        let mut options = EvalOptions::default().with_actor(self.actor.clone());
        options = match explain {
            ExplainMode::Disabled => options,
            ExplainMode::DefaultDepth => options.with_explain(),
            ExplainMode::Depth(depth) => options.with_explain_depth(depth),
        };
        let database = Database::from_store_for_options(&self.store, &options)
            .with_sources(self.sources.clone());
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator.run_fixpoint().context("query fixpoint failed")?;
        evaluator
            .eval_query(&query)
            .context("query evaluation failed")
    }
}

enum CommandOutput {
    Rows(Vec<anneal_core::runtime::Row>),
    One(serde_json::Value),
}

impl CommandOutput {
    fn write<W: Write>(self, mut writer: W) -> Result<()> {
        match self {
            Self::Rows(rows) => write_ndjson(&mut writer, rows)?,
            Self::One(value) => write_json_line(&mut writer, &value)?,
        }
        Ok(())
    }
}

fn write_json_line<W: Write, T: Serialize>(mut writer: W, value: &T) -> Result<()> {
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn parse_context(args: &[String]) -> Result<V2Command> {
    let mut goal = None;
    let mut budget = DEFAULT_READ_BUDGET;
    let mut hits = crate::DEFAULT_CONTEXT_HITS;
    let mut depth = crate::DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH;
    let mut include_low_confidence = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--budget" => budget = parse_i64(next_value(&mut iter, "--budget")?, "--budget")?,
            "--hits" => hits = parse_usize(next_value(&mut iter, "--hits")?, "--hits")?,
            "--depth" | "--neighborhood-depth" => {
                depth = parse_i64(next_value(&mut iter, arg)?, arg)?;
            }
            "--include-low-confidence" => include_low_confidence = true,
            value if value.starts_with("--budget=") => {
                budget = parse_i64(value_after_equals(value), "--budget")?;
            }
            value if value.starts_with("--hits=") => {
                hits = parse_usize(value_after_equals(value), "--hits")?;
            }
            value if value.starts_with("--depth=") => {
                depth = parse_i64(value_after_equals(value), "--depth")?;
            }
            value if value.starts_with("--neighborhood-depth=") => {
                depth = parse_i64(value_after_equals(value), "--neighborhood-depth")?;
            }
            value if value.starts_with('-') => bail!("unknown context option {value:?}"),
            value => assign_once(&mut goal, value, "context accepts one goal")?,
        }
    }
    Ok(V2Command::Context {
        goal: goal.context("context requires a goal")?,
        budget,
        hits,
        depth,
        include_low_confidence,
    })
}

fn parse_search(args: &[String]) -> Result<V2Command> {
    let mut query = None;
    let mut limit = DEFAULT_SEARCH_LIMIT;
    let mut include_low_confidence = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" => limit = parse_usize(next_value(&mut iter, "--limit")?, "--limit")?,
            "--include-low-confidence" => include_low_confidence = true,
            value if value.starts_with("--limit=") => {
                limit = parse_usize(value_after_equals(value), "--limit")?;
            }
            value if value.starts_with('-') => bail!("unknown search option {value:?}"),
            value => assign_once(&mut query, value, "search accepts one query")?,
        }
    }
    Ok(V2Command::Search {
        query: query.context("search requires a query")?,
        limit,
        include_low_confidence,
    })
}

fn parse_read(args: &[String]) -> Result<V2Command> {
    let mut handle = None;
    let mut budget = DEFAULT_READ_BUDGET;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--budget" => budget = parse_i64(next_value(&mut iter, "--budget")?, "--budget")?,
            value if value.starts_with("--budget=") => {
                budget = parse_i64(value_after_equals(value), "--budget")?;
            }
            value if value.starts_with('-') => bail!("unknown read option {value:?}"),
            value => assign_once(&mut handle, value, "read accepts one handle")?,
        }
    }
    Ok(V2Command::Read {
        handle: handle.context("read requires a handle")?,
        budget,
    })
}

fn parse_eval(args: &[String]) -> Result<V2Command> {
    let mut query = None;
    let mut explain = ExplainMode::Disabled;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--explain" => explain = ExplainMode::DefaultDepth,
            "--explain-depth" => {
                explain = ExplainMode::Depth(parse_usize(
                    next_value(&mut iter, "--explain-depth")?,
                    "--explain-depth",
                )?);
            }
            value if value.starts_with("--explain-depth=") => {
                explain =
                    ExplainMode::Depth(parse_usize(value_after_equals(value), "--explain-depth")?);
            }
            value if value.starts_with('-') => bail!("unknown eval option {value:?}"),
            value => assign_once(&mut query, value, "eval accepts one query string")?,
        }
    }
    Ok(V2Command::Eval {
        query: query.context("eval requires a query")?,
        explain,
    })
}

fn required_positional<'a>(args: &'a [String], message: &str) -> Result<&'a str> {
    match args {
        [value] => Ok(value),
        [] => bail!("{message}"),
        _ => bail!("{message}; got extra arguments"),
    }
}

fn assign_once(target: &mut Option<String>, value: &str, message: &str) -> Result<()> {
    if target.replace(value.to_string()).is_some() {
        bail!("{message}");
    }
    Ok(())
}

fn next_value<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a str> {
    iter.next()
        .map(String::as_str)
        .with_context(|| format!("{flag} requires a value"))
}

fn parse_i64(value: &str, flag: &str) -> Result<i64> {
    value
        .parse()
        .with_context(|| format!("{flag} value {value:?} is not an integer"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize> {
    value
        .parse()
        .with_context(|| format!("{flag} value {value:?} is not a positive integer"))
}

fn value_after_equals(value: &str) -> &str {
    value
        .split_once('=')
        .expect("caller checked prefix with equals")
        .1
}

fn is_ignored_global_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--json" | "--pretty" | "--plain" | "--minimal" | "--no-color"
    )
}

fn default_root() -> Utf8PathBuf {
    [".design", "docs"]
        .into_iter()
        .find(|candidate| Path::new(candidate).is_dir())
        .map_or_else(|| Utf8PathBuf::from("."), Utf8PathBuf::from)
}

fn default_markdown_config() -> Vec<ConfigEntry> {
    vec![
        ConfigEntry::scalar("md.file_extension", ".md"),
        ConfigEntry::scalar("md.scan_root", "."),
    ]
}

fn handle_query(handle: &str) -> String {
    let handle = datalog_string_literal(handle);
    format!(
        r#"
handle_focus({handle}).

handle_row({handle}, "self", {handle}, kind, status, file, line, summary) :=
  *handle{{id: {handle}, kind: kind, status: status, file: file, line: line, summary: summary}}.

handle_row({handle}, "out", other, kind, null, file, line, "") :=
  *edge{{from: {handle}, to: other, kind: kind, file: file, line: line}}.

handle_row({handle}, "in", other, kind, null, file, line, "") :=
  *edge{{to: {handle}, from: other, kind: kind, file: file, line: line}}.

? handle_row(h, relation, other, kind, status, file, line, summary).
"#
    )
}

fn blocked_query(handle: &str) -> String {
    let handle = datalog_string_literal(handle);
    format!(
        r"
blocked_focus({handle}).

blocked_row(h, energy, source, kind, status, file) :=
  blocked_focus(h),
  potential(h, energy),
  entropy(h, source),
  *handle{{id: h, kind: kind, status: status, file: file}}.

? blocked_row(h, energy, source, kind, status, file).
"
    )
}

fn prelude_error(error: PreludeError) -> anyhow::Error {
    anyhow!(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anneal_core::runtime::prelude::standard_prelude_program;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn routes_only_v2_commands() {
        assert!(should_handle_args(&os(&[
            "anneal", "--root", ".design", "context", "goal"
        ])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "-e",
            "? *handle{id: h}."
        ])));
        assert!(!should_handle_args(&os(&[
            "anneal", "--root", ".design", "status"
        ])));
        assert!(!should_handle_args(&os(&["anneal", "check", "--json"])));
        assert!(!should_handle_args(&os(&["anneal", "--mcp"])));
    }

    #[test]
    fn parses_context_options() {
        let parsed = Invocation::parse(os(&[
            "anneal",
            "--root=.design",
            "context",
            "v17 audit",
            "--budget",
            "1200",
            "--hits=2",
            "--depth=3",
        ]))
        .expect("parse");
        assert_eq!(parsed.root, Utf8PathBuf::from(".design"));
        assert_eq!(
            parsed.command,
            V2Command::Context {
                goal: "v17 audit".to_string(),
                budget: 1200,
                hits: 2,
                depth: 3,
                include_low_confidence: false,
            }
        );
    }

    #[test]
    fn parses_eval_explain_depth() {
        let parsed = Invocation::parse(os(&[
            "anneal",
            "-e",
            "? diagnostic(code, severity, subject, file, line, evidence).",
            "--explain-depth",
            "4",
        ]))
        .expect("parse");
        assert_eq!(
            parsed.command,
            V2Command::Eval {
                query: "? diagnostic(code, severity, subject, file, line, evidence).".to_string(),
                explain: ExplainMode::Depth(4),
            }
        );
    }

    #[test]
    fn handle_query_escapes_literals() {
        let query = handle_query("notes/\"quoted\".md");
        assert!(query.contains(r#""notes/\"quoted\".md""#));
        let mut program = standard_prelude_program().expect("prelude parses");
        program.statements.extend(
            parse_program("handle-test", &query)
                .expect("query parses")
                .statements,
        );
        analyze(program).expect("query analyzes");
    }

    #[test]
    fn sources_command_reports_linked_markdown_adapter() {
        let fixture = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.fixtures/sample-corpus");
        let session = RuntimeSession::load(&fixture).expect("fixture session loads");
        let output = session.run(V2Command::Sources).expect("sources runs");
        let CommandOutput::Rows(rows) = output else {
            panic!("sources should emit rows");
        };
        assert!(rows.iter().any(|row| {
            row.fields.get("name")
                == Some(&anneal_core::runtime::Value::String("markdown".to_string()))
        }));
    }
}
