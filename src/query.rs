use std::{
    collections::{HashMap, HashSet},
    io::Write,
};

use clap::{Args, Subcommand, ValueEnum};
use globset::{Glob, GlobMatcher};
use serde::Serialize;

use crate::analysis::{self, AnalysisContext};
use crate::checks::{Diagnostic, DiagnosticRecord, Severity, confidence_gap_levels};
use crate::cli::{DetailLevel, JsonEnvelope, JsonStyle, OutputMeta};
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{Handle, HandleKind, NodeId, resolved_file};
use crate::identity::suggestion_id;
use crate::lattice::Lattice;
use crate::obligations::{ObligationDisposition, collect_obligation_summaries};

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
    fn matches(self, handle_kind: &HandleKind) -> bool {
        match self {
            Self::File => matches!(handle_kind, HandleKind::File(_)),
            Self::Section => matches!(handle_kind, HandleKind::Section { .. }),
            Self::Label => matches!(handle_kind, HandleKind::Label { .. }),
            Self::Version => matches!(handle_kind, HandleKind::Version { .. }),
            Self::External => matches!(handle_kind, HandleKind::External { .. }),
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
    /// Filter by edge kind (e.g. DependsOn, Cites, Synthesizes)
    #[arg(long)]
    pub(crate) kind: Option<String>,
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
    /// Filter by severity
    #[arg(long, value_enum)]
    pub(crate) severity: Option<Severity>,
    /// Filter by diagnostic code
    #[arg(long)]
    pub(crate) code: Option<String>,
    /// Scope diagnostics to a single file path
    #[arg(long)]
    pub(crate) file: Option<String>,
    /// Filter by exact line number
    #[arg(long)]
    pub(crate) line: Option<u32>,
    /// Convenience alias for --severity error
    #[arg(long)]
    pub(crate) errors_only: bool,
    /// Convenience alias for --code W001
    #[arg(long)]
    pub(crate) stale: bool,
    /// Convenience alias for --code E002|I002
    #[arg(long)]
    pub(crate) obligations: bool,
    /// Convenience alias for --severity suggestion
    #[arg(long)]
    pub(crate) suggest: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ObligationQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
    #[arg(long)]
    pub(crate) namespace: Option<String>,
    #[arg(long)]
    pub(crate) undischarged: bool,
    #[arg(long)]
    pub(crate) discharged: bool,
    #[arg(long)]
    pub(crate) multi_discharged: bool,
    #[arg(long)]
    pub(crate) mooted: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct SuggestionQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
    #[arg(long)]
    pub(crate) code: Option<String>,
}

#[derive(Clone, Copy)]
struct CountFilter {
    min: Option<usize>,
    max: Option<usize>,
    eq: Option<usize>,
}

#[derive(Clone, Copy)]
struct HandleCandidate<'a> {
    handle: &'a Handle,
    terminal: bool,
    incoming_count: usize,
    outgoing_count: usize,
}

#[derive(Clone)]
struct EdgeCandidate {
    source: NodeId,
    target: NodeId,
    kind: EdgeKind,
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
pub(crate) struct ObligationRow {
    pub(crate) handle: String,
    pub(crate) namespace: String,
    pub(crate) disposition: ObligationDisposition,
    pub(crate) discharge_count: usize,
    pub(crate) file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SuggestionRow {
    pub(crate) suggestion_id: String,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

impl SuggestionRow {
    fn from_diagnostic(diagnostic: &Diagnostic) -> Option<Self> {
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
struct QueryPayload<T: Serialize> {
    kind: &'static str,
    items: Vec<T>,
}

type HandleQueryOutput = JsonEnvelope<QueryPayload<HandleRow>>;
type EdgeQueryOutput = JsonEnvelope<QueryPayload<EdgeRow>>;
type DiagnosticQueryOutput = JsonEnvelope<QueryPayload<DiagnosticRecord>>;
type ObligationQueryOutput = JsonEnvelope<QueryPayload<ObligationRow>>;
type SuggestionQueryOutput = JsonEnvelope<QueryPayload<SuggestionRow>>;

pub(crate) fn run(
    context: &AnalysisContext<'_>,
    command: &QueryCommand,
    json: bool,
    json_style: JsonStyle,
) -> anyhow::Result<()> {
    match command {
        QueryCommand::Handles(args) => {
            let output = build_handle_output(context.graph, context.lattice, args)?;
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
            let output =
                build_edge_output(context.graph, context.lattice, context.node_index, args);
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_edge_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
        QueryCommand::Diagnostics(args) => {
            let output = build_diagnostic_output(context, args);
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_diagnostic_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
        QueryCommand::Obligations(args) => {
            let output = build_obligation_output(context, args);
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_obligation_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
        QueryCommand::Suggestions(args) => {
            let output = build_suggestion_output(context, args);
            if json {
                crate::cli::print_json(&output, json_style)?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_suggestion_output_human(&output, &mut lock)?;
            }
            Ok(())
        }
    }
}

fn build_handle_output(
    graph: &DiGraph,
    lattice: &Lattice,
    args: &HandleQueryArgs,
) -> anyhow::Result<HandleQueryOutput> {
    let file_matcher = compile_glob(args.file_pattern.as_deref())?;
    let mut candidates = build_handle_candidates(graph, lattice, args.page.scope);
    candidates
        .retain(|candidate| matches_handle_filters(graph, candidate, args, file_matcher.as_ref()));
    candidates.sort_by(|a, b| a.handle.id.cmp(&b.handle.id));
    let (meta, items) = paginate(candidates, &args.page);
    Ok(JsonEnvelope::new(
        meta,
        QueryPayload {
            kind: "handles",
            items: items
                .into_iter()
                .map(|candidate| handle_row(graph, candidate))
                .collect(),
        },
    ))
}

fn build_edge_output(
    graph: &DiGraph,
    lattice: &Lattice,
    node_index: &HashMap<String, NodeId>,
    args: &EdgeQueryArgs,
) -> EdgeQueryOutput {
    let state_levels = args.confidence_gap.then(|| build_state_levels(lattice));
    let mut candidates = build_edge_candidates(
        graph,
        lattice,
        node_index,
        args.page.scope,
        args,
        state_levels.as_ref(),
    );
    candidates.sort_by(|a, b| compare_edge_candidates(graph, a, b));
    let (meta, items) = paginate(candidates, &args.page);
    JsonEnvelope::new(
        meta,
        QueryPayload {
            kind: "edges",
            items: items
                .into_iter()
                .map(|candidate| edge_row(graph, &candidate))
                .collect(),
        },
    )
}

fn build_diagnostic_output(
    context: &AnalysisContext<'_>,
    args: &DiagnosticQueryArgs,
) -> DiagnosticQueryOutput {
    let mut diagnostics =
        analysis::build_analysis_artifacts_with_selection(context, diagnostic_selection(args))
            .diagnostics;
    if let Some(file_filter) = &args.file {
        analysis::retain_diagnostics_for_file(&mut diagnostics, context.root.as_str(), file_filter);
    }

    let terminal_files = matches!(args.page.scope, QueryScope::Active)
        .then(|| crate::cli::terminal_file_set(context.graph, context.lattice))
        .unwrap_or_default();
    build_diagnostic_query_output(diagnostics, &terminal_files, args)
}

fn build_obligation_output(
    context: &AnalysisContext<'_>,
    args: &ObligationQueryArgs,
) -> ObligationQueryOutput {
    let mut rows = build_obligation_rows(
        context.graph,
        context.lattice,
        context.config,
        args.page.scope,
    );
    let selected = selected_obligation_dispositions(args);
    rows.retain(|row| matches_obligation_filters(row, args, selected.as_deref()));
    rows.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| a.handle.cmp(&b.handle))
    });
    let (meta, items) = paginate(rows, &args.page);
    JsonEnvelope::new(
        meta,
        QueryPayload {
            kind: "obligations",
            items,
        },
    )
}

fn build_suggestion_output(
    context: &AnalysisContext<'_>,
    args: &SuggestionQueryArgs,
) -> SuggestionQueryOutput {
    let terminal_files = crate::cli::terminal_file_set(context.graph, context.lattice);
    let diagnostics = analysis::build_analysis_artifacts_with_selection(
        context,
        suggestion_diagnostic_selection(args.code.as_deref()),
    )
    .diagnostics;
    build_suggestion_query_output(diagnostics, &terminal_files, args)
}

fn diagnostic_selection(args: &DiagnosticQueryArgs) -> crate::checks::DiagnosticSelection {
    let mut selection = crate::checks::DiagnosticSelection::none();
    let mut narrowed = false;

    if let Some(code) = args.code.as_deref() {
        narrowed = true;
        selection.widen_for_code(code);
    }

    if let Some(severity) = args.severity {
        narrowed = true;
        selection.widen_for_severity(severity);
    }

    if args.errors_only {
        narrowed = true;
        selection.widen_for_severity(Severity::Error);
    }
    if args.stale {
        narrowed = true;
        selection.widen_for_stale_alias();
    }
    if args.obligations {
        narrowed = true;
        selection.widen_for_obligation_alias();
    }
    if args.suggest {
        narrowed = true;
        selection.widen_for_severity(Severity::Suggestion);
    }

    if narrowed {
        selection
    } else {
        crate::checks::DiagnosticSelection::all()
    }
}

fn build_diagnostic_query_output(
    diagnostics: Vec<Diagnostic>,
    terminal_files: &HashSet<String>,
    args: &DiagnosticQueryArgs,
) -> DiagnosticQueryOutput {
    let filters = crate::cli::CheckFilters {
        errors_only: args.errors_only,
        suggest: args.suggest,
        stale: args.stale,
        obligations: args.obligations,
        active_only: matches!(args.page.scope, QueryScope::Active),
    };
    let diagnostics = filtered_diagnostics(
        diagnostics,
        terminal_files,
        &filters,
        |diagnostic| matches_diagnostic_filters(diagnostic, args),
        |a, b| diagnostic_sort_key(a).cmp(&diagnostic_sort_key(b)),
    );

    let (meta, items) = paginate(diagnostics, &args.page);
    JsonEnvelope::new(
        meta,
        QueryPayload {
            kind: "diagnostics",
            items: items
                .into_iter()
                .map(|diagnostic| DiagnosticRecord::from_diagnostic(&diagnostic))
                .collect(),
        },
    )
}

fn filtered_diagnostics(
    diagnostics: Vec<Diagnostic>,
    terminal_files: &HashSet<String>,
    filters: &crate::cli::CheckFilters,
    predicate: impl Fn(&Diagnostic) -> bool,
    mut sort: impl FnMut(&Diagnostic, &Diagnostic) -> std::cmp::Ordering,
) -> Vec<Diagnostic> {
    let mut diagnostics = crate::cli::apply_check_filters(diagnostics, filters, terminal_files);
    diagnostics.retain(|diagnostic| predicate(diagnostic));
    diagnostics.sort_by(|left, right| sort(left, right));
    diagnostics
}

pub(crate) fn suggestion_diagnostic_selection(
    code: Option<&str>,
) -> crate::checks::DiagnosticSelection {
    crate::checks::DiagnosticSelection::suggestions_only(code)
}

fn build_suggestion_query_output(
    diagnostics: Vec<Diagnostic>,
    terminal_files: &HashSet<String>,
    args: &SuggestionQueryArgs,
) -> SuggestionQueryOutput {
    let filters = crate::cli::CheckFilters {
        errors_only: false,
        suggest: true,
        stale: false,
        obligations: false,
        active_only: matches!(args.page.scope, QueryScope::Active),
    };
    let suggestions = filtered_diagnostics(
        diagnostics,
        terminal_files,
        &filters,
        |diagnostic| matches_suggestion_filters(diagnostic, args),
        |a, b| {
            a.code
                .cmp(b.code)
                .then_with(|| {
                    a.file
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.file.as_deref().unwrap_or(""))
                })
                .then_with(|| a.message.cmp(&b.message))
        },
    );
    let (meta, items) = paginate(suggestions, &args.page);
    JsonEnvelope::new(
        meta,
        QueryPayload {
            kind: "suggestions",
            items: items
                .into_iter()
                .filter_map(|diagnostic| SuggestionRow::from_diagnostic(&diagnostic))
                .collect(),
        },
    )
}

fn build_obligation_rows(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &crate::config::AnnealConfig,
    scope: QueryScope,
) -> Vec<ObligationRow> {
    collect_obligation_summaries(graph, lattice, config, !matches!(scope, QueryScope::Active))
        .into_iter()
        .map(|entry| ObligationRow {
            handle: entry.handle,
            namespace: entry.namespace,
            disposition: entry.disposition,
            discharge_count: entry.discharge_count,
            file: entry.file,
        })
        .collect()
}

fn build_handle_candidates<'a>(
    graph: &'a DiGraph,
    lattice: &Lattice,
    scope: QueryScope,
) -> Vec<HandleCandidate<'a>> {
    graph
        .nodes()
        .filter_map(|(node_id, handle)| {
            let terminal = handle.is_terminal(lattice);
            if matches!(scope, QueryScope::Active) && terminal {
                return None;
            }
            Some(HandleCandidate {
                handle,
                terminal,
                incoming_count: graph.incoming(node_id).len(),
                outgoing_count: graph.outgoing(node_id).len(),
            })
        })
        .collect()
}

fn build_edge_candidates(
    graph: &DiGraph,
    lattice: &Lattice,
    node_index: &HashMap<String, NodeId>,
    scope: QueryScope,
    args: &EdgeQueryArgs,
    state_levels: Option<&HashMap<&str, usize>>,
) -> Vec<EdgeCandidate> {
    let source_id = exact_node_id(node_index, args.source.as_deref());
    if args.source.is_some() && source_id.is_none() {
        return Vec::new();
    }
    let target_id = exact_node_id(node_index, args.target.as_deref());
    if args.target.is_some() && target_id.is_none() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    if let (Some(source), Some(target)) = (source_id, target_id) {
        let source_handle = graph.node(source);
        let target_handle = graph.node(target);
        if matches!(scope, QueryScope::Active)
            && (source_handle.is_terminal(lattice) || target_handle.is_terminal(lattice))
        {
            return Vec::new();
        }

        let source_edges = graph.outgoing(source).len();
        let target_edges = graph.incoming(target).len();
        if source_edges <= target_edges {
            collect_edge_candidates(
                graph,
                lattice,
                scope,
                args,
                state_levels,
                graph.outgoing(source).iter().map(|edge| EdgeCandidate {
                    source,
                    target: edge.target,
                    kind: edge.kind.clone(),
                }),
                &mut rows,
            );
        } else {
            collect_edge_candidates(
                graph,
                lattice,
                scope,
                args,
                state_levels,
                graph.incoming(target).iter().map(|edge| EdgeCandidate {
                    source: edge.source,
                    target,
                    kind: edge.kind.clone(),
                }),
                &mut rows,
            );
        }
        return rows;
    }

    if let Some(source) = source_id {
        let source_handle = graph.node(source);
        if !matches!(scope, QueryScope::Active) || !source_handle.is_terminal(lattice) {
            collect_edge_candidates(
                graph,
                lattice,
                scope,
                args,
                state_levels,
                graph.outgoing(source).iter().map(|edge| EdgeCandidate {
                    source,
                    target: edge.target,
                    kind: edge.kind.clone(),
                }),
                &mut rows,
            );
        }
        return rows;
    }

    if let Some(target) = target_id {
        let target_handle = graph.node(target);
        if matches!(scope, QueryScope::Active) && target_handle.is_terminal(lattice) {
            return Vec::new();
        }
        collect_edge_candidates(
            graph,
            lattice,
            scope,
            args,
            state_levels,
            graph.incoming(target).iter().map(|edge| EdgeCandidate {
                source: edge.source,
                target,
                kind: edge.kind.clone(),
            }),
            &mut rows,
        );
        return rows;
    }

    for (node_id, source_handle) in graph.nodes() {
        if matches!(scope, QueryScope::Active) && source_handle.is_terminal(lattice) {
            continue;
        }
        collect_edge_candidates(
            graph,
            lattice,
            scope,
            args,
            state_levels,
            graph.outgoing(node_id).iter().map(|edge| EdgeCandidate {
                source: node_id,
                target: edge.target,
                kind: edge.kind.clone(),
            }),
            &mut rows,
        );
    }
    rows
}

fn collect_edge_candidates(
    graph: &DiGraph,
    lattice: &Lattice,
    scope: QueryScope,
    args: &EdgeQueryArgs,
    state_levels: Option<&HashMap<&str, usize>>,
    candidates: impl Iterator<Item = EdgeCandidate>,
    rows: &mut Vec<EdgeCandidate>,
) {
    for candidate in candidates {
        let source_handle = graph.node(candidate.source);
        let target_handle = graph.node(candidate.target);
        if matches!(scope, QueryScope::Active)
            && (source_handle.is_terminal(lattice) || target_handle.is_terminal(lattice))
        {
            continue;
        }
        if matches_edge_filters(graph, lattice, args, &candidate, state_levels) {
            rows.push(candidate);
        }
    }
}

fn handle_row(graph: &DiGraph, candidate: HandleCandidate<'_>) -> HandleRow {
    let namespace = match &candidate.handle.kind {
        HandleKind::Label { prefix, .. } => Some(prefix.clone()),
        _ => None,
    };
    HandleRow {
        id: candidate.handle.id.clone(),
        handle_kind: candidate.handle.kind.as_str().to_string(),
        status: candidate.handle.status.clone(),
        file: resolved_file(candidate.handle, graph),
        namespace,
        terminal: candidate.terminal,
        incoming_count: candidate.incoming_count,
        outgoing_count: candidate.outgoing_count,
        updated: candidate.handle.metadata.updated,
    }
}

fn edge_row(graph: &DiGraph, candidate: &EdgeCandidate) -> EdgeRow {
    let source_handle = graph.node(candidate.source);
    let target_handle = graph.node(candidate.target);
    EdgeRow {
        source: source_handle.id.clone(),
        target: target_handle.id.clone(),
        edge_kind: candidate.kind.as_str().to_string(),
        source_kind: source_handle.kind.as_str().to_string(),
        target_kind: target_handle.kind.as_str().to_string(),
        source_status: source_handle.status.clone(),
        target_status: target_handle.status.clone(),
        source_file: resolved_file(source_handle, graph),
        target_file: resolved_file(target_handle, graph),
    }
}

fn matches_handle_filters(
    graph: &DiGraph,
    candidate: &HandleCandidate<'_>,
    args: &HandleQueryArgs,
    file_matcher: Option<&GlobMatcher>,
) -> bool {
    if args
        .kind
        .is_some_and(|kind| !kind.matches(&candidate.handle.kind))
    {
        return false;
    }
    if args
        .status
        .as_ref()
        .is_some_and(|status| candidate.handle.status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args.namespace.as_ref().is_some_and(|namespace| {
        label_namespace(candidate.handle).is_none_or(|prefix| prefix != namespace.as_str())
    }) {
        return false;
    }
    if args
        .terminal
        .is_some_and(|terminal| candidate.terminal != terminal)
    {
        return false;
    }
    if file_matcher.is_some_and(|matcher| {
        resolved_file(candidate.handle, graph).is_none_or(|path| !matcher.is_match(&path))
    }) {
        return false;
    }
    if !matches_count_filter(
        candidate.incoming_count,
        CountFilter {
            min: args.incoming_min,
            max: args.incoming_max,
            eq: args.incoming_eq,
        },
    ) || !matches_count_filter(
        candidate.outgoing_count,
        CountFilter {
            min: args.outgoing_min,
            max: args.outgoing_max,
            eq: args.outgoing_eq,
        },
    ) {
        return false;
    }
    if args.updated_before.is_some_and(|date| {
        candidate
            .handle
            .metadata
            .updated
            .is_none_or(|updated| updated >= date)
    }) {
        return false;
    }
    if args.updated_after.is_some_and(|date| {
        candidate
            .handle
            .metadata
            .updated
            .is_none_or(|updated| updated <= date)
    }) {
        return false;
    }
    if args.orphaned
        && (matches!(candidate.handle.kind, HandleKind::File(_)) || candidate.incoming_count != 0)
    {
        return false;
    }
    true
}

fn matches_edge_filters(
    graph: &DiGraph,
    lattice: &Lattice,
    args: &EdgeQueryArgs,
    candidate: &EdgeCandidate,
    state_levels: Option<&HashMap<&str, usize>>,
) -> bool {
    let source_handle = graph.node(candidate.source);
    let target_handle = graph.node(candidate.target);
    if args
        .kind
        .as_ref()
        .is_some_and(|k| k != candidate.kind.as_str())
    {
        return false;
    }
    if args
        .source
        .as_ref()
        .is_some_and(|source| source_handle.id != *source)
    {
        return false;
    }
    if args
        .target
        .as_ref()
        .is_some_and(|target| target_handle.id != *target)
    {
        return false;
    }
    if args
        .source_kind
        .is_some_and(|kind| !kind.matches(&source_handle.kind))
    {
        return false;
    }
    if args
        .target_kind
        .is_some_and(|kind| !kind.matches(&target_handle.kind))
    {
        return false;
    }
    if args
        .source_status
        .as_ref()
        .is_some_and(|status| source_handle.status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args
        .target_status
        .as_ref()
        .is_some_and(|status| target_handle.status.as_deref() != Some(status.as_str()))
    {
        return false;
    }
    if args.cross_file && resolved_file(source_handle, graph) == resolved_file(target_handle, graph)
    {
        return false;
    }
    if args.confidence_gap
        && confidence_gap_levels(
            &candidate.kind,
            source_handle
                .status
                .as_deref()
                .and_then(|status| state_level(status, lattice, state_levels)),
            target_handle
                .status
                .as_deref()
                .and_then(|status| state_level(status, lattice, state_levels)),
        )
        .is_none()
    {
        return false;
    }
    true
}

fn compile_glob(pattern: Option<&str>) -> anyhow::Result<Option<GlobMatcher>> {
    let Some(pattern) = pattern else {
        return Ok(None);
    };
    let matcher = Glob::new(pattern)?.compile_matcher();
    Ok(Some(matcher))
}

fn exact_node_id(node_index: &HashMap<String, NodeId>, identity: Option<&str>) -> Option<NodeId> {
    identity.and_then(|identity| node_index.get(identity).copied())
}

fn matches_diagnostic_filters(diagnostic: &Diagnostic, args: &DiagnosticQueryArgs) -> bool {
    if args
        .severity
        .is_some_and(|severity| severity != diagnostic.severity)
    {
        return false;
    }
    if args
        .code
        .as_ref()
        .is_some_and(|code| diagnostic.code != code.as_str())
    {
        return false;
    }
    if args.line.is_some_and(|line| diagnostic.line != Some(line)) {
        return false;
    }
    true
}

fn matches_obligation_filters(
    row: &ObligationRow,
    args: &ObligationQueryArgs,
    selected: Option<&[ObligationDisposition]>,
) -> bool {
    if args
        .namespace
        .as_ref()
        .is_some_and(|namespace| row.namespace != *namespace)
    {
        return false;
    }
    if selected.is_some_and(|selected| !selected.contains(&row.disposition)) {
        return false;
    }
    true
}

fn matches_suggestion_filters(diagnostic: &Diagnostic, args: &SuggestionQueryArgs) -> bool {
    if diagnostic.severity != Severity::Suggestion {
        return false;
    }
    if args
        .code
        .as_ref()
        .is_some_and(|code| diagnostic.code != code.as_str())
    {
        return false;
    }
    true
}

fn diagnostic_sort_key(diagnostic: &Diagnostic) -> (u8, &str, &str, u32, &str) {
    (
        diagnostic.severity as u8,
        diagnostic.code,
        diagnostic.file.as_deref().unwrap_or(""),
        diagnostic.line.unwrap_or(0),
        diagnostic.message.as_str(),
    )
}

fn build_state_levels(lattice: &Lattice) -> HashMap<&str, usize> {
    lattice
        .ordering
        .iter()
        .enumerate()
        .map(|(index, status)| (status.as_str(), index))
        .collect()
}

fn state_level(
    status: &str,
    lattice: &Lattice,
    state_levels: Option<&HashMap<&str, usize>>,
) -> Option<usize> {
    state_levels
        .and_then(|levels| levels.get(status).copied())
        .or_else(|| crate::lattice::state_level(status, lattice))
}

fn label_namespace(handle: &Handle) -> Option<&str> {
    match &handle.kind {
        HandleKind::Label { prefix, .. } => Some(prefix.as_str()),
        _ => None,
    }
}

fn matches_count_filter(value: usize, filter: CountFilter) -> bool {
    if filter.eq.is_some_and(|exact| value != exact) {
        return false;
    }
    if filter.min.is_some_and(|minimum| value < minimum) {
        return false;
    }
    if filter.max.is_some_and(|maximum| value > maximum) {
        return false;
    }
    true
}

fn compare_edge_candidates(
    graph: &DiGraph,
    left: &EdgeCandidate,
    right: &EdgeCandidate,
) -> std::cmp::Ordering {
    let left_source = &graph.node(left.source).id;
    let right_source = &graph.node(right.source).id;
    let left_target = &graph.node(left.target).id;
    let right_target = &graph.node(right.target).id;
    left_source
        .cmp(right_source)
        .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
        .then_with(|| left_target.cmp(right_target))
}

fn selected_obligation_dispositions(
    args: &ObligationQueryArgs,
) -> Option<Vec<ObligationDisposition>> {
    let mut selected = Vec::new();
    if args.undischarged {
        selected.push(ObligationDisposition::Outstanding);
    }
    if args.discharged {
        selected.push(ObligationDisposition::Discharged);
    }
    if args.multi_discharged {
        selected.push(ObligationDisposition::MultiDischarged);
    }
    if args.mooted {
        selected.push(ObligationDisposition::Mooted);
    }
    (!selected.is_empty()).then_some(selected)
}

fn paginate<T>(mut items: Vec<T>, page: &QueryPageArgs) -> (OutputMeta, Vec<T>) {
    let total = items.len();
    let offset = page.offset.min(total);
    let limit = page.limit.unwrap_or(DEFAULT_QUERY_LIMIT);
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
        OutputMeta::new(
            if page.full {
                DetailLevel::Full
            } else {
                DetailLevel::Sample
            },
            truncated,
            Some(returned),
            Some(total),
            expand,
        ),
        paged,
    )
}

fn print_handle_output_human(output: &HandleQueryOutput, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} handles",
        output.meta.returned.unwrap_or(0),
        output.meta.total.unwrap_or(0)
    )?;
    if output.data.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    let kind_width = output
        .data
        .items
        .iter()
        .map(|row| row.handle_kind.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let status_width = output
        .data
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
    for row in &output.data.items {
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
        output.meta.returned.unwrap_or(0),
        output.meta.total.unwrap_or(0)
    )?;
    if output.data.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    let kind_width = output
        .data
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
    for row in &output.data.items {
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

fn print_diagnostic_output_human(
    output: &DiagnosticQueryOutput,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} diagnostics",
        output.meta.returned.unwrap_or(0),
        output.meta.total.unwrap_or(0)
    )?;
    if output.data.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    for row in &output.data.items {
        write!(w, "{}[{}]: {}", row.severity, row.code, row.message)?;
        if let Some(file) = &row.file {
            write!(w, "\n  -> {file}")?;
            if let Some(line) = row.line {
                write!(w, ":{line}")?;
            }
        }
        writeln!(w)?;
    }
    if !output.meta.expand.is_empty() {
        writeln!(w)?;
        writeln!(w, "next       {}", output.meta.expand.join(", "))?;
    }
    Ok(())
}

fn print_obligation_output_human(
    output: &ObligationQueryOutput,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} obligations",
        output.meta.returned.unwrap_or(0),
        output.meta.total.unwrap_or(0)
    )?;
    if output.data.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(
        w,
        "{:<10}  {:<16}  {:>10}  handle",
        "namespace", "disposition", "discharges"
    )?;
    for row in &output.data.items {
        writeln!(
            w,
            "{:<10}  {:<16}  {:>10}  {}",
            row.namespace,
            row.disposition.as_str(),
            row.discharge_count,
            row.handle
        )?;
    }
    if !output.meta.expand.is_empty() {
        writeln!(w)?;
        writeln!(w, "next       {}", output.meta.expand.join(", "))?;
    }
    Ok(())
}

fn print_suggestion_output_human(
    output: &SuggestionQueryOutput,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(
        w,
        "matches    {} of {} suggestions",
        output.meta.returned.unwrap_or(0),
        output.meta.total.unwrap_or(0)
    )?;
    if output.data.items.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    for row in &output.data.items {
        write!(w, "suggestion[{}]: {}", row.code, row.message)?;
        if let Some(file) = &row.file {
            write!(w, "\n  -> {file}")?;
            if let Some(line) = row.line {
                write!(w, ":{line}")?;
            }
        }
        writeln!(w)?;
    }
    if !output.meta.expand.is_empty() {
        writeln!(w)?;
        writeln!(w, "next       {}", output.meta.expand.join(", "))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::checks::{Diagnostic, Severity};
    use crate::config::AnnealConfig;
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

    fn sample_config() -> AnnealConfig {
        let mut config = AnnealConfig::default();
        config.handles.linear = vec!["OQ".to_string()];
        config
    }

    fn diagnostic_args() -> DiagnosticQueryArgs {
        DiagnosticQueryArgs {
            page: QueryPageArgs {
                limit: None,
                offset: 0,
                full: false,
                scope: QueryScope::Active,
            },
            severity: None,
            code: None,
            file: None,
            line: None,
            errors_only: false,
            stale: false,
            obligations: false,
            suggest: false,
        }
    }

    fn sample_diagnostics() -> Vec<Diagnostic> {
        vec![
            Diagnostic {
                severity: Severity::Error,
                code: "E001",
                message: "broken reference".to_string(),
                file: Some("active.md".to_string()),
                line: Some(7),
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Suggestion,
                code: "S001",
                message: "orphaned handle".to_string(),
                file: Some("active.md".to_string()),
                line: Some(9),
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Warning,
                code: "W001",
                message: "stale reference".to_string(),
                file: Some("done.md".to_string()),
                line: Some(3),
                evidence: None,
            },
        ]
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
        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].id, "OQ-1");
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
        assert!(output.data.items.is_empty());
    }

    #[test]
    fn edge_query_confidence_gap_filters_depends_on() {
        let (graph, lattice) = sample_graph();
        let node_index = crate::resolve::build_node_index(&graph);
        let output = build_edge_output(
            &graph,
            &lattice,
            &node_index,
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
        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].edge_kind, "DependsOn");
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
        assert_eq!(meta.returned, Some(2));
        assert_eq!(meta.total, Some(4));
        assert!(meta.truncated);
        assert_eq!(items, vec![2, 3]);
    }

    #[test]
    fn diagnostic_query_active_scope_excludes_terminal_files() {
        let terminal_files = HashSet::from([String::from("done.md")]);
        let output = build_diagnostic_query_output(
            sample_diagnostics(),
            &terminal_files,
            &diagnostic_args(),
        );

        assert_eq!(output.data.items.len(), 2);
        assert!(
            output
                .data
                .items
                .iter()
                .all(|row| row.file.as_deref() != Some("done.md"))
        );
    }

    #[test]
    fn diagnostic_query_filters_by_severity() {
        let terminal_files = HashSet::new();
        let mut args = diagnostic_args();
        args.page.scope = QueryScope::All;
        args.severity = Some(Severity::Error);

        let output = build_diagnostic_query_output(sample_diagnostics(), &terminal_files, &args);

        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].code, "E001");
        assert_eq!(output.data.items[0].severity, "error");
    }

    #[test]
    fn diagnostic_query_suggest_alias_matches_suggestions() {
        let terminal_files = HashSet::new();
        let mut args = diagnostic_args();
        args.page.scope = QueryScope::All;
        args.suggest = true;

        let output = build_diagnostic_query_output(sample_diagnostics(), &terminal_files, &args);

        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].code, "S001");
    }

    #[test]
    fn diagnostic_selection_for_warning_skips_suggestions() {
        let mut args = diagnostic_args();
        args.severity = Some(Severity::Warning);

        let selection = diagnostic_selection(&args);

        assert!(selection.plausibility);
        assert!(selection.staleness);
        assert!(selection.confidence_gap);
        assert!(selection.conventions);
        assert!(!selection.suggestions);
    }

    #[test]
    fn edge_query_exact_target_filters_from_reverse_adjacency() {
        let (graph, lattice) = sample_graph();
        let node_index = crate::resolve::build_node_index(&graph);
        let output = build_edge_output(
            &graph,
            &lattice,
            &node_index,
            &EdgeQueryArgs {
                page: QueryPageArgs {
                    limit: None,
                    offset: 0,
                    full: false,
                    scope: QueryScope::All,
                },
                kind: None,
                source: None,
                target: Some("OQ-1".to_string()),
                source_kind: None,
                target_kind: None,
                source_status: None,
                target_status: None,
                cross_file: false,
                confidence_gap: false,
            },
        );

        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].target, "OQ-1");
        assert_eq!(output.data.items[0].edge_kind, "Cites");
    }

    #[test]
    fn obligation_query_filters_linear_labels() {
        let (graph, lattice) = sample_graph();
        let config = sample_config();
        let mut rows = build_obligation_rows(&graph, &lattice, &config, QueryScope::All);
        let args = ObligationQueryArgs {
            page: QueryPageArgs {
                limit: None,
                offset: 0,
                full: false,
                scope: QueryScope::All,
            },
            namespace: Some("OQ".to_string()),
            undischarged: true,
            discharged: false,
            multi_discharged: false,
            mooted: false,
        };
        let selected = selected_obligation_dispositions(&args);
        rows.retain(|row| matches_obligation_filters(row, &args, selected.as_deref()));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].handle, "OQ-1");
        assert_eq!(rows[0].disposition, ObligationDisposition::Outstanding);
    }

    #[test]
    fn suggestion_query_filters_by_code() {
        let output = build_suggestion_query_output(
            sample_diagnostics(),
            &HashSet::new(),
            &SuggestionQueryArgs {
                page: QueryPageArgs {
                    limit: None,
                    offset: 0,
                    full: false,
                    scope: QueryScope::All,
                },
                code: Some("S001".to_string()),
            },
        );
        assert_eq!(output.data.items.len(), 1);
        assert_eq!(output.data.items[0].code, "S001");
    }
}
