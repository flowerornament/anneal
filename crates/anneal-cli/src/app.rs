//! Command-line argument parsing and human-facing rendering.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;

use anneal_code::CodeSource;
use anneal_core::runtime::ast::DerivedAtom;
use anneal_core::runtime::eval::{ExplainOptions, NumberValue, QueryWarning};
use anneal_core::runtime::prelude::{LoadedPrelude, PreludeError, datalog_string_literal};
use anneal_core::runtime::{
    AnalyzedProgram, Atom, Body, CallArg, CallStyle, Database, EvalOptions, Evaluator, Expr,
    Literal, NegatedAtom, NumberLiteral, Program, QueryOutput, Row, StoredAtom, Value, analyze,
    parse_program, stored_relation_fields, write_ndjson,
};
use anneal_core::{
    ActorContext, CancellationToken, CodeTargetMeta, ConfigEntry, ConfigFact, ConfigFacts,
    CorpusId, EdgeFact, FactStore, Generation, InferredCorpusRoot, ProjectExtension,
    SnapshotAppendOutcome, SnapshotEntry, SnapshotEntryFact, Source, SourceContext, SourceInfo,
    VerbArg, VerbArgKind, VerbCapability, VerbDispatchError, VerbEntry, VerbLayer, VerbRegistry,
    append_snapshot_entry_capped, infer_corpus_root, load_project_extension, merge_program_layers,
    read_snapshot_history, render_verb_arg_facts,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::Serialize;

use crate::{
    ContextCommand, ContextOutput, DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT, DescribeCommand,
    ReadCommand, SearchCommand,
};
use anneal_core::ranking::{
    CONTEXT_NEIGHBOR_GROUP_CURRENT, CONTEXT_NEIGHBOR_GROUP_HIDDEN,
    CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT, CONTEXT_NEIGHBOR_GROUP_SUPERSEDED,
};

const DEFAULT_CORPUS: &str = "cli";
const EMPTY_ROWS_DIAGNOSTIC: &str = "(0 rows)";
const DEFAULT_AUTO_SNAPSHOT_LIMIT: usize = 100;
const DEFAULT_IMPACT_TRAVERSE: &[&str] = &["DependsOn", "Supersedes", "Verifies"];
const IMPACT_TRAVERSE_CONFIG_KEY: &str = "impact.traverse";
const SUPERSEDES_EDGE_KIND: &str = "Supersedes";

fn available_source_info() -> Vec<SourceInfo> {
    vec![MarkdownSource::default().describe(), CodeSource.describe()]
}
const RESOLVED_FILE_META_KEY: &str = "md.resolved_file";
const SKILL_MARKDOWN: &str = include_str!("../../../skills/anneal/SKILL.md");

pub fn should_handle_args(args: &[OsString]) -> bool {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let Some(arg) = arg.to_str() else {
            return true;
        };
        if matches!(arg, "-h" | "--help") {
            return true;
        }
        if arg == "--version" {
            return true;
        }
        if matches!(arg, "-e" | "--eval") {
            return true;
        }
        if matches!(arg, "--root" | "--format" | "--area" | "--since") {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--root=")
            || arg.starts_with("--format=")
            || arg.starts_with("--area=")
            || arg.starts_with("--since=")
            || is_routing_only_flag(arg)
        {
            continue;
        }
        if arg == "help" {
            let Some(topic) = iter.next().and_then(|next| next.to_str()) else {
                return true;
            };
            return HelpTopic::parse(topic).is_some() || !topic.starts_with('-');
        }
        if arg == "check" {
            return true;
        }
        return !arg.starts_with('-');
    }
    true
}

pub fn main_entry() -> Result<()> {
    run_args(std::env::args_os().collect())
}

pub fn run_args(args: Vec<OsString>) -> Result<()> {
    let mut invocation = Invocation::parse(args)?;
    if let RuntimeCommand::Version = invocation.command {
        return write_text(
            io::stdout().lock(),
            &format!("anneal {}\n", env!("CARGO_PKG_VERSION")),
        );
    }
    if let RuntimeCommand::Help { topic } = invocation.command {
        return write_text(io::stdout().lock(), &topic.render());
    }
    if let RuntimeCommand::Prime = invocation.command {
        return write_text(io::stdout().lock(), &HelpTopic::Agent.render());
    }
    invocation.resolve_root()?;
    if let RuntimeCommand::Init { dry_run, force } = invocation.command {
        let output = run_init(invocation.root.path(), dry_run, force)?;
        let stdout = io::stdout();
        let mode = invocation.output.resolve(stdout.is_terminal());
        output.write(stdout.lock(), mode)?;
        return Ok(());
    }
    if let Some(root) = invocation.root.implicit_unmarked_root() {
        bail!(
            "no marked corpus root found above {root}; refusing implicit scan. Run `anneal init --dry-run` to inspect a project file, `anneal init` to mark this corpus, or pass `--root <path>` to scan that directory explicitly."
        );
    }
    let stdin_explain = match &invocation.command {
        RuntimeCommand::Eval {
            query,
            explain,
            limit,
        } if query == "-" => Some((explain.clone(), *limit)),
        _ => None,
    };
    if let Some((explain, limit)) = stdin_explain {
        let mut stdin_query = String::new();
        io::stdin()
            .read_to_string(&mut stdin_query)
            .context("failed to read eval query from stdin")?;
        invocation.command = RuntimeCommand::Eval {
            query: stdin_query,
            explain,
            limit,
        };
    }
    if let RuntimeCommand::Verb { name, args } = &invocation.command
        && args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        let registry = RuntimeRegistry::load(invocation.root.path())?;
        let entry = match registry.registry.resolve_for_actor(name, &registry.actor) {
            Ok(entry) => entry,
            Err(VerbDispatchError::MissingVerb { .. }) => {
                bail!(
                    "unknown help topic {name:?}; use `anneal help agent` for the agent briefing, `anneal describe runtime` for the command map, or `anneal schema` for callable verbs"
                );
            }
            Err(error) => return Err(error.into()),
        };
        return write_text(io::stdout().lock(), &render_dynamic_verb_help(entry));
    }
    let session = RuntimeSession::load(invocation.root.path(), &invocation.command)?;
    let output = session.run(invocation.command)?;
    let stdout = io::stdout();
    let mode = invocation.output.resolve(stdout.is_terminal());
    let has_displayable_content = output.has_displayable_content();
    let mut stderr_messages = Vec::new();
    if let Some(message) = output.stderr_diagnostic(mode) {
        stderr_messages.push(message);
    }
    if let Some(message) = invocation.root.diagnostic(mode, has_displayable_content) {
        stderr_messages.push(message);
    }
    if !stderr_messages.is_empty() {
        writeln!(io::stderr().lock(), "{}", stderr_messages.join("\n"))?;
    }
    let gate_failed = output.gate_failed();
    output.write(stdout.lock(), mode)?;
    if gate_failed {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    root: RootSelection,
    output: OutputPreference,
    command: RuntimeCommand,
}

#[derive(Debug, PartialEq, Eq)]
enum RootSelection {
    Explicit(Utf8PathBuf),
    Inferred(InferredCorpusRoot),
    Undiscovered,
}

impl RootSelection {
    fn from_parse(root: Option<Utf8PathBuf>) -> Self {
        root.map_or(Self::Undiscovered, Self::Explicit)
    }

    fn resolve(&mut self) -> Result<()> {
        *self = match self {
            Self::Explicit(root) => Self::Explicit(absolute_root(root)?),
            Self::Inferred(root) => Self::Inferred(absolute_inferred_root(root)?),
            Self::Undiscovered => Self::Inferred(default_root()?),
        };
        Ok(())
    }

    fn path(&self) -> &Utf8Path {
        match self {
            Self::Explicit(root) => root,
            Self::Inferred(root) => root.path(),
            Self::Undiscovered => {
                unreachable!("runtime root must be resolved before loading the corpus")
            }
        }
    }

    fn implicit_unmarked_root(&self) -> Option<&Utf8Path> {
        match self {
            Self::Inferred(InferredCorpusRoot::Unmarked(root)) => Some(root),
            Self::Explicit(_)
            | Self::Inferred(InferredCorpusRoot::Marked(_))
            | Self::Undiscovered => None,
        }
    }

    fn diagnostic(&self, mode: OutputMode, output_has_content: bool) -> Option<String> {
        match self {
            Self::Explicit(_)
            | Self::Inferred(InferredCorpusRoot::Unmarked(_))
            | Self::Undiscovered => None,
            Self::Inferred(InferredCorpusRoot::Marked(root)) => {
                if matches!(mode, OutputMode::Json | OutputMode::JsonExplicit)
                    || !output_has_content
                {
                    Some(format!("resolved root: {root}"))
                } else {
                    None
                }
            }
        }
    }
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
            Self::Auto => OutputMode::Json,
            Self::Json => OutputMode::JsonExplicit,
            Self::Human => OutputMode::Human,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
    JsonExplicit,
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
            } else if rest.is_empty() && is_compatibility_filter_flag(&arg) {
                bail!(
                    "{arg} is a retired compatibility filter; express the filter in Datalog with `anneal -e`"
                );
            } else if rest.is_empty() && is_compatibility_render_flag(&arg) {
                bail!(
                    "{arg} is a retired compatibility rendering flag; use `--format=text`, `--format=json`, or `--json`"
                );
            } else {
                rest.push(arg);
            }
        }
        Ok(Self {
            root: RootSelection::from_parse(root),
            output,
            command: if rest.is_empty() {
                RuntimeCommand::Status
            } else {
                RuntimeCommand::parse(&rest)?
            },
        })
    }

    fn resolve_root(&mut self) -> Result<()> {
        self.root.resolve()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RuntimeCommand {
    Version,
    Status,
    Init {
        dry_run: bool,
        force: bool,
    },
    Prime,
    Context {
        goal: String,
        budget: i64,
        hits: usize,
        depth: i64,
        include_low_confidence: bool,
        read_spans: bool,
    },
    Search {
        query: String,
        limit: usize,
        include_low_confidence: bool,
    },
    Read {
        handle: String,
        budget: i64,
        span_id: Option<String>,
    },
    Handle {
        handle: String,
        impact: bool,
        lineage: bool,
    },
    Check {
        refresh_drift: bool,
    },
    Describe {
        name: String,
    },
    Schema,
    Eval {
        query: String,
        explain: ExplainOptions,
        limit: Option<usize>,
    },
    Verb {
        name: String,
        args: Vec<String>,
    },
    Help {
        topic: HelpTopic,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTopic {
    Top,
    Agent,
    Init,
    Status,
    Context,
    Search,
    Read,
    Handle,
    Check,
    Describe,
    Schema,
    Eval,
}

impl HelpTopic {
    fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "top" | "runtime" => Self::Top,
            "agent" => Self::Agent,
            "init" => Self::Init,
            "status" => Self::Status,
            "context" => Self::Context,
            "search" => Self::Search,
            "read" => Self::Read,
            "handle" | "H" => Self::Handle,
            "check" => Self::Check,
            "describe" => Self::Describe,
            "schema" => Self::Schema,
            "eval" | "-e" | "--eval" => Self::Eval,
            _ => return None,
        })
    }

    fn render(self) -> String {
        if matches!(self, Self::Agent) {
            return skill_briefing_body(SKILL_MARKDOWN).to_string();
        }

        let body = match self {
            Self::Top => {
                "\
Usage: anneal [OPTIONS] [COMMAND]

Arrive, discover the vocabulary, retrieve evidence, then ask precise Datalog
questions against a corpus.

First moves:
  anneal status
  anneal context \"goal\"

Discover:
  anneal schema
  anneal describe runtime
  anneal describe <axis>
  anneal describe <predicate>

Retrieve:
  anneal search \"text\" --limit 5
  anneal read <handle> --budget 4000
  anneal handle <handle> [--impact|--lineage]

Program:
  anneal -e '? predicate(args).'
  anneal help eval

More help:
  anneal help agent
  anneal help <command>

Root premise:
  Run from a marked corpus (.design, docs, or anneal.dl), pass --root PATH,
  or use anneal init --dry-run to preview a project file.
"
            }
            Self::Agent => unreachable!("agent help returns before static help rendering"),
            Self::Init => {
                "\
Usage: anneal [OPTIONS] init [OPTIONS]

Generate an anneal.dl project declaration from inferred markdown corpus
structure, or migrate an older anneal.toml to the unified runtime config.

Options:
      --dry-run                  Print the generated anneal.dl without writing
      --force                    Replace anneal.dl or migrate anneal.toml

Output: readable config preview at a terminal or with --format=text; JSON object when piped or with --json.

Use init when a directory is not yet marked. Runtime commands require either a
marked inferred root or an explicit --root PATH.
"
            }
            Self::Status => {
                "\
Usage: anneal [OPTIONS] status

Print compact corpus status from the programmable runtime.

Use this as the arrival command: it renders aggregate corpus vital signs and
copy-runnable orientation/work queries. For goal-less reading, run the
`recent_frontier` and `ranked_anchor` queries it prints; use `context GOAL`
once you have a specific goal.

Output: human summary at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Context => {
                "\
Usage: anneal [OPTIONS] context [OPTIONS] <GOAL>

Cold-agent orientation in one response. Composes summary-bearing span search,
bounded span metadata, and graph neighborhood. Use --read-spans to include matched
span bodies.

Arguments:
  <GOAL>                         Natural-language goal/query

Options:
      --budget <N>               Per-hit span selection cap; used for bodies with --read-spans
      --hits <N>                 Number of search winners (default: 3)
      --depth <N>                Alias for --neighborhood-depth
      --neighborhood-depth <N>   Graph distance around winners (default: 1)
      --include-low-confidence   Include low-confidence search hits
      --read-spans               Include matched span bodies in the output

Output: human summary at a terminal or with --format=text; NDJSON event rows when piped or with --json.
"
            }
            Self::Search => {
                "\
Usage: anneal [OPTIONS] search [OPTIONS] <TEXT>

Ranked content search over handles and heading spans. Span hits include
summary metadata.

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
      --span-id <ID>             Read one content span

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Handle => {
                "\
Usage: anneal [OPTIONS] handle [OPTIONS] <HANDLE>

Show one handle plus bounded incoming/outgoing references. Outgoing and
incoming edges are grouped by kind; in-repo code refs render in a dedicated
Code references section.

Arguments:
  <HANDLE>                       Handle id to inspect

Options:
      --impact                   Include direct/indirect reverse dependencies
      --lineage                  Include file supersession DAG and current head

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Check { .. } => {
                "\
Usage: anneal [OPTIONS] check

Hidden CI gate for error-severity diagnostics.

Options:
      --refresh-drift            Refresh design-code drift evidence before checking

For filtered diagnostic questions, use eval:
  anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'
  anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'

Deprecation: hidden alias retained for CI muscle memory; prefer eval composition in agent-facing workflows.

Output: readable error diagnostics at a terminal or with --format=text; NDJSON rows when piped or with --json. Exits 1 when any error row exists.
"
            }
            Self::Describe => {
                "\
Usage: anneal [OPTIONS] describe [NAME]

Describe a runtime primitive, predicate, or verb. Defaults to runtime.
Use `anneal describe runtime` for the compact map, then `anneal -e` for
composition.

Arguments:
  [NAME]                         Object to describe

Output: readable teaching cards by default, including when piped; use --json or --format=json for NDJSON rows.
"
            }
            Self::Schema => {
                "\
Usage: anneal [OPTIONS] schema

List runtime predicates, primitives, signatures, and provenance.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Eval => {
                "\
Usage: anneal [OPTIONS] -e [OPTIONS] <QUERY>
       anneal [OPTIONS] eval [OPTIONS] <QUERY>

Run a Datalog query against corpus facts. This is anneal's compositional
surface: use commands to orient, introspection to discover vocabulary, and
`-e` when you need a precise question.

Arguments:
  <QUERY>                        Query string

Options:
      --limit <N>                Cap returned rows after evaluation
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Grammar tour:
  Queries ask for rows:
    ? predicate(arg), other(arg2).

  Stored relations are source/runtime facts. They use `*name{field: value}`:
    ? *handle{id: h, kind: \"file\", status: s}.
    ? *edge{from: src, to: dst, kind: \"DependsOn\"}.
    `id: h` binds a variable. `kind: \"file\"` filters to a literal.

  Derived predicates and primitives use complete call syntax:
    ? frontier(h, energy).
    ? search(query: \"conformance\", handle: h, span_id: span, score: score,
        reason: reason, field: field, low_confidence: low).

  Relation-pattern calls use braces when you only care about some fields:
    ? diagnostic{severity: \"error\", subject: h}.
    ? search{query: \"conformance\", handle: h, score: score}.
    ? diagnostic{subject: h}, area_of{h: h, area: \"language\"}.
    Omitted fields behave like hidden wildcards and are not output columns.

  Local rules name reusable subqueries before the final `?` query:
    open_file(h) := *handle{id: h, kind: \"file\"}, active(h).
    ? open_file(h).

  Negation uses `not` after variables are positively bound:
    missing_discharge(h) := obligation(h), not discharged(h).

  Aggregates bind tuples from grouped rows:
    area(area) := area_of(h, area).
    area_count(area, n) :=
      area(area),
      n = Count{ h : area_of(h, area) }.

    ? (h, energy) = TopK{ k: 10, key: energy :
        (h, energy) : potential(h, energy)
      }.

  Time blocks query supported historical references:
    ? at(\"snapshot:last\") { *handle{id: h, status: old} },
      *handle{id: h, status: now},
      old != now.
    Only snapshot references are supported today; git refs like at(\"HEAD~5\") remain pending.

  Stratification rule of thumb:
    recursive rules are fine; negation and aggregates must not depend on
    themselves through a cycle. If analysis rejects a query, split the negative
    or aggregate part into a later rule.

Migration recipes:
  Hidden CI gate:
    anneal check
    anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'
    `anneal check` exits 1 when any error row exists; use eval for filtered agent workflows.

  Retired obligations:
    anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'

  Retired diff:
    anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'

Goal-less orientation:
  Start with `anneal status`; it prints these copy-runnable queries:
    anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12
    anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12
  Use `anneal context \"GOAL\"` after you can name the goal.

Discover before guessing:
  anneal schema --format=text
  anneal describe runtime --format=text
  anneal describe search --format=text
  anneal -e '? source_of(\"frontier\", file, lines).'
  Unknown predicate and stored-field errors include nearby names and allowed fields.

Examples:
  anneal -e '? *handle{id: h, kind: \"file\", status: s}.' --limit 20
  anneal -e '? *edge{from: src, to: dst, kind: \"DependsOn\"}.'
  anneal -e '? search{query: \"conformance\", handle: h, span_id: span, score: score}, *span{handle: h, id: span, summary: summary}.' --limit 20
  anneal -e '? read{handle: \"docs/runtime-overview.md\", budget: 4000, text: text}.'
  anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12
  anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12
  anneal -e '? diagnostic{severity: \"error\", subject: h, file: file}.'
  anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
  anneal -e '? changed_within(h, 7), *handle{id: h, kind: \"file\"}, search{query: \"conformance\", handle: h}.'
  anneal -e '? source_of(\"frontier\", file, lines).'
  anneal -e - < query.dl

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
        };
        if matches!(self, Self::Eval | Self::Check { .. }) {
            format!("{body}{RUNTIME_HELP_OPTIONS}")
        } else {
            format!("{body}{RUNTIME_PROVENANCE_OPTIONS}{RUNTIME_HELP_OPTIONS}")
        }
    }
}

const RUNTIME_PROVENANCE_OPTIONS: &str = "\
Provenance options:
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

";

const RUNTIME_HELP_OPTIONS: &str = "\
Global options:
      --root <PATH>              Corpus root (default: nearest .design, docs, or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json>       Force readable text or JSON/NDJSON output
";

impl RuntimeCommand {
    fn parse(args: &[String]) -> Result<Self> {
        let Some((command, rest)) = args.split_first() else {
            bail!("missing runtime command");
        };
        if matches!(command.as_str(), "-h" | "--help") {
            ensure_no_args(rest, command)?;
            return Ok(Self::Help {
                topic: HelpTopic::Top,
            });
        }
        if command == "help" {
            let Some(topic) = rest.first() else {
                return Ok(Self::Help {
                    topic: HelpTopic::Top,
                });
            };
            ensure!(
                rest.len() == 1,
                "help accepts one runtime command, topic, or verb name; use `anneal help agent` for the agent briefing"
            );
            if let Some(topic) = HelpTopic::parse(topic) {
                return Ok(Self::Help { topic });
            }
            if let Some(message) = retired_command_message(topic) {
                bail!("{message}");
            }
            return Ok(Self::Verb {
                name: topic.clone(),
                args: vec!["--help".to_string()],
            });
        }
        if rest
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
            && let Some(topic) = HelpTopic::parse(command)
        {
            return Ok(Self::Help { topic });
        }
        if rest.iter().any(|arg| is_explain_option(arg))
            && let Some(name) = standard_verb_name_for_explain(command)
        {
            return Ok(parse_dynamic_verb(
                name,
                &defaulted_dynamic_args_for_explain(name, rest),
            ));
        }
        match command.as_str() {
            "--version" | "version" => {
                ensure_no_args(rest, "--version")?;
                Ok(Self::Version)
            }
            "status" => {
                ensure_no_args(rest, "status")?;
                Ok(Self::Status)
            }
            "init" => parse_init(rest),
            "prime" => {
                ensure_no_args(rest, "prime")?;
                Ok(Self::Prime)
            }
            "context" => parse_context(rest),
            "search" => parse_search(rest),
            "read" => parse_read(rest),
            "handle" | "H" => parse_handle(rest),
            "check" => parse_check(rest),
            "describe" => match rest {
                [] => Ok(Self::Describe {
                    name: "runtime".to_string(),
                }),
                [name] if name.starts_with('-') => {
                    reject_runtime_compatibility_flag("describe", name)?;
                    Ok(Self::Describe { name: name.clone() })
                }
                [name] => Ok(Self::Describe { name: name.clone() }),
                _ => {
                    if let Some(flag) = rest.first().filter(|arg| arg.starts_with('-')) {
                        reject_runtime_compatibility_flag("describe", flag)?;
                    }
                    bail!(
                        "describe accepts at most one name; got {:?}",
                        rest.join(" ")
                    )
                }
            },
            "schema" => {
                ensure_no_args(rest, "schema")?;
                Ok(Self::Schema)
            }
            "save" => bail!("{}", retired_save_message()),
            "anneal" => bail!(
                "anneal anneal has been retired; bare `anneal` already runs `anneal status`, and goal-less orientation starts there"
            ),
            "-e" | "--eval" | "eval" => parse_eval(rest),
            other if other.starts_with('-') => bail!("unknown runtime option {other:?}"),
            other => {
                if let Some(message) = retired_command_message(other) {
                    bail!("{message}");
                }
                Ok(parse_dynamic_verb(other, rest))
            }
        }
    }
}

fn retired_command_message(command: &str) -> Option<&'static str> {
    match command {
        "cookbook" => Some(
            "anneal cookbook was folded into `anneal describe NAME`; use `anneal describe diagnostic` for worked joins or `anneal help eval` for query recipes",
        ),
        "vocab" => Some(
            "anneal vocab was folded into Code Mode queries; use `anneal describe runtime` for vocabulary recipes or `anneal -e '? *handle{status: status}.'`",
        ),
        "verbs" => Some(
            "anneal verbs was folded into introspection; use `anneal schema --format=text`, `anneal describe NAME`, or `anneal -e '? verbs(name, query, doc, output_schema).'",
        ),
        "examples" => Some(
            "anneal examples was folded into `anneal describe NAME`; use `anneal describe search` or query `examples(name, example)` with `anneal -e`",
        ),
        "save" => Some(retired_save_message()),
        "impact" => Some(
            "anneal impact has been retired; use `anneal handle <HANDLE> --impact` or compose `anneal -e '? impact(\"HANDLE\", affected, depth).'`",
        ),
        "find" => Some(
            "anneal find has been retired; use `anneal search TEXT` for content retrieval or compose `anneal -e '? *handle{id: h, kind: kind, status: status}, h contains \"TEXT\".'` for identity matching",
        ),
        "get" => Some(
            "anneal get has been retired; use `anneal handle <HANDLE>` for handle metadata and edges, or `anneal read <HANDLE>` for bounded content",
        ),
        "map" => Some(
            "anneal map has been retired; compose graph questions with `anneal -e '? *edge{from: src, to: dst, kind: kind}.'` or use `anneal handle <HANDLE>` for a local neighborhood",
        ),
        "health" => Some(
            "anneal health has been retired; use `anneal status` for the convergence header and compose diagnostics with `anneal -e '? diagnostic{code: code, severity: severity, subject: h, file: file, line: line}.'`",
        ),
        "diff" => Some(
            "anneal diff has been retired; use automatic status snapshots with `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`",
        ),
        "obligations" => Some(
            "anneal obligations has been retired; compose `anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'` or inspect `anneal describe undischarged`",
        ),
        "garden" => Some(
            "anneal garden has been retired; compose `frontier`, `primary_entropy`, and `*handle` with `anneal -e '? frontier(h, energy), primary_entropy(h, source), *handle{id: h, file: file, summary: summary}.'`, starting from `anneal status`",
        ),
        "orient" => Some(
            "anneal orient has been retired; start with `anneal status`, then run its `recent_frontier` and `ranked_anchor` queries for goal-less orientation or `anneal context \"GOAL\"` once you have a goal",
        ),
        "query" => Some(
            "anneal query has been retired; use the language directly with `anneal -e '? *handle{id: h}.'`",
        ),
        "explain" => Some(
            "anneal explain has been retired; use provenance on eval with `anneal -e '? diagnostic{code: code, subject: h, file: file, line: line}.' --explain`",
        ),
        "work" => Some(
            "anneal work has been retired; use `anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'` for ranked work, or `anneal status` for the convergence landing",
        ),
        "blocked" => Some(
            "anneal blocked has been retired; use `anneal -e '? blocker(h, energy, source), *handle{id: h, file: file, status: status}.'` or add `h = \"HANDLE\"` for a focused view",
        ),
        "diagnostics" => Some(
            "anneal diagnostics has been retired; use `anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'` for the full diagnostic stream or `anneal check` for the error-only CI gate",
        ),
        "broken" => Some(
            "anneal broken has been retired; use `anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'` for blockers or `anneal check` for the CI gate",
        ),
        "areas" => Some(
            "anneal areas has been retired; use `anneal -e '? area_health(area, grade, files, errors, cross_edges).'` or `anneal -e '? area_frontier(area, h, score, why).'`",
        ),
        "trend" => Some(
            "anneal trend has been retired; use `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'` for status changes between snapshots",
        ),
        "sources" => Some(
            "anneal sources has been retired; use `anneal -e '? sources(name, recognizes, capabilities, doc).'`",
        ),
        _ => None,
    }
}

fn skill_briefing_body(markdown: &str) -> &str {
    let trimmed = markdown.trim_start_matches(['\u{feff}']);
    let Some(rest) = trimmed.strip_prefix("---\n") else {
        return trimmed;
    };
    let Some(end) = rest.find("\n---\n") else {
        return trimmed;
    };
    rest[end + "\n---\n".len()..].trim_start_matches('\n')
}

fn retired_save_message() -> &'static str {
    "anneal save has been retired; edit anneal.dl directly and add an @verb(...) declaration, then verify with `anneal describe <name>` and a direct invocation"
}

struct RuntimeSession {
    root: Utf8PathBuf,
    program: Program,
    store: FactStore,
    registry: VerbRegistry,
    actor: ActorContext,
    sources: Vec<SourceInfo>,
    prelude_hash: String,
    git_mtimes: BTreeMap<String, String>,
}

struct CurrencyHitAnnotation {
    status: Option<String>,
    disposition: String,
    age_days: Option<i64>,
}

struct RuntimeRegistry {
    registry: VerbRegistry,
    actor: ActorContext,
}

impl RuntimeRegistry {
    fn load(root: &camino::Utf8Path) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let sources = available_source_info();
        let loaded_prelude = LoadedPrelude::load_active().map_err(prelude_error)?;
        if root.join("anneal.toml").is_file() {
            bail!(
                "anneal.toml is a legacy config file. Runtime commands use anneal.dl; run `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
            );
        }
        let project = if root.join(anneal_core::PROJECT_RULE_FILE).is_file() {
            Some(load_project_extension(
                root.as_std_path(),
                &sources,
                loaded_prelude.program(),
            )?)
        } else {
            None
        };
        let registry = match &project {
            Some(project) => VerbRegistry::from_layers(&[
                (VerbLayer::Prelude, loaded_prelude.program()),
                (VerbLayer::Project, project.program()),
            ])?,
            None => VerbRegistry::from_layers(&[(VerbLayer::Prelude, loaded_prelude.program())])?,
        };
        Ok(Self { registry, actor })
    }
}

impl RuntimeSession {
    fn load(root: &camino::Utf8Path, command: &RuntimeCommand) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let corpus = CorpusId::from(DEFAULT_CORPUS);
        let sources = available_source_info();
        let loaded_prelude = LoadedPrelude::load_active().map_err(prelude_error)?;
        let mut program = loaded_prelude.program().clone();
        let mut discovery = default_markdown_config();
        let has_legacy_toml = root.join("anneal.toml").is_file();
        if has_legacy_toml {
            bail!(
                "anneal.toml is a legacy config file. Runtime commands use anneal.dl; run `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
            );
        }
        let project = if root.join(anneal_core::PROJECT_RULE_FILE).is_file() {
            let extension = load_project_extension(root.as_std_path(), &sources, &program)?;
            merge_discovery(&mut discovery, extension.discovery());
            Some(extension)
        } else {
            None
        };
        if let Some(project) = &project {
            let (merged, warnings) = merge_program_layers(program, project.program().clone());
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

        let runtime_config = project
            .as_ref()
            .map_or_else(ConfigFacts::default, |project| {
                project.runtime_config().clone()
            });
        let config_facts = ConfigFacts::from_entries(discovery);
        let markdown_source = MarkdownSource::with_runtime_config(&runtime_config)
            .map_err(|err| anyhow!("markdown config failed: {err}"))?;
        let code_source = CodeSource;
        let roots = vec![root.to_path_buf()];
        let context = SourceContext {
            corpus: corpus.clone(),
            roots: roots.as_slice(),
            config_facts: &config_facts,
            probe_code_target_history: command.demands_code_target_history(),
            read_code_drift_evidence: command.demands_code_drift_evidence(),
            refresh_code_drift_evidence: command.refreshes_code_drift_evidence(),
            probe_edge_assertions: command.demands_edge_assertions()
                || command.refreshes_code_drift_evidence(),
            time_ref: None,
            previous_generation: Some(Generation::new(0)),
            actor: actor.clone(),
            cancellation: CancellationToken::new(),
        };
        let markdown_batch = markdown_source
            .extract(&context)
            .map_err(|err| anyhow!("markdown extraction failed: {err}"))?;
        let mut store = FactStore::default();
        store
            .merge(markdown_batch)
            .context("failed to merge markdown facts")?;
        if CodeSource::is_configured(&config_facts) {
            let code_batch = code_source
                .extract(&context)
                .map_err(|err| anyhow!("code extraction failed: {err}"))?;
            store
                .merge(code_batch)
                .context("failed to merge code facts")?;
        }
        let configs = runtime_config_facts(project.as_ref(), &corpus);
        if !configs.is_empty() {
            store
                .replace_configs(&corpus, configs)
                .context("failed to merge runtime config facts")?;
        }
        let git_mtimes = git_mtimes_for_files(
            root,
            store.handles().iter().map(|handle| handle.file.as_str()),
        );
        let history = read_snapshot_history(root).context("failed to read snapshot history")?;
        store.replace_snapshot_history(&history);
        let registry = match &project {
            Some(project) => VerbRegistry::from_layers(&[
                (VerbLayer::Prelude, loaded_prelude.program()),
                (VerbLayer::Project, project.program()),
            ])?,
            None => VerbRegistry::from_layers(&[(VerbLayer::Prelude, loaded_prelude.program())])?,
        };

        Ok(Self {
            root: root.to_path_buf(),
            program,
            store,
            registry,
            actor,
            sources,
            prelude_hash: loaded_prelude.set().hash().to_string(),
            git_mtimes,
        })
    }

    #[cfg(test)]
    fn load_for_test(root: &camino::Utf8Path) -> Result<Self> {
        Self::load(root, &RuntimeCommand::Schema)
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
                read_spans,
            } => {
                let command = ContextCommand::new(goal)
                    .with_budget(budget)
                    .with_hits(hits)
                    .with_neighborhood_depth(depth)
                    .include_low_confidence(include_low_confidence)
                    .read_spans(read_spans);
                let output = self.eval(command.datalog().as_str(), ExplainOptions::disabled())?;
                let output = command.group_rows(&output.rows)?;
                Ok(CommandOutput::Context(output))
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
                let output = self.eval(&query, ExplainOptions::disabled())?;
                let mut rows = output.rows;
                self.annotate_search_rows(&mut rows);
                Ok(CommandOutput::rows_with_warnings(
                    rows,
                    RowView::Search,
                    warning_texts(&output.warnings),
                ))
            }
            RuntimeCommand::Read {
                handle,
                budget,
                span_id,
            } => {
                let query = ReadCommand::new(handle)
                    .with_budget(budget)
                    .with_span_id(span_id)
                    .datalog();
                self.run_query(&query, ExplainOptions::disabled(), RowView::Read)
            }
            RuntimeCommand::Handle {
                handle,
                impact,
                lineage,
            } => self.run_handle(handle, impact, lineage),
            RuntimeCommand::Check { .. } => self.run_check_gate(),
            RuntimeCommand::Describe { name } => {
                let query = DescribeCommand::new(&name).datalog();
                let output = self.eval(&query, ExplainOptions::disabled())?;
                ensure!(
                    !output.rows.is_empty(),
                    "unknown runtime name {name:?}; use `anneal schema` or `anneal describe runtime`"
                );
                Ok(CommandOutput::rows(output.rows, RowView::Describe))
            }
            RuntimeCommand::Schema => self.run_verb("schema", RowView::Schema),
            RuntimeCommand::Eval {
                query,
                explain,
                limit,
            } => {
                let mut output = self.eval(&query, explain)?;
                if let Some(limit) = limit {
                    output.rows.truncate(limit);
                }
                let empty_binding_hint = self.empty_binding_hint_for_query(&query, &output.rows);
                Ok(CommandOutput::rows_with_empty_binding_hint_and_warnings(
                    output.rows,
                    RowView::Eval,
                    empty_binding_hint,
                    warning_texts(&output.warnings),
                ))
            }
            RuntimeCommand::Verb { name, args } => self.run_dynamic_verb(&name, &args),
            RuntimeCommand::Help { topic } => Ok(CommandOutput::Text(topic.render())),
            RuntimeCommand::Version | RuntimeCommand::Init { .. } | RuntimeCommand::Prime => {
                bail!("command is handled before runtime session load")
            }
        }
    }

    fn run_verb(&self, name: &str, view: RowView) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        self.run_query(plan.query_source(), ExplainOptions::disabled(), view)
    }

    fn run_handle(&self, handle: String, impact: bool, lineage: bool) -> Result<CommandOutput> {
        let mut output = self.eval(&handle_query(&handle), ExplainOptions::disabled())?;
        if output.rows.is_empty() && looks_like_retired_section_handle(&handle) {
            bail!("{}", retired_section_handle_message(&handle));
        }
        if impact {
            output.rows.extend(self.handle_impact_rows(&handle));
        }
        if lineage {
            output
                .rows
                .extend(handle_lineage_rows(&self.store, &handle));
        }
        Ok(CommandOutput::rows(
            output.rows,
            RowView::Handle {
                handle,
                impact,
                lineage,
            },
        ))
    }

    fn handle_impact_rows(&self, handle: &str) -> Vec<Row> {
        compute_handle_impact(&self.store, handle)
            .into_iter()
            .map(|dependency| impact_dependency_row(handle, dependency))
            .collect()
    }

    fn run_check_gate(&self) -> Result<CommandOutput> {
        let output = self.run_query(
            r#"? diagnostic{severity: "error", code: code, subject: subject, file: file, line: line, evidence: evidence}."#,
            ExplainOptions::disabled(),
            RowView::Broken,
        )?;
        let gate_failed = output.has_rows();
        Ok(output.with_gate_failed(gate_failed))
    }

    fn run_dynamic_verb(&self, name: &str, args: &[String]) -> Result<CommandOutput> {
        self.run_dynamic_verb_with_view(name, args, None)
    }

    fn run_dynamic_verb_with_view(
        &self,
        name: &str,
        args: &[String],
        view: Option<RowView>,
    ) -> Result<CommandOutput> {
        let entry = self.registry.resolve_for_actor(name, &self.actor)?;
        let invocation = DynamicVerbInvocation::parse(entry, args)?;
        if invocation.help {
            return Ok(CommandOutput::Text(render_dynamic_verb_help(entry)));
        }
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        let query = render_dynamic_verb_query(plan.query_source(), &invocation.bindings);
        let mut output = self.eval(&query, invocation.explain)?;
        if let Some(rows) = invocation.rows {
            output.rows.truncate(rows);
        }
        let empty_binding_hint = self.empty_binding_hint_for_query(&query, &output.rows);
        Ok(CommandOutput::rows_with_empty_binding_hint_and_warnings(
            output.rows,
            view.unwrap_or_else(|| RowView::Verb {
                name: plan.name().to_string(),
            }),
            empty_binding_hint,
            warning_texts(&output.warnings),
        ))
    }

    fn run_status(&self) -> Result<CommandOutput> {
        let snapshot_count_before = self.snapshot_history_count();
        let plan = self.registry.run_plan_for_actor("status", &self.actor)?;
        let output = self.eval(plan.query_source(), ExplainOptions::disabled())?;
        let append_outcome = match self.record_status_snapshot() {
            Ok(outcome) => Some(outcome),
            Err(err) => {
                eprintln!("warning: could not write automatic status snapshot: {err}");
                None
            }
        };
        let flow_baseline_ready = match append_outcome {
            Some(SnapshotAppendOutcome::Appended) if snapshot_count_before == 0 => false,
            _ => snapshot_count_before > 0,
        };
        Ok(CommandOutput::Status(StatusOutput {
            rows: output.rows,
            flow_baseline_ready,
        }))
    }

    fn record_status_snapshot(&self) -> Result<SnapshotAppendOutcome> {
        let entry = self.status_snapshot_entry();
        append_snapshot_entry_capped(&self.root, &entry, DEFAULT_AUTO_SNAPSHOT_LIMIT)
            .context("failed to append automatic status snapshot")
    }

    fn snapshot_history_count(&self) -> usize {
        self.store
            .snapshots()
            .iter()
            .map(|snapshot| snapshot.snapshot.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn status_snapshot_entry(&self) -> SnapshotEntry {
        let at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut facts = self
            .store
            .handles()
            .iter()
            .filter_map(|handle| {
                handle.status.as_ref().map(|status| {
                    SnapshotEntryFact::new(handle.id.as_str(), "status", status.as_str())
                })
            })
            .collect::<Vec<_>>();
        facts.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.key.cmp(&right.key))
                .then_with(|| left.value.cmp(&right.value))
        });
        SnapshotEntry::with_prelude_hash(
            format!("status-{at}"),
            at,
            CorpusId::from(DEFAULT_CORPUS),
            self.prelude_hash.clone(),
            facts,
        )
    }

    fn run_query(
        &self,
        query: &str,
        explain: ExplainOptions,
        view: RowView,
    ) -> Result<CommandOutput> {
        let output = self.eval(query, explain)?;
        Ok(CommandOutput::rows_with_warnings(
            output.rows,
            view,
            warning_texts(&output.warnings),
        ))
    }

    fn annotate_search_rows(&self, rows: &mut [Row]) {
        let handles = rows
            .iter()
            .filter_map(|row| match row.fields.get("h") {
                Some(Value::String(handle)) => Some(handle.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let annotations = self.currency_hit_annotations(&handles);
        for row in rows {
            let Some(Value::String(handle)) = row.fields.get("h") else {
                continue;
            };
            let Some(annotation) = annotations.get(handle.as_str()) else {
                continue;
            };
            row.fields.insert(
                "status".to_string(),
                annotation
                    .status
                    .as_ref()
                    .map_or(Value::Null, |status| Value::String(status.clone())),
            );
            row.fields.insert(
                "disposition".to_string(),
                Value::String(annotation.disposition.clone()),
            );
            row.fields.insert(
                "age_days".to_string(),
                annotation
                    .age_days
                    .map_or(Value::Null, |days| Value::Number(NumberValue::Int(days))),
            );
        }
    }

    fn currency_hit_annotations(
        &self,
        handles: &BTreeSet<String>,
    ) -> BTreeMap<&str, CurrencyHitAnnotation> {
        let today = Utc::now().date_naive();
        let mut superseded = BTreeSet::new();
        let mut successors = BTreeSet::new();
        for edge in self.store.edges() {
            if edge.kind != "Supersedes" {
                continue;
            }
            if handles.contains(edge.from.as_str()) {
                superseded.insert(edge.from.as_str());
            }
            if handles.contains(edge.to.as_str()) {
                successors.insert(edge.to.as_str());
            }
        }
        self.store
            .handles()
            .iter()
            .filter(|handle| handles.contains(handle.id.as_str()))
            .map(|handle| {
                let age_days = handle
                    .date
                    .as_deref()
                    .and_then(|date| authored_age_days(date, today));
                let disposition = if handle.kind == "file" {
                    currency_disposition(handle.id.as_str(), &superseded, &successors)
                } else {
                    "unknown"
                };
                (
                    handle.id.as_str(),
                    CurrencyHitAnnotation {
                        status: handle.status.clone(),
                        disposition: disposition.to_string(),
                        age_days,
                    },
                )
            })
            .collect()
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
            .with_sources(self.sources.clone())
            .with_git_mtimes(self.git_mtimes.clone());
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator
            .run_fixpoint_for_query(&query)
            .context("query fixpoint failed")?;
        let mut output = evaluator
            .eval_query(&query)
            .context("query evaluation failed")?;
        output
            .warnings
            .retain(|warning| warning_applies_to_query(query_source, warning));
        if let Some(warning) = retired_section_kind_warning(&query.query().body) {
            output.warnings.push(warning);
        }
        Ok(output)
    }

    fn empty_binding_hint_for_query(&self, query_source: &str, rows: &[Row]) -> Option<String> {
        if rows.is_empty() || rows.iter().any(|row| !row.fields.is_empty()) {
            return None;
        }
        let mut program = self.program.clone();
        let query_program = parse_program("cli-query", query_source).ok()?;
        program.statements.extend(query_program.statements);
        let analyzed = analyze(program).ok()?;
        let query = analyzed.queries().next()?.query();
        empty_binding_example(&analyzed, &query.body)
    }
}

impl RuntimeCommand {
    fn demands_code_target_history(&self) -> bool {
        match self {
            Self::Status | Self::Verb { .. } | Self::Check { .. } | Self::Handle { .. } => true,
            Self::Eval { query, .. } => query_demands_code_target_history(query),
            Self::Describe { name } => matches!(
                name.as_str(),
                "W006"
                    | "spec_code_drift"
                    | "target_exists"
                    | "target_history_status"
                    | "target_probe_base"
                    | "target_resolved_path"
            ),
            Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Schema
            | Self::Help { .. } => false,
        }
    }

    fn demands_code_drift_evidence(&self) -> bool {
        match self {
            Self::Status | Self::Check { .. } | Self::Handle { .. } => true,
            Self::Eval { query, .. } => query_demands_code_drift_evidence(query),
            Self::Describe { name } => matches!(
                name.as_str(),
                "referent_disposition"
                    | "assertion_drift"
                    | "referent_moved_head"
                    | "drift_profile"
            ),
            Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Schema
            | Self::Verb { .. }
            | Self::Help { .. } => false,
        }
    }

    const fn refreshes_code_drift_evidence(&self) -> bool {
        matches!(
            self,
            Self::Check {
                refresh_drift: true
            }
        )
    }

    fn demands_edge_assertions(&self) -> bool {
        match self {
            Self::Eval { query, .. } => query_demands_edge_assertions(query),
            Self::Describe { name } => {
                matches!(
                    name.as_str(),
                    "edge" | "*edge" | "assertion_date" | "assertion_revision"
                )
            }
            Self::Status
            | Self::Verb { .. }
            | Self::Check { .. }
            | Self::Version
            | Self::Init { .. }
            | Self::Prime
            | Self::Search { .. }
            | Self::Context { .. }
            | Self::Read { .. }
            | Self::Handle { .. }
            | Self::Schema
            | Self::Help { .. } => false,
        }
    }
}

fn query_demands_code_target_history(query: &str) -> bool {
    [
        "diagnostic",
        "spec_code_drift",
        "target_exists",
        "target_history_status",
        "target_probe_base",
        "target_resolved_path",
        "entropy",
        "primary_entropy",
        "potential",
        "potential_subject",
        "frontier",
        "ranked_work",
        "area_frontier",
        "blocked",
        "blocker",
        "holding",
        "flow",
        "status_item",
        "status_metric",
    ]
    .iter()
    .any(|needle| query_contains_identifier(query, needle))
}

fn query_demands_code_drift_evidence(query: &str) -> bool {
    [
        "referent_disposition",
        "assertion_drift",
        "referent_moved_head",
        "drift_profile",
        "code_ref",
        "code.referent_disposition",
        "code.referent_commits_since",
        "code.referent_moved_to",
        "code.referent_move_candidate",
    ]
    .iter()
    .any(|needle| query_contains_identifier(query, needle))
}

fn query_demands_edge_assertions(query: &str) -> bool {
    ["assertion_date", "assertion_revision"]
        .iter()
        .any(|needle| query_contains_identifier(query, needle))
}

fn query_contains_identifier(query: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = query[start..].find(needle) {
        let match_start = start + offset;
        let match_end = match_start + needle.len();
        let before = query[..match_start].chars().next_back();
        let after = query[match_end..].chars().next();
        if before.is_none_or(|ch| !is_ident_char(ch)) && after.is_none_or(|ch| !is_ident_char(ch)) {
            return true;
        }
        start = match_end;
    }
    false
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn empty_binding_example(analyzed: &AnalyzedProgram, body: &Body) -> Option<String> {
    for atom in &body.atoms {
        match atom {
            Atom::Stored(stored) => {
                let example = empty_binding_example_for_stored(stored)?;
                return Some(example);
            }
            Atom::Derived(derived) => {
                if !is_introspection_predicate(derived.predicate.name.as_str()) {
                    let example = empty_binding_example_for_derived(analyzed, derived)?;
                    return Some(example);
                }
            }
            Atom::TimeBlock(time_block) => {
                if let Some(example) = empty_binding_example(analyzed, &time_block.body) {
                    return Some(example);
                }
            }
            Atom::Aggregation(aggregate) => {
                if let Some(example) = empty_binding_example(analyzed, &aggregate.body) {
                    return Some(example);
                }
            }
            Atom::Comparison(_) | Atom::Negation(_) => {}
        }
    }
    None
}

fn warning_texts(warnings: &[QueryWarning]) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| format!("warning: {}", warning.message))
        .collect()
}

fn authored_age_days(date: &str, today: NaiveDate) -> Option<i64> {
    let authored = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(today.signed_duration_since(authored).num_days().max(0))
}

fn currency_disposition(
    handle: &str,
    superseded: &BTreeSet<&str>,
    successors: &BTreeSet<&str>,
) -> &'static str {
    if superseded.contains(handle) {
        "superseded"
    } else if successors.contains(handle) {
        "current_head"
    } else {
        "current"
    }
}

fn warning_applies_to_query(query_source: &str, warning: &QueryWarning) -> bool {
    warning.reference.as_deref().is_none_or(|reference| {
        query_source.contains(reference)
            || query_source.contains(&format!("at({})", datalog_string_literal(reference)))
    })
}

fn retired_section_kind_warning(body: &Body) -> Option<QueryWarning> {
    body_filters_retired_section_kind(body).then(|| QueryWarning {
        code: "retired_section_kind".to_string(),
        message: "the section handle kind was retired in v0.14; use `*span{id: span_id, handle: file, summary: heading}` for heading spans".to_string(),
        reference: None,
        source: None,
        relation: Some("handle".to_string()),
    })
}

fn body_filters_retired_section_kind(body: &Body) -> bool {
    body.atoms.iter().any(atom_filters_retired_section_kind)
}

fn atom_filters_retired_section_kind(atom: &Atom) -> bool {
    match atom {
        Atom::Stored(stored) => stored_filters_retired_section_kind(stored),
        Atom::Aggregation(aggregate) => body_filters_retired_section_kind(&aggregate.body),
        Atom::Negation(negation) => negated_atom_filters_retired_section_kind(&negation.atom),
        Atom::TimeBlock(time_block) => body_filters_retired_section_kind(&time_block.body),
        Atom::Derived(_) | Atom::Comparison(_) => false,
    }
}

fn negated_atom_filters_retired_section_kind(atom: &NegatedAtom) -> bool {
    match atom {
        NegatedAtom::Stored(stored) => stored_filters_retired_section_kind(stored),
        NegatedAtom::Derived(_) => false,
    }
}

fn stored_filters_retired_section_kind(stored: &StoredAtom) -> bool {
    if stored.relation.as_str() != "handle" {
        return false;
    }
    if stored_literal_field(stored, "source").is_some_and(|source| source != "markdown") {
        return false;
    }
    stored_literal_field(stored, "kind").is_some_and(|kind| kind == "section")
}

fn stored_literal_field<'a>(stored: &'a StoredAtom, name: &str) -> Option<&'a str> {
    stored.fields.iter().find_map(|field| {
        (field.field.as_str() == name).then(|| match field.term.expr() {
            Some(Expr::Literal(Literal::String(value))) => Some(value.as_str()),
            _ => None,
        })?
    })
}

fn empty_binding_example_for_stored(stored: &StoredAtom) -> Option<String> {
    let fields = stored_relation_fields(stored.relation.as_str())?;
    let existing_fields = stored
        .fields
        .iter()
        .map(|field| field.field.as_str())
        .collect::<BTreeSet<_>>();
    let field = fields
        .as_slice()
        .iter()
        .copied()
        .find(|field| !existing_fields.contains(field))?;
    let mut parts = render_literal_field_patterns(&stored.fields);
    parts.push(format!("{field}: {}", variable_for_field(field)));
    Some(format!("? *{}{{{}}}.", stored.relation, parts.join(", ")))
}

fn empty_binding_example_for_derived(
    analyzed: &AnalyzedProgram,
    derived: &DerivedAtom,
) -> Option<String> {
    let fields = analyzed.predicate_parameter_names(&derived.predicate)?;
    if matches!(derived.style, CallStyle::Pattern)
        || derived
            .args
            .iter()
            .any(|arg| matches!(arg, CallArg::Named { .. } | CallArg::Wildcard { .. }))
    {
        return empty_binding_example_for_pattern_derived(derived, &fields);
    }
    empty_binding_example_for_positional_derived(derived, &fields)
}

fn empty_binding_example_for_pattern_derived(
    derived: &DerivedAtom,
    fields: &[String],
) -> Option<String> {
    let suggested_index = derived
        .args
        .iter()
        .position(|arg| matches!(arg, CallArg::Wildcard { .. }))
        .unwrap_or(0);
    let field = fields.get(suggested_index).map(String::as_str)?;
    let mut parts = derived
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            if index == suggested_index {
                return None;
            }
            let field = fields.get(index)?;
            match arg {
                CallArg::Named { expr, .. } | CallArg::Positional { expr, .. } => {
                    render_literal_expr(expr).map(|value| format!("{field}: {value}"))
                }
                CallArg::Wildcard { .. } => None,
            }
        })
        .collect::<Vec<_>>();
    parts.push(format!("{field}: {}", variable_for_field(field)));
    Some(format!(
        "? {}{{{}}}.",
        derived.predicate.name,
        parts.join(", ")
    ))
}

fn empty_binding_example_for_positional_derived(
    derived: &DerivedAtom,
    fields: &[String],
) -> Option<String> {
    let arity = derived.args.len();
    if arity == 0 {
        return None;
    }
    let suggested_index = derived
        .args
        .iter()
        .position(|arg| !matches!(arg, CallArg::Wildcard { .. }))
        .unwrap_or(0);
    let args = (0..arity)
        .map(|index| {
            if index == suggested_index {
                Some(
                    fields
                        .get(index)
                        .map_or_else(|| "value".to_string(), |field| variable_for_field(field)),
                )
            } else {
                render_call_arg(&derived.args[index])
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "? {}({}).",
        derived.predicate.name,
        args.join(", ")
    ))
}

fn render_literal_field_patterns(fields: &[anneal_core::runtime::FieldPattern]) -> Vec<String> {
    fields
        .iter()
        .filter_map(|field| {
            let expr = field.term.expr()?;
            render_literal_expr(expr).map(|value| format!("{}: {value}", field.field))
        })
        .collect()
}

fn render_call_arg(arg: &CallArg) -> Option<String> {
    match arg {
        CallArg::Positional { expr, .. } | CallArg::Named { expr, .. } => render_literal_expr(expr),
        CallArg::Wildcard { .. } => Some("_".to_string()),
    }
}

fn render_literal_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(literal) => Some(render_literal(literal)),
        Expr::Var(_) | Expr::FunctionCall { .. } | Expr::Binary { .. } | Expr::Tuple(_) => None,
    }
}

fn render_literal(literal: &Literal) -> String {
    match literal {
        Literal::String(value) => datalog_string_literal(value),
        Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
        Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_string(),
        Literal::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(render_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn variable_for_field(field: &str) -> String {
    match field {
        "id" | "h" | "handle" | "subject" => "h".to_string(),
        "from" => "src".to_string(),
        "to" => "dst".to_string(),
        "affected" => "affected".to_string(),
        "depth" => "depth".to_string(),
        "code" => "code".to_string(),
        "severity" => "severity".to_string(),
        "file" => "file".to_string(),
        "line" => "line".to_string(),
        "energy" | "score" | "weight" => field.to_string(),
        "source" => "source".to_string(),
        "area" => "area".to_string(),
        "count" => "count".to_string(),
        "status" => "status".to_string(),
        "kind" => "kind".to_string(),
        "evidence" => "evidence".to_string(),
        other => other
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect(),
    }
}

fn is_introspection_predicate(name: &str) -> bool {
    matches!(
        name,
        "schema" | "predicates" | "verbs" | "describe" | "examples" | "sources"
    )
}

fn runtime_config_facts(
    project: Option<&ProjectExtension>,
    corpus: &CorpusId,
) -> Vec<anneal_core::ConfigFact> {
    project.map_or_else(Vec::new, |project| project.runtime_config_facts(corpus))
}

fn git_mtimes_for_files<'a>(
    root: &camino::Utf8Path,
    files: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, String> {
    if !is_inside_git_work_tree(root) {
        return BTreeMap::new();
    }

    let files = files
        .into_iter()
        .filter(|file| !file.is_empty())
        .collect::<BTreeSet<_>>();
    if files.is_empty() {
        return BTreeMap::new();
    }

    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["log", "--relative", "--format=%cI", "--name-only", "--"])
        .args(files.iter().copied())
        .output()
    else {
        return BTreeMap::new();
    };
    if !output.status.success() {
        return BTreeMap::new();
    }

    let mut mtimes = BTreeMap::new();
    let mut current_instant = None::<String>;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
    {
        if line.is_empty() {
            continue;
        }
        if DateTime::parse_from_rfc3339(line).is_ok() {
            current_instant = Some(line.to_string());
            continue;
        }
        if files.contains(line)
            && !mtimes.contains_key(line)
            && let Some(instant) = &current_instant
        {
            mtimes.insert(line.to_string(), instant.clone());
            if mtimes.len() == files.len() {
                break;
            }
        }
    }
    mtimes
}

fn is_inside_git_work_tree(root: &camino::Utf8Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImpactDependency {
    handle: String,
    depth: u32,
    kind: String,
    file: String,
    line: u32,
}

fn compute_handle_impact(store: &FactStore, handle: &str) -> Vec<ImpactDependency> {
    let traverse = impact_traverse_set(store.configs());
    let mut incoming = BTreeMap::<&str, Vec<&EdgeFact>>::new();
    for edge in store.edges() {
        if traverse.contains(edge.kind.as_str()) {
            incoming.entry(edge.to.as_str()).or_default().push(edge);
        }
    }

    let mut dependencies = Vec::new();
    let mut seen = BTreeSet::from([handle.to_string()]);
    let mut queue = VecDeque::from([(handle.to_string(), 0_u32)]);
    while let Some((current, depth)) = queue.pop_front() {
        let Some(edges) = incoming.get(current.as_str()) else {
            continue;
        };
        for edge in edges {
            if !seen.insert(edge.from.clone()) {
                continue;
            }
            let next_depth = depth.saturating_add(1);
            dependencies.push(ImpactDependency {
                handle: edge.from.clone(),
                depth: next_depth,
                kind: edge.kind.clone(),
                file: edge.file.clone(),
                line: edge.line,
            });
            queue.push_back((edge.from.clone(), next_depth));
        }
    }
    dependencies
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineageNode {
    handle: String,
    role: &'static str,
    depth: u32,
    disposition: &'static str,
    is_head: bool,
    status: Option<String>,
    file: String,
    line: u32,
    summary: String,
}

fn handle_lineage_rows(store: &FactStore, handle: &str) -> Vec<Row> {
    let Some(root) = resolve_lineage_file_handle(store, handle) else {
        return Vec::new();
    };
    compute_file_lineage(store, root.as_str())
        .into_iter()
        .map(|node| lineage_node_row(handle, root.as_str(), node))
        .collect()
}

fn resolve_lineage_file_handle(store: &FactStore, handle: &str) -> Option<String> {
    let file_handles = store
        .handles()
        .iter()
        .filter(|fact| fact.kind == "file")
        .map(|fact| fact.id.as_str())
        .collect::<BTreeSet<_>>();
    if file_handles.contains(handle) {
        return Some(handle.to_string());
    }

    if let Some(resolved) = store
        .meta()
        .iter()
        .find(|fact| fact.handle == handle && fact.key == RESOLVED_FILE_META_KEY)
        .map(|fact| fact.value.as_str())
        && file_handles.contains(resolved)
    {
        return Some(resolved.to_string());
    }

    let stem_matches = file_handles
        .iter()
        .filter(|candidate| file_handle_stem(candidate).is_some_and(|stem| stem == handle))
        .copied()
        .collect::<Vec<_>>();
    match stem_matches.as_slice() {
        [single] => Some((*single).to_string()),
        _ => None,
    }
}

fn file_handle_stem(handle: &str) -> Option<&str> {
    let file_name = handle.rsplit('/').next().unwrap_or(handle);
    file_name.strip_suffix(".md")
}

fn compute_file_lineage(store: &FactStore, root: &str) -> Vec<LineageNode> {
    let file_handles = store
        .handles()
        .iter()
        .filter(|fact| fact.kind == "file")
        .map(|fact| fact.id.as_str())
        .collect::<BTreeSet<_>>();
    let handle_index = store
        .handles()
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut successors = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut predecessors = BTreeMap::<&str, BTreeSet<&str>>::new();
    for edge in store.edges() {
        if edge.kind != SUPERSEDES_EDGE_KIND
            || !file_handles.contains(edge.from.as_str())
            || !file_handles.contains(edge.to.as_str())
        {
            continue;
        }
        successors
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
        predecessors
            .entry(edge.to.as_str())
            .or_default()
            .insert(edge.from.as_str());
    }

    let successor_depths = lineage_distances(root, &successors);
    let predecessor_depths = lineage_distances(root, &predecessors);
    let mut all_handles = BTreeSet::from([root]);
    all_handles.extend(successor_depths.keys().copied());
    all_handles.extend(predecessor_depths.keys().copied());

    all_handles
        .into_iter()
        .filter_map(|handle| {
            let fact = handle_index.get(handle).copied()?;
            let successor_depth = successor_depths.get(handle).copied();
            let predecessor_depth = predecessor_depths.get(handle).copied();
            let role = if handle == root {
                "root"
            } else if successor_depth.is_some() {
                "successor"
            } else if predecessor_depth.is_some() {
                "predecessor"
            } else {
                "related"
            };
            let depth = successor_depth.or(predecessor_depth).unwrap_or(0);
            let is_superseded = successors
                .get(handle)
                .is_some_and(|edges| !edges.is_empty());
            let is_head = !is_superseded
                && predecessors
                    .get(handle)
                    .is_some_and(|edges| !edges.is_empty());
            let disposition = if is_superseded {
                "superseded"
            } else if is_head {
                "current_head"
            } else {
                "current"
            };
            Some(LineageNode {
                handle: handle.to_string(),
                role,
                depth,
                disposition,
                is_head,
                status: fact.status.clone(),
                file: fact.file.clone(),
                line: fact.line,
                summary: fact.summary.clone(),
            })
        })
        .collect()
}

fn lineage_distances<'a>(
    root: &'a str,
    graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> BTreeMap<&'a str, u32> {
    let mut distances = BTreeMap::new();
    let mut seen = BTreeSet::from([root]);
    let mut queue = VecDeque::from([(root, 0_u32)]);
    while let Some((current, depth)) = queue.pop_front() {
        let Some(next_nodes) = graph.get(current) else {
            continue;
        };
        for next in next_nodes {
            if !seen.insert(*next) {
                continue;
            }
            let next_depth = depth.saturating_add(1);
            distances.insert(*next, next_depth);
            queue.push_back((*next, next_depth));
        }
    }
    distances
}

fn impact_traverse_set(configs: &[ConfigFact]) -> BTreeSet<&str> {
    let configured = configs
        .iter()
        .filter(|fact| fact.key == IMPACT_TRAVERSE_CONFIG_KEY)
        .map(|fact| fact.value.as_str())
        .collect::<BTreeSet<_>>();
    if configured.is_empty() {
        DEFAULT_IMPACT_TRAVERSE.iter().copied().collect()
    } else {
        configured
    }
}

fn lineage_node_row(requested: &str, normalized_root: &str, node: LineageNode) -> Row {
    Row {
        fields: BTreeMap::from([
            ("h".to_string(), Value::String(requested.to_string())),
            ("relation".to_string(), Value::String("lineage".to_string())),
            ("other".to_string(), Value::String(node.handle)),
            (
                "kind".to_string(),
                Value::String(SUPERSEDES_EDGE_KIND.to_string()),
            ),
            (
                "status".to_string(),
                node.status.map_or(Value::Null, Value::String),
            ),
            ("file".to_string(), Value::String(node.file)),
            (
                "line".to_string(),
                Value::Number(NumberValue::Int(i64::from(node.line))),
            ),
            ("summary".to_string(), Value::String(node.summary)),
            ("role".to_string(), Value::String(node.role.to_string())),
            (
                "depth".to_string(),
                Value::Number(NumberValue::Int(i64::from(node.depth))),
            ),
            (
                "disposition".to_string(),
                Value::String(node.disposition.to_string()),
            ),
            ("head".to_string(), Value::Bool(node.is_head)),
            (
                "normalized_root".to_string(),
                Value::String(normalized_root.to_string()),
            ),
        ]),
        derivation: None,
    }
}

fn impact_dependency_row(root: &str, dependency: ImpactDependency) -> Row {
    Row {
        fields: BTreeMap::from([
            ("h".to_string(), Value::String(root.to_string())),
            ("relation".to_string(), Value::String("impact".to_string())),
            ("other".to_string(), Value::String(dependency.handle)),
            ("kind".to_string(), Value::String(dependency.kind)),
            ("status".to_string(), Value::Null),
            ("file".to_string(), Value::String(dependency.file)),
            (
                "line".to_string(),
                Value::Number(NumberValue::Int(i64::from(dependency.line))),
            ),
            ("summary".to_string(), Value::String(String::new())),
            (
                "depth".to_string(),
                Value::Number(NumberValue::Int(i64::from(dependency.depth))),
            ),
        ]),
        derivation: None,
    }
}

enum CommandOutput {
    Rows {
        rows: Vec<Row>,
        view: RowView,
        gate_failed: bool,
        empty_binding_hint: Option<String>,
        warnings: Vec<String>,
    },
    Status(StatusOutput),
    Context(ContextOutput),
    Text(String),
}

struct InitCommandOutput {
    inner: anneal_md::InitOutput,
}

impl InitCommandOutput {
    fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match mode {
            OutputMode::Human => write_init_text(writer, &self.inner),
            OutputMode::Json | OutputMode::JsonExplicit => write_json_object(writer, &self.inner),
        }
    }
}

struct StatusOutput {
    rows: Vec<Row>,
    flow_baseline_ready: bool,
}

impl CommandOutput {
    const fn rows(rows: Vec<Row>, view: RowView) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint: None,
            warnings: Vec::new(),
        }
    }

    fn rows_with_warnings(rows: Vec<Row>, view: RowView, warnings: Vec<String>) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint: None,
            warnings,
        }
    }

    #[cfg(test)]
    fn rows_with_empty_binding_hint(
        rows: Vec<Row>,
        view: RowView,
        empty_binding_hint: Option<String>,
    ) -> Self {
        Self::rows_with_empty_binding_hint_and_warnings(rows, view, empty_binding_hint, Vec::new())
    }

    fn rows_with_empty_binding_hint_and_warnings(
        rows: Vec<Row>,
        view: RowView,
        empty_binding_hint: Option<String>,
        warnings: Vec<String>,
    ) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
            empty_binding_hint,
            warnings,
        }
    }

    fn with_gate_failed(self, gate_failed: bool) -> Self {
        match self {
            Self::Rows {
                rows,
                view,
                empty_binding_hint,
                warnings,
                ..
            } => Self::Rows {
                rows,
                view,
                gate_failed,
                empty_binding_hint,
                warnings,
            },
            other => other,
        }
    }

    fn has_rows(&self) -> bool {
        match self {
            Self::Rows { rows, .. } => !rows.is_empty(),
            Self::Status(output) => !output.rows.is_empty(),
            Self::Context(_) | Self::Text(_) => false,
        }
    }

    fn has_displayable_content(&self) -> bool {
        match self {
            Self::Rows { rows, .. } => !rows.is_empty(),
            Self::Status(output) => !output.rows.is_empty(),
            Self::Context(output) => {
                !output.hits.is_empty()
                    || !output.spans.is_empty()
                    || !output.neighborhood.is_empty()
            }
            Self::Text(_) => false,
        }
    }

    const fn gate_failed(&self) -> bool {
        match self {
            Self::Rows { gate_failed, .. } => *gate_failed,
            Self::Status(_) | Self::Context(_) | Self::Text(_) => false,
        }
    }

    fn empty_rows_diagnostic(&self, mode: OutputMode) -> Option<&'static str> {
        match (mode, self) {
            (_, Self::Rows { rows, .. })
            | (
                OutputMode::Json | OutputMode::JsonExplicit,
                Self::Status(StatusOutput { rows, .. }),
            ) if !matches!(mode, OutputMode::Human) && rows.is_empty() => {
                Some(EMPTY_ROWS_DIAGNOSTIC)
            }
            (_, Self::Status(_) | Self::Rows { .. } | Self::Context(_) | Self::Text(_)) => None,
        }
    }

    fn stderr_diagnostic(&self, mode: OutputMode) -> Option<String> {
        let mut messages = Vec::new();
        if let Self::Rows { warnings, .. } = self {
            messages.extend(warnings.iter().cloned());
        }
        if let Some(message) = self.empty_rows_diagnostic(mode) {
            messages.push(message.to_string());
        }
        match (mode, self) {
            (
                OutputMode::Json | OutputMode::JsonExplicit,
                Self::Rows {
                    rows,
                    empty_binding_hint: Some(example),
                    ..
                },
            ) if zero_binding_rows(rows) => {
                messages.push(empty_binding_hint_text(rows.len(), example));
            }
            _ => {}
        }
        (!messages.is_empty()).then(|| messages.join("\n"))
    }

    fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match (mode, self) {
            (OutputMode::Human, Self::Status(output)) => {
                write_status_text(writer, &output.rows, output.flow_baseline_ready)?;
            }
            (OutputMode::Human, Self::Context(output)) => write_context_text(writer, &output)?,
            (
                OutputMode::Human,
                Self::Rows {
                    rows,
                    view,
                    empty_binding_hint,
                    ..
                },
            ) => {
                write_rows_text(writer, &rows, &view, empty_binding_hint.as_deref())?;
            }
            (
                OutputMode::Json,
                Self::Rows {
                    rows,
                    view: RowView::Describe,
                    ..
                },
            ) => write_describe_text(writer, &rows)?,
            (_, Self::Status(output)) => write_ndjson(writer, output.rows)?,
            (_, Self::Rows { rows, .. }) => write_ndjson(writer, rows)?,
            (_, Self::Context(output)) => write_context_ndjson(writer, &output)?,
            (_, Self::Text(text)) => write_text(writer, &text)?,
        }
        Ok(())
    }
}

fn zero_binding_rows(rows: &[Row]) -> bool {
    !rows.is_empty() && rows.iter().all(|row| row.fields.is_empty())
}

fn empty_binding_hint_text(row_count: usize, example: &str) -> String {
    format!(
        "hint: matched {row_count} rows but no fields are bound for output.\n\
         Add a variable to extract values, e.g.:\n  {example}"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum RowView {
    Search,
    Read,
    Handle {
        handle: String,
        impact: bool,
        lineage: bool,
    },
    Broken,
    Describe,
    Schema,
    Eval,
    Verb {
        name: String,
    },
}

impl RowView {
    fn heading(&self, count: usize) -> Option<String> {
        let heading = match self {
            Self::Search => format!("Search ({count})"),
            Self::Read => format!("Read ({count})"),
            Self::Handle { handle, .. } => format!("Handle {handle} ({count} edges)"),
            Self::Broken => format!("Broken ({count})"),
            Self::Describe => return None,
            Self::Schema => format!("Schema ({count})"),
            Self::Eval => format!("Results ({count})"),
            Self::Verb { name } => format!("{name} ({count})"),
        };
        Some(heading)
    }
}

fn render_dynamic_verb_help(entry: &VerbEntry) -> String {
    let name = entry.name();
    let usage_args = entry
        .args()
        .iter()
        .filter(|arg| arg.default().is_none())
        .fold(String::new(), |mut out, arg| {
            let _ = write!(out, " <{}>", arg.name().to_ascii_uppercase());
            out
        });
    let schema = entry.output_schema().to_string();
    let args = if entry.args().is_empty() {
        "  none".to_string()
    } else {
        entry
            .args()
            .iter()
            .map(|arg| match arg.default() {
                Some(default) => {
                    format!("  {}: {} = {default}", arg.name(), arg.kind())
                }
                None => format!("  {}: {}", arg.name(), arg.kind()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let capabilities = if entry.capabilities().is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            entry
                .capabilities()
                .iter()
                .map(VerbCapability::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        "\
Usage: anneal [OPTIONS] {name} [OPTIONS]{usage_args}

{doc}

This is a saved @verb projected from the resolved VerbRegistry. Use it like a
standard verb, or inspect/modify the underlying query with `anneal describe {name}`,
`anneal schema`, and `anneal -e`.

Options:
      --rows <N>                 Cap returned rows after evaluation
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Arguments:
{args}

Output schema:
  {schema}

Capabilities: {capabilities}
Source: {source}:{line}

Query:
  {query}

Global options:
      --root <PATH>              Corpus root (default: nearest .design, docs, or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json>       Force readable text or JSON/NDJSON output
",
        doc = entry.doc(),
        source = entry.source().location().source_name,
        line = entry.source().location().line,
        query = entry.query_source(),
    )
}

#[derive(Debug, PartialEq)]
struct DynamicVerbInvocation {
    bindings: Vec<(String, Literal)>,
    explain: ExplainOptions,
    rows: Option<usize>,
    help: bool,
}

impl DynamicVerbInvocation {
    fn parse(entry: &VerbEntry, raw_args: &[String]) -> Result<Self> {
        DynamicVerbParser::new(entry).parse(raw_args)
    }
}

struct DynamicVerbParser<'a> {
    entry: &'a VerbEntry,
    values: BTreeMap<String, Literal>,
    next_positional: usize,
    explain: ExplainOptions,
    rows: Option<usize>,
    help: bool,
}

impl<'a> DynamicVerbParser<'a> {
    fn new(entry: &'a VerbEntry) -> Self {
        Self {
            entry,
            values: BTreeMap::new(),
            next_positional: 0,
            explain: ExplainOptions::disabled(),
            rows: None,
            help: false,
        }
    }

    fn parse(mut self, raw_args: &[String]) -> Result<DynamicVerbInvocation> {
        let mut iter = raw_args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-h" | "--help" => self.help = true,
                "--rows" => {
                    self.rows = Some(parse_usize(next_value(&mut iter, "--rows")?, "--rows")?);
                }
                value if value.starts_with("--rows=") => {
                    self.rows = Some(parse_usize(value_after_equals(value), "--rows")?);
                }
                "--explain" => self.explain = self.explain.with_first_rows(3),
                "--explain-depth" => {
                    let depth = parse_positive_usize(
                        next_value(&mut iter, "--explain-depth")?,
                        "--explain-depth",
                    )?;
                    self.explain = self.explain.with_depth_limit(depth);
                }
                value if value.starts_with("--explain-depth=") => {
                    let depth = parse_positive_usize(value_after_equals(value), "--explain-depth")?;
                    self.explain = self.explain.with_depth_limit(depth);
                }
                "--explain-first" => {
                    let rows = parse_positive_usize(
                        next_value(&mut iter, "--explain-first")?,
                        "--explain-first",
                    )?;
                    self.explain = self.explain.with_first_rows(rows);
                }
                value if value.starts_with("--explain-first=") => {
                    let rows = parse_positive_usize(value_after_equals(value), "--explain-first")?;
                    self.explain = self.explain.with_first_rows(rows);
                }
                "--explain-all" => self.explain = self.explain.with_all_rows(),
                value if value.starts_with("--") => self.parse_named(value, &mut iter)?,
                value if value.starts_with('-') => bail!("unknown verb option {value:?}"),
                value => self.parse_positional(value)?,
            }
        }
        self.finish()
    }

    fn parse_named(&mut self, raw: &str, iter: &mut std::slice::Iter<'_, String>) -> Result<()> {
        let without_prefix = raw.strip_prefix("--").expect("caller matched double-dash");
        let (name, inline_value) = without_prefix
            .split_once('=')
            .map_or((without_prefix, Option::<&str>::None), |(name, value)| {
                (name, Some(value))
            });
        if is_compatibility_render_flag(raw) {
            bail!(
                "verb '{}' has no argument '{}'; {raw} is a retired compatibility rendering flag. Runtime verbs use `--format=text`, `--format=json`, or `--json`",
                self.entry.name(),
                name,
            );
        }
        let arg = self.arg(raw, name)?;
        let value = match (inline_value, arg.kind()) {
            (Some(value), _) => value.to_string(),
            (None, VerbArgKind::Bool) => "true".to_string(),
            (None, _) => next_verb_arg_value(iter, raw)?.to_string(),
        };
        self.insert_value(arg, &value)
    }

    fn parse_positional(&mut self, value: &str) -> Result<()> {
        let Some(arg) = self
            .entry
            .args()
            .iter()
            .filter(|arg| arg.default().is_none())
            .nth(self.next_positional)
        else {
            bail!(
                "verb '{}' accepts no more positional arguments; expected args: {}",
                self.entry.name(),
                self.expected_args()
            );
        };
        self.next_positional += 1;
        self.insert_value(arg, value)
    }

    fn finish(mut self) -> Result<DynamicVerbInvocation> {
        if self.help {
            return Ok(DynamicVerbInvocation {
                bindings: self.values.into_iter().collect(),
                explain: self.explain,
                rows: self.rows,
                help: true,
            });
        }
        for arg in self.entry.args() {
            if self.values.contains_key(arg.name()) {
                continue;
            }
            if let Some(default) = arg.default() {
                self.insert_value(arg, default)?;
                continue;
            }
            bail!(
                "verb '{}' missing required argument '{}'; expected args: {}",
                self.entry.name(),
                arg.name(),
                self.expected_args()
            );
        }
        Ok(DynamicVerbInvocation {
            bindings: self.values.into_iter().collect(),
            explain: self.explain,
            rows: self.rows,
            help: self.help,
        })
    }

    fn arg(&self, raw: &str, name: &str) -> Result<&'a VerbArg> {
        self.entry
            .args()
            .iter()
            .find(|arg| arg.name() == name)
            .ok_or_else(|| {
                if is_compatibility_filter_flag(raw) {
                    anyhow::anyhow!(
                        "verb '{}' has no argument '{}'; {raw} is a retired compatibility filter, not a runtime verb option. Use a declared verb argument, or express the filter in Datalog with `anneal -e`",
                        self.entry.name(),
                        name,
                    )
                } else if is_compatibility_render_flag(raw) {
                    anyhow::anyhow!(
                        "verb '{}' has no argument '{}'; {raw} is a retired compatibility rendering flag. Runtime verbs use `--format=text`, `--format=json`, or `--json`",
                        self.entry.name(),
                        name,
                    )
                } else {
                    anyhow::anyhow!(
                    "verb '{}' has no argument '{}'; expected args: {}",
                    self.entry.name(),
                    name,
                    self.expected_args()
                    )
                }
            })
    }

    fn insert_value(&mut self, arg: &VerbArg, value: &str) -> Result<()> {
        let literal = arg.parse_literal(value)?;
        if self
            .values
            .insert(arg.name().to_string(), literal)
            .is_some()
        {
            bail!(
                "verb '{}' argument '{}' was provided twice",
                self.entry.name(),
                arg.name()
            );
        }
        Ok(())
    }

    fn expected_args(&self) -> String {
        if self.entry.args().is_empty() {
            "none".to_string()
        } else {
            self.entry
                .args()
                .iter()
                .map(|arg| match arg.default() {
                    Some(default) => format!("{}:{}={default}", arg.name(), arg.kind()),
                    None => format!("{}:{}", arg.name(), arg.kind()),
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

fn render_dynamic_verb_query(query_source: &str, bindings: &[(String, Literal)]) -> String {
    let mut rendered = render_verb_arg_facts(bindings);
    rendered.push_str(query_source);
    rendered
}

fn write_text<W: Write>(mut writer: W, text: &str) -> Result<()> {
    writer.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn write_json_object<W: Write, T: Serialize>(mut writer: W, value: &T) -> Result<()> {
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn run_init(root: &Utf8Path, dry_run: bool, force: bool) -> Result<InitCommandOutput> {
    let mode = anneal_md::InitMode::from_flags(dry_run, force);
    let inner =
        anneal_md::render_or_write_init(root, mode).context("failed to initialize anneal.dl")?;
    Ok(InitCommandOutput { inner })
}

fn write_init_text<W: Write>(mut writer: W, output: &anneal_md::InitOutput) -> Result<()> {
    if output.written {
        writeln!(writer, "Wrote {}", output.path)?;
        if let Some(path) = &output.backup_path {
            writeln!(writer, "Moved existing anneal.toml to {path}")?;
        }
    } else {
        writeln!(writer, "anneal.dl")?;
        writeln!(writer, "dry run — not written")?;
    }
    writeln!(writer)?;
    write_text(writer, &output.body)
}

fn write_status_text<W: Write>(
    mut writer: W,
    rows: &[Row],
    flow_baseline_ready: bool,
) -> Result<()> {
    writeln!(writer, "Status")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        writeln!(
            writer,
            "Note: no corpus facts found; root may be empty or unresolved."
        )?;
        return Ok(());
    }

    let mut metrics = BTreeMap::<(&str, &str), StatusMetric<'_>>::new();
    let mut pipeline = Vec::new();
    for row in rows {
        let metric = StatusMetric::from_row(row)?;
        if metric.category == "pipeline" {
            pipeline.push(metric);
        }
        metrics.insert((metric.category, metric.name), metric);
    }

    let total_handles = metric_count(&metrics, "scale", "handles");
    let file_handles = metric_count(&metrics, "scale", "file_handles");
    let files_with_status = metric_count(&metrics, "scale", "file_handles_with_status");
    let statusless_files = metric_count(&metrics, "scale", "statusless_file_handles");
    let coverage = percentage(files_with_status, file_handles);

    writeln!(
        writer,
        "Scale        {total_handles} handles · {file_handles} files · {coverage}% lifecycle coverage ({statusless_files} statusless files)"
    )?;
    if total_handles == 0 {
        writeln!(
            writer,
            "Note: no corpus facts found; root may be empty or unresolved."
        )?;
    }
    writeln!(
        writer,
        "Coverage     {coverage}% of file handles carry lifecycle status; orientation is graph+recency-led"
    )?;

    if !pipeline.is_empty() {
        pipeline.sort_by(|left, right| left.name.cmp(right.name));
        let parts = pipeline
            .iter()
            .map(|metric| format!("{} {}", metric.name, display_number(metric.count)))
            .collect::<Vec<_>>()
            .join(" · ");
        writeln!(writer, "Pipeline     {parts}")?;
    }

    if flow_baseline_ready {
        writeln!(
            writer,
            "Convergence  broken={}  blocked={}  open={}  advancing={}  holding={}  drifting={}",
            metric_count(&metrics, "convergence", "broken"),
            metric_count(&metrics, "convergence", "blocked"),
            metric_count(&metrics, "convergence", "open"),
            metric_count(&metrics, "convergence", "advancing"),
            metric_count(&metrics, "convergence", "holding"),
            metric_count(&metrics, "convergence", "drifting")
        )?;
    } else {
        writeln!(
            writer,
            "Convergence  broken={}  blocked={}  open={}  advancing=-  holding=-  drifting=-",
            metric_count(&metrics, "convergence", "broken"),
            metric_count(&metrics, "convergence", "blocked"),
            metric_count(&metrics, "convergence", "open")
        )?;
        writeln!(
            writer,
            "Note: flow signals empty until snapshot baseline accumulates."
        )?;
        writeln!(writer, "      Run `anneal status` again to populate.")?;
    }

    writeln!(
        writer,
        "Health       errors={}  blockers={}  spec_code_drift={}",
        metric_count(&metrics, "health", "errors"),
        metric_count(&metrics, "health", "blockers"),
        metric_count(&metrics, "health", "spec_code_drift")
    )?;
    let drift_cold = metric_count(&metrics, "drift", "cold");
    if has_metric_category(&metrics, "drift") {
        if drift_cold > 0 {
            writeln!(
                writer,
                "Code refs    drift evidence not built for {drift_cold} refs; run `anneal check --refresh-drift`"
            )?;
        } else {
            writeln!(
                writer,
                "Code refs    {} intact · {} drifted · {} moved · {} moved? · {} gone · {} unknown · {} dirty",
                metric_count(&metrics, "drift", "intact"),
                metric_count(&metrics, "drift", "drifted"),
                metric_count(&metrics, "drift", "moved"),
                metric_count(&metrics, "drift", "moved_ambiguous"),
                metric_count(&metrics, "drift", "gone"),
                metric_count(&metrics, "drift", "unknown"),
                metric_count(&metrics, "drift", "dirty")
            )?;
        }
    }
    writeln!(writer)?;
    writeln!(writer, "Read first")?;
    writeln!(
        writer,
        "  anneal -e '? recent_frontier(h, rank, recency), *handle{{id: h, file: file}} order by rank asc.' --limit 12"
    )?;
    writeln!(
        writer,
        "  anneal -e '? ranked_anchor(h, rank, score, why), *handle{{id: h, file: file}} order by rank asc.' --limit 12"
    )?;
    writeln!(writer, "Work")?;
    writeln!(
        writer,
        "  anneal -e '? diagnostic{{code: code, severity: severity, subject: h, file: file, line: line}}.' --limit 12"
    )?;
    writeln!(
        writer,
        "  anneal -e '? blocker(h, energy, source), *handle{{id: h, file: file, status: status}}.' --limit 12"
    )?;
    Ok(())
}

fn metric_count(
    metrics: &BTreeMap<(&str, &str), StatusMetric<'_>>,
    category: &str,
    name: &str,
) -> i64 {
    metrics
        .get(&(category, name))
        .and_then(|metric| number_to_i64(metric.count))
        .unwrap_or(0)
}

fn has_metric_category(metrics: &BTreeMap<(&str, &str), StatusMetric<'_>>, category: &str) -> bool {
    metrics
        .keys()
        .any(|(metric_category, _)| *metric_category == category)
}

fn percentage(numerator: i64, denominator: i64) -> i64 {
    if denominator <= 0 {
        0
    } else {
        numerator.saturating_mul(100) / denominator
    }
}

fn number_to_i64(number: &NumberValue) -> Option<i64> {
    match number {
        NumberValue::Int(value) => Some(*value),
        NumberValue::Float(_) => None,
    }
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
        let summary = hit
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
            .map_or(String::new(), |summary| {
                format!(" summary={}", display_string_value(summary))
            });
        let status = hit
            .status
            .as_deref()
            .map_or(String::new(), |status| format!(" status={status}"));
        let age = hit
            .age_days
            .map_or(String::new(), |days| format!(" age_days={days}"));
        let topic = if hit.newer_topic_sibling_count > 0 {
            hit.top_newer_topic_sibling
                .as_deref()
                .map_or_else(String::new, |top| {
                    let handle = datalog_string_literal(&hit.handle);
                    format!(
                        " topic=\"{} unmarked newer topical siblings (top: {}; follow-up: anneal -e '? currency_suspect({}, newer).')\"",
                        hit.newer_topic_sibling_count, top, handle
                    )
                })
        } else {
            String::new()
        };
        writeln!(
            writer,
            "{:>2}. {}  score={:.3}  field={}  reason={} disposition={}{}{}{}{}{}",
            index + 1,
            hit.handle,
            hit.score,
            hit.field,
            hit.reason,
            hit.disposition,
            status,
            age,
            topic,
            span,
            summary
        )?;
    }

    if !output.spans.is_empty() {
        writeln!(writer)?;
        let has_bodies = output.spans.iter().any(|span| span.text.is_some());
        writeln!(writer, "{}", if has_bodies { "Read" } else { "Spans" })?;
        for span in &output.spans {
            writeln!(
                writer,
                "{} span={} lines={}-{} tokens={}",
                span.handle, span.span_id, span.start_line, span.end_line, span.tokens
            )?;
            if let Some(text) = &span.text {
                write_text_block(&mut writer, text, MAX_TEXT_LINES_PER_SPAN)?;
            }
        }
    }

    if !output.neighborhood.is_empty() {
        let mut by_handle: BTreeMap<&str, Vec<&crate::ContextNeighbor>> = BTreeMap::new();
        for neighbor in &output.neighborhood {
            by_handle
                .entry(&neighbor.handle)
                .or_default()
                .push(neighbor);
        }

        writeln!(writer)?;
        writeln!(writer, "Neighborhood")?;
        for (handle, neighbors) in by_handle {
            writeln!(writer, "{handle}:")?;
            let groups = [
                (CONTEXT_NEIGHBOR_GROUP_CURRENT, "current"),
                (CONTEXT_NEIGHBOR_GROUP_IN_FLIGHT, "in-flight"),
                (CONTEXT_NEIGHBOR_GROUP_SUPERSEDED, "superseded"),
                (CONTEXT_NEIGHBOR_GROUP_HIDDEN, "hidden"),
            ];
            for (group, label) in groups {
                let group_neighbors = neighbors
                    .iter()
                    .copied()
                    .filter(|neighbor| neighbor.group == group)
                    .collect::<Vec<_>>();
                if group_neighbors.is_empty() {
                    continue;
                }
                let limit = if group == CONTEXT_NEIGHBOR_GROUP_HIDDEN {
                    1
                } else {
                    MAX_NEIGHBORS_PER_HANDLE
                };
                let omitted = group_neighbors.len().saturating_sub(limit);
                write!(writer, "  {label}: ")?;
                for (index, neighbor) in group_neighbors.iter().take(limit).enumerate() {
                    if index > 0 {
                        write!(writer, ", ")?;
                    }
                    write!(writer, "{}", neighbor.neighbor)?;
                    write!(writer, " disposition={}", neighbor.disposition)?;
                    if let Some(status) = &neighbor.status {
                        write!(writer, " status={status}")?;
                    }
                    if let Some(age_days) = neighbor.age_days {
                        write!(writer, " age_days={age_days}")?;
                    }
                    write!(writer, " degree={}", neighbor.degree)?;
                }
                if omitted == 0 {
                    writeln!(writer)?;
                } else if group == CONTEXT_NEIGHBOR_GROUP_HIDDEN {
                    writeln!(writer, ", ... {omitted} hidden inventory handles")?;
                } else {
                    writeln!(writer, ", ... {omitted} more")?;
                }
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "section")]
enum ContextEvent<'a> {
    #[serde(rename = "goal")]
    Goal { goal: &'a str },
    #[serde(rename = "hit")]
    Hit {
        handle: &'a str,
        span_id: Option<&'a str>,
        score: f64,
        reason: &'a str,
        field: &'a str,
        summary: Option<&'a str>,
        status: Option<&'a str>,
        disposition: &'a str,
        age_days: Option<i64>,
        topic_signal: &'a str,
        newer_topic_sibling_count: i64,
        top_newer_topic_sibling: Option<&'a str>,
    },
    #[serde(rename = "span")]
    Span {
        handle: &'a str,
        span_id: &'a str,
        start_line: i64,
        end_line: i64,
        tokens: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<&'a str>,
    },
    #[serde(rename = "neighbor")]
    Neighbor {
        handle: &'a str,
        neighbor: &'a str,
        status: Option<&'a str>,
        disposition: &'a str,
        age_days: Option<i64>,
        degree: i64,
        group: &'a str,
    },
}

fn write_context_ndjson<W: Write>(writer: W, output: &ContextOutput) -> Result<()> {
    let events = std::iter::once(ContextEvent::Goal {
        goal: output.goal.as_str(),
    })
    .chain(output.hits.iter().map(|hit| ContextEvent::Hit {
        handle: hit.handle.as_str(),
        span_id: hit.span_id.as_deref(),
        score: hit.score,
        reason: hit.reason.as_str(),
        field: hit.field.as_str(),
        summary: hit.summary.as_deref(),
        status: hit.status.as_deref(),
        disposition: hit.disposition.as_str(),
        age_days: hit.age_days,
        topic_signal: hit.topic_signal.as_str(),
        newer_topic_sibling_count: hit.newer_topic_sibling_count,
        top_newer_topic_sibling: hit.top_newer_topic_sibling.as_deref(),
    }))
    .chain(output.spans.iter().map(|span| ContextEvent::Span {
        handle: span.handle.as_str(),
        span_id: span.span_id.as_str(),
        start_line: span.start_line,
        end_line: span.end_line,
        tokens: span.tokens,
        text: span.text.as_deref(),
    }))
    .chain(
        output
            .neighborhood
            .iter()
            .map(|neighbor| ContextEvent::Neighbor {
                handle: neighbor.handle.as_str(),
                neighbor: neighbor.neighbor.as_str(),
                status: neighbor.status.as_deref(),
                disposition: neighbor.disposition.as_str(),
                age_days: neighbor.age_days,
                degree: neighbor.degree,
                group: neighbor.group.as_str(),
            }),
    );
    write_ndjson(writer, events)?;
    Ok(())
}

fn write_rows_text<W: Write>(
    mut writer: W,
    rows: &[Row],
    view: &RowView,
    empty_binding_hint: Option<&str>,
) -> Result<()> {
    if let RowView::Handle {
        handle,
        impact,
        lineage,
    } = view
    {
        return write_handle_text(writer, handle, *impact, *lineage, rows);
    }

    if *view == RowView::Describe {
        return write_describe_text(writer, rows);
    }

    if *view == RowView::Read {
        return write_read_text(writer, rows);
    }

    if let Some(heading) = view.heading(rows.len()) {
        writeln!(writer, "{heading}")?;
    }
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
    if zero_binding_rows(rows)
        && let Some(example) = empty_binding_hint
    {
        writeln!(writer)?;
        writeln!(writer, "{}", empty_binding_hint_text(rows.len(), example))?;
    }
    Ok(())
}

fn write_read_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    const MAX_TEXT_LINES_PER_SPAN: usize = 80;

    writeln!(writer, "Read ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            writeln!(writer)?;
        }

        let span_id = required_string(row, "span_id")?;
        let start_line = required_number(row, "start_line")?;
        let end_line = required_number(row, "end_line")?;
        let tokens = required_number(row, "tokens")?;
        let total_tokens = optional_number(row, "total_tokens")?;
        let text = required_string(row, "text")?;

        writeln!(
            writer,
            "{:>2}. {}  lines={}-{}  tokens={}",
            index + 1,
            span_id,
            display_number(start_line),
            display_number(end_line),
            display_number(tokens)
        )?;

        write_text_block(&mut writer, text, MAX_TEXT_LINES_PER_SPAN)?;
        if let Some(total_tokens) = total_tokens
            && number_gt(total_tokens, tokens)
        {
            writeln!(
                writer,
                "    read: showing first {} tokens of span ({} total); use --budget {} to read the full span",
                display_number(tokens),
                display_number(total_tokens),
                display_number(total_tokens)
            )?;
        }
    }
    Ok(())
}

fn write_text_block<W: Write>(writer: &mut W, text: &str, max_lines: usize) -> Result<()> {
    let mut lines = text.lines().skip_while(|line| line.trim().is_empty());
    for line in lines.by_ref().take(max_lines) {
        writeln!(writer, "  {}", line.trim_end())?;
    }
    if lines.next().is_some() {
        writeln!(writer, "  ...")?;
    }
    Ok(())
}

fn write_describe_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    let mut wrote_any = false;
    let mut doc_rows = Vec::new();
    let mut other_rows = Vec::new();
    for row in rows {
        if let Some(doc) = optional_string(row, "doc")? {
            doc_rows.push(doc.to_string());
        } else {
            other_rows.push(row);
        }
    }

    for doc in doc_rows {
        if wrote_any {
            writeln!(writer)?;
        }
        writeln!(writer, "{doc}")?;
        wrote_any = true;
    }

    for (index, row) in other_rows.iter().enumerate() {
        if wrote_any {
            writeln!(writer)?;
        }
        write!(writer, "{:>2}.", index + 1)?;
        for (field, value) in &row.fields {
            write!(writer, " {field}={}", display_value(value))?;
        }
        writeln!(writer)?;
        wrote_any = true;
    }
    Ok(())
}

fn write_handle_text<W: Write>(
    mut writer: W,
    handle: &str,
    include_impact: bool,
    include_lineage: bool,
    rows: &[Row],
) -> Result<()> {
    let edge_count = rows
        .iter()
        .filter(|row| {
            matches!(
                required_string(row, "relation"),
                Ok("in" | "out" | "code_ref")
            )
        })
        .count();

    writeln!(writer, "Handle {handle} ({edge_count} edges)")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut code_refs = Vec::new();
    let mut direct_impact = Vec::new();
    let mut indirect_impact = Vec::new();
    let mut lineage = Vec::new();
    let mut wrote_self = false;
    for row in rows {
        let relation = required_string(row, "relation")?;
        match relation {
            "self" => {
                wrote_self = true;
                let kind = required_string(row, "kind")?;
                let status = optional_string(row, "status")?.unwrap_or("unknown");
                let file = required_string(row, "file")?;
                let line = required_number(row, "line")?;
                writeln!(
                    writer,
                    "kind={kind}  status={status}  at={file}:{}",
                    display_number(line)
                )?;
                if let Some(summary) = optional_string(row, "summary")?
                    && !summary.trim().is_empty()
                {
                    writeln!(writer, "summary={}", display_string_value(summary))?;
                }
            }
            "in" => incoming.push(row),
            "out" => outgoing.push(row),
            "code_ref" => code_refs.push(row),
            "impact" => {
                let depth = required_number(row, "depth")?;
                if matches!(depth, NumberValue::Int(1)) {
                    direct_impact.push(row);
                } else {
                    indirect_impact.push(row);
                }
            }
            "lineage" => lineage.push(row),
            _ => {}
        }
    }

    if !wrote_self {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
    }
    write_handle_edges(&mut writer, "Outgoing", "->", &outgoing)?;
    write_handle_code_refs(&mut writer, handle, &code_refs)?;
    write_handle_edges(&mut writer, "Incoming", "<-", &incoming)?;
    if include_impact {
        write_handle_impact(&mut writer, &direct_impact, &indirect_impact)?;
    }
    if include_lineage {
        write_handle_lineage(&mut writer, handle, &lineage)?;
    }
    Ok(())
}

fn write_handle_edges<W: Write>(
    writer: &mut W,
    heading: &str,
    arrow: &str,
    rows: &[&Row],
) -> Result<()> {
    const MAX_HANDLE_EDGES_PER_SECTION: usize = 24;

    if rows.is_empty() {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "{heading}")?;
    let mut by_kind = BTreeMap::<&str, Vec<&Row>>::new();
    for row in rows {
        by_kind
            .entry(required_string(row, "kind")?)
            .or_default()
            .push(row);
    }
    for (kind, group) in by_kind {
        writeln!(writer, "{kind} ({})", group.len())?;
        for (index, row) in group.iter().take(MAX_HANDLE_EDGES_PER_SECTION).enumerate() {
            let other = required_string(row, "other")?;
            let file = required_string(row, "file")?;
            let line = required_number(row, "line")?;
            writeln!(
                writer,
                "{:>2}. {arrow} {other}  at={file}:{}",
                index + 1,
                display_number(line)
            )?;
        }
        let omitted = group.len().saturating_sub(MAX_HANDLE_EDGES_PER_SECTION);
        if omitted > 0 {
            writeln!(writer, "    ... {omitted} more")?;
        }
    }
    Ok(())
}

fn write_handle_code_refs<W: Write>(writer: &mut W, handle: &str, rows: &[&Row]) -> Result<()> {
    const MAX_CODE_REFERENCES: usize = 24;

    if rows.is_empty() {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "Code references ({})", rows.len())?;
    for (index, row) in rows.iter().take(MAX_CODE_REFERENCES).enumerate() {
        let target = optional_string(row, "summary")?
            .filter(|summary| !summary.is_empty())
            .unwrap_or(required_string(row, "other")?);
        let annotation = code_ref_annotation(row)?;
        let file = required_string(row, "file")?;
        let line = required_number(row, "line")?;
        writeln!(
            writer,
            "{:>2}. {target}{annotation}  at={file}:{}",
            index + 1,
            display_number(line)
        )?;
    }
    if rows
        .iter()
        .any(|row| matches!(optional_string(row, "disposition"), Ok(None)))
    {
        writeln!(
            writer,
            "    drift evidence not built; run `anneal check --refresh-drift`"
        )?;
    }
    if !rows.is_empty() {
        let handle_literal = datalog_string_literal(handle);
        writeln!(
            writer,
            "    follow-up: anneal -e '? assertion_drift({handle_literal}, target, commits).'"
        )?;
    }
    let omitted = rows.len().saturating_sub(MAX_CODE_REFERENCES);
    if omitted > 0 {
        writeln!(writer, "    ... {omitted} more")?;
    }
    Ok(())
}

fn code_ref_annotation(row: &Row) -> Result<String> {
    let Some(disposition) = optional_string(row, "disposition")? else {
        return Ok(String::new());
    };
    let mut parts = vec![disposition.to_string()];
    if let Some(commits) = optional_string(row, "candidate_count")?
        && disposition == "referent-moved-ambiguous"
    {
        parts.push(format!("{commits} candidates"));
    }
    if let Some(target) = optional_string(row, "moved_to")?
        && disposition == "referent-moved"
    {
        parts.push(format!("moved to {target}"));
    }
    Ok(format!("  [{}]", parts.join(" · ")))
}

fn write_handle_impact<W: Write>(writer: &mut W, direct: &[&Row], indirect: &[&Row]) -> Result<()> {
    writeln!(writer)?;
    writeln!(writer, "Impact (configured reverse traversal)")?;
    write_handle_impact_group(writer, "Direct", direct)?;
    write_handle_impact_group(writer, "Indirect", indirect)?;
    Ok(())
}

fn write_handle_impact_group<W: Write>(writer: &mut W, heading: &str, rows: &[&Row]) -> Result<()> {
    writeln!(writer, "{heading} ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "    (none)")?;
        return Ok(());
    }
    for (index, row) in rows.iter().enumerate() {
        let other = required_string(row, "other")?;
        writeln!(writer, "{:>2}. {other}", index + 1)?;
    }
    Ok(())
}

fn write_handle_lineage<W: Write>(writer: &mut W, handle: &str, rows: &[&Row]) -> Result<()> {
    writeln!(writer)?;
    writeln!(writer, "Lineage (file supersession)")?;
    if rows.is_empty() {
        writeln!(
            writer,
            "    (none; no file-handle Supersedes lineage found)"
        )?;
        return Ok(());
    }

    let normalized_root = rows
        .first()
        .map(|row| required_string(row, "normalized_root"))
        .transpose()?
        .unwrap_or(handle);
    if normalized_root != handle {
        writeln!(writer, "normalized_root={normalized_root}")?;
    }

    let root = rows
        .iter()
        .find(|row| required_string(row, "role").is_ok_and(|role| role == "root"));
    if let Some(root) = root {
        writeln!(writer, "root: {}", lineage_row_summary(root)?)?;
    }

    let mut heads = lineage_rows_with_bool(rows, "head", true)?;
    let mut successors = lineage_rows_with_role(rows, "successor")?;
    let mut predecessors = lineage_rows_with_role(rows, "predecessor")?;
    sort_lineage_rows(&mut heads, false);
    sort_lineage_rows(&mut successors, false);
    sort_lineage_rows(&mut predecessors, true);

    write_handle_lineage_group(writer, "Current head(s)", &heads)?;
    write_handle_lineage_group(writer, "Newer", &successors)?;
    write_handle_lineage_group(writer, "Older", &predecessors)?;
    Ok(())
}

fn lineage_rows_with_bool<'a>(rows: &[&'a Row], field: &str, value: bool) -> Result<Vec<&'a Row>> {
    rows.iter()
        .copied()
        .filter(|row| required_bool(row, field).is_ok_and(|actual| actual == value))
        .map(Ok)
        .collect()
}

fn lineage_rows_with_role<'a>(rows: &[&'a Row], role: &str) -> Result<Vec<&'a Row>> {
    rows.iter()
        .copied()
        .filter(|row| required_string(row, "role").is_ok_and(|actual| actual == role))
        .map(Ok)
        .collect()
}

fn sort_lineage_rows(rows: &mut [&Row], reverse_depth: bool) {
    rows.sort_by(|left, right| {
        let left_depth = lineage_row_depth(left);
        let right_depth = lineage_row_depth(right);
        let depth_order = if reverse_depth {
            right_depth.cmp(&left_depth)
        } else {
            left_depth.cmp(&right_depth)
        };
        depth_order.then_with(|| lineage_row_handle(left).cmp(lineage_row_handle(right)))
    });
}

fn lineage_row_depth(row: &Row) -> i64 {
    required_number(row, "depth")
        .ok()
        .and_then(number_to_i64)
        .unwrap_or(i64::MAX)
}

fn lineage_row_handle(row: &Row) -> &str {
    required_string(row, "other").unwrap_or("")
}

fn write_handle_lineage_group<W: Write>(
    writer: &mut W,
    heading: &str,
    rows: &[&Row],
) -> Result<()> {
    writeln!(writer, "{heading} ({})", rows.len())?;
    if rows.is_empty() {
        writeln!(writer, "    (none)")?;
        return Ok(());
    }
    for (index, row) in rows.iter().enumerate() {
        writeln!(writer, "{:>2}. {}", index + 1, lineage_row_summary(row)?)?;
    }
    Ok(())
}

fn lineage_row_summary(row: &Row) -> Result<String> {
    let handle = required_string(row, "other")?;
    let disposition = required_string(row, "disposition")?;
    let status = optional_string(row, "status")?.unwrap_or("unknown");
    let depth = required_number(row, "depth")?;
    let file = required_string(row, "file")?;
    Ok(format!(
        "{handle}  disposition={disposition}  status={status}  depth={}  read=`anneal read {file}`",
        display_number(depth),
    ))
}

#[derive(Clone, Copy)]
struct StatusMetric<'a> {
    category: &'a str,
    name: &'a str,
    count: &'a NumberValue,
}

impl<'a> StatusMetric<'a> {
    fn from_row(row: &'a Row) -> Result<Self> {
        Ok(Self {
            category: required_string(row, "category")?,
            name: required_string(row, "name")?,
            count: required_number(row, "count")?,
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

fn optional_string<'a>(row: &'a Row, field: &str) -> Result<Option<&'a str>> {
    match row.fields.get(field) {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("row field {field:?} must be a string"),
    }
}

fn required_number<'a>(row: &'a Row, field: &str) -> Result<&'a NumberValue> {
    match row.fields.get(field) {
        Some(Value::Number(value)) => Ok(value),
        Some(_) => bail!("status row field {field:?} must be a number"),
        None => bail!("status row missing field {field:?}"),
    }
}

fn optional_number<'a>(row: &'a Row, field: &str) -> Result<Option<&'a NumberValue>> {
    match row.fields.get(field) {
        Some(Value::Number(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("row field {field:?} must be a number"),
    }
}

fn required_bool(row: &Row, field: &str) -> Result<bool> {
    match row.fields.get(field) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("row field {field:?} must be a bool"),
        None => bail!("row missing field {field:?}"),
    }
}

fn number_gt(left: &NumberValue, right: &NumberValue) -> bool {
    match (left, right) {
        (NumberValue::Int(left), NumberValue::Int(right)) => left > right,
        (NumberValue::Float(left), NumberValue::Float(right)) => left > right,
        (NumberValue::Int(left), NumberValue::Float(right)) => left
            .to_string()
            .parse::<f64>()
            .is_ok_and(|left| left > *right),
        (NumberValue::Float(left), NumberValue::Int(right)) => right
            .to_string()
            .parse::<f64>()
            .map_or(true, |right| *left > right),
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
    let mut read_spans = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--budget" => budget = parse_i64(next_value(&mut iter, "--budget")?, "--budget")?,
            "--hits" => hits = parse_usize(next_value(&mut iter, arg)?, arg)?,
            "--depth" | "--neighborhood-depth" => {
                depth = parse_i64(next_value(&mut iter, arg)?, arg)?;
            }
            "--include-low-confidence" => include_low_confidence = true,
            "--read-spans" => read_spans = true,
            "--limit" => {
                bail!(
                    "context uses --hits for search winners; use `anneal context <GOAL> --hits N`"
                )
            }
            value if value.starts_with("--budget=") => {
                budget = parse_i64(value_after_equals(value), "--budget")?;
            }
            value if value.starts_with("--hits=") => {
                hits = parse_usize(value_after_equals(value), "--hits")?;
            }
            value if value.starts_with("--limit=") => {
                bail!(
                    "context uses --hits for search winners; use `anneal context <GOAL> --hits N`"
                )
            }
            value if value.starts_with("--depth=") => {
                depth = parse_i64(value_after_equals(value), "--depth")?;
            }
            value if value.starts_with("--neighborhood-depth=") => {
                depth = parse_i64(value_after_equals(value), "--neighborhood-depth")?;
            }
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("context", value)?;
                bail!("unknown context option {value:?}");
            }
            value => assign_once(&mut goal, value, "context accepts one goal")?,
        }
    }
    Ok(RuntimeCommand::Context {
        goal: goal.context("context requires a goal")?,
        budget,
        hits,
        depth,
        include_low_confidence,
        read_spans,
    })
}

fn parse_init(args: &[String]) -> Result<RuntimeCommand> {
    let mut dry_run = false;
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "-h" | "--help" => {
                return Ok(RuntimeCommand::Help {
                    topic: HelpTopic::Init,
                });
            }
            other if other.starts_with('-') => bail!("unknown init option {other:?}"),
            other => bail!("init does not accept positional argument {other:?}"),
        }
    }
    Ok(RuntimeCommand::Init { dry_run, force })
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
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("search", value)?;
                bail!("unknown search option {value:?}");
            }
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
    let mut span_id = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--budget" => budget = parse_i64(next_value(&mut iter, "--budget")?, "--budget")?,
            value if value.starts_with("--budget=") => {
                budget = parse_i64(value_after_equals(value), "--budget")?;
            }
            "--span-id" => {
                let value = next_value(&mut iter, "--span-id")?;
                ensure!(!value.trim().is_empty(), "--span-id must not be empty");
                span_id = Some(value.to_string());
            }
            value if value.starts_with("--span-id=") => {
                let value = value_after_equals(value);
                ensure!(!value.trim().is_empty(), "--span-id must not be empty");
                span_id = Some(value.to_string());
            }
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("read", value)?;
                bail!("unknown read option {value:?}");
            }
            value => assign_once(&mut handle, value, "read accepts one handle")?,
        }
    }
    Ok(RuntimeCommand::Read {
        handle: handle.context("read requires a handle")?,
        budget,
        span_id,
    })
}

fn parse_handle(args: &[String]) -> Result<RuntimeCommand> {
    let mut handle = None;
    let mut impact = false;
    let mut lineage = false;
    for arg in args {
        match arg.as_str() {
            "--impact" => impact = true,
            "--lineage" => lineage = true,
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("handle", value)?;
                bail!("unknown handle option {value:?}");
            }
            value => assign_once(&mut handle, value, "handle accepts one handle")?,
        }
    }
    Ok(RuntimeCommand::Handle {
        handle: handle.context("handle requires a handle")?,
        impact,
        lineage,
    })
}

fn parse_check(args: &[String]) -> Result<RuntimeCommand> {
    let mut refresh_drift = false;
    for arg in args {
        match arg.as_str() {
            "--refresh-drift" => refresh_drift = true,
            flag if flag.starts_with('-') => {
                reject_runtime_compatibility_flag("check", flag)?;
                bail!("unknown check option {flag:?}");
            }
            _ => bail!(
                "check is a hidden CI gate for error-severity diagnostics and accepts no filters; use `anneal -e '? diagnostic{{...}}.'` for filtered checks"
            ),
        }
    }
    Ok(RuntimeCommand::Check { refresh_drift })
}

fn parse_eval(args: &[String]) -> Result<RuntimeCommand> {
    let mut query = None;
    let mut explain = ExplainOptions::disabled();
    let mut limit = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" => {
                limit = Some(parse_usize(next_value(&mut iter, "--limit")?, "--limit")?);
            }
            value if value.starts_with("--limit=") => {
                limit = Some(parse_usize(value_after_equals(value), "--limit")?);
            }
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
            "-" => assign_once(&mut query, "-", "eval accepts one query string")?,
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("eval", value)?;
                bail!("unknown eval option {value:?}");
            }
            value => assign_once(&mut query, value, "eval accepts one query string")?,
        }
    }
    Ok(RuntimeCommand::Eval {
        query: query.context("eval requires a query")?,
        explain,
        limit,
    })
}

fn parse_dynamic_verb(name: &str, args: &[String]) -> RuntimeCommand {
    RuntimeCommand::Verb {
        name: name.to_string(),
        args: args.to_vec(),
    }
}

fn standard_verb_name_for_explain(command: &str) -> Option<&'static str> {
    Some(match command {
        "status" => "status",
        "context" => "context",
        "search" => "search",
        "read" => "read",
        "handle" | "H" => "handle",
        "describe" => "describe",
        "schema" => "schema",
        _ => return None,
    })
}

fn defaulted_dynamic_args_for_explain(name: &str, raw_args: &[String]) -> Vec<String> {
    if name == "describe" && raw_args.iter().all(|arg| arg.starts_with('-')) {
        let mut args = vec!["runtime".to_string()];
        args.extend_from_slice(raw_args);
        args
    } else {
        raw_args.to_vec()
    }
}

fn is_explain_option(value: &str) -> bool {
    matches!(
        value,
        "--explain" | "--explain-all" | "--explain-depth" | "--explain-first"
    ) || value.starts_with("--explain-depth=")
        || value.starts_with("--explain-first=")
}

fn reject_runtime_compatibility_flag(command: &str, flag: &str) -> Result<()> {
    if is_compatibility_filter_flag(flag) {
        bail!(
            "{command} does not accept retired compatibility filter {flag}; express the filter in Datalog with `anneal -e`"
        );
    }
    if is_compatibility_render_flag(flag) {
        bail!(
            "{command} does not accept retired compatibility rendering flag {flag}; use `--format=text`, `--format=json`, or `--json`"
        );
    }
    Ok(())
}

fn ensure_no_args(args: &[String], command: &str) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else if let Some(flag) = args.first().filter(|arg| is_compatibility_filter_flag(arg)) {
        bail!(
            "{command} does not accept retired compatibility filter {flag}; express the filter in Datalog with `anneal -e`"
        )
    } else if let Some(flag) = args.first().filter(|arg| is_compatibility_render_flag(arg)) {
        bail!(
            "{command} does not accept retired compatibility rendering flag {flag}; use `--format=text`, `--format=json`, or `--json`"
        )
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

fn next_verb_arg_value<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a str> {
    let value = next_value(iter, flag)?;
    ensure!(
        !value.starts_with("--"),
        "{flag} requires a value; got option {value:?}"
    );
    Ok(value)
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

fn is_routing_only_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--json" | "--pretty" | "--plain" | "--minimal" | "--no-color" | "--recent"
    )
}

fn is_compatibility_filter_flag(arg: &str) -> bool {
    matches!(arg, "--active-only" | "--area" | "--recent" | "--since")
        || arg.starts_with("--area=")
        || arg.starts_with("--since=")
}

fn is_compatibility_render_flag(arg: &str) -> bool {
    matches!(arg, "--pretty" | "--plain" | "--minimal" | "--no-color")
        || arg.starts_with("--pretty=")
        || arg.starts_with("--plain=")
        || arg.starts_with("--minimal=")
        || arg.starts_with("--no-color=")
}

fn default_root() -> Result<InferredCorpusRoot> {
    let cwd = current_dir_utf8()?;
    Ok(infer_corpus_root(&cwd))
}

fn absolute_root(root: &Utf8Path) -> Result<Utf8PathBuf> {
    if root.is_absolute() {
        return Ok(root.to_path_buf());
    }
    Ok(current_dir_utf8()?.join(root))
}

fn absolute_inferred_root(root: &InferredCorpusRoot) -> Result<InferredCorpusRoot> {
    Ok(match root {
        InferredCorpusRoot::Marked(root) => InferredCorpusRoot::Marked(absolute_root(root)?),
        InferredCorpusRoot::Unmarked(root) => InferredCorpusRoot::Unmarked(absolute_root(root)?),
    })
}

fn current_dir_utf8() -> Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    Utf8PathBuf::from_path_buf(cwd).map_err(|path| {
        anyhow!(
            "current directory path is not valid UTF-8: {}",
            path.display()
        )
    })
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
    let external_class = CodeTargetMeta::EXTERNAL_CLASS;
    let class_code = CodeTargetMeta::CLASS_CODE;
    let target_path = CodeTargetMeta::TARGET_PATH;
    let referent_disposition = CodeTargetMeta::REFERENT_DISPOSITION;
    let move_candidate_count = CodeTargetMeta::REFERENT_MOVE_CANDIDATE_COUNT;
    let moved_to = CodeTargetMeta::REFERENT_MOVED_TO;
    format!(
        r#"
handle_focus({handle}).

handle_row({handle}, "self", {handle}, kind, status, file, line, summary, null, null, null) :=
  *handle{{id: {handle}, kind: kind, status: status, file: file, line: line, summary: summary}}.

handle_row({handle}, "out", other, kind, null, file, line, "", null, null, null) :=
  *edge{{from: {handle}, to: other, kind: kind, file: file, line: line}},
  not code_reference(other).

handle_row({handle}, "code_ref", other, "Cites", null, file, line, target_path, disposition, candidate_count, moved_to) :=
  *edge{{from: {handle}, to: other, kind: "Cites", file: file, line: line}},
  *meta{{handle: other, key: "{external_class}", value: "{class_code}"}},
  *meta{{handle: other, key: "{target_path}", value: target_path}},
  code_ref_disposition(other, disposition),
  code_ref_candidate_count(other, candidate_count),
  code_ref_moved_to(other, moved_to).

handle_row({handle}, "in", other, kind, null, file, line, "", null, null, null) :=
  *edge{{to: {handle}, from: other, kind: kind, file: file, line: line}}.

code_reference(h) :=
  *meta{{handle: h, key: "{external_class}", value: "{class_code}"}}.

code_ref_disposition(h, disposition) :=
  *meta{{handle: h, key: "{referent_disposition}", value: disposition}}.

code_ref_disposition(h, null) :=
  code_reference(h),
  not code_ref_disposition_present(h).

code_ref_disposition_present(h) :=
  *meta{{handle: h, key: "{referent_disposition}", value: disposition}}.

code_ref_candidate_count(h, count) :=
  *meta{{handle: h, key: "{move_candidate_count}", value: count}}.

code_ref_candidate_count(h, null) :=
  code_reference(h),
  not code_ref_candidate_count_present(h).

code_ref_candidate_count_present(h) :=
  *meta{{handle: h, key: "{move_candidate_count}", value: count}}.

code_ref_moved_to(h, target) :=
  *meta{{handle: h, key: "{moved_to}", value: target}}.

code_ref_moved_to(h, null) :=
  code_reference(h),
  not code_ref_moved_to_present(h).

code_ref_moved_to_present(h) :=
  *meta{{handle: h, key: "{moved_to}", value: target}}.

? handle_row(h, relation, other, kind, status, file, line, summary, disposition, candidate_count, moved_to).
"#
    )
}

fn looks_like_retired_section_handle(handle: &str) -> bool {
    handle.contains('#') && !handle.starts_with("http://") && !handle.starts_with("https://")
}

fn retired_section_handle_message(handle: &str) -> String {
    let file = handle.split_once('#').map_or(handle, |(file, _)| file);
    let file_literal = datalog_string_literal(file);
    format!(
        "section handles were retired in v0.14; use `anneal -e '? *span{{handle: {file_literal}, id: span_id, summary: heading}}.'` to find heading spans"
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
    use anneal_core::{
        FactBatch, FactBatchMode, FactIdentity, HandleFact, MetaFact, NativeId, OriginUri,
        Revision, SourceName,
    };
    use std::fs;
    use std::num::NonZeroUsize;
    use tempfile::tempdir;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn git(root: &camino::Utf8Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root.as_std_path())
            .args(args)
            .status()
            .unwrap_or_else(|err| panic!("git {args:?} failed to run: {err}"));
        assert!(status.success(), "git {args:?} failed: {status}");
    }

    fn test_identity(native_id: &str) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            NativeId::from(native_id),
            OriginUri::from(format!("file:///{native_id}")),
            Revision::from("test-revision"),
            Generation::new(1),
        )
    }

    fn test_handle(id: &str, kind: &str, status: Option<&str>, file: &str) -> HandleFact {
        HandleFact {
            identity: test_identity(id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(str::to_string),
            namespace: String::new(),
            file: file.to_string(),
            line: 1,
            date: None,
            area: String::new(),
            summary: String::new(),
        }
    }

    fn test_edge(from: &str, to: &str, kind: &str) -> EdgeFact {
        EdgeFact {
            identity: test_identity(from),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: from.to_string(),
            line: 1,
            assertion_date: None,
            assertion_revision: None,
        }
    }

    fn test_meta(handle: &str, key: &str, value: &str) -> MetaFact {
        MetaFact {
            identity: test_identity(handle),
            handle: handle.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn lineage_store() -> FactStore {
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        batch.handles.extend([
            test_handle(
                "implementation/2026-05-30-unified.md",
                "file",
                Some("superseded"),
                "implementation/2026-05-30-unified.md",
            ),
            test_handle(
                "compiler/2026-03-30-cell-graph.md",
                "file",
                Some("superseded"),
                "compiler/2026-03-30-cell-graph.md",
            ),
            test_handle(
                "implementation/2026-05-31-program-space.md",
                "file",
                Some("active"),
                "implementation/2026-05-31-program-space.md",
            ),
            test_handle(
                "formal-model/history/sample-formal-model-v14.md",
                "file",
                Some("superseded"),
                "formal-model/history/sample-formal-model-v14.md",
            ),
            test_handle(
                "formal-model/sample-formal-model-v17.md",
                "file",
                Some("authoritative"),
                "formal-model/sample-formal-model-v17.md",
            ),
            test_handle("sample-formal-model-v14", "version", None, ""),
            test_handle("sample-formal-model-v17", "version", None, ""),
            test_handle("raw-v14", "version", None, ""),
            test_handle("raw-v17", "version", None, ""),
        ]);
        batch.edges.extend([
            test_edge(
                "implementation/2026-05-30-unified.md",
                "implementation/2026-05-31-program-space.md",
                SUPERSEDES_EDGE_KIND,
            ),
            test_edge(
                "compiler/2026-03-30-cell-graph.md",
                "implementation/2026-05-31-program-space.md",
                SUPERSEDES_EDGE_KIND,
            ),
            test_edge(
                "formal-model/history/sample-formal-model-v14.md",
                "formal-model/sample-formal-model-v17.md",
                SUPERSEDES_EDGE_KIND,
            ),
            test_edge(
                "sample-formal-model-v17",
                "sample-formal-model-v14",
                SUPERSEDES_EDGE_KIND,
            ),
            test_edge("raw-v17", "raw-v14", SUPERSEDES_EDGE_KIND),
        ]);
        batch.meta.push(test_meta(
            "sample-formal-model-v14",
            RESOLVED_FILE_META_KEY,
            "formal-model/history/sample-formal-model-v14.md",
        ));
        let mut store = FactStore::default();
        store.merge(batch).expect("merge lineage batch");
        store
    }

    #[test]
    fn routes_only_runtime_commands() {
        assert!(should_handle_args(&os(&["anneal"])));
        assert!(should_handle_args(&os(&["anneal", "--help"])));
        assert!(should_handle_args(&os(&["anneal", "-h"])));
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
        assert!(should_handle_args(&os(&["anneal", "help", "agent"])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "--root",
            ".design",
            "release-blockers"
        ])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "help",
            "release-blockers"
        ])));
        assert!(should_handle_args(&os(&["anneal", "anneal"])));
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
        assert!(should_handle_args(&os(&["anneal", "areas"])));
        assert!(should_handle_args(&os(&["anneal", "help", "areas"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--area", "compiler", "status"
        ])));
        assert!(should_handle_args(&os(&["anneal", "--pretty", "status"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--root", ".design", "health"
        ])));
        for retired in [
            "work",
            "blocked",
            "diagnostics",
            "broken",
            "areas",
            "trend",
            "sources",
            "impact",
            "find",
            "get",
            "map",
            "health",
            "diff",
            "obligations",
            "garden",
            "orient",
            "query",
            "explain",
        ] {
            assert!(
                should_handle_args(&os(&["anneal", retired])),
                "retired command {retired:?} should route to runtime recovery"
            );
            assert!(
                should_handle_args(&os(&["anneal", "help", retired])),
                "retired help topic {retired:?} should route to runtime recovery"
            );
        }
        assert!(should_handle_args(&os(&["anneal", "check"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--area", "compiler", "check"
        ])));
        assert!(should_handle_args(&os(&["anneal", "init"])));
        assert!(should_handle_args(&os(&["anneal", "prime"])));
        assert!(should_handle_args(&os(&["anneal", "help", "check"])));
        assert!(should_handle_args(&os(&["anneal", "--version"])));
        assert!(should_handle_args(&os(&["anneal", "--help"])));
        assert!(should_handle_args(&os(&["anneal", "check", "--json"])));
        assert!(!should_handle_args(&os(&["anneal", "--mcp"])));
    }

    #[test]
    fn parses_version_without_loading_corpus() {
        let parsed = Invocation::parse(os(&["anneal", "--version"])).expect("parse version");
        assert_eq!(parsed.command, RuntimeCommand::Version);

        let parsed = Invocation::parse(os(&["anneal", "version"])).expect("parse version command");
        assert_eq!(parsed.command, RuntimeCommand::Version);

        let err = Invocation::parse(os(&["anneal", "--version", "status"]))
            .expect_err("version accepts no args");
        assert!(err.to_string().contains("accepts no arguments"), "{err}");
    }

    #[test]
    fn runtime_rejects_compatibility_dialect_flags() {
        let err = Invocation::parse(os(&["anneal", "--area=compiler", "status"]))
            .expect_err("runtime should reject compatibility filters");
        assert!(err.to_string().contains("compatibility filter"), "{err}");

        let err = Invocation::parse(os(&["anneal", "--pretty", "status"]))
            .expect_err("runtime should reject compatibility render flags");
        assert!(
            err.to_string().contains("compatibility rendering flag"),
            "{err}"
        );

        let err = Invocation::parse(os(&["anneal", "status", "--area=compiler"]))
            .expect_err("standard runtime verbs should reject compatibility filters");
        assert!(
            err.to_string()
                .contains("does not accept retired compatibility filter"),
            "{err}"
        );

        let parsed = Invocation::parse(os(&["anneal", "release-blockers", "--area", "compiler"]))
            .expect("dynamic verbs may declare their own area argument");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Verb {
                name: "release-blockers".to_string(),
                args: vec!["--area".to_string(), "compiler".to_string()],
            }
        );
    }

    #[test]
    fn marked_root_is_reported_for_json_or_empty_outputs() {
        let root = Utf8PathBuf::from("/tmp/corpus/.design");

        assert_eq!(
            RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
                .diagnostic(OutputMode::Json, true),
            Some("resolved root: /tmp/corpus/.design".to_string())
        );
        assert_eq!(
            RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
                .diagnostic(OutputMode::Human, false),
            Some("resolved root: /tmp/corpus/.design".to_string())
        );
        assert_eq!(
            RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
                .diagnostic(OutputMode::Human, true),
            None
        );
        assert_eq!(
            RootSelection::Explicit(root).diagnostic(OutputMode::Json, true),
            None
        );
    }

    #[test]
    fn unmarked_root_is_rejected_before_runtime_output() {
        let root = Utf8PathBuf::from("/tmp/stray");
        let selection = RootSelection::Inferred(InferredCorpusRoot::Unmarked(root.clone()));

        assert_eq!(
            selection.implicit_unmarked_root(),
            Some(Utf8Path::new("/tmp/stray"))
        );
        assert_eq!(selection.diagnostic(OutputMode::Human, true), None);

        let explicit = RootSelection::Explicit(root);
        assert_eq!(explicit.implicit_unmarked_root(), None);
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
            "--read-spans",
        ]))
        .expect("parse");
        assert_eq!(
            parsed.root,
            RootSelection::Explicit(Utf8PathBuf::from(".design"))
        );
        assert_eq!(
            parsed.command,
            RuntimeCommand::Context {
                goal: "v17 audit".to_string(),
                budget: 1200,
                hits: 2,
                depth: 3,
                include_low_confidence: false,
                read_spans: true,
            }
        );
    }

    #[test]
    fn parses_read_span_id_option() {
        let parsed = Invocation::parse(os(&[
            "anneal",
            "read",
            "docs/a.md",
            "--budget=1200",
            "--span-id",
            "docs/a.md#h/target",
        ]))
        .expect("parse read");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Read {
                handle: "docs/a.md".to_string(),
                budget: 1200,
                span_id: Some("docs/a.md#h/target".to_string()),
            }
        );
    }

    #[test]
    fn rejects_empty_read_span_id() {
        let err = Invocation::parse(os(&["anneal", "read", "docs/a.md", "--span-id="]))
            .expect_err("empty span id should fail");

        assert!(err.to_string().contains("--span-id must not be empty"));
    }

    #[test]
    fn parses_check_gate_alias() {
        let parsed = Invocation::parse(os(&["anneal", "check"])).expect("parse check");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Check {
                refresh_drift: false
            }
        );

        let parsed = Invocation::parse(os(&["anneal", "check", "--json"])).expect("parse check");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Check {
                refresh_drift: false
            }
        );
        assert_eq!(parsed.output, OutputPreference::Json);

        let parsed =
            Invocation::parse(os(&["anneal", "check", "--refresh-drift"])).expect("parse check");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Check {
                refresh_drift: true
            }
        );

        let err = Invocation::parse(os(&["anneal", "check", "--active-only"]))
            .expect_err("check no longer accepts compatibility filters");
        assert!(
            err.to_string().contains("retired compatibility filter"),
            "{err}"
        );

        let err = Invocation::parse(os(&["anneal", "diagnostics", "--gate"]))
            .expect_err("diagnostics is retired");
        assert!(
            err.to_string().contains("diagnostics has been retired"),
            "{err}"
        );
    }

    #[test]
    fn rejects_context_limit_alias() {
        let err = Invocation::parse(os(&["anneal", "context", "v17 audit", "--limit=4"]))
            .expect_err("context has hits, not a generic limit");
        assert!(err.to_string().contains("context uses --hits"), "{err}");
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
        let RuntimeCommand::Eval {
            query,
            explain,
            limit,
        } = parsed.command
        else {
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
        assert_eq!(limit, None);
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
        let RuntimeCommand::Eval { query, explain, .. } = parsed.command else {
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
        let RuntimeCommand::Eval { query, explain, .. } = parsed.command else {
            panic!("expected eval command");
        };
        assert_eq!(query, "? blocked(h).");
        assert!(explain.is_enabled());
        assert_eq!(explain.row_limit(), ExplainRowLimit::All);
    }

    #[test]
    fn parses_runtime_subcommand_help_without_loading_corpus() {
        for (command, topic, expected_output) in [
            ("agent", HelpTopic::Agent, "# Anneal"),
            ("context", HelpTopic::Context, "Output: human summary"),
            ("search", HelpTopic::Search, "Output: readable rows"),
            ("read", HelpTopic::Read, "Output: readable rows"),
            (
                "check",
                HelpTopic::Check,
                "Hidden CI gate for error-severity diagnostics",
            ),
        ] {
            let parsed = Invocation::parse(os(&["anneal", "--root=.design", command, "--help"]))
                .expect("parse command help");

            assert_eq!(parsed.command, RuntimeCommand::Help { topic });
            assert!(topic.render().contains(expected_output));
            if !matches!(topic, HelpTopic::Agent) {
                assert!(topic.render().contains("Usage: anneal"));
            }
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
            HelpTopic::Status.render().contains("arrival command")
                && !HelpTopic::Status.render().contains("0.10 and earlier"),
            "status help should teach the current arrival surface"
        );
    }

    #[test]
    fn parses_top_level_help_without_loading_corpus() {
        for help_flag in ["--help", "-h"] {
            let parsed =
                Invocation::parse(os(&["anneal", "--root=.design", help_flag])).expect("parse");

            assert_eq!(
                parsed.command,
                RuntimeCommand::Help {
                    topic: HelpTopic::Top
                }
            );
        }

        let parsed = Invocation::parse(os(&["anneal", "help"])).expect("parse help");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Help {
                topic: HelpTopic::Top
            }
        );

        let rendered = HelpTopic::Top.render();
        assert!(rendered.contains("Usage: anneal [OPTIONS] [COMMAND]"));
        assert!(rendered.contains("anneal describe runtime"));
        assert!(rendered.contains("anneal describe <axis>"));
        assert!(rendered.contains("anneal help agent"));
        assert!(rendered.contains("anneal help eval"));
    }

    #[test]
    fn help_agent_renders_shipped_skill_briefing() {
        let rendered = HelpTopic::Agent.render();

        assert_eq!(rendered, skill_briefing_body(SKILL_MARKDOWN));
        assert!(rendered.contains("# Anneal"));
        assert!(rendered.contains("## First Moves"));
        assert!(rendered.contains("## Agent Rules"));
        assert!(!rendered.starts_with("---"));
    }

    #[test]
    fn unknown_help_topic_points_to_agent_briefing() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        let err = run_args(vec![
            OsString::from("anneal"),
            OsString::from("--root"),
            OsString::from(root.as_str()),
            OsString::from("help"),
            OsString::from("banana"),
        ])
        .expect_err("unknown help topic should error");

        assert!(
            err.to_string().contains("anneal help agent"),
            "unexpected error: {err:#}"
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
        let rendered = HelpTopic::Eval.render();
        assert!(rendered.contains("--explain-depth"));
        assert!(rendered.contains("--explain-first"));
        assert!(rendered.contains("--explain-all"));
        assert!(rendered.contains("Discover before guessing"));
        assert!(rendered.contains("source_of"));
        assert!(rendered.contains("anneal -e - < query.dl"));
        assert!(rendered.contains("at(\"snapshot:last\")"));
        assert!(rendered.contains("at(\"HEAD~5\") remain pending"));
        assert!(rendered.contains("Migration recipes"));
        assert!(rendered.contains("severity: \"error\""));
        assert!(rendered.contains("undischarged(h), obligation(h)"));
        assert!(!rendered.contains('\t'));
    }

    #[test]
    fn parses_eval_stdin_marker() {
        let parsed = Invocation::parse(os(&["anneal", "-e", "-"])).expect("parse stdin eval");

        let RuntimeCommand::Eval {
            query,
            explain,
            limit,
        } = parsed.command
        else {
            panic!("expected eval command");
        };
        assert_eq!(query, "-");
        assert!(!explain.is_enabled());
        assert_eq!(limit, None);
    }

    #[test]
    fn parses_eval_limit() {
        let parsed = Invocation::parse(os(&["anneal", "-e", "? *handle{id: h}.", "--limit=7"]))
            .expect("parse eval limit");

        let RuntimeCommand::Eval { limit, .. } = parsed.command else {
            panic!("expected eval command");
        };
        assert_eq!(limit, Some(7));
    }

    #[test]
    fn parses_dynamic_verb_projection_options() {
        let parsed = Invocation::parse(os(&[
            "anneal",
            "release-blockers",
            "--rows=5",
            "--explain-first=2",
        ]))
        .expect("parse dynamic verb");

        let RuntimeCommand::Verb { name, args } = parsed.command else {
            panic!("expected dynamic verb command");
        };
        assert_eq!(name, "release-blockers");
        assert_eq!(args, ["--rows=5", "--explain-first=2"]);

        let parsed =
            Invocation::parse(os(&["anneal", "release-blockers", "--help"])).expect("parse help");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Verb {
                name: "release-blockers".to_string(),
                args: vec!["--help".to_string()],
            }
        );
    }

    #[test]
    fn standard_verb_explain_routes_through_dynamic_projection() {
        let parsed = Invocation::parse(os(&["anneal", "handle", "OQ-1", "--explain"]))
            .expect("parse standard explain");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Verb {
                name: "handle".to_string(),
                args: vec!["OQ-1".to_string(), "--explain".to_string()],
            }
        );
    }

    #[test]
    fn dynamic_verb_preserves_positional_arguments_for_registry_parse() {
        let parsed = Invocation::parse(os(&["anneal", "release-blockers", "v0.11"]))
            .expect("parse dynamic verb args");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Verb {
                name: "release-blockers".to_string(),
                args: vec!["v0.11".to_string()],
            }
        );
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
            Invocation::parse(os(&["anneal", "--format=text", "status"])).expect("parse status");

        assert_eq!(parsed.command, RuntimeCommand::Status);
        assert_eq!(parsed.output, OutputPreference::Human);

        let parsed =
            Invocation::parse(os(&["anneal", "schema", "--format", "json"])).expect("parse schema");

        assert_eq!(parsed.command, RuntimeCommand::Schema);
        assert_eq!(parsed.output, OutputPreference::Json);
    }

    #[test]
    fn retired_teaching_commands_point_to_describe_and_eval() {
        for (command, expected) in [
            ("cookbook", "folded into `anneal describe NAME`"),
            ("vocab", "folded into Code Mode queries"),
            ("verbs", "folded into introspection"),
            ("examples", "folded into `anneal describe NAME`"),
            ("save", "edit anneal.dl directly"),
            ("impact", "handle <HANDLE> --impact"),
            ("find", "h contains \"TEXT\""),
            ("get", "anneal handle <HANDLE>"),
            ("map", "*edge{from: src, to: dst, kind: kind}"),
            (
                "health",
                "diagnostic{code: code, severity: severity, subject: h, file: file, line: line}",
            ),
            ("diff", "at(\"snapshot:last\")"),
            (
                "obligations",
                "undischarged(h), obligation(h), *handle{id: h, file: file, status: status}",
            ),
            ("garden", "primary_entropy"),
            ("orient", "recent_frontier"),
            ("query", "use the language directly"),
            (
                "explain",
                "diagnostic{code: code, subject: h, file: file, line: line}",
            ),
            ("work", "frontier(h, energy)"),
            ("blocked", "blocker(h, energy, source)"),
            (
                "diagnostics",
                "diagnostic(code, severity, subject, file, line, evidence)",
            ),
            (
                "broken",
                "diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}",
            ),
            (
                "areas",
                "area_health(area, grade, files, errors, cross_edges)",
            ),
            ("trend", "at(\"snapshot:last\")"),
            ("sources", "sources(name, recognizes, capabilities, doc)"),
        ] {
            let err = Invocation::parse(os(&["anneal", command]))
                .expect_err("retired command should teach replacement");
            assert!(err.to_string().contains(expected), "{command}: {err}");

            let err = Invocation::parse(os(&["anneal", "help", command]))
                .expect_err("retired help topic should teach same replacement");
            assert!(err.to_string().contains(expected), "help {command}: {err}");
        }
    }

    #[test]
    fn parses_handle_impact_flag() {
        let parsed =
            Invocation::parse(os(&["anneal", "handle", "b.md", "--impact"])).expect("parse");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Handle {
                handle: "b.md".to_string(),
                impact: true,
                lineage: false,
            }
        );

        let parsed = Invocation::parse(os(&["anneal", "H", "--impact", "b.md"])).expect("parse");
        assert_eq!(
            parsed.command,
            RuntimeCommand::Handle {
                handle: "b.md".to_string(),
                impact: true,
                lineage: false,
            }
        );

        assert!(HelpTopic::Handle.render().contains("--impact"));
        assert!(HelpTopic::Handle.render().contains("--lineage"));
    }

    #[test]
    fn parses_handle_lineage_flag() {
        let parsed =
            Invocation::parse(os(&["anneal", "handle", "b.md", "--lineage"])).expect("parse");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Handle {
                handle: "b.md".to_string(),
                impact: false,
                lineage: true,
            }
        );
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
    fn empty_row_outputs_report_zero_rows_to_stderr() {
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            CommandOutput::rows(
                Vec::new(),
                RowView::Handle {
                    handle: "missing.md".to_string(),
                    impact: false,
                    lineage: false,
                },
            )
            .empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Broken)
                .empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            status_output(Vec::new()).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            status_output(Vec::new()).empty_rows_diagnostic(OutputMode::Human),
            None
        );
    }

    #[test]
    fn empty_binding_rows_emit_a_human_hint() {
        let output = CommandOutput::rows_with_empty_binding_hint(
            vec![row(&[]), row(&[])],
            RowView::Eval,
            Some(r#"? diagnostic{severity: "error", code: code}."#.to_string()),
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render rows");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Results (2)"));
        assert!(rendered.contains("hint: matched 2 rows but no fields are bound for output."));
        assert!(rendered.contains(r#"? diagnostic{severity: "error", code: code}."#));
    }

    #[test]
    fn empty_binding_rows_emit_a_json_stderr_hint() {
        let output = CommandOutput::rows_with_empty_binding_hint(
            vec![row(&[])],
            RowView::Eval,
            Some("? settled(h).".to_string()),
        );

        assert_eq!(
            output.stderr_diagnostic(OutputMode::Json),
            Some(
                "hint: matched 1 rows but no fields are bound for output.\nAdd a variable to extract values, e.g.:\n  ? settled(h)."
                    .to_string()
            )
        );
    }

    #[test]
    fn status_human_render_shows_aggregate_dashboard_and_pointers() {
        let output = status_output(vec![
            status_metric("scale", "handles", 10),
            status_metric("scale", "file_handles", 8),
            status_metric("scale", "file_handles_with_status", 2),
            status_metric("scale", "statusless_file_handles", 6),
            status_metric("convergence", "broken", 1),
            status_metric("convergence", "blocked", 2),
            status_metric("convergence", "open", 3),
            status_metric("convergence", "advancing", 4),
            status_metric("convergence", "holding", 5),
            status_metric("convergence", "drifting", 6),
            status_metric("health", "errors", 1),
            status_metric("health", "blockers", 2),
            status_metric("health", "spec_code_drift", 1),
            status_metric("drift", "cold", 3),
        ]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Status\n"));
        assert!(rendered.contains("Scale        10 handles · 8 files · 25% lifecycle coverage"));
        assert!(
            rendered.contains("Coverage     25% of file handles carry lifecycle status; orientation is graph+recency-led")
        );
        assert!(rendered.contains(
            "Convergence  broken=1  blocked=2  open=3  advancing=4  holding=5  drifting=6"
        ));
        assert!(rendered.contains("Health       errors=1  blockers=2  spec_code_drift=1"));
        assert!(rendered.contains(
            "Code refs    drift evidence not built for 3 refs; run `anneal check --refresh-drift`"
        ));
        assert!(rendered.contains("Read first"));
        assert!(rendered.contains("recent_frontier(h, rank, recency)"));
        assert!(rendered.contains(
            "? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc."
        ));
        assert!(rendered.contains("ranked_anchor(h, rank, score, why)"));
        assert!(rendered.contains(
            "? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc."
        ));
        assert!(rendered.contains("Work"));
        assert!(rendered.contains("diagnostic{code: code, severity: severity"));
        assert!(!rendered.contains("bad.md"));
    }

    #[test]
    fn status_human_render_marks_flow_pending_without_snapshot_baseline() {
        let output = status_output_with_baseline(
            vec![
                status_metric("scale", "handles", 1),
                status_metric("scale", "file_handles", 1),
                status_metric("scale", "file_handles_with_status", 1),
                status_metric("scale", "statusless_file_handles", 0),
                status_metric("convergence", "broken", 0),
                status_metric("convergence", "blocked", 0),
                status_metric("convergence", "open", 1),
            ],
            false,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains(
            "Convergence  broken=0  blocked=0  open=1  advancing=-  holding=-  drifting=-"
        ));
        assert!(rendered.contains("Note: flow signals empty until snapshot baseline accumulates."));
        assert!(rendered.contains("Run `anneal status` again to populate."));
    }

    #[test]
    fn status_human_render_orders_pipeline_rows_by_status_name() {
        let output = status_output(vec![
            status_metric("scale", "handles", 1),
            status_metric("scale", "file_handles", 1),
            status_metric("scale", "file_handles_with_status", 1),
            status_metric("scale", "statusless_file_handles", 0),
            status_metric("pipeline", "stable", 2),
            status_metric("pipeline", "draft", 3),
        ]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(
            rendered.contains("Pipeline     draft 3 · stable 2"),
            "pipeline should render deterministically:\n{rendered}"
        );
    }

    #[test]
    fn status_json_render_preserves_ndjson() {
        let output = status_output(vec![status_metric("convergence", "open", 42)]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Json)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with(
            "{\"category\":\"convergence\",\"count\":42,\"detail\":null,\"name\":\"open\"}\n"
        ));
    }

    #[test]
    fn generic_rows_human_render_is_readable() {
        let output = CommandOutput::rows(
            vec![row(&[
                ("category", Value::String("status".to_string())),
                ("value", Value::String("open question".to_string())),
                ("count", Value::Number(NumberValue::Int(2))),
            ])],
            RowView::Eval,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render rows");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Results (1)\n 1."));
        assert!(rendered.contains("category=status"));
        assert!(rendered.contains(r#"value="open question""#));
        assert!(rendered.contains("count=2"));
    }

    #[test]
    fn eval_empty_binding_hint_uses_query_schema() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "---\ndepends-on: missing.md\n---\n# A\n")
            .expect("write file");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? diagnostic{severity: "error"}."#.to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval runs");
        let CommandOutput::Rows {
            rows,
            empty_binding_hint,
            ..
        } = output
        else {
            panic!("eval should emit rows");
        };

        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| row.fields.is_empty()));
        assert_eq!(
            empty_binding_hint,
            Some(r#"? diagnostic{severity: "error", code: code}."#.to_string())
        );

        assert_eq!(
            session.empty_binding_hint_for_query(r#"? examples("diagnostic", "noop")."#, &rows),
            None
        );
    }

    #[test]
    fn eval_warns_when_query_filters_retired_section_kind() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? *handle{id: h, kind: "section"}."#.to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval runs");
        let CommandOutput::Rows { rows, warnings, .. } = output else {
            panic!("eval should emit rows");
        };

        assert!(rows.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("section handle kind was retired in v0.14"));
        assert!(warnings[0].contains("*span"));
    }

    #[test]
    fn eval_does_not_warn_for_code_section_handles() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? *handle{source: "code", id: h, kind: "section"}."#.to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval runs");
        let CommandOutput::Rows { warnings, .. } = output else {
            panic!("eval should emit rows");
        };

        assert!(warnings.is_empty());
    }

    #[test]
    fn handle_recovers_retired_section_handle_shape() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let Err(error) = session.run(RuntimeCommand::Handle {
            handle: "a.md#A".to_string(),
            impact: false,
            lineage: false,
        }) else {
            panic!("retired section handle should recover");
        };
        let message = error.to_string();

        assert!(message.contains("section handles were retired in v0.14"));
        assert!(message.contains(r#"? *span{handle: "a.md""#));
    }

    #[test]
    fn read_human_render_shows_content_blocks() {
        let output = CommandOutput::rows(
            vec![row(&[
                ("span_id", Value::String("plan.md#full".to_string())),
                ("start_line", Value::Number(NumberValue::Int(10))),
                ("end_line", Value::Number(NumberValue::Int(12))),
                ("tokens", Value::Number(NumberValue::Int(8))),
                (
                    "text",
                    Value::String("Release blocker details.\nNext line.".to_string()),
                ),
            ])],
            RowView::Read,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render read");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Read (1)\n 1. plan.md#full  lines=10-12  tokens=8"));
        assert!(rendered.contains("\n  Release blocker details.\n  Next line.\n"));
        assert!(!rendered.contains("text="));
    }

    #[test]
    fn read_human_render_hints_when_span_is_truncated() {
        let output = CommandOutput::rows(
            vec![row(&[
                ("span_id", Value::String("plan.md#h/long".to_string())),
                ("start_line", Value::Number(NumberValue::Int(10))),
                ("end_line", Value::Number(NumberValue::Int(40))),
                ("tokens", Value::Number(NumberValue::Int(12))),
                ("total_tokens", Value::Number(NumberValue::Int(80))),
                (
                    "text",
                    Value::String("Release blocker details.".to_string()),
                ),
            ])],
            RowView::Read,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render read");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("showing first 12 tokens of span (80 total)"));
        assert!(rendered.contains("use --budget 80"));
    }

    #[test]
    fn describe_human_render_shows_all_doc_cards() {
        let output = CommandOutput::rows(
            vec![
                row(&[(
                    "doc",
                    Value::String(
                        "Search primitive internals.\nKind: engine primitive.".to_string(),
                    ),
                )]),
                row(&[(
                    "doc",
                    Value::String("Search command surface.\nKind: verb.".to_string()),
                )]),
            ],
            RowView::Describe,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render describe");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert_eq!(
            rendered,
            "Search primitive internals.\nKind: engine primitive.\n\nSearch command surface.\nKind: verb.\n"
        );
    }

    #[test]
    fn describe_auto_json_mode_still_renders_teaching_cards() {
        let output = CommandOutput::rows(
            vec![row(&[(
                "doc",
                Value::String("Search command surface.\nKind: verb.".to_string()),
            )])],
            RowView::Describe,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Json)
            .expect("render describe");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert_eq!(rendered, "Search command surface.\nKind: verb.\n");
    }

    #[test]
    fn describe_explicit_json_preserves_ndjson() {
        let output = CommandOutput::rows(
            vec![row(&[(
                "doc",
                Value::String("Search command surface.\nKind: verb.".to_string()),
            )])],
            RowView::Describe,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::JsonExplicit)
            .expect("render describe");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert_eq!(
            rendered,
            "{\"doc\":\"Search command surface.\\nKind: verb.\"}\n"
        );
    }

    #[test]
    fn status_human_render_rejects_schema_drift() {
        let output = status_output(vec![row(&[
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
                summary: Some("Release".to_string()),
                status: Some("active".to_string()),
                disposition: "current_head".to_string(),
                age_days: Some(12),
                topic_signal: "siblings".to_string(),
                newer_topic_sibling_count: 2,
                top_newer_topic_sibling: Some("next.md".to_string()),
            }],
            spans: vec![crate::ContextSpan {
                handle: "plan.md".to_string(),
                span_id: "body".to_string(),
                start_line: 10,
                end_line: 12,
                tokens: 12,
                text: Some("Release blocker details.\nNext line.".to_string()),
            }],
            neighborhood: vec![crate::ContextNeighbor {
                handle: "plan.md".to_string(),
                neighbor: "dep.md".to_string(),
                status: Some("active".to_string()),
                disposition: "current".to_string(),
                age_days: Some(3),
                degree: 4,
                group: "current".to_string(),
            }],
        });
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render context");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Context\nGoal: find release blockers"));
        assert!(rendered.contains("Hits\n 1. plan.md"));
        assert!(rendered.contains("disposition=current_head status=active age_days=12"));
        assert!(rendered.contains("2 unmarked newer topical siblings (top: next.md; follow-up: anneal -e '? currency_suspect(\"plan.md\", newer).')"));
        assert!(rendered.contains("summary=Release"));
        assert!(rendered.contains("Read\nplan.md span=body lines=10-12 tokens=12"));
        assert!(rendered.contains("Neighborhood\nplan.md:\n  current: dep.md disposition=current status=active age_days=3 degree=4"));
    }

    #[test]
    fn context_json_render_streams_event_rows() {
        let output = CommandOutput::Context(ContextOutput {
            goal: "find release blockers".to_string(),
            hits: vec![crate::ContextHit {
                handle: "plan.md".to_string(),
                span_id: Some("body".to_string()),
                score: 0.9,
                reason: "body:release".to_string(),
                field: "body".to_string(),
                summary: Some("Release".to_string()),
                status: Some("active".to_string()),
                disposition: "current_head".to_string(),
                age_days: Some(12),
                topic_signal: "siblings".to_string(),
                newer_topic_sibling_count: 2,
                top_newer_topic_sibling: Some("next.md".to_string()),
            }],
            spans: vec![crate::ContextSpan {
                handle: "plan.md".to_string(),
                span_id: "body".to_string(),
                start_line: 10,
                end_line: 12,
                tokens: 12,
                text: None,
            }],
            neighborhood: vec![crate::ContextNeighbor {
                handle: "plan.md".to_string(),
                neighbor: "dep.md".to_string(),
                status: Some("active".to_string()),
                disposition: "current".to_string(),
                age_days: Some(3),
                degree: 4,
                group: "current".to_string(),
            }],
        });
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::JsonExplicit)
            .expect("render context");
        let rendered = String::from_utf8(rendered).expect("utf8");
        let rows = rendered
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json row"))
            .collect::<Vec<_>>();

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["section"], "goal");
        assert_eq!(rows[0]["goal"], "find release blockers");
        assert_eq!(rows[1]["section"], "hit");
        assert_eq!(rows[1]["handle"], "plan.md");
        assert_eq!(rows[1]["disposition"], "current_head");
        assert_eq!(rows[1]["status"], "active");
        assert_eq!(rows[1]["age_days"], 12);
        assert_eq!(rows[1]["topic_signal"], "siblings");
        assert_eq!(rows[1]["newer_topic_sibling_count"], 2);
        assert_eq!(rows[1]["top_newer_topic_sibling"], "next.md");
        assert_eq!(rows[2]["section"], "span");
        assert_eq!(rows[2]["span_id"], "body");
        assert!(rows[2].get("text").is_none());
        assert_eq!(rows[3]["section"], "neighbor");
        assert_eq!(rows[3]["neighbor"], "dep.md");
        assert_eq!(rows[3]["disposition"], "current");
        assert_eq!(rows[3]["status"], "active");
        assert_eq!(rows[3]["age_days"], 3);
        assert_eq!(rows[3]["degree"], 4);
        assert_eq!(rows[3]["group"], "current");
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
    fn handle_human_render_groups_edges_and_code_refs() {
        let rows = vec![
            row(&[
                ("h", Value::String("doc.md".to_string())),
                ("relation", Value::String("self".to_string())),
                ("other", Value::String("doc.md".to_string())),
                ("kind", Value::String("file".to_string())),
                ("status", Value::String("draft".to_string())),
                ("file", Value::String("doc.md".to_string())),
                ("line", Value::Number(NumberValue::Int(1))),
                ("summary", Value::String(String::new())),
            ]),
            row(&[
                ("h", Value::String("doc.md".to_string())),
                ("relation", Value::String("out".to_string())),
                ("other", Value::String("plan.md".to_string())),
                ("kind", Value::String("DependsOn".to_string())),
                ("status", Value::Null),
                ("file", Value::String("doc.md".to_string())),
                ("line", Value::Number(NumberValue::Int(4))),
                ("summary", Value::String(String::new())),
            ]),
            row(&[
                ("h", Value::String("doc.md".to_string())),
                ("relation", Value::String("code_ref".to_string())),
                (
                    "other",
                    Value::String("lib/example/admission.rs:142-167".to_string()),
                ),
                ("kind", Value::String("Cites".to_string())),
                ("status", Value::Null),
                ("file", Value::String("doc.md".to_string())),
                ("line", Value::Number(NumberValue::Int(8))),
                (
                    "summary",
                    Value::String("lib/example/admission.rs".to_string()),
                ),
            ]),
        ];
        let mut rendered = Vec::new();

        write_handle_text(&mut rendered, "doc.md", false, false, &rows).expect("render handle");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Outgoing\nDependsOn (1)"));
        assert!(rendered.contains(" 1. -> plan.md  at=doc.md:4"));
        assert!(rendered.contains("Code references (1)"));
        assert!(rendered.contains(" 1. lib/example/admission.rs  at=doc.md:8"));
        assert!(rendered.contains("drift evidence not built; run `anneal check --refresh-drift`"));
        assert!(
            rendered
                .contains("follow-up: anneal -e '? assertion_drift(\"doc.md\", target, commits).'")
        );
    }

    #[test]
    fn handle_human_render_annotates_code_ref_drift() {
        let rows = vec![
            row(&[
                ("h", Value::String("doc.md".to_string())),
                ("relation", Value::String("self".to_string())),
                ("other", Value::String("doc.md".to_string())),
                ("kind", Value::String("file".to_string())),
                ("status", Value::String("draft".to_string())),
                ("file", Value::String("doc.md".to_string())),
                ("line", Value::Number(NumberValue::Int(1))),
                ("summary", Value::String(String::new())),
            ]),
            row(&[
                ("h", Value::String("doc.md".to_string())),
                ("relation", Value::String("code_ref".to_string())),
                (
                    "other",
                    Value::String("external:code:doc.md:8:src/cli.rs".to_string()),
                ),
                ("kind", Value::String("Cites".to_string())),
                ("status", Value::Null),
                ("file", Value::String("doc.md".to_string())),
                ("line", Value::Number(NumberValue::Int(8))),
                ("summary", Value::String("src/cli.rs".to_string())),
                (
                    "disposition",
                    Value::String("referent-moved-ambiguous".to_string()),
                ),
                ("candidate_count", Value::String("11".to_string())),
                ("moved_to", Value::Null),
            ]),
        ];
        let mut rendered = Vec::new();

        write_handle_text(&mut rendered, "doc.md", false, false, &rows).expect("render handle");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(
            rendered
                .contains("src/cli.rs  [referent-moved-ambiguous · 11 candidates]  at=doc.md:8")
        );
        assert!(!rendered.contains("drift evidence not built"));
    }

    #[test]
    fn lineage_normalizes_short_handles_before_walking_file_edges() {
        let store = lineage_store();
        let rows = handle_lineage_rows(&store, "sample-formal-model-v14");
        let lineage = rows
            .iter()
            .filter(|row| required_string(row, "relation").is_ok_and(|value| value == "lineage"))
            .collect::<Vec<_>>();

        assert!(lineage.iter().all(|row| {
            required_string(row, "normalized_root")
                .is_ok_and(|root| root == "formal-model/history/sample-formal-model-v14.md")
        }));
        assert!(lineage.iter().any(|row| {
            required_string(row, "other")
                .is_ok_and(|other| other == "formal-model/sample-formal-model-v17.md")
                && required_string(row, "role").is_ok_and(|role| role == "successor")
                && required_string(row, "disposition")
                    .is_ok_and(|disposition| disposition == "current_head")
        }));
        assert!(
            handle_lineage_rows(&store, "raw-v14").is_empty(),
            "raw reversed short-id edges must not be walked without file normalization"
        );
    }

    #[test]
    fn lineage_renderer_shows_merge_predecessors_and_heads() {
        let store = lineage_store();
        let rows = handle_lineage_rows(&store, "implementation/2026-05-31-program-space.md");
        let mut rendered = Vec::new();

        CommandOutput::rows(
            rows,
            RowView::Handle {
                handle: "implementation/2026-05-31-program-space.md".to_string(),
                impact: false,
                lineage: true,
            },
        )
        .write(&mut rendered, OutputMode::Human)
        .expect("render lineage");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Lineage (file supersession)"));
        assert!(rendered.contains("Current head(s) (1)"));
        assert!(rendered.contains("implementation/2026-05-31-program-space.md"));
        assert!(rendered.contains("Older (2)"));
        assert!(rendered.contains("implementation/2026-05-30-unified.md"));
        assert!(rendered.contains("compiler/2026-03-30-cell-graph.md"));
    }

    fn status_output(rows: Vec<Row>) -> CommandOutput {
        status_output_with_baseline(rows, true)
    }

    fn status_output_with_baseline(rows: Vec<Row>, flow_baseline_ready: bool) -> CommandOutput {
        CommandOutput::Status(StatusOutput {
            rows,
            flow_baseline_ready,
        })
    }

    fn status_metric(category: &str, name: &str, count: i64) -> Row {
        row(&[
            ("category", Value::String(category.to_string())),
            ("name", Value::String(name.to_string())),
            ("count", Value::Number(NumberValue::Int(count))),
            ("detail", Value::Null),
        ])
    }

    fn convergence_metric_counts(rows: &[Row]) -> BTreeMap<String, i64> {
        rows.iter()
            .filter_map(|row| {
                let category = required_string(row, "category").ok()?;
                if category != "convergence" {
                    return None;
                }
                let name = required_string(row, "name").ok()?.to_string();
                let count = required_number(row, "count").ok()?;
                let NumberValue::Int(count) = *count else {
                    return None;
                };
                Some((name, count))
            })
            .collect()
    }

    fn status_item_section_counts(rows: &[Row]) -> BTreeMap<String, i64> {
        let mut counts = BTreeMap::new();
        for row in rows {
            let section = required_string(row, "section").expect("status_item row has section");
            *counts.entry(section.to_string()).or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn project_discovery_facts_affect_markdown_extraction() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::create_dir(root.join("included")).expect("create included");
        fs::write(
            root.join("anneal.dl"),
            r#"source md { scan_root("included"). }"#,
        )
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

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Search {
                query: "shared marker".to_string(),
                limit: 10,
                include_low_confidence: false,
            })
            .expect("search runs");
        let CommandOutput::Rows { rows, .. } = output else {
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

    #[test]
    fn project_potential_weight_rule_changes_energy() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("anneal.dl"),
            r#"
            config frontmatter {
              field("depends-on", "DependsOn", "forward").
            }

            potential_weight("broken_ref", 1).
            "#,
        )
        .expect("write project rules");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\ndepends-on: missing.md\n---\n# A\n",
        )
        .expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? potential_weight("broken_ref", weight), potential("a.md", energy)."#
                    .to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval runs");
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("eval should emit rows");
        };

        assert!(rows.iter().any(|row| {
            row.fields.get("weight")
                == Some(&anneal_core::runtime::Value::Number(NumberValue::Int(1)))
        }));
        assert!(rows.iter().any(|row| {
            row.fields.get("energy")
                == Some(&anneal_core::runtime::Value::Number(NumberValue::Int(1)))
        }));
    }

    #[test]
    fn search_boost_project_config_changes_rank_order() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("anneal.dl"),
            r#"
            config search_boost {
              status("draft", 0.09).
              status("authoritative", 0).
              hub(0).
            }
            "#,
        )
        .expect("write project rules");
        fs::write(
            root.join("draft.md"),
            "---\nstatus: draft\n---\n# Draft\n\nlease protocol\n",
        )
        .expect("write draft doc");
        fs::write(
            root.join("authority.md"),
            "---\nstatus: authoritative\n---\n# Authority\n\nlease protocol\n",
        )
        .expect("write authoritative doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Search {
                query: "lease protocol".to_string(),
                limit: 2,
                include_low_confidence: false,
            })
            .expect("search runs");
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("search should emit rows");
        };

        let first = rows.first().expect("first search row");
        assert_eq!(
            first.fields.get("h"),
            Some(&anneal_core::runtime::Value::String("draft.md".to_string()))
        );
    }

    #[test]
    fn handle_impact_projects_configured_reverse_dependencies() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("anneal.dl"),
            r#"
            config frontmatter {
              field("synthesizes", "Synthesizes", "forward").
              field("references", "Cites", "forward").
            }

            config impact {
              traverse(["DependsOn", "Synthesizes"]).
            }
            "#,
        )
        .expect("write project rules");
        fs::write(root.join("b.md"), "# B\n").expect("write b");
        fs::write(root.join("a.md"), "---\ndepends-on: b.md\n---\n# A\n").expect("write a");
        fs::write(root.join("c.md"), "---\nsynthesizes: b.md\n---\n# C\n").expect("write c");
        fs::write(root.join("d.md"), "---\nreferences: b.md\n---\n# D\n").expect("write d");
        fs::write(root.join("e.md"), "---\ndepends-on: a.md\n---\n# E\n").expect("write e");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Handle {
                handle: "b.md".to_string(),
                impact: true,
                lineage: false,
            })
            .expect("handle runs");
        let CommandOutput::Rows { rows, view, .. } = output else {
            panic!("handle should emit rows");
        };
        assert_eq!(
            view,
            RowView::Handle {
                handle: "b.md".to_string(),
                impact: true,
                lineage: false,
            }
        );

        let impacted = rows
            .iter()
            .filter(|row| required_string(row, "relation").is_ok_and(|value| value == "impact"))
            .map(|row| {
                (
                    required_string(row, "other").expect("other").to_string(),
                    *required_number(row, "depth").expect("depth"),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(impacted.get("a.md"), Some(&NumberValue::Int(1)));
        assert_eq!(impacted.get("c.md"), Some(&NumberValue::Int(1)));
        assert_eq!(impacted.get("e.md"), Some(&NumberValue::Int(2)));
        assert!(!impacted.contains_key("d.md"));

        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? impact("b.md", affected, depth), depth = 1."#.to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("impact eval runs");
        let CommandOutput::Rows {
            rows: eval_rows, ..
        } = output
        else {
            panic!("impact eval should emit rows");
        };
        let direct_eval = eval_rows
            .iter()
            .filter_map(|row| required_string(row, "affected").ok().map(ToOwned::to_owned))
            .collect::<BTreeSet<_>>();
        let direct_handle = impacted
            .iter()
            .filter_map(|(handle, depth)| (depth == &NumberValue::Int(1)).then_some(handle.clone()))
            .collect::<BTreeSet<_>>();

        assert_eq!(direct_handle, direct_eval);

        let mut rendered = Vec::new();
        CommandOutput::rows(
            rows,
            RowView::Handle {
                handle: "b.md".to_string(),
                impact: true,
                lineage: false,
            },
        )
        .write(&mut rendered, OutputMode::Human)
        .expect("render handle");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.contains("Impact (configured reverse traversal)\nDirect (2)"));
        assert!(rendered.contains("Indirect (1)"));
        assert!(rendered.contains("a.md"));
        assert!(rendered.contains("c.md"));
        assert!(rendered.contains("e.md"));
        let impact_text = rendered
            .split("Impact (configured reverse traversal)\n")
            .nth(1)
            .expect("impact section");
        assert!(!impact_text.contains("d.md"));
    }

    #[test]
    fn status_writes_capped_automatic_snapshot_history() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "---\nstatus: draft\n---\n# A\n").expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let first = session.run(RuntimeCommand::Status).expect("status runs");
        let CommandOutput::Status(first) = first else {
            panic!("status should emit status output");
        };
        assert!(!first.flow_baseline_ready);

        let session = RuntimeSession::load_for_test(&root).expect("session reloads");
        let second = session
            .run(RuntimeCommand::Status)
            .expect("unchanged status runs");
        let CommandOutput::Status(second) = second else {
            panic!("status should emit status output");
        };
        assert!(second.flow_baseline_ready);

        let history = anneal_core::read_snapshot_history(&root).expect("read history");

        assert_eq!(history.entries().len(), 1);
        assert!(
            history.entries()[0]
                .facts
                .iter()
                .any(|fact| { fact.id == "a.md" && fact.key == "status" && fact.value == "draft" })
        );
    }

    #[test]
    fn status_dashboard_counts_match_status_item_sections() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\n---\n# A\n\nThis cites MISSING-REF.\n",
        )
        .expect("write a");
        fs::write(root.join("b.md"), "---\nstatus: draft\n---\n# B\n").expect("write b");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let status = session.run(RuntimeCommand::Status).expect("status runs");
        let CommandOutput::Status(status) = status else {
            panic!("status should emit status output");
        };
        let metrics = convergence_metric_counts(&status.rows);

        let item_rows = session
            .run(RuntimeCommand::Eval {
                query: "? status_item(section, h, score, why).".to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("status_item eval runs");
        let CommandOutput::Rows { rows, .. } = item_rows else {
            panic!("eval should emit rows");
        };
        let section_counts = status_item_section_counts(&rows);

        for (metric, section) in [
            ("broken", "broken"),
            ("blocked", "blocked"),
            ("open", "work"),
            ("advancing", "advancing"),
            ("holding", "holding"),
            ("drifting", "drifting"),
        ] {
            assert_eq!(
                metrics.get(metric).copied().unwrap_or_default(),
                section_counts.get(section).copied().unwrap_or_default(),
                "{metric} dashboard count should match status_item({section})"
            );
        }
    }

    #[test]
    fn transitive_convergence_queries_demand_code_target_history() {
        for query in [
            "? status_item(section, h, score, why).",
            "? holding(h).",
            "? flow(h, direction).",
            "? ranked_work(h, energy, rank).",
            "? area_frontier(area, h, score, why).",
            "? primary_entropy(h, source).",
        ] {
            assert!(
                query_demands_code_target_history(query),
                "{query} should demand target-history facts through potential/entropy"
            );
        }
        assert!(query_demands_code_target_history(
            "? *meta{handle: h, key: \"target_exists\", value: exists}."
        ));
        assert!(query_demands_code_target_history(
            "? frontier(h, energy), *handle{id: h}."
        ));
        assert!(!query_demands_code_target_history("? *handle{id: h}."));
        assert!(!query_demands_code_target_history(
            "? recent_frontier(h, rank, recency), *handle{id: h}."
        ));
    }

    #[test]
    fn edge_assertion_queries_demand_edge_assertion_probe_only_when_explicit() {
        assert!(query_demands_edge_assertions(
            "? *edge{from: a, to: b, assertion_date: date}."
        ));
        assert!(query_demands_edge_assertions(
            "? *edge{from: a, to: b, assertion_revision: rev}."
        ));
        assert!(!query_demands_edge_assertions(
            "? *edge{from: a, to: b, file: file, line: line}."
        ));
        assert!(!query_demands_edge_assertions(
            "? recent_frontier(h, rank, recency)."
        ));
    }

    #[test]
    fn code_reference_queries_demand_drift_evidence() {
        assert!(query_demands_code_drift_evidence(
            "? code_ref(spec, ref, path, code_handle, disposition)."
        ));
        assert!(query_demands_code_drift_evidence(
            "? drift_profile(bucket, count)."
        ));
        assert!(!query_demands_code_drift_evidence("? *handle{id: h}."));
    }

    #[test]
    fn runtime_loads_snapshot_history_for_eval_at_blocks() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "---\nstatus: current\n---\n# A\n").expect("write doc");
        anneal_core::append_snapshot_entry(
            &root,
            &anneal_core::SnapshotEntry::with_prelude_hash(
                "s1",
                "2026-05-13T10:00:00Z",
                CorpusId::from(DEFAULT_CORPUS),
                "test-prelude",
                vec![anneal_core::SnapshotEntryFact::new(
                    "a.md", "status", "draft",
                )],
            ),
        )
        .expect("append history");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: r#"? at("snapshot:last") { *handle{id: h, status: prior_status} }, *handle{id: h, status: current_status}, prior_status != current_status."#
                    .to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval at block runs");
        let CommandOutput::Rows { rows, warnings, .. } = output else {
            panic!("eval should emit rows");
        };

        assert!(rows.iter().any(|row| {
            row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
                && row.fields.get("prior_status")
                    == Some(&anneal_core::runtime::Value::String("draft".to_string()))
                && row.fields.get("current_status")
                    == Some(&anneal_core::runtime::Value::String("current".to_string()))
        }));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("at(\"snapshot:last\") used snapshot fallback")),
            "expected partial-history warning, got {warnings:?}"
        );

        let quiet_output = session
            .run(RuntimeCommand::Eval {
                query: "? *handle{id: h}.".to_string(),
                explain: ExplainOptions::disabled(),
                limit: Some(1),
            })
            .expect("ordinary eval runs");
        let CommandOutput::Rows {
            warnings: quiet_warnings,
            ..
        } = quiet_output
        else {
            panic!("eval should emit rows");
        };
        assert!(
            quiet_warnings.is_empty(),
            "ordinary eval should not inherit prelude flow warnings: {quiet_warnings:?}"
        );
    }

    #[test]
    fn eval_git_mtime_uses_git_history() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "---\nstatus: draft\n---\n# A\n").expect("write doc");
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "anneal@example.test"]);
        git(&root, &["config", "user.name", "Anneal Test"]);
        git(&root, &["add", "a.md"]);
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root.as_std_path())
            .args(["commit", "-m", "initial"])
            .env("GIT_AUTHOR_DATE", "2026-05-20T12:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2026-05-20T12:00:00+00:00")
            .status()
            .expect("git commit runs");
        assert!(status.success(), "git commit failed: {status}");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Eval {
                query: "? *handle{id: h, file: file}, git_mtime(file, instant).".to_string(),
                explain: ExplainOptions::disabled(),
                limit: None,
            })
            .expect("eval runs");
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("eval should emit rows");
        };

        assert!(rows.iter().any(|row| {
            row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
                && row.fields.get("instant")
                    == Some(&anneal_core::runtime::Value::String(
                        "2026-05-20T12:00:00Z".to_string(),
                    ))
        }));
    }

    #[test]
    fn describe_cards_teach_common_join_patterns() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let runtime = session
            .run(RuntimeCommand::Describe {
                name: "runtime".to_string(),
            })
            .expect("describe runtime runs");
        let CommandOutput::Rows { rows, .. } = runtime else {
            panic!("describe runtime should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| {
                    doc.contains("Visible commands: status, context, search, read, handle, schema, describe, eval, init")
                        && doc.contains("Hidden support commands: check, prime.")
                        && !doc.contains("Hidden support commands: work")
                        && doc.contains("Dimensional map: axis(name, question, oracle, disposition)")
                        && doc.contains("? axis_of(\"currency_suspect\", axis). -> Output: axis")
                        && doc.contains("Observed vocabulary recipes")
                        && doc.contains("? *handle{id: h, file: file}, git_mtime(file, instant). -> Output: h, file, instant")
                        && doc.contains("? changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}. -> Output: h, summary")
                })
            }),
            "describe runtime should fold the command map and vocabulary recipes into the teaching card"
        );

        for name in [
            "diagnostic",
            "search",
            "handle",
            "upstream",
            "downstream",
            "frontier",
            "blocker",
            "broken_reference",
            "blocked",
            "entropy",
            "undischarged",
            "obligation",
            "snapshot",
            "check",
            "E001",
            "W005",
            "lifecycle_config_gap",
            "*meta",
            "external_class",
            "target_path",
        ] {
            let output = session
                .run(RuntimeCommand::Describe {
                    name: name.to_string(),
                })
                .unwrap_or_else(|err| panic!("describe {name} runs: {err}"));
            let CommandOutput::Rows { rows, .. } = output else {
                panic!("describe should emit rows");
            };
            assert!(
                rows.iter().any(|row| {
                    required_string(row, "doc").is_ok_and(|doc| doc.contains("Common joins:"))
                }),
                "describe {name} should teach common joins: {rows:?}"
            );
        }

        let diagnostic = session
            .run(RuntimeCommand::Describe {
                name: "diagnostic".to_string(),
            })
            .expect("describe diagnostic runs");
        let CommandOutput::Rows { rows, .. } = diagnostic else {
            panic!("describe diagnostic should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| {
                    doc.contains("diagnostic{subject: h}, area_of")
                        && doc.contains("Example: ? diagnostic{code: \"E001\"")
                        && doc.contains("Output: h")
                })
            }),
            "describe diagnostic should carry the folded recipe and example"
        );

        let diagnostic_code = session
            .run(RuntimeCommand::Describe {
                name: "E001".to_string(),
            })
            .expect("describe E001 runs");
        let CommandOutput::Rows { rows, .. } = diagnostic_code else {
            panic!("describe E001 should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| {
                    doc.contains("Diagnostic code: E001")
                        && doc.contains("Rule predicate: broken_reference")
                        && doc.contains("Common joins:")
                        && doc.contains("Output: src, target, file, line")
                })
            }),
            "describe E001 should route to the diagnostic catalog and rule predicate"
        );

        let handle = session
            .run(RuntimeCommand::Describe {
                name: "handle".to_string(),
            })
            .expect("describe handle runs");
        let CommandOutput::Rows { rows, .. } = handle else {
            panic!("describe handle should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| {
                    doc.contains("anneal handle H --impact")
                        && doc.contains("*edge{to: h, from: src}")
                        && doc.contains("Output: h, src, kind")
                        && !doc.contains("Output: anneal")
                })
            }),
            "describe handle should teach --impact and reverse dependency shape"
        );

        let meta = session
            .run(RuntimeCommand::Describe {
                name: "*meta".to_string(),
            })
            .expect("describe *meta runs");
        let CommandOutput::Rows { rows, .. } = meta else {
            panic!("describe *meta should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| {
                    doc.contains("STANDARD (defined by anneal")
                        && doc.contains("SOURCE (produced by a specific source adapter")
                        && doc.contains("FRONTMATTER (passed through from YAML")
                        && doc.contains("external_class")
                        && doc.contains("target_path")
                })
            }),
            "describe *meta should teach metadata key categories"
        );
    }

    #[test]
    fn project_verbs_are_callable_from_cli_projection() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");
        fs::write(
            root.join("anneal.dl"),
            r#"
            @verb(
              name: "release-blockers",
              query: "release_blocker(\"ok\", \"v0.11\", false).\nrelease_blocker(\"strict\", \"v0.11\", true).\nrelease_row(h, milestone, strict) :=\n  verb_arg(\"milestone\", milestone),\n  verb_arg(\"strict\", strict),\n  release_blocker(h, milestone, strict).\n\n? release_row(h, milestone, strict).",
              doc: "Project-specific blockers.",
              output_schema: "{\"h\":\"String\",\"milestone\":\"String\",\"strict\":\"Bool\"}",
              args: ["milestone:String", "strict:Bool=false"],
              capabilities: ["read"]
            ).
            "#,
        )
        .expect("write project rules");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Verb {
                name: "release-blockers".to_string(),
                args: vec!["v0.11".to_string()],
            })
            .expect("project verb runs");
        let CommandOutput::Rows { rows, view, .. } = output else {
            panic!("project verb should emit rows");
        };
        assert_eq!(
            view,
            RowView::Verb {
                name: "release-blockers".to_string(),
            }
        );
        assert_eq!(
            rows[0].fields.get("h"),
            Some(&anneal_core::runtime::Value::String("ok".to_string()))
        );
        assert_eq!(
            rows[0].fields.get("milestone"),
            Some(&anneal_core::runtime::Value::String("v0.11".to_string()))
        );
        assert_eq!(
            rows[0].fields.get("strict"),
            Some(&anneal_core::runtime::Value::Bool(false))
        );

        let output = session
            .run(RuntimeCommand::Verb {
                name: "release-blockers".to_string(),
                args: vec![
                    "--milestone".to_string(),
                    "v0.11".to_string(),
                    "--strict".to_string(),
                ],
            })
            .expect("project verb named args run");
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("project verb should emit rows");
        };
        assert_eq!(
            rows[0].fields.get("h"),
            Some(&anneal_core::runtime::Value::String("strict".to_string()))
        );
        assert_eq!(
            rows[0].fields.get("strict"),
            Some(&anneal_core::runtime::Value::Bool(true))
        );
    }

    #[test]
    fn project_verb_named_arg_rejects_option_as_missing_value() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");
        fs::write(
            root.join("anneal.dl"),
            r#"
            @verb(
              name: "release-blockers",
              query: "release_blocker(\"ok\").\nrelease_row(h, milestone, strict) :=\n  verb_arg(\"milestone\", milestone),\n  verb_arg(\"strict\", strict),\n  release_blocker(h).\n\n? release_row(h, milestone, strict).",
              doc: "Project-specific blockers.",
              output_schema: "{\"h\":\"String\",\"milestone\":\"String\",\"strict\":\"Bool\"}",
              args: ["milestone:String", "strict:Bool=false"],
              capabilities: ["read"]
            ).
            "#,
        )
        .expect("write project rules");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let Err(err) = session.run(RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec!["--milestone".to_string(), "--strict".to_string()],
        }) else {
            panic!("missing value should fail");
        };

        assert!(err.to_string().contains("--milestone requires a value"));
    }

    #[test]
    fn project_verb_help_uses_resolved_registry_entry() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");
        fs::write(
            root.join("anneal.dl"),
            r#"
            @verb(
              name: "project-pulse",
              query: "? pulse(h).",
              doc: "Project-specific pulse.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: ["read"]
            ).
            pulse("ok").
            "#,
        )
        .expect("write project rules");

        let session = RuntimeSession::load_for_test(&root).expect("session loads");
        let output = session
            .run(RuntimeCommand::Verb {
                name: "project-pulse".to_string(),
                args: vec!["--help".to_string()],
            })
            .expect("project verb help runs");
        let CommandOutput::Text(text) = output else {
            panic!("project verb help should emit text");
        };
        assert!(text.contains("Usage: anneal [OPTIONS] project-pulse"));
        assert!(text.contains("Project-specific pulse."));
        assert!(text.contains("Output schema:"));
        assert!(text.contains("? pulse(h)."));
    }

    #[test]
    fn runtime_rejects_legacy_toml_config() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("anneal.toml"),
            "[convergence]\nactive = [\"draft\"]\n",
        )
        .expect("write legacy config");

        let Err(err) = RuntimeSession::load(&root, &RuntimeCommand::Schema) else {
            panic!("legacy TOML should be migration-only");
        };

        assert!(
            err.to_string()
                .contains("anneal.toml is a legacy config file")
        );
        assert!(err.to_string().contains("anneal init --force"));
    }
}
