use std::collections::{HashMap, HashSet};
use std::path::Path;

use anneal_core::runtime::{CallArg, Expr, Literal, NumberLiteral, Statement, parse_program};
use anneal_core::runtime_config_declaration_for;
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// Files smaller than this (in bytes) are treated as stubs and
    /// excluded from orient unless they're curated hubs (basename match,
    /// `status: living`, or `purpose:` containing an orientation cue).
    /// Raise if the corpus has legitimately short standalone specs;
    /// lower if redirect stubs leak into Foundation.
    pub(crate) stub_bytes: u32,
    /// Additive score bonus for curated hubs in Foundation. Sized to
    /// compete with heavy recency-weighted centrality — a `README.md`
    /// should beat a 50-citation hub by default. Raise if curated hubs
    /// still get outranked.
    pub(crate) curated_hub_weight: f64,
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
            stub_bytes: 1000,
            curated_hub_weight: 10.0,
        }
    }
}

/// Top-level repository configuration.
///
/// An absent repo config is a valid coloring (zero-config case, KB-P3).
/// `deny_unknown_fields` catches config typos early.
/// Impact analysis configuration.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// Load repository configuration at the given root path.
///
/// Returns `Ok(AnnealConfig::default())` if the file does not exist (CONFIG-02:
/// zero-config is valid). Returns an error on malformed TOML.
pub(crate) fn load_config(root: &Path) -> Result<AnnealConfig> {
    let Some(config) = load_legacy_config(root)? else {
        return load_unified_config(root);
    };

    if unified_config_declared(root)? {
        anyhow::bail!(
            "both anneal.toml and anneal.dl config blocks are present; keep one repo config authority. Run `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
        );
    }

    Ok(config)
}

pub(crate) fn load_legacy_config(root: &Path) -> Result<Option<AnnealConfig>> {
    let config_path = root.join("anneal.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };

    toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))
        .map(Some)
}

fn unified_config_declared(root: &Path) -> Result<bool> {
    let path = root.join("anneal.dl");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let program = parse_program(&path.display().to_string(), &content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(program
        .statements
        .iter()
        .any(|statement| matches!(statement, Statement::ConfigBlock(_))))
}

fn load_unified_config(root: &Path) -> Result<AnnealConfig> {
    let path = root.join("anneal.dl");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AnnealConfig::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    parse_unified_config(&path.display().to_string(), &content)
        .with_context(|| format!("failed to parse config declarations in {}", path.display()))
}

fn parse_unified_config(source_name: &str, content: &str) -> Result<AnnealConfig> {
    let program = parse_program(source_name, content)?;
    let mut config = AnnealConfig::default();
    for statement in program.statements {
        match statement {
            Statement::ConfigBlock(block) => {
                for declaration in block.declarations {
                    let values = declaration_values(&declaration.args).with_context(|| {
                        format!("invalid config declaration '{}'", declaration.name)
                    })?;
                    apply_config_declaration(
                        &mut config,
                        block.section.as_str(),
                        declaration.name.as_str(),
                        values,
                    )?;
                }
            }
            // v1-parity compatibility only: the legacy command set can project
            // markdown source config, but future adapters stay on the runtime path.
            Statement::SourceBlock(block) if block.source.as_str() == "md" => {
                for declaration in block.declarations {
                    let values = declaration_values(&declaration.args).with_context(|| {
                        format!("invalid source declaration '{}'", declaration.name)
                    })?;
                    apply_markdown_source_declaration(
                        &mut config,
                        declaration.name.as_str(),
                        values,
                    )?;
                }
            }
            Statement::SourceBlock(block) => {
                // The legacy command set is markdown-only. Future adapters are
                // handled by the runtime surface rather than this v1 bridge.
                anyhow::bail!("unknown source block source {} {{ ... }}", block.source);
            }
            _ => {}
        }
    }
    Ok(config)
}

fn apply_markdown_source_declaration(
    config: &mut AnnealConfig,
    name: &str,
    values: Vec<String>,
) -> Result<()> {
    match name {
        "scan_root" => config.root = one_string("source md.scan_root", values)?,
        "scan_exclude" => config.exclude.extend(values),
        "file_extension" | "label_pattern" | "linear_namespace" | "version_pattern" => {}
        _ => anyhow::bail!("unknown source declaration source md {{ {name}(...) }}"),
    }
    Ok(())
}

fn apply_config_declaration(
    config: &mut AnnealConfig,
    section: &str,
    name: &str,
    values: Vec<String>,
) -> Result<()> {
    if !matches!(
        (section, name),
        ("convergence", "description")
            | ("frontmatter", "field")
            | ("suppress", "rule")
            | ("concerns", "group")
    ) && runtime_config_declaration_for(section, name).is_none()
    {
        anyhow::bail!("unknown config declaration config {section} {{ {name}(...) }}");
    }

    match (section, name) {
        ("corpus", "exclude") => config.exclude = values,
        ("corpus", "root") => config.root = one_string("corpus.root", values)?,
        ("convergence", "active") => config.convergence.active = values,
        ("convergence", "terminal") => config.convergence.terminal = values,
        ("convergence", "ordering") => config.convergence.ordering = values,
        ("convergence", "description") => {
            if values.len() != 2 {
                anyhow::bail!("convergence.description expects status and description");
            }
            config
                .convergence
                .descriptions
                .insert(values[0].clone(), values[1].clone());
        }
        ("handles", "confirmed") => config.handles.confirmed = values,
        ("handles", "rejected") => config.handles.rejected = values,
        ("handles", "linear") => config.handles.linear = values,
        ("frontmatter", "field") => {
            if values.len() != 3 {
                anyhow::bail!("frontmatter.field expects key, edge_kind, and direction");
            }
            config.frontmatter.fields.insert(
                values[0].clone(),
                FrontmatterFieldMapping {
                    edge_kind: values[1].clone(),
                    direction: match values[2].as_str() {
                        "forward" => Direction::Forward,
                        "inverse" => Direction::Inverse,
                        other => anyhow::bail!(
                            "frontmatter.field direction must be forward or inverse; got {other:?}"
                        ),
                    },
                },
            );
        }
        ("freshness", "warn") => config.freshness.warn = one_u32("freshness.warn", values)?,
        ("freshness", "error") => config.freshness.error = one_u32("freshness.error", values)?,
        ("state", "history_mode") => {
            config.state.history_mode =
                Some(match one_string("state.history_mode", values)?.as_str() {
                    "xdg" => HistoryMode::Xdg,
                    "repo" => HistoryMode::Repo,
                    "off" => HistoryMode::Off,
                    other => {
                        anyhow::bail!("state.history_mode must be xdg, repo, or off; got {other:?}")
                    }
                });
        }
        ("check", "default_filter") => {
            config.check.default_filter = Some(one_string("check.default_filter", values)?);
        }
        ("suppress", "code") => config.suppress.codes = values,
        ("suppress", "rule") => {
            if values.len() != 2 {
                anyhow::bail!("suppress.rule expects code and target");
            }
            config.suppress.rules.push(SuppressRule {
                code: values[0].clone(),
                target: values[1].clone(),
            });
        }
        ("concerns", "group") => {
            if values.len() < 2 {
                anyhow::bail!("concerns.group expects a name and one or more patterns");
            }
            config
                .concerns
                .insert(values[0].clone(), values[1..].to_vec());
        }
        ("impact", "traverse") => config.impact.traverse = values,
        ("areas", "orphan_threshold") => {
            config.areas.orphan_threshold = one_string("areas.orphan_threshold", values)?
                .parse::<usize>()
                .with_context(|| "areas.orphan_threshold expects an unsigned integer")?;
        }
        ("temporal", "recent_days") => {
            config.temporal.recent_days = one_u32("temporal.recent_days", values)?;
        }
        ("orient", "edge_weight") => {
            config.orient.edge_weight = one_f64("orient.edge_weight", values)?;
        }
        ("orient", "label_weight") => {
            config.orient.label_weight = one_f64("orient.label_weight", values)?;
        }
        ("orient", "recency_weight") => {
            config.orient.recency_weight = one_f64("orient.recency_weight", values)?;
        }
        ("orient", "recency_half_life_days") => {
            config.orient.recency_half_life_days =
                one_u32("orient.recency_half_life_days", values)?;
        }
        ("orient", "budget") => config.orient.budget = one_string("orient.budget", values)?,
        ("orient", "depth") => config.orient.depth = one_u32("orient.depth", values)?,
        ("orient", "pin") => config.orient.pin = values,
        ("orient", "exclude") => config.orient.exclude = values,
        ("orient", "stub_bytes") => {
            config.orient.stub_bytes = one_u32("orient.stub_bytes", values)?;
        }
        ("orient", "curated_hub_weight") => {
            config.orient.curated_hub_weight = one_f64("orient.curated_hub_weight", values)?;
        }
        _ => {
            anyhow::bail!("unknown config declaration config {section} {{ {name}(...) }}");
        }
    }
    Ok(())
}

fn one_string(key: &str, values: Vec<String>) -> Result<String> {
    let [value]: [String; 1] = values.try_into().map_err(|values: Vec<String>| {
        anyhow::anyhow!("{key} expects exactly one value; got {}", values.len())
    })?;
    Ok(value)
}

fn one_u32(key: &str, values: Vec<String>) -> Result<u32> {
    let value = one_string(key, values)?;
    value
        .parse::<u32>()
        .with_context(|| format!("{key} expects an unsigned integer"))
}

fn one_f64(key: &str, values: Vec<String>) -> Result<f64> {
    let value = one_string(key, values)?;
    value
        .parse::<f64>()
        .with_context(|| format!("{key} expects a number"))
}

fn declaration_values(args: &[CallArg]) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for arg in args {
        let Expr::Literal(literal) = arg.expr() else {
            anyhow::bail!("values must be static literals");
        };
        push_literal_values(&mut values, literal);
    }
    Ok(values)
}

fn push_literal_values(out: &mut Vec<String>, literal: &Literal) {
    match literal {
        Literal::String(value) => out.push(value.clone()),
        Literal::Number(NumberLiteral::Int(value)) => out.push(value.to_string()),
        Literal::Number(NumberLiteral::Float(value)) => out.push(value.to_string()),
        Literal::Bool(value) => out.push(value.to_string()),
        Literal::Null => out.push("null".to_string()),
        Literal::List(items) => {
            for item in items {
                push_literal_values(out, item);
            }
        }
    }
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
    fn unified_datalog_config_parses_to_legacy_config() {
        let config = parse_unified_config(
            "anneal.dl",
            r#"
            source md {
              scan_exclude(["feedback"]).
            }

            config convergence {
              ordering(["raw", "draft", "current"]).
              active(["draft", "current"]).
              terminal(["archived"]).
            }

            config handles {
              confirmed(["OQ", "CR-D"]).
              linear(["OQ"]).
            }

            config suppress {
              rule("E001", "synthesis/v17.md").
            }
            "#,
        )
        .expect("unified config parses");

        assert_eq!(config.exclude, ["feedback"]);
        assert_eq!(config.convergence.ordering, ["raw", "draft", "current"]);
        assert_eq!(config.convergence.active, ["draft", "current"]);
        assert_eq!(config.convergence.terminal, ["archived"]);
        assert_eq!(config.handles.confirmed, ["OQ", "CR-D"]);
        assert_eq!(config.handles.linear, ["OQ"]);
        assert_eq!(config.suppress.rules.len(), 1);
        assert_eq!(config.suppress.rules[0].code, "E001");
        assert_eq!(config.suppress.rules[0].target, "synthesis/v17.md");
    }

    #[test]
    fn unified_datalog_config_rejects_unknown_source_blocks() {
        let err = parse_unified_config(
            "anneal.dl",
            r#"
            source host {
              endpoint("https://example.invalid").
            }
            "#,
        )
        .expect_err("unknown source block rejected");

        assert!(err.to_string().contains("unknown source block"));
    }

    #[test]
    fn load_config_rejects_two_repo_config_authorities() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("anneal.toml"),
            "[convergence]\nactive = [\"draft\"]\n",
        )
        .expect("write toml");
        std::fs::write(
            dir.path().join("anneal.dl"),
            "config convergence { active([\"current\"]). }",
        )
        .expect("write dl");

        let err = load_config(dir.path()).expect_err("two authorities rejected");

        assert!(err.to_string().contains("one repo config authority"));
    }

    #[test]
    fn load_legacy_config_reads_toml_even_when_unified_config_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("anneal.toml"),
            "[convergence]\nactive = [\"legacy-active\"]\n",
        )
        .expect("write toml");
        std::fs::write(
            dir.path().join("anneal.dl"),
            "config convergence { active([\"unified-active\"]). }",
        )
        .expect("write dl");

        let config = load_legacy_config(dir.path())
            .expect("legacy config loads")
            .expect("legacy config present");

        assert_eq!(config.convergence.active, ["legacy-active"]);
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
