use std::io::Write;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::analysis::AnalysisContext;
use crate::checks::{Diagnostic, Evidence};
use crate::cli::{JsonEnvelope, JsonStyle, OutputMeta};
use crate::identity::diagnostic_id;

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ExplainCommand {
    Diagnostic(DiagnosticExplainArgs),
    #[command(hide = true)]
    Impact(ImpactExplainArgs),
    #[command(hide = true)]
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
    pub(crate) facts: Vec<ExplanationFact>,
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
            if json {
                crate::cli::print_json(
                    &JsonEnvelope::new(OutputMeta::full(), explanation),
                    json_style,
                )?;
            } else {
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                print_diagnostic_explanation_human(&explanation, &mut lock)
                    .context("failed to write explain diagnostic output")?;
            }
            Ok(())
        }
        ExplainCommand::Impact(_) => {
            bail!("anneal explain impact is not implemented yet on this branch")
        }
        ExplainCommand::Convergence(_) => {
            bail!("anneal explain convergence is not implemented yet on this branch")
        }
        ExplainCommand::Obligation(_) => {
            bail!("anneal explain obligation is not implemented yet on this branch")
        }
        ExplainCommand::Suggestion(_) => {
            bail!("anneal explain suggestion is not implemented yet on this branch")
        }
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
        severity: diagnostic.severity.as_str().to_string(),
        code: diagnostic.code.to_string(),
        message: diagnostic.message.clone(),
        file: diagnostic.file.clone(),
        line: diagnostic.line,
        rule: diagnostic_rule(diagnostic.code).map(str::to_string),
        facts: diagnostic_facts(diagnostic),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Severity;

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
}
