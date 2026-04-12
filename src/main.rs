use std::collections::HashMap;
use std::io::Write;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

mod analysis;
mod checks;
mod cli;
mod config;
mod explain;
mod extraction;
mod graph;
mod handle;
mod identity;
mod impact;
mod lattice;
mod obligations;
mod parse;
mod query;
mod resolve;
mod snapshot;
mod style;

/// Convergence assistant for knowledge corpora.
#[derive(Parser)]
#[command(
    name = "anneal",
    about = "Convergence assistant for knowledge corpora",
    version,
    long_about = "\
Convergence assistant for knowledge corpora.

anneal reads a directory of markdown files, computes a typed knowledge graph,
checks it for local consistency, and tracks convergence over time. It helps
disconnected intelligences (agents across sessions with no shared memory)
orient in a shared body of knowledge and push it toward settledness.

Use it to:
  orient       What exists here? What is active? Where is uncertainty highest?
  inspect      What does this handle mean? What depends on it?
  validate     Which references, obligations, or pipeline states are wrong?
  resume       What changed since the last session?

CORE CONCEPTS:

  Handle    The unit of knowledge. Five kinds:
              file     — a markdown document (e.g., formal-model/v17.md)
              section  — a heading within a file (e.g., v17.md#§definitions)
              label    — a cross-reference tag (e.g., OQ-64, FM-17)
              version  — a versioned artifact (e.g., v17 of formal-model)
              external — an external URL referenced from corpus metadata

  Edge      A typed relationship between handles:
              Cites      — references without dependency (most body-text refs)
              DependsOn  — structural dependency (frontmatter depends-on)
              Supersedes — version chain (v17 supersedes v16)
              Verifies   — formal verification link
              Discharges — obligation fulfillment

  Status    Frontmatter `status:` field. Partitioned into active (in-progress)
            and terminal (settled). Configure in anneal.toml [convergence].

  Lattice   The convergence lattice tracks how handles move from active toward
            terminal. If ordering is configured, anneal shows a pipeline
            histogram (e.g., 10 raw → 3 draft → 6 exploratory → 11 authoritative).
            Knowledge has a degree of settledness, and the corpus is healthier
            when it is converging.

  Snapshot  A point-in-time capture of graph state, appended to local anneal
            history (XDG state by default, repo-local only if configured).
            Enables convergence tracking (advancing/holding/drifting) and diff.

START HERE:

  anneal status               Dashboard: corpus health, pipeline, convergence
  anneal check                Diagnostics: broken refs, staleness, obligations
  anneal get REQ-12           Inspect one handle with bounded context
  anneal find ADR             Search handle identities
  anneal impact spec/v3.md    Reverse dependencies for safe edits
  anneal diff                 Change since last snapshot or git ref
  anneal obligations          Linear namespace obligation summary
  anneal map --around=REQ-12  Neighborhood view around one handle
  anneal init                 Generate anneal.toml from inferred structure

ROOT DIRECTORY:

  anneal auto-detects the corpus root from the working directory:
    1. --root <path>   if given explicitly
    2. .design/        if it exists
    3. docs/           if it exists
    4. .               current directory (fallback)

  anneal.toml (if present) is read from the root.
  Machine-local anneal config (if present) is read from:
    $XDG_CONFIG_HOME/anneal/config.toml
    ~/.config/anneal/config.toml        (fallback)

CONFIGURATION:

  anneal.toml is optional. Without it, anneal infers structure and runs in
  existence-lattice mode (reference checking only). With it, you get pipeline
  tracking, obligation monitoring, concern groups, and targeted suggestions.

  Run `anneal init` to generate a repo config from inferred structure, then
  tune it. Local runtime preferences like history location live in user config.

OUTPUT:

  All commands support --json for machine consumption. Risky commands use
  progressive disclosure: bounded JSON by default, explicit expansion flags for
  more detail, and --full for full dumps when intentionally requested.
  Human output is designed for terminal use with auto-colored diagnostics
  (disabled when piped).

  Exit code 1 when errors are found (check command), 0 otherwise."
)]
struct Cli {
    /// Root directory to scan (defaults to .design/ > docs/ > current directory)
    #[arg(long)]
    root: Option<String>,

    /// Output as JSON (all commands). Disables color. Suitable for piping to jq or programmatic consumption.
    #[arg(long, global = true)]
    json: bool,

    /// Pretty-print JSON output for humans. Only applies with --json.
    #[arg(long, global = true)]
    pretty: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run local consistency checks
    #[command(
        long_about = "\
Run five local consistency checks and five structural suggestions against the
knowledge graph. Produces compiler-style diagnostics with error codes.

CHECK RULES:
  E001  Broken reference — a handle references something that doesn't exist
  E002  Undischarged obligation — a linear handle has no Discharges edge
  W001  Stale dependency — an active handle has a DependsOn edge to a terminal one
  W002  Confidence gap — a handle at a higher pipeline level depends on a lower one
  W003  Missing frontmatter — files without status: field (above threshold)
  I001  Section references — summary of unresolved section cross-references
  I002  Multiple discharges — a linear handle discharged more than once

SUGGESTION RULES (shown with --suggest):
  S001  Orphaned handles — non-file handles with no incoming edges
  S002  Candidate namespaces — recurring label prefixes not yet confirmed
  S003  Pipeline stalls — status levels with no outflow to next level
  S004  Abandoned namespaces — all members terminal or stale
  S005  Concern group candidates — label prefixes co-occurring across files

By default, `anneal check` shows actionable diagnostics from active files and
skips terminal-file noise. Use `--include-terminal` for the full picture.

Use filter flags to narrow the result set. `--file=<path>` scopes output to a
single source file. `--errors-only`, `--stale`, `--obligations`, and `--suggest`
select diagnostic families.

Appends a snapshot to anneal history for convergence tracking.
Exit code 1 if any errors found, 0 otherwise.",
        after_help = "\
EXAMPLES:
  anneal check                    # Actionable diagnostics from active files
  anneal check --include-terminal # Full diagnostics, including terminal files
  anneal check --file=spec/v3.md  # Scope to one file
  anneal check --errors-only      # Errors only (for CI/pre-commit hooks)
  anneal check --suggest          # Show only structural suggestions
  anneal check --stale            # Show only staleness warnings (W001)
  anneal check --obligations      # Show only obligation diagnostics (E002/I002)
  anneal check --active-only      # Explicitly keep active-only filtering
  anneal check --json             # Bounded machine-readable summary
  anneal check --json --diagnostics --limit 50
                                  # Include 50 diagnostics in JSON
  anneal check --json --full      # Full diagnostics and extraction detail"
    )]
    Check {
        /// Show only errors (for CI/pre-commit hooks)
        #[arg(long)]
        errors_only: bool,
        /// Show only suggestions (structural improvement hints: S001-S005)
        #[arg(long)]
        suggest: bool,
        /// Show only staleness diagnostics (W001: active DependsOn to terminal)
        #[arg(long)]
        stale: bool,
        /// Show only obligation diagnostics (E002 undischarged, I002 multi-discharge)
        #[arg(long)]
        obligations: bool,
        /// Skip diagnostics sourced from terminal (settled) files
        #[arg(long, conflicts_with = "include_terminal")]
        active_only: bool,
        /// Include diagnostics sourced from terminal (settled) files
        #[arg(long, conflicts_with = "active_only")]
        include_terminal: bool,
        /// Scope diagnostics to a single file path
        #[arg(long)]
        file: Option<String>,
        /// Include diagnostics collection in JSON output (bounded by --limit)
        #[arg(long)]
        diagnostics: bool,
        /// Include aggregate extraction facts in JSON output
        #[arg(long)]
        extractions_summary: bool,
        /// Include full extraction payloads in JSON output
        #[arg(long)]
        full_extractions: bool,
        /// Include full diagnostics and extraction payloads in JSON output
        #[arg(long)]
        full: bool,
        /// Maximum diagnostics to include in JSON sample mode
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Look up a handle by identity
    #[command(
        long_about = "\
Resolve a handle identity and show its kind, status, source file, snippet, and
edges.

Handle identities are strings like:
  OQ-64                    label (namespace OQ, number 64)
  OQ-064                   zero-padded label alias for OQ-64
  formal-model/v17.md      file path (relative to root)
  v17.md#§definitions      section heading

Use `anneal find` to search if you don't know the exact identity. Use
`anneal impact` if you need reverse dependencies from this handle.",
        after_help = "\
EXAMPLES:
  anneal get OQ-64
  anneal get OQ-064               # Zero-padded labels normalize automatically
  anneal get formal-model/v17.md
  anneal get OQ-64 --context     # Compact agent briefing
  anneal get OQ-64 --refs        # Bounded incoming/outgoing references
  anneal get --json OQ-64        # Summary JSON with counts and samples
  anneal get --json OQ-64 --trace --full
                                 # Full adjacency in JSON"
    )]
    Get {
        /// Handle identity to look up (e.g., OQ-64, formal-model/v17.md)
        handle: String,
        /// Include bounded incoming/outgoing reference lists
        #[arg(long)]
        refs: bool,
        /// Print a compact agent-oriented briefing
        #[arg(long)]
        context: bool,
        /// Include full adjacency / lineage detail
        #[arg(long)]
        trace: bool,
        /// Include full edge lists without sampling
        #[arg(long)]
        full: bool,
        /// Maximum edges per direction in bounded output
        #[arg(long)]
        limit_edges: Option<usize>,
    },

    /// Search handles by text
    #[command(
        long_about = "\
Search handle identities for a substring match. By default, excludes terminal
(settled) handles to focus on active work. Use --all to include everything.

Searches identity strings only, not file content. For label namespaces, search
by prefix (e.g., 'FM' finds FM-1 through FM-25).",
        after_help = "\
EXAMPLES:
  anneal find FM                          # All active FM-* labels
  anneal find FM --all                    # Include terminal FM labels
  anneal find formal --kind=file          # Only file handles
  anneal find github --kind=external      # External URL handles
  anneal find draft --status=draft        # Handles with status 'draft'
  anneal find OQ --namespace=OQ           # Labels in OQ namespace
  anneal find OQ --limit 25               # Bounded result sample
  anneal find \"\" --status=current         # Broad but narrowed query
  anneal find \"\" --full --all             # Explicitly return everything"
    )]
    Find {
        /// Text to search for in handle identities
        query: String,
        /// Include terminal (settled) handles in results
        #[arg(long)]
        all: bool,
        /// Filter to handles in this namespace (label prefix, e.g., OQ)
        #[arg(long)]
        namespace: Option<String>,
        /// Filter to handles with this frontmatter status value
        #[arg(long)]
        status: Option<String>,
        /// Filter by handle kind: file, label, section, version, external
        #[arg(long)]
        kind: Option<String>,
        /// Maximum number of matches to return (default: 25 unless --full)
        #[arg(long)]
        limit: Option<usize>,
        /// Result offset after sorting by handle id
        #[arg(long, default_value = "0")]
        offset: usize,
        /// Return the full match set
        #[arg(long)]
        full: bool,
        /// Skip facet counts in JSON output
        #[arg(long)]
        no_facets: bool,
    },

    /// Generate anneal.toml from inferred structure
    #[command(
        long_about = "\
Analyze the corpus and generate an anneal.toml configuration file.

Infers:
  - Active/terminal status partition from directory conventions
  - Confirmed label namespaces from sequential cardinality (e.g., OQ-1..OQ-69)
  - Frontmatter field-to-edge mappings from observed field names
  - Freshness thresholds (defaults: warn=30d, error=90d)

Does NOT infer (requires domain knowledge):
  - Pipeline ordering (which statuses flow into which)
  - Linear namespaces (which labels are obligations)
  - Concern groups (which namespaces cluster together)

Review the generated file and tune these sections manually.
Machine-local user config (for example, history_dir in XDG config) is separate
and is not generated by `anneal init`.
Use --dry-run to preview without writing.",
        after_help = "\
EXAMPLES:
  anneal init                     # Write anneal.toml
  anneal init --dry-run           # Preview without writing
  anneal init --json              # JSON output of inferred config"
    )]
    Init {
        /// Show what would be written without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Show what's affected if a handle changes
    #[command(
        long_about = "\
Reverse graph traversal from a handle. Shows which other handles depend on it,
directly and transitively. Traverses edge kinds configured in [impact] traverse
in anneal.toml (defaults to DependsOn, Supersedes, Verifies when absent).

Corpora with custom edge kinds for structural relationships (Synthesizes,
Implements, Reconciles) should configure the traversal set for accurate
blast radius. Cites and Discharges are excluded by default.

Use this before editing a key file to understand blast radius.",
        after_help = "\
EXAMPLES:
  anneal impact formal-model/v17.md    # What depends on the formal model?
  anneal impact OQ-64                  # What depends on this open question?
  anneal impact --json v17.md          # JSON with direct/indirect lists"
    )]
    Impact {
        /// Handle identity to analyze (e.g., OQ-64, formal-model/v17.md)
        handle: String,
    },

    /// Render the knowledge graph
    #[command(
        long_about = "\
Render the knowledge graph as text or graphviz DOT.

By default, shows all active (non-terminal) handles grouped by kind and
namespace. Use --around to focus on a specific handle's neighborhood, or
--concern to show a configured concern group.

Text format groups handles by kind (files, labels by namespace, sections,
versions, external URLs) with edges listed separately. DOT format produces
valid graphviz input — pipe to `dot -Tpng` for visual output.",
        after_help = "\
EXAMPLES:
  anneal map                                    # Graph summary
  anneal map --render=text --full               # Full active graph (text)
  anneal map --render=dot --full | dot -Tpng -o g.png
                                                # Graphviz PNG
  anneal map --around=OQ-64 --depth=1           # 1-hop neighborhood of OQ-64
  anneal map --around=FM-17 --depth=1           # Immediate neighbors only
  anneal map --concern=formal-model             # Concern group subgraph
  anneal map --json                             # JSON graph summary
  anneal map --json --nodes --limit-nodes 50    # Structured node sample"
    )]
    Map {
        /// Render mode: summary (default), text, or dot
        #[arg(long, alias = "format", value_enum)]
        render: Option<MapRender>,
        /// Show only handles in this concern group (from anneal.toml [concerns])
        #[arg(long)]
        concern: Option<String>,
        /// Show BFS neighborhood around this handle
        #[arg(long)]
        around: Option<String>,
        /// BFS depth for --around (default: 1)
        #[arg(long, default_value = "1")]
        depth: u32,
        /// Include a structured node list in JSON output
        #[arg(long)]
        nodes: bool,
        /// Include a structured edge list in JSON output
        #[arg(long)]
        edges: bool,
        /// Allow full graph rendering or full node/edge lists
        #[arg(long)]
        full: bool,
        /// Maximum nodes to include in JSON node lists
        #[arg(long)]
        limit_nodes: Option<usize>,
        /// Maximum edges to include in JSON edge lists
        #[arg(long)]
        limit_edges: Option<usize>,
    },

    /// Orientation dashboard for arriving agents
    #[command(
        long_about = "\
Single-screen dashboard answering: what's the state of this knowledge corpus?

Shows:
  corpus        File, handle, and edge counts. Active vs frozen partition.
  pipeline      If ordering is configured, a histogram of handles per status level
                (e.g., 10 raw → 3 draft → 11 authoritative).
  health        Error and warning counts from check rules. Obligation summary
                if linear namespaces are configured.
  convergence   Direction signal from snapshot comparison:
                  advancing — resolution outpacing creation
                  holding   — balanced (or first run, no history)
                  drifting  — creation outpacing resolution
  suggestions   Count by type (S001-S005) with labels.

Appends a snapshot to anneal history. Run status periodically to build
convergence history — the signal becomes meaningful after 2+ snapshots.

Read it as the active shape of the corpus: where work is accumulating, where it
is settling, and whether the whole system is advancing or drifting.

Use `anneal check` for detailed diagnostics and `anneal diff` for
between-sessions change.",
        after_help = "\
EXAMPLES:
  anneal status                   # Human-readable dashboard
  anneal status --verbose         # Expand pipeline to list files per level
  anneal status --json            # Full snapshot as JSON
  anneal status --json --compact  # Compact agent session-start payload"
    )]
    Status {
        /// Expand pipeline histogram to list files per level
        #[arg(long, short)]
        verbose: bool,
        /// Emit the compact JSON orientation payload
        #[arg(long)]
        compact: bool,
    },

    /// Show what changed since last session
    #[command(
        long_about = "\
Graph-level change tracking. Answers: what changed while I was away?

Three reference modes:
  (default)      Compare against the most recent snapshot in local anneal history
  --days=N       Compare against the snapshot closest to N days ago
  <REF>          Reconstruct graph at a git ref (e.g., HEAD~3) and diff structurally

Shows deltas for handles (created/active/frozen), state transitions, obligations,
edges, and per-namespace statistics. Only non-zero deltas are shown.

Requires at least one prior snapshot (from `anneal status` or `anneal check`).
Legacy repo-local history is still read for compatibility when available.
On first run with no history, prints an informative message.

Use this for session resume. Use git refs when you want structural comparison
against repository history instead of local anneal history. It recovers the
delta accumulated while no single intelligence was present to witness it.",
        after_help = "\
EXAMPLES:
  anneal diff                     # Changes since last snapshot
  anneal diff --days=7            # Changes since ~7 days ago
  anneal diff HEAD~3              # Structural diff against 3 commits ago
  anneal diff main                # Structural diff against main branch
  anneal diff --json              # JSON delta output"
    )]
    Diff {
        /// Compare against snapshot from N days ago
        #[arg(long)]
        days: Option<u32>,
        /// Git ref to compare against (e.g., HEAD~3, main, abc123)
        #[arg(value_name = "REF")]
        git_ref: Option<String>,
    },

    /// Show linear namespace obligation status
    #[command(
        long_about = "\
Show outstanding, discharged, and mooted obligations for linear namespaces.

Linear namespaces are configured in anneal.toml [handles] linear = [...].
Each label in a linear namespace is an obligation that must be discharged
exactly once by a Discharges edge from another handle.

STATUS:
  outstanding  No Discharges edge yet (work remains)
  discharged   Has a Discharges edge (obligation fulfilled)
  mooted       Handle has terminal status (no longer relevant)",
        after_help = "\
EXAMPLES:
  anneal obligations              # Human-readable summary
  anneal obligations --json       # Machine-readable output"
    )]
    Obligations,

    /// Query structural facts derived from the current corpus
    #[command(long_about = "\
Run bounded structural queries over anneal's current in-memory graph and
derived analysis facts.

`query` is the ad hoc structural selector. It answers graph-shaped questions
that are too specific for `status`, too broad for `get`, and intentionally out
of scope for `find`, which remains an identity search.

The current surface is typed by domain:
  handles       query handle properties and local graph counts
  edges         query typed graph edges and endpoint properties
  diagnostics   query the same freshly-derived diagnostic set used by check
  obligations   query obligation state
  suggestions   query structural suggestion outputs

All query domains inherit anneal's bounded-output discipline: limits, offsets,
scope controls, and explicit --full expansion.")]
    Query {
        #[command(subcommand)]
        command: query::QueryCommand,
    },

    /// Explain why anneal produced a derived result
    #[command(long_about = "\
Explain why anneal produced a diagnostic, impact set, convergence signal,
obligation state, or suggestion.

`explain` is the provenance-oriented companion to anneal's structural outputs.
It does not search semantically. It justifies a specific derived answer in
terms of handles, edges, statuses, rules, and snapshots.

The current surface is typed by explanation domain:
  diagnostic    explain one diagnostic, primarily by diagnostic_id
  impact        explain why impact included each affected handle
  convergence   explain the current status-style convergence signal
  obligation    explain one obligation's current disposition
  suggestion    explain one suggestion, primarily by suggestion_id")]
    Explain {
        #[command(subcommand)]
        command: explain::ExplainCommand,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MapRender {
    Summary,
    Text,
    Dot,
}

fn emit_output<T: Serialize>(
    output: &T,
    json: bool,
    json_style: cli::JsonStyle,
    render_human: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    human_context: &'static str,
) -> anyhow::Result<()> {
    if json {
        cli::print_json(output, json_style)?;
    } else {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        render_human(&mut lock).context(human_context)?;
    }
    Ok(())
}

fn emit_full_output<T: Serialize>(
    output: T,
    json: bool,
    json_style: cli::JsonStyle,
    render_human: impl FnOnce(&T, &mut dyn Write) -> std::io::Result<()>,
    human_context: &'static str,
) -> anyhow::Result<()> {
    if json {
        cli::print_json(
            &cli::JsonEnvelope::new(cli::OutputMeta::full(), output),
            json_style,
        )?;
    } else {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        render_human(&output, &mut lock).context(human_context)?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        // Silently exit on broken pipe (e.g., `anneal check | head`).
        for cause in e.chain() {
            if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                && io_err.kind() == std::io::ErrorKind::BrokenPipe
            {
                std::process::exit(0);
            }
        }
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli_args = Cli::parse();
    let json_style = if cli_args.pretty {
        cli::JsonStyle::Pretty
    } else {
        cli::JsonStyle::Compact
    };

    let cwd = Utf8PathBuf::try_from(
        std::env::current_dir().context("failed to determine current directory")?,
    )
    .context("current directory is not valid UTF-8")?;

    let root = if let Some(ref r) = cli_args.root {
        Utf8PathBuf::from(r)
    } else {
        parse::infer_root(&cwd)
    };

    let config = config::load_config(root.as_std_path())?;
    let user_config = match config::load_user_config() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("warning: ignoring malformed anneal user config: {err:#}");
            config::UserConfig::default()
        }
    };
    let state_config = config::resolve_state_config(&config, &user_config);
    let mut result = parse::build_graph(&root, &config)?;

    let stats = resolve::resolve_all(
        &mut result.graph,
        &result.label_candidates,
        &result.pending_edges,
        &config,
        &root,
        &result.filename_index,
    );

    // Phase 6: Resolution cascade -- structural transforms on remaining unresolved edges
    let pre_cascade_index = resolve::build_node_index(&result.graph);
    let root_str = root.to_string();
    let cascade_results = resolve::cascade_unresolved(
        &mut result.graph,
        &result.pending_edges,
        &pre_cascade_index,
        &root_str,
    );

    // Build cascade candidates lookup for diagnostic enrichment
    let cascade_candidates: HashMap<String, Vec<String>> = cascade_results
        .iter()
        .filter(|r| !r.candidates.is_empty())
        .map(|r| {
            let target = result.pending_edges[r.edge_index].target_identity.clone();
            (target, r.candidates.clone())
        })
        .collect();

    let lattice = lattice::infer_lattice(
        std::mem::take(&mut result.observed_statuses),
        &config,
        &result.terminal_by_directory,
    );
    let graph = &result.graph;

    // Rebuild node index after cascade may have added edges via root-prefix resolution
    let node_index = resolve::build_node_index(graph);
    let analysis = analysis::AnalysisContext {
        root: &root,
        graph,
        lattice: &lattice,
        config: &config,
        state_config: &state_config,
        result: &result,
        node_index: &node_index,
        cascade_candidates: &cascade_candidates,
    };

    match cli_args.command {
        None => {
            // Bare `anneal` (no subcommand): show graph summary
            let summary = cli::build_summary(&root_str, graph, &stats, &lattice);
            emit_output(
                &summary,
                cli_args.json,
                json_style,
                |w| summary.print_human(w),
                "failed to write summary",
            )?;
        }

        Some(Command::Check {
            errors_only,
            suggest,
            stale,
            obligations,
            active_only,
            include_terminal,
            file,
            diagnostics: diagnostics_flag,
            extractions_summary,
            full_extractions,
            full,
            limit,
        }) => {
            let active_only = if include_terminal {
                false
            } else {
                active_only || config.check.default_filter.as_deref() == Some("active-only")
            };

            let mut diagnostics = analysis::build_analysis_artifacts(&analysis).diagnostics;
            if let Some(ref file_filter) = file {
                analysis::retain_diagnostics_for_file(&mut diagnostics, &root_str, file_filter);
            }
            let snap = snapshot::build_snapshot(graph, &lattice, &config, &diagnostics);
            let terminal_files = cli::terminal_file_set(graph, &lattice);

            let filters = cli::CheckFilters {
                errors_only,
                suggest,
                stale,
                obligations,
                active_only,
            };
            let output = cli::cmd_check(diagnostics, &filters, &terminal_files);
            if cli_args.json {
                let needs_extractions = extractions_summary || full_extractions || full;
                let filtered_extractions = if needs_extractions {
                    result
                        .extractions
                        .iter()
                        .filter(|extraction| {
                            file.as_ref().is_none_or(|file_filter| {
                                analysis::matches_scoped_file(&extraction.file, file_filter)
                            })
                        })
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                };
                let json_output = cli::build_check_json_output(
                    &output,
                    &filtered_extractions,
                    &cli::CheckJsonOptions {
                        include_diagnostics: diagnostics_flag,
                        diagnostics_limit: limit.unwrap_or(50),
                        include_extractions_summary: extractions_summary,
                        include_full_extractions: full_extractions,
                        full,
                    },
                );
                cli::print_json(&json_output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                output
                    .print_human(&mut lock)
                    .context("failed to write check output")?;
            }

            // Append snapshot after output (D-04, D-20)
            snapshot::append_snapshot(&root, &state_config, &snap)?;

            if output.errors > 0 {
                std::process::exit(1);
            }
        }

        Some(Command::Get {
            ref handle,
            refs,
            context,
            trace,
            full,
            limit_edges,
        }) => {
            if let Some(data) = cli::cmd_get(
                graph,
                &node_index,
                &result.file_snippets,
                &result.label_snippets,
                handle,
            ) {
                let limit_edges = limit_edges.unwrap_or(10);
                if cli_args.json {
                    let output = cli::build_get_json_output(
                        &data,
                        &cli::GetJsonOptions {
                            mode: if context {
                                cli::GetJsonMode::Context
                            } else if trace || full {
                                cli::GetJsonMode::Trace
                            } else if refs {
                                cli::GetJsonMode::Refs
                            } else {
                                cli::GetJsonMode::Summary
                            },
                            limit_edges,
                        },
                    );
                    cli::print_json(&output, json_style)?;
                } else {
                    let output = cli::GetHumanOutput {
                        data,
                        limit_edges,
                        context,
                    };
                    let stdout = std::io::stdout();
                    let mut lock = stdout.lock();
                    output
                        .print_human(&mut lock)
                        .context("failed to write get output")?;
                }
            } else {
                eprintln!("handle not found: {handle}");
                std::process::exit(1);
            }
        }

        Some(Command::Find {
            ref query,
            all,
            ref namespace,
            ref status,
            ref kind,
            limit,
            offset,
            full,
            no_facets,
        }) => {
            let output = cli::cmd_find(
                graph,
                &lattice,
                query,
                &cli::FindFilters {
                    namespace: namespace.as_deref(),
                    status: status.as_deref(),
                    kind: kind.as_deref(),
                    include_all: all,
                    limit,
                    offset,
                    full,
                    no_facets: no_facets || !cli_args.json,
                },
            )?;
            emit_output(
                &output,
                cli_args.json,
                json_style,
                |w| output.print_human(w),
                "failed to write find output",
            )?;
        }

        Some(Command::Init { dry_run }) => {
            let output = cli::cmd_init(
                &root,
                &lattice,
                &stats,
                &result.observed_frontmatter_keys,
                dry_run,
            )?;
            emit_full_output(
                output,
                cli_args.json,
                json_style,
                |output, w| output.print_human(w),
                "failed to write init output",
            )?;
        }

        Some(Command::Impact { ref handle }) => {
            let traverse_set = config.impact.resolve_traverse_set();
            if let Some(output) = cli::cmd_impact(graph, &node_index, handle, &traverse_set) {
                emit_full_output(
                    output,
                    cli_args.json,
                    json_style,
                    |output, w| output.print_human(w),
                    "failed to write impact output",
                )?;
            } else {
                eprintln!("handle not found: {handle}");
                std::process::exit(1);
            }
        }

        Some(Command::Map {
            render,
            ref concern,
            ref around,
            depth,
            nodes,
            edges,
            full,
            limit_nodes,
            limit_edges,
        }) => {
            let render = match (render, cli_args.json, around.is_some() || concern.is_some()) {
                (Some(render), _, _) => render,
                (None, false, true) => MapRender::Text,
                _ => MapRender::Summary,
            };
            if matches!(render, MapRender::Text | MapRender::Dot)
                && !full
                && around.is_none()
                && concern.is_none()
            {
                anyhow::bail!(
                    "full graph rendering requires --full; use `anneal map --render=text --full` or focus with --around/--concern"
                );
            }
            let output = cli::cmd_map(&cli::MapOptions {
                graph,
                node_index: &node_index,
                lattice: &lattice,
                config: &config,
                concern: concern.as_deref(),
                around: around.as_deref(),
                depth,
                render,
                include_nodes: nodes,
                include_edges: edges,
                full,
                limit_nodes: limit_nodes.unwrap_or(100),
                limit_edges: limit_edges.unwrap_or(250),
            });
            emit_output(
                &output,
                cli_args.json,
                json_style,
                |w| output.print_human(w),
                "failed to write map output",
            )?;
        }

        Some(Command::Status { verbose, compact }) => {
            let analysis::AnalysisArtifacts {
                previous_snapshot,
                diagnostics,
            } = analysis::build_analysis_artifacts(&analysis);
            let snap = snapshot::build_snapshot(graph, &lattice, &config, &diagnostics);
            let output = cli::cmd_status(graph, &lattice, &snap, &diagnostics);

            // Compute convergence from history (D-05, D-06)
            let convergence = snapshot::summary_from_previous(&snap, previous_snapshot.as_ref())
                .map(|summary| cli::ConvergenceSummaryOutput {
                    signal: summary.signal.to_string(),
                    detail: summary.detail,
                });
            let output = output.with_convergence(convergence);

            // Append snapshot AFTER computing convergence (D-04)
            snapshot::append_snapshot(&root, &state_config, &snap)?;
            if cli_args.json {
                if compact {
                    let compact_output = output.compact_json();
                    cli::print_json(&compact_output, json_style)?;
                } else {
                    cli::print_json(
                        &cli::JsonEnvelope::new(cli::OutputMeta::full(), output),
                        json_style,
                    )?;
                }
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                output
                    .print_human_with_options(&mut lock, verbose, graph, &lattice)
                    .context("failed to write status output")?;
            }
        }

        Some(Command::Diff { days, ref git_ref }) => {
            let diagnostics = analysis::build_analysis_artifacts(&analysis).diagnostics;
            let current_snap = snapshot::build_snapshot(graph, &lattice, &config, &diagnostics);

            let output = cli::cmd_diff(
                &root,
                &state_config,
                &current_snap,
                days,
                git_ref.as_deref(),
            )?;
            emit_full_output(
                output,
                cli_args.json,
                json_style,
                |output, w| output.print_human(w),
                "failed to write diff output",
            )?;
        }

        Some(Command::Obligations) => {
            let output = cli::cmd_obligations(graph, &lattice, &config);
            emit_full_output(
                output,
                cli_args.json,
                json_style,
                |output, w| output.print_human(w),
                "failed to write obligations output",
            )?;
        }

        Some(Command::Query { ref command }) => {
            query::run(&analysis, command, cli_args.json, json_style)?;
        }

        Some(Command::Explain { ref command }) => {
            explain::run(&analysis, command, cli_args.json, json_style)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn cli_parses_query_scaffolding() {
        let cli = Cli::try_parse_from(["anneal", "query", "handles"]).expect("parse query");
        assert!(matches!(
            cli.command,
            Some(Command::Query {
                command: query::QueryCommand::Handles(_)
            })
        ));
    }

    #[test]
    fn cli_parses_query_diagnostics() {
        let cli = Cli::try_parse_from(["anneal", "query", "diagnostics", "--severity", "warning"])
            .expect("parse query diagnostics");
        assert!(matches!(
            cli.command,
            Some(Command::Query {
                command: query::QueryCommand::Diagnostics(_)
            })
        ));
    }

    #[test]
    fn cli_parses_query_obligations() {
        let cli = Cli::try_parse_from(["anneal", "query", "obligations", "--undischarged"])
            .expect("parse query obligations");
        assert!(matches!(
            cli.command,
            Some(Command::Query {
                command: query::QueryCommand::Obligations(_)
            })
        ));
    }

    #[test]
    fn cli_parses_explain_scaffolding() {
        let cli = Cli::try_parse_from(["anneal", "explain", "convergence"]).expect("parse explain");
        assert!(matches!(
            cli.command,
            Some(Command::Explain {
                command: explain::ExplainCommand::Convergence(_),
            })
        ));
    }

    #[test]
    fn cli_parses_explain_suggestion() {
        let cli = Cli::try_parse_from([
            "anneal",
            "explain",
            "suggestion",
            "S001",
            "--handle",
            "OQ-64",
        ])
        .expect("parse explain suggestion");
        assert!(matches!(
            cli.command,
            Some(Command::Explain {
                command: explain::ExplainCommand::Suggestion(_),
            })
        ));
    }

    #[test]
    fn cli_parses_explain_diagnostic() {
        let cli = Cli::try_parse_from(["anneal", "explain", "diagnostic", "--id", "diag_deadbeef"])
            .expect("parse explain diagnostic");
        assert!(matches!(
            cli.command,
            Some(Command::Explain {
                command: explain::ExplainCommand::Diagnostic(_),
            })
        ));
    }

    #[test]
    fn test_murail_corpus() {
        let root = Utf8PathBuf::from(std::env::var("HOME").expect("HOME not set"))
            .join("code/murail/.design");
        if !root.exists() {
            eprintln!("Murail corpus not found at {root}, skipping");
            return;
        }
        let config = config::load_config(root.as_std_path()).expect("config load");
        let mut result = parse::build_graph(&root, &config).expect("build_graph");

        // D-04: Verify directory convention analysis found terminal statuses
        assert!(
            !result.terminal_by_directory.is_empty(),
            "Expected terminal statuses from directory convention, got empty set"
        );
        // "superseded" appears exclusively in history/ dirs in Murail
        assert!(
            result.terminal_by_directory.contains("superseded"),
            "Expected 'superseded' in terminal_by_directory, got {:?}",
            result.terminal_by_directory
        );

        let stats = resolve::resolve_all(
            &mut result.graph,
            &result.label_candidates,
            &result.pending_edges,
            &config,
            &root,
            &result.filename_index,
        );

        assert!(
            result.graph.node_count() > 100,
            "Expected >100 handles, got {}",
            result.graph.node_count()
        );
        assert!(
            result.graph.edge_count() > 100,
            "Expected >100 edges, got {}",
            result.graph.edge_count()
        );

        assert!(
            stats.namespaces.contains("OQ"),
            "OQ namespace not found in {:?}",
            stats.namespaces
        );
        assert!(
            stats.namespaces.contains("FM"),
            "FM namespace not found in {:?}",
            stats.namespaces
        );

        assert!(!stats.namespaces.contains("SHA"), "SHA should be rejected");
        assert!(!stats.namespaces.contains("AVX"), "AVX should be rejected");
        assert!(!stats.namespaces.contains("GPT"), "GPT should be rejected");

        // D-02: Bare filename resolution + D-03 URL rejection + D-08 code block skip
        // should reduce unresolved count from Phase 1's 3396
        assert!(
            stats.pending_edges_unresolved < 3396,
            "Expected fewer unresolved pending edges than Phase 1 baseline of 3396, got {}",
            stats.pending_edges_unresolved
        );

        // D-04: Verify lattice has terminal statuses from directory convention
        let lattice = lattice::infer_lattice(
            result.observed_statuses.clone(),
            &config,
            &result.terminal_by_directory,
        );
        assert!(
            !lattice.terminal.is_empty(),
            "Expected terminal statuses in lattice from directory convention"
        );
    }
}
