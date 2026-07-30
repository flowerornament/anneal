//! Process entry and orchestration for the anneal CLI.

use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};

use anneal_core::VerbDispatchError;
use anyhow::{Context, Result, bail};

mod command;
mod help;
mod navigation;
mod output;
mod query_guidance;
mod session;

use command::{Invocation, RuntimeCommand, is_routing_only_flag};
use help::HelpTopic;
use output::{render_dynamic_verb_help_with_collision, run_init, write_text};
use session::{RuntimeRegistry, RuntimeSession, drift_refresh_announcement};

/// Returns whether the compatibility entry point should route these arguments to v2.
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

/// Runs the CLI with arguments from the current process.
pub fn main_entry() -> Result<()> {
    run_args(std::env::args_os().collect())
}

/// Parses and executes one complete CLI invocation.
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
    if let RuntimeCommand::HelpName { name } = &invocation.command
        && let Some(root) = invocation.root.implicit_unmarked_root()
    {
        bail!(
            "help for runtime name {name:?} is corpus-scoped because project vocabulary can change its meaning; no marked corpus root was found above {root}. Use `anneal help` or `anneal help top` for root-free command help, run from a marked corpus, or pass `--root <path>`."
        );
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
    if let RuntimeCommand::HelpName { name } = &invocation.command {
        let registry = RuntimeRegistry::load(invocation.root.path())?;
        match registry.resolve(name) {
            Ok(entry) => {
                return write_text(
                    io::stdout().lock(),
                    &render_dynamic_verb_help_with_collision(
                        entry,
                        registry.has_described_name(name),
                    ),
                );
            }
            Err(VerbDispatchError::MissingVerb { .. }) => {
                invocation.command = RuntimeCommand::Describe { name: name.clone() };
            }
            Err(error) => return Err(error.into()),
        }
    } else if let RuntimeCommand::Verb { name, args } = &invocation.command
        && args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        let registry = RuntimeRegistry::load(invocation.root.path())?;
        let entry = match registry.resolve(name) {
            Ok(entry) => entry,
            Err(VerbDispatchError::MissingVerb { .. }) => {
                bail!(
                    "unknown help topic {name:?}; use `anneal help agent` for the agent briefing, `anneal describe runtime` for the command map, or `anneal schema` for callable verbs"
                );
            }
            Err(error) => return Err(error.into()),
        };
        return write_text(
            io::stdout().lock(),
            &render_dynamic_verb_help_with_collision(entry, registry.has_described_name(name)),
        );
    }
    if let Some(message) = drift_refresh_announcement(&invocation.command) {
        eprintln!("{message}");
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

#[cfg(test)]
mod tests;
