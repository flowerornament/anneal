//! Project-verb relationships, related vocabulary, and examples.

/// Places a verb relative to the relation it projects.
pub(in crate::runtime::introspection) fn verb_relationship(name: &str) -> &'static str {
    match name {
        "status" => {
            "Saved query over `primary_entropy`, non-blocked `potential` rows, `flow`, and `diagnostic`; human rendering summarizes convergence counts and sorts rows for arrival."
        }
        "search" => {
            "Saved query over the `search` primitive; applies TopK by calibrated score, filters `low_confidence = false`, and adds summary for span hits."
        }
        "context" => {
            "Saved query that composes boosted span-granular `search`, span metadata, `neighborhood`, TopK, and TakeUntil into one orientation bundle. The CLI `--read-spans` flag expands matched span bodies."
        }
        "read" => {
            "Saved query over the `read` primitive; the CLI can target one heading span with `--span-id`, usually copied from search/context output."
        }
        "handle" => {
            "Saved query over `*handle` and `*edge` for one focused handle; `anneal handle H --impact` adds reverse-dependency impact rows."
        }
        "describe" => "Saved query over the `describe` primitive.",
        "schema" => "Saved query over the `schema` primitive.",
        _ => "Saved @verb projected from the resolved prelude/project registry.",
    }
}

/// Returns adjacent vocabulary for verb drill-down.
pub(in crate::runtime::introspection) fn verb_see_also(name: &str) -> &'static [&'static str] {
    match name {
        "status" => &[
            "frontier",
            "blocked",
            "flow",
            "diagnostic",
            "snapshot_history_present",
        ],
        "context" => &["search", "read", "handle"],
        "search" => &["context", "read", "schema"],
        "handle" => &["*handle", "*edge", "search"],
        "describe" => &["schema", "examples", "source-of"],
        "schema" => &["describe", "examples"],
        _ => &[],
    }
}

/// Returns a verb invocation that is executable against the real grammar.
pub(in crate::runtime::introspection) fn verb_example(name: &str) -> Option<&'static str> {
    match name {
        "status" => Some("anneal status"),
        "context" => Some(r#"anneal context "runtime overview" --hits 3"#),
        "search" => Some(r#"anneal search "runtime overview" --limit 5"#),
        "read" => Some("anneal read docs/runtime-overview.md --budget 4000"),
        "handle" => Some("anneal handle docs/runtime-overview.md --impact"),
        "describe" => Some("anneal describe search"),
        "schema" => Some("anneal schema"),
        _ => None,
    }
}
