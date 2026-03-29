use std::collections::HashSet;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::Parser;
use serde::Serialize;

mod checks;
mod config;
mod graph;
mod handle;
mod impact;
mod lattice;
mod parse;
mod resolve;

use crate::graph::DiGraph;
use crate::lattice::{Lattice, LatticeKind};
use crate::resolve::ResolveStats;

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
}

#[derive(Serialize)]
struct GraphSummary<'a> {
    root: &'a str,
    files: usize,
    handles: usize,
    edges: usize,
    namespaces: Vec<String>,
    versions: usize,
    labels_resolved: usize,
    labels_skipped: usize,
    pending_edges_resolved: usize,
    pending_edges_unresolved: usize,
    lattice_kind: LatticeKind,
    observed_statuses: usize,
    active_statuses: usize,
    terminal_statuses: usize,
}

fn sorted_namespace_names(ns: &HashSet<String>) -> Vec<&str> {
    let mut list: Vec<&str> = ns.iter().map(String::as_str).collect();
    list.sort_unstable();
    list
}

fn print_summary(
    root: &str,
    file_count: usize,
    graph: &DiGraph,
    stats: &ResolveStats,
    lattice: &Lattice,
) {
    let ns_display = sorted_namespace_names(&stats.namespaces).join(", ");

    println!("anneal: knowledge graph built");
    println!("  root: {root}");
    println!("  files: {file_count}");
    println!("  handles: {}", graph.node_count());
    println!("  edges: {}", graph.edge_count());
    println!("  namespaces: {} ({ns_display})", stats.namespaces.len());
    println!(
        "  labels resolved: {}, skipped: {}",
        stats.labels_resolved, stats.labels_skipped
    );
    println!("  versions resolved: {}", stats.versions_resolved);
    println!(
        "  pending edges resolved: {}, unresolved: {}",
        stats.pending_edges_resolved, stats.pending_edges_unresolved
    );
    println!("  lattice: {:?}", lattice.kind);

    if lattice.kind == LatticeKind::Confidence {
        println!(
            "  statuses: {} observed ({} active, {} terminal)",
            lattice.observed_statuses.len(),
            lattice.active.len(),
            lattice.terminal.len()
        );
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let cwd = Utf8PathBuf::try_from(
        std::env::current_dir().context("failed to determine current directory")?,
    )
    .context("current directory is not valid UTF-8")?;

    let root = if let Some(ref r) = cli.root {
        Utf8PathBuf::from(r)
    } else {
        parse::infer_root(&cwd)
    };

    let config = config::load_config(root.as_std_path())?;
    let mut result = parse::build_graph(&root, &config)?;

    let file_count = result
        .graph
        .nodes()
        .filter(|(_, h)| matches!(h.kind, handle::HandleKind::File(_)))
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

    if cli.json {
        let output = GraphSummary {
            root: &root_str,
            files: file_count,
            handles: graph.node_count(),
            edges: graph.edge_count(),
            namespaces: sorted_namespace_names(&stats.namespaces)
                .into_iter()
                .map(String::from)
                .collect(),
            versions: stats.versions_resolved,
            labels_resolved: stats.labels_resolved,
            labels_skipped: stats.labels_skipped,
            pending_edges_resolved: stats.pending_edges_resolved,
            pending_edges_unresolved: stats.pending_edges_unresolved,
            lattice_kind: lattice.kind,
            observed_statuses: lattice.observed_statuses.len(),
            active_statuses: lattice.active.len(),
            terminal_statuses: lattice.terminal.len(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("failed to serialize JSON output")?
        );
    } else {
        print_summary(&root_str, file_count, graph, &stats, &lattice);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
