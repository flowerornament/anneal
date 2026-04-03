use anyhow::bail;
use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ExplainCommand {
    Diagnostic(DiagnosticExplainArgs),
    Impact(ImpactExplainArgs),
    Convergence(ConvergenceExplainArgs),
    Obligation(ObligationExplainArgs),
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

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ExplanationFact {
    pub(crate) fact_type: String,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticExplanation {
    pub(crate) diagnostic_id: String,
    pub(crate) code: String,
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

pub(crate) fn run(_command: &ExplainCommand) -> anyhow::Result<()> {
    bail!("anneal explain is not implemented yet on this branch")
}
