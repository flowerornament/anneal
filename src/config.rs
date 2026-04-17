use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
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

/// Suppression rules for known false positives in diagnostics.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct SuppressConfig {
    /// Diagnostic codes suppressed globally.
    pub(crate) codes: Vec<String>,
    /// Targeted suppressions for a specific code + target pair.
    pub(crate) rules: Vec<SuppressRule>,
}

/// A targeted suppression rule for one diagnostic code and target identity.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SuppressRule {
    pub(crate) code: String,
    pub(crate) target: String,
}

/// Configuration for the `anneal areas` command.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AreasConfig {
    /// Orphan count at or above this threshold downgrades an area to grade B.
    pub(crate) orphan_threshold: usize,
}

impl Default for AreasConfig {
    fn default() -> Self {
        Self {
            orphan_threshold: 5,
        }
    }
}

/// Configuration for temporal features (`--recent`, `--since`, file dates).
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct TemporalConfig {
    /// Default window in days for the `--recent` flag.
    pub(crate) recent_days: u32,
}

impl Default for TemporalConfig {
    fn default() -> Self {
        Self { recent_days: 7 }
    }
}

/// Configuration for the `anneal orient` command.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct OrientConfig {
    /// Weight applied to incoming+outgoing edge count in the score.
    pub(crate) edge_weight: f64,
    /// Weight applied to the label count contributed by the file.
    pub(crate) label_weight: f64,
    /// Weight applied to the recency bonus (0.0–1.0). The bonus decays
    /// exponentially with file age using `recency_half_life_days`.
    pub(crate) recency_weight: f64,
    /// Half-life of the recency bonus in days. A file this many days old
    /// contributes half as much recency as a file touched today. The default
    /// of 90 days treats the last quarter as "fresh" while letting older
    /// material fall off smoothly.
    pub(crate) recency_half_life_days: u32,
    /// Default token budget when `--budget` is omitted. Examples: "50k", "100k".
    pub(crate) budget: String,
    /// Traversal depth for `--file` upstream walks and cross-area tiers.
    pub(crate) depth: u32,
    /// Files always included first (pinned context).
    pub(crate) pin: Vec<String>,
    /// Files never included (glob or plain name, noise for agents).
    pub(crate) exclude: Vec<String>,
}

impl Default for OrientConfig {
    fn default() -> Self {
        Self {
            edge_weight: 1.0,
            label_weight: 1.0,
            // Up from 0.5 in 0.9.0. With exponential decay now anchored at
            // today, a recent file's bonus is meaningful against edge and
            // label scores instead of being rounding error against them.
            recency_weight: 5.0,
            recency_half_life_days: 90,
            budget: "50k".to_string(),
            depth: 3,
            pin: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

/// Top-level configuration from `anneal.toml`.
///
/// An absent `anneal.toml` is a valid coloring (zero-config case, KB-P3).
/// `deny_unknown_fields` catches config typos early.
/// Impact analysis configuration.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ImpactConfig {
    /// Edge kinds to traverse during impact analysis. When empty (default),
    /// falls back to the built-in set (DependsOn, Supersedes, Verifies).
    /// Values are parsed through `EdgeKind::from_name` (case-insensitive for
    /// well-known kinds).
    pub(crate) traverse: Vec<String>,
}

impl ImpactConfig {
    /// Resolve the configured traversal set to `EdgeKind` values.
    /// Returns the default set if `traverse` is empty.
    pub(crate) fn resolve_traverse_set(&self) -> Vec<crate::graph::EdgeKind> {
        if self.traverse.is_empty() {
            crate::impact::DEFAULT_TRAVERSE.to_vec()
        } else {
            self.traverse
                .iter()
                .map(|s| crate::graph::EdgeKind::from_name(s))
                .collect()
        }
    }
}

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
    /// Check command behavior configuration.
    pub(crate) check: CheckConfig,
    /// Known false-positive suppressions for diagnostics.
    pub(crate) suppress: SuppressConfig,
    /// Local runtime state preferences.
    pub(crate) state: StateConfig,
    /// Concern groups mapping name -> list of handle patterns.
    #[serde(default)]
    pub(crate) concerns: HashMap<String, Vec<String>>,
    /// Impact analysis configuration.
    pub(crate) impact: ImpactConfig,
    /// Area health grading thresholds.
    pub(crate) areas: AreasConfig,
    /// Temporal features configuration.
    pub(crate) temporal: TemporalConfig,
    /// Orient command configuration.
    pub(crate) orient: OrientConfig,
}

/// Where derived history should be stored.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryMode {
    Xdg,
    Repo,
    Off,
}

/// Repo-local or user-local state preferences.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct StateConfig {
    /// History backend preference. If absent, falls back to user config, then
    /// the built-in default (`xdg`).
    pub(crate) history_mode: Option<HistoryMode>,
    /// Optional override for the base directory used for XDG-style history.
    pub(crate) history_dir: Option<String>,
}

/// Machine-local user configuration loaded from XDG config.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UserConfig {
    pub(crate) state: StateConfig,
}

/// Fully resolved runtime state settings after applying precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedStateConfig {
    pub(crate) history_mode: HistoryMode,
    pub(crate) history_dir: Option<Utf8PathBuf>,
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
    /// Optional human-readable descriptions per status (status -> description).
    /// Used by `explain convergence` to annotate the pipeline display.
    pub(crate) descriptions: HashMap<String, String>,
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

/// Configuration for check command behavior.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CheckConfig {
    /// Default filter for `anneal check`. `"active-only"` skips diagnostics
    /// from terminal files; any other value keeps the full picture.
    pub(crate) default_filter: Option<String>,
}

impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            default_filter: Some("active-only".to_string()),
        }
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

    let content = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AnnealConfig::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };

    let config: AnnealConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    Ok(config)
}

/// Load machine-local user configuration from XDG config.
///
/// Search path:
/// - `$XDG_CONFIG_HOME/anneal/config.toml`
/// - `~/.config/anneal/config.toml`
///
/// Missing config is valid and resolves to defaults.
pub(crate) fn load_user_config() -> Result<UserConfig> {
    let Some(config_path) = user_config_path() else {
        return Ok(UserConfig::default());
    };

    let content = match std::fs::read_to_string(config_path.as_std_path()) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UserConfig::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", config_path.as_str()));
        }
    };

    toml::from_str(&content).with_context(|| format!("failed to parse {}", config_path.as_str()))
}

pub(crate) fn resolve_state_config(
    repo_config: &AnnealConfig,
    user_config: &UserConfig,
) -> ResolvedStateConfig {
    let history_mode = repo_config
        .state
        .history_mode
        .or(user_config.state.history_mode)
        .unwrap_or(HistoryMode::Xdg);

    // Machine-local storage paths come only from user config. Repo config may
    // choose the backend mode, but not an arbitrary location on the user's
    // machine.
    let history_dir = user_config
        .state
        .history_dir
        .as_deref()
        .map(Utf8PathBuf::from);

    ResolvedStateConfig {
        history_mode,
        history_dir,
    }
}

fn user_config_path() -> Option<Utf8PathBuf> {
    let base = if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        Utf8PathBuf::from_path_buf(dir.into()).ok()
    } else {
        std::env::var_os("HOME")
            .and_then(|home| Utf8PathBuf::from_path_buf(home.into()).ok())
            .map(|home| home.join(".config"))
    }?;

    Some(base.join("anneal/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_without_check_section_parses() {
        let toml_str = r#"
root = "docs"
exclude = [".git"]
"#;
        let config: AnnealConfig = toml::from_str(toml_str).expect("should parse without [check]");
        assert_eq!(
            config.check.default_filter.as_deref(),
            Some("active-only"),
            "default_filter should default to active-only when [check] section absent"
        );
    }

    #[test]
    fn config_with_check_active_only() {
        let toml_str = r#"
[check]
default_filter = "active-only"
"#;
        let config: AnnealConfig =
            toml::from_str(toml_str).expect("should parse with [check] section");
        assert_eq!(
            config.check.default_filter.as_deref(),
            Some("active-only"),
            "default_filter should be active-only"
        );
    }

    #[test]
    fn config_with_non_active_default_filter_is_preserved() {
        let toml_str = r#"
[check]
default_filter = "all"
"#;
        let config: AnnealConfig =
            toml::from_str(toml_str).expect("non-active default filter values should be accepted");
        assert_eq!(config.check.default_filter.as_deref(), Some("all"));
    }

    #[test]
    fn config_empty_parses_to_default() {
        let config: AnnealConfig = toml::from_str("").expect("empty TOML should parse");
        assert_eq!(config.check.default_filter.as_deref(), Some("active-only"));
        assert!(config.root.is_empty());
        assert!(config.suppress.codes.is_empty());
        assert!(config.suppress.rules.is_empty());
        assert_eq!(config.state.history_mode, None);
        assert_eq!(config.state.history_dir, None);
    }

    #[test]
    fn config_with_suppress_codes_parses() {
        let toml_str = r#"
[suppress]
codes = ["I001"]
"#;
        let config: AnnealConfig =
            toml::from_str(toml_str).expect("should parse with suppress codes");
        assert_eq!(config.suppress.codes, vec!["I001"]);
        assert!(config.suppress.rules.is_empty());
    }

    #[test]
    fn config_with_suppress_rule_parses() {
        let toml_str = r#"
[[suppress.rules]]
code = "E001"
target = "synthesis/v17.md"
"#;
        let config: AnnealConfig =
            toml::from_str(toml_str).expect("should parse with suppress rules");
        assert!(config.suppress.codes.is_empty());
        assert_eq!(config.suppress.rules.len(), 1);
        assert_eq!(config.suppress.rules[0].code, "E001");
        assert_eq!(config.suppress.rules[0].target, "synthesis/v17.md");
    }

    #[test]
    fn config_without_suppress_section_uses_default() {
        let toml_str = r#"
[check]
default_filter = "active-only"
"#;
        let config: AnnealConfig =
            toml::from_str(toml_str).expect("should parse without [suppress]");
        assert!(config.suppress.codes.is_empty());
        assert!(config.suppress.rules.is_empty());
    }

    #[test]
    fn config_with_state_section_parses() {
        let toml_str = r#"
[state]
history_mode = "repo"
history_dir = "/tmp/anneal-state"
"#;
        let config: AnnealConfig = toml::from_str(toml_str).expect("should parse with [state]");
        assert_eq!(config.state.history_mode, Some(HistoryMode::Repo));
        assert_eq!(
            config.state.history_dir.as_deref(),
            Some("/tmp/anneal-state")
        );
    }

    #[test]
    fn resolve_state_config_prefers_repo_over_user_over_default() {
        let repo = AnnealConfig {
            state: StateConfig {
                history_mode: Some(HistoryMode::Repo),
                history_dir: Some("/repo".to_string()),
            },
            ..AnnealConfig::default()
        };
        let user = UserConfig {
            state: StateConfig {
                history_mode: Some(HistoryMode::Off),
                history_dir: Some("/user".to_string()),
            },
        };

        let resolved = resolve_state_config(&repo, &user);
        assert_eq!(resolved.history_mode, HistoryMode::Repo);
        assert_eq!(
            resolved
                .history_dir
                .as_deref()
                .map(camino::Utf8Path::as_str),
            Some("/user")
        );
    }

    #[test]
    fn resolve_state_config_uses_user_when_repo_omits_state() {
        let repo = AnnealConfig::default();
        let user = UserConfig {
            state: StateConfig {
                history_mode: Some(HistoryMode::Off),
                history_dir: None,
            },
        };

        let resolved = resolve_state_config(&repo, &user);
        assert_eq!(resolved.history_mode, HistoryMode::Off);
        assert_eq!(resolved.history_dir, None);
    }
}
