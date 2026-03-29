use std::collections::HashMap;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};

mod checks;
mod cli;
mod config;
mod graph;
mod handle;
mod impact;
mod lattice;
mod parse;
mod resolve;
mod snapshot;
mod style;

use crate::handle::NodeId;

/// Convergence assistant for knowledge corpora.
#[derive(Parser)]
#[command(
    name = "anneal",
    about = "Convergence assistant for knowledge corpora",
    long_about = "\
Convergence assistant for knowledge corpora.

anneal reads a directory of markdown files, computes a typed knowledge graph,
checks it for local consistency, and tracks convergence over time. It helps
disconnected intelligences (agents across sessions with no shared memory) orient
in a body of knowledge and push it toward settledness.

CORE CONCEPTS:

  Handle    The unit of knowledge. Four kinds:
              file     — a markdown document (e.g., formal-model/v17.md)
              section  — a heading within a file (e.g., v17.md#§definitions)
              label    — a cross-reference tag (e.g., OQ-64, FM-17)
              version  — a versioned artifact (e.g., v17 of formal-model)

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

  Snapshot  A point-in-time capture of graph state, appended to .anneal/history.jsonl.
            Enables convergence tracking (advancing/holding/drifting) and diff.

QUICK START:

  anneal status               Orient: what exists, what's broken, convergence direction
  anneal check                Find broken references, staleness, obligation violations
  anneal get OQ-64            Look up a specific handle
  anneal find FM              Search handles by text
  anneal map --around=OQ-64   Visualize neighborhood of a handle
  anneal impact v17.md        What depends on this file?
  anneal diff                 What changed since last session?
  anneal init                 Generate anneal.toml from inferred structure

CONFIGURATION:

  anneal.toml is optional. Without it, anneal infers structure and runs in
  existence-lattice mode (reference checking only). With it, you get pipeline
  tracking, obligation monitoring, and targeted suggestions.

  Run `anneal init` to generate a config from inferred structure, then tune it.

OUTPUT:

  All commands support --json for machine consumption. Human output is designed
  for terminal use with auto-colored diagnostics (disabled when piped).

  Exit code 1 when errors are found (check command), 0 otherwise."
)]
struct Cli {
    /// Root directory to scan (defaults to .design/ > docs/ > current directory)
    #[arg(long)]
    root: Option<String>,

    /// Output as JSON (all commands). Disables color. Suitable for piping to jq or programmatic consumption.
    #[arg(long, global = true)]
    json: bool,

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
  W001  Stale reference — an active handle references a terminal one
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

Filter flags are mutually exclusive subsets. If none set, all diagnostics shown.

Appends a snapshot to .anneal/history.jsonl for convergence tracking.
Exit code 1 if any errors found, 0 otherwise.",
        after_help = "\
EXAMPLES:
  anneal check                    # All diagnostics
  anneal check --errors-only      # Errors only (for CI/pre-commit hooks)
  anneal check --suggest          # Show only structural suggestions
  anneal check --stale            # Show only staleness warnings (W001)
  anneal check --obligations      # Show only obligation diagnostics (E002/I002)
  anneal check --active-only      # Skip diagnostics from terminal (settled) files
  anneal check --json             # Machine-readable output"
    )]
    Check {
        /// Show only errors (for CI/pre-commit hooks)
        #[arg(long)]
        errors_only: bool,
        /// Show only suggestions (structural improvement hints: S001-S005)
        #[arg(long)]
        suggest: bool,
        /// Show only staleness diagnostics (W001: active referencing terminal)
        #[arg(long)]
        stale: bool,
        /// Show only obligation diagnostics (E002 undischarged, I002 multi-discharge)
        #[arg(long)]
        obligations: bool,
        /// Skip diagnostics sourced from terminal (settled) files
        #[arg(long)]
        active_only: bool,
    },

    /// Look up a handle by identity
    #[command(
        long_about = "\
Resolve a handle identity and show its kind, status, source file, and edges.

Handle identities are strings like:
  OQ-64                    label (namespace OQ, number 64)
  formal-model/v17.md      file path (relative to root)
  v17.md#§definitions      section heading

Use `anneal find` to search if you don't know the exact identity.",
        after_help = "\
EXAMPLES:
  anneal get OQ-64
  anneal get formal-model/v17.md
  anneal get --json OQ-64        # JSON with edges, status, file path"
    )]
    Get {
        /// Handle identity to look up (e.g., OQ-64, formal-model/v17.md)
        handle: String,
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
  anneal find draft --status=draft        # Handles with status 'draft'
  anneal find OQ --namespace=OQ           # Labels in OQ namespace"
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
        /// Filter by handle kind: file, label, section, version
        #[arg(long)]
        kind: Option<String>,
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
directly and transitively. Traverses DependsOn, Supersedes, and Verifies edges
in reverse. Does NOT traverse Cites (citations are not dependencies) or
Discharges (obligation links are not structural dependencies).

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
versions) with edges listed separately. DOT format produces valid graphviz
input — pipe to `dot -Tpng` for visual output.",
        after_help = "\
EXAMPLES:
  anneal map                                    # Full active graph (text)
  anneal map --format=dot | dot -Tpng -o g.png  # Graphviz PNG
  anneal map --around=OQ-64 --depth=2           # 2-hop neighborhood of OQ-64
  anneal map --around=FM-17 --depth=1           # Immediate neighbors only
  anneal map --concern=formal-model             # Concern group subgraph
  anneal map --json                             # JSON with node/edge counts"
    )]
    Map {
        /// Output format: text (default) or dot (graphviz)
        #[arg(long, value_enum, default_value = "text")]
        format: MapFormat,
        /// Show only handles in this concern group (from anneal.toml [concerns])
        #[arg(long)]
        concern: Option<String>,
        /// Show BFS neighborhood around this handle
        #[arg(long)]
        around: Option<String>,
        /// BFS depth for --around (default: 2)
        #[arg(long, default_value = "2")]
        depth: u32,
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

Appends a snapshot to .anneal/history.jsonl. Run status periodically to build
convergence history — the signal becomes meaningful after 2+ snapshots.",
        after_help = "\
EXAMPLES:
  anneal status                   # Human-readable dashboard
  anneal status --verbose         # Expand pipeline to list files per level
  anneal status --json            # Full snapshot as JSON"
    )]
    Status {
        /// Expand pipeline histogram to list files per level
        #[arg(long, short)]
        verbose: bool,
    },

    /// Show what changed since last session
    #[command(
        long_about = "\
Graph-level change tracking. Answers: what changed while I was away?

Three reference modes:
  (default)      Compare against the most recent snapshot in .anneal/history.jsonl
  --days=N       Compare against the snapshot closest to N days ago
  <REF>          Reconstruct graph at a git ref (e.g., HEAD~3) and diff structurally

Shows deltas for handles (created/active/frozen), state transitions, obligations,
edges, and per-namespace statistics. Only non-zero deltas are shown.

Requires at least one prior snapshot (from `anneal status` or `anneal check`).
On first run with no history, prints an informative message.",
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
}

#[derive(Clone, Copy, ValueEnum)]
enum MapFormat {
    Text,
    Dot,
}

/// Collect unresolved pending edges and clone them into an owned vec.
///
/// Returns `(owned_unresolved_edges, section_ref_count)`. Section refs are
/// counted separately for the I001 summary diagnostic.
fn collect_unresolved_owned(
    pending: &[parse::PendingEdge],
    node_index: &HashMap<String, NodeId>,
) -> (Vec<parse::PendingEdge>, usize) {
    let mut unresolved = Vec::new();
    let mut section_ref_count: usize = 0;

    for edge in pending {
        if node_index.contains_key(&edge.target_identity) {
            continue;
        }
        if edge.target_identity.starts_with("section:") {
            section_ref_count += 1;
        } else {
            unresolved.push(edge.clone());
        }
    }

    (unresolved, section_ref_count)
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
    let mut result = parse::build_graph(&root, &config)?;

    let stats = resolve::resolve_all(
        &mut result.graph,
        &result.label_candidates,
        &result.pending_edges,
        &config,
        &root,
        &result.filename_index,
    );

    let lattice = lattice::infer_lattice(
        result.observed_statuses,
        &config,
        &result.terminal_by_directory,
    );
    let graph = &result.graph;
    let root_str = root.to_string();

    // Build node index for get/find/impact commands
    let node_index = resolve::build_node_index(graph);

    match cli_args.command {
        None => {
            // Bare `anneal` (no subcommand): show graph summary
            let summary = cli::build_summary(&root_str, graph, &stats, &lattice);
            if cli_args.json {
                cli::print_json(&summary)?;
            } else {
                summary
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write summary")?;
            }
        }

        Some(Command::Check {
            errors_only,
            suggest,
            stale,
            obligations,
            active_only,
        }) => {
            let (unresolved_owned, section_ref_count) =
                collect_unresolved_owned(&result.pending_edges, &node_index);

            // Compute diagnostics once — used for both check output and snapshot
            let all_diagnostics = checks::run_checks(
                graph,
                &lattice,
                &config,
                &unresolved_owned,
                section_ref_count,
            );
            let snap = snapshot::build_snapshot(graph, &lattice, &config, &all_diagnostics);
            let terminal_files = cli::terminal_file_set(graph, &lattice);

            let filters = cli::CheckFilters {
                errors_only,
                suggest,
                stale,
                obligations,
                active_only,
            };
            let output = cli::cmd_check(all_diagnostics, &filters, &terminal_files);
            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write check output")?;
            }

            // Append snapshot after output (D-04, D-20)
            snapshot::append_snapshot(&root, &snap)?;

            if output.errors > 0 {
                std::process::exit(1);
            }
        }

        Some(Command::Get { ref handle }) => {
            if let Some(output) = cli::cmd_get(graph, &node_index, handle) {
                if cli_args.json {
                    cli::print_json(&output)?;
                } else {
                    output
                        .print_human(&mut std::io::stdout().lock())
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
                },
            );
            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write find output")?;
            }
        }

        Some(Command::Init { dry_run }) => {
            let output = cli::cmd_init(
                &root,
                &lattice,
                &stats,
                &result.observed_frontmatter_keys,
                dry_run,
            )?;
            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write init output")?;
            }
        }

        Some(Command::Impact { ref handle }) => {
            if let Some(output) = cli::cmd_impact(graph, &node_index, handle) {
                if cli_args.json {
                    cli::print_json(&output)?;
                } else {
                    output
                        .print_human(&mut std::io::stdout().lock())
                        .context("failed to write impact output")?;
                }
            } else {
                eprintln!("handle not found: {handle}");
                std::process::exit(1);
            }
        }

        Some(Command::Map {
            format,
            ref concern,
            ref around,
            depth,
        }) => {
            let output = cli::cmd_map(&cli::MapOptions {
                graph,
                node_index: &node_index,
                lattice: &lattice,
                config: &config,
                concern: concern.as_deref(),
                around: around.as_deref(),
                depth,
                format,
            });
            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write map output")?;
            }
        }

        Some(Command::Status { verbose }) => {
            let (unresolved_owned, section_ref_count) =
                collect_unresolved_owned(&result.pending_edges, &node_index);

            // Compute diagnostics once — used for status output, snapshot, and convergence
            let all_diagnostics = checks::run_checks(
                graph,
                &lattice,
                &config,
                &unresolved_owned,
                section_ref_count,
            );
            let snap = snapshot::build_snapshot(graph, &lattice, &config, &all_diagnostics);
            let output = cli::cmd_status(graph, &lattice, &snap, &all_diagnostics);

            // Compute convergence from history (D-05, D-06)
            let convergence =
                snapshot::latest_summary(&root, &snap).map(|s| cli::ConvergenceSummaryOutput {
                    signal: s.signal.to_string(),
                    detail: s.detail,
                });
            let output = output.with_convergence(convergence);

            // Append snapshot AFTER computing convergence (D-04)
            snapshot::append_snapshot(&root, &snap)?;

            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human_with_options(
                        &mut std::io::stdout().lock(),
                        verbose,
                        graph,
                        &lattice,
                    )
                    .context("failed to write status output")?;
            }
        }

        Some(Command::Diff { days, ref git_ref }) => {
            let (unresolved_owned, section_ref_count) =
                collect_unresolved_owned(&result.pending_edges, &node_index);
            let all_diagnostics = checks::run_checks(
                graph,
                &lattice,
                &config,
                &unresolved_owned,
                section_ref_count,
            );
            let current_snap = snapshot::build_snapshot(graph, &lattice, &config, &all_diagnostics);

            let output = cli::cmd_diff(&root, &current_snap, days, git_ref.as_deref())?;

            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write diff output")?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

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
