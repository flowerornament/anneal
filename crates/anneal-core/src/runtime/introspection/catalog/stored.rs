//! Stored-relation signatures, teaching notes, and dynamic fallbacks.

/// Renders a stored-relation signature from its declared field order.
pub(in crate::runtime::introspection) fn stored_signature(
    name: &str,
    fields: &[impl AsRef<str>],
) -> String {
    let fields = fields
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ");
    format!("*{name}{{{fields}}}")
}

/// Adds relation-specific teaching notes to stored-relation cards.
pub(in crate::runtime::introspection) fn stored_relation_extra_lines(name: &str) -> Vec<String> {
    match name {
        "edge" => vec![
            "assertion_date and assertion_revision describe the cited line that asserted the edge; revision describes the source fact identity.".to_string(),
            "Adapters leave assertion_* null when assertion-time evidence is not verified.".to_string(),
        ],
        "meta" => vec![
            "Open metadata extension on handles. role is derived, authored_modeled, or authored_unmodeled; adapters assign it at emission from source authorship and effective configuration.".to_string(),
            "Three ownership families of keys:".to_string(),
            "STANDARD (defined by anneal, same meaning on any corpus): external_class, target_path, target_start_line, target_end_line, target_exists, target_history_status, target_probe_base, target_resolved_path.".to_string(),
            "SOURCE (produced by a specific source adapter, prefix tells you which): md.resolved_file, md.parent_dir.".to_string(),
            "FRONTMATTER (passed through from YAML, corpus-defined): status, date, author, depends-on, tags, and project-specific fields. Modeled fields retain their authored row while also feeding a typed projection.".to_string(),
            r#"Discover open authored frontmatter with `? *meta{handle: h, key: key, role: "authored_unmodeled"}.`."#.to_string(),
        ],
        "snapshot" => vec![
            "Automatic status snapshots power `at(\"snapshot:last\")` queries; agents do not manage a snapshot command.".to_string(),
            "Retired diff equivalent: `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`.".to_string(),
            "Use raw *snapshot rows only when you need key/value history rather than an at-block composition.".to_string(),
        ],
        _ => Vec::new(),
    }
}

/// Returns the adjacent vocabulary taught for one stored relation.
pub(in crate::runtime::introspection) fn stored_relation_see_also(
    name: &str,
) -> &'static [&'static str] {
    match name {
        "edge" => &["*handle", "currency", "target_history_status"],
        "meta" => &["external_class", "target_path", "*handle", "schema"],
        "snapshot" => &["*handle", "diagnostic", "runtime"],
        _ => &[],
    }
}

/// Builds an executable example when a dynamic relation declares no example.
pub(in crate::runtime::introspection) fn fallback_stored_relation_example(
    name: &str,
    fields: &[impl AsRef<str>],
) -> String {
    let field = fields
        .iter()
        .find_map(|field| {
            let field = field.as_ref();
            (!matches!(
                field,
                "corpus" | "source" | "native_id" | "origin_uri" | "revision" | "generation"
            ))
            .then_some(field)
        })
        .unwrap_or_else(|| fields.first().map_or("value", AsRef::as_ref));
    format!("? *{name}{{{field}: value}}.")
}
