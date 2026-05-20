use std::collections::HashMap;

use anneal_core::runtime::prelude::datalog_string_literal;
use anneal_core::{RuntimeConfigKey, runtime_config_declaration_by_key};
use anyhow::Context;
use camino::Utf8Path;
use serde::Serialize;

use crate::config::{
    AnnealConfig, CheckConfig, ConvergenceConfig, Direction, FreshnessConfig, FrontmatterConfig,
    FrontmatterFieldMapping, HandlesConfig, SuppressConfig,
};
use crate::lattice::Lattice;
use crate::output::{Line, Printer, Render};
use crate::resolve::ResolveStats;

// ---------------------------------------------------------------------------
// Init command (CLI-06, CONFIG-04)
// ---------------------------------------------------------------------------

/// Output of `anneal init`: generated config.
#[derive(Debug, Serialize)]
pub(crate) struct InitOutput {
    pub(crate) config: AnnealConfig,
    pub(crate) body: String,
    pub(crate) written: bool,
    pub(crate) path: String,
    pub(crate) backup_path: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct InitRequest<'a> {
    pub(crate) root: &'a Utf8Path,
    pub(crate) existing_config: &'a AnnealConfig,
    pub(crate) lattice: &'a Lattice,
    pub(crate) stats: &'a ResolveStats,
    pub(crate) observed_frontmatter_keys: &'a HashMap<String, usize>,
    pub(crate) mode: InitMode,
}

#[derive(Clone, Copy)]
pub(crate) enum InitMode {
    DryRun,
    Write { force: bool },
}

impl InitMode {
    pub(crate) const fn from_flags(dry_run: bool, force: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else {
            Self::Write { force }
        }
    }

    const fn dry_run(self) -> bool {
        matches!(self, Self::DryRun)
    }

    const fn force(self) -> bool {
        matches!(self, Self::Write { force: true })
    }
}

/// Frontmatter keys that are metadata-only (not edge-producing references).
const METADATA_ONLY_KEYS: &[&str] = &["status", "updated", "title", "description", "tags", "date"];

impl Render for InitOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        if self.written {
            p.line(
                &Line::new()
                    .success("✓ ")
                    .text("Wrote ")
                    .path(self.path.clone()),
            )?;
            if let Some(path) = &self.backup_path {
                p.line(
                    &Line::new()
                        .text("Moved existing ")
                        .path("anneal.toml")
                        .text(" to ")
                        .path(path.clone()),
                )?;
            }
        } else {
            p.heading("anneal.dl", None)?;
            p.caption("dry run — not written")?;
        }
        p.blank()?;
        // Datalog config syntax; emit raw so it parses if the user pipes it
        // into a file.
        for line in self.body.lines() {
            p.raw_line(line)?;
        }
        Ok(())
    }
}

/// Propose frontmatter field mapping based on field name heuristics (D-07).
/// Returns Some(mapping) only for field names that look like edge-producing references.
/// Scalar metadata fields (version, type, authors, etc.) return None.
fn propose_mapping(field_name: &str) -> Option<FrontmatterFieldMapping> {
    let lower = field_name.to_lowercase();
    match lower.as_str() {
        "affects" | "impacts" => Some(FrontmatterFieldMapping {
            edge_kind: "DependsOn".to_string(),
            direction: Direction::Inverse,
        }),
        "source" | "sources" | "based-on" | "builds-on" | "extends" | "parent" => {
            Some(FrontmatterFieldMapping {
                edge_kind: "DependsOn".to_string(),
                direction: Direction::Forward,
            })
        }
        "resolves" | "addresses" => Some(FrontmatterFieldMapping {
            edge_kind: "Discharges".to_string(),
            direction: Direction::Forward,
        }),
        "references" | "refs" | "related" | "see-also" | "cites" => Some(FrontmatterFieldMapping {
            edge_kind: "Cites".to_string(),
            direction: Direction::Forward,
        }),
        _ => None, // Scalar metadata — don't propose
    }
}

/// Generate an `AnnealConfig` from inferred structure.
///
/// Scans the lattice, resolve stats, and observed frontmatter keys to build
/// a config that represents the current corpus structure. The D-07 auto-
/// detection adds frontmatter field mappings for keys seen >= 3 times that
/// are not already in the default mapping.
pub(crate) fn cmd_init(request: InitRequest<'_>) -> anyhow::Result<InitOutput> {
    let InitRequest {
        root,
        existing_config,
        lattice,
        stats,
        observed_frontmatter_keys,
        mode,
    } = request;
    // Build convergence section from lattice
    let mut active: Vec<String> = lattice.active.iter().cloned().collect();
    active.sort();
    let mut terminal: Vec<String> = lattice.terminal.iter().cloned().collect();
    terminal.sort();
    let mut ordering = lattice.ordering.clone();
    if active.is_empty() && terminal.is_empty() && ordering.is_empty() {
        active = vec![
            "draft".to_string(),
            "current".to_string(),
            "stable".to_string(),
        ];
        terminal = vec!["superseded".to_string(), "archived".to_string()];
        ordering = vec![
            "raw".to_string(),
            "draft".to_string(),
            "current".to_string(),
            "stable".to_string(),
        ];
    }

    let convergence = ConvergenceConfig {
        active,
        terminal,
        ordering,
        descriptions: HashMap::new(),
    };

    // Namespaces are inferred from the corpus at extraction time. Config only
    // carries explicit policy, so init does not snapshot observed prefixes.
    let _ = stats;
    let handles = HandlesConfig {
        force: Vec::new(),
        rejected: Vec::new(),
        linear: Vec::new(),
    };

    // Build frontmatter section: start with defaults, add auto-detected fields
    let default_fm = FrontmatterConfig::default();
    let default_keys: std::collections::HashSet<String> =
        default_fm.fields.keys().cloned().collect();

    let mut fields = default_fm.fields;

    for (key, count) in observed_frontmatter_keys {
        if default_keys.contains(key) || METADATA_ONLY_KEYS.contains(&key.as_str()) {
            continue;
        }
        // Only propose fields seen in >= 3 files with edge-like names
        if *count >= 3
            && let Some(mapping) = propose_mapping(key)
        {
            fields.insert(key.clone(), mapping);
        }
    }

    let frontmatter = FrontmatterConfig { fields };

    let inferred_config = AnnealConfig {
        root: String::new(),
        exclude: Vec::new(),
        convergence,
        handles,
        freshness: FreshnessConfig::default(),
        frontmatter,
        check: CheckConfig::default(),
        suppress: SuppressConfig::default(),
        state: crate::config::StateConfig::default(),
        concerns: HashMap::new(),
        impact: crate::config::ImpactConfig::default(),
        areas: crate::config::AreasConfig::default(),
        temporal: crate::config::TemporalConfig::default(),
        orient: crate::config::OrientConfig::default(),
    };

    let legacy_config_exists = root.join("anneal.toml").exists();
    let unified_config_exists = root.join("anneal.dl").exists();
    let config = if legacy_config_exists || unified_config_exists {
        let mut config = existing_config.clone();
        if legacy_config_exists && !unified_config_exists {
            // Legacy `[handles].confirmed` was an inventory of observed
            // namespaces. In unified config, `force` is policy for sparse
            // prefixes only, so migration deliberately drops the old list.
            config.handles.force.clear();
        }
        config
    } else {
        inferred_config
    };

    cmd_init_from_config(root, config, mode)
}

pub(crate) fn cmd_init_from_config(
    root: &Utf8Path,
    mut config: AnnealConfig,
    mode: InitMode,
) -> anyhow::Result<InitOutput> {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    let backup_path = root.join("anneal.toml.legacy");
    let path_str = config_path.to_string();
    let migrating_legacy = legacy_path.exists();
    if migrating_legacy && !config_path.exists() {
        // The app entrypoint can migrate directly from a loaded legacy config,
        // bypassing `cmd_init`; keep the confirmed-inventory drop centralized
        // at the final render/write boundary too.
        config.handles.force.clear();
    }
    let body = render_unified_config(&config);

    let (written, backup_path) = if mode.dry_run() {
        (false, None)
    } else if (config_path.exists() || migrating_legacy) && !mode.force() {
        anyhow::bail!("{}", existing_config_message(root));
    } else {
        write_unified_config(root, &body)?;
        (true, migrating_legacy.then(|| backup_path.to_string()))
    };

    Ok(InitOutput {
        config,
        body,
        written,
        path: path_str,
        backup_path,
    })
}

pub(crate) fn existing_config_message(root: &Utf8Path) -> String {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    match (config_path.exists(), legacy_path.exists()) {
        (true, true) => format!(
            "{config_path} and {legacy_path} already exist; no files were changed. Rerun `anneal init --dry-run` to preview or `anneal init --force` to replace anneal.dl and move anneal.toml aside"
        ),
        (true, false) => format!(
            "{config_path} already exists; no files were changed. Rerun `anneal init --dry-run` to preview or `anneal init --force` to replace it"
        ),
        (false, true) => format!(
            "{legacy_path} already exists; no files were changed. Rerun `anneal init --dry-run` to preview or `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
        ),
        (false, false) => "config already exists".to_string(),
    }
}

fn write_unified_config(root: &Utf8Path, body: &str) -> anyhow::Result<()> {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    let backup_path = root.join("anneal.toml.legacy");
    let temp_path = root.join("anneal.dl.tmp");

    if legacy_path.exists() && backup_path.exists() {
        anyhow::bail!("{backup_path} already exists; move it before forcing config migration");
    }

    std::fs::write(&temp_path, body)
        .with_context(|| format!("failed to write temporary config {temp_path}"))?;

    if legacy_path.exists() {
        std::fs::rename(&legacy_path, &backup_path)
            .with_context(|| format!("failed to move {legacy_path} to {backup_path}"))?;
        if let Err(err) = std::fs::rename(&temp_path, &config_path) {
            let restore_result = std::fs::rename(&backup_path, &legacy_path);
            if let Err(restore_err) = restore_result {
                anyhow::bail!(
                    "failed to install {config_path}: {err}; also failed to restore {legacy_path}: {restore_err}"
                );
            }
            return Err(err).with_context(|| format!("failed to install {config_path}"));
        }
    } else {
        std::fs::rename(&temp_path, &config_path)
            .with_context(|| format!("failed to install {config_path}"))?;
    }

    Ok(())
}

fn render_unified_config(config: &AnnealConfig) -> String {
    let mut out = String::new();
    out.push_str("source md {\n");
    out.push_str("  file_extension(\".md\").\n");
    if config.root.is_empty() {
        out.push_str("  scan_root(\".\").\n");
    } else {
        line_call(&mut out, "scan_root", &[config.root.as_str()]);
    }
    if !config.exclude.is_empty() {
        list_call(&mut out, "scan_exclude", &config.exclude);
    }
    out.push_str("}\n\n");

    if !config.convergence.ordering.is_empty()
        || !config.convergence.active.is_empty()
        || !config.convergence.terminal.is_empty()
        || !config.convergence.descriptions.is_empty()
    {
        out.push_str("config convergence {\n");
        if !config.convergence.ordering.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::ConvergenceOrdering,
                &config.convergence.ordering,
            );
        }
        if !config.convergence.active.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::ConvergenceActive,
                &config.convergence.active,
            );
        }
        if !config.convergence.terminal.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::ConvergenceTerminal,
                &config.convergence.terminal,
            );
        }
        for (status, description) in sorted_map(&config.convergence.descriptions) {
            line_config_call(
                &mut out,
                RuntimeConfigKey::ConvergenceDescription,
                &[status, description],
            );
        }
        out.push_str("}\n\n");
    }

    if !config.handles.force.is_empty()
        || !config.handles.rejected.is_empty()
        || !config.handles.linear.is_empty()
    {
        out.push_str("config handles {\n");
        if !config.handles.force.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::HandlesForce,
                &config.handles.force,
            );
        }
        if !config.handles.rejected.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::HandlesRejected,
                &config.handles.rejected,
            );
        }
        if !config.handles.linear.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::HandlesLinear,
                &config.handles.linear,
            );
        }
        out.push_str("}\n\n");
    }

    out.push_str("config frontmatter {\n");
    for (field, mapping) in sorted_map(&config.frontmatter.fields) {
        let direction = match &mapping.direction {
            Direction::Forward => "forward",
            Direction::Inverse => "inverse",
        };
        line_config_call(
            &mut out,
            RuntimeConfigKey::FrontmatterField,
            &[field, mapping.edge_kind.as_str(), direction],
        );
    }
    out.push_str("}\n\n");

    out.push_str("config freshness {\n");
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::FreshnessWarn,
        config.freshness.warn,
    );
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::FreshnessError,
        config.freshness.error,
    );
    out.push_str("}\n\n");

    out.push_str("config check {\n");
    if let Some(filter) = &config.check.default_filter {
        line_config_call(&mut out, RuntimeConfigKey::CheckDefaultFilter, &[filter]);
    }
    out.push_str("}\n\n");

    if config.state.history_mode.is_some() {
        out.push_str("config state {\n");
        if let Some(mode) = config.state.history_mode {
            let value = match mode {
                crate::config::HistoryMode::Xdg => "xdg",
                crate::config::HistoryMode::Repo => "repo",
                crate::config::HistoryMode::Off => "off",
            };
            line_config_call(&mut out, RuntimeConfigKey::StateHistoryMode, &[value]);
        }
        out.push_str("}\n\n");
    }

    if !config.suppress.codes.is_empty() || !config.suppress.rules.is_empty() {
        out.push_str("config suppress {\n");
        if !config.suppress.codes.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::SuppressCode,
                &config.suppress.codes,
            );
        }
        for rule in &config.suppress.rules {
            line_config_call(
                &mut out,
                RuntimeConfigKey::SuppressRule,
                &[rule.code.as_str(), rule.target.as_str()],
            );
        }
        out.push_str("}\n\n");
    }

    for (name, patterns) in sorted_map(&config.concerns) {
        out.push_str("config concerns {\n");
        let mut values = Vec::with_capacity(patterns.len() + 1);
        values.push(name);
        values.extend(patterns.iter().map(String::as_str));
        line_config_call(&mut out, RuntimeConfigKey::ConcernsGroup, &values);
        out.push_str("}\n\n");
    }

    if !config.impact.traverse.is_empty() {
        out.push_str("config impact {\n");
        list_config_call(
            &mut out,
            RuntimeConfigKey::ImpactTraverse,
            &config.impact.traverse,
        );
        out.push_str("}\n\n");
    }

    out.push_str("config areas {\n");
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::AreasOrphanThreshold,
        config.areas.orphan_threshold,
    );
    out.push_str("}\n\n");

    out.push_str("config temporal {\n");
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::TemporalRecentDays,
        config.temporal.recent_days,
    );
    out.push_str("}\n\n");

    out.push_str("config orient {\n");
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientEdgeWeight,
        config.orient.edge_weight,
    );
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientLabelWeight,
        config.orient.label_weight,
    );
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientRecencyWeight,
        config.orient.recency_weight,
    );
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientRecencyHalfLifeDays,
        config.orient.recency_half_life_days,
    );
    line_config_call(
        &mut out,
        RuntimeConfigKey::OrientBudget,
        &[config.orient.budget.as_str()],
    );
    scalar_config_call(&mut out, RuntimeConfigKey::OrientDepth, config.orient.depth);
    if !config.orient.pin.is_empty() {
        list_config_call(&mut out, RuntimeConfigKey::OrientPin, &config.orient.pin);
    }
    if !config.orient.exclude.is_empty() {
        list_config_call(
            &mut out,
            RuntimeConfigKey::OrientExclude,
            &config.orient.exclude,
        );
    }
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientStubBytes,
        config.orient.stub_bytes,
    );
    scalar_config_call(
        &mut out,
        RuntimeConfigKey::OrientCuratedHubWeight,
        config.orient.curated_hub_weight,
    );
    out.push_str("}\n");

    out
}

fn sorted_map<V>(map: &HashMap<String, V>) -> Vec<(&str, &V)> {
    let mut items: Vec<_> = map
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    items.sort_by_key(|(key, _)| *key);
    items
}

fn line_call(out: &mut String, name: &str, values: &[&str]) {
    out.push_str("  ");
    out.push_str(name);
    out.push('(');
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&dl_string(value));
    }
    out.push_str(").\n");
}

fn list_call(out: &mut String, name: &str, values: &[String]) {
    out.push_str("  ");
    out.push_str(name);
    out.push_str("([");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&dl_string(value));
    }
    out.push_str("]).\n");
}

fn scalar_call(out: &mut String, name: &str, value: impl std::fmt::Display) {
    out.push_str("  ");
    out.push_str(name);
    out.push('(');
    out.push_str(&value.to_string());
    out.push_str(").\n");
}

fn line_config_call(out: &mut String, key: RuntimeConfigKey, values: &[&str]) {
    line_call(out, runtime_config_name(key), values);
}

fn list_config_call(out: &mut String, key: RuntimeConfigKey, values: &[String]) {
    list_call(out, runtime_config_name(key), values);
}

fn scalar_config_call(out: &mut String, key: RuntimeConfigKey, value: impl std::fmt::Display) {
    scalar_call(out, runtime_config_name(key), value);
}

fn runtime_config_name(key: RuntimeConfigKey) -> &'static str {
    runtime_config_declaration_by_key(key)
        .expect("runtime config key has declaration")
        .name()
}

fn dl_string(value: &str) -> String {
    datalog_string_literal(value)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use tempfile::tempdir;

    use crate::lattice::Lattice;
    use crate::resolve::ResolveStats;

    use super::*;

    #[test]
    fn init_scaffolds_lattice_on_defaults_for_markdown_only_project() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        let lattice = Lattice::test_empty();
        let stats = ResolveStats {
            namespaces: HashSet::new(),
            labels_resolved: 0,
            labels_skipped: 0,
            versions_resolved: 0,
            pending_edges_resolved: 0,
            pending_edges_unresolved: 0,
        };

        let output = cmd_init(InitRequest {
            root,
            existing_config: &AnnealConfig::default(),
            lattice: &lattice,
            stats: &stats,
            observed_frontmatter_keys: &HashMap::new(),
            mode: InitMode::Write { force: false },
        })
        .expect("init writes");

        assert!(output.written);
        assert_eq!(
            output.config.convergence.ordering,
            ["raw", "draft", "current", "stable"]
        );
        assert_eq!(
            output.config.convergence.active,
            ["draft", "current", "stable"]
        );
        assert_eq!(
            output.config.convergence.terminal,
            ["superseded", "archived"]
        );
        assert!(root.join("anneal.dl").exists());
    }

    #[test]
    fn init_refuses_to_overwrite_existing_config_without_force() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        std::fs::write(
            root.join("anneal.toml"),
            "[convergence]\nactive = [\"draft\"]\n",
        )
        .expect("write existing config");
        let lattice = Lattice::test_empty();
        let stats = ResolveStats {
            namespaces: HashSet::new(),
            labels_resolved: 0,
            labels_skipped: 0,
            versions_resolved: 0,
            pending_edges_resolved: 0,
            pending_edges_unresolved: 0,
        };

        let error = cmd_init(InitRequest {
            root,
            existing_config: &AnnealConfig::default(),
            lattice: &lattice,
            stats: &stats,
            observed_frontmatter_keys: &HashMap::new(),
            mode: InitMode::Write { force: false },
        })
        .expect_err("existing config should be protected");

        assert!(error.to_string().contains("already exists"));
        assert!(error.to_string().contains("--force"));
        assert!(error.to_string().contains("no files were changed"));
    }

    #[test]
    fn init_existing_config_message_names_actual_paths() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");

        std::fs::write(root.join("anneal.dl"), "").expect("write unified");
        let unified_only = existing_config_message(root);
        assert!(unified_only.contains(&root.join("anneal.dl").to_string()));
        assert!(!unified_only.contains(&root.join("anneal.toml").to_string()));
        assert!(unified_only.contains("replace it"));

        std::fs::remove_file(root.join("anneal.dl")).expect("remove unified");
        std::fs::write(root.join("anneal.toml"), "").expect("write legacy");
        let legacy_only = existing_config_message(root);
        assert!(legacy_only.contains(&root.join("anneal.toml").to_string()));
        assert!(!legacy_only.contains(&root.join("anneal.dl").to_string()));
        assert!(legacy_only.contains("move anneal.toml aside"));

        std::fs::write(root.join("anneal.dl"), "").expect("rewrite unified");
        let both = existing_config_message(root);
        assert!(both.contains(&root.join("anneal.dl").to_string()));
        assert!(both.contains(&root.join("anneal.toml").to_string()));
    }

    #[test]
    fn init_force_replaces_existing_config() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        std::fs::write(
            root.join("anneal.toml"),
            "[convergence]\nactive = [\"custom\"]\n",
        )
        .expect("write existing config");
        let lattice = Lattice::test_empty();
        let stats = ResolveStats {
            namespaces: HashSet::new(),
            labels_resolved: 0,
            labels_skipped: 0,
            versions_resolved: 0,
            pending_edges_resolved: 0,
            pending_edges_unresolved: 0,
        };

        let existing_config = AnnealConfig {
            convergence: ConvergenceConfig {
                active: vec!["custom".to_string()],
                ..ConvergenceConfig::default()
            },
            ..AnnealConfig::default()
        };
        let output = cmd_init(InitRequest {
            root,
            existing_config: &existing_config,
            lattice: &lattice,
            stats: &stats,
            observed_frontmatter_keys: &HashMap::new(),
            mode: InitMode::Write { force: true },
        })
        .expect("force writes");
        let written = std::fs::read_to_string(root.join("anneal.dl")).expect("read config");

        assert!(output.written);
        assert!(written.contains("custom"));
        assert!(root.join("anneal.toml.legacy").exists());
        assert!(!root.join("anneal.toml").exists());
    }

    #[test]
    fn init_drops_legacy_confirmed_namespace_inventory() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        std::fs::write(
            root.join("anneal.toml"),
            "[handles]\nconfirmed = [\"OQ\", \"REQ\"]\n",
        )
        .expect("write legacy config");
        let lattice = Lattice::test_empty();
        let stats = ResolveStats {
            namespaces: HashSet::new(),
            labels_resolved: 0,
            labels_skipped: 0,
            versions_resolved: 0,
            pending_edges_resolved: 0,
            pending_edges_unresolved: 0,
        };
        let existing_config = AnnealConfig {
            handles: HandlesConfig {
                force: vec!["OQ".to_string(), "REQ".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let output = cmd_init(InitRequest {
            root,
            existing_config: &existing_config,
            lattice: &lattice,
            stats: &stats,
            observed_frontmatter_keys: &HashMap::new(),
            mode: InitMode::DryRun,
        })
        .expect("dry run renders");

        assert!(
            !output.body.contains("force("),
            "legacy confirmed inventory should not become sparse force policy"
        );
        assert!(
            !output.body.contains("config handles"),
            "empty handle policy should be omitted from generated config"
        );
    }

    #[test]
    fn init_from_loaded_legacy_config_drops_confirmed_namespace_inventory() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        std::fs::write(
            root.join("anneal.toml"),
            "[handles]\nconfirmed = [\"OQ\", \"REQ\"]\n",
        )
        .expect("write legacy config");
        let config = AnnealConfig {
            handles: HandlesConfig {
                force: vec!["OQ".to_string(), "REQ".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let output = cmd_init_from_config(root, config, InitMode::DryRun).expect("dry run renders");

        assert!(!output.body.contains("force("));
        assert!(!output.body.contains("config handles"));
    }

    #[test]
    fn init_rewrites_loaded_unified_config_without_confirmed_inventory() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(dir.path()).expect("tempdir path is utf8");
        std::fs::write(
            root.join("anneal.dl"),
            "config handles { confirmed([\"OQ\"]). }",
        )
        .expect("write unified config");
        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OQ".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let output = cmd_init_from_config(root, config, InitMode::DryRun).expect("dry run renders");

        assert!(!output.body.contains("confirmed"));
        assert!(output.body.contains("linear([\"OQ\"])"));
    }
}
