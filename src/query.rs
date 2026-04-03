use std::io::Write;

use anyhow::bail;
use clap::{Args, Subcommand, ValueEnum};
use globset::{Glob, GlobMatcher};
use serde::Serialize;

use crate::checks::{Diagnostic, Severity};
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{Handle, HandleKind};
use crate::identity::{diagnostic_id, suggestion_id};
use crate::lattice::{Lattice, state_level};

const DEFAULT_QUERY_LIMIT: usize = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryScope {
    Active,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum QueryHandleKind {
    File,
    Section,
    Label,
    Version,
    External,
}

impl QueryHandleKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Section => "section",
            Self::Label => "label",
            Self::Version => "version",
            Self::External => "external",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum QueryEdgeKind {
    Cites,
    DependsOn,
    Supersedes,
    Verifies,
    Discharges,
}

impl QueryEdgeKind {
    fn as_edge_kind(self) -> EdgeKind {
        match self {
            Self::Cites => EdgeKind::Cites,
            Self::DependsOn => EdgeKind::DependsOn,
            Self::Supersedes => EdgeKind::Supersedes,
            Self::Verifies => EdgeKind::Verifies,
            Self::Discharges => EdgeKind::Discharges,
        }
    }
}

#[derive(Args, Clone, Debug)]
pub(crate) struct QueryPageArgs {
    /// Maximum rows to return (default: 25 unless --full)
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    /// Skip this many rows after deterministic sorting
    #[arg(long, default_value = "0")]
    pub(crate) offset: usize,
    /// Return the full result set
    #[arg(long)]
    pub(crate) full: bool,
    /// Query scope: active view only or the full visible corpus
    #[arg(long, value_enum, default_value_t = QueryScope::Active)]
    pub(crate) scope: QueryScope,
}

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum QueryCommand {
    Handles(HandleQueryArgs),
    Edges(EdgeQueryArgs),
    Diagnostics(DiagnosticQueryArgs),
    Obligations(ObligationQueryArgs),
    Suggestions(SuggestionQueryArgs),
}

#[derive(Args, Clone, Debug)]
pub(crate) struct HandleQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
    /// Filter by handle kind
    #[arg(long, value_enum)]
    pub(crate) kind: Option<QueryHandleKind>,
    /// Filter by declared status value
    #[arg(long)]
    pub(crate) status: Option<String>,
    /// Filter labels by namespace prefix
    #[arg(long)]
    pub(crate) namespace: Option<String>,
    /// Filter by whether the handle is terminal
    #[arg(long)]
    pub(crate) terminal: Option<bool>,
    /// Filter by file path glob
    #[arg(long)]
    pub(crate) file_pattern: Option<String>,
    /// Minimum incoming edge count
    #[arg(long)]
    pub(crate) incoming_min: Option<usize>,
    /// Maximum incoming edge count
    #[arg(long)]
    pub(crate) incoming_max: Option<usize>,
    /// Exact incoming edge count
    #[arg(long)]
    pub(crate) incoming_eq: Option<usize>,
    /// Minimum outgoing edge count
    #[arg(long)]
    pub(crate) outgoing_min: Option<usize>,
    /// Maximum outgoing edge count
    #[arg(long)]
    pub(crate) outgoing_max: Option<usize>,
    /// Exact outgoing edge count
    #[arg(long)]
    pub(crate) outgoing_eq: Option<usize>,
    /// Filter to handles updated before this ISO date
    #[arg(long)]
    pub(crate) updated_before: Option<chrono::NaiveDate>,
    /// Filter to handles updated after this ISO date
    #[arg(long)]
    pub(crate) updated_after: Option<chrono::NaiveDate>,
    /// Filter to orphaned non-file handles
    #[arg(long)]
    pub(crate) orphaned: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct EdgeQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
    /// Filter by edge kind
    #[arg(long, value_enum)]
    pub(crate) kind: Option<QueryEdgeKind>,
    /// Filter by exact source handle id
    #[arg(long)]
    pub(crate) source: Option<String>,
    /// Filter by exact target handle id
    #[arg(long)]
    pub(crate) target: Option<String>,
    /// Filter by source handle kind
    #[arg(long, value_enum)]
    pub(crate) source_kind: Option<QueryHandleKind>,
    /// Filter by target handle kind
    #[arg(long, value_enum)]
    pub(crate) target_kind: Option<QueryHandleKind>,
    /// Filter by source status
    #[arg(long)]
    pub(crate) source_status: Option<String>,
    /// Filter by target status
    #[arg(long)]
    pub(crate) target_status: Option<String>,
    /// Restrict to edges whose endpoints live in different files
    #[arg(long)]
    pub(crate) cross_file: bool,
    /// Restrict to DependsOn edges whose source outranks the target
    #[arg(long)]
    pub(crate) confidence_gap: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct DiagnosticQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ObligationQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct SuggestionQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HandleRow {
    pub(crate) id: String,
    pub(crate) handle_kind: String,
    pub(crate) status: Option<String>,
    pub(crate) file: Option<String>,
    pub(crate) namespace: Option<String>,
    pub(crate) terminal: bool,
    pub(crate) incoming_count: usize,
    pub(crate) outgoing_count: usize,
    pub(crate) updated: Option<chrono::NaiveDate>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct EdgeRow {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) edge_kind: String,
    pub(crate) source_kind: String,
    pub(crate) target_kind: String,
    pub(crate) source_status: Option<String>,
    pub(crate) target_status: Option<String>,
    pub(crate) source_file: Option<String>,
    pub(crate) target_file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticRow {
    pub(crate) diagnostic_id: String,
    pub(crate) severity: String,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SuggestionRow {
    pub(crate) suggestion_id: String,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

impl DiagnosticRow {
    #[allow(dead_code)]
    pub(crate) fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self {
            diagnostic_id: diagnostic_id(diagnostic),
            severity: severity_name(diagnostic.severity).to_string(),
            code: diagnostic.code,
            message: diagnostic.message.clone(),
            file: diagnostic.file.clone(),
            line: diagnostic.line,
        }
    }
}

impl SuggestionRow {
    #[allow(dead_code)]
    pub(crate) fn from_diagnostic(diagnostic: &Diagnostic) -> Option<Self> {
        suggestion_id(diagnostic).map(|id| Self {
            suggestion_id: id,
            code: diagnostic.code,
            message: diagnostic.message.clone(),
            file: diagnostic.file.clone(),
            line: diagnostic.line,
        })
    }
}

#[derive(Serialize)]
struct QueryMeta {
    schema_version: u32,
    detail: &'static str,
    truncated: bool,
    returned: usize,
    total: usize,
    expand: Vec<String>,
}

#[derive(Serialize)]
struct HandleQueryOutput {
    #[serde(rename = "_meta")]
    meta: QueryMeta,
    kind: &'static str,
    items: Vec<HandleRow>,
}

#[derive(Serialize)]
struct EdgeQueryOutput {
    #[serde(rename = "_meta")]
    meta: QueryMeta,
    kind: &'static str,
    items: Vec<EdgeRow>,
}

pub(crate) fn run(
    graph: &DiGraph,
    lattice: &Lattice,
    command: &QueryCommand,
    json: bool,
    json_style: crate::cli::JsonStyle,
) -> anyhow::Result<()> {
    match command {
        QueryCommand::Handles(args) => {
            let output = build_handle_output(graph, lattice, args)?;
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_handle_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
        QueryCommand::Edges(args) => {
            let output = build_edge_output(graph, lattice, args);
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_edge_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
        QueryCommand::Diagnostics(_) => {
            bail!("anneal query diagnostics is not implemented yet on this branch")
        }
        QueryCommand::Obligations(_) => {
            bail!("anneal query obligations is not implemented yet on this branch")
        }
        QueryCommand::Suggestions(_) => {
            bail!("anneal query suggestions is not implemented yet on this branch")
        }
    }
}

fn build_handle_output(
    graph: &DiGraph,
    lattice: &Lattice,
    args: &HandleQueryArgs,
) -> anyhow::Result<HandleQueryOutput> {
    let file_matcher = compile_glob(args.file_pattern.as_deref())?;
    let mut rows = build_handle_rows(graph, lattice, args.page.scope);
    rows.retain(|row| matches_handle_filters(row, args, file_matcher.as_ref()));
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    let (meta, items) = paginate(rows, &args.page);
    Ok(HandleQueryOutput {
        meta,
        kind: "handles",
        items,
    })
}

fn build_edge_output(graph: &DiGraph, lattice: &Lattice, args: &EdgeQueryArgs) -> EdgeQueryOutput {
    let mut rows = build_edge_rows(graph, lattice, args.page.scope);
    rows.retain(|row| matches_edge_filters(row, lattice, args));
    rows.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.edge_kind.cmp(&b.edge_kind))
            .then_with(|| a.target.cmp(&b.target))
    });
    let (meta, items) = paginate(rows, &args.page);
    EdgeQueryOutput {
        meta,
        kind: "edges",
        items,
    }
}

fn build_handle_rows(graph: &DiGraph, lattice: &Lattice, scope: QueryScope) -> Vec<HandleRow> {
    graph
        .nodes()
        .filter_map(|(node_id, handle)| {
            let terminal = handle.is_terminal(lattice);
            if matches!(scope, QueryScope::Active) && terminal {
                return None;
            }
            Some(handle_row(graph, node_id, handle, terminal))
        })
        .collect()
}

fn build_edge_rows(graph: &DiGraph, lattice: &Lattice, scope: QueryScope) -> Vec<EdgeRow> {
    let mut rows = Vec::new();
    for (node_id, source_handle) in graph.nodes() {
        if matches!(scope, QueryScope::Active) && source_handle.is_terminal(lattice) {
            continue;
        }
        for edge in graph.outgoing(node_id) {
            let target_handle = graph.node(edge.target);
            if matches!(scope, QueryScope::Active) && target_handle.is_terminal(lattice) {
                continue;
            }
            rows.push(EdgeRow {
                source: source_handle.id.clone(),
                target: target_handle.id.clone(),
                edge_kind: edge.kind.as_str().to_string(),
                source_kind: source_handle.kind.as_str().to_string(),
                target_kind: target_handle.kind.as_str().to_string(),
                source_status: source_handle.status.clone(),
                target_status: target_handle.status.clone(),
                source_file: display_file(source_handle),
                target_file: display_file(target_handle),
            });
        }
    }
    rows
}

fn handle_row(
    graph: &DiGraph,
    node_id: crate::handle::NodeId,
    handle: &Handle,
    terminal: bool,
) -> HandleRow {
    let namespace = match &handle.kind {
        HandleKind::Label { prefix, .. } => Some(prefix.clone()),
        _ => None,
    };
    HandleRow {
        id: handle.id.clone(),
        handle_kind: handle.kind.as_str().to_string(),
        status: handle.status.clone(),
        file: display_file(handle),
        namespace,
        terminal,
        incoming_count: graph.incoming(node_id).len(),
        outgoing_count: graph.outgoing(node_id).len(),
        updated: handle.metadata.updated,
    }
}

fn display_file(handle: &Handle) -> Option<String> {
    handle.file_path.as_ref().map(ToString::to_string)
}

fn matches_handle_filters(
    row: &HandleRow,
    args: &HandleQueryArgs,
    file_matcher: Option<&GlobMatcher>,
) -> bool {
    if args
        .kind
        .is_some_and(|kind| row.handle_kind != kind.as_str())
    {
        return false;
    }
    if args
        .status
        .as_ref()
        .is_some_and(|status| row.status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args
        .namespace
        .as_ref()
        .is_some_and(|namespace| row.namespace.as_deref() != Some(namespace.as_str()))
    {
        return false;
    }
    if args
        .terminal
        .is_some_and(|terminal| row.terminal != terminal)
    {
        return false;
    }
    if file_matcher
        .is_some_and(|matcher| row.file.as_ref().is_none_or(|path| !matcher.is_match(path)))
    {
        return false;
    }
    if args
        .incoming_min
        .is_some_and(|minimum| row.incoming_count < minimum)
    {
        return false;
    }
    if args
        .incoming_max
        .is_some_and(|maximum| row.incoming_count > maximum)
    {
        return false;
    }
    if args
        .incoming_eq
        .is_some_and(|exact| row.incoming_count != exact)
    {
        return false;
    }
    if args
        .outgoing_min
        .is_some_and(|minimum| row.outgoing_count < minimum)
    {
        return false;
    }
    if args
        .outgoing_max
        .is_some_and(|maximum| row.outgoing_count > maximum)
    {
        return false;
    }
    if args
        .outgoing_eq
        .is_some_and(|exact| row.outgoing_count != exact)
    {
        return false;
    }
    if args
        .updated_before
        .is_some_and(|date| row.updated.is_none_or(|updated| updated >= date))
    {
        return false;
    }
    if args
        .updated_after
        .is_some_and(|date| row.updated.is_none_or(|updated| updated <= date))
    {
        return false;
    }
    if args.orphaned && !(row.handle_kind != "file" && row.incoming_count == 0) {
        return false;
    }
    true
}

fn matches_edge_filters(row: &EdgeRow, lattice: &Lattice, args: &EdgeQueryArgs) -> bool {
    if args
        .kind
        .is_some_and(|kind| row.edge_kind != kind.as_edge_kind().as_str())
    {
        return false;
    }
    if args
        .source
        .as_ref()
        .is_some_and(|source| row.source != *source)
    {
        return false;
    }
    if args
        .target
        .as_ref()
        .is_some_and(|target| row.target != *target)
    {
        return false;
    }
    if args
        .source_kind
        .is_some_and(|kind| row.source_kind != kind.as_str())
    {
        return false;
    }
    if args
        .target_kind
        .is_some_and(|kind| row.target_kind != kind.as_str())
    {
        return false;
    }
    if args
        .source_status
        .as_ref()
        .is_some_and(|status| row.source_status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args
        .target_status
        .as_ref()
        .is_some_and(|status| row.target_status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args.cross_file && row.source_file == row.target_file {
        return false;
    }
    if args.confidence_gap && !is_confidence_gap(row, lattice) {
        return false;
    }
    true
}

fn is_confidence_gap(row: &EdgeRow, lattice: &Lattice) -> bool {
    if row.edge_kind != EdgeKind::DependsOn.as_str() {
        return false;
    }
    let Some(source_status) = row.source_status.as_deref() else {
        return false;
    };
    let Some(target_status) = row.target_status.as_deref() else {
        return false;
    };
    match (
        state_level(source_status, lattice),
        state_level(target_status, lattice),
    ) {
        (Some(source), Some(target)) => source > target,
        _ => false,
    }
}

fn compile_glob(pattern: Option<&str>) -> anyhow::Result<Option<GlobMatcher>> {
    let Some(pattern) = pattern else {
        return Ok(None);
    };
    let matcher = Glob::new(pattern)?.compile_matcher();
    Ok(Some(matcher))
}

fn paginate<T>(mut items: Vec<T>, page: &QueryPageArgs) -> (QueryMeta, Vec<T>) {
    let total = items.len();
    let offset = page.offset.min(total);
    let limit = page.limit.unwrap_or(DEFAULT_QUERY_LIMIT);
    let detail = if page.full { "full" } else { "sample" };
    let paged = if page.full {
        items.drain(offset..).collect::<Vec<_>>()
    } else {
        items
            .drain(offset..items.len().min(offset + limit))
            .collect::<Vec<_>>()
    };
    let returned = paged.len();
    let truncated = !page.full && offset + returned < total;
    let mut expand = Vec::new();
    if !page.full {
        expand.push(format!(
            "--limit {}",
            limit.saturating_mul(2).max(DEFAULT_QUERY_LIMIT)
        ));
        if offset + returned < total {
            expand.push(format!("--offset {}", offset + returned));
        }
        expand.push("--full".to_string());
    }
    (
        QueryMeta {
            schema_version: 2,
            detail,
            truncated,
            returned,
            total,
            expand,
        },
        paged,
    )
}

fn print_handle_output_human(output: &HandleQueryOutput, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} handles",
        output.meta.returned, output.meta.total
    )?;
    if output.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    let kind_width = output
        .items
        .iter()
        .map(|row| row.handle_kind.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let status_width = output
        .items
        .iter()
        .map(|row| row.status.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(6)
        .max(6);
    writeln!(
        w,
        "{:<kind_width$}  {:<status_width$}  {:>8}  {:>8}  handle",
        "kind",
        "status",
        "incoming",
        "outgoing",
        kind_width = kind_width,
        status_width = status_width,
    )?;
    for row in &output.items {
        writeln!(
            w,
            "{:<kind_width$}  {:<status_width$}  {:>8}  {:>8}  {}",
            row.handle_kind,
            row.status.as_deref().unwrap_or("-"),
            row.incoming_count,
            row.outgoing_count,
            row.id,
            kind_width = kind_width,
            status_width = status_width,
        )?;
    }
    if !output.meta.expand.is_empty() {
        writeln!(w)?;
        writeln!(w, "next       {}", output.meta.expand.join(", "))?;
    }
    Ok(())
}

fn print_edge_output_human(output: &EdgeQueryOutput, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} edges",
        output.meta.returned, output.meta.total
    )?;
    if output.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    let kind_width = output
        .items
        .iter()
        .map(|row| row.edge_kind.len())
        .max()
        .unwrap_or(4)
        .max(4);
    writeln!(
        w,
        "{:<kind_width$}  {:<32}  target",
        "kind",
        "source",
        kind_width = kind_width,
    )?;
    for row in &output.items {
        writeln!(
            w,
            "{:<kind_width$}  {:<32}  {}",
            row.edge_kind,
            row.source,
            row.target,
            kind_width = kind_width,
        )?;
    }
    if !output.meta.expand.is_empty() {
        writeln!(w)?;
        writeln!(w, "next       {}", output.meta.expand.join(", "))?;
    }
    Ok(())
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Suggestion => "suggestion",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DiGraph;
    use crate::handle::Handle;
    use crate::lattice::{Lattice, LatticeKind};

    fn make_lattice(active: &[&str], terminal: &[&str], ordering: &[&str]) -> Lattice {
        Lattice {
            observed_statuses: active
                .iter()
                .chain(terminal.iter())
                .copied()
                .map(String::from)
                .collect(),
            active: active.iter().copied().map(String::from).collect(),
            terminal: terminal.iter().copied().map(String::from).collect(),
            ordering: ordering.iter().copied().map(String::from).collect(),
            kind: LatticeKind::Confidence,
        }
    }

    fn sample_graph() -> (DiGraph, Lattice) {
        let mut graph = DiGraph::new();
        let file = graph.add_node(Handle::test_file("spec/a.md", Some("formal")));
        let synth = graph.add_node(Handle::test_file("synthesis/b.md", Some("provisional")));
        let label = graph.add_node(Handle::test_label("OQ", 1, Some("open")));
        graph.add_edge(file, synth, EdgeKind::DependsOn);
        graph.add_edge(file, label, EdgeKind::Cites);
        (
            graph,
            make_lattice(
                &["formal", "provisional", "open"],
                &["verified"],
                &["provisional", "formal", "verified"],
            ),
        )
    }

    #[test]
    fn handle_query_filters_by_namespace() {
        let (graph, lattice) = sample_graph();
        let output = build_handle_output(
            &graph,
            &lattice,
            &HandleQueryArgs {
                page: QueryPageArgs {
                    limit: None,
                    offset: 0,
                    full: false,
                    scope: QueryScope::All,
                },
                kind: None,
                status: None,
                namespace: Some("OQ".into()),
                terminal: None,
                file_pattern: None,
                incoming_min: None,
                incoming_max: None,
                incoming_eq: None,
                outgoing_min: None,
                outgoing_max: None,
                outgoing_eq: None,
                updated_before: None,
                updated_after: None,
                orphaned: false,
            },
        )
        .expect("output");
        assert_eq!(output.items.len(), 1);
        assert_eq!(output.items[0].id, "OQ-1");
    }

    #[test]
    fn handle_query_orphaned_excludes_files() {
        let (graph, lattice) = sample_graph();
        let output = build_handle_output(
            &graph,
            &lattice,
            &HandleQueryArgs {
                page: QueryPageArgs {
                    limit: None,
                    offset: 0,
                    full: false,
                    scope: QueryScope::All,
                },
                kind: None,
                status: None,
                namespace: None,
                terminal: None,
                file_pattern: None,
                incoming_min: None,
                incoming_max: None,
                incoming_eq: None,
                outgoing_min: None,
                outgoing_max: None,
                outgoing_eq: None,
                updated_before: None,
                updated_after: None,
                orphaned: true,
            },
        )
        .expect("output");
        assert!(output.items.is_empty());
    }

    #[test]
    fn edge_query_confidence_gap_filters_depends_on() {
        let (graph, lattice) = sample_graph();
        let output = build_edge_output(
            &graph,
            &lattice,
            &EdgeQueryArgs {
                page: QueryPageArgs {
                    limit: None,
                    offset: 0,
                    full: false,
                    scope: QueryScope::All,
                },
                kind: None,
                source: None,
                target: None,
                source_kind: None,
                target_kind: None,
                source_status: None,
                target_status: None,
                cross_file: false,
                confidence_gap: true,
            },
        );
        assert_eq!(output.items.len(), 1);
        assert_eq!(output.items[0].edge_kind, "DependsOn");
    }

    #[test]
    fn pagination_sets_meta_and_offset() {
        let rows = vec![1, 2, 3, 4];
        let (meta, items) = paginate(
            rows,
            &QueryPageArgs {
                limit: Some(2),
                offset: 1,
                full: false,
                scope: QueryScope::Active,
            },
        );
        assert_eq!(meta.returned, 2);
        assert_eq!(meta.total, 4);
        assert!(meta.truncated);
        assert_eq!(items, vec![2, 3]);
    }
}
