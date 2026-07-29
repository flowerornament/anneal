//! CLI invocation grammar, command routing, and root selection.

use std::collections::BTreeMap;
use std::ffi::OsString;

use anneal_core::runtime::Literal;
use anneal_core::runtime::eval::ExplainOptions;
use anneal_core::{
    InferredCorpusRoot, VerbArg, VerbArgKind, VerbEntry, infer_corpus_root, render_verb_arg_facts,
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};

use crate::{DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT};

use super::help::{HelpTopic, retired_command_message, retired_save_message};

#[derive(Debug, PartialEq, Eq)]
/// Parsed process invocation before corpus loading.
pub(super) struct Invocation {
    pub(super) root: RootSelection,
    pub(super) output: OutputPreference,
    pub(super) command: RuntimeCommand,
}

#[derive(Debug, PartialEq, Eq)]
/// Explicit, inferred, or not-yet-discovered corpus root.
pub(super) enum RootSelection {
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

    /// Return the resolved corpus path.
    pub(super) fn path(&self) -> &Utf8Path {
        match self {
            Self::Explicit(root) => root,
            Self::Inferred(root) => root.path(),
            Self::Undiscovered => {
                unreachable!("runtime root must be resolved before loading the corpus")
            }
        }
    }

    /// Return an inferred unmarked root that should receive recovery guidance.
    pub(super) fn implicit_unmarked_root(&self) -> Option<&Utf8Path> {
        match self {
            Self::Inferred(InferredCorpusRoot::Unmarked(root)) => Some(root),
            Self::Explicit(_)
            | Self::Inferred(InferredCorpusRoot::Marked(_))
            | Self::Undiscovered => None,
        }
    }

    /// Explain an inferred marked root when the selected output cannot carry it.
    pub(super) fn diagnostic(&self, mode: OutputMode, output_has_content: bool) -> Option<String> {
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
/// User preference before terminal-aware output resolution.
pub(super) enum OutputPreference {
    #[default]
    Auto,
    Human,
    Json,
}

impl OutputPreference {
    /// Resolve automatic output against terminal presence.
    pub(super) const fn resolve(self, stdout_is_terminal: bool) -> OutputMode {
        match self {
            Self::Auto if stdout_is_terminal => OutputMode::Human,
            Self::Auto => OutputMode::Json,
            Self::Json => OutputMode::JsonExplicit,
            Self::Human => OutputMode::Human,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Concrete renderer selected for this invocation.
pub(super) enum OutputMode {
    Human,
    Json,
    JsonExplicit,
}

impl Invocation {
    /// Parse process arguments without loading a corpus.
    pub(super) fn parse(args: Vec<OsString>) -> Result<Self> {
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
                        .context("--format requires json, ndjson, or text")?
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

    /// Resolve an explicit or inferred root before runtime construction.
    pub(super) fn resolve_root(&mut self) -> Result<()> {
        self.root.resolve()
    }
}

#[derive(Debug, PartialEq, Eq)]
/// Complete command grammar dispatched by the CLI runtime.
pub(super) enum RuntimeCommand {
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
    HelpName {
        name: String,
    },
}

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
            return Ok(Self::HelpName {
                name: topic.clone(),
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

fn parse_output_format(value: &str) -> Result<OutputPreference> {
    match value {
        "json" | "ndjson" => Ok(OutputPreference::Json),
        "text" => Ok(OutputPreference::Human),
        _ => bail!("--format accepts json, ndjson, or text; got {value:?}"),
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
            "--limit" => {
                limit = parse_positive_usize(next_value(&mut iter, "--limit")?, "--limit")?;
            }
            "--include-low-confidence" => include_low_confidence = true,
            value if value.starts_with("--limit=") => {
                limit = parse_positive_usize(value_after_equals(value), "--limit")?;
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
                limit = Some(parse_positive_usize(
                    next_value(&mut iter, "--limit")?,
                    "--limit",
                )?);
            }
            value if value.starts_with("--limit=") => {
                limit = Some(parse_positive_usize(value_after_equals(value), "--limit")?);
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

/// Return whether a flag affects process routing rather than command semantics.
pub(super) fn is_routing_only_flag(arg: &str) -> bool {
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

#[derive(Debug, PartialEq)]
/// Parsed arguments and rendering controls for a project-defined verb.
pub(super) struct DynamicVerbInvocation {
    pub(super) bindings: Vec<(String, Literal)>,
    pub(super) explain: ExplainOptions,
    pub(super) rows: Option<usize>,
    pub(super) help: bool,
}

impl DynamicVerbInvocation {
    /// Parse arguments according to the resolved verb declaration.
    pub(super) fn parse(entry: &VerbEntry, raw_args: &[String]) -> Result<Self> {
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

/// Prepend typed argument facts to a project-defined verb query.
pub(super) fn render_dynamic_verb_query(
    query_source: &str,
    bindings: &[(String, Literal)],
) -> String {
    let mut rendered = render_verb_arg_facts(bindings);
    rendered.push_str(query_source);
    rendered
}
