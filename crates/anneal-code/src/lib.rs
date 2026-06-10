//! Rust code adapter for anneal.
//!
//! This crate ingests pre-built `rustdoc --output-format json` artifacts.
//! It does not build rustdoc artifacts, scan source bodies, or inspect git.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufReader;

use anneal_core::{
    ConfigFacts, ConfigKey, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
    HandleFact, MetaFact, NativeId, OriginUri, Pattern, Revision, Source, SourceCapabilities,
    SourceContext, SourceError, SourceInfo, SourceName, SpanFact, default_lexical_search_info,
    fnv1a_64, normalize_path_inside_root, normalize_relative_path,
};
use camino::{Utf8Path, Utf8PathBuf};
use rustdoc_types::{
    Crate as RustdocCrate, FunctionSignature, Id, Impl, Item, ItemEnum, ItemKind, Type, Visibility,
};
use serde_json::Value as JsonValue;

const SOURCE_NAME: &str = "code";
const DEFAULT_CONTENT_BUDGET_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_MEMBER_DOC_BUDGET_BYTES: usize = 16 * 1024;

mod config_key {
    pub(super) const RUSTDOC_JSON: &str = "code.rustdoc_json";
    pub(super) const SOURCE_ROOT: &str = "code.source_root";
    pub(super) const PACKAGE: &str = "code.package";
    pub(super) const ARTIFACT_MANIFEST: &str = "code.artifact_manifest";
    pub(super) const ARTIFACT_REVISION: &str = "code.artifact_revision";
    pub(super) const CONTENT_BUDGET_BYTES: &str = "code.content_budget_bytes";
    pub(super) const MEMBER_DOC_BUDGET_BYTES: &str = "code.member_doc_budget_bytes";
}

mod meta_key {
    pub(super) const QUALIFIED_NAME: &str = "code.qualified_name";
    pub(super) const KIND: &str = "code.kind";
    pub(super) const VISIBILITY: &str = "code.visibility";
    pub(super) const DEPRECATED_NOTE: &str = "code.deprecated.note";
    pub(super) const DEPRECATED_SINCE: &str = "code.deprecated.since";
    pub(super) const PACKAGE: &str = "code.package";
    pub(super) const ARTIFACT_PATH: &str = "code.artifact.path";
    pub(super) const ARTIFACT_FORMAT: &str = "code.artifact.format";
    pub(super) const ARTIFACT_FORMAT_VERSION: &str = "code.artifact.format_version";
    pub(super) const ARTIFACT_REVISION: &str = "code.artifact.revision";
    pub(super) const ARTIFACT_REVISION_STATE: &str = "code.artifact.revision_state";
    pub(super) const PACKAGE_VERSION: &str = "code.package.version";
    pub(super) const IMPLEMENTS_KIND: &str = "code.implements.kind";
    pub(super) const IMPLEMENTS_SIGNATURE: &str = "code.implements.signature";
    pub(super) const CONTENT_TRUNCATED: &str = "code.content_truncated";
    pub(super) const CONTENT_BUDGET_DISPOSITION: &str = "code.content_budget_disposition";
    pub(super) const EXTERNAL_CLASS: &str = "code.external_class";
    pub(super) const CONTENT_BUDGET_ROOT_DISPOSITION: &str = "code.content_budget.disposition";
    pub(super) const CONTENT_BUDGET_BYTES: &str = "code.content_budget.bytes";
    pub(super) const MEMBER_DOC_ITEM_BYTES: &str = "code.content_budget.member_doc_item_bytes";
    pub(super) const DOC_BYTES: &str = "code.content.doc_bytes";
    pub(super) const MEMBER_DOC_BYTES: &str = "code.content.member_doc_bytes";
    pub(super) const STRUCTURAL_DOC_BYTES: &str = "code.content.structural_doc_bytes";
    pub(super) const SIGNATURE_BYTES: &str = "code.content.signature_bytes";
}

mod relation_value {
    pub(super) const ARTIFACT_REVISION_UNKNOWN: &str = "artifact_revision_unknown";
    pub(super) const BUDGET_COMPLETE: &str = "complete";
    pub(super) const BUDGET_TRUNCATED: &str = "truncated";
    pub(super) const FIRST_PARAGRAPH: &str = "first_paragraph";
    pub(super) const PER_ITEM_CAP: &str = "per_item_cap";
}

mod edge_kind {
    pub(super) const CONTAINS: &str = "Contains";
    pub(super) const CITES: &str = "Cites";
    pub(super) const IMPLEMENTS: &str = "Implements";
    pub(super) const USES_TYPE: &str = "UsesType";
}

/// Rustdoc JSON `Source` implementation.
#[derive(Clone, Debug, Default)]
pub struct CodeSource;

impl Source for CodeSource {
    fn describe(&self) -> SourceInfo {
        SourceInfo {
            name: SOURCE_NAME,
            recognizes: vec![Pattern::new("**/*.json")],
            doc: "Extracts pre-built rustdoc JSON artifacts into code graph facts.",
            config_keys: vec![
                ConfigKey::optional_exact(config_key::RUSTDOC_JSON, 1),
                ConfigKey::optional_exact(config_key::SOURCE_ROOT, 1),
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
        if config.artifacts.is_empty() {
            return Ok(combined);
        }

        for root in cx.roots {
            cx.cancellation.check()?;
            let manifest = config
                .manifest
                .as_ref()
                .map(|path| read_manifest(root, path))
                .transpose()?;
            for artifact in &config.artifacts {
                let batch = extract_rustdoc(root, cx, &config, manifest.as_ref(), artifact)?;
                combined.append(batch);
            }
        }
        Ok(combined)
    }
}

#[derive(Clone, Debug)]
struct CodeDiscoveryConfig {
    artifacts: Vec<Utf8PathBuf>,
    source_root: Utf8PathBuf,
    package: Option<String>,
    manifest: Option<Utf8PathBuf>,
    artifact_revision: Option<String>,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
}

impl CodeDiscoveryConfig {
    fn from_facts(facts: &ConfigFacts) -> Result<Self, SourceError> {
        let artifacts = facts
            .values(config_key::RUSTDOC_JSON)
            .map(valid_relative_path)
            .collect::<Result<Vec<_>, _>>()?;
        let source_root = facts
            .first(config_key::SOURCE_ROOT)
            .map(valid_relative_path)
            .transpose()?
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        let manifest = facts
            .first(config_key::ARTIFACT_MANIFEST)
            .map(valid_relative_path)
            .transpose()?;
        let package = facts.first(config_key::PACKAGE).map(ToOwned::to_owned);
        let artifact_revision = facts
            .first(config_key::ARTIFACT_REVISION)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let content_budget_bytes = facts
            .first(config_key::CONTENT_BUDGET_BYTES)
            .map(parse_usize_config)
            .transpose()?
            .unwrap_or(DEFAULT_CONTENT_BUDGET_BYTES);
        let member_doc_budget_bytes = facts
            .first(config_key::MEMBER_DOC_BUDGET_BYTES)
            .map(parse_usize_config)
            .transpose()?
            .unwrap_or(DEFAULT_MEMBER_DOC_BUDGET_BYTES);

        Ok(Self {
            artifacts,
            source_root,
            package,
            manifest,
            artifact_revision,
            content_budget_bytes,
            member_doc_budget_bytes,
        })
    }
}

fn parse_usize_config(value: &str) -> Result<usize, SourceError> {
    value.parse::<usize>().map_err(|source| {
        SourceError::InvalidConfig(format!("code budget values must be byte counts: {source}"))
    })
}

fn valid_relative_path(value: &str) -> Result<Utf8PathBuf, SourceError> {
    normalize_relative_path(value, anneal_core::RelativePathPolicy::ALLOW_EMPTY).ok_or_else(|| {
        SourceError::InvalidConfig(format!(
            "code paths must be relative paths inside the corpus root; got {value:?}"
        ))
    })
}

fn extract_rustdoc(
    root: &Utf8Path,
    cx: &SourceContext<'_>,
    config: &CodeDiscoveryConfig,
    manifest: Option<&ArtifactManifest>,
    artifact: &Utf8Path,
) -> Result<FactBatch, SourceError> {
    let artifact_path = root.join(artifact);
    let artifact_file =
        File::open(&artifact_path).map_err(|source| SourceError::io(&artifact_path, source))?;
    let rustdoc: RustdocCrate = serde_json::from_reader(BufReader::new(artifact_file))
        .map_err(|source| SourceError::Other(format!("{artifact_path}: {source}")))?;

    let source_revision = config
        .artifact_revision
        .clone()
        .or_else(|| manifest.and_then(ArtifactManifest::source_revision))
        .unwrap_or_else(|| relation_value::ARTIFACT_REVISION_UNKNOWN.to_string());
    let package = config
        .package
        .clone()
        .or_else(|| manifest.and_then(ArtifactManifest::package_name))
        .or_else(|| crate_name_from_root(&rustdoc))
        .unwrap_or_else(|| "rustdoc".to_string());

    let mut batch = FactBatch::new(
        cx.corpus.clone(),
        SourceName::from(SOURCE_NAME),
        FactBatchMode::FullSnapshot,
        cx.next_generation(),
    );
    let mut projector = RustdocProjector::new(ProjectorInput {
        root,
        source_root: &config.source_root,
        artifact,
        revision: source_revision,
        package,
        content_budget_bytes: config.content_budget_bytes,
        member_doc_budget_bytes: config.member_doc_budget_bytes,
        rustdoc: &rustdoc,
    });
    projector.project(&mut batch)?;
    Ok(batch)
}

fn read_manifest(root: &Utf8Path, manifest: &Utf8Path) -> Result<ArtifactManifest, SourceError> {
    let path = root.join(manifest);
    let text = fs::read_to_string(&path).map_err(|source| SourceError::io(&path, source))?;
    let value = serde_json::from_str::<JsonValue>(&text)
        .map_err(|source| SourceError::Other(format!("{path}: {source}")))?;
    Ok(ArtifactManifest { value })
}

#[derive(Clone, Debug)]
struct ArtifactManifest {
    value: JsonValue,
}

impl ArtifactManifest {
    fn source_revision(&self) -> Option<String> {
        self.string_at(&[
            &["source_revision"],
            &["source", "revision"],
            &["artifact", "source_revision"],
        ])
    }

    fn package_name(&self) -> Option<String> {
        self.string_at(&[
            &["package"],
            &["package_name"],
            &["package", "name"],
            &["crate"],
            &["crate_name"],
        ])
    }

    fn string_at(&self, paths: &[&[&str]]) -> Option<String> {
        paths.iter().find_map(|path| {
            let mut value = &self.value;
            for key in *path {
                value = value.get(*key)?;
            }
            value.as_str().map(ToOwned::to_owned)
        })
    }
}

struct RustdocProjector<'a> {
    root: &'a Utf8Path,
    source_root: Utf8PathBuf,
    artifact: &'a Utf8Path,
    revision: Revision,
    revision_text: String,
    package: String,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
    rustdoc: &'a RustdocCrate,
    local_handles: BTreeMap<Id, String>,
    local_files: BTreeMap<Id, String>,
    external_handles: BTreeMap<Id, String>,
    file_handles: BTreeSet<String>,
    used_handles: BTreeSet<String>,
    package_handle: String,
    budget: ContentBudgetReport,
}

struct ProjectorInput<'a> {
    root: &'a Utf8Path,
    source_root: &'a Utf8Path,
    artifact: &'a Utf8Path,
    revision: String,
    package: String,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
    rustdoc: &'a RustdocCrate,
}

impl<'a> RustdocProjector<'a> {
    fn new(input: ProjectorInput<'a>) -> Self {
        let package_handle = package_root_file(input.root, input.source_root);
        Self {
            root: input.root,
            source_root: input.source_root.to_path_buf(),
            artifact: input.artifact,
            revision: Revision::from(input.revision.clone()),
            revision_text: input.revision,
            package: input.package,
            content_budget_bytes: input.content_budget_bytes,
            member_doc_budget_bytes: input.member_doc_budget_bytes,
            rustdoc: input.rustdoc,
            local_handles: BTreeMap::new(),
            local_files: BTreeMap::new(),
            external_handles: BTreeMap::new(),
            file_handles: BTreeSet::new(),
            used_handles: BTreeSet::new(),
            package_handle,
            budget: ContentBudgetReport::default(),
        }
    }

    fn project(&mut self, batch: &mut FactBatch) -> Result<(), SourceError> {
        self.index_handles();
        self.emit_file_handles(batch);
        self.emit_package_meta(batch);
        self.emit_item_handles(batch);
        self.emit_structure_edges(batch);
        self.emit_type_edges(batch)?;
        self.emit_doc_link_edges(batch);
        self.emit_impl_edges(batch);
        self.emit_content(batch);
        self.emit_budget_meta(batch);
        Ok(())
    }

    fn index_handles(&mut self) {
        self.file_handles.insert(self.package_handle.clone());
        let mut items = self.local_items().map(|(id, _)| *id).collect::<Vec<_>>();
        items.sort_by_key(|id| {
            let item = self.rustdoc.index.get(id).expect("local item exists");
            (
                self.item_file(item)
                    .unwrap_or_else(|| self.package_handle.clone()),
                qualified_name(self.rustdoc, *id).unwrap_or_default(),
                item_kind_name(item.inner.item_kind()),
                item.span.as_ref().map_or(0, |span| span.begin.0),
            )
        });

        for id in items {
            let item = self.rustdoc.index.get(&id).expect("local item exists");
            if matches!(item.inner, ItemEnum::Impl(_)) {
                continue;
            }
            let file = self
                .item_file(item)
                .unwrap_or_else(|| self.package_handle.clone());
            self.file_handles.insert(file.clone());
            let local = adapter_local_id(self.rustdoc, id, item, &file);
            let handle = self.unique_handle(format!("{file}#{local}"));
            self.local_files.insert(id, file);
            self.local_handles.insert(id, handle);
        }
    }

    fn local_items(&self) -> impl Iterator<Item = (&Id, &Item)> {
        self.rustdoc
            .index
            .iter()
            .filter(|(_, item)| item.crate_id == 0)
    }

    fn unique_handle(&mut self, base: String) -> String {
        if self.used_handles.insert(base.clone()) {
            return base;
        }
        for ordinal in 2usize.. {
            let candidate = format!("{base}~{ordinal}");
            if self.used_handles.insert(candidate.clone()) {
                return candidate;
            }
        }
        unreachable!("unbounded ordinal loop returns");
    }

    fn item_file(&self, item: &Item) -> Option<String> {
        item.span
            .as_ref()
            .and_then(|span| normalize_span_filename(self.root, &self.source_root, &span.filename))
    }

    fn emit_file_handles(&self, batch: &mut FactBatch) {
        for file in &self.file_handles {
            let summary = if file == &self.package_handle {
                format!("{} rustdoc package root", self.package)
            } else {
                file.clone()
            };
            batch.handles.push(HandleFact {
                identity: self.identity_for(batch, file, file),
                id: file.clone(),
                kind: "file".to_string(),
                status: None,
                namespace: String::new(),
                file: file.clone(),
                line: 1,
                date: None,
                area: area_for(file),
                summary,
            });
        }
    }

    fn emit_package_meta(&self, batch: &mut FactBatch) {
        let identity = self.identity_for(batch, &self.package_handle, &self.package_handle);
        Self::push_meta(
            batch,
            &identity,
            &self.package_handle,
            meta_key::PACKAGE,
            &self.package,
        );
        Self::push_meta(
            batch,
            &identity,
            &self.package_handle,
            meta_key::ARTIFACT_PATH,
            self.artifact.as_str(),
        );
        Self::push_meta(
            batch,
            &identity,
            &self.package_handle,
            meta_key::ARTIFACT_FORMAT,
            "rustdoc-json",
        );
        Self::push_meta(
            batch,
            &identity,
            &self.package_handle,
            meta_key::ARTIFACT_FORMAT_VERSION,
            &self.rustdoc.format_version.to_string(),
        );
        Self::push_meta(
            batch,
            &identity,
            &self.package_handle,
            meta_key::ARTIFACT_REVISION,
            &self.revision_text,
        );
        if self.revision_text == relation_value::ARTIFACT_REVISION_UNKNOWN {
            Self::push_meta(
                batch,
                &identity,
                &self.package_handle,
                meta_key::ARTIFACT_REVISION_STATE,
                relation_value::ARTIFACT_REVISION_UNKNOWN,
            );
        }
        if let Some(version) = &self.rustdoc.crate_version {
            Self::push_meta(
                batch,
                &identity,
                &self.package_handle,
                meta_key::PACKAGE_VERSION,
                version,
            );
        }
    }

    fn emit_item_handles(&self, batch: &mut FactBatch) {
        for (id, handle) in &self.local_handles {
            let Some(item) = self.rustdoc.index.get(id) else {
                continue;
            };
            let file = self
                .local_files
                .get(id)
                .cloned()
                .unwrap_or_else(|| self.package_handle.clone());
            let line = item
                .span
                .as_ref()
                .and_then(|span| u32::try_from(span.begin.0).ok())
                .unwrap_or(1);
            let qualified = qualified_name(self.rustdoc, *id)
                .or_else(|| item.name.clone())
                .unwrap_or_else(|| handle.clone());
            let kind = item_kind_name(item.inner.item_kind());
            batch.handles.push(HandleFact {
                identity: self.identity_for(batch, handle, &file),
                id: handle.clone(),
                kind: "section".to_string(),
                status: item.deprecation.as_ref().map(|_| "deprecated".to_string()),
                namespace: kind.to_string(),
                file: file.clone(),
                line,
                date: None,
                area: area_for(&file),
                summary: item_summary(item, &qualified),
            });

            let identity = self.identity_for(batch, handle, &file);
            Self::push_meta(
                batch,
                &identity,
                handle,
                meta_key::QUALIFIED_NAME,
                &qualified,
            );
            Self::push_meta(batch, &identity, handle, meta_key::KIND, kind);
            Self::push_meta(
                batch,
                &identity,
                handle,
                meta_key::VISIBILITY,
                visibility_name(&item.visibility),
            );
            if let Some(deprecation) = &item.deprecation {
                if let Some(note) = &deprecation.note {
                    Self::push_meta(batch, &identity, handle, meta_key::DEPRECATED_NOTE, note);
                }
                if let Some(since) = &deprecation.since {
                    Self::push_meta(batch, &identity, handle, meta_key::DEPRECATED_SINCE, since);
                }
            }
        }
    }

    fn emit_structure_edges(&self, batch: &mut FactBatch) {
        let mut ordinal = 0usize;
        for (parent_id, item) in self.local_items() {
            let Some(parent) = self.local_handles.get(parent_id) else {
                continue;
            };
            for child_id in direct_children(&item.inner) {
                if let Some(child) = self.local_handles.get(&child_id) {
                    self.push_edge(batch, parent, child, edge_kind::CONTAINS, item, ordinal);
                    ordinal += 1;
                }
            }
        }
    }

    fn emit_type_edges(&mut self, batch: &mut FactBatch) -> Result<(), SourceError> {
        let mut ordinal = 0usize;
        let ids = self.local_handles.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let Some(from) = self.local_handles.get(&id).cloned() else {
                continue;
            };
            let item = self
                .rustdoc
                .index
                .get(&id)
                .expect("indexed local item exists");
            let targets = resolved_path_ids(&item.inner)?;
            let mut seen = BTreeSet::new();
            for target_id in targets {
                if target_id == id || !seen.insert(target_id) {
                    continue;
                }
                let target = self.handle_for_target(batch, target_id);
                self.push_edge(batch, &from, &target, edge_kind::USES_TYPE, item, ordinal);
                ordinal += 1;
            }
        }
        Ok(())
    }

    fn emit_doc_link_edges(&mut self, batch: &mut FactBatch) {
        let mut ordinal = 0usize;
        let ids = self.local_handles.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let Some(from) = self.local_handles.get(&id).cloned() else {
                continue;
            };
            let item = self
                .rustdoc
                .index
                .get(&id)
                .expect("indexed local item exists");
            for target_id in item.links.values().copied() {
                let target = self.handle_for_target(batch, target_id);
                self.push_edge(batch, &from, &target, edge_kind::CITES, item, ordinal);
                ordinal += 1;
            }
        }
    }

    fn emit_impl_edges(&mut self, batch: &mut FactBatch) {
        let mut ordinal = 0usize;
        let impls = self
            .local_items()
            .filter_map(|(_, item)| match &item.inner {
                ItemEnum::Impl(impl_) if impl_.trait_.is_some() => {
                    Some((item.clone(), impl_.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (item, impl_) in impls {
            let Some(trait_) = &impl_.trait_ else {
                continue;
            };
            let from = self.handle_for_impl_type(batch, &impl_.for_);
            let to = self.handle_for_target(batch, trait_.id);
            let edge = self.push_edge(batch, &from, &to, edge_kind::IMPLEMENTS, &item, ordinal);
            let file = self.item_file(&item).unwrap_or_default();
            let identity = self.identity_for(batch, &edge, &file);
            Self::push_meta(
                batch,
                &identity,
                &edge,
                meta_key::IMPLEMENTS_KIND,
                impl_kind(&impl_),
            );
            Self::push_meta(
                batch,
                &identity,
                &edge,
                meta_key::IMPLEMENTS_SIGNATURE,
                &impl_signature(&impl_),
            );
            ordinal += 1;
        }
    }

    fn emit_content(&mut self, batch: &mut FactBatch) {
        self.budget = self.content_budget_report();
        let mut ids = self.local_handles.keys().copied().collect::<Vec<_>>();
        ids.sort_by_key(|id| self.local_handles.get(id).cloned().unwrap_or_default());

        for id in ids {
            let Some(handle) = self.local_handles.get(&id).cloned() else {
                continue;
            };
            let item = self
                .rustdoc
                .index
                .get(&id)
                .expect("indexed local item exists");
            let file = self
                .local_files
                .get(&id)
                .cloned()
                .unwrap_or_else(|| self.package_handle.clone());
            let qualified = qualified_name(self.rustdoc, id)
                .or_else(|| item.name.clone())
                .unwrap_or_else(|| handle.clone());
            let signature = item_signature(item, &qualified);
            let raw_docs = item.docs.as_deref().unwrap_or_default();
            let (docs, disposition) = self.budgeted_docs(item, raw_docs);
            let text = content_text(&signature, docs);
            if text.trim().is_empty() {
                continue;
            }

            let span_id = format!("{handle}#docs");
            let start_line = item
                .span
                .as_ref()
                .and_then(|span| u32::try_from(span.begin.0).ok())
                .unwrap_or(1);
            let lines = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
            let identity = self.identity_for(batch, &span_id, &file);
            let summary = signature
                .lines()
                .next()
                .filter(|line| !line.is_empty())
                .map_or_else(|| item_summary(item, &qualified), ToOwned::to_owned);
            batch.spans.push(SpanFact {
                identity: identity.clone(),
                id: span_id.clone(),
                handle: handle.clone(),
                start_line,
                end_line: start_line.saturating_add(lines.saturating_sub(1)),
                summary,
            });
            batch.content.push(ContentFact {
                identity,
                handle: handle.clone(),
                span_id,
                lines,
                tokens: token_count(&text),
                text,
            });
            if let Some(disposition) = disposition {
                let identity = self.identity_for(batch, &handle, &file);
                Self::push_meta(
                    batch,
                    &identity,
                    &handle,
                    meta_key::CONTENT_TRUNCATED,
                    "true",
                );
                Self::push_meta(
                    batch,
                    &identity,
                    &handle,
                    meta_key::CONTENT_BUDGET_DISPOSITION,
                    disposition,
                );
            }
        }
    }

    fn budgeted_docs<'b>(&self, item: &Item, docs: &'b str) -> (&'b str, Option<&'static str>) {
        if docs.is_empty() || self.budget.disposition == relation_value::BUDGET_COMPLETE {
            return (docs, None);
        }
        if !is_member_item(item.inner.item_kind()) {
            return (docs, None);
        }
        if docs.len() <= self.member_doc_budget_bytes {
            return (docs, None);
        }
        let paragraph = first_paragraph(docs);
        if !paragraph.is_empty() && paragraph.len() <= self.member_doc_budget_bytes {
            return (paragraph, Some(relation_value::FIRST_PARAGRAPH));
        }
        (
            truncate_at_char_boundary(docs, self.member_doc_budget_bytes),
            Some(relation_value::PER_ITEM_CAP),
        )
    }

    fn content_budget_report(&self) -> ContentBudgetReport {
        let mut report = ContentBudgetReport {
            content_budget_bytes: self.content_budget_bytes,
            member_doc_budget_bytes: self.member_doc_budget_bytes,
            ..ContentBudgetReport::default()
        };
        for (id, item) in self.local_items() {
            let Some(handle) = self.local_handles.get(id) else {
                continue;
            };
            let qualified = qualified_name(self.rustdoc, *id)
                .or_else(|| item.name.clone())
                .unwrap_or_else(|| handle.clone());
            let signature = item_signature(item, &qualified);
            report.signature_bytes += signature.len();
            let doc_bytes = item.docs.as_deref().unwrap_or_default().len();
            report.doc_bytes += doc_bytes;
            if is_member_item(item.inner.item_kind()) {
                report.member_doc_bytes += doc_bytes;
            } else {
                report.structural_doc_bytes += doc_bytes;
            }
        }
        report.disposition =
            if report.doc_bytes + report.signature_bytes > self.content_budget_bytes {
                relation_value::BUDGET_TRUNCATED.to_string()
            } else {
                relation_value::BUDGET_COMPLETE.to_string()
            };
        report
    }

    fn emit_budget_meta(&self, batch: &mut FactBatch) {
        let identity = self.identity_for(batch, &self.package_handle, &self.package_handle);
        let values = [
            (
                meta_key::CONTENT_BUDGET_ROOT_DISPOSITION,
                self.budget.disposition.as_str(),
            ),
            (
                meta_key::CONTENT_BUDGET_BYTES,
                &self.budget.content_budget_bytes.to_string(),
            ),
            (
                meta_key::MEMBER_DOC_ITEM_BYTES,
                &self.budget.member_doc_budget_bytes.to_string(),
            ),
            (meta_key::DOC_BYTES, &self.budget.doc_bytes.to_string()),
            (
                meta_key::MEMBER_DOC_BYTES,
                &self.budget.member_doc_bytes.to_string(),
            ),
            (
                meta_key::STRUCTURAL_DOC_BYTES,
                &self.budget.structural_doc_bytes.to_string(),
            ),
            (
                meta_key::SIGNATURE_BYTES,
                &self.budget.signature_bytes.to_string(),
            ),
        ];
        for (key, value) in values {
            Self::push_meta(batch, &identity, &self.package_handle, key, value);
        }
    }

    fn handle_for_target(&mut self, batch: &mut FactBatch, id: Id) -> String {
        if let Some(handle) = self.local_handles.get(&id) {
            return handle.clone();
        }
        if let Some(handle) = self.external_handles.get(&id) {
            return handle.clone();
        }
        let qualified =
            qualified_name(self.rustdoc, id).unwrap_or_else(|| format!("rustdoc-id-{}", id.0));
        let handle = format!("rustdoc://{}", stable_fragment(&qualified));
        self.external_handles.insert(id, handle.clone());
        self.emit_external_handle(batch, &handle, &qualified);
        handle
    }

    fn handle_for_impl_type(&mut self, batch: &mut FactBatch, type_: &Type) -> String {
        if let Some(id) = type_target_id(type_)
            && let Some(handle) = self.local_handles.get(&id)
        {
            return handle.clone();
        }
        if let Some(id) = type_target_id(type_) {
            return self.handle_for_target(batch, id);
        }
        let qualified = type_label(type_);
        let synthetic_id = Id(
            u32::try_from(fnv1a_64(qualified.as_bytes()) & u64::from(u32::MAX)).unwrap_or(u32::MAX),
        );
        if let Some(handle) = self.external_handles.get(&synthetic_id) {
            return handle.clone();
        }
        let handle = format!("rustdoc://{}", stable_fragment(&qualified));
        self.external_handles.insert(synthetic_id, handle.clone());
        self.emit_external_handle(batch, &handle, &qualified);
        handle
    }

    fn emit_external_handle(&self, batch: &mut FactBatch, handle: &str, qualified: &str) {
        let identity = self.identity_for(batch, handle, "");
        batch.handles.push(HandleFact {
            identity: identity.clone(),
            id: handle.to_string(),
            kind: "external".to_string(),
            status: None,
            namespace: "code".to_string(),
            file: String::new(),
            line: 0,
            date: None,
            area: String::new(),
            summary: qualified.to_string(),
        });
        Self::push_meta(
            batch,
            &identity,
            handle,
            meta_key::QUALIFIED_NAME,
            qualified,
        );
        Self::push_meta(
            batch,
            &identity,
            handle,
            meta_key::EXTERNAL_CLASS,
            SOURCE_NAME,
        );
    }

    fn push_edge(
        &self,
        batch: &mut FactBatch,
        from: &str,
        to: &str,
        kind: &str,
        item: &Item,
        ordinal: usize,
    ) -> String {
        let file = self.item_file(item).unwrap_or_default();
        let line = item
            .span
            .as_ref()
            .and_then(|span| u32::try_from(span.begin.0).ok())
            .unwrap_or(0);
        let native_id = format!("{from}::edge::{ordinal}::{kind}::{to}::{line}");
        batch.edges.push(EdgeFact {
            identity: self.identity_for(batch, &native_id, &file),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file,
            line,
            assertion_date: None,
            assertion_revision: None,
        });
        native_id
    }

    fn push_meta(
        batch: &mut FactBatch,
        identity: &FactIdentity,
        handle: &str,
        key: &str,
        value: &str,
    ) {
        batch.meta.push(MetaFact {
            identity: identity.clone(),
            handle: handle.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    fn identity_for(&self, batch: &FactBatch, native_id: &str, file: &str) -> FactIdentity {
        let origin_uri = if file.is_empty() {
            format!("rustdoc://{native_id}")
        } else {
            format!("file://{}", self.root.join(file))
        };
        FactIdentity::new(
            batch.corpus.clone(),
            batch.source.clone(),
            NativeId::from(native_id.to_string()),
            OriginUri::from(origin_uri),
            self.revision.clone(),
            batch.generation,
        )
    }
}

#[derive(Clone, Debug)]
struct ContentBudgetReport {
    disposition: String,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
    doc_bytes: usize,
    member_doc_bytes: usize,
    structural_doc_bytes: usize,
    signature_bytes: usize,
}

impl Default for ContentBudgetReport {
    fn default() -> Self {
        Self {
            disposition: relation_value::BUDGET_COMPLETE.to_string(),
            content_budget_bytes: DEFAULT_CONTENT_BUDGET_BYTES,
            member_doc_budget_bytes: DEFAULT_MEMBER_DOC_BUDGET_BYTES,
            doc_bytes: 0,
            member_doc_bytes: 0,
            structural_doc_bytes: 0,
            signature_bytes: 0,
        }
    }
}

fn crate_name_from_root(rustdoc: &RustdocCrate) -> Option<String> {
    rustdoc.index.get(&rustdoc.root).and_then(|item| {
        item.name
            .clone()
            .or_else(|| qualified_name(rustdoc, rustdoc.root))
    })
}

fn package_root_file(root: &Utf8Path, source_root: &Utf8Path) -> String {
    if root.join(source_root).join("Cargo.toml").is_file() {
        let path = source_root.join("Cargo.toml");
        return normalize_relative_path(
            path.as_str(),
            anneal_core::RelativePathPolicy::ALLOW_EMPTY,
        )
        .map_or_else(|| path.to_string(), |path| path.to_string());
    }
    "Cargo.toml".to_string()
}

fn normalize_span_filename(
    root: &Utf8Path,
    source_root: &Utf8Path,
    filename: &std::path::Path,
) -> Option<String> {
    let path = Utf8PathBuf::from_path_buf(filename.to_path_buf()).ok()?;
    let relative = if path.is_absolute() {
        normalize_path_inside_root(root, &path)?
    } else if path.starts_with(source_root) {
        path
    } else {
        source_root.join(path)
    };
    normalize_relative_path(
        relative.as_str(),
        anneal_core::RelativePathPolicy::ALLOW_EMPTY,
    )
    .map(|path| path.to_string())
}

fn qualified_name(rustdoc: &RustdocCrate, id: Id) -> Option<String> {
    rustdoc
        .paths
        .get(&id)
        .and_then(|summary| (!summary.path.is_empty()).then(|| summary.path.join("::")))
}

fn adapter_local_id(rustdoc: &RustdocCrate, id: Id, item: &Item, file: &str) -> String {
    let name = qualified_name(rustdoc, id)
        .or_else(|| item.name.clone())
        .unwrap_or_else(|| {
            let line = item.span.as_ref().map_or(0, |span| span.begin.0);
            format!("{}@{line}", item_kind_name(item.inner.item_kind()))
        });
    let line = item.span.as_ref().map_or(0, |span| span.begin.0);
    let seed = format!(
        "{file}:{line}:{name}:{}",
        item_kind_name(item.inner.item_kind())
    );
    format!("{}@{:x}", stable_fragment(&name), fnv1a_64(seed.as_bytes()))
}

fn stable_fragment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '.') {
            ch
        } else {
            '-'
        };
        if normalized == '-' {
            if !last_dash {
                out.push(normalized);
            }
            last_dash = true;
        } else {
            out.push(normalized);
            last_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn item_summary(item: &Item, qualified: &str) -> String {
    let kind = item_kind_name(item.inner.item_kind());
    item.docs
        .as_deref()
        .and_then(|docs| docs.lines().find(|line| !line.trim().is_empty()))
        .map(str::trim)
        .map_or_else(|| format!("{kind} {qualified}"), ToOwned::to_owned)
}

fn item_signature(item: &Item, qualified: &str) -> String {
    match &item.inner {
        ItemEnum::Function(function) => function_signature(qualified, &function.sig),
        ItemEnum::Struct(_) => format!("struct {qualified}"),
        ItemEnum::Union(_) => format!("union {qualified}"),
        ItemEnum::Enum(_) => format!("enum {qualified}"),
        ItemEnum::Trait(_) => format!("trait {qualified}"),
        ItemEnum::TraitAlias(_) => format!("trait {qualified} = ..."),
        ItemEnum::TypeAlias(alias) => format!("type {qualified} = {}", type_label(&alias.type_)),
        ItemEnum::Constant { type_, .. } => format!("const {qualified}: {}", type_label(type_)),
        ItemEnum::Static(static_) => format!("static {qualified}: {}", type_label(&static_.type_)),
        ItemEnum::StructField(type_) => format!("{qualified}: {}", type_label(type_)),
        ItemEnum::Variant(_) => format!("variant {qualified}"),
        ItemEnum::Macro(value) => {
            if value.trim().is_empty() {
                format!("macro {qualified}")
            } else {
                value.clone()
            }
        }
        ItemEnum::ProcMacro(_) => format!("proc_macro {qualified}"),
        ItemEnum::Module(_) => format!("mod {qualified}"),
        ItemEnum::AssocConst { type_, .. } => {
            format!("const {qualified}: {}", type_label(type_))
        }
        ItemEnum::AssocType { type_, .. } => type_.as_ref().map_or_else(
            || format!("type {qualified}"),
            |type_| format!("type {qualified} = {}", type_label(type_)),
        ),
        _ => format!("{} {qualified}", item_kind_name(item.inner.item_kind())),
    }
}

fn function_signature(qualified: &str, signature: &FunctionSignature) -> String {
    let mut out = String::new();
    let _ = write!(&mut out, "fn {qualified}(");
    for (idx, (name, type_)) in signature.inputs.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        if name.is_empty() {
            out.push_str(&type_label(type_));
        } else {
            let _ = write!(&mut out, "{name}: {}", type_label(type_));
        }
    }
    out.push(')');
    if let Some(output) = &signature.output {
        let _ = write!(&mut out, " -> {}", type_label(output));
    }
    out
}

fn content_text(signature: &str, docs: &str) -> String {
    match (signature.trim().is_empty(), docs.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => signature.to_string(),
        (true, false) => docs.to_string(),
        (false, false) => format!("{signature}\n\n{docs}"),
    }
}

fn type_label(type_: &Type) -> String {
    match type_ {
        Type::ResolvedPath(path) => path.path.clone(),
        Type::DynTrait(value) => value
            .traits
            .iter()
            .map(|trait_| trait_.trait_.path.as_str())
            .collect::<Vec<_>>()
            .join(" + "),
        Type::Generic(value) | Type::Primitive(value) => value.clone(),
        Type::FunctionPointer(pointer) => {
            let inputs = pointer
                .sig
                .inputs
                .iter()
                .map(|(_, type_)| type_label(type_))
                .collect::<Vec<_>>();
            let output = pointer
                .sig
                .output
                .as_ref()
                .map_or_else(|| "()".to_string(), type_label);
            format!("fn({}) -> {output}", inputs.join(", "))
        }
        Type::Tuple(items) => {
            let labels = items.iter().map(type_label).collect::<Vec<_>>();
            format!("({})", labels.join(", "))
        }
        Type::Slice(inner) => format!("[{}]", type_label(inner)),
        Type::Array { type_, len } => format!("[{}; {len}]", type_label(type_)),
        Type::Pat { type_, .. } => type_label(type_),
        Type::ImplTrait(_) => "impl Trait".to_string(),
        Type::Infer => "_".to_string(),
        Type::RawPointer { is_mutable, type_ } => {
            format!(
                "*{} {}",
                if *is_mutable { "mut" } else { "const" },
                type_label(type_)
            )
        }
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let lifetime = lifetime.as_ref().map_or("", String::as_str);
            format!(
                "&{lifetime}{}{}",
                if *is_mutable { " mut " } else { " " },
                type_label(type_)
            )
        }
        Type::QualifiedPath {
            name,
            self_type,
            trait_,
            ..
        } => trait_.as_ref().map_or_else(
            || format!("{}::{name}", type_label(self_type)),
            |trait_| format!("{} as {}::{name}", type_label(self_type), trait_.path),
        ),
    }
}

fn type_target_id(type_: &Type) -> Option<Id> {
    match type_ {
        Type::ResolvedPath(path) => Some(path.id),
        Type::BorrowedRef { type_, .. }
        | Type::RawPointer { type_, .. }
        | Type::Array { type_, .. }
        | Type::Slice(type_)
        | Type::Pat { type_, .. } => type_target_id(type_),
        _ => None,
    }
}

fn direct_children(inner: &ItemEnum) -> Vec<Id> {
    match inner {
        ItemEnum::Module(module) => module.items.clone(),
        ItemEnum::Struct(struct_) => {
            let mut out = match &struct_.kind {
                rustdoc_types::StructKind::Plain { fields, .. } => fields.clone(),
                rustdoc_types::StructKind::Tuple(fields) => {
                    fields.iter().flatten().copied().collect()
                }
                rustdoc_types::StructKind::Unit => Vec::new(),
            };
            out.extend(&struct_.impls);
            out
        }
        ItemEnum::Union(union_) => {
            let mut out = union_.fields.clone();
            out.extend(&union_.impls);
            out
        }
        ItemEnum::Enum(enum_) => {
            let mut out = enum_.variants.clone();
            out.extend(&enum_.impls);
            out
        }
        ItemEnum::Trait(trait_) => {
            let mut out = trait_.items.clone();
            out.extend(&trait_.implementations);
            out
        }
        ItemEnum::Impl(impl_) => impl_.items.clone(),
        _ => Vec::new(),
    }
}

fn resolved_path_ids(inner: &ItemEnum) -> Result<Vec<Id>, SourceError> {
    let value = serde_json::to_value(inner)
        .map_err(|source| SourceError::Other(format!("rustdoc type traversal failed: {source}")))?;
    let mut out = Vec::new();
    collect_resolved_path_ids(&value, &mut out);
    Ok(out)
}

fn collect_resolved_path_ids(value: &JsonValue, out: &mut Vec<Id>) {
    match value {
        JsonValue::Object(map) => {
            if let Some(JsonValue::Object(path)) = map.get("resolved_path")
                && let Some(id) = json_id(path.get("id"))
            {
                out.push(id);
            }
            for child in map.values() {
                collect_resolved_path_ids(child, out);
            }
        }
        JsonValue::Array(values) => {
            for child in values {
                collect_resolved_path_ids(child, out);
            }
        }
        _ => {}
    }
}

fn json_id(value: Option<&JsonValue>) -> Option<Id> {
    value
        .and_then(JsonValue::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .map(Id)
}

fn impl_kind(impl_: &Impl) -> &'static str {
    if impl_.blanket_impl.is_some() {
        "blanket_impl"
    } else {
        "trait_impl"
    }
}

fn impl_signature(impl_: &Impl) -> String {
    let for_type = type_label(&impl_.for_);
    let trait_ = impl_
        .trait_
        .as_ref()
        .map_or_else(|| "inherent".to_string(), |path| path.path.clone());
    if let Some(blanket) = &impl_.blanket_impl {
        format!(
            "impl {trait_} for {for_type} where blanket = {}",
            type_label(blanket)
        )
    } else {
        format!("impl {trait_} for {for_type}")
    }
}

fn is_member_item(kind: ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Function
            | ItemKind::StructField
            | ItemKind::Variant
            | ItemKind::Constant
            | ItemKind::Static
            | ItemKind::AssocConst
            | ItemKind::AssocType
    )
}

fn first_paragraph(text: &str) -> &str {
    text.split("\n\n").next().unwrap_or(text).trim_end()
}

fn truncate_at_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end()
}

fn item_kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Module => "module",
        ItemKind::ExternCrate => "extern_crate",
        ItemKind::Use => "use",
        ItemKind::Struct => "struct",
        ItemKind::StructField => "struct_field",
        ItemKind::Union => "union",
        ItemKind::Enum => "enum",
        ItemKind::Variant => "variant",
        ItemKind::Function => "function",
        ItemKind::TypeAlias => "type_alias",
        ItemKind::Constant => "constant",
        ItemKind::Trait => "trait",
        ItemKind::TraitAlias => "trait_alias",
        ItemKind::Impl => "impl",
        ItemKind::Static => "static",
        ItemKind::ExternType => "extern_type",
        ItemKind::Macro => "macro",
        ItemKind::ProcAttribute => "proc_attribute",
        ItemKind::ProcDerive => "proc_derive",
        ItemKind::AssocConst => "assoc_const",
        ItemKind::AssocType => "assoc_type",
        ItemKind::Primitive => "primitive",
        ItemKind::Keyword => "keyword",
        ItemKind::Attribute => "attribute",
    }
}

fn visibility_name(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Default => "default",
        Visibility::Crate => "crate",
        Visibility::Restricted { .. } => "restricted",
    }
}

fn area_for(file: &str) -> String {
    Utf8Path::new(file)
        .parent()
        .map_or_else(String::new, ToString::to_string)
}

fn token_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use anneal_core::{
        ActorContext, CancellationToken, ConfigEntry, ConfigFacts, CorpusId, Generation,
    };
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn context<'a>(root: &'a Utf8PathBuf, config: &'a ConfigFacts) -> SourceContext<'a> {
        SourceContext {
            corpus: CorpusId::from("test"),
            roots: std::slice::from_ref(root),
            config_facts: config,
            probe_code_target_history: false,
            probe_edge_assertions: false,
            time_ref: None,
            previous_generation: Some(Generation::new(0)),
            actor: ActorContext {
                actor: "test".to_string(),
                capabilities: BTreeSet::new(),
            },
            cancellation: CancellationToken::new(),
        }
    }

    fn write_fixture(root: &Utf8Path) {
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("write cargo");
        fs::write(
            root.join("src/lib.rs"),
            "pub struct Widget;\npub trait Other {}\npub fn make() -> Widget { Widget }\n",
        )
        .expect("write lib");
        let rustdoc = json!({
            "root": 0,
            "crate_version": "0.1.0",
            "includes_private": false,
            "target": {"triple": "x86_64-unknown-linux-gnu", "target_features": []},
            "format_version": rustdoc_types::FORMAT_VERSION,
            "external_crates": {},
            "paths": {
                "0": {"crate_id": 0, "path": ["demo"], "kind": "module"},
                "1": {"crate_id": 0, "path": ["demo", "Widget"], "kind": "struct"},
                "2": {"crate_id": 0, "path": ["demo", "Other"], "kind": "trait"},
                "3": {"crate_id": 0, "path": ["demo", "make"], "kind": "function"}
            },
            "index": {
                "0": {
                    "id": 0,
                    "crate_id": 0,
                    "name": "demo",
                    "span": {"filename": "src/lib.rs", "begin": [1, 1], "end": [3, 1]},
                    "visibility": "public",
                    "docs": "Crate docs.",
                    "links": {},
                    "attrs": [],
                    "deprecation": null,
                    "inner": {"module": {"is_crate": true, "items": [1, 2, 3], "is_stripped": false}}
                },
                "1": {
                    "id": 1,
                    "crate_id": 0,
                    "name": "Widget",
                    "span": {"filename": "src/lib.rs", "begin": [1, 1], "end": [1, 18]},
                    "visibility": "public",
                    "docs": "Widget docs with [Other].",
                    "links": {"Other": 2},
                    "attrs": [],
                    "deprecation": null,
                    "inner": {"struct": {"kind": "unit", "generics": {"params": [], "where_predicates": []}, "impls": []}}
                },
                "2": {
                    "id": 2,
                    "crate_id": 0,
                    "name": "Other",
                    "span": {"filename": "src/lib.rs", "begin": [2, 1], "end": [2, 19]},
                    "visibility": "public",
                    "docs": "Other docs.",
                    "links": {},
                    "attrs": [],
                    "deprecation": null,
                    "inner": {"trait": {
                        "is_auto": false,
                        "is_unsafe": false,
                        "is_dyn_compatible": true,
                        "items": [],
                        "generics": {"params": [], "where_predicates": []},
                        "bounds": [],
                        "implementations": []
                    }}
                },
                "3": {
                    "id": 3,
                    "crate_id": 0,
                    "name": "make",
                    "span": {"filename": "src/lib.rs", "begin": [3, 1], "end": [3, 40]},
                    "visibility": "public",
                    "docs": "Make a widget.",
                    "links": {},
                    "attrs": [],
                    "deprecation": null,
                    "inner": {"function": {
                        "sig": {
                            "inputs": [],
                            "output": {"resolved_path": {"path": "demo::Widget", "id": 1, "args": null}},
                            "is_c_variadic": false
                        },
                        "generics": {"params": [], "where_predicates": []},
                        "header": {
                            "is_const": false,
                            "is_unsafe": false,
                            "is_async": false,
                            "abi": "Rust"
                        },
                        "has_body": true
                    }}
                }
            }
        });
        fs::write(
            root.join("target/doc/demo.json"),
            serde_json::to_string_pretty(&rustdoc).expect("fixture json"),
        )
        .expect("write rustdoc");
    }

    #[test]
    fn rustdoc_source_projects_code_facts_without_raw_ids() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("target/doc")).expect("create target doc");
        write_fixture(&root);
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(config_key::RUSTDOC_JSON, "target/doc/demo.json"),
            ConfigEntry::scalar(config_key::SOURCE_ROOT, "."),
            ConfigEntry::scalar(config_key::PACKAGE, "demo"),
            ConfigEntry::scalar(config_key::ARTIFACT_REVISION, "abc123"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");

        assert!(batch.handles.iter().any(|handle| handle.id == "src/lib.rs"));
        let widget = batch
            .handles
            .iter()
            .find(|handle| {
                handle.kind == "section" && handle.id.starts_with("src/lib.rs#demo::Widget@")
            })
            .expect("widget handle");
        assert!(
            !widget.id.ends_with("#1"),
            "raw rustdoc ids must not become emitted handle ids"
        );
        assert!(batch.meta.iter().any(|meta| {
            meta.handle == widget.id
                && meta.key == meta_key::QUALIFIED_NAME
                && meta.value == "demo::Widget"
        }));
        assert!(batch.edges.iter().any(|edge| {
            edge.from == widget.id
                && edge.kind == edge_kind::CITES
                && edge.to.contains("demo::Other")
        }));
        assert!(batch.edges.iter().any(|edge| {
            edge.kind == edge_kind::USES_TYPE
                && edge.from.contains("demo::make")
                && edge.to == widget.id
        }));
        assert!(batch.content.iter().any(|content| {
            content.handle == widget.id && content.text.contains("Widget docs")
        }));
        assert!(batch.meta.iter().any(|meta| {
            meta.handle == "Cargo.toml"
                && meta.key == meta_key::CONTENT_BUDGET_ROOT_DISPOSITION
                && meta.value == relation_value::BUDGET_COMPLETE
        }));
    }

    #[test]
    fn rustdoc_source_declares_visible_content_budget_truncation() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("target/doc")).expect("create target doc");
        write_fixture(&root);
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(config_key::RUSTDOC_JSON, "target/doc/demo.json"),
            ConfigEntry::scalar(config_key::CONTENT_BUDGET_BYTES, "1"),
            ConfigEntry::scalar(config_key::MEMBER_DOC_BUDGET_BYTES, "8"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");

        assert!(batch.meta.iter().any(|meta| {
            meta.handle == "Cargo.toml"
                && meta.key == meta_key::CONTENT_BUDGET_ROOT_DISPOSITION
                && meta.value == relation_value::BUDGET_TRUNCATED
        }));
        assert!(batch.meta.iter().any(|meta| {
            meta.handle.contains("demo::make")
                && meta.key == meta_key::CONTENT_BUDGET_DISPOSITION
                && meta.value == relation_value::PER_ITEM_CAP
        }));
    }
}
