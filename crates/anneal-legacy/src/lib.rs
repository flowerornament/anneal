#![allow(dead_code, unused_imports)]

mod analysis;
mod app;
mod area;
mod checks;
mod cli;
mod config;
mod explain;
mod extraction;
mod graph;
mod handle;
mod identity;
mod impact;
mod lattice;
mod obligations;
mod output;
mod parse;
mod query;
mod resolve;
mod snapshot;

pub mod v2_adapter;

pub use app::main_entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapRender {
    Summary,
    Text,
    Dot,
    /// Printer-based focused-neighborhood rendering used by default for
    /// `--around`/`--concern` without an explicit `--render`. Not
    /// intended as a user-facing choice.
    #[clap(hide = true)]
    Around,
}

pub(crate) fn emit_rendered<T: serde::Serialize + output::Render>(
    output: &T,
    envelope_meta: Option<cli::OutputMeta>,
    json: bool,
    json_style: cli::JsonStyle,
    style: output::OutputStyle,
    human_context: &'static str,
) -> anyhow::Result<()> {
    use anyhow::Context as _;

    if json {
        match envelope_meta {
            Some(meta) => cli::print_json(&cli::JsonEnvelope::new(meta, output), json_style)?,
            None => cli::print_json(output, json_style)?,
        }
    } else {
        let writer = std::io::BufWriter::new(std::io::stdout());
        let mut printer = output::Printer::new(writer, style);
        output.render(&mut printer).context(human_context)?;
    }
    Ok(())
}
