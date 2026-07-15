//! Rust code adapter for anneal.
//!
//! This crate ingests pre-built `rustdoc --output-format json` and EEP-48 artifacts.
//! It does not build rustdoc artifacts or ingest source bodies.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufReader, Cursor};
use std::process::Command;

use anneal_core::{
    ConcernFact, ConfigFacts, ConfigKey, ContentFact, EdgeFact, FactBatch, FactBatchMode,
    FactIdentity, HandleFact, MetaFact, NativeId, OriginUri, Pattern, Revision, Source,
    SourceCapabilities, SourceContext, SourceError, SourceInfo, SourceName, SpanFact,
    default_lexical_search_info, fnv1a_64, normalize_path_inside_root, normalize_relative_path,
};
use beam_file::RawBeamFile;
use camino::{Utf8Path, Utf8PathBuf};
use eetf::Term as EetfTerm;
use rustdoc_types::{
    Crate as RustdocCrate, FunctionSignature, Id, Impl, Item, ItemEnum, ItemKind, Type, Visibility,
};
use serde_json::Value as JsonValue;

mod classify;
mod config;
mod eep48;
mod emit;
mod rustdoc;
mod vocab;

use classify::SourceTreeClassification;
use config::{
    ArtifactManifest, CodeDiscoveryConfig, ensure_source_root_within_project, read_manifest,
};
use eep48::extract_eep48_set;
use emit::{
    ContentBudgetReport, area_for, code_identity, emit_content_budget_meta,
    ensure_external_code_handle, first_paragraph, git_version_tags, item_kind_name, meta_values,
    normalize_code_source_path, package_root_file, push_code_meta, push_meta_fact, token_count,
    truncate_at_char_boundary, version_handle_id, visibility_name,
};
use rustdoc::{
    content_text, extract_rustdoc, markdown_links, signature_type_refs, stable_fragment,
};
use vocab::{
    DEFAULT_CONTENT_BUDGET_BYTES, DEFAULT_MEMBER_DOC_BUDGET_BYTES, DEFAULT_SOURCE_EXTENSIONS,
    SOURCE_NAME, concern_name, config_key, edge_kind, meta_key, relation_value,
};

/// Rustdoc JSON `Source` implementation.
#[derive(Clone, Debug, Default)]
pub struct CodeSource;

impl CodeSource {
    #[must_use]
    pub fn is_configured(config: &ConfigFacts) -> bool {
        config.values(config_key::RUSTDOC_JSON).next().is_some()
            || config.values(config_key::EEP48_BEAM).next().is_some()
            || config.values(config_key::EEP48_BEAM_DIR).next().is_some()
            || config.values(config_key::EEP48_DOC_CHUNK).next().is_some()
            || config.first(config_key::SOURCE_ROOT).is_some()
    }
}

impl Source for CodeSource {
    fn describe(&self) -> SourceInfo {
        SourceInfo {
            name: SOURCE_NAME,
            recognizes: vec![Pattern::new("**/*.json")],
            doc: "Extracts code graph facts from pre-built rustdoc JSON and EEP-48 artifacts, or from a bare source tree (source_root alone) as a language-agnostic file-level corpus.",
            config_keys: vec![
                ConfigKey::optional_exact(config_key::EEP48_BEAM, 1),
                ConfigKey::optional_exact(config_key::EEP48_BEAM_DIR, 1),
                ConfigKey::optional_exact(config_key::EEP48_DOC_CHUNK, 1),
                ConfigKey::optional_exact(config_key::RUSTDOC_JSON, 1),
                ConfigKey::optional_exact(config_key::SOURCE_ROOT, 1),
                ConfigKey::optional(config_key::SOURCE_EXTENSION),
                ConfigKey::optional_exact(config_key::PACKAGE, 1),
                ConfigKey::optional_exact(config_key::ARTIFACT_MANIFEST, 1),
                ConfigKey::optional_exact(config_key::ARTIFACT_REVISION, 1),
                ConfigKey::optional_exact(config_key::CONTENT_BUDGET_BYTES, 1),
                ConfigKey::optional_exact(config_key::MEMBER_DOC_BUDGET_BYTES, 1),
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

        let config = CodeDiscoveryConfig::from_facts(cx.config_facts)?;
        let generation = cx.next_generation();
        let mut combined = FactBatch::new(
            cx.corpus.clone(),
            SourceName::from(SOURCE_NAME),
            FactBatchMode::FullSnapshot,
            generation,
        );
        if !Self::is_configured(cx.config_facts) {
            return Ok(combined);
        }

        for root in cx.roots {
            cx.cancellation.check()?;
            let mut root_batch = FactBatch::new(
                cx.corpus.clone(),
                SourceName::from(SOURCE_NAME),
                FactBatchMode::FullSnapshot,
                generation,
            );
            let manifest = config
                .manifest
                .as_ref()
                .map(|path| read_manifest(root, path))
                .transpose()?;
            let manifest_revision = manifest
                .as_ref()
                .and_then(ArtifactManifest::source_revision);
            let classification_revision = config
                .artifact_revision
                .as_deref()
                .or(manifest_revision.as_deref());
            if config
                .source_root
                .components()
                .any(|component| component.as_str() == "..")
            {
                ensure_source_root_within_project(root, &config.source_root)?;
            }
            let classification = SourceTreeClassification::scan(
                root,
                &config.source_root,
                &config.source_extensions,
            )?;
            for artifact in &config.artifacts {
                let batch = extract_rustdoc(root, cx, &config, manifest.as_ref(), artifact)?;
                root_batch.append(batch);
            }
            if !config.eep48_beams.is_empty()
                || !config.eep48_beam_dirs.is_empty()
                || !config.eep48_doc_chunks.is_empty()
            {
                let batch = extract_eep48_set(root, cx, &config, manifest.as_ref())?;
                root_batch.append(batch);
            }
            classification.project(
                &mut root_batch,
                root,
                &config.source_root,
                classification_revision,
            );
            combined.append(root_batch);
        }
        Ok(combined)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
