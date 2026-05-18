use std::collections::HashMap;

use anneal_core::runtime::prelude::datalog_string_literal;
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

    let config = if root.join("anneal.toml").exists() || root.join("anneal.dl").exists() {
        existing_config.clone()
    } else {
        inferred_config
    };

    cmd_init_from_config(root, config, mode)
}

pub(crate) fn cmd_init_from_config(
    root: &Utf8Path,
    config: AnnealConfig,
    mode: InitMode,
) -> anyhow::Result<InitOutput> {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    let backup_path = root.join("anneal.toml.legacy");
    let path_str = config_path.to_string();
    let migrating_legacy = legacy_path.exists();
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
            "{config_path} and {legacy_path} already exist; rerun `anneal init --dry-run` to preview or `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
        ),
        (true, false) => format!(
            "{config_path} already exists; rerun `anneal init --dry-run` to preview or `anneal init --force` to replace it"
        ),
        (false, true) => format!(
            "{legacy_path} already exists; rerun `anneal init --dry-run` to preview or `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
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
            list_call(&mut out, "ordering", &config.convergence.ordering);
        }
        if !config.convergence.active.is_empty() {
            list_call(&mut out, "active", &config.convergence.active);
        }
        if !config.convergence.terminal.is_empty() {
            list_call(&mut out, "terminal", &config.convergence.terminal);
        }
        for (status, description) in sorted_map(&config.convergence.descriptions) {
            line_call(&mut out, "description", &[status, description]);
        }
        out.push_str("}\n\n");
    }

    if !config.handles.force.is_empty()
        || !config.handles.rejected.is_empty()
        || !config.handles.linear.is_empty()
    {
        out.push_str("config handles {\n");
        if !config.handles.force.is_empty() {
            list_call(&mut out, "force", &config.handles.force);
        }
        if !config.handles.rejected.is_empty() {
            list_call(&mut out, "rejected", &config.handles.rejected);
        }
        if !config.handles.linear.is_empty() {
            list_call(&mut out, "linear", &config.handles.linear);
        }
        out.push_str("}\n\n");
    }

    out.push_str("config frontmatter {\n");
    for (field, mapping) in sorted_map(&config.frontmatter.fields) {
        let direction = match &mapping.direction {
            Direction::Forward => "forward",
            Direction::Inverse => "inverse",
        };
        line_call(
            &mut out,
            "field",
            &[field, mapping.edge_kind.as_str(), direction],
        );
    }
    out.push_str("}\n\n");

    out.push_str("config freshness {\n");
    scalar_call(&mut out, "warn", config.freshness.warn);
    scalar_call(&mut out, "error", config.freshness.error);
    out.push_str("}\n\n");

    out.push_str("config check {\n");
    if let Some(filter) = &config.check.default_filter {
        line_call(&mut out, "default_filter", &[filter]);
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
            line_call(&mut out, "history_mode", &[value]);
        }
        out.push_str("}\n\n");
    }

    if !config.suppress.codes.is_empty() || !config.suppress.rules.is_empty() {
        out.push_str("config suppress {\n");
        if !config.suppress.codes.is_empty() {
            list_call(&mut out, "code", &config.suppress.codes);
        }
        for rule in &config.suppress.rules {
            line_call(
                &mut out,
                "rule",
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
        line_call(&mut out, "group", &values);
        out.push_str("}\n\n");
    }

    if !config.impact.traverse.is_empty() {
        out.push_str("config impact {\n");
        list_call(&mut out, "traverse", &config.impact.traverse);
        out.push_str("}\n\n");
    }

    out.push_str("config areas {\n");
    scalar_call(&mut out, "orphan_threshold", config.areas.orphan_threshold);
    out.push_str("}\n\n");

    out.push_str("config temporal {\n");
    scalar_call(&mut out, "recent_days", config.temporal.recent_days);
    out.push_str("}\n\n");

    out.push_str("config orient {\n");
    scalar_call(&mut out, "edge_weight", config.orient.edge_weight);
    scalar_call(&mut out, "label_weight", config.orient.label_weight);
    scalar_call(&mut out, "recency_weight", config.orient.recency_weight);
    scalar_call(
        &mut out,
        "recency_half_life_days",
        config.orient.recency_half_life_days,
    );
    line_call(&mut out, "budget", &[config.orient.budget.as_str()]);
    scalar_call(&mut out, "depth", config.orient.depth);
    if !config.orient.pin.is_empty() {
        list_call(&mut out, "pin", &config.orient.pin);
    }
    if !config.orient.exclude.is_empty() {
        list_call(&mut out, "exclude", &config.orient.exclude);
    }
    scalar_call(&mut out, "stub_bytes", config.orient.stub_bytes);
    scalar_call(
        &mut out,
        "curated_hub_weight",
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
}
