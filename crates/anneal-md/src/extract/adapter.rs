//! Adapter entry points that lower markdown extraction into core facts.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Command;

use anneal_core::runtime::prelude::datalog_string_literal;
use anneal_core::target_probe::{
    CodeDriftEvidence, CodeDriftEvidenceCache, CodeDriftEvidenceMode, CodeDriftEvidenceRequest,
};
use anneal_core::{
    CodeTargetMeta, CodeTargetProbeCache, ConcernFact, ContentFact, EdgeFact, FactBatch,
    FactBatchMode, FactIdentity, Generation, HandleFact, MetaFact, NativeId, OriginUri, Revision,
    RuntimeConfigKey, SourceName, SpanFact, runtime_config_declaration_by_key,
};
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

use crate::extract::config;
use crate::extract::extraction::UnresolvedRefDisposition;
use crate::extract::graph::{DiGraph, EdgeKind};
use crate::extract::handle::{Handle, HandleKind, NodeId, resolved_file};
use crate::extract::parse::{self, PendingEdge};

#[derive(Clone, Debug, Default)]
pub struct MarkdownExtractionOptions {
    pub scan_roots: Vec<Utf8PathBuf>,
    pub exclude: Vec<String>,
    pub linear_namespaces: Vec<String>,
    pub probe_code_target_history: bool,
    pub read_code_drift_evidence: bool,
    pub refresh_code_drift_evidence: bool,
    pub probe_edge_assertions: bool,
}

#[derive(Clone, Debug)]
pub struct MarkdownConfig {
    config: config::AnnealConfig,
}

impl MarkdownConfig {
    pub fn from_runtime_facts(facts: &anneal_core::ConfigFacts) -> Result<Self> {
        let mut config = config::AnnealConfig::default();
        config::apply_runtime_config_facts(&mut config, facts)?;
        Ok(Self { config })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InitMode {
    DryRun,
    Write { force: bool },
}

impl InitMode {
    const fn dry_run(self) -> bool {
        matches!(self, Self::DryRun)
    }

    const fn force(self) -> bool {
        matches!(self, Self::Write { force: true })
    }
}

#[derive(Debug, Serialize)]
pub struct InitOutput {
    pub body: String,
    pub written: bool,
    pub path: String,
    pub backup_path: Option<String>,
}

pub fn render_or_write_init(root: &Utf8Path, mode: InitMode) -> Result<InitOutput> {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    let unified_exists = config_path.exists();
    let legacy_exists = legacy_path.exists();
    if matches!(mode, InitMode::Write { force: false }) && (unified_exists || legacy_exists) {
        anyhow::bail!("{}", existing_config_message(root));
    }
    if (mode.dry_run() || mode.force()) && unified_exists && !legacy_exists {
        let body = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {config_path}"))?;
        if mode.force() {
            write_unified_config(root, &body)?;
        }
        return Ok(InitOutput {
            body,
            written: mode.force(),
            path: config_path.to_string(),
            backup_path: None,
        });
    }
    if (mode.dry_run() || mode.force()) && (unified_exists || legacy_exists) {
        let config = if let Some(toml_config) = config::load_legacy_config(root.as_std_path())? {
            toml_config
        } else {
            config::load_unified_config_for_init(root.as_std_path())?
        };
        return render_or_write_init_from_config(root, config, mode);
    }

    let mut config = config::load_config(root.as_std_path())?;
    let result = parse::build_graph(root, &config)?;
    let observed_statuses = result
        .graph
        .nodes()
        .filter_map(|(_, handle)| handle.status.clone())
        .collect::<HashSet<_>>();
    let (active, terminal) =
        infer_lifecycle_partition(&observed_statuses, &config, &result.terminal_by_directory);
    config.convergence.active = active;
    config.convergence.terminal = terminal;
    if config.convergence.active.is_empty()
        && config.convergence.terminal.is_empty()
        && config.convergence.ordering.is_empty()
    {
        config.convergence.active = vec![
            "draft".to_string(),
            "current".to_string(),
            "stable".to_string(),
        ];
        config.convergence.terminal = vec!["superseded".to_string(), "archived".to_string()];
        config.convergence.ordering = vec![
            "raw".to_string(),
            "draft".to_string(),
            "current".to_string(),
            "stable".to_string(),
        ];
    }
    config.frontmatter.fields = inferred_frontmatter_fields(&result.observed_frontmatter_keys);
    render_or_write_init_from_config(root, config, mode)
}

#[cfg(test)]
pub fn extract_markdown_facts(
    root: &Utf8Path,
    corpus: anneal_core::CorpusId,
    source: SourceName,
    generation: Generation,
) -> Result<FactBatch> {
    extract_markdown_facts_with_options(
        root,
        corpus,
        source,
        generation,
        &MarkdownExtractionOptions::default(),
    )
}

pub fn extract_markdown_facts_with_options(
    root: &Utf8Path,
    corpus: anneal_core::CorpusId,
    source: SourceName,
    generation: Generation,
    options: &MarkdownExtractionOptions,
) -> Result<FactBatch> {
    let mut config = config::load_config(root.as_std_path())?;
    extract_markdown_facts_from_anneal_config(
        root,
        corpus,
        source,
        generation,
        &mut config,
        options,
    )
}

fn extract_markdown_facts_from_anneal_config(
    root: &Utf8Path,
    corpus: anneal_core::CorpusId,
    source: SourceName,
    generation: Generation,
    config: &mut config::AnnealConfig,
    options: &MarkdownExtractionOptions,
) -> Result<FactBatch> {
    config.exclude.extend(options.exclude.iter().cloned());
    config
        .handles
        .linear
        .extend(options.linear_namespaces.iter().cloned());
    let scan_roots = if options.scan_roots.is_empty() {
        vec![Utf8PathBuf::from(".")]
    } else {
        options.scan_roots.clone()
    };
    let mut result = parse::build_graph_scoped(root, config, &scan_roots)?;
    let _stats = crate::extract::resolve::resolve_all(
        &mut result.graph,
        &result.label_candidates,
        &result.pending_edges,
        config,
        root,
        &result.filename_index,
    );
    let pre_cascade_index = crate::extract::resolve::build_node_index(&result.graph);
    let root_str = root.to_string();
    let cascade_results = crate::extract::resolve::cascade_unresolved(
        &mut result.graph,
        &result.pending_edges,
        &pre_cascade_index,
        &root_str,
    );
    let node_index = crate::extract::resolve::build_node_index(&result.graph);

    let mut batch = FactBatch::new(corpus, source, FactBatchMode::FullSnapshot, generation);
    let mut revisions = RevisionCache::new(root, &result);
    let mut edge_assertions = EdgeAssertionCache::new(root, options.probe_edge_assertions);

    for (node_id, handle) in result.graph.nodes() {
        let fact = handle_fact(&batch, &mut revisions, &result, node_id, handle);
        batch.handles.push(fact);
        emit_resolved_file_meta(&mut batch, &mut revisions, &result.graph, handle);
    }

    let edge_order_context = EdgeOrderContext {
        root,
        config,
        result: &result,
        pre_cascade_index: &pre_cascade_index,
        node_index: &node_index,
        cascade_results: &cascade_results,
    };
    emit_ordered_edges(
        &mut batch,
        &mut revisions,
        &mut edge_assertions,
        &edge_order_context,
    );

    for extraction in &result.extractions {
        emit_file_parent_meta(&mut batch, &mut revisions, &extraction.file);
        emit_frontmatter_meta(&mut batch, &mut revisions, &result, &extraction.file);
    }
    emit_implausible_ref_meta(&mut batch, &mut revisions, &result)?;
    emit_code_ref_meta(
        &mut batch,
        &mut revisions,
        &mut edge_assertions,
        root,
        &result,
        options,
    )?;
    let file_payloads = std::mem::take(&mut result.file_payloads);
    let heading_spans = std::mem::take(&mut result.heading_spans);
    emit_content_spans(
        &mut batch,
        &mut revisions,
        &result,
        file_payloads,
        heading_spans,
    );
    emit_concerns(&mut batch, &mut revisions, config, &result);

    Ok(batch)
}

pub fn extract_markdown_facts_with_markdown_config(
    root: &Utf8Path,
    corpus: anneal_core::CorpusId,
    source: SourceName,
    generation: Generation,
    markdown_config: &MarkdownConfig,
    options: &MarkdownExtractionOptions,
) -> Result<FactBatch> {
    let mut config = markdown_config.config.clone();
    extract_markdown_facts_from_anneal_config(
        root,
        corpus,
        source,
        generation,
        &mut config,
        options,
    )
}

const METADATA_ONLY_KEYS: &[&str] = &["status", "updated", "title", "description", "tags", "date"];

fn infer_lifecycle_partition(
    observed_statuses: &HashSet<String>,
    config: &config::AnnealConfig,
    terminal_by_directory: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut active = config
        .convergence
        .active
        .iter()
        .filter(|status| observed_statuses.contains(*status))
        .cloned()
        .collect::<HashSet<_>>();
    let mut terminal = config
        .convergence
        .terminal
        .iter()
        .filter(|status| observed_statuses.contains(*status))
        .cloned()
        .collect::<HashSet<_>>();
    for status in observed_statuses {
        if active.contains(status) || terminal.contains(status) {
            continue;
        }
        if terminal_by_directory.contains(status) || anneal_core::is_terminal_status(status) {
            terminal.insert(status.clone());
        } else {
            active.insert(status.clone());
        }
    }
    let mut active = active.into_iter().collect::<Vec<_>>();
    let mut terminal = terminal.into_iter().collect::<Vec<_>>();
    active.sort();
    terminal.sort();
    (active, terminal)
}

fn inferred_frontmatter_fields(
    observed_frontmatter_keys: &HashMap<String, usize>,
) -> HashMap<String, config::FrontmatterFieldMapping> {
    let default_fm = config::FrontmatterConfig::default();
    let default_keys = default_fm.fields.keys().cloned().collect::<HashSet<_>>();
    let mut fields = default_fm.fields;
    for (key, count) in observed_frontmatter_keys {
        if default_keys.contains(key) || METADATA_ONLY_KEYS.contains(&key.as_str()) {
            continue;
        }
        if *count >= 3
            && let Some(mapping) = propose_mapping(key)
        {
            fields.insert(key.clone(), mapping);
        }
    }
    fields
}

fn propose_mapping(field_name: &str) -> Option<config::FrontmatterFieldMapping> {
    let lower = field_name.to_lowercase();
    match lower.as_str() {
        "affects" | "impacts" => Some(config::FrontmatterFieldMapping {
            edge_kind: "DependsOn".to_string(),
            direction: config::Direction::Inverse,
        }),
        "source" | "sources" | "based-on" | "builds-on" | "extends" | "parent" => {
            Some(config::FrontmatterFieldMapping {
                edge_kind: "DependsOn".to_string(),
                direction: config::Direction::Forward,
            })
        }
        "resolves" | "addresses" => Some(config::FrontmatterFieldMapping {
            edge_kind: "Discharges".to_string(),
            direction: config::Direction::Forward,
        }),
        "references" | "refs" | "related" | "see-also" | "cites" => {
            Some(config::FrontmatterFieldMapping {
                edge_kind: "Cites".to_string(),
                direction: config::Direction::Forward,
            })
        }
        _ => None,
    }
}

fn render_or_write_init_from_config(
    root: &Utf8Path,
    mut config: config::AnnealConfig,
    mode: InitMode,
) -> Result<InitOutput> {
    let config_path = root.join("anneal.dl");
    let legacy_path = root.join("anneal.toml");
    let backup_path = root.join("anneal.toml.legacy");
    let migrating_legacy = legacy_path.exists();
    if migrating_legacy && !config_path.exists() {
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
        body,
        written,
        path: config_path.to_string(),
        backup_path,
    })
}

fn existing_config_message(root: &Utf8Path) -> String {
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

fn write_unified_config(root: &Utf8Path, body: &str) -> Result<()> {
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

fn render_unified_config(config: &config::AnnealConfig) -> String {
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
        || !config.convergence.asserts_code.is_empty()
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
        if !config.convergence.asserts_code.is_empty() {
            list_config_call(
                &mut out,
                RuntimeConfigKey::ConvergenceAssertsCode,
                &config.convergence.asserts_code,
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
            config::Direction::Forward => "forward",
            config::Direction::Inverse => "inverse",
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
                config::HistoryMode::Xdg => "xdg",
                config::HistoryMode::Repo => "repo",
                config::HistoryMode::Off => "off",
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

    if !config.code_path_root.root.is_empty() {
        out.push_str("config code_path_root {\n");
        list_config_call(
            &mut out,
            RuntimeConfigKey::CodePathRoot,
            &config.code_path_root.root,
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
    let mut items = map
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
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
        out.push_str(&datalog_string_literal(value));
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
        out.push_str(&datalog_string_literal(value));
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

struct RevisionCache<'a> {
    root: &'a Utf8Path,
    parsed_revisions: HashMap<String, Revision>,
    revisions: HashMap<String, Revision>,
}

impl<'a> RevisionCache<'a> {
    fn new(root: &'a Utf8Path, result: &parse::BuildResult) -> Self {
        let parsed_revisions = result
            .file_payloads
            .iter()
            .map(|(file, payload)| (file.clone(), Revision::from(payload.revision.clone())))
            .collect();
        Self {
            root,
            parsed_revisions,
            revisions: HashMap::new(),
        }
    }

    fn revision_for(&mut self, file: &str) -> Revision {
        if let Some(revision) = self.parsed_revisions.get(file) {
            return revision.clone();
        }
        self.revisions
            .entry(file.to_string())
            .or_insert_with(|| {
                let path = self.root.join(file);
                let bytes = std::fs::read(path).unwrap_or_default();
                Revision::from(format!("{:016x}", anneal_core::fnv1a_64(&bytes)))
            })
            .clone()
    }
}

#[derive(Clone)]
struct EdgeAssertion {
    date: String,
    revision: String,
}

struct EdgeAssertionCache<'a> {
    root: &'a Utf8Path,
    repo_root: Option<Utf8PathBuf>,
    enabled: bool,
    assertions: HashMap<(String, u32), Option<EdgeAssertion>>,
}

impl<'a> EdgeAssertionCache<'a> {
    fn new(root: &'a Utf8Path, enabled: bool) -> Self {
        let repo_root = enabled.then(|| git_root_for(root)).flatten();
        Self {
            root,
            repo_root,
            enabled,
            assertions: HashMap::new(),
        }
    }

    fn assertion_for(&mut self, file: &str, line: u32) -> Option<EdgeAssertion> {
        if !self.enabled || file.is_empty() || line == 0 {
            return None;
        }
        let key = (file.to_string(), line);
        if let Some(assertion) = self.assertions.get(&key) {
            return assertion.clone();
        }
        let assertion = self.blame_line(file, line);
        self.assertions.insert(key, assertion.clone());
        assertion
    }

    fn blame_line(&self, file: &str, line: u32) -> Option<EdgeAssertion> {
        let repo_root = self.repo_root.as_ref()?;
        let file_path = std::fs::canonicalize(self.root.join(file).as_std_path()).ok()?;
        let file_path = Utf8PathBuf::from_path_buf(file_path).ok()?;
        let rel_path = file_path.strip_prefix(repo_root).ok()?;
        let line_arg = format!("{line},{line}");
        let blame = git_command(repo_root)
            .args(["blame", "-w", "-M", "-C", "-L", line_arg.as_str(), "--"])
            .arg(rel_path.as_str())
            .output()
            .ok()?;
        if !blame.status.success() || blame.stdout.is_empty() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&blame.stdout);
        let revision = stdout.split_whitespace().next()?.trim_start_matches('^');
        if revision.is_empty() {
            return None;
        }
        let date = git_command(repo_root)
            .args(["show", "-s", "--format=%cI", revision])
            .output()
            .ok()?;
        if !date.status.success() || date.stdout.is_empty() {
            return None;
        }
        let date = String::from_utf8_lossy(&date.stdout);
        let date = date.trim().get(..10)?;
        Some(EdgeAssertion {
            date: date.to_string(),
            revision: revision.to_string(),
        })
    }
}

fn git_root_for(root: &Utf8Path) -> Option<Utf8PathBuf> {
    let output = git_command(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout);
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let path = std::fs::canonicalize(path).ok()?;
    Utf8PathBuf::from_path_buf(path).ok()
}

fn git_command(root: &Utf8Path) -> Command {
    let mut command = Command::new("git");
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .arg("-C")
        .arg(root.as_str());
    command
}

fn handle_fact(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
    node_id: NodeId,
    handle: &Handle,
) -> HandleFact {
    let file = handle
        .file_path
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let native_id = native_id_for(handle);
    let identity = identity_for(batch, revisions, &native_id, &file);
    let namespace = match &handle.kind {
        HandleKind::Label { prefix, .. } => prefix.clone(),
        _ => String::new(),
    };
    let snippet = match &handle.kind {
        HandleKind::File(path) => result.file_snippets.get(path.as_str()).map(String::as_str),
        HandleKind::Label { .. } => result.label_snippets.get(&handle.id).map(String::as_str),
        _ => None,
    };

    HandleFact {
        identity,
        id: handle.id.clone(),
        kind: handle.kind.as_str().to_string(),
        status: handle.status.clone(),
        namespace,
        file: file.clone(),
        line: declaration_line(result, node_id, handle),
        date: handle.date.map(|date| date.format("%Y-%m-%d").to_string()),
        area: area_for(&file),
        summary: handle.summary(snippet).unwrap_or_default().to_string(),
    }
}

fn emit_resolved_file_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    graph: &DiGraph,
    handle: &Handle,
) {
    let Some(file) = resolved_file(handle, graph).map(ToString::to_string) else {
        return;
    };
    if file.is_empty() {
        return;
    }
    let native_id = native_id_for(handle);
    batch.meta.push(MetaFact {
        identity: identity_for(batch, revisions, &native_id, &file),
        handle: handle.id.clone(),
        key: "md.resolved_file".to_string(),
        value: file,
    });
}

fn declaration_line(_result: &parse::BuildResult, _node_id: NodeId, handle: &Handle) -> u32 {
    u32::from(matches!(handle.kind, HandleKind::File(_)))
}

struct EdgeFactInput<'a> {
    to: String,
    kind: &'a str,
    line: u32,
    ordinal: usize,
}

fn edge_fact(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    assertions: &mut EdgeAssertionCache<'_>,
    source_handle: &Handle,
    input: EdgeFactInput<'_>,
) -> EdgeFact {
    let file = source_handle
        .file_path
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let native_id = native_id_for_edge(
        source_handle,
        input.kind,
        &input.to,
        input.line,
        input.ordinal,
    );
    let assertion = assertions.assertion_for(&file, input.line);
    EdgeFact {
        identity: identity_for(batch, revisions, &native_id, &file),
        from: source_handle.id.clone(),
        to: input.to,
        kind: input.kind.to_string(),
        file,
        line: input.line,
        assertion_date: assertion.as_ref().map(|value| value.date.clone()),
        assertion_revision: assertion.map(|value| value.revision),
    }
}

fn native_id_for_edge(
    source_handle: &Handle,
    kind: &str,
    target: &str,
    line: u32,
    ordinal: usize,
) -> String {
    format!(
        "{}::edge::{ordinal}::{kind}::{target}::{line}",
        native_id_for(source_handle)
    )
}

#[derive(Clone)]
struct OrderedEdge {
    source: NodeId,
    target: NodeId,
    kind: EdgeKind,
    line: u32,
}

struct OrderedEdges {
    edges: Vec<OrderedEdge>,
    consumed: Vec<bool>,
    by_key: HashMap<(NodeId, NodeId, String), VecDeque<usize>>,
}

impl OrderedEdges {
    fn new(edges: Vec<OrderedEdge>) -> Self {
        let consumed = vec![false; edges.len()];
        let mut by_key = HashMap::<(NodeId, NodeId, String), VecDeque<usize>>::new();
        for (index, edge) in edges.iter().enumerate() {
            by_key
                .entry((edge.source, edge.target, edge.kind.as_str().to_string()))
                .or_default()
                .push_back(index);
        }
        Self {
            edges,
            consumed,
            by_key,
        }
    }

    fn take_edge(
        &mut self,
        source: NodeId,
        target: NodeId,
        kind: &EdgeKind,
        line: u32,
    ) -> Option<OrderedEdge> {
        let key = (source, target, kind.as_str().to_string());
        let indexes = self.by_key.get_mut(&key)?;
        let index = loop {
            let candidate = indexes.pop_front()?;
            if !self.consumed[candidate] {
                break candidate;
            }
        };
        self.consumed[index] = true;
        let mut edge = self.edges[index].clone();
        edge.line = line;
        Some(edge)
    }

    fn take_parse_time_external_edges(&mut self, graph: &DiGraph, ordered: &mut Vec<OrderedEdge>) {
        for (index, edge) in self.edges.iter().enumerate() {
            if self.consumed[index] {
                continue;
            }
            let target = graph.node(edge.target);
            if matches!(target.kind, HandleKind::External { .. }) {
                self.consumed[index] = true;
                ordered.push(edge.clone());
            }
        }
    }

    fn append_remaining(self, ordered: &mut Vec<OrderedEdge>) {
        for (edge, consumed) in self.edges.into_iter().zip(self.consumed) {
            if !consumed {
                ordered.push(edge);
            }
        }
    }
}

struct EdgeOrderContext<'a> {
    root: &'a Utf8Path,
    config: &'a config::AnnealConfig,
    result: &'a parse::BuildResult,
    pre_cascade_index: &'a HashMap<String, NodeId>,
    node_index: &'a HashMap<String, NodeId>,
    cascade_results: &'a [crate::extract::resolve::CascadeResult],
}

fn emit_ordered_edges(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    assertions: &mut EdgeAssertionCache<'_>,
    context: &EdgeOrderContext<'_>,
) {
    let mut remaining = OrderedEdges::new(graph_edges(&context.result.graph));
    let mut ordered = Vec::new();

    take_label_edges(
        context.config,
        context.result,
        context.node_index,
        &mut remaining,
        &mut ordered,
    );
    take_version_edges(&context.result.graph, &mut remaining, &mut ordered);
    take_pending_edges(
        context.root,
        context.result,
        context.pre_cascade_index,
        &mut remaining,
        &mut ordered,
    );
    take_cascade_edges(
        context.result,
        context.node_index,
        context.cascade_results,
        &mut remaining,
        &mut ordered,
    );
    take_parse_time_external_edges(&context.result.graph, &mut remaining, &mut ordered);
    remaining.append_remaining(&mut ordered);

    for (ordinal, edge) in ordered.into_iter().enumerate() {
        let source_handle = context.result.graph.node(edge.source);
        let target_handle = context.result.graph.node(edge.target);
        batch.edges.push(edge_fact(
            batch,
            revisions,
            assertions,
            source_handle,
            EdgeFactInput {
                to: target_handle.id.clone(),
                kind: edge.kind.as_str(),
                line: edge.line,
                ordinal,
            },
        ));
    }

    let ordered_count = batch.edges.len();
    for (idx, edge) in context.result.pending_edges.iter().enumerate() {
        if context.node_index.contains_key(&edge.target_identity) {
            continue;
        }
        if edge.unresolved_disposition == UnresolvedRefDisposition::AmbiguousExternalOk {
            continue;
        }
        let source_handle = context.result.graph.node(edge.source);
        // CR-R6: unresolved reference attempts remain stored edge facts so
        // fact-backed diagnostics can reproduce v1 existence checks.
        batch.edges.push(edge_fact(
            batch,
            revisions,
            assertions,
            source_handle,
            EdgeFactInput {
                to: edge.target_identity.clone(),
                kind: edge.kind.as_str(),
                line: edge.line.unwrap_or(0),
                ordinal: ordered_count + idx,
            },
        ));
    }
}

fn graph_edges(graph: &DiGraph) -> Vec<OrderedEdge> {
    let mut edges = Vec::new();
    for (node_id, _handle) in graph.nodes() {
        for edge in graph.outgoing(node_id) {
            edges.push(OrderedEdge {
                source: edge.source,
                target: edge.target,
                kind: edge.kind.clone(),
                line: 0,
            });
        }
    }
    edges
}

fn take_parse_time_external_edges(
    graph: &DiGraph,
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
) {
    remaining.take_parse_time_external_edges(graph, ordered);
}

fn take_label_edges(
    config: &config::AnnealConfig,
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
) {
    let namespaces = crate::extract::resolve::infer_namespaces(&result.label_candidates, config);
    let heading_first = result
        .label_candidates
        .iter()
        .filter(|candidate| candidate.is_heading)
        .chain(
            result
                .label_candidates
                .iter()
                .filter(|candidate| !candidate.is_heading),
        );

    for candidate in heading_first {
        if !namespaces.contains(&candidate.prefix) {
            continue;
        }
        let Some(&target) = node_index.get(&format!("{}-{}", candidate.prefix, candidate.number))
        else {
            continue;
        };
        let Some(&source) = node_index.get(candidate.file_path.as_str()) else {
            continue;
        };
        take_edge(remaining, ordered, source, target, &candidate.edge_kind, 0);
    }
}

fn take_version_edges(
    graph: &DiGraph,
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
) {
    for (source, handle) in graph.nodes() {
        if !matches!(handle.kind, HandleKind::Version { .. }) {
            continue;
        }
        for edge in graph.outgoing(source) {
            if edge.kind == EdgeKind::Supersedes {
                take_edge(remaining, ordered, edge.source, edge.target, &edge.kind, 0);
            }
        }
    }
}

fn take_pending_edges(
    root: &Utf8Path,
    result: &parse::BuildResult,
    pre_cascade_index: &HashMap<String, NodeId>,
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
) {
    for edge in &result.pending_edges {
        let Some(target) = resolve_pending_target(root, result, pre_cascade_index, edge) else {
            continue;
        };
        let (source, target) = if edge.inverse {
            (target, edge.source)
        } else {
            (edge.source, target)
        };
        take_edge(
            remaining,
            ordered,
            source,
            target,
            &edge.kind,
            edge.line.unwrap_or(0),
        );
    }
}

fn take_cascade_edges(
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    cascade_results: &[crate::extract::resolve::CascadeResult],
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
) {
    for cascade in cascade_results {
        let Some(edge) = result.pending_edges.get(cascade.edge_index) else {
            continue;
        };
        for candidate in &cascade.candidates {
            let Some(&target) = node_index.get(candidate) else {
                continue;
            };
            let (source, target) = if edge.inverse {
                (target, edge.source)
            } else {
                (edge.source, target)
            };
            if take_edge(
                remaining,
                ordered,
                source,
                target,
                &edge.kind,
                edge.line.unwrap_or(0),
            ) {
                break;
            }
        }
    }
}

fn take_edge(
    remaining: &mut OrderedEdges,
    ordered: &mut Vec<OrderedEdge>,
    source: NodeId,
    target: NodeId,
    kind: &EdgeKind,
    line: u32,
) -> bool {
    let Some(edge) = remaining.take_edge(source, target, kind, line) else {
        return false;
    };
    ordered.push(edge);
    true
}

fn resolve_pending_target(
    root: &Utf8Path,
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    edge: &PendingEdge,
) -> Option<NodeId> {
    node_index.get(&edge.target_identity).copied().or_else(|| {
        let target_path = std::path::Path::new(&edge.target_identity);
        if !target_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            || edge.target_identity.contains('/')
        {
            return None;
        }
        let referring_file = result.graph.node(edge.source).file_path.as_deref()?;
        crate::extract::resolve::resolve_bare_filename(
            &edge.target_identity,
            referring_file,
            root,
            node_index,
        )
        .or_else(|| {
            result
                .filename_index
                .get(&edge.target_identity)
                .filter(|paths| paths.len() == 1)
                .and_then(|paths| node_index.get(paths[0].as_str()).copied())
        })
    })
}

fn emit_file_parent_meta(batch: &mut FactBatch, revisions: &mut RevisionCache<'_>, file: &str) {
    let parent = Utf8Path::new(file)
        .parent()
        .map_or_else(String::new, ToString::to_string);
    batch.meta.push(MetaFact {
        identity: identity_for(batch, revisions, file, file),
        handle: file.to_string(),
        key: "md.parent_dir".to_string(),
        value: parent,
    });
}

fn emit_frontmatter_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
    file: &str,
) {
    let Some(payload) = result.file_payloads.get(file) else {
        return;
    };
    let identity = identity_for(batch, revisions, file, file);
    for (key, value) in &payload.frontmatter_scalars {
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: file.to_string(),
            key: key.clone(),
            value: value.clone(),
        });
    }
}

fn emit_implausible_ref_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
) -> Result<()> {
    for reference in &result.implausible_refs {
        // CR-D31: adapter-qualified diagnostic evidence lives in stored facts
        // even when it is not a graph relationship.
        let value = serde_json::json!({
            "value": reference.raw_value,
            "reason": reference.reason.to_string(),
            "line": reference.line,
        });
        batch.meta.push(MetaFact {
            identity: identity_for(batch, revisions, &reference.file, &reference.file),
            handle: reference.file.clone(),
            key: "md.implausible_ref".to_string(),
            value: serde_json::to_string(&value)?,
        });
    }
    Ok(())
}

fn emit_code_ref_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    assertions: &mut EdgeAssertionCache<'_>,
    root: &Utf8Path,
    result: &parse::BuildResult,
    options: &MarkdownExtractionOptions,
) -> Result<()> {
    let mut seen = HashSet::new();
    let mut probe_cache = CodeTargetProbeCache::new();
    let drift_mode = match (
        options.read_code_drift_evidence,
        options.refresh_code_drift_evidence,
    ) {
        (_, true) => CodeDriftEvidenceMode::Refresh,
        (true, false) => CodeDriftEvidenceMode::ReadCache,
        (false, false) => CodeDriftEvidenceMode::Disabled,
    };
    let mut drift_cache = CodeDriftEvidenceCache::open(root, drift_mode);
    for reference in &result.code_refs {
        if !seen.insert(reference.handle_id.clone()) {
            continue;
        }
        let identity = identity_for(batch, revisions, &reference.handle_id, &reference.file);
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: reference.handle_id.clone(),
            key: CodeTargetMeta::EXTERNAL_CLASS.to_string(),
            value: CodeTargetMeta::CLASS_CODE.to_string(),
        });
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: reference.handle_id.clone(),
            key: CodeTargetMeta::TARGET_PATH.to_string(),
            value: reference.path.clone(),
        });
        let probe = if options.probe_code_target_history {
            probe_cache.probe(root, &reference.path)
        } else {
            probe_cache.probe_without_history(root, &reference.path)
        };
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: reference.handle_id.clone(),
            key: CodeTargetMeta::TARGET_EXISTS.to_string(),
            value: probe.exists.as_str().to_string(),
        });
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: reference.handle_id.clone(),
            key: CodeTargetMeta::TARGET_HISTORY_STATUS.to_string(),
            value: probe.history_status.as_str().to_string(),
        });
        if let Some(base) = probe.probe_base {
            batch.meta.push(MetaFact {
                identity: identity.clone(),
                handle: reference.handle_id.clone(),
                key: CodeTargetMeta::TARGET_PROBE_BASE.to_string(),
                value: base.to_string(),
            });
        }
        if let Some(path) = probe.resolved_path {
            batch.meta.push(MetaFact {
                identity: identity.clone(),
                handle: reference.handle_id.clone(),
                key: CodeTargetMeta::TARGET_RESOLVED_PATH.to_string(),
                value: path.to_string(),
            });
        }
        if let Some(start_line) = reference.start_line {
            batch.meta.push(MetaFact {
                identity: identity.clone(),
                handle: reference.handle_id.clone(),
                key: CodeTargetMeta::TARGET_START_LINE.to_string(),
                value: start_line.to_string(),
            });
        }
        if let Some(end_line) = reference.end_line {
            batch.meta.push(MetaFact {
                identity: identity.clone(),
                handle: reference.handle_id.clone(),
                key: CodeTargetMeta::TARGET_END_LINE.to_string(),
                value: end_line.to_string(),
            });
        }
        let assertion = assertions.assertion_for(&reference.file, reference.source_line);
        if let Some(evidence) = drift_cache.evidence_for(&CodeDriftEvidenceRequest {
            ref_handle: reference.handle_id.clone(),
            target_path: reference.path.clone(),
            edge_file: reference.file.clone(),
            assertion_date: assertion.as_ref().map(|value| value.date.clone()),
            assertion_revision: assertion.map(|value| value.revision),
        }) {
            emit_code_drift_evidence_meta(batch, &identity, &reference.handle_id, &evidence);
        }
    }
    drift_cache
        .save()
        .context("failed to write drift evidence cache")
}

fn emit_code_drift_evidence_meta(
    batch: &mut FactBatch,
    identity: &FactIdentity,
    handle: &str,
    evidence: &CodeDriftEvidence,
) {
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: CodeTargetMeta::REFERENT_DISPOSITION.to_string(),
        value: evidence.disposition.clone(),
    });
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: CodeTargetMeta::REFERENT_EVIDENCE_HEAD.to_string(),
        value: evidence.evidence_head.clone(),
    });
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: CodeTargetMeta::REFERENT_ASSERTION_PREMISE.to_string(),
        value: evidence.assertion_premise.clone(),
    });
    if let Some(commits) = evidence.commits_since_assertion {
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: handle.to_string(),
            key: CodeTargetMeta::REFERENT_COMMITS_SINCE.to_string(),
            value: commits.to_string(),
        });
    }
    if let Some(moved_to) = &evidence.moved_to {
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: handle.to_string(),
            key: CodeTargetMeta::REFERENT_MOVED_TO.to_string(),
            value: moved_to.clone(),
        });
    }
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: CodeTargetMeta::REFERENT_MOVE_CANDIDATE_COUNT.to_string(),
        value: evidence.move_candidates.len().to_string(),
    });
    for candidate in &evidence.move_candidates {
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: handle.to_string(),
            key: CodeTargetMeta::REFERENT_MOVE_CANDIDATE.to_string(),
            value: candidate.clone(),
        });
    }
}

fn emit_content_spans(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
    mut file_payloads: HashMap<String, parse::ParsedMarkdownFile>,
    mut heading_spans: HashMap<String, Vec<crate::extract::body_scan::HeadingSpan>>,
) {
    enum ContentTask {
        File(String),
        Label(NodeId),
    }

    let tasks = result
        .graph
        .nodes()
        .filter_map(|(node_id, handle)| match &handle.kind {
            HandleKind::File(path) => Some(ContentTask::File(path.to_string())),
            HandleKind::Label { .. } => Some(ContentTask::Label(node_id)),
            _ => None,
        })
        .collect::<Vec<_>>();

    for task in tasks {
        match task {
            ContentTask::File(path_str) => {
                let Some(payload) = file_payloads.remove(&path_str) else {
                    continue;
                };
                let headings = heading_spans.remove(&path_str).unwrap_or_default();
                let body = payload.body;
                let start_line = payload.body_start_line;
                let line_count = u32::try_from(body.lines().count()).unwrap_or(u32::MAX);
                let tokens = token_count(&body);
                let body_ranges = BodyTextRanges::new(&body);
                let heading_facts = headings
                    .into_iter()
                    .map(|heading| {
                        let text = body_ranges.text_in_range(
                            start_line,
                            heading.start_line,
                            heading.end_line,
                        );
                        let lines = heading
                            .end_line
                            .saturating_sub(heading.start_line)
                            .saturating_add(1);
                        let tokens = token_count(&text);
                        let identity = identity_for(batch, revisions, &heading.id, &path_str);
                        (
                            SpanFact {
                                identity: identity.clone(),
                                id: heading.id.clone(),
                                handle: path_str.clone(),
                                start_line: heading.start_line,
                                end_line: heading.end_line,
                                summary: heading.path,
                            },
                            ContentFact {
                                identity,
                                handle: path_str.clone(),
                                span_id: heading.id,
                                lines,
                                text,
                                tokens,
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                let span_id = format!("{path_str}#full");
                let identity = identity_for(batch, revisions, &path_str, &path_str);
                batch.content.push(ContentFact {
                    identity: identity.clone(),
                    handle: path_str.clone(),
                    span_id: span_id.clone(),
                    lines: line_count,
                    text: body,
                    tokens,
                });
                batch.spans.push(SpanFact {
                    identity,
                    id: span_id,
                    handle: path_str.clone(),
                    start_line,
                    end_line: start_line.saturating_add(line_count.saturating_sub(1)),
                    summary: result
                        .file_snippets
                        .get(&path_str)
                        .cloned()
                        .unwrap_or_default(),
                });
                for (span, content) in heading_facts {
                    batch.spans.push(span);
                    batch.content.push(content);
                }
            }
            ContentTask::Label(node_id) => {
                let handle = result.graph.node(node_id);
                let Some(summary) = result.label_snippets.get(&handle.id) else {
                    continue;
                };
                let file = handle.file_path.as_deref().map_or("", Utf8Path::as_str);
                let span_id = format!("{}#definition", handle.id);
                let identity = identity_for(batch, revisions, &native_id_for(handle), file);
                batch.spans.push(SpanFact {
                    identity: identity.clone(),
                    id: span_id.clone(),
                    handle: handle.id.clone(),
                    start_line: declaration_line(result, node_id, handle),
                    end_line: declaration_line(result, node_id, handle),
                    summary: summary.clone(),
                });
                batch.content.push(ContentFact {
                    identity,
                    handle: handle.id.clone(),
                    span_id,
                    lines: 1,
                    text: summary.clone(),
                    tokens: token_count(summary),
                });
            }
        }
    }
}

struct BodyTextRanges<'a> {
    body: &'a str,
    line_offsets: Vec<usize>,
    normalize_cr: bool,
}

impl<'a> BodyTextRanges<'a> {
    fn new(body: &'a str) -> Self {
        let normalize_cr = body.contains('\r');
        let mut line_offsets = vec![0];
        if !normalize_cr {
            line_offsets.extend(
                body.match_indices('\n')
                    .map(|(index, _)| index.saturating_add(1)),
            );
            if *line_offsets.last().unwrap_or(&0) != body.len() {
                line_offsets.push(body.len());
            }
        }
        Self {
            body,
            line_offsets,
            normalize_cr,
        }
    }

    fn text_in_range(&self, body_start_line: u32, start_line: u32, end_line: u32) -> String {
        let start = start_line.saturating_sub(body_start_line);
        let count = end_line.saturating_sub(start_line).saturating_add(1);
        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let count = usize::try_from(count).unwrap_or(usize::MAX);

        if self.normalize_cr {
            return body_text_in_range_normalized(self.body, start, count);
        }

        let Some(&start_byte) = self.line_offsets.get(start) else {
            return String::new();
        };
        let end_byte = self
            .line_offsets
            .get(start.saturating_add(count))
            .copied()
            .unwrap_or(self.body.len());
        self.body[start_byte..end_byte]
            .strip_suffix('\n')
            .unwrap_or(&self.body[start_byte..end_byte])
            .to_string()
    }
}

fn body_text_in_range_normalized(body: &str, start: usize, count: usize) -> String {
    let lines = body.lines().skip(start).take(count).collect::<Vec<_>>();
    let capacity = lines
        .iter()
        .map(|line| line.len())
        .sum::<usize>()
        .saturating_add(lines.len().saturating_sub(1));
    let mut text = String::new();
    text.reserve_exact(capacity);
    for line in lines {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(line);
    }
    text
}

fn emit_concerns(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    config: &config::AnnealConfig,
    result: &parse::BuildResult,
) {
    for (name, members) in &config.concerns {
        for member in members {
            for (_, handle) in result.graph.nodes() {
                if !concern_pattern_matches(member, &handle.id) {
                    continue;
                }
                let file = handle.file_path.as_deref().map_or("", Utf8Path::as_str);
                batch.concerns.push(ConcernFact {
                    identity: identity_for(batch, revisions, &native_id_for(handle), file),
                    name: name.clone(),
                    member: handle.id.clone(),
                });
            }
        }
    }
}

fn concern_pattern_matches(pattern: &str, handle: &str) -> bool {
    pattern == handle
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| handle.starts_with(prefix))
}

fn identity_for(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    native_id: &str,
    file: &str,
) -> FactIdentity {
    let revision = if file.is_empty() {
        Revision::from("unknown")
    } else {
        revisions.revision_for(file)
    };
    let origin_uri = if file.is_empty() {
        format!("anneal://{native_id}")
    } else {
        format!("file://{}", revisions.root.join(file))
    };
    FactIdentity::new(
        batch.corpus.clone(),
        batch.source.clone(),
        NativeId::from(native_id),
        OriginUri::from(origin_uri),
        revision,
        batch.generation,
    )
}

fn native_id_for(handle: &Handle) -> String {
    handle
        .file_path
        .as_ref()
        .map_or_else(|| handle.id.clone(), ToString::to_string)
}

fn area_for(file: &str) -> String {
    if file.is_empty() {
        String::new()
    } else {
        crate::extract::area::area_of(file).to_string()
    }
}

fn token_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use anneal_core::{CodeTargetMeta, CorpusId, Generation, SourceName};
    use camino::{Utf8Path, Utf8PathBuf};
    use tempfile::tempdir;

    use super::{
        InitMode, MarkdownExtractionOptions, area_for, extract_markdown_facts, render_or_write_init,
    };
    use crate::extract::adapter::extract_markdown_facts_with_options;

    #[test]
    fn area_for_groups_root_files_under_root_area() {
        assert_eq!(area_for("README.md"), "(root)");
        assert_eq!(area_for("compiler/design.md"), "compiler");
        assert_eq!(area_for("compiler/sub/design.md"), "compiler");
        assert_eq!(area_for(""), "");
    }

    #[test]
    fn init_preserves_existing_unified_config_body() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(temp.path()).expect("utf8 tempdir");
        let body = "source md {\n  file_extension(\".md\").\n  scan_root(\".\").\n}\n\nconfig search_boost {\n  status(\"authoritative\", 1.5).\n}\n";
        std::fs::write(root.join("anneal.dl"), body).expect("write config");

        let output = render_or_write_init(root, InitMode::DryRun).expect("init dry run");

        assert!(!output.written);
        assert_eq!(output.body, body);
    }

    #[test]
    fn markdown_facts_emit_code_refs_as_external_cites_with_metadata() {
        let temp = tempdir().expect("tempdir");
        let corpus = temp.path().join("corpus");
        std::fs::create_dir_all(corpus.join("lib/example")).expect("create corpus");
        std::fs::write(
            corpus.join("lib/example/admission.rs"),
            "pub fn admit() {}\n",
        )
        .expect("write code");
        std::fs::write(
            corpus.join("doc.md"),
            "# Doc\n\nSee `lib/example/admission.rs:142-167`.\n",
        )
        .expect("write doc");
        let root = Utf8Path::from_path(&corpus).expect("utf8 tempdir");

        let batch = extract_markdown_facts(
            root,
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
        )
        .expect("extract facts");

        let handle_id = "external:code:doc.md:3:lib/example/admission.rs:142-167";
        assert!(
            batch
                .handles
                .iter()
                .any(|handle| handle.id == handle_id && handle.kind == "external"),
            "expected external code handle in {:?}",
            batch.handles
        );
        assert!(
            batch.edges.iter().any(|edge| {
                edge.from == "doc.md"
                    && edge.to == handle_id
                    && edge.kind == "Cites"
                    && edge.line == 3
            }),
            "expected code Cites edge with source line in {:?}",
            batch.edges
        );
        for (key, value) in [
            (CodeTargetMeta::EXTERNAL_CLASS, CodeTargetMeta::CLASS_CODE),
            (CodeTargetMeta::TARGET_PATH, "lib/example/admission.rs"),
            (CodeTargetMeta::TARGET_EXISTS, "true"),
            (CodeTargetMeta::TARGET_HISTORY_STATUS, "unavailable"),
            (CodeTargetMeta::TARGET_START_LINE, "142"),
            (CodeTargetMeta::TARGET_END_LINE, "167"),
        ] {
            assert!(
                batch
                    .meta
                    .iter()
                    .any(|meta| meta.handle == handle_id && meta.key == key && meta.value == value),
                "missing {key}={value} metadata in {:?}",
                batch.meta
            );
        }
        assert!(batch.meta.iter().any(|meta| {
            meta.handle == handle_id
                && meta.key == CodeTargetMeta::TARGET_PROBE_BASE
                && meta.value == root.as_str()
        }));
        let resolved_path = root.join("lib/example/admission.rs");
        assert!(batch.meta.iter().any(|meta| {
            meta.handle == handle_id
                && meta.key == CodeTargetMeta::TARGET_RESOLVED_PATH
                && meta.value == resolved_path.as_str()
        }));
    }

    #[test]
    fn frontmatter_code_path_mints_external_code_handle_and_avoids_broken_ref() {
        // A frontmatter reference field citing an existing in-repo non-.md
        // source file must route through the code-ref pipeline: an external:code
        // handle with target_exists=true, and a Cites edge whose target resolves
        // to that handle (so E001 broken_ref cannot fire).
        let temp = tempdir().expect("tempdir");
        let corpus = temp.path().join("corpus");
        std::fs::create_dir_all(corpus.join("formal-model/proofs")).expect("create corpus");
        std::fs::write(
            corpus.join("formal-model/proofs/Space.agda"),
            "module Space where\n",
        )
        .expect("write agda");
        std::fs::write(
            corpus.join("doc.md"),
            "---\ndepends-on: formal-model/proofs/Space.agda\n---\n# Doc\n\nBody.\n",
        )
        .expect("write doc");
        let root = Utf8Path::from_path(&corpus).expect("utf8 tempdir");

        let batch = extract_markdown_facts(
            root,
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
        )
        .expect("extract facts");

        let handle_id = "external:code:doc.md:1:formal-model/proofs/Space.agda";
        assert!(
            batch
                .handles
                .iter()
                .any(|handle| handle.id == handle_id && handle.kind == "external"),
            "expected external code handle for frontmatter code path in {:?}",
            batch.handles
        );
        assert!(
            batch.edges.iter().any(|edge| {
                edge.from == "doc.md" && edge.to == handle_id && edge.kind == "Cites"
            }),
            "expected code Cites edge for frontmatter code path in {:?}",
            batch.edges
        );
        assert_code_ref_meta(&batch, handle_id, CodeTargetMeta::TARGET_EXISTS, "true");
        assert_code_ref_meta(
            &batch,
            handle_id,
            CodeTargetMeta::TARGET_PATH,
            "formal-model/proofs/Space.agda",
        );
        // The frontmatter path must NOT leak a plain corpus-gated edge whose
        // target has no handle — that is exactly the E001 broken_ref shape.
        assert!(
            !batch.edges.iter().any(|edge| {
                edge.from == "doc.md" && edge.to == "formal-model/proofs/Space.agda"
            }),
            "frontmatter code path must not emit a raw corpus-gated edge: {:?}",
            batch.edges
        );
    }

    #[test]
    fn frontmatter_code_path_missing_target_flags_drift_not_broken_ref() {
        // A frontmatter code path whose target is gone from disk resolves to
        // target_exists=false (the W006 spec_code_drift condition), while still
        // minting an external:code handle so E001 broken_ref cannot fire.
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().join("corpus")).expect("utf8 tempdir");
        std::fs::create_dir_all(root.join("lib")).expect("create lib");
        std::fs::write(root.join("lib/old.rs"), "pub fn old() {}\n").expect("write old code");
        run_git(&root, &["init"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "add old code"]);
        std::fs::remove_file(root.join("lib/old.rs")).expect("remove old code");
        std::fs::write(
            root.join("doc.md"),
            "---\ndepends-on: lib/old.rs\n---\n# Doc\n\nBody.\n",
        )
        .expect("write doc");

        let batch = extract_markdown_facts_with_options(
            root.as_path(),
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
            &MarkdownExtractionOptions {
                probe_code_target_history: true,
                ..MarkdownExtractionOptions::default()
            },
        )
        .expect("extract with history");

        let handle_id = "external:code:doc.md:1:lib/old.rs";
        assert!(
            batch
                .handles
                .iter()
                .any(|handle| handle.id == handle_id && handle.kind == "external"),
            "expected external code handle so broken_ref cannot fire: {:?}",
            batch.handles
        );
        // W006 spec_code_drift keys on target_exists=false; E001 is avoided
        // because the Cites edge target resolves to the minted handle above.
        assert_code_ref_meta(&batch, handle_id, CodeTargetMeta::TARGET_EXISTS, "false");
        assert!(
            !batch
                .edges
                .iter()
                .any(|edge| edge.from == "doc.md" && edge.to == "lib/old.rs"),
            "missing frontmatter code path must not emit a raw corpus-gated edge: {:?}",
            batch.edges
        );
    }

    #[test]
    fn unresolved_ambiguous_wikilinks_do_not_emit_broken_corpus_edges() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(temp.path()).expect("utf8 tempdir");
        std::fs::write(root.join("doc.md"), "# Doc\n\nSee [[claim]].\n").expect("write doc");

        let batch = extract_markdown_facts(
            root,
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
        )
        .expect("extract facts");

        assert!(
            !batch
                .edges
                .iter()
                .any(|edge| edge.from == "doc.md" && edge.to == "claim"),
            "ambiguous unresolved wikilink should not emit an E001-producing edge: {:?}",
            batch.edges
        );
    }

    #[test]
    fn code_ref_history_probe_is_opt_in_at_extraction_time() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().join("corpus")).expect("utf8 tempdir");
        std::fs::create_dir_all(root.join("lib")).expect("create lib");
        std::fs::write(root.join("lib/old.rs"), "pub fn old() {}\n").expect("write old code");
        run_git(&root, &["init"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "add old code"]);
        std::fs::remove_file(root.join("lib/old.rs")).expect("remove old code");
        std::fs::write(root.join("doc.md"), "# Doc\n\nSee `lib/old.rs`.\n").expect("write doc");

        let without_history = extract_markdown_facts_with_options(
            root.as_path(),
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
            &MarkdownExtractionOptions::default(),
        )
        .expect("extract without history");
        assert_code_ref_meta(
            &without_history,
            "external:code:doc.md:3:lib/old.rs",
            CodeTargetMeta::TARGET_EXISTS,
            "unknown",
        );
        assert_code_ref_meta(
            &without_history,
            "external:code:doc.md:3:lib/old.rs",
            CodeTargetMeta::TARGET_HISTORY_STATUS,
            "unavailable",
        );

        let with_history = extract_markdown_facts_with_options(
            root.as_path(),
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
            &MarkdownExtractionOptions {
                probe_code_target_history: true,
                ..MarkdownExtractionOptions::default()
            },
        )
        .expect("extract with history");
        assert_code_ref_meta(
            &with_history,
            "external:code:doc.md:3:lib/old.rs",
            CodeTargetMeta::TARGET_EXISTS,
            "false",
        );
        assert_code_ref_meta(
            &with_history,
            "external:code:doc.md:3:lib/old.rs",
            CodeTargetMeta::TARGET_HISTORY_STATUS,
            "present",
        );
    }

    #[test]
    fn edge_assertion_probe_is_verified_or_null() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().join("corpus")).expect("utf8 tempdir");
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::write(root.join("src/lib.rs"), "pub fn target() {}\n").expect("write code");
        std::fs::write(root.join("doc.md"), "# Doc\n\nSee `src/lib.rs:1`.\n").expect("write doc");
        run_git(&root, &["init"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "add cited doc"]);

        let without_assertions = extract_markdown_facts_with_options(
            root.as_path(),
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
            &MarkdownExtractionOptions::default(),
        )
        .expect("extract without assertions");
        let edge = cited_edge(
            &without_assertions,
            "doc.md",
            "external:code:doc.md:3:src/lib.rs:1",
        );
        assert_eq!(edge.assertion_date, None);
        assert_eq!(edge.assertion_revision, None);

        let with_assertions = extract_markdown_facts_with_options(
            root.as_path(),
            CorpusId::from("test"),
            SourceName::from("markdown"),
            Generation::initial(),
            &MarkdownExtractionOptions {
                probe_edge_assertions: true,
                ..MarkdownExtractionOptions::default()
            },
        )
        .expect("extract with assertions");
        let edge = cited_edge(
            &with_assertions,
            "doc.md",
            "external:code:doc.md:3:src/lib.rs:1",
        );
        assert!(
            edge.assertion_date
                .as_deref()
                .is_some_and(|date| { date.len() == 10 && date.chars().nth(4) == Some('-') })
        );
        assert!(
            edge.assertion_revision
                .as_deref()
                .is_some_and(|revision| revision.len() >= 7)
        );
    }

    fn assert_code_ref_meta(batch: &anneal_core::FactBatch, target: &str, key: &str, value: &str) {
        assert!(
            batch
                .meta
                .iter()
                .any(|meta| meta.handle == target && meta.key == key && meta.value == value),
            "missing {key}={value} for {target} in {:?}",
            batch.meta
        );
    }

    fn cited_edge<'a>(
        batch: &'a anneal_core::FactBatch,
        from: &str,
        to: &str,
    ) -> &'a anneal_core::EdgeFact {
        batch
            .edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to && edge.kind == "Cites")
            .unwrap_or_else(|| panic!("missing {from} -> {to} Cites edge in {:?}", batch.edges))
    }

    fn run_git(root: &Utf8Path, args: &[&str]) {
        let status = Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_CONFIG_GLOBAL", root.join(".anneal-test-gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-c")
            .arg("user.name=Anneal Test")
            .arg("-c")
            .arg("user.email=anneal@example.test")
            .arg("-C")
            .arg(root.as_std_path())
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed: {status}");
    }
}
