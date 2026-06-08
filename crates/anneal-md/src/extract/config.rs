//! Markdown adapter configuration and migration parsing.

use std::collections::HashMap;
use std::path::Path;

use anneal_core::runtime::{CallArg, Expr, Literal, NumberLiteral, Statement, parse_program};
use anneal_core::{
    ConfigEntry, ConfigFacts, RuntimeConfigLifecycle, runtime_config_declaration_for,
};
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

/// Configuration for code-path reference extraction in markdown bodies.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CodePathRootConfig {
    /// Additional root prefixes recognized as in-repo code references.
    pub(crate) root: Vec<String>,
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnnealConfig {
    /// Root directory to scan (defaults to nearest `.design/`, `docs/`, or `anneal.dl` upward).
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
    /// Project-specific code path roots for markdown body reference extraction.
    pub(crate) code_path_root: CodePathRootConfig,
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

/// Configuration for the convergence lattice (active/terminal partition).
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ConvergenceConfig {
    /// Status values considered active (in-progress, not yet settled).
    pub(crate) active: Vec<String>,
    /// Status values considered terminal (settled, no further work expected).
    pub(crate) terminal: Vec<String>,
    /// Status values that claim facts about this corpus's current code.
    pub(crate) asserts_code: Vec<String>,
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
    /// Namespace prefixes forced as real label namespaces despite sparse evidence.
    #[serde(alias = "confirmed")]
    pub(crate) force: Vec<String>,
    /// Namespace prefixes rejected (false positives like SHA, AVX).
    pub(crate) rejected: Vec<String>,
    /// Namespace prefixes whose handles are linear (must be discharged exactly once).
    pub(crate) linear: Vec<String>,
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
    load_unified_config_with_mode(root, ObsoleteConfirmedMode::Reject)
}

pub(crate) fn load_unified_config_for_init(root: &Path) -> Result<AnnealConfig> {
    load_unified_config_with_mode(root, ObsoleteConfirmedMode::Drop)
}

fn load_unified_config_with_mode(
    root: &Path,
    obsolete_confirmed: ObsoleteConfirmedMode,
) -> Result<AnnealConfig> {
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
    parse_unified_config_with_mode(&path.display().to_string(), &content, obsolete_confirmed)
        .with_context(|| format!("failed to parse config declarations in {}", path.display()))
}

#[cfg(test)]
fn parse_unified_config(source_name: &str, content: &str) -> Result<AnnealConfig> {
    parse_unified_config_with_mode(source_name, content, ObsoleteConfirmedMode::Reject)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObsoleteConfirmedMode {
    Reject,
    Drop,
}

fn parse_unified_config_with_mode(
    source_name: &str,
    content: &str,
    obsolete_confirmed: ObsoleteConfirmedMode,
) -> Result<AnnealConfig> {
    let program = parse_program(source_name, content)?;
    let mut config = AnnealConfig::default();
    let mut runtime_entries = Vec::new();
    let mut markdown_source_declarations = Vec::new();
    for statement in program.statements {
        match statement {
            Statement::ConfigBlock(block) => {
                for declaration in block.declarations {
                    if obsolete_confirmed == ObsoleteConfirmedMode::Drop
                        && block.section.as_str() == "handles"
                        && declaration.name.as_str() == "confirmed"
                    {
                        continue;
                    }
                    let values = declaration_values(&declaration.args).with_context(|| {
                        format!("invalid config declaration '{}'", declaration.name)
                    })?;
                    runtime_entries.extend(config_declaration_entries(
                        block.section.as_str(),
                        declaration.name.as_str(),
                        values,
                    )?);
                }
            }
            // Compatibility with pre-source-block configs: markdown source
            // declarations project into this adapter's native config.
            Statement::SourceBlock(block) if block.source.as_str() == "md" => {
                for declaration in block.declarations {
                    let values = declaration_values(&declaration.args).with_context(|| {
                        format!("invalid source declaration '{}'", declaration.name)
                    })?;
                    markdown_source_declarations.push((declaration.name, values));
                }
            }
            Statement::SourceBlock(block) => {
                // Non-markdown adapters are handled by the runtime surface; this
                // extractor only projects markdown config.
                anyhow::bail!("unknown source block source {} {{ ... }}", block.source);
            }
            _ => {}
        }
    }
    let runtime_facts = ConfigFacts::try_from_entries(runtime_entries)
        .context("runtime config declarations contain duplicate ordered entries")?;
    apply_runtime_config_facts(&mut config, &runtime_facts)?;
    for (name, values) in markdown_source_declarations {
        apply_markdown_source_declaration(&mut config, name.as_str(), values)?;
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

fn config_declaration_entries(
    section: &str,
    name: &str,
    values: Vec<String>,
) -> Result<Vec<ConfigEntry>> {
    let Some(declaration) = runtime_config_declaration_for(section, name) else {
        anyhow::bail!("unknown config declaration config {section} {{ {name}(...) }}");
    };
    if declaration.lifecycle() == RuntimeConfigLifecycle::ObsoleteConfirmedNamespace {
        anyhow::bail!(
            "config handles confirmed(...) is no longer valid. Label namespaces are inferred automatically; delete this declaration, or use config handles force([...]) only for sparse prefixes that need an explicit override. Run `anneal init --dry-run` to preview the repaired config"
        );
    }
    declaration
        .entries(values)
        .with_context(|| format!("invalid config declaration config {section} {{ {name}(...) }}"))
}

fn one_string(key: impl std::fmt::Display, values: Vec<String>) -> Result<String> {
    let [value]: [String; 1] = values.try_into().map_err(|values: Vec<String>| {
        anyhow::anyhow!("{key} expects exactly one value; got {}", values.len())
    })?;
    Ok(value)
}

fn declaration_values(args: &[CallArg]) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for arg in args {
        let Some(Expr::Literal(literal)) = arg.expr() else {
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

pub(crate) fn apply_runtime_config_facts(
    config: &mut AnnealConfig,
    facts: &ConfigFacts,
) -> Result<()> {
    config.root = facts.first("corpus.root").unwrap_or_default().to_string();
    config.exclude = facts.values("corpus.exclude").map(str::to_string).collect();
    config.convergence.ordering = facts
        .values("convergence.ordering")
        .map(str::to_string)
        .collect();
    config.convergence.active = facts
        .values("convergence.active")
        .map(str::to_string)
        .collect();
    config.convergence.terminal = facts
        .values("convergence.terminal")
        .map(str::to_string)
        .collect();
    config.convergence.asserts_code = facts
        .values("convergence.asserts_code")
        .map(str::to_string)
        .collect();
    config.handles.force = facts.values("handles.force").map(str::to_string).collect();
    config.handles.rejected = facts
        .values("handles.rejected")
        .map(str::to_string)
        .collect();
    config.handles.linear = facts.values("handles.linear").map(str::to_string).collect();
    config.suppress.codes = facts.values("suppress.code").map(str::to_string).collect();
    config.impact.traverse = facts
        .values("impact.traverse")
        .map(str::to_string)
        .collect();
    config.orient.pin = facts.values("orient.pin").map(str::to_string).collect();
    config.orient.exclude = facts.values("orient.exclude").map(str::to_string).collect();
    config.code_path_root.root = facts
        .values("code_path_root.root")
        .map(str::to_string)
        .collect();

    apply_first_u32(facts, "freshness.warn", &mut config.freshness.warn)?;
    apply_first_u32(facts, "freshness.error", &mut config.freshness.error)?;
    apply_first_string(
        facts,
        "check.default_filter",
        &mut config.check.default_filter,
    );
    apply_history_mode(facts, &mut config.state.history_mode)?;
    apply_first_usize(
        facts,
        "areas.orphan_threshold",
        &mut config.areas.orphan_threshold,
    )?;
    apply_first_u32(
        facts,
        "temporal.recent_days",
        &mut config.temporal.recent_days,
    )?;
    apply_first_f64(facts, "orient.edge_weight", &mut config.orient.edge_weight)?;
    apply_first_f64(
        facts,
        "orient.label_weight",
        &mut config.orient.label_weight,
    )?;
    apply_first_f64(
        facts,
        "orient.recency_weight",
        &mut config.orient.recency_weight,
    )?;
    apply_first_u32(
        facts,
        "orient.recency_half_life_days",
        &mut config.orient.recency_half_life_days,
    )?;
    apply_first_plain_string(facts, "orient.budget", &mut config.orient.budget);
    apply_first_u32(facts, "orient.depth", &mut config.orient.depth)?;
    apply_first_u32(facts, "orient.stub_bytes", &mut config.orient.stub_bytes)?;
    apply_first_f64(
        facts,
        "orient.curated_hub_weight",
        &mut config.orient.curated_hub_weight,
    )?;

    for entry in facts.entries() {
        if let Some(status) = entry.key.strip_prefix("convergence.description.") {
            config
                .convergence
                .descriptions
                .insert(status.to_string(), entry.value.clone());
        } else if let Some(code) = entry.key.strip_prefix("suppress.rule.") {
            config.suppress.rules.push(SuppressRule {
                code: code.to_string(),
                target: entry.value.clone(),
            });
        } else if let Some(name) = entry.key.strip_prefix("concerns.group.") {
            config
                .concerns
                .entry(name.to_string())
                .or_default()
                .push(entry.value.clone());
        }
    }

    apply_frontmatter_fields(config, facts)?;
    Ok(())
}

fn apply_frontmatter_fields(config: &mut AnnealConfig, facts: &ConfigFacts) -> Result<()> {
    let mut fields = std::collections::BTreeMap::<String, (Option<String>, Option<String>)>::new();
    for entry in facts.entries() {
        let Some(rest) = entry.key.strip_prefix("frontmatter.field.") else {
            continue;
        };
        let Some((field, property)) = rest.rsplit_once('.') else {
            continue;
        };
        let slot = fields.entry(field.to_string()).or_default();
        match property {
            "edge_kind" => slot.0 = Some(entry.value.clone()),
            "direction" => slot.1 = Some(entry.value.clone()),
            _ => {}
        }
    }

    for (field, (edge_kind, direction)) in fields {
        let Some(edge_kind) = edge_kind else {
            anyhow::bail!("frontmatter.field.{field} is missing edge_kind");
        };
        let Some(direction) = direction else {
            anyhow::bail!("frontmatter.field.{field} is missing direction");
        };
        let direction = match direction.as_str() {
            "forward" => Direction::Forward,
            "inverse" => Direction::Inverse,
            other => anyhow::bail!(
                "frontmatter.field.{field}.direction must be forward or inverse; got {other:?}"
            ),
        };
        config.frontmatter.fields.insert(
            field,
            FrontmatterFieldMapping {
                edge_kind,
                direction,
            },
        );
    }

    Ok(())
}

fn apply_first_string(facts: &ConfigFacts, key: &str, target: &mut Option<String>) {
    if let Some(value) = facts.first(key) {
        *target = Some(value.to_string());
    }
}

fn apply_first_plain_string(facts: &ConfigFacts, key: &str, target: &mut String) {
    if let Some(value) = facts.first(key) {
        *target = value.to_string();
    }
}

fn apply_history_mode(facts: &ConfigFacts, target: &mut Option<HistoryMode>) -> Result<()> {
    let Some(value) = facts.first("state.history_mode") else {
        return Ok(());
    };
    *target = Some(match value {
        "xdg" => HistoryMode::Xdg,
        "repo" => HistoryMode::Repo,
        "off" => HistoryMode::Off,
        other => anyhow::bail!("state.history_mode must be xdg, repo, or off; got {other:?}"),
    });
    Ok(())
}

fn apply_first_u32(facts: &ConfigFacts, key: &str, target: &mut u32) -> Result<()> {
    if let Some(value) = facts.first(key) {
        *target = value
            .parse::<u32>()
            .with_context(|| format!("{key} expects an unsigned integer"))?;
    }
    Ok(())
}

fn apply_first_usize(facts: &ConfigFacts, key: &str, target: &mut usize) -> Result<()> {
    if let Some(value) = facts.first(key) {
        *target = value
            .parse::<usize>()
            .with_context(|| format!("{key} expects an unsigned integer"))?;
    }
    Ok(())
}

fn apply_first_f64(facts: &ConfigFacts, key: &str, target: &mut f64) -> Result<()> {
    if let Some(value) = facts.first(key) {
        *target = value
            .parse::<f64>()
            .with_context(|| format!("{key} expects a number"))?;
    }
    Ok(())
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
    fn unified_datalog_config_parses_to_markdown_config() {
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
              asserts_code(["draft"]).
              description("draft", "needs work").
            }

            config handles {
              force(["OQ", "CR-D"]).
              linear(["OQ"]).
            }

            config frontmatter {
              field("relates-to", "Cites", "forward").
            }

            config suppress {
              code(["I001"]).
              rule("E001", "synthesis/v17.md").
            }

            config concerns {
              group("runtime", "CR-D*", "LR-*").
            }
            "#,
        )
        .expect("unified config parses");

        assert_eq!(config.exclude, ["feedback"]);
        assert_eq!(config.convergence.ordering, ["raw", "draft", "current"]);
        assert_eq!(config.convergence.active, ["draft", "current"]);
        assert_eq!(config.convergence.terminal, ["archived"]);
        assert_eq!(config.convergence.asserts_code, ["draft"]);
        assert_eq!(
            config
                .convergence
                .descriptions
                .get("draft")
                .map(String::as_str),
            Some("needs work")
        );
        assert_eq!(config.handles.force, ["OQ", "CR-D"]);
        assert_eq!(config.handles.linear, ["OQ"]);
        let frontmatter = config
            .frontmatter
            .fields
            .get("relates-to")
            .expect("frontmatter field");
        assert_eq!(frontmatter.edge_kind, "Cites");
        assert!(matches!(frontmatter.direction, Direction::Forward));
        assert_eq!(config.suppress.codes, ["I001"]);
        assert_eq!(config.suppress.rules.len(), 1);
        assert_eq!(config.suppress.rules[0].code, "E001");
        assert_eq!(config.suppress.rules[0].target, "synthesis/v17.md");
        assert_eq!(
            config.concerns.get("runtime").expect("concern group"),
            &["CR-D*", "LR-*"]
        );
    }

    #[test]
    fn unified_datalog_config_rejects_confirmed_with_upgrade_hint() {
        let err = parse_unified_config(
            "anneal.dl",
            r#"
            config handles {
              confirmed(["OQ"]).
            }
            "#,
        )
        .expect_err("confirmed is not a unified config declaration");

        let msg = format!("{err:#}");
        assert!(msg.contains("confirmed(...) is no longer valid"), "{msg}");
        assert!(err.to_string().contains("inferred automatically"));
        assert!(err.to_string().contains("force"));
    }

    #[test]
    fn unified_datalog_config_for_init_drops_confirmed_inventory() {
        let config = parse_unified_config_with_mode(
            "anneal.dl",
            r#"
            config handles {
              confirmed(["OQ", "REQ"]).
              linear(["OQ"]).
              rejected(["SHA"]).
              force(["SPARSE"]).
            }
            "#,
            ObsoleteConfirmedMode::Drop,
        )
        .expect("init repair mode drops confirmed");

        assert_eq!(config.handles.force, ["SPARSE"]);
        assert_eq!(config.handles.linear, ["OQ"]);
        assert_eq!(config.handles.rejected, ["SHA"]);
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
}
