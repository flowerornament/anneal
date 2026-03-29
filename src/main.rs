use std::collections::HashSet;

use camino::Utf8PathBuf;
use clap::Parser;
use serde::Serialize;

mod config;
mod graph;
mod handle;
mod lattice;
mod parse;
mod resolve;

use crate::graph::DiGraph;
use crate::lattice::{Lattice, LatticeKind};
use crate::resolve::ResolveStats;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// JSON-serializable summary of the constructed knowledge graph.
#[derive(Serialize)]
struct GraphSummary {
    root: String,
    files: usize,
    handles: usize,
    edges: usize,
    namespaces: Vec<String>,
    versions: usize,
    labels_resolved: usize,
    labels_skipped: usize,
    pending_edges_resolved: usize,
    pending_edges_unresolved: usize,
    lattice_kind: String,
    observed_statuses: usize,
    active_statuses: usize,
    terminal_statuses: usize,
}

// ---------------------------------------------------------------------------
// Human-readable output
// ---------------------------------------------------------------------------

fn print_summary(
    root: &str,
    file_count: usize,
    graph: &DiGraph,
    stats: &ResolveStats,
    lattice: &Lattice,
) {
    let mut ns_list: Vec<&String> = stats.namespaces.iter().collect();
    ns_list.sort();
    let ns_display = ns_list
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");

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

// ---------------------------------------------------------------------------
// Main pipeline
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 1. Determine root
    let cwd = Utf8PathBuf::try_from(std::env::current_dir()?)?;
    let root = if let Some(ref r) = cli.root {
        Utf8PathBuf::from(r)
    } else {
        parse::infer_root(&cwd)
    };

    // 2. Load config (zero-config is valid)
    let config = config::load_config(root.as_std_path())?;

    // 3. Build raw graph (scan files, parse frontmatter, scan content)
    let (mut graph, candidates, pending) = parse::build_graph(&root, &config)?;

    // Count files before resolution adds label/version nodes
    let file_count = graph
        .nodes()
        .filter(|(_, h)| matches!(h.kind, handle::HandleKind::File(_)))
        .count();

    // 4. Resolve handles (namespace inference, label nodes, version nodes, pending edges)
    let stats = resolve::resolve_all(&mut graph, &candidates, &pending, &config, &root);

    // 5. Infer lattice from observed statuses
    let observed_statuses: HashSet<String> = graph
        .nodes()
        .filter_map(|(_, h)| h.status.clone())
        .collect();
    // For Phase 1, we don't compute terminal_by_directory (requires directory analysis).
    // Pass an empty set -- config overrides still work, and unrecognized statuses default active.
    let terminal_by_directory = HashSet::new();
    let lattice = lattice::infer_lattice(&observed_statuses, &config, &terminal_by_directory);

    // 6. Print results
    let root_str = root.to_string();

    if cli.json {
        let mut ns_list: Vec<String> = stats.namespaces.iter().cloned().collect();
        ns_list.sort();

        let output = GraphSummary {
            root: root_str,
            files: file_count,
            handles: graph.node_count(),
            edges: graph.edge_count(),
            namespaces: ns_list,
            versions: stats.versions_resolved,
            labels_resolved: stats.labels_resolved,
            labels_skipped: stats.labels_skipped,
            pending_edges_resolved: stats.pending_edges_resolved,
            pending_edges_unresolved: stats.pending_edges_unresolved,
            lattice_kind: format!("{:?}", lattice.kind),
            observed_statuses: lattice.observed_statuses.len(),
            active_statuses: lattice.active.len(),
            terminal_statuses: lattice.terminal.len(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_summary(&root_str, file_count, &graph, &stats, &lattice);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        let (mut graph, candidates, pending) =
            parse::build_graph(&root, &config).expect("build_graph");
        let stats = resolve::resolve_all(&mut graph, &candidates, &pending, &config, &root);

        // Phase 1 success criteria from ROADMAP:
        // "~500 handles and ~2000 edges in <100ms"
        assert!(
            graph.node_count() > 100,
            "Expected >100 handles, got {}",
            graph.node_count()
        );
        assert!(
            graph.edge_count() > 100,
            "Expected >100 edges, got {}",
            graph.edge_count()
        );

        // Namespace inference should find real namespaces
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

        // False positives should be rejected
        assert!(!stats.namespaces.contains("SHA"), "SHA should be rejected");
        assert!(!stats.namespaces.contains("AVX"), "AVX should be rejected");
        assert!(!stats.namespaces.contains("GPT"), "GPT should be rejected");
    }
}
