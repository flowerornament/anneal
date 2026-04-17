use std::collections::HashMap;
use std::io::Write;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

mod analysis;
mod area;
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

  anneal areas                Per-area health profiles — the \"what exists here?\" view
  anneal orient --budget=50k  Token-budgeted reading list (agent-oriented)
  anneal garden               Ranked maintenance tasks with fix/context/verify hints
  anneal status               Dashboard: corpus health, pipeline, convergence
  anneal check                Diagnostics: broken refs, staleness, obligations
  anneal get REQ-12           Inspect one handle with bounded context
  anneal find ADR             Search handle identities
  anneal impact spec/v3.md    Reverse dependencies for safe edits (downstream)
  anneal diff                 Change since last snapshot or git ref
  anneal map --around=REQ-12  Neighborhood view around one handle
  anneal obligations          Linear namespace obligation summary
  anneal init                 Generate anneal.toml from inferred structure

PAIRED WORKFLOWS:

  Before → after edit:
    anneal orient --file=X    Upstream reading list (what feeds into X)
    anneal impact X           Downstream review (what breaks if X changes)

  Gardening loop:
    anneal garden             Surface ranked tasks with hints
    anneal orient --area=X    Follow each task's context: hint
    anneal check --area=X     Follow each task's verify: hint

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

    /// Scope output to a single area (directory or concern group name)
    #[arg(long, global = true)]
    area: Option<String>,

    /// Filter to files within the default recent window (configurable via [temporal] recent_days)
    #[arg(long, global = true)]
    recent: bool,

    /// Filter to files dated within the last N days (e.g. --since=14d)
    #[arg(long, global = true, conflicts_with = "recent")]
    since: Option<String>,

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
  anneal check --area=compiler    # Scope to one area (directory or concern)
  anneal check --recent           # Only diagnostics from recently-touched files
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
  anneal find --status=active --kind=file --context
                                          # Active inventory with purpose/note
  anneal find --recent --kind=file --sort=date
                                          # Files touched in the last 7 days
  anneal find --status=current            # Broad but narrowed query
  anneal find --full --all               # Explicitly return everything"
    )]
    Find {
        /// Text to search for in handle identities
        query: Option<String>,
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
        /// Sort order: id (default) or date (most recent first)
        #[arg(long, value_enum)]
        sort: Option<FindSort>,
        /// Include `purpose:`/`note:` frontmatter (or body snippet) with each match
        #[arg(long)]
        context: bool,
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
        /// With --around, walk only outgoing edges (what this handle builds on)
        #[arg(long, requires = "around", conflicts_with = "downstream")]
        upstream: bool,
        /// With --around, walk only incoming edges (what depends on this handle)
        #[arg(long, requires = "around")]
        downstream: bool,
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
  anneal diff --days=7            # Changes since ~7 days ago (session resume)
  anneal diff --days=30           # Coarser session-resume view
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

    /// Show per-area health profiles
    #[command(
        long_about = "\
Show health profiles for each area (top-level directory) in the corpus.

Each area gets a grade (A-D) based on error count, connectivity, and metadata
coverage. The table shows file count, edges per handle (connectivity), cross-area
edges, and a signal summary.

GRADES:
  [A]  Healthy: no errors, adequate connectivity, has active files
  [B]  Attention: no errors, but low connectivity, no active metadata, or orphans
  [C]  Action required: has errors (E001/E002)
  [D]  Degraded: has errors and low connectivity

Areas are auto-detected from the top-level directory structure. Files in the
corpus root are grouped under \"(root)\". When [concerns] is configured in
anneal.toml, concern groups can also act as areas.",
        after_help = "\
EXAMPLES:
  anneal areas                    # Per-area health table
  anneal areas --sort=grade       # Worst areas first
  anneal areas --sort=name        # Alphabetical
  anneal areas --include-terminal # Include terminal-only areas
  anneal areas --json             # Machine-readable output"
    )]
    Areas {
        /// Sort order for the area table
        #[arg(long, value_enum, default_value = "files")]
        sort: cli::AreaSort,
        /// Include areas that contain only terminal files
        #[arg(long)]
        include_terminal: bool,
    },

    /// Surface ranked maintenance tasks (the \"what's degrading?\" view)
    #[command(
        long_about = "\
Surface maintenance tasks ranked by blast radius. Each task includes fix,
context, and verify hints so agents can close the garden → orient → fix → check
loop without human guidance.

CATEGORIES:
  fix     E001 broken refs, E002 undischarged obligations (blast=high)
  tidy    S001 orphaned labels grouped by area (blast=med)
  link    Areas with zero cross-links — structural islands (blast=low)
  stale   Old files with no edges to recent work (blast=low)
  meta    W003 files missing status/frontmatter (blast=low)
  drift   Namespaces leaking across area boundaries (blast=low)

Tasks are ranked by blast: errors first, then orphan density, island size,
stale age × handle density, metadata gaps, and namespace dispersion. Use
--category to filter, --area to scope, --limit to bound the list.",
        after_help = "\
EXAMPLES:
  anneal garden                                # Top 10 maintenance tasks
  anneal garden --area=compiler                # Scope to one area
  anneal garden --category=fix                 # Only correctness blockers
  anneal garden --json                         # Structured output for agents
  anneal garden --limit=25                     # More tasks"
    )]
    Garden {
        /// Filter to one task category
        #[arg(long, value_enum)]
        category: Option<cli::GardenCategory>,
        /// Maximum number of tasks to surface (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Generate a context-budgeted reading list for agents
    #[command(
        long_about = "\
Generate a context-budgeted reading list for agents. Answers: \"I'm about to work
on this area/file — what should I read, within a token budget?\"

Files are ranked by a score combining edge centrality, label density, recency,
and status. Results are grouped in tiers: pinned files first, then area entry
points, then upstream context, then downstream consumers. Tiers fill greedily
until the budget is exhausted.

TIERS:
  pinned       Files listed in [orient] pin — always first
  entry        Files in the target area (or --file upstream walk)
  upstream     Files outside the area that the area depends on
  downstream   Files outside the area that depend on the area

TOKENS are estimated as file size in bytes / 4. This is a soft cap.

Use --file=X to orient around one file's dependency ancestry instead of an area
(the upstream complement to `impact`). Use --paths-only to pipe the reading
list into another tool.",
        after_help = "\
EXAMPLES:
  anneal orient                              # Reading list for the whole corpus
  anneal orient --area=compiler              # Reading list for one area
  anneal orient --area=compiler --budget=30k # Tighter budget
  anneal orient --file=impl-plan.md          # Upstream ancestry of one file
  anneal orient --paths-only                 # Bare paths (for piping)
  anneal orient --json                       # Structured output for agents"
    )]
    Orient {
        /// Token budget (e.g., 50k, 100k)
        #[arg(long)]
        budget: Option<String>,
        /// Emit bare file paths, one per line (for piping to other tools)
        #[arg(long, conflicts_with = "file")]
        paths_only: bool,
        /// Scope to the upstream dependency ancestry of a specific file
        #[arg(long)]
        file: Option<String>,
    },

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

/// Sort order for `find` results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum FindSort {
    /// Sort alphabetically by handle id (default).
    Id,
    /// Sort by file date, most recent first.
    Date,
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

fn parse_since_days(s: &str) -> anyhow::Result<u32> {
    let s = s.trim().to_lowercase();
    let num_str = s.strip_suffix('d').unwrap_or(&s);
    num_str.parse::<u32>().map_err(|_| {
        anyhow::anyhow!("invalid --since value: expected format like '14d' or '14', got '{s}'")
    })
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

    let snippets = cli::SnippetIndex {
        files: &result.file_snippets,
        labels: &result.label_snippets,
    };

    let area_filter = cli_args.area.as_ref().map(|a| area::AreaFilter::new(a));

    let temporal_filter = if cli_args.recent {
        let cutoff = chrono::Local::now().date_naive()
            - chrono::Duration::days(i64::from(config.temporal.recent_days));
        Some(area::TemporalFilter::new(cutoff, graph))
    } else if let Some(ref since) = cli_args.since {
        let days = parse_since_days(since)?;
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(i64::from(days));
        Some(area::TemporalFilter::new(cutoff, graph))
    } else {
        None
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
            if let Some(ref af) = area_filter {
                diagnostics.retain(|d| d.file.as_deref().is_some_and(|f| af.matches_file(f)));
            }
            if let Some(ref tf) = temporal_filter {
                diagnostics.retain(|d| d.file.as_deref().is_some_and(|f| tf.matches_file(f)));
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
            if let Some(data) = cli::cmd_get(graph, &node_index, snippets, handle) {
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
            sort,
            context,
        }) => {
            let output = cli::cmd_find(
                graph,
                &lattice,
                query.as_deref().unwrap_or(""),
                &cli::FindFilters {
                    namespace: namespace.as_deref(),
                    status: status.as_deref(),
                    kind: kind.as_deref(),
                    include_all: all,
                    limit,
                    offset,
                    full,
                    no_facets: no_facets || !cli_args.json,
                    area: area_filter.as_ref(),
                    temporal: temporal_filter.as_ref(),
                    sort_date: sort == Some(FindSort::Date),
                    context: context.then_some(snippets),
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
            if let Some(mut output) = cli::cmd_impact(graph, &node_index, handle, &traverse_set) {
                if let Some(ref af) = area_filter {
                    output.retain_area(af, &node_index, graph);
                }
                if let Some(ref tf) = temporal_filter {
                    output.retain_temporal(tf, &node_index, graph);
                }
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
            upstream,
            downstream,
            depth,
            nodes,
            edges,
            full,
            limit_nodes,
            limit_edges,
        }) => {
            let has_focus = around.is_some()
                || concern.is_some()
                || area_filter.is_some()
                || temporal_filter.is_some();
            let render = match (render, cli_args.json, has_focus) {
                (Some(render), _, _) => render,
                (None, false, true) => MapRender::Text,
                _ => MapRender::Summary,
            };
            if matches!(render, MapRender::Text | MapRender::Dot) && !full && !has_focus {
                anyhow::bail!(
                    "full graph rendering requires --full; use `anneal map --render=text --full` or focus with --around/--concern/--area"
                );
            }
            let direction = if upstream {
                cli::TraversalDirection::Upstream
            } else if downstream {
                cli::TraversalDirection::Downstream
            } else {
                cli::TraversalDirection::Both
            };
            let output = cli::cmd_map(&cli::MapOptions {
                graph,
                node_index: &node_index,
                lattice: &lattice,
                config: &config,
                concern: concern.as_deref(),
                around: around.as_deref(),
                direction,
                area: area_filter.as_ref(),
                temporal: temporal_filter.as_ref(),
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
                mut diagnostics,
            } = analysis::build_analysis_artifacts(&analysis);
            if let Some(ref af) = area_filter {
                diagnostics.retain(|d| d.file.as_deref().is_some_and(|f| af.matches_file(f)));
            }
            if let Some(ref tf) = temporal_filter {
                diagnostics.retain(|d| d.file.as_deref().is_some_and(|f| tf.matches_file(f)));
            }
            let snap = snapshot::build_snapshot(graph, &lattice, &config, &diagnostics);
            let output = cli::cmd_status(
                graph,
                &lattice,
                &snap,
                &diagnostics,
                area_filter.as_ref(),
                temporal_filter.as_ref(),
            );

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

        Some(Command::Areas {
            sort,
            include_terminal,
        }) => {
            let diagnostics = analysis::build_analysis_artifacts(&analysis).diagnostics;
            let output = cli::cmd_areas(
                graph,
                &lattice,
                &diagnostics,
                &config.areas,
                sort,
                include_terminal,
            );
            emit_full_output(
                output,
                cli_args.json,
                json_style,
                |output, w| output.print_human(w),
                "failed to write areas output",
            )?;
        }

        Some(Command::Garden { category, limit }) => {
            let diagnostics = analysis::build_analysis_artifacts(&analysis).diagnostics;
            let areas = area::compute_areas(graph, &lattice, &diagnostics, &config.areas);
            let output = cli::cmd_garden(&cli::GardenOptions {
                graph,
                diagnostics: &diagnostics,
                areas: &areas,
                area_filter: area_filter.as_ref(),
                category,
                limit,
                config: &config,
            });
            emit_output(
                &output,
                cli_args.json,
                json_style,
                |w| output.print_human(w),
                "failed to write garden output",
            )?;
        }

        Some(Command::Orient {
            ref budget,
            paths_only,
            ref file,
        }) => {
            let budget_str = budget.as_deref().unwrap_or(&config.orient.budget);
            let budget_tokens = cli::parse_budget(budget_str)?;
            let diagnostics = analysis::build_analysis_artifacts(&analysis).diagnostics;
            let areas_summary = area::compute_areas(graph, &lattice, &diagnostics, &config.areas);
            let area_health = area_filter
                .as_ref()
                .and_then(|af| areas_summary.iter().find(|a| a.name == af.name()));
            let output = cli::cmd_orient(&cli::OrientOptions {
                graph,
                node_index: &node_index,
                config: &config.orient,
                area: area_filter.as_ref(),
                file: file.as_deref(),
                budget_tokens,
                snippets,
                area_health,
            })?;

            if paths_only {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                output
                    .print_paths_only(&mut lock)
                    .context("failed to write orient paths")?;
            } else {
                emit_output(
                    &output,
                    cli_args.json,
                    json_style,
                    |w| output.print_human(w),
                    "failed to write orient output",
                )?;
            }
        }

        Some(Command::Query { ref command }) => {
            query::run(
                &analysis,
                command,
                cli_args.json,
                json_style,
                area_filter.as_ref(),
                temporal_filter.as_ref(),
                snippets,
            )?;
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
    fn parse_since_days_with_suffix() {
        assert_eq!(parse_since_days("14d").unwrap(), 14);
        assert_eq!(parse_since_days("7d").unwrap(), 7);
        assert_eq!(parse_since_days("0d").unwrap(), 0);
    }

    #[test]
    fn parse_since_days_without_suffix() {
        assert_eq!(parse_since_days("14").unwrap(), 14);
        assert_eq!(parse_since_days("30").unwrap(), 30);
    }

    #[test]
    fn parse_since_days_invalid() {
        assert!(parse_since_days("abc").is_err());
        assert!(parse_since_days("").is_err());
        assert!(parse_since_days("14w").is_err());
    }

    #[test]
    fn cli_parses_recent_flag() {
        let cli =
            Cli::try_parse_from(["anneal", "--recent", "find", "OQ"]).expect("parse --recent");
        assert!(cli.recent);
        assert!(cli.since.is_none());
    }

    #[test]
    fn cli_parses_since_flag() {
        let cli =
            Cli::try_parse_from(["anneal", "--since=14d", "find", "OQ"]).expect("parse --since");
        assert!(!cli.recent);
        assert_eq!(cli.since.as_deref(), Some("14d"));
    }

    #[test]
    fn cli_recent_and_since_conflict() {
        let result = Cli::try_parse_from(["anneal", "--recent", "--since=14d", "find", "OQ"]);
        assert!(result.is_err(), "--recent and --since should conflict");
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
        // At least one status should be inferred from terminal directories
        // (the specific statuses depend on corpus state).

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
