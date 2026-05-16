use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use anneal_core::runtime::eval::{ExplainOptions, NumberValue};
use anneal_core::runtime::prelude::{LoadedPrelude, PreludeError, datalog_string_literal};
use anneal_core::runtime::{
    Database, EvalOptions, Evaluator, Program, QueryOutput, Row, Value, analyze, parse_program,
    write_ndjson,
};
use anneal_core::{
    ActorContext, CancellationToken, ConfigEntry, ConfigFacts, CorpusId, FactStore, Generation,
    Source, SourceContext, SourceInfo, VerbLayer, VerbRegistry, load_project_extension,
    load_runtime_configs_if_present, merge_program_layers,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::Utf8PathBuf;

use crate::{
    ContextCommand, ContextOutput, DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT, DescribeCommand,
    ReadCommand, SearchCommand, SourcesCommand,
};

const DEFAULT_CORPUS: &str = "cli";
const EMPTY_ROWS_DIAGNOSTIC: &str = "(0 rows)";

pub fn should_handle_args(args: &[OsString]) -> bool {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        if matches!(arg, "-e" | "--eval") {
            return true;
        }
        if matches!(arg, "--root" | "--format") {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--root=") || arg.starts_with("--format=") || is_ignored_global_flag(arg)
        {
            continue;
        }
        if arg == "help" {
            return iter
                .next()
                .and_then(|next| next.to_str())
                .is_some_and(|topic| HelpTopic::parse(topic).is_some());
        }
        return RuntimeCommand::recognizes_first_arg(arg);
    }
    true
}

pub fn main_entry() -> Result<()> {
    run_args(std::env::args_os().collect())
}

pub fn run_args(args: Vec<OsString>) -> Result<()> {
    let invocation = Invocation::parse(args)?;
    if let RuntimeCommand::Help { topic } = invocation.command {
        return write_text(io::stdout().lock(), topic.render());
    }
    let session = RuntimeSession::load(&invocation.root)?;
    let output = session.run(invocation.command)?;
    let stdout = io::stdout();
    let mode = invocation.output.resolve(stdout.is_terminal());
    if let Some(message) = output.empty_rows_diagnostic(mode) {
        writeln!(io::stderr().lock(), "{message}")?;
    }
    output.write(stdout.lock(), mode)
}

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    root: Utf8PathBuf,
    output: OutputPreference,
    command: RuntimeCommand,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OutputPreference {
    #[default]
    Auto,
    Human,
    Json,
}

impl OutputPreference {
    const fn resolve(self, stdout_is_terminal: bool) -> OutputMode {
        match self {
            Self::Auto if stdout_is_terminal => OutputMode::Human,
            Self::Auto | Self::Json => OutputMode::Json,
            Self::Human => OutputMode::Human,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
}

impl Invocation {
    fn parse(args: Vec<OsString>) -> Result<Self> {
        let mut root = None;
        let mut output = OutputPreference::Auto;
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
            } else if arg == "--json" {
                output = OutputPreference::Json;
            } else if arg == "--format" {
                output = parse_output_format(
                    iter.next()
                        .context("--format requires json or text")?
                        .to_str()
                        .context("--format value is not valid UTF-8")?,
                )?;
            } else if let Some(value) = arg.strip_prefix("--format=") {
                output = parse_output_format(value)?;
            } else if !is_ignored_global_flag(&arg) {
                rest.push(arg);
            }
        }
        Ok(Self {
            root: root.unwrap_or_else(default_root),
            output,
            command: if rest.is_empty() {
                RuntimeCommand::Status
            } else {
                RuntimeCommand::parse(&rest)?
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RuntimeCommand {
    Status,
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
    Vocab,
    Describe {
        name: String,
    },
    Sources,
    Schema,
    Verbs,
    Eval {
        query: String,
        explain: ExplainOptions,
    },
    Help {
        topic: HelpTopic,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTopic {
    Status,
    Context,
    Search,
    Read,
    Handle,
    Work,
    Blocked,
    Broken,
    Trend,
    Vocab,
    Describe,
    Sources,
    Schema,
    Verbs,
    Eval,
}

impl HelpTopic {
    fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "status" => Self::Status,
            "context" => Self::Context,
            "search" => Self::Search,
            "read" => Self::Read,
            "H" => Self::Handle,
            "work" => Self::Work,
            "blocked" => Self::Blocked,
            "broken" => Self::Broken,
            "trend" => Self::Trend,
            "vocab" => Self::Vocab,
            "describe" => Self::Describe,
            "sources" => Self::Sources,
            "schema" => Self::Schema,
            "verbs" => Self::Verbs,
            "eval" | "-e" | "--eval" => Self::Eval,
            _ => return None,
        })
    }

    const fn render(self) -> &'static str {
        match self {
            Self::Status => {
                "\
Usage: anneal [OPTIONS] status

Print compact corpus status from the programmable runtime.

Migration note: in 0.10 and earlier, `anneal status` printed the corpus health
overview. That compatibility report is now `anneal health`; `status` is the
runtime work-prioritization view.

Output: human summary at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Context => {
                "\
Usage: anneal [OPTIONS] context [OPTIONS] <GOAL>

Cold-agent orientation in one response. Composes search, bounded read
spans, and graph neighborhood.

Arguments:
  <GOAL>                         Natural-language goal/query

Options:
      --budget <N>               Derives one per-hit read cap; not divided by hits
      --hits <N>                 Number of search winners (default: 3)
      --depth <N>                Alias for --neighborhood-depth
      --neighborhood-depth <N>   Graph distance around winners (default: 1)
      --include-low-confidence   Include low-confidence search hits

Output: human summary at a terminal or with --format=text; one JSON object when piped or with --json.
"
            }
            Self::Search => {
                "\
Usage: anneal [OPTIONS] search [OPTIONS] <TEXT>

Ranked content search over handles and spans.

Arguments:
  <TEXT>                         Search query

Options:
      --limit <N>                Maximum rows (default: 25)
      --include-low-confidence   Include low-confidence hits

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Read => {
                "\
Usage: anneal [OPTIONS] read [OPTIONS] <HANDLE>

Read bounded content spans for a handle.

Arguments:
  <HANDLE>                       Handle id to read

Options:
      --budget <N>               Token budget (default: 4000)

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Handle => {
                "\
Usage: anneal [OPTIONS] H <HANDLE>

Show one handle plus bounded incoming/outgoing references.

Arguments:
  <HANDLE>                       Handle id to inspect

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Work => {
                "\
Usage: anneal [OPTIONS] work

Show ranked work candidates from the standard-library work verb.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Blocked => {
                "\
Usage: anneal [OPTIONS] blocked <HANDLE>

Show why one handle is blocked according to convergence rules.

Arguments:
  <HANDLE>                       Handle id to inspect

For a corpus-wide blocked list, use `anneal status` or `anneal work`.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Broken => {
                "\
Usage: anneal [OPTIONS] broken

Show diagnostic blockers from the standard-library checks prelude.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Trend => {
                "\
Usage: anneal [OPTIONS] trend

Show status changes when snapshot history exists. No-history corpora emit no rows.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Vocab => {
                "\
Usage: anneal [OPTIONS] vocab

List observed corpus vocabulary: status values, edge kinds, namespaces, and
frontmatter fields.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Describe => {
                "\
Usage: anneal [OPTIONS] describe [NAME]

Describe a runtime primitive, predicate, or verb. Defaults to runtime.

Arguments:
  [NAME]                         Object to describe

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Sources => {
                "\
Usage: anneal [OPTIONS] sources

List linked sources/adapters and their capabilities.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Schema => {
                "\
Usage: anneal [OPTIONS] schema

List runtime predicates, primitives, signatures, and provenance.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Verbs => {
                "\
Usage: anneal [OPTIONS] verbs

List standard-library and project @verb declarations.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Eval => {
                "\
Usage: anneal [OPTIONS] -e [OPTIONS] <QUERY>
       anneal [OPTIONS] eval [OPTIONS] <QUERY>

Run a raw Datalog query against the programmable runtime.

Arguments:
  <QUERY>                        Query string

Options:
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
        }
    }
}

impl RuntimeCommand {
    fn parse(args: &[String]) -> Result<Self> {
        let Some((command, rest)) = args.split_first() else {
            bail!("missing runtime command");
        };
        if command == "help" {
            let topic = rest
                .first()
                .and_then(|topic| HelpTopic::parse(topic))
                .context("help requires a runtime command")?;
            return Ok(Self::Help { topic });
        }
        if rest
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
            && let Some(topic) = HelpTopic::parse(command)
        {
            return Ok(Self::Help { topic });
        }
        match command.as_str() {
            "status" => {
                ensure_no_args(rest, "status")?;
                Ok(Self::Status)
            }
            "context" => parse_context(rest),
            "search" => parse_search(rest),
            "read" => parse_read(rest),
            "H" => Ok(Self::Handle {
                handle: required_positional(rest, "H requires a handle")?.to_string(),
            }),
            "work" => {
                ensure_no_args(rest, "work")?;
                Ok(Self::Work)
            }
            "blocked" => Ok(Self::Blocked {
                handle: required_positional(
                    rest,
                    "blocked inspects one handle; pass `anneal blocked <HANDLE>` or use `anneal status` for a corpus-wide blocked list",
                )?
                .to_string(),
            }),
            "broken" => {
                ensure_no_args(rest, "broken")?;
                Ok(Self::Broken)
            }
            "trend" => {
                ensure_no_args(rest, "trend")?;
                Ok(Self::Trend)
            }
            "vocab" => {
                ensure_no_args(rest, "vocab")?;
                Ok(Self::Vocab)
            }
            "describe" => match rest {
                [] => Ok(Self::Describe {
                    name: "runtime".to_string(),
                }),
                [name] => Ok(Self::Describe { name: name.clone() }),
                _ => bail!(
                    "describe accepts at most one name; got {:?}",
                    rest.join(" ")
                ),
            },
            "sources" => {
                ensure_no_args(rest, "sources")?;
                Ok(Self::Sources)
            }
            "schema" => {
                ensure_no_args(rest, "schema")?;
                Ok(Self::Schema)
            }
            "verbs" => {
                ensure_no_args(rest, "verbs")?;
                Ok(Self::Verbs)
            }
            "-e" | "--eval" | "eval" => parse_eval(rest),
            other => bail!("unknown runtime command {other:?}"),
        }
    }

    fn recognizes_first_arg(arg: &str) -> bool {
        HelpTopic::parse(arg).is_some()
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
            merge_discovery(&mut discovery, extension.discovery());
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

    fn run(&self, command: RuntimeCommand) -> Result<CommandOutput> {
        match command {
            RuntimeCommand::Status => self.run_status(),
            RuntimeCommand::Context {
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
                let output = self.eval(command.datalog().as_str(), ExplainOptions::disabled())?;
                Ok(CommandOutput::Context(command.group_rows(&output.rows)?))
            }
            RuntimeCommand::Search {
                query,
                limit,
                include_low_confidence,
            } => {
                let query = SearchCommand::new(query)
                    .with_limit(limit)
                    .include_low_confidence(include_low_confidence)
                    .datalog();
                self.run_query(&query, ExplainOptions::disabled())
            }
            RuntimeCommand::Read { handle, budget } => {
                let query = ReadCommand::new(handle).with_budget(budget).datalog();
                self.run_query(&query, ExplainOptions::disabled())
            }
            RuntimeCommand::Handle { handle } => {
                self.run_query(&handle_query(&handle), ExplainOptions::disabled())
            }
            RuntimeCommand::Work => self.run_verb("work"),
            RuntimeCommand::Blocked { handle } => {
                self.run_query(&blocked_query(&handle), ExplainOptions::disabled())
            }
            RuntimeCommand::Broken => self.run_verb("broken"),
            RuntimeCommand::Trend => self.run_verb("trend"),
            RuntimeCommand::Vocab => self.run_verb("vocab"),
            RuntimeCommand::Describe { name } => {
                let query = DescribeCommand::new(name).datalog();
                self.run_query(&query, ExplainOptions::disabled())
            }
            RuntimeCommand::Sources => {
                self.run_query(SourcesCommand.datalog(), ExplainOptions::disabled())
            }
            RuntimeCommand::Schema => self.run_verb("schema"),
            RuntimeCommand::Verbs => self.run_verb("verbs"),
            RuntimeCommand::Eval { query, explain } => self.run_query(&query, explain),
            RuntimeCommand::Help { topic } => Ok(CommandOutput::Text(topic.render())),
        }
    }

    fn run_verb(&self, name: &str) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        self.run_query(plan.query_source(), ExplainOptions::disabled())
    }

    fn run_status(&self) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor("status", &self.actor)?;
        let output = self.eval(plan.query_source(), ExplainOptions::disabled())?;
        Ok(CommandOutput::Status(output.rows))
    }

    fn run_query(&self, query: &str, explain: ExplainOptions) -> Result<CommandOutput> {
        let output = self.eval(query, explain)?;
        Ok(CommandOutput::Rows(output.rows))
    }

    fn eval(&self, query_source: &str, explain: ExplainOptions) -> Result<QueryOutput> {
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
        if explain.is_enabled() {
            options = options.with_explain_options(explain);
        }
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
    Rows(Vec<Row>),
    Status(Vec<Row>),
    Context(ContextOutput),
    Text(&'static str),
}

impl CommandOutput {
    fn empty_rows_diagnostic(&self, mode: OutputMode) -> Option<&'static str> {
        match (mode, self) {
            (_, Self::Rows(rows)) | (OutputMode::Json, Self::Status(rows)) if rows.is_empty() => {
                Some(EMPTY_ROWS_DIAGNOSTIC)
            }
            (_, Self::Status(_) | Self::Rows(_) | Self::Context(_) | Self::Text(_)) => None,
        }
    }

    fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match (mode, self) {
            (OutputMode::Human, Self::Status(rows)) => write_status_text(writer, &rows)?,
            (OutputMode::Human, Self::Context(output)) => write_context_text(writer, &output)?,
            (OutputMode::Human, Self::Rows(rows)) => write_rows_text(writer, &rows)?,
            (_, Self::Status(rows) | Self::Rows(rows)) => write_ndjson(writer, rows)?,
            (_, Self::Context(output)) => write_ndjson(writer, std::iter::once(output))?,
            (_, Self::Text(text)) => write_text(writer, text)?,
        }
        Ok(())
    }
}

fn write_text<W: Write>(mut writer: W, text: &str) -> Result<()> {
    writer.write_all(text.as_bytes())?;
    Ok(())
}

fn write_status_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    const SECTION_ORDER: [&str; 4] = ["broken", "blocked", "work", "advancing"];
    const MAX_ROWS_PER_SECTION: usize = 12;

    writeln!(writer, "Status")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    let mut sections: BTreeMap<&str, Vec<StatusRow<'_>>> = BTreeMap::new();
    for row in rows {
        let row = StatusRow::from_row(row)?;
        sections.entry(row.section).or_default().push(row);
    }

    let mut wrote_section = false;
    for section in SECTION_ORDER
        .into_iter()
        .chain(sections.keys().copied().filter(|section| {
            !SECTION_ORDER
                .iter()
                .any(|ordered| ordered.eq_ignore_ascii_case(section))
        }))
    {
        let Some(section_rows) = sections.get(section) else {
            continue;
        };
        if wrote_section {
            writeln!(writer)?;
        }
        wrote_section = true;
        writeln!(writer, "{}", section_title(section))?;
        for (index, row) in section_rows.iter().take(MAX_ROWS_PER_SECTION).enumerate() {
            writeln!(
                writer,
                "{:>2}. {}  score={}  {}",
                index + 1,
                row.handle,
                display_number(row.score),
                row.why
            )?;
        }
        let omitted = section_rows.len().saturating_sub(MAX_ROWS_PER_SECTION);
        if omitted > 0 {
            writeln!(writer, "    ... {omitted} more")?;
        }
    }
    Ok(())
}

fn write_context_text<W: Write>(mut writer: W, output: &ContextOutput) -> Result<()> {
    const MAX_TEXT_LINES_PER_SPAN: usize = 8;
    const MAX_NEIGHBORS_PER_HANDLE: usize = 8;

    writeln!(writer, "Context")?;
    writeln!(writer, "Goal: {}", output.goal)?;

    if output.hits.is_empty() {
        writeln!(writer, "(0 hits)")?;
        return Ok(());
    }

    writeln!(writer)?;
    writeln!(writer, "Hits")?;
    for (index, hit) in output.hits.iter().enumerate() {
        let span = hit
            .span_id
            .as_deref()
            .map_or(String::new(), |span| format!(" span={span}"));
        writeln!(
            writer,
            "{:>2}. {}  score={:.3}  field={}  reason={}{}",
            index + 1,
            hit.handle,
            hit.score,
            hit.field,
            hit.reason,
            span
        )?;
    }

    if !output.spans.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "Read")?;
        for span in &output.spans {
            writeln!(
                writer,
                "{}:{}-{}  tokens={}",
                span.handle, span.start_line, span.end_line, span.tokens
            )?;
            let mut lines = span.text.lines();
            for line in lines.by_ref().take(MAX_TEXT_LINES_PER_SPAN) {
                writeln!(writer, "  {}", line.trim_end())?;
            }
            if lines.next().is_some() {
                writeln!(writer, "  ...")?;
            }
        }
    }

    if !output.neighborhood.is_empty() {
        let mut by_handle: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for neighbor in &output.neighborhood {
            by_handle
                .entry(&neighbor.handle)
                .or_default()
                .push(&neighbor.neighbor);
        }

        writeln!(writer)?;
        writeln!(writer, "Neighborhood")?;
        for (handle, neighbors) in by_handle {
            let omitted = neighbors.len().saturating_sub(MAX_NEIGHBORS_PER_HANDLE);
            write!(writer, "{handle}: ")?;
            for (index, neighbor) in neighbors.iter().take(MAX_NEIGHBORS_PER_HANDLE).enumerate() {
                if index > 0 {
                    write!(writer, ", ")?;
                }
                write!(writer, "{neighbor}")?;
            }
            if omitted == 0 {
                writeln!(writer)?;
            } else {
                writeln!(writer, ", ... {omitted} more")?;
            }
        }
    }

    Ok(())
}

fn write_rows_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    writeln!(writer, "Rows")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    for (index, row) in rows.iter().enumerate() {
        write!(writer, "{:>2}.", index + 1)?;
        for (field, value) in &row.fields {
            write!(writer, " {field}={}", display_value(value))?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

fn section_title(section: &str) -> String {
    let mut chars = section.chars();
    let Some(first) = chars.next() else {
        return "Other".to_string();
    };
    first.to_uppercase().chain(chars).collect()
}

struct StatusRow<'a> {
    section: &'a str,
    handle: &'a str,
    score: &'a NumberValue,
    why: &'a str,
}

impl<'a> StatusRow<'a> {
    fn from_row(row: &'a Row) -> Result<Self> {
        Ok(Self {
            section: required_string(row, "section")?,
            handle: required_string(row, "h")?,
            score: required_number(row, "score")?,
            why: required_string(row, "why")?,
        })
    }
}

fn required_string<'a>(row: &'a Row, field: &str) -> Result<&'a str> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(value),
        Some(_) => bail!("status row field {field:?} must be a string"),
        None => bail!("status row missing field {field:?}"),
    }
}

fn required_number<'a>(row: &'a Row, field: &str) -> Result<&'a NumberValue> {
    match row.fields.get(field) {
        Some(Value::Number(value)) => Ok(value),
        Some(_) => bail!("status row field {field:?} must be a number"),
        None => bail!("status row missing field {field:?}"),
    }
}

fn display_number(value: &NumberValue) -> String {
    match value {
        NumberValue::Int(value) => value.to_string(),
        NumberValue::Float(value) => format!("{value:.3}"),
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => display_string_value(value),
        Value::Number(value) => display_number(value),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::List(values) => {
            let values = values
                .iter()
                .map(display_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({values})")
        }
    }
}

fn display_string_value(value: &str) -> String {
    const MAX_INLINE_CHARS: usize = 96;

    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rendered = String::new();
    for (index, ch) in collapsed.chars().enumerate() {
        if index == MAX_INLINE_CHARS {
            rendered.push_str("...");
            break;
        }
        rendered.push(ch);
    }
    if rendered.is_empty() {
        r#""""#.to_string()
    } else if rendered
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '/' | '_' | '-' | ':' | '#'))
    {
        rendered
    } else {
        format!("{rendered:?}")
    }
}

fn parse_output_format(value: &str) -> Result<OutputPreference> {
    match value {
        "json" => Ok(OutputPreference::Json),
        "text" => Ok(OutputPreference::Human),
        _ => bail!("--format accepts json or text; got {value:?}"),
    }
}

fn parse_context(args: &[String]) -> Result<RuntimeCommand> {
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
    Ok(RuntimeCommand::Context {
        goal: goal.context("context requires a goal")?,
        budget,
        hits,
        depth,
        include_low_confidence,
    })
}

fn parse_search(args: &[String]) -> Result<RuntimeCommand> {
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
    let query = query.context("search requires a query")?;
    ensure!(!query.trim().is_empty(), "search query must not be empty");
    Ok(RuntimeCommand::Search {
        query,
        limit,
        include_low_confidence,
    })
}

fn parse_read(args: &[String]) -> Result<RuntimeCommand> {
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
    Ok(RuntimeCommand::Read {
        handle: handle.context("read requires a handle")?,
        budget,
    })
}

fn parse_eval(args: &[String]) -> Result<RuntimeCommand> {
    let mut query = None;
    let mut explain = ExplainOptions::disabled();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--explain" => explain = explain.with_first_rows(3),
            "--explain-depth" => {
                let depth = parse_positive_usize(
                    next_value(&mut iter, "--explain-depth")?,
                    "--explain-depth",
                )?;
                explain = explain.with_depth_limit(depth);
            }
            value if value.starts_with("--explain-depth=") => {
                let depth = parse_positive_usize(value_after_equals(value), "--explain-depth")?;
                explain = explain.with_depth_limit(depth);
            }
            "--explain-first" => {
                let rows = parse_positive_usize(
                    next_value(&mut iter, "--explain-first")?,
                    "--explain-first",
                )?;
                explain = explain.with_first_rows(rows);
            }
            value if value.starts_with("--explain-first=") => {
                let rows = parse_positive_usize(value_after_equals(value), "--explain-first")?;
                explain = explain.with_first_rows(rows);
            }
            "--explain-all" => explain = explain.with_all_rows(),
            value if value.starts_with('-') => bail!("unknown eval option {value:?}"),
            value => assign_once(&mut query, value, "eval accepts one query string")?,
        }
    }
    Ok(RuntimeCommand::Eval {
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

fn ensure_no_args(args: &[String], command: &str) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        bail!("{command} accepts no arguments; got {:?}", args.join(" "))
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

fn parse_positive_usize(value: &str, flag: &str) -> Result<usize> {
    let parsed = parse_usize(value, flag)?;
    ensure!(
        parsed > 0,
        "{flag} value {value:?} must be greater than zero"
    );
    Ok(parsed)
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

fn merge_discovery(discovery: &mut Vec<ConfigEntry>, extension: &ConfigFacts) {
    for entry in extension.entries() {
        if entry.ordinal.is_none() {
            discovery.retain(|existing| existing.key != entry.key || existing.ordinal.is_some());
        }
        discovery.push(entry.clone());
    }
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
    use anneal_core::runtime::eval::ExplainRowLimit;
    use anneal_core::runtime::prelude::standard_prelude_program;
    use std::fs;
    use std::num::NonZeroUsize;
    use tempfile::tempdir;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn routes_only_runtime_commands() {
        assert!(should_handle_args(&os(&["anneal"])));
        assert!(should_handle_args(&os(&["anneal", "--root", ".design"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--root", ".design", "context", "goal"
        ])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "-e",
            "? *handle{id: h}."
        ])));
        assert!(should_handle_args(&os(&["anneal", "help", "context"])));
        assert!(!should_handle_args(&os(&["anneal", "anneal"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--root", ".design", "status"
        ])));
        assert!(should_handle_args(&os(&[
            "anneal", "--format", "text", "work"
        ])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "--format=text",
            "vocab"
        ])));
        assert!(!should_handle_args(&os(&[
            "anneal", "--root", ".design", "health"
        ])));
        assert!(!should_handle_args(&os(&["anneal", "--help"])));
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
            RuntimeCommand::Context {
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
        let RuntimeCommand::Eval { query, explain } = parsed.command else {
            panic!("expected eval command");
        };
        assert_eq!(
            query,
            "? diagnostic(code, severity, subject, file, line, evidence)."
        );
        assert!(explain.is_enabled());
        assert_eq!(explain.depth().get(), 4);
        assert!(explain.explicit_depth());
        assert_eq!(explain.row_limit(), ExplainRowLimit::default());
    }

    #[test]
    fn parses_eval_explain_row_limit_options() {
        let parsed = Invocation::parse(os(&[
            "anneal",
            "-e",
            "? blocked(h).",
            "--explain-first=2",
            "--explain-depth",
            "4",
        ]))
        .expect("parse explain first");
        let RuntimeCommand::Eval { query, explain } = parsed.command else {
            panic!("expected eval command");
        };
        assert_eq!(query, "? blocked(h).");
        assert!(explain.is_enabled());
        assert_eq!(explain.depth().get(), 4);
        assert_eq!(
            explain.row_limit(),
            ExplainRowLimit::First(NonZeroUsize::new(2).expect("nonzero"))
        );

        let parsed = Invocation::parse(os(&["anneal", "-e", "? blocked(h).", "--explain-all"]))
            .expect("parse explain all");
        let RuntimeCommand::Eval { query, explain } = parsed.command else {
            panic!("expected eval command");
        };
        assert_eq!(query, "? blocked(h).");
        assert!(explain.is_enabled());
        assert_eq!(explain.row_limit(), ExplainRowLimit::All);
    }

    #[test]
    fn parses_runtime_subcommand_help_without_loading_corpus() {
        for (command, topic, expected_output) in [
            ("context", HelpTopic::Context, "Output: human summary"),
            ("search", HelpTopic::Search, "Output: readable rows"),
            ("read", HelpTopic::Read, "Output: readable rows"),
            ("vocab", HelpTopic::Vocab, "Output: readable rows"),
        ] {
            let parsed = Invocation::parse(os(&["anneal", "--root=.design", command, "--help"]))
                .expect("parse command help");

            assert_eq!(parsed.command, RuntimeCommand::Help { topic });
            assert!(topic.render().contains("Usage: anneal"));
            assert!(topic.render().contains(expected_output));
        }
        assert!(
            HelpTopic::Context
                .render()
                .contains(&format!("default: {}", crate::DEFAULT_CONTEXT_HITS))
        );
        assert!(HelpTopic::Context.render().contains(&format!(
            "default: {}",
            crate::DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH
        )));
        assert!(
            HelpTopic::Search
                .render()
                .contains(&format!("default: {DEFAULT_SEARCH_LIMIT}"))
        );
        assert!(
            HelpTopic::Read
                .render()
                .contains(&format!("default: {DEFAULT_READ_BUDGET}"))
        );
        assert!(
            HelpTopic::Status.render().contains("0.10 and earlier")
                && HelpTopic::Status.render().contains("anneal health"),
            "status help should explain the health rename"
        );
    }

    #[test]
    fn parses_eval_help_aliases() {
        let parsed = Invocation::parse(os(&["anneal", "-e", "--help"])).expect("parse eval help");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Help {
                topic: HelpTopic::Eval
            }
        );
        assert!(HelpTopic::Eval.render().contains("--explain-depth"));
        assert!(HelpTopic::Eval.render().contains("--explain-first"));
        assert!(HelpTopic::Eval.render().contains("--explain-all"));
    }

    #[test]
    fn parses_help_subcommand_for_runtime_topics() {
        let parsed =
            Invocation::parse(os(&["anneal", "help", "context"])).expect("parse help context");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Help {
                topic: HelpTopic::Context
            }
        );
        assert!(HelpTopic::Context.render().contains("<GOAL>"));
    }

    #[test]
    fn bare_invocation_defaults_to_status() {
        let parsed = Invocation::parse(os(&["anneal", "--root=.design"])).expect("parse");

        assert_eq!(parsed.command, RuntimeCommand::Status);
        assert_eq!(parsed.output, OutputPreference::Auto);
    }

    #[test]
    fn parses_json_output_preference() {
        let parsed = Invocation::parse(os(&["anneal", "--json", "status"])).expect("parse status");

        assert_eq!(parsed.command, RuntimeCommand::Status);
        assert_eq!(parsed.output, OutputPreference::Json);
    }

    #[test]
    fn parses_text_output_preference() {
        let parsed =
            Invocation::parse(os(&["anneal", "--format=text", "work"])).expect("parse work");

        assert_eq!(parsed.command, RuntimeCommand::Work);
        assert_eq!(parsed.output, OutputPreference::Human);

        let parsed =
            Invocation::parse(os(&["anneal", "vocab", "--format", "json"])).expect("parse vocab");

        assert_eq!(parsed.command, RuntimeCommand::Vocab);
        assert_eq!(parsed.output, OutputPreference::Json);
    }

    #[test]
    fn describe_rejects_extra_names() {
        let error = Invocation::parse(os(&["anneal", "describe", "runtime", "extra"]))
            .expect_err("extra describe args should fail");

        assert!(
            error
                .to_string()
                .contains("describe accepts at most one name")
        );
    }

    #[test]
    fn search_rejects_empty_query() {
        let error =
            Invocation::parse(os(&["anneal", "search", "   "])).expect_err("empty search fails");

        assert!(error.to_string().contains("search query must not be empty"));
    }

    #[test]
    fn blocked_without_handle_points_to_list_command() {
        let error = Invocation::parse(os(&["anneal", "blocked"]))
            .expect_err("blocked without handle fails");

        assert!(error.to_string().contains("blocked inspects one handle"));
        assert!(error.to_string().contains("anneal status"));
    }

    #[test]
    fn empty_row_outputs_report_zero_rows_to_stderr() {
        assert_eq!(
            CommandOutput::Rows(Vec::new()).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::Rows(Vec::new()).empty_rows_diagnostic(OutputMode::Human),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::Status(Vec::new()).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::Status(Vec::new()).empty_rows_diagnostic(OutputMode::Human),
            None
        );
    }

    #[test]
    fn status_human_render_groups_sections() {
        let output = CommandOutput::Status(vec![
            row(&[
                ("section", Value::String("work".to_string())),
                ("h", Value::String("plan.md".to_string())),
                ("score", Value::Number(NumberValue::Int(42))),
                ("why", Value::String("potential".to_string())),
            ]),
            row(&[
                ("section", Value::String("broken".to_string())),
                ("h", Value::String("bad.md".to_string())),
                ("score", Value::Number(NumberValue::Int(100))),
                ("why", Value::String("E001".to_string())),
            ]),
        ]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Status\n"));
        assert!(rendered.contains("Broken\n 1. bad.md"));
        assert!(rendered.contains("Work\n 1. plan.md"));
    }

    #[test]
    fn status_json_render_preserves_ndjson() {
        let output = CommandOutput::Status(vec![row(&[
            ("section", Value::String("work".to_string())),
            ("h", Value::String("plan.md".to_string())),
            ("score", Value::Number(NumberValue::Int(42))),
            ("why", Value::String("potential".to_string())),
        ])]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Json)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with(
            "{\"h\":\"plan.md\",\"score\":42,\"section\":\"work\",\"why\":\"potential\"}\n"
        ));
    }

    #[test]
    fn generic_rows_human_render_is_readable() {
        let output = CommandOutput::Rows(vec![row(&[
            ("category", Value::String("status".to_string())),
            ("value", Value::String("open question".to_string())),
            ("count", Value::Number(NumberValue::Int(2))),
        ])]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render rows");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Rows\n 1."));
        assert!(rendered.contains("category=status"));
        assert!(rendered.contains(r#"value="open question""#));
        assert!(rendered.contains("count=2"));
    }

    #[test]
    fn status_human_render_rejects_schema_drift() {
        let output = CommandOutput::Status(vec![row(&[
            ("section", Value::String("work".to_string())),
            ("h", Value::String("plan.md".to_string())),
            ("why", Value::String("potential".to_string())),
        ])]);
        let mut rendered = Vec::new();

        let error = output
            .write(&mut rendered, OutputMode::Human)
            .expect_err("missing score should fail");

        assert!(error.to_string().contains("status row missing field"));
    }

    #[test]
    fn context_human_render_is_readable() {
        let output = CommandOutput::Context(ContextOutput {
            goal: "find release blockers".to_string(),
            hits: vec![crate::ContextHit {
                handle: "plan.md".to_string(),
                span_id: Some("body".to_string()),
                score: 0.9,
                reason: "body:release".to_string(),
                field: "body".to_string(),
            }],
            spans: vec![crate::ContextSpan {
                handle: "plan.md".to_string(),
                span_id: "body".to_string(),
                start_line: 10,
                end_line: 12,
                tokens: 12,
                text: "Release blocker details.\nNext line.".to_string(),
            }],
            neighborhood: vec![crate::ContextNeighbor {
                handle: "plan.md".to_string(),
                neighbor: "dep.md".to_string(),
            }],
        });
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render context");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Context\nGoal: find release blockers"));
        assert!(rendered.contains("Hits\n 1. plan.md"));
        assert!(rendered.contains("Read\nplan.md:10-12"));
        assert!(rendered.contains("Neighborhood\nplan.md: dep.md"));
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

    fn row(fields: &[(&str, Value)]) -> Row {
        Row {
            fields: fields
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
            derivation: None,
        }
    }

    #[test]
    fn sources_command_reports_linked_markdown_adapter() {
        let fixture = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.fixtures/sample-corpus");
        let session = RuntimeSession::load(&fixture).expect("fixture session loads");
        let output = session.run(RuntimeCommand::Sources).expect("sources runs");
        let CommandOutput::Rows(rows) = output else {
            panic!("sources should emit rows");
        };
        assert!(rows.iter().any(|row| {
            row.fields.get("name")
                == Some(&anneal_core::runtime::Value::String("markdown".to_string()))
        }));
    }

    #[test]
    fn project_discovery_facts_affect_markdown_extraction() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::create_dir(root.join("included")).expect("create included");
        fs::write(root.join("anneal.toml"), "").expect("write config");
        fs::write(root.join("anneal.dl"), r#"md.scan_root("included")."#)
            .expect("write project rules");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\n---\n# Excluded\nshared marker\n",
        )
        .expect("write excluded doc");
        fs::write(
            root.join("included").join("b.md"),
            "---\nstatus: draft\n---\n# Included\nshared marker\n",
        )
        .expect("write included doc");

        let session = RuntimeSession::load(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Search {
                query: "shared marker".to_string(),
                limit: 10,
                include_low_confidence: false,
            })
            .expect("search runs");
        let CommandOutput::Rows(rows) = output else {
            panic!("search should emit rows");
        };

        assert!(rows.iter().any(|row| {
            row.fields.get("h")
                == Some(&anneal_core::runtime::Value::String(
                    "included/b.md".to_string(),
                ))
        }));
        assert!(!rows.iter().any(|row| {
            row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
        }));
    }
}
