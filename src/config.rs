use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
