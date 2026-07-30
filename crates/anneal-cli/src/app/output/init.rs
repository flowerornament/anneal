//! Markdown initialization execution and rendering.

use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use serde::Serialize;

use super::super::command::OutputMode;
use super::write_text;

/// Typed markdown-project initialization result awaiting channel rendering.
pub(in crate::app) struct InitCommandOutput {
    pub(super) inner: anneal_md::InitOutput,
}

impl InitCommandOutput {
    /// Render initialization as readable text or a JSON object.
    pub(in crate::app) fn write<W: Write>(self, writer: W, mode: OutputMode) -> Result<()> {
        match mode {
            OutputMode::Human => write_init_text(writer, &self.inner),
            OutputMode::Json | OutputMode::JsonExplicit => write_json_object(writer, &self.inner),
        }
    }
}

fn write_json_object<W: Write, T: Serialize>(mut writer: W, value: &T) -> Result<()> {
    serde_json::to_writer(&mut writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Run markdown initialization and retain its typed render result.
pub(in crate::app) fn run_init(
    root: &Utf8Path,
    dry_run: bool,
    force: bool,
) -> Result<InitCommandOutput> {
    let mode = anneal_md::InitMode::from_flags(dry_run, force);
    let inner =
        anneal_md::render_or_write_init(root, mode).context("failed to initialize anneal.dl")?;
    Ok(InitCommandOutput { inner })
}

fn write_init_text<W: Write>(mut writer: W, output: &anneal_md::InitOutput) -> Result<()> {
    if output.written {
        writeln!(writer, "Wrote {}", output.path)?;
        if let Some(path) = &output.backup_path {
            writeln!(writer, "Moved existing anneal.toml to {path}")?;
        }
    } else {
        writeln!(writer, "anneal.dl")?;
        writeln!(writer, "dry run — not written")?;
    }
    writeln!(writer)?;
    write_text(writer, &output.body)
}
