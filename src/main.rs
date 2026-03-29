use std::collections::HashMap;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

mod checks;
mod cli;
mod config;
mod graph;
mod handle;
mod impact;
mod lattice;
mod parse;
mod resolve;

use crate::handle::{HandleKind, NodeId};

/// Convergence assistant for knowledge corpora.
#[derive(Parser)]
#[command(name = "anneal", about = "Convergence assistant for knowledge corpora")]
struct Cli {
    /// Root directory to scan (inferred if not provided).
    #[arg(long)]
    root: Option<String>,

    /// Output as JSON.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run local consistency checks
    Check {
        /// Show only errors (for pre-commit hooks)
        #[arg(long)]
        errors_only: bool,
    },
    /// Resolve a handle and show its content
    Get {
        /// Handle identity to look up
        handle: String,
    },
    /// Search handles by text
    Find {
        /// Text to search for in handle identities
        query: String,
        /// Include terminal (settled) handles in results
        #[arg(long)]
        all: bool,
        /// Filter to handles in this namespace
        #[arg(long)]
        namespace: Option<String>,
        /// Filter to handles with this status
        #[arg(long)]
        status: Option<String>,
    },
    /// Generate anneal.toml from inferred structure
    Init {
        /// Show what would be written without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Show what's affected if a handle changes
    Impact {
        /// Handle identity to analyze
        handle: String,
    },
}

/// Build a lookup index from handle identity strings to `NodeId`s.
fn build_node_index(graph: &graph::DiGraph) -> HashMap<String, NodeId> {
    let mut index = HashMap::with_capacity(graph.node_count());
    for (node_id, h) in graph.nodes() {
        index.insert(h.id.clone(), node_id);
    }
    index
}

/// Collect unresolved pending edges after resolution.
///
/// An edge is unresolved if its target identity does not appear in the
/// node index. Section refs (target starting with "section:") are counted
/// separately for the I001 summary diagnostic.
fn collect_unresolved<'a>(
    pending: &'a [parse::PendingEdge],
    node_index: &HashMap<String, NodeId>,
) -> (Vec<&'a parse::PendingEdge>, usize) {
    let mut unresolved = Vec::new();
    let mut section_ref_count: usize = 0;

    for edge in pending {
        if node_index.contains_key(&edge.target_identity) {
            continue;
        }
        if edge.target_identity.starts_with("section:") {
            section_ref_count += 1;
        } else {
            unresolved.push(edge);
        }
    }

    (unresolved, section_ref_count)
}

fn main() -> anyhow::Result<()> {
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

    let file_count = result
        .graph
        .nodes()
        .filter(|(_, h)| matches!(h.kind, HandleKind::File(_)))
        .count();

    let stats = resolve::resolve_all(
        &mut result.graph,
        &result.label_candidates,
        &result.pending_edges,
        &config,
        &root,
    );

    let lattice = lattice::infer_lattice(
        result.observed_statuses,
        &config,
        &result.terminal_by_directory,
    );
    let graph = &result.graph;
    let root_str = root.to_string();

    // Build node index for get/find/impact commands
    let node_index = build_node_index(graph);

    match cli_args.command {
        None => {
            // Bare `anneal` (no subcommand): show graph summary
            let summary = cli::build_summary(&root_str, file_count, graph, &stats, &lattice);
            if cli_args.json {
                cli::print_json(&summary)?;
            } else {
                summary
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write summary")?;
            }
        }

        Some(Command::Check { errors_only }) => {
            let (unresolved_refs, section_ref_count) =
                collect_unresolved(&result.pending_edges, &node_index);
            // Convert &[&PendingEdge] to owned slice for run_checks
            let unresolved_owned: Vec<parse::PendingEdge> = unresolved_refs
                .iter()
                .map(|e| parse::PendingEdge {
                    source: e.source,
                    target_identity: e.target_identity.clone(),
                    kind: e.kind,
                    inverse: e.inverse,
                })
                .collect();
            let output = cli::cmd_check(
                graph,
                &lattice,
                &config,
                &unresolved_owned,
                section_ref_count,
                errors_only,
            );
            if cli_args.json {
                cli::print_json(&output)?;
            } else {
                output
                    .print_human(&mut std::io::stdout().lock())
                    .context("failed to write check output")?;
            }
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
        }) => {
            let output = cli::cmd_find(
                graph,
                &lattice,
                query,
                namespace.as_deref(),
                status.as_deref(),
                all,
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
