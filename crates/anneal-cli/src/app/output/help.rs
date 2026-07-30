//! Dynamic project-verb help rendering.

use std::fmt::Write as _;

use anneal_core::{VerbCapability, VerbEntry};

/// Render help from one resolved project-verb registry entry.
pub(in crate::app) fn render_dynamic_verb_help(entry: &VerbEntry) -> String {
    let name = entry.name();
    let usage_args = entry
        .args()
        .iter()
        .filter(|arg| arg.default().is_none())
        .fold(String::new(), |mut out, arg| {
            let _ = write!(out, " <{}>", arg.name().to_ascii_uppercase());
            out
        });
    let schema = entry.output_schema().to_string();
    let args = if entry.args().is_empty() {
        "  none".to_string()
    } else {
        entry
            .args()
            .iter()
            .map(|arg| match arg.default() {
                Some(default) => {
                    format!("  {}: {} = {default}", arg.name(), arg.kind())
                }
                None => format!("  {}: {}", arg.name(), arg.kind()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let capabilities = if entry.capabilities().is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            entry
                .capabilities()
                .iter()
                .map(VerbCapability::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        "\
Usage: anneal [OPTIONS] {name} [OPTIONS]{usage_args}

{doc}

This is a saved @verb projected from the resolved VerbRegistry. Use it like a
standard verb, or inspect/modify the underlying query with `anneal describe {name}`,
`anneal schema`, and `anneal -e`.

Options:
      --rows <N>                 Cap returned rows after evaluation
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Arguments:
{args}

Output schema:
  {schema}

Capabilities: {capabilities}
Source: {source}:{line}

Query:
  {query}

Global options:
      --root <PATH>              Corpus root (default: nearest .design, docs, or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json|ndjson> Force readable text or JSON/NDJSON output
",
        doc = entry.doc(),
        source = entry.source().location().source_name,
        line = entry.source().location().line,
        query = entry.query_source(),
    )
}

/// Render project-verb help and disclose a same-named runtime topic.
pub(in crate::app) fn render_dynamic_verb_help_with_collision(
    entry: &VerbEntry,
    collision: bool,
) -> String {
    let mut help = render_dynamic_verb_help(entry);
    if collision {
        let _ = writeln!(
            help,
            "Also: `anneal describe {}` teaches the additional runtime vocabulary sharing this name.",
            entry.name()
        );
    }
    help
}
