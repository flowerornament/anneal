//! Markdown adapter for anneal v2.

use anneal_core::{
    ConfigKey, FactBatch, FactBatchMode, Pattern, Source, SourceCapabilities, SourceContext,
    SourceError, SourceInfo, SourceName, default_lexical_search_info,
};

const SOURCE_NAME: &str = "markdown";

/// Markdown `Source` implementation.
#[derive(Clone, Debug, Default)]
pub struct MarkdownSource;

impl Source for MarkdownSource {
    fn describe(&self) -> SourceInfo {
        SourceInfo {
            name: SOURCE_NAME,
            recognizes: vec![Pattern::new("**/*.md")],
            doc: "Extracts markdown files through the v1 parse/resolve pipeline and emits stored relation facts.",
            config_keys: vec![
                ConfigKey::required("md.file_extension"),
                ConfigKey::required("md.scan_root"),
                ConfigKey::optional("md.scan_exclude"),
                ConfigKey::optional("md.label_pattern"),
                ConfigKey::optional("md.linear_namespace"),
                ConfigKey::optional("md.version_pattern"),
                ConfigKey::optional("md.section_min_depth"),
                ConfigKey::optional("md.section_max_depth"),
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
        let mut combined = FactBatch::new(
            cx.corpus.clone(),
            SourceName::from(SOURCE_NAME),
            FactBatchMode::FullSnapshot,
            generation,
        );
        for root in cx.roots {
            cx.cancellation.check()?;
            let batch = anneal_legacy::v2_adapter::extract_markdown_facts(
                root,
                cx.corpus.clone(),
                SourceName::from(SOURCE_NAME),
                generation,
            )
            .map_err(|err| SourceError::Other(err.to_string()))?;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use anneal_core::{
        ActorContext, CancellationToken, ConfigFacts, CorpusId, FactStore, Generation,
        SourceContext,
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
            root.join("anneal.toml"),
            "[handles]\nconfirmed = [\"OQ\"]\n\n[frontmatter.fields.depends-on]\nedge_kind = \"DependsOn\"\ndirection = \"forward\"\n",
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

        let batch = MarkdownSource
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
        let first = MarkdownSource
            .extract(&context(&root, &config, None))
            .expect("first extract");
        store.merge(first).expect("first merge");

        fs::write(
            root.join("a.md"),
            "---\nstatus: active\npurpose: New\n---\n# A\nNew body.\n",
        )
        .expect("write new");
        let second = MarkdownSource
            .extract(&context(&root, &config, Some(Generation::new(1))))
            .expect("second extract");
        store.merge(second).expect("second merge");

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
