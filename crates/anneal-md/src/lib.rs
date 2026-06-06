//! Markdown adapter for anneal v2.

use anneal_core::{
    ConfigFacts, ConfigKey, FactBatch, FactBatchMode, Pattern, RelativePathPolicy, Source,
    SourceCapabilities, SourceContext, SourceError, SourceInfo, SourceName,
    default_lexical_search_info, normalize_relative_path,
};
use camino::Utf8PathBuf;
use serde::Serialize;

const SOURCE_NAME: &str = "markdown";

#[derive(Clone, Copy, Debug)]
pub enum InitMode {
    DryRun,
    Write { force: bool },
}

impl InitMode {
    pub const fn from_flags(dry_run: bool, force: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else {
            Self::Write { force }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InitOutput {
    pub body: String,
    pub written: bool,
    pub path: String,
    pub backup_path: Option<String>,
}

pub fn render_or_write_init(root: &camino::Utf8Path, mode: InitMode) -> anyhow::Result<InitOutput> {
    let mode = match mode {
        InitMode::DryRun => anneal_legacy::v2_adapter::InitMode::DryRun,
        InitMode::Write { force } => anneal_legacy::v2_adapter::InitMode::Write { force },
    };
    let output = anneal_legacy::v2_adapter::render_or_write_init(root, mode)?;
    Ok(InitOutput {
        body: output.body,
        written: output.written,
        path: output.path,
        backup_path: output.backup_path,
    })
}

/// Markdown `Source` implementation.
#[derive(Clone, Debug, Default)]
pub struct MarkdownSource {
    legacy_config: Option<anneal_legacy::v2_adapter::MarkdownLegacyConfig>,
}

impl MarkdownSource {
    pub fn with_runtime_config(config: &ConfigFacts) -> Result<Self, SourceError> {
        let legacy_config =
            anneal_legacy::v2_adapter::MarkdownLegacyConfig::from_runtime_facts(config)
                .map_err(|err| SourceError::Other(err.to_string()))?;
        Ok(Self {
            legacy_config: Some(legacy_config),
        })
    }
}

impl Source for MarkdownSource {
    fn describe(&self) -> SourceInfo {
        SourceInfo {
            name: SOURCE_NAME,
            recognizes: vec![Pattern::new("**/*.md")],
            doc: "Extracts markdown files through the v1 parse/resolve pipeline and emits stored relation facts.",
            config_keys: vec![
                ConfigKey::required_exact("md.file_extension", 1),
                ConfigKey::required_exact("md.scan_root", 1),
                ConfigKey::optional_at_least("md.scan_exclude", 1),
                ConfigKey::optional_exact("md.label_pattern", 3),
                ConfigKey::optional_exact("md.linear_namespace", 1),
                ConfigKey::optional_exact("md.version_pattern", 2),
                ConfigKey::optional_exact("md.section_min_depth", 1),
                ConfigKey::optional_exact("md.section_max_depth", 1),
            ],
            capabilities: SourceCapabilities {
                supports_git_ref: false,
                supports_time_snapshot: false,
                supports_incremental: false,
                live_only: false,
            },
            search: Some(default_lexical_search_info()),
        }
    }

    fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
        if cx.time_ref.is_some() {
            return Err(SourceError::UnsupportedTimeRef(
                cx.time_ref.clone().expect("checked above"),
            ));
        }

        let generation = cx.next_generation();
        let mut config = MarkdownDiscoveryConfig::from_facts(cx.config_facts)?;
        config.options.probe_code_target_history = cx.probe_code_target_history;
        let mut combined = FactBatch::new(
            cx.corpus.clone(),
            SourceName::from(SOURCE_NAME),
            FactBatchMode::FullSnapshot,
            generation,
        );
        for root in cx.roots {
            cx.cancellation.check()?;
            let batch = if let Some(legacy_config) = &self.legacy_config {
                anneal_legacy::v2_adapter::extract_markdown_facts_with_legacy_config(
                    root,
                    cx.corpus.clone(),
                    SourceName::from(SOURCE_NAME),
                    generation,
                    legacy_config,
                    &config.options,
                )
            } else {
                anneal_legacy::v2_adapter::extract_markdown_facts_with_options(
                    root,
                    cx.corpus.clone(),
                    SourceName::from(SOURCE_NAME),
                    generation,
                    &config.options,
                )
            }
            .map_err(|err| SourceError::Other(err.to_string()))?;
            combined.visibility.extend(batch.visibility);
            combined.handles.extend(batch.handles);
            combined.edges.extend(batch.edges);
            combined.content.extend(batch.content);
            combined.spans.extend(batch.spans);
            combined.meta.extend(batch.meta);
            combined.concerns.extend(batch.concerns);
            combined.retractions.extend(batch.retractions);
        }
        Ok(combined)
    }
}

struct MarkdownDiscoveryConfig {
    options: anneal_legacy::v2_adapter::MarkdownExtractionOptions,
}

impl MarkdownDiscoveryConfig {
    fn from_facts(facts: &ConfigFacts) -> Result<Self, SourceError> {
        validate_file_extensions(facts)?;
        reject_unsupported(facts, "md.label_pattern")?;
        reject_unsupported(facts, "md.version_pattern")?;
        reject_unsupported(facts, "md.section_min_depth")?;
        reject_unsupported(facts, "md.section_max_depth")?;

        let mut scan_roots = facts
            .values("md.scan_root")
            .map(valid_relative_path)
            .collect::<Result<Vec<_>, _>>()?;
        if scan_roots.is_empty() {
            scan_roots.push(Utf8PathBuf::from("."));
        }

        Ok(Self {
            options: anneal_legacy::v2_adapter::MarkdownExtractionOptions {
                scan_roots,
                exclude: facts
                    .values("md.scan_exclude")
                    .map(str::to_string)
                    .collect(),
                linear_namespaces: facts
                    .values("md.linear_namespace")
                    .map(str::to_string)
                    .collect(),
                probe_code_target_history: false,
            },
        })
    }
}

fn validate_file_extensions(facts: &ConfigFacts) -> Result<(), SourceError> {
    for extension in facts.values("md.file_extension") {
        if !matches!(extension, ".md" | "md") {
            return Err(SourceError::Other(format!(
                "markdown source only supports md.file_extension(\".md\") during the legacy bridge; got {extension:?}"
            )));
        }
    }
    Ok(())
}

fn reject_unsupported(facts: &ConfigFacts, key: &str) -> Result<(), SourceError> {
    if let Some(value) = facts.first(key) {
        return Err(SourceError::Other(format!(
            "markdown source does not support {key}({value:?}) through the legacy bridge yet"
        )));
    }
    Ok(())
}

fn valid_relative_path(value: &str) -> Result<Utf8PathBuf, SourceError> {
    normalize_relative_path(value, RelativePathPolicy::ALLOW_EMPTY).ok_or_else(|| {
        SourceError::Other(format!(
            "md.scan_root must be a relative path inside the corpus root; got {value:?}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use anneal_core::{
        ActorContext, CancellationToken, ConfigFacts, CorpusId, FactStore, Generation,
        OneShotSourceDriver, SourceContext, SourceRefreshRequest, refresh_source,
    };
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;

    fn context<'a>(
        root: &'a Utf8PathBuf,
        config: &'a ConfigFacts,
        previous_generation: Option<Generation>,
    ) -> SourceContext<'a> {
        SourceContext {
            corpus: CorpusId::from("test"),
            roots: std::slice::from_ref(root),
            config_facts: config,
            probe_code_target_history: false,
            time_ref: None,
            previous_generation,
            actor: ActorContext {
                actor: "test".to_string(),
                capabilities: BTreeSet::new(),
            },
            cancellation: CancellationToken::new(),
        }
    }

    #[test]
    fn markdown_source_extracts_v1_graph_facts() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(
            root.join("anneal.dl"),
            "config handles {\n  force([\"OQ\"]).\n}\n\nconfig frontmatter {\n  field(\"depends-on\", \"DependsOn\", \"forward\").\n}\n",
        )
        .expect("write config");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\ndepends-on: b.md\npurpose: Test file\n---\n# A\nBody text with OQ-1.\n",
        )
        .expect("write a");
        fs::write(root.join("b.md"), "---\nstatus: active\n---\n# B\n").expect("write b");
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".md".to_string()),
            ("md.scan_root".to_string(), ".".to_string()),
        ]);

        let batch = MarkdownSource::default()
            .extract(&context(&root, &config, Some(Generation::new(7))))
            .expect("extract");

        assert_eq!(batch.generation, Generation::new(8));
        assert!(batch.handles.iter().any(|fact| fact.id == "a.md"));
        assert!(batch.handles.iter().any(|fact| fact.id == "OQ-1"));
        assert!(
            batch
                .edges
                .iter()
                .any(|fact| fact.from == "a.md" && fact.to == "b.md" && fact.kind == "DependsOn")
        );
        assert!(batch.meta.iter().any(|fact| {
            fact.handle == "a.md" && fact.key == "purpose" && fact.value == "Test file"
        }));
        assert!(batch.spans.iter().any(|fact| fact.handle == "a.md"));
        assert!(batch.content.iter().any(|fact| fact.handle == "a.md"));
        assert!(
            !batch.handles.iter().any(|fact| fact.kind == "section"),
            "markdown headings should not emit section handles"
        );
        assert!(
            batch
                .spans
                .iter()
                .any(|fact| fact.id == "a.md#h/a" && fact.summary == "A"),
            "markdown headings should emit structural spans"
        );
        assert!(
            batch
                .content
                .iter()
                .any(|fact| fact.span_id == "a.md#h/a" && fact.text.contains("Body text")),
            "heading spans should have readable content"
        );
    }

    #[test]
    fn markdown_source_honors_scan_root_discovery_fact() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::create_dir(root.join("included")).expect("create included");
        fs::write(root.join("anneal.toml"), "").expect("write root config");
        fs::write(root.join("a.md"), "---\nstatus: draft\n---\n# A\n").expect("write excluded doc");
        fs::write(
            root.join("included").join("b.md"),
            "---\nstatus: active\n---\n# B\n",
        )
        .expect("write included doc");
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".md".to_string()),
            ("md.scan_root".to_string(), "included".to_string()),
        ]);

        let batch = MarkdownSource::default()
            .extract(&context(&root, &config, Some(Generation::new(0))))
            .expect("extract");

        assert!(!batch.handles.iter().any(|fact| fact.id == "a.md"));
        assert!(batch.handles.iter().any(|fact| fact.id == "included/b.md"));
    }

    #[test]
    fn markdown_source_rejects_unsupported_discovery_facts_loudly() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".txt".to_string()),
            ("md.scan_root".to_string(), ".".to_string()),
        ]);

        let err = MarkdownSource::default()
            .extract(&context(&root, &config, Some(Generation::new(0))))
            .expect_err("unsupported extension rejects");

        assert!(err.to_string().contains("md.file_extension"));
    }

    #[test]
    fn markdown_source_rejects_invalid_runtime_config_values() {
        let config = ConfigFacts::new(vec![(
            "state.history_mode".to_string(),
            "sideways".to_string(),
        )]);

        let err =
            MarkdownSource::with_runtime_config(&config).expect_err("invalid history mode rejects");

        assert!(err.to_string().contains("state.history_mode"));
    }

    #[test]
    fn markdown_source_rejects_invalid_frontmatter_direction() {
        let config = ConfigFacts::new(vec![
            (
                "frontmatter.field.depends-on.edge_kind".to_string(),
                "DependsOn".to_string(),
            ),
            (
                "frontmatter.field.depends-on.direction".to_string(),
                "sideways".to_string(),
            ),
        ]);

        let err = MarkdownSource::with_runtime_config(&config)
            .expect_err("invalid frontmatter direction rejects");

        assert!(
            err.to_string()
                .contains("frontmatter.field.depends-on.direction")
        );
    }

    #[test]
    fn full_snapshot_reextract_retracts_edited_file_facts() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir(&root).expect("create corpus root");
        fs::write(root.join("anneal.toml"), "").expect("write config");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\npurpose: Old\n---\n# A\nOld body.\n",
        )
        .expect("write old");
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".md".to_string()),
            ("md.scan_root".to_string(), ".".to_string()),
        ]);
        let mut store = FactStore::default();
        let driver = OneShotSourceDriver::new(MarkdownSource::default());
        let request = SourceRefreshRequest::new("test", std::slice::from_ref(&root), &config)
            .with_actor(ActorContext {
                actor: "test".to_string(),
                capabilities: BTreeSet::new(),
            });
        let first = refresh_source(&driver, &request, &mut store).expect("first refresh");
        assert_eq!(first.previous_generation, None);

        fs::write(
            root.join("a.md"),
            "---\nstatus: active\npurpose: New\n---\n# A\nNew body.\n",
        )
        .expect("write new");
        let second = refresh_source(&driver, &request, &mut store).expect("second refresh");
        assert_eq!(second.previous_generation, Some(Generation::new(1)));
        assert_eq!(second.current_generation, Generation::new(2));

        assert!(
            store
                .handles()
                .iter()
                .any(|fact| fact.id == "a.md" && fact.status.as_deref() == Some("active"))
        );
        assert!(
            !store
                .meta()
                .iter()
                .any(|fact| fact.handle == "a.md" && fact.value == "Old")
        );
        assert!(
            store
                .meta()
                .iter()
                .any(|fact| fact.handle == "a.md" && fact.value == "New")
        );
    }
}
