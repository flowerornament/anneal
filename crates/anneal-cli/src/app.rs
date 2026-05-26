use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;

use anneal_core::runtime::eval::{ExplainOptions, NumberValue};
use anneal_core::runtime::prelude::{LoadedPrelude, PreludeError, datalog_string_literal};
use anneal_core::runtime::{
    Database, EvalOptions, Evaluator, Literal, Program, QueryOutput, Row, Value, analyze,
    parse_program, write_ndjson,
};
use anneal_core::{
    ActorContext, CancellationToken, ConfigEntry, ConfigFacts, CorpusId, FactStore, Generation,
    ProjectExtension, Source, SourceContext, SourceInfo, VerbArg, VerbArgKind, VerbCapability,
    VerbEntry, VerbLayer, VerbRegistry, load_project_extension, merge_program_layers,
    render_verb_arg_facts,
};
use anneal_md::MarkdownSource;
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::Utf8PathBuf;
use serde_json::json;

use crate::{
    ContextCommand, ContextOutput, DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT, DescribeCommand,
    ReadCommand, SaveCommand, SaveOutcome, SearchCommand, SourcesCommand,
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
                return false;
            };
            return HelpTopic::parse(topic).is_some()
                || (!topic.starts_with('-') && !is_legacy_surface_command(topic));
        }
        if arg == "check" {
            return true;
        }
        return !arg.starts_with('-') && !is_legacy_surface_command(arg);
    }
    true
}

pub fn main_entry() -> Result<()> {
    run_args(std::env::args_os().collect())
}

pub fn run_args(args: Vec<OsString>) -> Result<()> {
    let mut invocation = Invocation::parse(args)?;
    if let RuntimeCommand::Help { topic } = invocation.command {
        return write_text(io::stdout().lock(), &topic.render());
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
        let registry = RuntimeRegistry::load(&invocation.root)?;
        let entry = registry.registry.resolve_for_actor(name, &registry.actor)?;
        return write_text(io::stdout().lock(), &render_dynamic_verb_help(entry));
    }
    if let RuntimeCommand::Save(command) = &invocation.command {
        let session = RuntimeSession::load(&invocation.root)?;
        let outcome = command.run(&invocation.root, &session.program, &session.registry)?;
        let stdout = io::stdout();
        let mode = invocation.output.resolve(stdout.is_terminal());
        return write_save_outcome(stdout.lock(), mode, &outcome);
    }
    let session = RuntimeSession::load(&invocation.root)?;
    let output = session.run(invocation.command)?;
    let stdout = io::stdout();
    let mode = invocation.output.resolve(stdout.is_terminal());
    if let Some(message) = output.empty_rows_diagnostic(mode) {
        writeln!(io::stderr().lock(), "{message}")?;
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
                    "{arg} is a compatibility filter, not a runtime verb option; use it with compatibility commands such as check/find/map/garden/orient, or express the filter in Datalog with `anneal -e`"
                );
            } else if rest.is_empty() && is_compatibility_render_flag(&arg) {
                bail!(
                    "{arg} is a compatibility rendering flag; runtime verbs use `--format=text`, `--format=json`, or `--json`"
                );
            } else {
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
    Diagnostics {
        gate: bool,
        args: Vec<String>,
    },
    Check,
    Broken,
    Areas,
    Trend,
    Describe {
        name: String,
    },
    Sources,
    Schema,
    Save(SaveCommand),
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
    Status,
    Context,
    Search,
    Read,
    Handle,
    Work,
    Blocked,
    Diagnostics,
    Broken,
    Areas,
    Trend,
    Describe,
    Sources,
    Schema,
    Save,
    Eval,
}

impl HelpTopic {
    fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "status" => Self::Status,
            "context" => Self::Context,
            "search" => Self::Search,
            "read" => Self::Read,
            "handle" | "H" => Self::Handle,
            "work" => Self::Work,
            "blocked" => Self::Blocked,
            "diagnostics" | "check" => Self::Diagnostics,
            "broken" => Self::Broken,
            "areas" => Self::Areas,
            "trend" => Self::Trend,
            "describe" => Self::Describe,
            "sources" => Self::Sources,
            "schema" => Self::Schema,
            "save" => Self::Save,
            "eval" | "-e" | "--eval" => Self::Eval,
            _ => return None,
        })
    }

    fn render(self) -> String {
        let body = match self {
            Self::Status => {
                "\
Usage: anneal [OPTIONS] status

Print compact corpus status from the programmable runtime.

Use this as the arrival command: it summarizes the active convergence frontier
and points at work, blockers, and broken facts.

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
Usage: anneal [OPTIONS] handle <HANDLE>

Show one handle plus bounded incoming/outgoing references.

Alias: anneal H <HANDLE>

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
            Self::Diagnostics => {
                "\
Usage: anneal [OPTIONS] diagnostics [--gate]
       anneal [OPTIONS] check

Show the full diagnostic stream from the checks prelude: errors, warnings,
suggestions, and informational facts.

Options:
      --gate                    Exit 1 if any error-severity diagnostic exists

Filtering is compositional. Use eval pattern calls instead of one-off flags:
  anneal -e '? diagnostic{file: \"document.md\", code: code, severity: severity, subject: h}.'
  anneal -e '? diagnostic{subject: h, code: code}, area_of{h: h, area: \"language\"}.'
  anneal -e '? diagnostic{severity: \"warning\", code: code, subject: h, evidence: why}.'

`anneal check` is a hidden CI-friendly alias for `anneal diagnostics --gate`.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Broken => {
                "\
Usage: anneal [OPTIONS] broken

Show error diagnostics only. For the full stream use `anneal diagnostics`;
for filtered questions use `anneal -e '? diagnostic{...}.'`.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
            Self::Areas => {
                "\
Usage: anneal [OPTIONS] areas

Show per-area health grades and the strongest unsettled-work frontier inside
each area.

Use this after `anneal status` when the convergence frontier points at a broad
area and you need a smaller place to start.

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
            Self::Save => {
                "\
Usage: anneal [OPTIONS] save <NAME> <QUERY> --doc <TEXT> [--args <ARGS>] [--force]

Promote a working eval query into a project @verb declaration in anneal.dl.
Saved verbs are callable as `anneal <NAME>` and documented by
`anneal describe <NAME>` / `anneal help <NAME>`.

Arguments:
  <NAME>                         Verb name, e.g. broken-area
  <QUERY>                        Datalog query text

Options:
      --doc <TEXT>               Verb help text to write into @verb
      --args <ARGS>              Comma-separated typed args, e.g. area:String,limit:Int=10
      --force                    Replace an existing project verb or shadow a prelude verb

If a declared arg name appears as a variable in the final query and the query
does not already bind it with `verb_arg(\"name\", name)`, save injects that
binding into the saved query. For more complex local-rule shapes, write the
verb_arg(...) join explicitly.

Recovery: inspect anneal.dl, remove the generated @verb(...) block, or rerun
anneal save with --force to replace the saved verb.

Examples:
  anneal save broken-area '? diagnostic{subject: h}, area_of{h: h, area: area}.' \\
    --args area:String --doc 'Diagnostics in one area.'
  anneal broken-area language --format=text
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
    ? top_work(h, energy).
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
        (h, energy) : top_work(h, energy)
      }.

  Time blocks query supported historical references:
    ? at(\"snapshot:last\") { *handle{id: h, status: old} },
      *handle{id: h, status: now},
      old != now.

  Stratification rule of thumb:
    recursive rules are fine; negation and aggregates must not depend on
    themselves through a cycle. If analysis rejects a query, split the negative
    or aggregate part into a later rule.

Discover before guessing:
  anneal schema --format=text
  anneal describe runtime --format=text
  anneal describe search --format=text
  anneal -e '? source_of(\"work\", file, lines).'

Examples:
  anneal -e '? *handle{id: h, kind: \"file\", status: s}.' --limit 20
  anneal -e '? *edge{from: src, to: dst, kind: \"DependsOn\"}.'
  anneal -e '? search{query: \"conformance\", handle: h, score: score}.' --limit 20
  anneal -e '? read{handle: \"formal-model/v17.md\", budget: 4000, text: text}.'
  anneal -e '? diagnostic{severity: \"error\", subject: h, file: file}.'
  anneal -e '? top_work(h, energy), *handle{id: h, file: file, summary: summary}.'
  anneal -e '? source_of(\"work\", file, lines).'
  anneal -e - < query.dl

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
        };
        if matches!(self, Self::Eval | Self::Save) {
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
      --root <PATH>              Corpus root (default: .design, docs, or .)
      --json                     Force JSON/NDJSON output
      --format <text|json>       Force readable text or JSON/NDJSON output
";

impl RuntimeCommand {
    fn parse(args: &[String]) -> Result<Self> {
        let Some((command, rest)) = args.split_first() else {
            bail!("missing runtime command");
        };
        if command == "help" {
            let topic = rest.first().context("help requires a runtime command")?;
            ensure!(
                rest.len() == 1,
                "help accepts one runtime command or verb name"
            );
            if let Some(topic) = HelpTopic::parse(topic) {
                return Ok(Self::Help { topic });
            }
            if let Some(message) = retired_teaching_command_message(topic) {
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
            "status" => {
                ensure_no_args(rest, "status")?;
                Ok(Self::Status)
            }
            "context" => parse_context(rest),
            "search" => parse_search(rest),
            "read" => parse_read(rest),
            "handle" | "H" => Ok(Self::Handle {
                handle: required_runtime_positional(rest, "handle", "handle requires a handle")?
                    .to_string(),
            }),
            "work" => {
                ensure_no_args(rest, "work")?;
                Ok(Self::Work)
            }
            "blocked" => Ok(Self::Blocked {
                handle: required_runtime_positional(
                    rest,
                    "blocked",
                    "blocked inspects one handle; pass `anneal blocked <HANDLE>` or use `anneal status` for a corpus-wide blocked list",
                )?
                .to_string(),
            }),
            "diagnostics" => Ok(parse_diagnostics(rest)),
            "check" => parse_check(rest),
            "broken" => {
                ensure_no_args(rest, "broken")?;
                Ok(Self::Broken)
            }
            "areas" => {
                ensure_no_args(rest, "areas")?;
                Ok(Self::Areas)
            }
            "trend" => {
                ensure_no_args(rest, "trend")?;
                Ok(Self::Trend)
            }
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
                    bail!("describe accepts at most one name; got {:?}", rest.join(" "))
                }
            },
            "sources" => {
                ensure_no_args(rest, "sources")?;
                Ok(Self::Sources)
            }
            "schema" => {
                ensure_no_args(rest, "schema")?;
                Ok(Self::Schema)
            }
            "save" => parse_save(rest),
            "-e" | "--eval" | "eval" => parse_eval(rest),
            other if other.starts_with('-') => bail!("unknown runtime option {other:?}"),
            other @ ("cookbook" | "vocab" | "verbs" | "examples") => {
                bail!("{}", retired_teaching_command_message(other).expect("retired command message"))
            }
            other => Ok(parse_dynamic_verb(other, rest)),
        }
    }
}

fn retired_teaching_command_message(command: &str) -> Option<&'static str> {
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
        _ => None,
    }
}

struct RuntimeSession {
    program: Program,
    store: FactStore,
    registry: VerbRegistry,
    actor: ActorContext,
    sources: Vec<SourceInfo>,
}

struct RuntimeRegistry {
    registry: VerbRegistry,
    actor: ActorContext,
}

impl RuntimeRegistry {
    fn load(root: &camino::Utf8Path) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let source_info = MarkdownSource::default().describe();
        let sources = vec![source_info];
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
    fn load(root: &camino::Utf8Path) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let corpus = CorpusId::from(DEFAULT_CORPUS);
        let source_info = MarkdownSource::default().describe();
        let sources = vec![source_info];
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
        let source = MarkdownSource::with_runtime_config(&runtime_config)
            .map_err(|err| anyhow!("markdown config failed: {err}"))?;
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
        let configs = runtime_config_facts(project.as_ref(), &corpus);
        if !configs.is_empty() {
            store
                .replace_configs(&corpus, configs)
                .context("failed to merge runtime config facts")?;
        }
        let registry = match &project {
            Some(project) => VerbRegistry::from_layers(&[
                (VerbLayer::Prelude, loaded_prelude.program()),
                (VerbLayer::Project, project.program()),
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
                self.run_query(&query, ExplainOptions::disabled(), RowView::Search)
            }
            RuntimeCommand::Read { handle, budget } => {
                let query = ReadCommand::new(handle).with_budget(budget).datalog();
                self.run_query(&query, ExplainOptions::disabled(), RowView::Read)
            }
            RuntimeCommand::Handle { handle } => self.run_query(
                &handle_query(&handle),
                ExplainOptions::disabled(),
                RowView::Handle { handle },
            ),
            RuntimeCommand::Work => self.run_verb("work", RowView::Work),
            RuntimeCommand::Blocked { handle } => self.run_query(
                &blocked_query(&handle),
                ExplainOptions::disabled(),
                RowView::Blocked,
            ),
            RuntimeCommand::Diagnostics { gate, args } => self.run_diagnostics(gate, &args),
            RuntimeCommand::Check => self.run_check_gate(),
            RuntimeCommand::Broken => self.run_verb("broken", RowView::Broken),
            RuntimeCommand::Areas => self.run_verb("areas", RowView::Areas),
            RuntimeCommand::Trend => self.run_verb("trend", RowView::Trend),
            RuntimeCommand::Describe { name } => {
                let query = DescribeCommand::new(&name).datalog();
                let output = self.eval(&query, ExplainOptions::disabled())?;
                ensure!(
                    !output.rows.is_empty(),
                    "unknown runtime name {name:?}; use `anneal schema` or `anneal describe runtime`"
                );
                Ok(CommandOutput::rows(output.rows, RowView::Describe))
            }
            RuntimeCommand::Sources => self.run_query(
                SourcesCommand.datalog(),
                ExplainOptions::disabled(),
                RowView::Sources,
            ),
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
                Ok(CommandOutput::rows(output.rows, RowView::Eval))
            }
            RuntimeCommand::Verb { name, args } => self.run_dynamic_verb(&name, &args),
            RuntimeCommand::Save(_) => {
                bail!("save is handled before corpus evaluation")
            }
            RuntimeCommand::Help { topic } => Ok(CommandOutput::Text(topic.render())),
        }
    }

    fn run_verb(&self, name: &str, view: RowView) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        self.run_query(plan.query_source(), ExplainOptions::disabled(), view)
    }

    fn run_diagnostics(&self, gate: bool, args: &[String]) -> Result<CommandOutput> {
        let output =
            self.run_dynamic_verb_with_view("diagnostics", args, Some(RowView::Diagnostics))?;
        let gate_failed = gate && self.error_diagnostics_exist()?;
        Ok(output.with_gate_failed(gate_failed))
    }

    fn run_check_gate(&self) -> Result<CommandOutput> {
        let output = self.run_verb("broken", RowView::Broken)?;
        let gate_failed = output.has_rows();
        Ok(output.with_gate_failed(gate_failed))
    }

    fn error_diagnostics_exist(&self) -> Result<bool> {
        Ok(self.run_verb("broken", RowView::Broken)?.has_rows())
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
        Ok(CommandOutput::rows(
            output.rows,
            view.unwrap_or_else(|| RowView::Verb {
                name: plan.name().to_string(),
            }),
        ))
    }

    fn run_status(&self) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor("status", &self.actor)?;
        let output = self.eval(plan.query_source(), ExplainOptions::disabled())?;
        Ok(CommandOutput::Status(output.rows))
    }

    fn run_query(
        &self,
        query: &str,
        explain: ExplainOptions,
        view: RowView,
    ) -> Result<CommandOutput> {
        let output = self.eval(query, explain)?;
        Ok(CommandOutput::rows(output.rows, view))
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

fn runtime_config_facts(
    project: Option<&ProjectExtension>,
    corpus: &CorpusId,
) -> Vec<anneal_core::ConfigFact> {
    project.map_or_else(Vec::new, |project| project.runtime_config_facts(corpus))
}

enum CommandOutput {
    Rows {
        rows: Vec<Row>,
        view: RowView,
        gate_failed: bool,
    },
    Status(Vec<Row>),
    Context(ContextOutput),
    Text(String),
}

impl CommandOutput {
    const fn rows(rows: Vec<Row>, view: RowView) -> Self {
        Self::Rows {
            rows,
            view,
            gate_failed: false,
        }
    }

    fn with_gate_failed(self, gate_failed: bool) -> Self {
        match self {
            Self::Rows { rows, view, .. } => Self::Rows {
                rows,
                view,
                gate_failed,
            },
            other => other,
        }
    }

    fn has_rows(&self) -> bool {
        match self {
            Self::Rows { rows, .. } | Self::Status(rows) => !rows.is_empty(),
            Self::Context(_) | Self::Text(_) => false,
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
            | (OutputMode::Json | OutputMode::JsonExplicit, Self::Status(rows))
                if !matches!(mode, OutputMode::Human) && rows.is_empty() =>
            {
                Some(EMPTY_ROWS_DIAGNOSTIC)
            }
            (_, Self::Status(_) | Self::Rows { .. } | Self::Context(_) | Self::Text(_)) => None,
        }
    }

    fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match (mode, self) {
            (OutputMode::Human, Self::Status(rows)) => write_status_text(writer, &rows)?,
            (OutputMode::Human, Self::Context(output)) => write_context_text(writer, &output)?,
            (OutputMode::Human, Self::Rows { rows, view, .. }) => {
                write_rows_text(writer, &rows, &view)?;
            }
            (
                OutputMode::Json,
                Self::Rows {
                    rows,
                    view: RowView::Describe,
                    ..
                },
            ) => write_describe_text(writer, &rows)?,
            (_, Self::Status(rows) | Self::Rows { rows, .. }) => write_ndjson(writer, rows)?,
            (_, Self::Context(output)) => write_ndjson(writer, std::iter::once(output))?,
            (_, Self::Text(text)) => write_text(writer, &text)?,
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RowView {
    Search,
    Read,
    Handle { handle: String },
    Work,
    Blocked,
    Diagnostics,
    Broken,
    Areas,
    Trend,
    Describe,
    Sources,
    Schema,
    Eval,
    Verb { name: String },
}

impl RowView {
    fn heading(&self, count: usize) -> Option<String> {
        let heading = match self {
            Self::Search => format!("Search ({count})"),
            Self::Read => format!("Read ({count})"),
            Self::Handle { handle } => format!("Handle {handle} ({count} edges)"),
            Self::Work => format!("Work ({count})"),
            Self::Blocked => format!("Blocked ({count})"),
            Self::Diagnostics => format!("Diagnostics ({count})"),
            Self::Broken => format!("Broken ({count})"),
            Self::Areas => format!("Areas ({count})"),
            Self::Trend => format!("Trend ({count})"),
            Self::Describe => return None,
            Self::Sources => format!("Sources ({count})"),
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
      --root <PATH>              Corpus root (default: .design, docs, or .)
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
                "verb '{}' has no argument '{}'; {raw} is a compatibility rendering flag. Runtime verbs use `--format=text`, `--format=json`, or `--json`",
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
                        "verb '{}' has no argument '{}'; {raw} is a compatibility filter, not a runtime verb option. Use a declared verb argument, or express the filter in Datalog with `anneal -e`",
                        self.entry.name(),
                        name,
                    )
                } else if is_compatibility_render_flag(raw) {
                    anyhow::anyhow!(
                        "verb '{}' has no argument '{}'; {raw} is a compatibility rendering flag. Runtime verbs use `--format=text`, `--format=json`, or `--json`",
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
    Ok(())
}

fn write_save_outcome<W: Write>(
    mut writer: W,
    mode: OutputMode,
    outcome: &SaveOutcome,
) -> Result<()> {
    if matches!(mode, OutputMode::Json | OutputMode::JsonExplicit) {
        let record = json!({
            "name": outcome.name,
            "path": outcome.path.as_str(),
            "replaced": outcome.replaced,
            "shadowed": outcome.shadowed,
        });
        writeln!(writer, "{}", serde_json::to_string(&record)?)?;
        return Ok(());
    }
    writeln!(writer, "Saved verb {}", outcome.name)?;
    writeln!(writer, "Path: {}", outcome.path)?;
    if let Some(replaced) = &outcome.replaced {
        writeln!(writer, "Replaced: {replaced}")?;
    }
    if let Some(shadowed) = &outcome.shadowed {
        writeln!(writer, "Shadows: {shadowed}")?;
    }
    writeln!(writer, "Next: anneal {} --help", outcome.name)?;
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
    for section_rows in sections.values_mut() {
        section_rows.sort_by(compare_status_rows);
    }

    writeln!(
        writer,
        "Convergence  broken={}  blocked={}  work={}  advancing={}",
        section_len(&sections, "broken"),
        section_len(&sections, "blocked"),
        section_len(&sections, "work"),
        section_len(&sections, "advancing")
    )?;

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
        writeln!(writer)?;
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

fn section_len(sections: &BTreeMap<&str, Vec<StatusRow<'_>>>, section: &str) -> usize {
    sections.get(section).map_or(0, Vec::len)
}

fn compare_status_rows(left: &StatusRow<'_>, right: &StatusRow<'_>) -> std::cmp::Ordering {
    right
        .score
        .cmp(left.score)
        .then_with(|| status_reason_rank(left.why).cmp(&status_reason_rank(right.why)))
        .then_with(|| left.handle.cmp(right.handle))
}

fn status_reason_rank(reason: &str) -> u8 {
    match reason {
        "E001" | "broken_ref" => 0,
        "undischarged" => 1,
        "stale_dep" => 2,
        "confidence_gap" => 3,
        "freshness_decay" => 4,
        "missing_meta" => 5,
        "orphan_label" => 6,
        "potential" => 7,
        "recently_advanced" => 8,
        _ => 9,
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
            write_text_block(&mut writer, &span.text, MAX_TEXT_LINES_PER_SPAN)?;
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

fn write_rows_text<W: Write>(mut writer: W, rows: &[Row], view: &RowView) -> Result<()> {
    if let RowView::Handle { handle } = view {
        return write_handle_text(writer, handle, rows);
    }

    if *view == RowView::Describe {
        return write_describe_text(writer, rows);
    }

    if *view == RowView::Read {
        return write_read_text(writer, rows);
    }

    if *view == RowView::Areas {
        return write_areas_text(writer, rows);
    }

    if *view == RowView::Trend && rows.is_empty() {
        writeln!(writer, "No trend rows -- snapshot history is empty.")?;
        return Ok(());
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
    Ok(())
}

fn write_areas_text<W: Write>(mut writer: W, rows: &[Row]) -> Result<()> {
    let mut health = Vec::new();
    let mut frontier = Vec::new();

    for row in rows {
        match required_string(row, "section")? {
            "health" => health.push(row),
            "frontier" => frontier.push(row),
            section => bail!("areas row has unknown section {section:?}"),
        }
    }

    health.sort_by(|left, right| {
        let left_grade = optional_string(left, "grade").ok().flatten().unwrap_or("");
        let right_grade = optional_string(right, "grade").ok().flatten().unwrap_or("");
        area_grade_rank(left_grade)
            .cmp(&area_grade_rank(right_grade))
            .then_with(|| {
                required_string(left, "area")
                    .unwrap_or("")
                    .cmp(required_string(right, "area").unwrap_or(""))
            })
    });
    frontier.sort_by(|left, right| {
        let left_score = required_number(left, "score")
            .ok()
            .copied()
            .unwrap_or(NumberValue::Int(0));
        let right_score = required_number(right, "score")
            .ok()
            .copied()
            .unwrap_or(NumberValue::Int(0));
        required_string(left, "area")
            .unwrap_or("")
            .cmp(required_string(right, "area").unwrap_or(""))
            .then_with(|| right_score.cmp(&left_score))
            .then_with(|| {
                required_string(left, "h")
                    .unwrap_or("")
                    .cmp(required_string(right, "h").unwrap_or(""))
            })
    });

    writeln!(writer, "Areas")?;
    writeln!(writer, "Health ({})", health.len())?;
    if health.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
    } else {
        for (index, row) in health.iter().enumerate() {
            let area = required_string(row, "area")?;
            let grade = optional_string(row, "grade")?.unwrap_or("-");
            let files = required_number(row, "files")?;
            let errors = required_number(row, "errors")?;
            let cross_edges = required_number(row, "cross_edges")?;
            writeln!(
                writer,
                "{:>2}. {area}  grade={grade}  files={}  errors={}  cross_edges={}",
                index + 1,
                display_number(files),
                display_number(errors),
                display_number(cross_edges)
            )?;
        }
    }

    if !frontier.is_empty() {
        writeln!(writer)?;
        writeln!(writer, "Frontier ({})", frontier.len())?;
        for (index, row) in frontier.iter().enumerate() {
            let area = required_string(row, "area")?;
            let handle = optional_string(row, "h")?.unwrap_or("-");
            let score = required_number(row, "score")?;
            let why = optional_string(row, "why")?.unwrap_or("-");
            writeln!(
                writer,
                "{:>2}. {area}  {handle}  score={}  {why}",
                index + 1,
                display_number(score)
            )?;
        }
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
    }
    Ok(())
}

fn area_grade_rank(grade: &str) -> u8 {
    match grade {
        "D" => 0,
        "C" => 1,
        "B" => 2,
        "A" => 3,
        _ => 4,
    }
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

fn write_handle_text<W: Write>(mut writer: W, handle: &str, rows: &[Row]) -> Result<()> {
    let edge_count = rows
        .iter()
        .filter(|row| !matches!(required_string(row, "relation"), Ok("self")))
        .count();

    writeln!(writer, "Handle {handle} ({edge_count} edges)")?;
    if rows.is_empty() {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
        return Ok(());
    }

    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
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
            _ => {}
        }
    }

    if !wrote_self {
        writeln!(writer, "{EMPTY_ROWS_DIAGNOSTIC}")?;
    }
    write_handle_edges(&mut writer, "Outgoing", "->", &outgoing)?;
    write_handle_edges(&mut writer, "Incoming", "<-", &incoming)?;
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
    for (index, row) in rows.iter().take(MAX_HANDLE_EDGES_PER_SECTION).enumerate() {
        let other = required_string(row, "other")?;
        let kind = required_string(row, "kind")?;
        let file = required_string(row, "file")?;
        let line = required_number(row, "line")?;
        writeln!(
            writer,
            "{:>2}. {kind} {arrow} {other}  at={file}:{}",
            index + 1,
            display_number(line)
        )?;
    }
    let omitted = rows.len().saturating_sub(MAX_HANDLE_EDGES_PER_SECTION);
    if omitted > 0 {
        writeln!(writer, "    ... {omitted} more")?;
    }
    Ok(())
}

fn section_title(section: &str) -> String {
    if section == "work" {
        return "Other work".to_string();
    }

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
            "--hits" => hits = parse_usize(next_value(&mut iter, arg)?, arg)?,
            "--depth" | "--neighborhood-depth" => {
                depth = parse_i64(next_value(&mut iter, arg)?, arg)?;
            }
            "--include-low-confidence" => include_low_confidence = true,
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
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--budget" => budget = parse_i64(next_value(&mut iter, "--budget")?, "--budget")?,
            value if value.starts_with("--budget=") => {
                budget = parse_i64(value_after_equals(value), "--budget")?;
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
    })
}

fn parse_diagnostics(args: &[String]) -> RuntimeCommand {
    let mut gate = false;
    let mut verb_args = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--gate" => gate = true,
            value => verb_args.push(value.to_string()),
        }
    }
    RuntimeCommand::Diagnostics {
        gate,
        args: verb_args,
    }
}

fn parse_check(args: &[String]) -> Result<RuntimeCommand> {
    if args.is_empty() {
        return Ok(RuntimeCommand::Check);
    }
    bail!(
        "check is a hidden gate alias for `anneal diagnostics --gate` and accepts no filters; use `anneal -e '? diagnostic{{...}}.'` for filtered checks"
    )
}

fn parse_save(args: &[String]) -> Result<RuntimeCommand> {
    let mut name = None;
    let mut query = None;
    let mut doc = None;
    let mut arg_specs = Vec::new();
    let mut force = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(RuntimeCommand::Help {
                    topic: HelpTopic::Save,
                });
            }
            "--doc" => {
                assign_once(
                    &mut doc,
                    next_value(&mut iter, "--doc")?,
                    "save accepts one --doc value",
                )?;
            }
            value if value.starts_with("--doc=") => {
                assign_once(
                    &mut doc,
                    value_after_equals(value),
                    "save accepts one --doc value",
                )?;
            }
            "--args" => {
                arg_specs.extend(parse_save_arg_list(next_value(&mut iter, "--args")?));
            }
            value if value.starts_with("--args=") => {
                arg_specs.extend(parse_save_arg_list(value_after_equals(value)));
            }
            "--force" => force = true,
            value if value.starts_with('-') => {
                reject_runtime_compatibility_flag("save", value)?;
                bail!("unknown save option {value:?}");
            }
            value if name.is_none() => {
                name = Some(value.to_string());
            }
            value if query.is_none() => {
                query = Some(value.to_string());
            }
            value => bail!("save accepts one name and one query; unexpected argument {value:?}"),
        }
    }
    Ok(RuntimeCommand::Save(SaveCommand {
        name: name.context("save requires a verb name")?,
        query: query.context("save requires a query string")?,
        args: arg_specs,
        doc: doc.context("save requires --doc <TEXT>")?,
        force,
    }))
}

fn parse_save_arg_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
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
        "work" => "work",
        "blocked" => "blocked",
        "broken" => "broken",
        "areas" => "areas",
        "trend" => "trend",
        "describe" => "describe",
        "sources" => "sources",
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

fn required_runtime_positional<'a>(
    args: &'a [String],
    command: &str,
    message: &str,
) -> Result<&'a str> {
    match args {
        [value] if value.starts_with('-') => {
            reject_runtime_compatibility_flag(command, value)?;
            Ok(value)
        }
        [value] => Ok(value),
        [] => bail!("{message}"),
        _ => {
            if let Some(flag) = args.first().filter(|arg| arg.starts_with('-')) {
                reject_runtime_compatibility_flag(command, flag)?;
            }
            bail!("{message}; got extra arguments")
        }
    }
}

fn reject_runtime_compatibility_flag(command: &str, flag: &str) -> Result<()> {
    if is_compatibility_filter_flag(flag) {
        bail!(
            "{command} does not accept compatibility filter {flag}; express the filter in Datalog with `anneal -e`"
        );
    }
    if is_compatibility_render_flag(flag) {
        bail!(
            "{command} does not accept compatibility rendering flag {flag}; use `--format=text`, `--format=json`, or `--json`"
        );
    }
    Ok(())
}

fn ensure_no_args(args: &[String], command: &str) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else if let Some(flag) = args.first().filter(|arg| is_compatibility_filter_flag(arg)) {
        bail!(
            "{command} does not accept compatibility filter {flag}; express the filter in Datalog with `anneal -e`"
        )
    } else if let Some(flag) = args.first().filter(|arg| is_compatibility_render_flag(arg)) {
        bail!(
            "{command} does not accept compatibility rendering flag {flag}; use `--format=text`, `--format=json`, or `--json`"
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
    matches!(arg, "--area" | "--recent" | "--since")
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

fn is_legacy_surface_command(arg: &str) -> bool {
    matches!(
        arg,
        "init"
            | "prime"
            | "anneal"
            | "check"
            | "get"
            | "find"
            | "impact"
            | "map"
            | "health"
            | "diff"
            | "obligations"
            | "garden"
            | "orient"
            | "query"
            | "explain"
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

    fn run_save(root: &camino::Utf8Path, command: &SaveCommand) -> Result<SaveOutcome> {
        let session = RuntimeSession::load(root)?;
        command.run(root, &session.program, &session.registry)
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
        assert!(should_handle_args(&os(&["anneal", "areas"])));
        assert!(should_handle_args(&os(&["anneal", "help", "areas"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--area", "compiler", "status"
        ])));
        assert!(should_handle_args(&os(&["anneal", "--pretty", "status"])));
        assert!(!should_handle_args(&os(&[
            "anneal", "--root", ".design", "health"
        ])));
        assert!(should_handle_args(&os(&["anneal", "diagnostics"])));
        assert!(should_handle_args(&os(&[
            "anneal",
            "--format=text",
            "diagnostics",
            "--gate"
        ])));
        assert!(should_handle_args(&os(&["anneal", "help", "diagnostics"])));
        assert!(should_handle_args(&os(&["anneal", "check"])));
        assert!(should_handle_args(&os(&[
            "anneal", "--area", "compiler", "check"
        ])));
        assert!(!should_handle_args(&os(&["anneal", "init"])));
        assert!(!should_handle_args(&os(&["anneal", "prime"])));
        assert!(should_handle_args(&os(&["anneal", "help", "check"])));
        assert!(!should_handle_args(&os(&["anneal", "--help"])));
        assert!(should_handle_args(&os(&["anneal", "check", "--json"])));
        assert!(!should_handle_args(&os(&["anneal", "--mcp"])));
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
                .contains("does not accept compatibility filter"),
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
    fn parses_diagnostics_and_gate_alias() {
        let parsed = Invocation::parse(os(&["anneal", "diagnostics"])).expect("parse diagnostics");
        let RuntimeCommand::Diagnostics { gate, args } = parsed.command else {
            panic!("expected diagnostics command");
        };
        assert!(!gate);
        assert!(args.is_empty());

        let parsed =
            Invocation::parse(os(&["anneal", "diagnostics", "--gate"])).expect("parse gate");
        let RuntimeCommand::Diagnostics { gate, args } = parsed.command else {
            panic!("expected diagnostics command");
        };
        assert!(gate);
        assert!(args.is_empty());

        let parsed = Invocation::parse(os(&[
            "anneal",
            "diagnostics",
            "--gate",
            "--explain-first=1",
        ]))
        .expect("parse gate with explain");
        let RuntimeCommand::Diagnostics { gate, args } = parsed.command else {
            panic!("expected diagnostics command");
        };
        assert!(gate);
        assert_eq!(args, ["--explain-first=1"]);

        let parsed = Invocation::parse(os(&["anneal", "check"])).expect("parse check");
        assert_eq!(parsed.command, RuntimeCommand::Check);

        let parsed = Invocation::parse(os(&["anneal", "check", "--json"])).expect("parse check");
        assert_eq!(parsed.command, RuntimeCommand::Check);
        assert_eq!(parsed.output, OutputPreference::Json);

        let err = Invocation::parse(os(&["anneal", "check", "--active-only"]))
            .expect_err("check no longer accepts compatibility filters");
        assert!(
            err.to_string().contains("check is a hidden gate alias"),
            "{err}"
        );

        let parsed = Invocation::parse(os(&["anneal", "diagnostics", "--area=language"]))
            .expect("diagnostics lets dynamic verb parsing report argument errors");
        let RuntimeCommand::Diagnostics { args, .. } = parsed.command else {
            panic!("expected diagnostics command");
        };
        assert_eq!(args, ["--area=language"]);

        let parsed = Invocation::parse(os(&["anneal", "diagnostics", "anneal-spec.md"]))
            .expect("diagnostics lets dynamic verb parsing report positional errors");
        let RuntimeCommand::Diagnostics { args, .. } = parsed.command else {
            panic!("expected diagnostics command");
        };
        assert_eq!(args, ["anneal-spec.md"]);
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
            ("context", HelpTopic::Context, "Output: human summary"),
            ("search", HelpTopic::Search, "Output: readable rows"),
            ("read", HelpTopic::Read, "Output: readable rows"),
            (
                "diagnostics",
                HelpTopic::Diagnostics,
                "`anneal check` is a hidden CI-friendly alias",
            ),
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
            HelpTopic::Status.render().contains("arrival command")
                && !HelpTopic::Status.render().contains("0.10 and earlier"),
            "status help should teach the current arrival surface"
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
        assert!(
            HelpTopic::Eval
                .render()
                .contains("Discover before guessing")
        );
        assert!(HelpTopic::Eval.render().contains("source_of"));
        assert!(HelpTopic::Eval.render().contains("anneal -e - < query.dl"));
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
        let parsed = Invocation::parse(os(&["anneal", "blocked", "OQ-1", "--explain"]))
            .expect("parse standard explain");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Verb {
                name: "blocked".to_string(),
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
            Invocation::parse(os(&["anneal", "--format=text", "work"])).expect("parse work");

        assert_eq!(parsed.command, RuntimeCommand::Work);
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
    fn parses_areas_as_runtime_command() {
        let parsed = Invocation::parse(os(&["anneal", "areas"])).expect("parse areas");

        assert_eq!(parsed.command, RuntimeCommand::Areas);
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
            CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Trend)
                .empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            CommandOutput::rows(
                Vec::new(),
                RowView::Handle {
                    handle: "missing.md".to_string(),
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
            CommandOutput::Status(Vec::new()).empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::Status(Vec::new()).empty_rows_diagnostic(OutputMode::Human),
            None
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Diagnostics)
                .empty_rows_diagnostic(OutputMode::Json),
            Some(EMPTY_ROWS_DIAGNOSTIC)
        );
        assert_eq!(
            CommandOutput::rows(Vec::new(), RowView::Diagnostics)
                .empty_rows_diagnostic(OutputMode::Human),
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
        assert!(rendered.contains("Convergence  broken=1  blocked=0  work=1  advancing=0"));
        assert!(rendered.contains("Broken\n 1. bad.md"));
        assert!(rendered.contains("Other work\n 1. plan.md"));
    }

    #[test]
    fn status_human_render_sorts_by_score_and_reason_signal() {
        let output = CommandOutput::Status(vec![
            row(&[
                ("section", Value::String("blocked".to_string())),
                ("h", Value::String("metadata.md".to_string())),
                ("score", Value::Number(NumberValue::Int(3))),
                ("why", Value::String("missing_meta".to_string())),
            ]),
            row(&[
                ("section", Value::String("blocked".to_string())),
                ("h", Value::String("dependency.md".to_string())),
                ("score", Value::Number(NumberValue::Int(3))),
                ("why", Value::String("stale_dep".to_string())),
            ]),
            row(&[
                ("section", Value::String("blocked".to_string())),
                ("h", Value::String("broken.md".to_string())),
                ("score", Value::Number(NumberValue::Int(4))),
                ("why", Value::String("broken_ref".to_string())),
            ]),
        ]);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render status");
        let rendered = String::from_utf8(rendered).expect("utf8");

        let broken = rendered.find("broken.md").expect("broken row");
        let dependency = rendered.find("dependency.md").expect("dependency row");
        let metadata = rendered.find("metadata.md").expect("metadata row");
        assert!(
            broken < dependency && dependency < metadata,
            "rendered status should order high scores first, then stronger reasons:\n{rendered}"
        );
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
    fn areas_human_render_groups_health_and_frontier() {
        let output = CommandOutput::rows(
            vec![
                row(&[
                    ("section", Value::String("frontier".to_string())),
                    ("area", Value::String("compiler".to_string())),
                    ("grade", Value::Null),
                    ("files", Value::Null),
                    ("errors", Value::Null),
                    ("cross_edges", Value::Null),
                    ("h", Value::String("compiler/plan.md".to_string())),
                    ("score", Value::Number(NumberValue::Int(7))),
                    ("why", Value::String("broken_ref".to_string())),
                ]),
                row(&[
                    ("section", Value::String("health".to_string())),
                    ("area", Value::String("compiler".to_string())),
                    ("grade", Value::String("C".to_string())),
                    ("files", Value::Number(NumberValue::Int(12))),
                    ("errors", Value::Number(NumberValue::Int(1))),
                    ("cross_edges", Value::Number(NumberValue::Int(4))),
                    ("h", Value::Null),
                    ("score", Value::Null),
                    ("why", Value::Null),
                ]),
            ],
            RowView::Areas,
        );
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render areas");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert!(rendered.starts_with("Areas\nHealth (1)\n"));
        assert!(rendered.contains("compiler  grade=C  files=12  errors=1  cross_edges=4"));
        assert!(
            rendered.contains("Frontier (1)\n 1. compiler  compiler/plan.md  score=7  broken_ref")
        );
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
    fn trend_human_empty_render_is_specific() {
        let output = CommandOutput::rows(Vec::new(), RowView::Trend);
        let mut rendered = Vec::new();

        output
            .write(&mut rendered, OutputMode::Human)
            .expect("render trend");
        let rendered = String::from_utf8(rendered).expect("utf8");

        assert_eq!(rendered, "No trend rows -- snapshot history is empty.\n");
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
        let CommandOutput::Rows { rows, .. } = output else {
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

        let session = RuntimeSession::load(&root).expect("session loads");
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
    fn describe_cards_teach_common_join_patterns() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");

        let session = RuntimeSession::load(&root).expect("session loads");
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
                        && doc.contains("Observed vocabulary recipes")
                })
            }),
            "describe runtime should fold the command map and vocabulary recipes into the teaching card"
        );

        for name in [
            "diagnostic",
            "search",
            "upstream",
            "downstream",
            "top_work",
            "blocked",
            "entropy",
            "undischarged",
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
                })
            }),
            "describe diagnostic should carry the folded recipe and example"
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

        let session = RuntimeSession::load(&root).expect("session loads");
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
    fn save_promotes_eval_query_to_callable_project_verb() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::create_dir(root.join("language")).expect("create area");
        fs::write(root.join("language").join("a.md"), "# A\n").expect("write doc");

        let outcome = run_save(
            &root,
            &SaveCommand {
                name: "by-area".to_string(),
                query: r"? area_of{h: h, area: area}.".to_string(),
                args: vec!["area:String".to_string()],
                doc: "Files in one area.".to_string(),
                force: false,
            },
        )
        .expect("save succeeds");

        assert_eq!(outcome.name, "by-area");
        let project = fs::read_to_string(root.join("anneal.dl")).expect("project rules");
        assert!(project.contains(r#"name: "by-area""#));
        assert!(project.contains(r#"verb_arg(\"area\", area)"#));

        let session = RuntimeSession::load(&root).expect("session reloads");
        let output = session
            .run(RuntimeCommand::Verb {
                name: "by-area".to_string(),
                args: vec!["language".to_string()],
            })
            .expect("saved verb runs");
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("saved verb should emit rows");
        };
        assert!(rows.iter().any(|row| {
            row.fields.get("h")
                == Some(&anneal_core::runtime::Value::String(
                    "language/a.md".to_string(),
                ))
        }));
    }

    #[test]
    fn save_arg_injection_ignores_question_marks_inside_strings() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");

        run_save(
            &root,
            &SaveCommand {
                name: "not-why".to_string(),
                query: r#"? *handle{id: h}, h != "why?"."#.to_string(),
                args: vec!["h:HandleId".to_string()],
                doc: "Reject a literal handle.".to_string(),
                force: false,
            },
        )
        .expect("question mark inside literal does not corrupt injected query");

        let project = fs::read_to_string(root.join("anneal.dl")).expect("project rules");
        assert!(project.contains(r#"verb_arg(\"h\", h)"#));
        assert!(project.contains(r"why?"));
    }

    #[test]
    fn save_requires_force_for_collisions_and_replaces_project_verbs() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");

        run_save(
            &root,
            &SaveCommand {
                name: "mine".to_string(),
                query: r#"? *handle{id: h, kind: "file"}."#.to_string(),
                args: Vec::new(),
                doc: "First version.".to_string(),
                force: false,
            },
        )
        .expect("first save succeeds");

        let err = run_save(
            &root,
            &SaveCommand {
                name: "mine".to_string(),
                query: r"? *handle{id: h}.".to_string(),
                args: Vec::new(),
                doc: "Second version.".to_string(),
                force: false,
            },
        )
        .expect_err("collision without force rejected");
        assert!(err.to_string().contains("use --force"));

        let outcome = run_save(
            &root,
            &SaveCommand {
                name: "mine".to_string(),
                query: r"? *handle{id: h}.".to_string(),
                args: Vec::new(),
                doc: "Second version.".to_string(),
                force: true,
            },
        )
        .expect("force replaces project verb");

        assert!(outcome.replaced.is_some());
        let project = fs::read_to_string(root.join("anneal.dl")).expect("project rules");
        assert_eq!(project.matches(r#"name: "mine""#).count(), 1);
        assert!(project.contains("Second version."));
    }

    #[test]
    fn save_force_can_shadow_prelude_verbs_with_warning_metadata() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("a.md"), "# A\n").expect("write doc");

        let err = run_save(
            &root,
            &SaveCommand {
                name: "work".to_string(),
                query: r"? *handle{id: h}.".to_string(),
                args: Vec::new(),
                doc: "Local work definition.".to_string(),
                force: false,
            },
        )
        .expect_err("prelude collision without force rejected");
        assert!(err.to_string().contains("use --force"));

        let outcome = run_save(
            &root,
            &SaveCommand {
                name: "work".to_string(),
                query: r"? *handle{id: h}.".to_string(),
                args: Vec::new(),
                doc: "Local work definition.".to_string(),
                force: true,
            },
        )
        .expect("force shadows prelude");

        assert!(outcome.shadowed.is_some());
        assert!(
            fs::read_to_string(root.join("anneal.dl"))
                .expect("project rules")
                .contains(r#"name: "work""#)
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

        let session = RuntimeSession::load(&root).expect("session loads");
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

        let session = RuntimeSession::load(&root).expect("session loads");
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

        let Err(err) = RuntimeSession::load(&root) else {
            panic!("legacy TOML should be migration-only");
        };

        assert!(
            err.to_string()
                .contains("anneal.toml is a legacy config file")
        );
        assert!(err.to_string().contains("anneal init --force"));
    }
}
