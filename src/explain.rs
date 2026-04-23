use std::io::Write;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::analysis::AnalysisContext;
use crate::checks::{Diagnostic, DiagnosticCode, Evidence, SuggestionEvidence};
use crate::cli::{JsonStyle, OutputMeta};
use crate::handle::HandleKind;
use crate::identity::{diagnostic_id, suggestion_id};
use crate::impact;
use crate::obligations::lookup_obligation;
use crate::output::{Line, OutputStyle, Printer, Render, Tone};
use crate::snapshot;

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ExplainCommand {
    #[command(about = "Explain why a specific diagnostic was produced")]
    Diagnostic(DiagnosticExplainArgs),
    #[command(about = "Explain why impact included each affected handle")]
    Impact(ImpactExplainArgs),
    #[command(about = "Explain the current convergence signal")]
    Convergence(ConvergenceExplainArgs),
    #[command(about = "Explain an obligation's current disposition")]
    Obligation(ObligationExplainArgs),
    #[command(about = "Explain why a structural suggestion was produced")]
    Suggestion(SuggestionExplainArgs),
}

#[derive(Args, Clone, Debug)]
pub(crate) struct DiagnosticExplainArgs {
    #[arg(long)]
    pub(crate) id: Option<String>,
    #[arg(long)]
    pub(crate) code: Option<String>,
    #[arg(long)]
    pub(crate) file: Option<String>,
    #[arg(long)]
    pub(crate) line: Option<u32>,
    #[arg(long)]
    pub(crate) handle: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ImpactExplainArgs {
    pub(crate) handle: String,
    #[arg(long)]
    pub(crate) full: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ConvergenceExplainArgs {}

#[derive(Args, Clone, Debug)]
pub(crate) struct ObligationExplainArgs {
    pub(crate) handle: String,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct SuggestionExplainArgs {
    #[arg(long)]
    pub(crate) id: Option<String>,
    pub(crate) code: Option<String>,
    #[arg(long)]
    pub(crate) handle: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExplanationFact {
    pub(crate) fact_type: String,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticExplanation {
    pub(crate) diagnostic_id: String,
    pub(crate) severity: crate::checks::Severity,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) rule: Option<String>,
    pub(crate) facts: Vec<ExplanationFact>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactExplanation {
    pub(crate) root: String,
    pub(crate) direct: Vec<ImpactPath>,
    pub(crate) indirect: Vec<ImpactPath>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactPath {
    pub(crate) target: String,
    pub(crate) path: Vec<ImpactHop>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactHop {
    pub(crate) source: String,
    pub(crate) edge_kind: String,
    pub(crate) target: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConvergenceExplanation {
    pub(crate) signal: String,
    pub(crate) detail: String,
    pub(crate) current: ConvergenceSnapshotSummary,
    pub(crate) previous: Option<ConvergenceSnapshotSummary>,
    pub(crate) pipeline: PipelineSummary,
    pub(crate) facts: Vec<ExplanationFact>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PipelineSummary {
    pub(crate) active: Vec<String>,
    pub(crate) terminal: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) ordering: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) descriptions: Vec<StatusDescription>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct StatusDescription {
    pub(crate) status: String,
    pub(crate) description: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConvergenceSnapshotSummary {
    pub(crate) handles_total: usize,
    pub(crate) handles_active: usize,
    pub(crate) handles_frozen: usize,
    pub(crate) obligations_outstanding: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ObligationExplanation {
    pub(crate) handle: String,
    pub(crate) namespace: String,
    pub(crate) disposition: String,
    pub(crate) facts: Vec<ExplanationFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remediation: Option<RemediationHint>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<CandidateDischarger>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RemediationHint {
    pub(crate) action: String,
    pub(crate) frontmatter_syntax: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CandidateDischarger {
    pub(crate) handle: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SuggestionExplanation {
    pub(crate) suggestion_id: String,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) rule: Option<String>,
    pub(crate) facts: Vec<ExplanationFact>,
}

pub(crate) fn run(
    context: &AnalysisContext<'_>,
    command: &ExplainCommand,
    json: bool,
    json_style: JsonStyle,
    output_style: OutputStyle,
) -> anyhow::Result<()> {
    match command {
        ExplainCommand::Diagnostic(args) => crate::emit_rendered(
            &build_diagnostic_explanation_output(context, args)?,
            Some(OutputMeta::full()),
            json,
            json_style,
            output_style,
            "failed to write explain diagnostic output",
        ),
        ExplainCommand::Impact(args) => crate::emit_rendered(
            &build_impact_explanation_output(context, args)?,
            Some(OutputMeta::full()),
            json,
            json_style,
            output_style,
            "failed to write explain impact output",
        ),
        ExplainCommand::Convergence(_) => crate::emit_rendered(
            &build_convergence_explanation_output(context),
            Some(OutputMeta::full()),
            json,
            json_style,
            output_style,
            "failed to write explain convergence output",
        ),
        ExplainCommand::Obligation(args) => crate::emit_rendered(
            &build_obligation_explanation_output(context, args)?,
            Some(OutputMeta::full()),
            json,
            json_style,
            output_style,
            "failed to write explain obligation output",
        ),
        ExplainCommand::Suggestion(args) => crate::emit_rendered(
            &build_suggestion_explanation_output(context, args)?,
            Some(OutputMeta::full()),
            json,
            json_style,
            output_style,
            "failed to write explain suggestion output",
        ),
    }
}

fn build_diagnostic_explanation_output(
    context: &AnalysisContext<'_>,
    args: &DiagnosticExplainArgs,
) -> anyhow::Result<DiagnosticExplanation> {
    let diagnostics = crate::analysis::build_analysis_artifacts(context).diagnostics;
    let diagnostic = select_diagnostic(&diagnostics, args)?;
    Ok(DiagnosticExplanation {
        diagnostic_id: diagnostic_id(diagnostic),
        severity: diagnostic.severity,
        code: diagnostic.code.to_string(),
        message: diagnostic.message.clone(),
        file: diagnostic.file.clone(),
        line: diagnostic.line,
        rule: Some(crate::checks::diagnostic_rule_name(diagnostic.code).to_string()),
        facts: diagnostic_facts(diagnostic),
    })
}

fn build_convergence_explanation_output(context: &AnalysisContext<'_>) -> ConvergenceExplanation {
    let analysis = crate::analysis::build_analysis_artifacts(context);
    let current = snapshot::build_snapshot(
        context.graph,
        context.lattice,
        context.config,
        &analysis.diagnostics,
    );
    let current_summary = convergence_snapshot_summary(&current);
    let pipeline = build_pipeline_summary(context);

    if let Some(previous) = analysis.previous_snapshot.as_ref() {
        let convergence = snapshot::analyze_convergence(&current, previous);
        let detail = convergence.detail.clone();
        ConvergenceExplanation {
            signal: convergence.signal.to_string(),
            detail,
            current: current_summary,
            previous: Some(convergence_snapshot_summary(previous)),
            pipeline,
            facts: convergence_facts(&current, previous, &convergence),
        }
    } else {
        ConvergenceExplanation {
            signal: "no_history".to_string(),
            detail: "no previous snapshot available; run `anneal status` or `anneal check` again after this snapshot is stored".to_string(),
            current: current_summary,
            previous: None,
            pipeline,
            facts: vec![
                fact("history", "previous_snapshot", "missing"),
                fact("history", "status_behavior", "status shows no convergence signal on first run"),
            ],
        }
    }
}

fn build_pipeline_summary(context: &AnalysisContext<'_>) -> PipelineSummary {
    // Prefer configured partition so users see what they declared, not just
    // what the corpus happens to use. Fall back to observed lattice when the
    // config section is absent (zero-config mode).
    let cfg_active = &context.config.convergence.active;
    let cfg_terminal = &context.config.convergence.terminal;
    let mut active = if cfg_active.is_empty() {
        context.lattice.active.iter().cloned().collect::<Vec<_>>()
    } else {
        cfg_active.clone()
    };
    active.sort();
    let mut terminal = if cfg_terminal.is_empty() {
        context.lattice.terminal.iter().cloned().collect::<Vec<_>>()
    } else {
        cfg_terminal.clone()
    };
    terminal.sort();
    // Ordering is normally copied into the lattice from config; fall back to
    // config directly in case a caller constructs a lattice without it.
    let ordering = if context.lattice.ordering.is_empty() {
        context.config.convergence.ordering.clone()
    } else {
        context.lattice.ordering.clone()
    };

    let mut descriptions: Vec<StatusDescription> = context
        .config
        .convergence
        .descriptions
        .iter()
        .map(|(status, description)| StatusDescription {
            status: status.clone(),
            description: description.clone(),
        })
        .collect();
    descriptions.sort_by(|a, b| a.status.cmp(&b.status));

    PipelineSummary {
        active,
        terminal,
        ordering,
        descriptions,
    }
}

fn build_impact_explanation_output(
    context: &AnalysisContext<'_>,
    args: &ImpactExplainArgs,
) -> anyhow::Result<ImpactExplanation> {
    let node_id = crate::cli::lookup_handle(context.node_index, &args.handle)
        .with_context(|| format!("handle not found: {}", args.handle))?;
    let root = context.graph.node(node_id).id.clone();
    let traverse_set = context.config.impact.resolve_traverse_set();
    let paths = impact::compute_impact_paths(context.graph, node_id, &traverse_set);

    Ok(ImpactExplanation {
        root,
        direct: paths
            .direct
            .into_iter()
            .map(|entry| impact_path(entry, context.graph))
            .collect(),
        indirect: paths
            .indirect
            .into_iter()
            .map(|entry| impact_path(entry, context.graph))
            .collect(),
    })
}

fn impact_path(entry: impact::ImpactPathEntry, graph: &crate::graph::DiGraph) -> ImpactPath {
    ImpactPath {
        target: graph.node(entry.target).id.clone(),
        path: entry
            .path
            .into_iter()
            .map(|hop| ImpactHop {
                source: graph.node(hop.source).id.clone(),
                edge_kind: hop.edge_kind.as_str().to_string(),
                target: graph.node(hop.target).id.clone(),
            })
            .collect(),
    }
}

fn build_obligation_explanation_output(
    context: &AnalysisContext<'_>,
    args: &ObligationExplainArgs,
) -> anyhow::Result<ObligationExplanation> {
    let node_id = crate::cli::lookup_handle(context.node_index, &args.handle)
        .with_context(|| format!("handle not found: {}", args.handle))?;
    let handle = context.graph.node(node_id);
    let HandleKind::Label { .. } = &handle.kind else {
        bail!("handle is not a label obligation: {}", args.handle);
    };
    let entry = lookup_obligation(context.graph, context.lattice, context.config, node_id)
        .with_context(|| format!("handle is not in a linear namespace: {}", args.handle))?;

    let mut facts = vec![
        fact("obligation", "namespace", &entry.namespace),
        fact("obligation", "disposition", entry.disposition.as_str()),
        fact("count", "dischargers", &entry.discharge_count.to_string()),
        fact(
            "state",
            "terminal",
            if entry.disposition == crate::obligations::ObligationDisposition::Mooted {
                "true"
            } else {
                "false"
            },
        ),
    ];
    if let Some(file) = &entry.file {
        facts.push(fact("location", "file", file));
    }
    if entry.dischargers.is_empty() {
        facts.push(fact("discharger", "handles", "none"));
    } else {
        facts.push(fact("discharger", "handles", &entry.dischargers.join(", ")));
    }

    let outstanding = entry.disposition == crate::obligations::ObligationDisposition::Outstanding;
    let remediation = outstanding.then(|| RemediationHint {
        action: "Add to the resolving document's frontmatter".to_string(),
        frontmatter_syntax: format!("discharges: [{}]", entry.handle),
    });
    let candidates = if outstanding {
        find_candidate_dischargers(context, &entry, node_id)
    } else {
        Vec::new()
    };

    Ok(ObligationExplanation {
        handle: entry.handle,
        namespace: entry.namespace,
        disposition: entry.disposition.as_str().to_string(),
        facts,
        remediation,
        candidates,
    })
}

/// Up to 5 files ranked by graph-proximity signals: citing handles in the same
/// namespace (weight 1) and edges to the obligation's defining file (weight 2).
/// Files already discharging the obligation are excluded.
fn find_candidate_dischargers(
    context: &AnalysisContext<'_>,
    entry: &crate::obligations::ObligationEntry,
    obligation_node: crate::handle::NodeId,
) -> Vec<CandidateDischarger> {
    use std::collections::{HashMap, HashSet};
    let graph = context.graph;
    let mut scores: HashMap<crate::handle::NodeId, (u32, Vec<String>)> = HashMap::new();

    let already_discharging: HashSet<&str> = entry.dischargers.iter().map(String::as_str).collect();

    let mut accumulate = |source_node: crate::handle::NodeId, weight: u32, reason: String| {
        let source = graph.node(source_node);
        if !matches!(source.kind, HandleKind::File(_)) {
            return;
        }
        if already_discharging.contains(source.id.as_str()) {
            return;
        }
        let (score, reasons) = scores.entry(source_node).or_insert((0, Vec::new()));
        *score += weight;
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    };

    for (other_id, other) in graph.nodes() {
        if other_id == obligation_node {
            continue;
        }
        let HandleKind::Label { prefix, .. } = &other.kind else {
            continue;
        };
        if prefix != &entry.namespace {
            continue;
        }
        for edge in graph.incoming(other_id) {
            accumulate(edge.source, 1, format!("cites {}", other.id));
        }
    }

    if let Some(file_path) = &entry.file
        && let Some(&file_node) = context.node_index.get(file_path.as_str())
    {
        for edge in graph.incoming(file_node) {
            accumulate(
                edge.source,
                2,
                format!("{} {}", edge.kind.as_str().to_lowercase(), file_path),
            );
        }
    }

    let mut ranked: Vec<(crate::handle::NodeId, u32, Vec<String>)> = scores
        .into_iter()
        .map(|(nid, (score, reasons))| (nid, score, reasons))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| graph.node(a.0).id.cmp(&graph.node(b.0).id))
    });
    ranked.truncate(5);

    ranked
        .into_iter()
        .map(|(nid, _, reasons)| CandidateDischarger {
            handle: graph.node(nid).id.clone(),
            reason: reasons.join(", "),
        })
        .collect()
}

fn build_suggestion_explanation_output(
    context: &AnalysisContext<'_>,
    args: &SuggestionExplainArgs,
) -> anyhow::Result<SuggestionExplanation> {
    let diagnostics = crate::analysis::build_analysis_artifacts_with_selection(
        context,
        crate::query::suggestion_diagnostic_selection(args.code.as_deref()),
    )
    .diagnostics;
    let diagnostic = select_suggestion(&diagnostics, args)?;
    let suggestion_id =
        suggestion_id(diagnostic).context("selected diagnostic is not a suggestion")?;
    Ok(SuggestionExplanation {
        suggestion_id,
        code: diagnostic.code.to_string(),
        message: diagnostic.message.clone(),
        file: diagnostic.file.clone(),
        line: diagnostic.line,
        rule: Some(crate::checks::diagnostic_rule_name(diagnostic.code).to_string()),
        facts: diagnostic_facts(diagnostic),
    })
}

fn convergence_snapshot_summary(snapshot: &snapshot::Snapshot) -> ConvergenceSnapshotSummary {
    ConvergenceSnapshotSummary {
        handles_total: snapshot.handles.total,
        handles_active: snapshot.handles.active,
        handles_frozen: snapshot.handles.frozen,
        obligations_outstanding: snapshot.obligations.outstanding,
    }
}

fn convergence_facts(
    current: &snapshot::Snapshot,
    previous: &snapshot::Snapshot,
    analysis: &snapshot::ConvergenceAnalysis,
) -> Vec<ExplanationFact> {
    let mut facts = vec![
        fact(
            "delta",
            "resolution_gain",
            &analysis.resolution_gain.to_string(),
        ),
        fact(
            "delta",
            "creation_gain",
            &analysis.creation_gain.to_string(),
        ),
        fact(
            "delta",
            "obligations_delta",
            &analysis.obligations_delta.to_string(),
        ),
        fact(
            "current",
            "handles_frozen",
            &current.handles.frozen.to_string(),
        ),
        fact(
            "previous",
            "handles_frozen",
            &previous.handles.frozen.to_string(),
        ),
        fact(
            "current",
            "handles_total",
            &current.handles.total.to_string(),
        ),
        fact(
            "previous",
            "handles_total",
            &previous.handles.total.to_string(),
        ),
        fact(
            "current",
            "obligations_outstanding",
            &current.obligations.outstanding.to_string(),
        ),
        fact(
            "previous",
            "obligations_outstanding",
            &previous.obligations.outstanding.to_string(),
        ),
    ];

    match analysis.signal {
        snapshot::ConvergenceSignal::Advancing => {
            facts.push(fact(
                "rule",
                "selected_branch",
                "resolution_gain > creation_gain && obligations_delta <= 0",
            ));
            facts.push(fact(
                "rule",
                "why_not_drifting",
                "creation did not exceed resolution and obligations did not increase",
            ));
        }
        snapshot::ConvergenceSignal::Drifting => {
            facts.push(fact(
                "rule",
                "selected_branch",
                "creation_gain > resolution_gain || obligations_delta > 0",
            ));
            if analysis.creation_gain > analysis.resolution_gain {
                facts.push(fact("rule", "drift_driver", "creation exceeded resolution"));
            }
            if analysis.obligations_delta > 0 {
                facts.push(fact(
                    "rule",
                    "drift_driver",
                    "outstanding obligations increased",
                ));
            }
        }
        snapshot::ConvergenceSignal::Holding => {
            facts.push(fact(
                "rule",
                "selected_branch",
                "neither advancing nor drifting conditions applied",
            ));
            facts.push(fact(
                "rule",
                "why_not_advancing",
                "resolution did not exceed creation enough to outpace it",
            ));
            facts.push(fact(
                "rule",
                "why_not_drifting",
                "creation did not exceed resolution and obligations did not increase",
            ));
        }
    }

    facts
}

fn select_diagnostic<'a>(
    diagnostics: &'a [Diagnostic],
    args: &DiagnosticExplainArgs,
) -> anyhow::Result<&'a Diagnostic> {
    if let Some(id) = &args.id {
        return diagnostics
            .iter()
            .find(|diagnostic| diagnostic_id(diagnostic) == *id)
            .with_context(|| format!("no diagnostic found for id {id}"));
    }

    let mut matches: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| matches_secondary_selectors(diagnostic, args))
        .collect();

    match matches.len() {
        0 => bail!(
            "no diagnostic matched the provided selectors; use --id or narrow with --code/--file/--line/--handle"
        ),
        1 => Ok(matches.remove(0)),
        count => bail!(
            "{count} diagnostics matched the provided selectors; use --id from `anneal query diagnostics --json` or `anneal check --json`"
        ),
    }
}

fn select_suggestion<'a>(
    diagnostics: &'a [Diagnostic],
    args: &SuggestionExplainArgs,
) -> anyhow::Result<&'a Diagnostic> {
    if let Some(id) = &args.id {
        return diagnostics
            .iter()
            .find(|diagnostic| suggestion_id(diagnostic).as_deref() == Some(id.as_str()))
            .with_context(|| format!("no suggestion found for id {id}"));
    }

    let selector = args.handle.as_deref().map(build_handle_selector);
    let mut matches: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == crate::checks::Severity::Suggestion)
        .filter(|diagnostic| {
            args.code
                .as_ref()
                .is_none_or(|code| diagnostic.code.as_str() == code.as_str())
        })
        .filter(|diagnostic| {
            selector
                .as_ref()
                .is_none_or(|selector| handle_mentions_selector(diagnostic, selector, false))
        })
        .collect();

    match matches.len() {
        0 => bail!(
            "no suggestion matched the provided selectors; use --id or narrow with code/--handle"
        ),
        1 => Ok(matches.remove(0)),
        count => bail!(
            "{count} suggestions matched the provided selectors; use --id from `anneal query suggestions --json` or `anneal check --json`"
        ),
    }
}

fn matches_secondary_selectors(diagnostic: &Diagnostic, args: &DiagnosticExplainArgs) -> bool {
    let selector = args.handle.as_deref().map(build_handle_selector);
    if args.id.is_none()
        && args.code.is_none()
        && args.file.is_none()
        && args.line.is_none()
        && selector.is_none()
    {
        return false;
    }
    if args
        .code
        .as_ref()
        .is_some_and(|code| diagnostic.code.as_str() != code.as_str())
    {
        return false;
    }
    if args.file.as_ref().is_some_and(|file| {
        diagnostic
            .file
            .as_deref()
            .is_none_or(|path| !crate::analysis::matches_scoped_file(path, file))
    }) {
        return false;
    }
    if args.line.is_some_and(|line| diagnostic.line != Some(line)) {
        return false;
    }
    if selector
        .as_ref()
        .is_some_and(|selector| !handle_mentions_selector(diagnostic, selector, true))
    {
        return false;
    }
    true
}

struct HandleSelector<'a> {
    raw: &'a str,
    namespace: Option<&'a str>,
}

fn build_handle_selector(raw: &str) -> HandleSelector<'_> {
    HandleSelector {
        raw,
        namespace: crate::resolve::split_trailing_numeric_suffix(raw)
            .map(|(prefix, _)| prefix.trim_end_matches('-')),
    }
}

fn handle_mentions_selector(
    diagnostic: &Diagnostic,
    selector: &HandleSelector<'_>,
    allow_message_fallback: bool,
) -> bool {
    if diagnostic
        .file
        .as_deref()
        .is_some_and(|file| crate::analysis::matches_scoped_file(file, selector.raw))
    {
        return true;
    }
    if allow_message_fallback && diagnostic.message.contains(selector.raw) {
        return true;
    }
    match diagnostic.evidence.as_ref() {
        Some(Evidence::BrokenRef { target, candidates }) => {
            target == selector.raw || candidates.iter().any(|candidate| candidate == selector.raw)
        }
        Some(Evidence::Suggestion { suggestion }) => {
            suggestion_matches_selector(suggestion, selector)
        }
        _ => false,
    }
}

fn diagnostic_facts(diagnostic: &Diagnostic) -> Vec<ExplanationFact> {
    let mut facts = Vec::new();
    facts.push(fact("diagnostic", "severity", diagnostic.severity.as_str()));
    facts.push(fact("diagnostic", "code", diagnostic.code.as_str()));
    if let Some(file) = &diagnostic.file {
        facts.push(fact("location", "file", file));
    }
    if let Some(line) = diagnostic.line {
        facts.push(fact("location", "line", &line.to_string()));
    }

    match diagnostic.evidence.as_ref() {
        Some(Evidence::BrokenRef { target, candidates }) => {
            facts.push(fact("target", "missing", target));
            if candidates.is_empty() {
                facts.push(fact("resolution", "candidates", "none"));
            } else {
                facts.push(fact("resolution", "candidates", &candidates.join(", ")));
            }
        }
        Some(Evidence::StaleRef {
            source_status,
            target_status,
        }) => {
            if let Some((source, target)) = parse_binary_handle_message(
                &diagnostic.message,
                "stale reference: ",
                " (active) references ",
                " (",
            ) {
                facts.push(fact("handle", "source", &source));
                facts.push(fact("handle", "target", &target));
            }
            facts.push(fact("state", "source_status", source_status));
            facts.push(fact("state", "target_status", target_status));
            facts.push(fact("edge", "kind", "references"));
        }
        Some(Evidence::ConfidenceGap {
            source_status,
            source_level,
            target_status,
            target_level,
        }) => {
            if let Some((source, target)) = parse_binary_handle_message(
                &diagnostic.message,
                "confidence gap: ",
                " (",
                ") depends on ",
            ) {
                facts.push(fact("handle", "source", &source));
                facts.push(fact("handle", "target", &target));
            }
            facts.push(fact("edge", "kind", "DependsOn"));
            facts.push(fact("state", "source_status", source_status));
            facts.push(fact("state", "source_level", &source_level.to_string()));
            facts.push(fact("state", "target_status", target_status));
            facts.push(fact("state", "target_level", &target_level.to_string()));
        }
        Some(Evidence::Implausible { value, reason }) => {
            facts.push(fact("value", "raw", value));
            facts.push(fact("value", "reason", reason));
        }
        Some(Evidence::Suggestion { suggestion }) => {
            add_suggestion_facts(&mut facts, suggestion);
        }
        None => add_message_derived_facts(&mut facts, diagnostic),
    }

    facts
}

fn add_message_derived_facts(facts: &mut Vec<ExplanationFact>, diagnostic: &Diagnostic) {
    match diagnostic.code {
        DiagnosticCode::I001 => {
            if let Some(count) = diagnostic.message.split_whitespace().next() {
                facts.push(fact("count", "section_references", count));
            }
            facts.push(fact(
                "rule",
                "section_notation",
                "not resolvable to heading slugs",
            ));
        }
        DiagnosticCode::E002 => {
            if let Some(handle) =
                parse_after_prefix(&diagnostic.message, "undischarged obligation: ", " has ")
            {
                facts.push(fact("handle", "obligation", &handle));
            }
            facts.push(fact("edge", "kind", "Discharges"));
            facts.push(fact("status", "disposition", "outstanding"));
        }
        DiagnosticCode::I002 => {
            if let Some((handle, count)) = parse_before_and_between(
                &diagnostic.message,
                "multiple discharges: ",
                " discharged ",
                " times",
            ) {
                facts.push(fact("handle", "obligation", &handle));
                facts.push(fact("count", "discharges", &count));
            }
            facts.push(fact("status", "disposition", "multiple_discharges"));
        }
        DiagnosticCode::W003 => {
            if let Some(handle) =
                parse_after_prefix(&diagnostic.message, "missing frontmatter: ", " has ")
            {
                facts.push(fact("handle", "file", &handle));
            }
        }
        _ => {}
    }
}

fn add_suggestion_facts(facts: &mut Vec<ExplanationFact>, suggestion: &SuggestionEvidence) {
    match suggestion {
        SuggestionEvidence::OrphanedHandle { handle } => {
            facts.push(fact("handle", "orphan", handle));
            facts.push(fact("count", "incoming_edges", "0"));
        }
        SuggestionEvidence::CandidateNamespace { prefix, count } => {
            facts.push(fact("namespace", "prefix", prefix));
            facts.push(fact("count", "labels", &count.to_string()));
        }
        SuggestionEvidence::PipelineStall {
            status,
            count,
            next_status,
            based_on_history,
        } => {
            facts.push(fact("state", "status", status));
            facts.push(fact("count", "handles", &count.to_string()));
            if let Some(next_status) = next_status {
                facts.push(fact("state", "next_status", next_status));
            }
            facts.push(fact(
                "signal",
                "based_on_history",
                if *based_on_history { "true" } else { "false" },
            ));
        }
        SuggestionEvidence::AbandonedNamespace {
            prefix,
            member_count,
            terminal_members,
            stale_members,
        } => {
            facts.push(fact("namespace", "prefix", prefix));
            facts.push(fact("count", "members", &member_count.to_string()));
            facts.push(fact(
                "count",
                "terminal_members",
                &terminal_members.to_string(),
            ));
            facts.push(fact("count", "stale_members", &stale_members.to_string()));
        }
        SuggestionEvidence::ConcernGroupCandidate {
            left_prefix,
            right_prefix,
            file_count,
        } => {
            facts.push(fact("namespace", "left", left_prefix));
            facts.push(fact("namespace", "right", right_prefix));
            facts.push(fact("count", "files", &file_count.to_string()));
        }
    }
}

fn suggestion_matches_selector(
    suggestion: &SuggestionEvidence,
    selector: &HandleSelector<'_>,
) -> bool {
    match suggestion {
        SuggestionEvidence::OrphanedHandle { handle } => handle == selector.raw,
        SuggestionEvidence::CandidateNamespace { prefix, .. }
        | SuggestionEvidence::AbandonedNamespace { prefix, .. } => {
            selector.raw == prefix || selector.namespace.is_some_and(|ns| ns == prefix)
        }
        SuggestionEvidence::PipelineStall { .. } => false,
        SuggestionEvidence::ConcernGroupCandidate {
            left_prefix,
            right_prefix,
            ..
        } => {
            selector.raw == left_prefix
                || selector.raw == right_prefix
                || selector
                    .namespace
                    .is_some_and(|ns| ns == left_prefix || ns == right_prefix)
        }
    }
}

fn parse_after_prefix(message: &str, prefix: &str, before: &str) -> Option<String> {
    message
        .strip_prefix(prefix)
        .and_then(|rest| rest.split_once(before).map(|(value, _)| value.to_string()))
}

fn parse_before_and_between(
    message: &str,
    prefix: &str,
    middle: &str,
    suffix: &str,
) -> Option<(String, String)> {
    let rest = message.strip_prefix(prefix)?;
    let (left, right) = rest.split_once(middle)?;
    let (middle_value, _) = right.split_once(suffix)?;
    Some((left.to_string(), middle_value.to_string()))
}

fn parse_binary_handle_message(
    message: &str,
    prefix: &str,
    left_stop: &str,
    middle: &str,
) -> Option<(String, String)> {
    let rest = message.strip_prefix(prefix)?;
    let (left, rest) = rest.split_once(left_stop)?;
    let (_, right_rest) = rest.split_once(middle)?;
    let right = right_rest
        .split_once(" (")
        .map_or(right_rest, |(value, _)| value);
    Some((left.to_string(), right.to_string()))
}

fn fact(fact_type: &str, key: &str, value: &str) -> ExplanationFact {
    ExplanationFact {
        fact_type: fact_type.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Emit `Facts (N)` heading followed by aligned `fact_type key value`
/// rows. Shared across every explanation variant so facts surface
/// identically regardless of origin.
fn render_facts<W: Write>(p: &mut Printer<W>, facts: &[ExplanationFact]) -> std::io::Result<()> {
    if facts.is_empty() {
        return Ok(());
    }
    p.blank()?;
    p.heading("Facts", Some(facts.len()))?;
    let type_width = facts.iter().map(|f| f.fact_type.len()).max().unwrap_or(0);
    let key_width = facts.iter().map(|f| f.key.len()).max().unwrap_or(0);
    for fact in facts {
        let type_pad = type_width - fact.fact_type.len();
        let key_pad = key_width - fact.key.len();
        p.line_at(
            4,
            &Line::new()
                .toned(Tone::Dim, fact.fact_type.clone())
                .pad(type_pad + 2)
                .toned(Tone::Heading, fact.key.clone())
                .pad(key_pad + 2)
                .text(fact.value.clone()),
        )?;
    }
    Ok(())
}

fn location_line(file: &str, line: Option<u32>) -> Line {
    match line {
        Some(ln) => Line::new().path(format!("{file}:{ln}")),
        None => Line::new().path(file.to_string()),
    }
}

impl Render for DiagnosticExplanation {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.line(
            &Line::new()
                .heading(format!("Diagnostic {}", self.code))
                .text("  ")
                .toned(self.severity.to_output().tone(), self.severity.as_str()),
        )?;
        p.blank()?;

        let mut rows: Vec<(&str, Line)> =
            vec![("id", Line::new().text(self.diagnostic_id.clone()))];
        if let Some(rule) = &self.rule {
            rows.push(("rule", Line::new().text(rule.clone())));
        }
        if let Some(file) = &self.file {
            rows.push(("location", location_line(file, self.line)));
        }
        rows.push(("message", Line::new().text(self.message.clone())));
        p.kv_block(&rows)?;

        render_facts(p, &self.facts)?;
        Ok(())
    }
}

impl Render for ConvergenceExplanation {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.heading(&format!("Convergence {}", self.signal), None)?;
        p.blank()?;

        let current_line = snapshot_summary_line(&self.current);
        let mut rows: Vec<(&str, Line)> = vec![
            ("detail", Line::new().text(self.detail.clone())),
            ("current", current_line),
        ];
        if let Some(previous) = &self.previous {
            rows.push(("previous", snapshot_summary_line(previous)));
        } else {
            rows.push(("previous", Line::new().dim("(none)")));
        }
        p.kv_block(&rows)?;

        render_pipeline(p, &self.pipeline)?;
        render_facts(p, &self.facts)?;
        Ok(())
    }
}

fn snapshot_summary_line(summary: &ConvergenceSnapshotSummary) -> Line {
    Line::new()
        .text("handles ")
        .count(summary.handles_total)
        .text(", active ")
        .count(summary.handles_active)
        .text(", frozen ")
        .count(summary.handles_frozen)
        .text(", obligations ")
        .count(summary.obligations_outstanding)
}

fn render_pipeline<W: Write>(
    p: &mut Printer<W>,
    pipeline: &PipelineSummary,
) -> std::io::Result<()> {
    if pipeline.active.is_empty()
        && pipeline.terminal.is_empty()
        && pipeline.ordering.is_empty()
        && pipeline.descriptions.is_empty()
    {
        return Ok(());
    }
    p.blank()?;
    p.heading("Pipeline", None)?;
    let mut rows: Vec<(&str, Line)> = Vec::new();
    if !pipeline.active.is_empty() {
        rows.push(("active", Line::new().text(pipeline.active.join(", "))));
    }
    if !pipeline.terminal.is_empty() {
        rows.push(("terminal", Line::new().text(pipeline.terminal.join(", "))));
    }
    if !pipeline.ordering.is_empty() {
        rows.push(("ordering", Line::new().text(pipeline.ordering.join(" → "))));
    }
    if !rows.is_empty() {
        p.kv_block(&rows)?;
    }
    if !pipeline.descriptions.is_empty() {
        p.blank()?;
        p.heading("Descriptions", Some(pipeline.descriptions.len()))?;
        let desc_rows: Vec<(&str, Line)> = pipeline
            .descriptions
            .iter()
            .map(|d| (d.status.as_str(), Line::new().text(d.description.clone())))
            .collect();
        p.kv_block(&desc_rows)?;
    }
    Ok(())
}

impl Render for ImpactExplanation {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.line(
            &Line::new()
                .heading("Impact")
                .text("  ")
                .path(self.root.clone()),
        )?;
        p.blank()?;
        render_impact_section(p, "Direct", &self.direct)?;
        p.blank()?;
        render_impact_section(p, "Indirect", &self.indirect)?;
        Ok(())
    }
}

fn render_impact_section<W: Write>(
    p: &mut Printer<W>,
    label: &str,
    paths: &[ImpactPath],
) -> std::io::Result<()> {
    p.heading(label, Some(paths.len()))?;
    if paths.is_empty() {
        p.line_at(4, &Line::new().dim("(none)"))?;
        return Ok(());
    }
    for path in paths {
        p.line_at(4, &Line::new().path(path.target.clone()))?;
        for hop in &path.path {
            p.line_at(
                6,
                &Line::new()
                    .path(hop.source.clone())
                    .dim(" ")
                    .toned(Tone::Heading, hop.edge_kind.clone())
                    .dim(" ")
                    .path(hop.target.clone()),
            )?;
        }
    }
    Ok(())
}

impl Render for ObligationExplanation {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.heading(&format!("Obligation {}", self.handle), None)?;
        p.blank()?;

        let rows: Vec<(&str, Line)> = vec![
            ("namespace", Line::new().text(self.namespace.clone())),
            ("status", Line::new().text(self.disposition.clone())),
        ];
        p.kv_block(&rows)?;

        render_facts(p, &self.facts)?;

        if let Some(remediation) = &self.remediation {
            p.blank()?;
            p.heading("Remediation", None)?;
            p.line_at(4, &Line::new().text(format!("{}:", remediation.action)))?;
            p.line_at(
                6,
                &Line::new().toned(Tone::Path, remediation.frontmatter_syntax.clone()),
            )?;
        }
        if !self.candidates.is_empty() {
            p.blank()?;
            p.heading("Candidates", Some(self.candidates.len()))?;
            p.caption("ranked by graph proximity")?;
            let width = self
                .candidates
                .iter()
                .map(|c| c.handle.len())
                .max()
                .unwrap_or(0);
            for candidate in &self.candidates {
                let pad = width - candidate.handle.len();
                p.line_at(
                    4,
                    &Line::new()
                        .path(candidate.handle.clone())
                        .pad(pad + 2)
                        .dim(candidate.reason.clone()),
                )?;
            }
        }
        Ok(())
    }
}

impl Render for SuggestionExplanation {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        p.heading(&format!("Suggestion {}", self.code), None)?;
        p.blank()?;

        let mut rows: Vec<(&str, Line)> =
            vec![("id", Line::new().text(self.suggestion_id.clone()))];
        if let Some(rule) = &self.rule {
            rows.push(("rule", Line::new().text(rule.clone())));
        }
        if let Some(file) = &self.file {
            rows.push(("location", location_line(file, self.line)));
        }
        rows.push(("message", Line::new().text(self.message.clone())));
        p.kv_block(&rows)?;

        render_facts(p, &self.facts)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Severity;
    use crate::config::{AnnealConfig, HistoryMode, ResolvedStateConfig};
    use crate::graph::{DiGraph, EdgeKind};
    use crate::handle::Handle;
    use crate::lattice::{Lattice, LatticeKind};
    use crate::parse::BuildResult;
    use camino::Utf8Path;
    use std::collections::{HashMap, HashSet};

    fn sample_analysis_context<'a>(
        graph: &'a DiGraph,
        lattice: &'a Lattice,
        config: &'a AnnealConfig,
        state_config: &'a ResolvedStateConfig,
        result: &'a BuildResult,
        node_index: &'a HashMap<String, crate::handle::NodeId>,
        cascade_candidates: &'a HashMap<String, Vec<String>>,
    ) -> AnalysisContext<'a> {
        AnalysisContext {
            root: Utf8Path::new("."),
            graph,
            lattice,
            config,
            state_config,
            result,
            node_index,
            cascade_candidates,
        }
    }

    fn empty_result() -> BuildResult {
        BuildResult {
            graph: DiGraph::new(),
            label_candidates: Vec::new(),
            pending_edges: Vec::new(),
            observed_statuses: HashSet::new(),
            terminal_by_directory: HashSet::new(),
            observed_frontmatter_keys: HashMap::new(),
            filename_index: HashMap::new(),
            implausible_refs: Vec::new(),
            external_refs: Vec::new(),
            extractions: Vec::new(),
            file_snippets: HashMap::new(),
            label_snippets: HashMap::new(),
            malformed_frontmatter: Vec::new(),
            skipped_non_utf8: 0,
        }
    }

    fn simple_lattice() -> Lattice {
        Lattice {
            observed_statuses: HashSet::new(),
            active: HashSet::new(),
            terminal: HashSet::new(),
            ordering: Vec::new(),
            kind: LatticeKind::Existence,
        }
    }

    fn sample_linear_config() -> AnnealConfig {
        let mut config = AnnealConfig::default();
        config.handles.linear = vec!["P".to_string()];
        config
    }

    fn sample_diagnostic() -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: DiagnosticCode::W002,
            message: "confidence gap: formal-model/v17.md (formal) depends on synthesis/v17.md (provisional)".to_string(),
            file: Some("formal-model/v17.md".to_string()),
            line: Some(42),
            evidence: Some(Evidence::ConfidenceGap {
                source_status: "formal".to_string(),
                source_level: 2,
                target_status: "provisional".to_string(),
                target_level: 1,
            }),
        }
    }

    fn sample_suggestion(
        code: DiagnosticCode,
        message: &str,
        evidence: SuggestionEvidence,
    ) -> Diagnostic {
        Diagnostic {
            severity: Severity::Suggestion,
            code,
            message: message.to_string(),
            file: Some("labels.md".to_string()),
            line: Some(1),
            evidence: Some(Evidence::Suggestion {
                suggestion: evidence,
            }),
        }
    }

    #[test]
    fn select_diagnostic_by_id_prefers_stable_identity() {
        let diagnostic = sample_diagnostic();
        let args = DiagnosticExplainArgs {
            id: Some(diagnostic_id(&diagnostic)),
            code: None,
            file: None,
            line: None,
            handle: None,
        };

        let selected =
            select_diagnostic(std::slice::from_ref(&diagnostic), &args).expect("selected");
        assert_eq!(selected.code, DiagnosticCode::W002);
    }

    #[test]
    fn select_diagnostic_requires_unambiguous_secondary_selectors() {
        let first = Diagnostic {
            code: DiagnosticCode::E001,
            file: Some("spec.md".to_string()),
            ..sample_diagnostic()
        };
        let second = Diagnostic {
            code: DiagnosticCode::E001,
            file: Some("other.md".to_string()),
            ..sample_diagnostic()
        };
        let args = DiagnosticExplainArgs {
            id: None,
            code: Some("E001".to_string()),
            file: None,
            line: None,
            handle: None,
        };

        let error = select_diagnostic(&[first, second], &args).expect_err("ambiguous");
        assert!(error.to_string().contains("use --id"));
    }

    #[test]
    fn diagnostic_facts_include_confidence_gap_details() {
        let facts = diagnostic_facts(&sample_diagnostic());
        assert!(
            facts
                .iter()
                .any(|fact| fact.key == "source_status" && fact.value == "formal")
        );
        assert!(
            facts
                .iter()
                .any(|fact| fact.key == "kind" && fact.value == "DependsOn")
        );
    }

    #[test]
    fn convergence_explanation_reports_no_history_when_none_exists() {
        let graph = DiGraph::new();
        let lattice = simple_lattice();
        let config = AnnealConfig::default();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = HashMap::new();
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_convergence_explanation_output(&context);
        assert_eq!(explanation.signal, "no_history");
        assert!(explanation.previous.is_none());
    }

    #[test]
    fn convergence_explanation_includes_pipeline_from_config() {
        let graph = DiGraph::new();
        let lattice = simple_lattice();
        let mut config = AnnealConfig::default();
        config.convergence.active = vec!["draft".to_string(), "active".to_string()];
        config.convergence.terminal = vec!["archived".to_string()];
        config.convergence.ordering = vec!["draft".to_string(), "active".to_string()];
        config
            .convergence
            .descriptions
            .insert("draft".to_string(), "Under construction".to_string());
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = HashMap::new();
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_convergence_explanation_output(&context);
        assert!(
            explanation.pipeline.active.contains(&"draft".to_string())
                && explanation.pipeline.active.contains(&"active".to_string())
        );
        assert_eq!(explanation.pipeline.terminal, vec!["archived".to_string()]);
        assert_eq!(
            explanation.pipeline.ordering,
            vec!["draft".to_string(), "active".to_string()]
        );
        assert_eq!(explanation.pipeline.descriptions.len(), 1);
        assert_eq!(explanation.pipeline.descriptions[0].status, "draft");

        let mut buf = Vec::new();
        {
            let mut printer = Printer::new(&mut buf, OutputStyle::plain());
            explanation.render(&mut printer).expect("render");
        }
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("Pipeline"));
        assert!(text.contains("draft → active"));
        assert!(text.contains("Under construction"));
    }

    #[test]
    fn impact_explanation_returns_canonical_path_chain() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("a.md", None));
        let b = graph.add_node(Handle::test_file("b.md", None));
        let c = graph.add_node(Handle::test_file("c.md", None));
        graph.add_edge(a, b, EdgeKind::DependsOn);
        graph.add_edge(b, c, EdgeKind::DependsOn);

        let lattice = simple_lattice();
        let config = AnnealConfig::default();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = crate::resolve::build_node_index(&graph);
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_impact_explanation_output(
            &context,
            &ImpactExplainArgs {
                handle: "c.md".to_string(),
                full: false,
            },
        )
        .expect("impact explanation");

        assert_eq!(explanation.root, "c.md");
        assert_eq!(explanation.direct.len(), 1);
        assert_eq!(explanation.direct[0].target, "b.md");
        assert_eq!(explanation.direct[0].path[0].source, "b.md");
        assert_eq!(explanation.direct[0].path[0].target, "c.md");
        assert_eq!(explanation.indirect.len(), 1);
        assert_eq!(explanation.indirect[0].target, "a.md");
        assert_eq!(explanation.indirect[0].path.len(), 2);
        assert_eq!(explanation.indirect[0].path[0].source, "a.md");
        assert_eq!(explanation.indirect[0].path[1].source, "b.md");
    }

    #[test]
    fn obligation_explanation_reports_disposition_and_dischargers() {
        let mut graph = DiGraph::new();
        let label = graph.add_node(Handle::test_label("P", 3, None));
        let discharger = graph.add_node(Handle::test_file("worker.md", None));
        graph.add_edge(discharger, label, EdgeKind::Discharges);

        let lattice = simple_lattice();
        let config = sample_linear_config();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = crate::resolve::build_node_index(&graph);
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_obligation_explanation_output(
            &context,
            &ObligationExplainArgs {
                handle: "P-3".to_string(),
            },
        )
        .expect("obligation explanation");

        assert_eq!(explanation.handle, "P-3");
        assert_eq!(explanation.disposition, "discharged");
        assert!(
            explanation
                .facts
                .iter()
                .any(|fact| fact.key == "handles" && fact.value.contains("worker.md"))
        );
    }

    #[test]
    fn outstanding_obligation_includes_remediation_and_candidates() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_label("P", 1, None));
        let p2 = graph.add_node(Handle::test_label("P", 2, None));
        let spec = graph.add_node(Handle::test_file("spec.md", Some("active")));
        graph.add_edge(spec, p2, EdgeKind::Cites);

        let lattice = simple_lattice();
        let config = sample_linear_config();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = crate::resolve::build_node_index(&graph);
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_obligation_explanation_output(
            &context,
            &ObligationExplainArgs {
                handle: "P-1".to_string(),
            },
        )
        .expect("explanation");

        assert_eq!(explanation.disposition, "outstanding");
        let remediation = explanation
            .remediation
            .as_ref()
            .expect("outstanding obligation has remediation");
        assert_eq!(remediation.frontmatter_syntax, "discharges: [P-1]");
        assert!(
            !explanation.candidates.is_empty(),
            "spec.md cites P-2 → candidate"
        );
        assert!(
            explanation
                .candidates
                .iter()
                .any(|c| c.handle == "spec.md" && c.reason.contains("cites P-2"))
        );
    }

    #[test]
    fn discharged_obligation_has_no_remediation() {
        let mut graph = DiGraph::new();
        let label = graph.add_node(Handle::test_label("P", 9, None));
        let worker = graph.add_node(Handle::test_file("worker.md", None));
        graph.add_edge(worker, label, EdgeKind::Discharges);

        let lattice = simple_lattice();
        let config = sample_linear_config();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = crate::resolve::build_node_index(&graph);
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_obligation_explanation_output(
            &context,
            &ObligationExplainArgs {
                handle: "P-9".to_string(),
            },
        )
        .expect("explanation");

        assert_eq!(explanation.disposition, "discharged");
        assert!(explanation.remediation.is_none());
        assert!(explanation.candidates.is_empty());
    }

    #[test]
    fn suggestion_explanation_resolves_by_secondary_selector() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(Handle::test_label("LONE", 1, None));

        let lattice = simple_lattice();
        let config = AnnealConfig::default();
        let state_config = ResolvedStateConfig {
            history_mode: HistoryMode::Off,
            history_dir: None,
        };
        let result = empty_result();
        let node_index = crate::resolve::build_node_index(&graph);
        let cascade_candidates = HashMap::new();
        let context = sample_analysis_context(
            &graph,
            &lattice,
            &config,
            &state_config,
            &result,
            &node_index,
            &cascade_candidates,
        );

        let explanation = build_suggestion_explanation_output(
            &context,
            &SuggestionExplainArgs {
                id: None,
                code: Some("S001".to_string()),
                handle: Some("LONE-1".to_string()),
            },
        )
        .expect("suggestion explanation");

        assert_eq!(explanation.code, "S001");
        assert!(explanation.suggestion_id.starts_with("sugg_"));
    }

    #[test]
    fn select_suggestion_matches_namespace_from_structured_evidence() {
        let suggestion = sample_suggestion(
            DiagnosticCode::S002,
            "candidate namespace available",
            SuggestionEvidence::CandidateNamespace {
                prefix: "OQ".to_string(),
                count: 4,
            },
        );

        let selected = select_suggestion(
            std::slice::from_ref(&suggestion),
            &SuggestionExplainArgs {
                id: None,
                code: Some("S002".to_string()),
                handle: Some("OQ-17".to_string()),
            },
        )
        .expect("selected suggestion");

        assert_eq!(selected.code, DiagnosticCode::S002);
    }

    #[test]
    fn diagnostic_facts_use_structured_suggestion_evidence() {
        let suggestion = sample_suggestion(
            DiagnosticCode::S005,
            "message text should not matter here",
            SuggestionEvidence::ConcernGroupCandidate {
                left_prefix: "FM".to_string(),
                right_prefix: "OQ".to_string(),
                file_count: 3,
            },
        );

        let facts = diagnostic_facts(&suggestion);
        assert!(
            facts
                .iter()
                .any(|fact| fact.key == "left" && fact.value == "FM")
        );
        assert!(
            facts
                .iter()
                .any(|fact| fact.key == "files" && fact.value == "3")
        );
    }
}
