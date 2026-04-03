use std::io::Write;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::analysis::AnalysisContext;
use crate::checks::{Diagnostic, Evidence};
use crate::cli::{JsonEnvelope, JsonStyle, OutputMeta};
use crate::identity::diagnostic_id;
use crate::impact;
use crate::snapshot;

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ExplainCommand {
    Diagnostic(DiagnosticExplainArgs),
    Impact(ImpactExplainArgs),
    Convergence(ConvergenceExplainArgs),
    #[command(hide = true)]
    Obligation(ObligationExplainArgs),
    #[command(hide = true)]
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

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) enum Explanation {
    Diagnostic(DiagnosticExplanation),
    Impact(ImpactExplanation),
    Convergence(ConvergenceExplanation),
    Obligation(ObligationExplanation),
    Suggestion(SuggestionExplanation),
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
    pub(crate) severity: String,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) rule: Option<String>,
    pub(crate) facts: Vec<ExplanationFact>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactExplanation {
    pub(crate) root: String,
    pub(crate) direct: Vec<ImpactPath>,
    pub(crate) indirect: Vec<ImpactPath>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactPath {
    pub(crate) target: String,
    pub(crate) path: Vec<ImpactHop>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImpactHop {
    pub(crate) source: String,
    pub(crate) edge_kind: String,
    pub(crate) target: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConvergenceExplanation {
    pub(crate) signal: String,
    pub(crate) detail: String,
    pub(crate) current: ConvergenceSnapshotSummary,
    pub(crate) previous: Option<ConvergenceSnapshotSummary>,
    pub(crate) facts: Vec<ExplanationFact>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConvergenceSnapshotSummary {
    pub(crate) handles_total: usize,
    pub(crate) handles_active: usize,
    pub(crate) handles_frozen: usize,
    pub(crate) obligations_outstanding: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ObligationExplanation {
    pub(crate) handle: String,
    pub(crate) facts: Vec<ExplanationFact>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SuggestionExplanation {
    pub(crate) suggestion_id: String,
    pub(crate) code: String,
    pub(crate) facts: Vec<ExplanationFact>,
}

pub(crate) fn run(
    context: &AnalysisContext<'_>,
    command: &ExplainCommand,
    json: bool,
    json_style: JsonStyle,
) -> anyhow::Result<()> {
    match command {
        ExplainCommand::Diagnostic(args) => {
            let explanation = build_diagnostic_explanation_output(context, args)?;
            emit_explanation(
                explanation,
                json,
                json_style,
                print_diagnostic_explanation_human,
                "failed to write explain diagnostic output",
            )?;
            Ok(())
        }
        ExplainCommand::Impact(args) => {
            let explanation = build_impact_explanation_output(context, args)?;
            emit_explanation(
                explanation,
                json,
                json_style,
                print_impact_explanation_human,
                "failed to write explain impact output",
            )?;
            Ok(())
        }
        ExplainCommand::Convergence(_) => {
            let explanation = build_convergence_explanation_output(context);
            emit_explanation(
                explanation,
                json,
                json_style,
                print_convergence_explanation_human,
                "failed to write explain convergence output",
            )?;
            Ok(())
        }
        ExplainCommand::Obligation(_) => {
            bail!("anneal explain obligation is not implemented yet on this branch")
        }
        ExplainCommand::Suggestion(_) => {
            bail!("anneal explain suggestion is not implemented yet on this branch")
        }
    }
}

fn emit_explanation<T: Serialize>(
    explanation: T,
    json: bool,
    json_style: JsonStyle,
    render_human: impl FnOnce(&T, &mut dyn Write) -> std::io::Result<()>,
    human_context: &'static str,
) -> anyhow::Result<()> {
    if json {
        crate::cli::print_json(
            &JsonEnvelope::new(OutputMeta::full(), explanation),
            json_style,
        )?;
    } else {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        render_human(&explanation, &mut lock).context(human_context)?;
    }
    Ok(())
}

fn build_diagnostic_explanation_output(
    context: &AnalysisContext<'_>,
    args: &DiagnosticExplainArgs,
) -> anyhow::Result<DiagnosticExplanation> {
    let diagnostics = crate::analysis::build_analysis_artifacts(context).diagnostics;
    let diagnostic = select_diagnostic(&diagnostics, args)?;
    Ok(DiagnosticExplanation {
        diagnostic_id: diagnostic_id(diagnostic),
        severity: diagnostic.severity.as_str().to_string(),
        code: diagnostic.code.to_string(),
        message: diagnostic.message.clone(),
        file: diagnostic.file.clone(),
        line: diagnostic.line,
        rule: diagnostic_rule(diagnostic.code).map(str::to_string),
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

    if let Some(previous) = analysis.previous_snapshot.as_ref() {
        let convergence = snapshot::analyze_convergence(&current, previous);
        let detail = convergence.detail.clone();
        ConvergenceExplanation {
            signal: convergence.signal.to_string(),
            detail,
            current: current_summary,
            previous: Some(convergence_snapshot_summary(previous)),
            facts: convergence_facts(&current, previous, &convergence),
        }
    } else {
        ConvergenceExplanation {
            signal: "no_history".to_string(),
            detail: "no previous snapshot available; run `anneal status` or `anneal check` again after this snapshot is stored".to_string(),
            current: current_summary,
            previous: None,
            facts: vec![
                fact("history", "previous_snapshot", "missing"),
                fact("history", "status_behavior", "status shows no convergence signal on first run"),
            ],
        }
    }
}

fn build_impact_explanation_output(
    context: &AnalysisContext<'_>,
    args: &ImpactExplainArgs,
) -> anyhow::Result<ImpactExplanation> {
    let node_id = crate::cli::lookup_handle(context.node_index, &args.handle)
        .with_context(|| format!("handle not found: {}", args.handle))?;
    let root = context.graph.node(node_id).id.clone();
    let paths = impact::compute_impact_paths(context.graph, node_id);

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

fn matches_secondary_selectors(diagnostic: &Diagnostic, args: &DiagnosticExplainArgs) -> bool {
    if args.id.is_none()
        && args.code.is_none()
        && args.file.is_none()
        && args.line.is_none()
        && args.handle.is_none()
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
    if args
        .handle
        .as_ref()
        .is_some_and(|handle| !diagnostic_mentions_handle(diagnostic, handle))
    {
        return false;
    }
    true
}

fn diagnostic_mentions_handle(diagnostic: &Diagnostic, handle: &str) -> bool {
    if diagnostic
        .file
        .as_deref()
        .is_some_and(|file| crate::analysis::matches_scoped_file(file, handle))
    {
        return true;
    }
    if diagnostic.message.contains(handle) {
        return true;
    }
    match diagnostic.evidence.as_ref() {
        Some(Evidence::BrokenRef { target, candidates }) => {
            target == handle || candidates.iter().any(|candidate| candidate == handle)
        }
        _ => false,
    }
}

fn diagnostic_rule(code: &str) -> Option<&'static str> {
    match code {
        "I001" | "E001" => Some("KB-R1 existence"),
        "W004" => Some("plausibility filter"),
        "W001" => Some("KB-R2 staleness"),
        "W002" => Some("KB-R3 confidence gap"),
        "E002" | "I002" => Some("KB-R4 linearity"),
        "W003" => Some("KB-R5 convention adoption"),
        "S001" => Some("SUGGEST-01 orphaned handles"),
        "S002" => Some("SUGGEST-02 candidate namespaces"),
        "S003" => Some("SUGGEST-03 pipeline stalls"),
        "S004" => Some("SUGGEST-04 abandoned namespaces"),
        "S005" => Some("SUGGEST-05 concern group candidates"),
        _ => None,
    }
}

fn diagnostic_facts(diagnostic: &Diagnostic) -> Vec<ExplanationFact> {
    let mut facts = Vec::new();
    facts.push(fact("diagnostic", "severity", diagnostic.severity.as_str()));
    facts.push(fact("diagnostic", "code", diagnostic.code));
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
        None => add_message_derived_facts(&mut facts, diagnostic),
    }

    facts
}

fn add_message_derived_facts(facts: &mut Vec<ExplanationFact>, diagnostic: &Diagnostic) {
    match diagnostic.code {
        "I001" => {
            if let Some(count) = diagnostic.message.split_whitespace().next() {
                facts.push(fact("count", "section_references", count));
            }
            facts.push(fact(
                "rule",
                "section_notation",
                "not resolvable to heading slugs",
            ));
        }
        "E002" => {
            if let Some(handle) =
                parse_after_prefix(&diagnostic.message, "undischarged obligation: ", " has ")
            {
                facts.push(fact("handle", "obligation", &handle));
            }
            facts.push(fact("edge", "kind", "Discharges"));
            facts.push(fact("status", "disposition", "outstanding"));
        }
        "I002" => {
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
        "W003" => {
            if let Some(handle) =
                parse_after_prefix(&diagnostic.message, "missing frontmatter: ", " has ")
            {
                facts.push(fact("handle", "file", &handle));
            }
        }
        "S001" => {
            if let Some(handle) =
                parse_after_prefix(&diagnostic.message, "orphaned handle: ", " has ")
            {
                facts.push(fact("handle", "orphan", &handle));
            }
            facts.push(fact("count", "incoming_edges", "0"));
        }
        "S002" => {
            if let Some((prefix, count)) = parse_before_and_between(
                &diagnostic.message,
                "candidate namespace: ",
                " (",
                " labels found",
            ) {
                facts.push(fact("namespace", "prefix", &prefix));
                facts.push(fact("count", "labels", &count));
            }
        }
        "S003" => {
            if let Some(status) = diagnostic
                .message
                .split('\'')
                .nth(1)
                .filter(|status| !status.is_empty())
            {
                facts.push(fact("state", "status", status));
            }
        }
        "S004" => {
            if let Some((count, prefix)) = parse_before_and_between(
                &diagnostic.message,
                "abandoned namespace: all ",
                " members of ",
                " are",
            ) {
                facts.push(fact("count", "members", &count));
                facts.push(fact("namespace", "prefix", &prefix));
            }
        }
        "S005" => {
            if let Some(rest) = diagnostic.message.strip_prefix("concern group candidate: ")
                && let Some((pair, count)) = rest.split_once(" co-occur in ")
                && let Some((left, right)) = pair.split_once(" and ")
            {
                facts.push(fact("namespace", "left", left));
                facts.push(fact("namespace", "right", right));
                facts.push(fact("count", "files", count.trim_end_matches(" files")));
            }
        }
        _ => {}
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

fn print_diagnostic_explanation_human(
    explanation: &DiagnosticExplanation,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(
        w,
        "diagnostic  {}  {}",
        explanation.code, explanation.severity
    )?;
    writeln!(w, "id          {}", explanation.diagnostic_id)?;
    if let Some(rule) = &explanation.rule {
        writeln!(w, "rule        {rule}")?;
    }
    if let Some(file) = &explanation.file {
        if let Some(line) = explanation.line {
            writeln!(w, "location    {file}:{line}")?;
        } else {
            writeln!(w, "location    {file}")?;
        }
    }
    writeln!(w)?;
    writeln!(w, "message     {}", explanation.message)?;
    if !explanation.facts.is_empty() {
        writeln!(w)?;
        writeln!(w, "facts")?;
        for fact in &explanation.facts {
            writeln!(
                w,
                "  {:<10} {:<16} {}",
                fact.fact_type, fact.key, fact.value
            )?;
        }
    }
    Ok(())
}

fn print_convergence_explanation_human(
    explanation: &ConvergenceExplanation,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(w, "convergence  {}", explanation.signal)?;
    writeln!(w, "detail       {}", explanation.detail)?;
    writeln!(
        w,
        "current      handles {}  active {}  frozen {}  obligations {}",
        explanation.current.handles_total,
        explanation.current.handles_active,
        explanation.current.handles_frozen,
        explanation.current.obligations_outstanding,
    )?;
    if let Some(previous) = &explanation.previous {
        writeln!(
            w,
            "previous     handles {}  active {}  frozen {}  obligations {}",
            previous.handles_total,
            previous.handles_active,
            previous.handles_frozen,
            previous.obligations_outstanding,
        )?;
    } else {
        writeln!(w, "previous     (none)")?;
    }
    if !explanation.facts.is_empty() {
        writeln!(w)?;
        writeln!(w, "facts")?;
        for fact in &explanation.facts {
            writeln!(
                w,
                "  {:<10} {:<20} {}",
                fact.fact_type, fact.key, fact.value
            )?;
        }
    }
    Ok(())
}

fn print_impact_explanation_human(
    explanation: &ImpactExplanation,
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(w, "impact       {}", explanation.root)?;
    writeln!(w)?;
    print_impact_section("direct", &explanation.direct, w)?;
    writeln!(w)?;
    print_impact_section("indirect", &explanation.indirect, w)?;
    Ok(())
}

fn print_impact_section(
    label: &str,
    paths: &[ImpactPath],
    w: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(w, "{label}")?;
    if paths.is_empty() {
        writeln!(w, "  (none)")?;
        return Ok(());
    }
    for path in paths {
        writeln!(w, "  {}", path.target)?;
        for hop in &path.path {
            writeln!(w, "    {} {} {}", hop.source, hop.edge_kind, hop.target)?;
        }
    }
    Ok(())
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

    fn sample_diagnostic() -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: "W002",
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
        assert_eq!(selected.code, "W002");
    }

    #[test]
    fn select_diagnostic_requires_unambiguous_secondary_selectors() {
        let first = Diagnostic {
            code: "E001",
            file: Some("spec.md".to_string()),
            ..sample_diagnostic()
        };
        let second = Diagnostic {
            code: "E001",
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
}
