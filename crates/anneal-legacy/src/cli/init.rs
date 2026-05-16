use std::collections::HashMap;

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
#[derive(Serialize)]
pub(crate) struct InitOutput {
    pub(crate) config: AnnealConfig,
    pub(crate) written: bool,
    pub(crate) path: String,
}

/// Frontmatter keys that are metadata-only (not edge-producing references).
const METADATA_ONLY_KEYS: &[&str] = &["status", "updated", "title", "description", "tags", "date"];

impl Render for InitOutput {
    fn render(&self, p: &mut Printer) -> std::io::Result<()> {
        let toml_str =
            toml::to_string_pretty(&self.config).unwrap_or_else(|e| format!("# error: {e}"));
        if self.written {
            p.line(
                &Line::new()
                    .success("✓ ")
                    .text("Wrote ")
                    .path(self.path.clone()),
            )?;
        } else {
            p.heading("anneal.toml", None)?;
            p.caption("dry run — not written")?;
        }
        p.blank()?;
        // TOML body is config syntax; emit raw so it parses if the user
        // pipes it into a file.
        for line in toml_str.lines() {
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
pub(crate) fn cmd_init(
    root: &Utf8Path,
    lattice: &Lattice,
    stats: &ResolveStats,
    observed_frontmatter_keys: &HashMap<String, usize>,
    dry_run: bool,
) -> anyhow::Result<InitOutput> {
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

    // Build handles section from namespaces
    let mut confirmed: Vec<String> = stats.namespaces.iter().cloned().collect();
    confirmed.sort();

    let handles = HandlesConfig {
        confirmed,
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

    let config = AnnealConfig {
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

    let config_path = root.join("anneal.toml");
    let path_str = config_path.to_string();

    let written = if dry_run {
        false
    } else {
        let toml_str = toml::to_string_pretty(&config)?;
        std::fs::write(&config_path, toml_str)?;
        true
    };

    Ok(InitOutput {
        config,
        written,
        path: path_str,
    })
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

        let output = cmd_init(root, &lattice, &stats, &HashMap::new(), false).expect("init writes");

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
        assert!(root.join("anneal.toml").exists());
    }
}
