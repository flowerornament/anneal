use anyhow::bail;
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::checks::Diagnostic;
use crate::identity::{diagnostic_id, suggestion_id};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryScope {
    Active,
    All,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct QueryPageArgs {
    #[arg(long)]
    pub(crate) limit: Option<usize>,
    #[arg(long, default_value = "0")]
    pub(crate) offset: usize,
    #[arg(long)]
    pub(crate) full: bool,
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
}

#[derive(Args, Clone, Debug)]
pub(crate) struct EdgeQueryArgs {
    #[command(flatten)]
    pub(crate) page: QueryPageArgs,
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

#[allow(dead_code)]
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
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticRow {
    pub(crate) diagnostic_id: String,
    pub(crate) severity: String,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

#[allow(dead_code)]
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
            severity: format!("{:?}", diagnostic.severity).to_lowercase(),
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

pub(crate) fn run(_command: &QueryCommand) -> anyhow::Result<()> {
    bail!("anneal query is not implemented yet on this branch")
}
