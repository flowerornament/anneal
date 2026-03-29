use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Direction of an edge created from a frontmatter field.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    /// Source file -> referenced target (e.g., "depends-on: X" means this file `DependsOn` X).
    Forward,
    /// Referenced target -> source file (e.g., "affects: X" means X `DependsOn` this file).
    Inverse,
}

/// Mapping from a frontmatter field name to an edge kind and direction.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct FrontmatterFieldMapping {
    pub(crate) edge_kind: String,
    pub(crate) direction: Direction,
}

/// Configuration for frontmatter field-to-edge mapping (D-05, D-06, CONFIG-03).
///
/// Does NOT use `deny_unknown_fields` because the `fields` map has arbitrary
/// user-defined keys.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct FrontmatterConfig {
    pub(crate) fields: HashMap<String, FrontmatterFieldMapping>,
}

impl Default for FrontmatterConfig {
    fn default() -> Self {
        let mut fields = HashMap::new();
        fields.insert(
            "superseded-by".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "Supersedes".to_string(),
                direction: Direction::Forward,
            },
        );
        fields.insert(
            "depends-on".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "DependsOn".to_string(),
                direction: Direction::Forward,
            },
        );
        fields.insert(
            "discharges".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "Discharges".to_string(),
                direction: Direction::Forward,
            },
        );
        fields.insert(
            "verifies".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "Verifies".to_string(),
                direction: Direction::Forward,
            },
        );
        fields.insert(
            "supersedes".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "Supersedes".to_string(),
                direction: Direction::Inverse,
            },
        );
        fields.insert(
            "affects".to_string(),
            FrontmatterFieldMapping {
                edge_kind: "DependsOn".to_string(),
                direction: Direction::Inverse,
            },
        );
        Self { fields }
    }
}

/// Top-level configuration from `anneal.toml`.
///
/// All fields use concrete types with `Default` impls -- no `Option<T>` wrapping.
/// An absent `anneal.toml` is a valid coloring (zero-config case, KB-P3).
/// `deny_unknown_fields` catches config typos early.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnnealConfig {
    /// Root directory to scan (defaults to inferred: `.design/` > `docs/` > `.`).
    pub(crate) root: String,
    /// Additional directories to exclude beyond defaults.
    pub(crate) exclude: Vec<String>,
    /// Convergence lattice configuration.
    pub(crate) convergence: ConvergenceConfig,
    /// Handle namespace configuration.
    pub(crate) handles: HandlesConfig,
    /// Freshness threshold configuration.
    pub(crate) freshness: FreshnessConfig,
    /// Extensible frontmatter field-to-edge mapping (CONFIG-03).
    pub(crate) frontmatter: FrontmatterConfig,
    /// Concern groups mapping name -> list of handle patterns.
    #[serde(default)]
    pub(crate) concerns: HashMap<String, Vec<String>>,
}

/// Configuration for the convergence lattice (active/terminal partition).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ConvergenceConfig {
    /// Status values considered active (in-progress, not yet settled).
    pub(crate) active: Vec<String>,
    /// Status values considered terminal (settled, no further work expected).
    pub(crate) terminal: Vec<String>,
    /// Optional ordering for pipeline flow analysis.
    pub(crate) ordering: Vec<String>,
}

/// Configuration for handle namespace recognition.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HandlesConfig {
    /// Namespace prefixes confirmed as real label namespaces.
    pub(crate) confirmed: Vec<String>,
    /// Namespace prefixes rejected (false positives like SHA, AVX).
    pub(crate) rejected: Vec<String>,
    /// Namespace prefixes whose handles are linear (must be discharged exactly once).
    pub(crate) linear: Vec<String>,
}

impl HandlesConfig {
    pub(crate) fn linear_set(&self) -> HashSet<&str> {
        self.linear.iter().map(String::as_str).collect()
    }

    pub(crate) fn confirmed_set(&self) -> HashSet<&str> {
        self.confirmed.iter().map(String::as_str).collect()
    }
}

/// Configuration for freshness thresholds.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FreshnessConfig {
    /// Days before a file's age triggers a warning.
    pub(crate) warn: u32,
    /// Days before a file's age triggers an error.
    pub(crate) error: u32,
}

impl Default for FreshnessConfig {
    fn default() -> Self {
        Self {
            warn: 30,
            error: 90,
        }
    }
}

/// Load configuration from `anneal.toml` at the given root path.
///
/// Returns `Ok(AnnealConfig::default())` if the file does not exist (CONFIG-02:
/// zero-config is valid). Returns an error on malformed TOML.
pub(crate) fn load_config(root: &Path) -> Result<AnnealConfig> {
    let config_path = root.join("anneal.toml");

    if !config_path.exists() {
        return Ok(AnnealConfig::default());
    }

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let config: AnnealConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    Ok(config)
}
