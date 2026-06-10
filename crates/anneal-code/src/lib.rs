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

const SOURCE_NAME: &str = "code";
const DEFAULT_SOURCE_EXTENSIONS: &[&str] = &["rs", "ex", "exs"];
const DEFAULT_CONTENT_BUDGET_BYTES: usize = 1024 * 1024;
const DEFAULT_MEMBER_DOC_BUDGET_BYTES: usize = 16 * 1024;

mod config_key {
    pub(super) const EEP48_BEAM: &str = "code.eep48_beam";
    pub(super) const EEP48_BEAM_DIR: &str = "code.eep48_beam_dir";
    pub(super) const EEP48_DOC_CHUNK: &str = "code.eep48_doc_chunk";
    pub(super) const RUSTDOC_JSON: &str = "code.rustdoc_json";
    pub(super) const SOURCE_ROOT: &str = "code.source_root";
    pub(super) const SOURCE_EXTENSION: &str = "code.source_extension";
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
    pub(super) const CLASS: &str = "code.class";
    pub(super) const OBLIGATION: &str = "code.obligation";
    pub(super) const OBLIGATION_COUNT: &str = "code.obligation.count";
    pub(super) const VERSION_TAG: &str = "code.version_tag";
    pub(super) const DOCS_SOURCE: &str = "code.docs_source";
    pub(super) const DOC_STATE: &str = "code.doc_state";
    pub(super) const HIDDEN: &str = "code.hidden";
    pub(super) const SINCE: &str = "code.since";
}

mod relation_value {
    pub(super) const ARTIFACT_REVISION_UNKNOWN: &str = "artifact_revision_unknown";
    pub(super) const BUDGET_COMPLETE: &str = "complete";
    pub(super) const BUDGET_TRUNCATED: &str = "truncated";
    pub(super) const FIRST_PARAGRAPH: &str = "first_paragraph";
    pub(super) const PER_ITEM_CAP: &str = "per_item_cap";
    pub(super) const CLASS_GENERATED: &str = "generated";
    pub(super) const CLASS_PRIVATE: &str = "private";
    pub(super) const CLASS_PUBLIC_API: &str = "public-api";
    pub(super) const CLASS_TEST: &str = "test";
    pub(super) const DOC_STATE_DOCUMENTED: &str = "documented";
    pub(super) const DOC_STATE_HIDDEN: &str = "hidden";
    pub(super) const DOC_STATE_MISSING: &str = "missing";
    pub(super) const DOC_STATE_NONE: &str = "none";
    pub(super) const DOC_SOURCE_BEAM: &str = "beam_docs_chunk";
    pub(super) const DOC_SOURCE_EXTERNAL: &str = "external_doc_chunk";
    pub(super) const DOC_SOURCE_MISSING: &str = "missing_or_stripped";
}

mod edge_kind {
    pub(super) const CONTAINS: &str = "Contains";
    pub(super) const CITES: &str = "Cites";
    pub(super) const IMPLEMENTS: &str = "Implements";
    pub(super) const USES_TYPE: &str = "UsesType";
}

mod concern_name {
    pub(super) const CODE_FIXME: &str = "code.fixme";
    pub(super) const CODE_TODO: &str = "code.todo";
}

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

#[derive(Clone, Debug)]
struct CodeDiscoveryConfig {
    artifacts: Vec<Utf8PathBuf>,
    eep48_beams: Vec<Utf8PathBuf>,
    eep48_beam_dirs: Vec<Utf8PathBuf>,
    eep48_doc_chunks: Vec<Utf8PathBuf>,
    source_root: Utf8PathBuf,
    source_extensions: Vec<String>,
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
        let eep48_beams = facts
            .values(config_key::EEP48_BEAM)
            .map(valid_relative_path)
            .collect::<Result<Vec<_>, _>>()?;
        let eep48_beam_dirs = facts
            .values(config_key::EEP48_BEAM_DIR)
            .map(valid_relative_path)
            .collect::<Result<Vec<_>, _>>()?;
        let eep48_doc_chunks = facts
            .values(config_key::EEP48_DOC_CHUNK)
            .map(valid_relative_path)
            .collect::<Result<Vec<_>, _>>()?;
        let source_root = facts
            .first(config_key::SOURCE_ROOT)
            .map(valid_source_root)
            .transpose()?
            .unwrap_or_else(|| Utf8PathBuf::from("."));
        let mut source_extensions = facts
            .values(config_key::SOURCE_EXTENSION)
            .map(|value| value.trim().trim_start_matches('.').to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if source_extensions.is_empty() {
            source_extensions = DEFAULT_SOURCE_EXTENSIONS
                .iter()
                .map(|ext| (*ext).to_string())
                .collect();
        }
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
            eep48_beams,
            eep48_beam_dirs,
            eep48_doc_chunks,
            source_root,
            source_extensions,
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

/// `code.source_root` may climb above the corpus root (a `.design` corpus
/// pointing at the code beside it), but never escapes the enclosing project
/// root — the same boundary the drift probe honors. Containment is enforced
/// against the resolved path at extraction time.
fn valid_source_root(value: &str) -> Result<Utf8PathBuf, SourceError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(Utf8PathBuf::from("."));
    }
    let path = Utf8PathBuf::from(trimmed);
    if path.is_absolute() {
        return Err(SourceError::InvalidConfig(format!(
            "code.source_root must be a relative path; got {value:?}"
        )));
    }
    Ok(path)
}

fn ensure_source_root_within_project(
    root: &Utf8Path,
    source_root: &Utf8Path,
) -> Result<(), SourceError> {
    let resolved = root.join(source_root);
    let canonical = resolved
        .canonicalize_utf8()
        .map_err(|source| SourceError::io(&resolved, source))?;
    let boundary = anneal_core::enclosing_project_root(root).ok_or_else(|| {
        SourceError::InvalidConfig(format!(
            "code.source_root {source_root:?} climbs above the corpus root, but no enclosing project root (git repo or workspace manifest) was found to bound it"
        ))
    })?;
    let boundary = boundary
        .canonicalize_utf8()
        .map_err(|source| SourceError::io(&boundary, source))?;
    if !canonical.starts_with(&boundary) {
        return Err(SourceError::InvalidConfig(format!(
            "code.source_root {source_root:?} resolves outside the enclosing project root {boundary}"
        )));
    }
    Ok(())
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

fn extract_eep48_set(
    root: &Utf8Path,
    cx: &SourceContext<'_>,
    config: &CodeDiscoveryConfig,
    manifest: Option<&ArtifactManifest>,
) -> Result<FactBatch, SourceError> {
    let package = config
        .package
        .clone()
        .or_else(|| manifest.and_then(ArtifactManifest::package_name))
        .unwrap_or_else(|| "elixir".to_string());
    let source_revision = config
        .artifact_revision
        .clone()
        .or_else(|| manifest.and_then(ArtifactManifest::source_revision))
        .unwrap_or_else(|| relation_value::ARTIFACT_REVISION_UNKNOWN.to_string());
    let mut docs = Vec::new();
    for artifact in eep48_artifacts(root, config)? {
        let module_docs = eep48_docs_from_artifact(&root.join(&artifact))?;
        let parsed = parse_eep48_docs(&module_docs)?;
        docs.push(Eep48ArtifactDocs {
            artifact,
            docs: module_docs,
            parsed,
        });
    }
    let budget = eep48_content_budget_report(
        &docs,
        config.content_budget_bytes,
        config.member_doc_budget_bytes,
    );

    let mut batch = FactBatch::new(
        cx.corpus.clone(),
        SourceName::from(SOURCE_NAME),
        FactBatchMode::FullSnapshot,
        cx.next_generation(),
    );
    for Eep48ArtifactDocs {
        artifact,
        docs,
        parsed,
    } in docs
    {
        let mut projector = Eep48Projector::new(Eep48ProjectorInput {
            root,
            source_root: &config.source_root,
            artifact: &artifact,
            revision: source_revision.clone(),
            package: package.clone(),
            content_budget_bytes: config.content_budget_bytes,
            member_doc_budget_bytes: config.member_doc_budget_bytes,
            docs,
            parsed,
            budget_override: Some(budget.clone()),
        });
        projector.project(&mut batch);
    }
    emit_content_budget_meta(
        &mut batch,
        root,
        &Revision::from(source_revision),
        &package_root_file(root, &config.source_root),
        &budget,
    );
    Ok(batch)
}

fn eep48_artifacts(
    root: &Utf8Path,
    config: &CodeDiscoveryConfig,
) -> Result<Vec<Utf8PathBuf>, SourceError> {
    let mut artifacts = config
        .eep48_beams
        .iter()
        .chain(config.eep48_doc_chunks.iter())
        .cloned()
        .collect::<Vec<_>>();
    for dir in &config.eep48_beam_dirs {
        collect_eep48_beams(root, dir, &root.join(dir), &mut artifacts)?;
    }
    artifacts.sort();
    artifacts.dedup();
    Ok(artifacts)
}

fn collect_eep48_beams(
    root: &Utf8Path,
    configured_dir: &Utf8Path,
    dir: &Utf8Path,
    out: &mut Vec<Utf8PathBuf>,
) -> Result<(), SourceError> {
    let entries = fs::read_dir(dir).map_err(|source| SourceError::io(dir, source))?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| SourceError::io(dir, source))
                .and_then(|path| {
                    Utf8PathBuf::from_path_buf(path).map_err(|path| {
                        SourceError::Other(format!(
                            "artifact path is not UTF-8: {}",
                            path.display()
                        ))
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_eep48_beams(root, configured_dir, &path, out)?;
            continue;
        }
        if path.extension() != Some("beam") {
            continue;
        }
        let Some(relative) = normalize_path_inside_root(root, &path) else {
            return Err(SourceError::InvalidConfig(format!(
                "EEP-48 beam directory {configured_dir} produced path outside corpus root: {path}"
            )));
        };
        out.push(relative);
    }
    Ok(())
}

fn eep48_docs_from_artifact(path: &Utf8Path) -> Result<Eep48ModuleDocs, SourceError> {
    if path.extension() == Some("beam") {
        return eep48_docs_from_beam(path);
    }
    let bytes = fs::read(path).map_err(|source| SourceError::io(path, source))?;
    let term = EetfTerm::decode(Cursor::new(bytes))
        .map_err(|source| SourceError::Other(format!("{path}: {source}")))?;
    Ok(Eep48ModuleDocs {
        module: module_name_from_artifact(path),
        docs_source: relation_value::DOC_SOURCE_EXTERNAL.to_string(),
        term: Some(term),
    })
}

fn eep48_docs_from_beam(path: &Utf8Path) -> Result<Eep48ModuleDocs, SourceError> {
    let beam = RawBeamFile::from_file(path)
        .map_err(|source| SourceError::Other(format!("{path}: {source}")))?;
    if let Some(chunk) = beam.chunks.iter().find(|chunk| chunk.id == *b"Docs") {
        let term = EetfTerm::decode(Cursor::new(&chunk.data))
            .map_err(|source| SourceError::Other(format!("{path}: {source}")))?;
        return Ok(Eep48ModuleDocs {
            module: module_name_from_artifact(path),
            docs_source: relation_value::DOC_SOURCE_BEAM.to_string(),
            term: Some(term),
        });
    }

    if let Some(fallback) = external_doc_chunk_for_beam(path)
        && fallback.is_file()
    {
        let bytes = fs::read(&fallback).map_err(|source| SourceError::io(&fallback, source))?;
        let term = EetfTerm::decode(Cursor::new(bytes))
            .map_err(|source| SourceError::Other(format!("{fallback}: {source}")))?;
        return Ok(Eep48ModuleDocs {
            module: module_name_from_artifact(path),
            docs_source: relation_value::DOC_SOURCE_EXTERNAL.to_string(),
            term: Some(term),
        });
    }

    Ok(Eep48ModuleDocs {
        module: module_name_from_artifact(path),
        docs_source: relation_value::DOC_SOURCE_MISSING.to_string(),
        term: None,
    })
}

fn external_doc_chunk_for_beam(path: &Utf8Path) -> Option<Utf8PathBuf> {
    let module = path.file_stem()?;
    let ebin = path.parent()?;
    let app = ebin.parent()?;
    Some(
        app.join("doc")
            .join("chunks")
            .join(format!("{module}.chunk")),
    )
}

fn module_name_from_artifact(path: &Utf8Path) -> String {
    path.file_stem()
        .unwrap_or("unknown")
        .strip_prefix("Elixir.")
        .unwrap_or_else(|| path.file_stem().unwrap_or("unknown"))
        .to_string()
}

#[derive(Clone, Debug)]
struct Eep48ModuleDocs {
    module: String,
    docs_source: String,
    term: Option<EetfTerm>,
}

struct Eep48ArtifactDocs {
    artifact: Utf8PathBuf,
    docs: Eep48ModuleDocs,
    parsed: Eep48ParsedDocs,
}

#[derive(Clone, Debug)]
struct Eep48Entry {
    kind: String,
    name: String,
    arity: i32,
    line: u32,
    signatures: Vec<String>,
    doc: Eep48Doc,
    metadata: Eep48Metadata,
}

#[derive(Clone, Debug, Default)]
struct Eep48Metadata {
    behaviours: Vec<String>,
    deprecated: Option<String>,
    hidden: bool,
    since: Option<String>,
    source_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Eep48ParsedDocs {
    doc_format: Option<String>,
    module_doc: Eep48Doc,
    metadata: Eep48Metadata,
    entries: Vec<Eep48Entry>,
}

#[derive(Clone, Debug, Default)]
struct Eep48Doc {
    state: String,
    text: String,
}

impl Eep48Doc {
    fn documented(text: String) -> Self {
        Self {
            state: relation_value::DOC_STATE_DOCUMENTED.to_string(),
            text,
        }
    }

    fn hidden() -> Self {
        Self {
            state: relation_value::DOC_STATE_HIDDEN.to_string(),
            text: String::new(),
        }
    }

    fn missing() -> Self {
        Self {
            state: relation_value::DOC_STATE_MISSING.to_string(),
            text: String::new(),
        }
    }

    fn none() -> Self {
        Self {
            state: relation_value::DOC_STATE_NONE.to_string(),
            text: String::new(),
        }
    }
}

fn parse_eep48_docs(docs: &Eep48ModuleDocs) -> Result<Eep48ParsedDocs, SourceError> {
    let Some(term) = docs.term.as_ref() else {
        return Ok(Eep48ParsedDocs {
            module_doc: Eep48Doc::missing(),
            ..Eep48ParsedDocs::default()
        });
    };
    let elements = tuple_elements(term).ok_or_else(|| {
        SourceError::Other(format!("{} EEP-48 Docs term is not a tuple", docs.module))
    })?;
    if elements.len() != 7 || atom_name(&elements[0]) != Some("docs_v1") {
        return Err(SourceError::Other(format!(
            "{} EEP-48 Docs term is not docs_v1",
            docs.module
        )));
    }
    let doc_format = term_string(&elements[3]);
    let module_doc = eep48_doc(&elements[4]);
    let metadata = eep48_metadata(&elements[5]);
    let entries = list_elements(&elements[6])
        .map(|entries| {
            entries
                .iter()
                .filter_map(eep48_entry)
                .collect::<Vec<Eep48Entry>>()
        })
        .unwrap_or_default();
    Ok(Eep48ParsedDocs {
        doc_format,
        module_doc,
        metadata,
        entries,
    })
}

fn eep48_content_budget_report(
    docs: &[Eep48ArtifactDocs],
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
) -> ContentBudgetReport {
    let mut report = ContentBudgetReport {
        content_budget_bytes,
        member_doc_budget_bytes,
        ..ContentBudgetReport::default()
    };
    for docs in docs {
        let parsed = &docs.parsed;
        report.signature_bytes += format!("defmodule {}", docs.docs.module).len();
        report.doc_bytes += parsed.module_doc.text.len();
        report.structural_doc_bytes += parsed.module_doc.text.len();
        for entry in &parsed.entries {
            let signature = if entry.signatures.is_empty() {
                format!("{}({})", entry.name, entry.arity)
            } else {
                entry.signatures.join("\n")
            };
            report.signature_bytes += signature.len();
            report.doc_bytes += entry.doc.text.len();
            report.member_doc_bytes += entry.doc.text.len();
        }
    }
    report.disposition = if report.doc_bytes + report.signature_bytes > content_budget_bytes {
        relation_value::BUDGET_TRUNCATED.to_string()
    } else {
        relation_value::BUDGET_COMPLETE.to_string()
    };
    report
}

fn eep48_entry(term: &EetfTerm) -> Option<Eep48Entry> {
    let elements = tuple_elements(term)?;
    if elements.len() != 5 {
        return None;
    }
    let identity = tuple_elements(&elements[0])?;
    if identity.len() != 3 {
        return None;
    }
    Some(Eep48Entry {
        kind: term_string(&identity[0])?,
        name: term_string(&identity[1])?,
        arity: term_i32(&identity[2])?,
        line: annotation_line(&elements[1]),
        signatures: list_elements(&elements[2])
            .map(|items| items.iter().filter_map(term_string).collect())
            .unwrap_or_default(),
        doc: eep48_doc(&elements[3]),
        metadata: eep48_metadata(&elements[4]),
    })
}

fn eep48_doc(term: &EetfTerm) -> Eep48Doc {
    match term {
        EetfTerm::Atom(atom) if atom.name == "hidden" => Eep48Doc::hidden(),
        EetfTerm::Atom(atom) if atom.name == "none" => Eep48Doc::none(),
        EetfTerm::Atom(atom) if atom.name == "nil" => Eep48Doc::missing(),
        EetfTerm::List(list) if list.elements.is_empty() => Eep48Doc::missing(),
        EetfTerm::Map(map) => {
            let text = map
                .map
                .iter()
                .filter_map(|(_, value)| term_string(value))
                .collect::<Vec<_>>()
                .join("\n\n");
            if text.is_empty() {
                Eep48Doc::none()
            } else {
                Eep48Doc::documented(text)
            }
        }
        _ => Eep48Doc::missing(),
    }
}

fn eep48_metadata(term: &EetfTerm) -> Eep48Metadata {
    let mut out = Eep48Metadata::default();
    let Some(map) = term_map(term) else {
        return out;
    };
    for (key, value) in map {
        let Some(key) = term_string(key) else {
            continue;
        };
        match key.as_str() {
            "behaviours" => out.behaviours = metadata_strings(value),
            "deprecated" => {
                out.deprecated = term_string(value)
                    .or_else(|| bool_metadata(value).map(|value| value.to_string()));
            }
            "hidden" => out.hidden = bool_metadata(value).unwrap_or(false),
            "since" => out.since = term_string(value),
            "source_path" => out.source_path = term_string(value),
            _ => {}
        }
    }
    out
}

fn metadata_strings(term: &EetfTerm) -> Vec<String> {
    list_elements(term).map_or_else(
        || term_string(term).into_iter().collect(),
        |items| items.iter().filter_map(term_string).collect(),
    )
}

fn bool_metadata(term: &EetfTerm) -> Option<bool> {
    match atom_name(term) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

fn annotation_line(term: &EetfTerm) -> u32 {
    if let Some(line) = term_i32(term).and_then(|line| u32::try_from(line).ok()) {
        return line;
    }
    tuple_elements(term)
        .and_then(|items| items.first())
        .and_then(term_i32)
        .and_then(|line| u32::try_from(line).ok())
        .unwrap_or(1)
}

fn tuple_elements(term: &EetfTerm) -> Option<&[EetfTerm]> {
    match term {
        EetfTerm::Tuple(tuple) => Some(&tuple.elements),
        _ => None,
    }
}

fn list_elements(term: &EetfTerm) -> Option<&[EetfTerm]> {
    match term {
        EetfTerm::List(list) => Some(&list.elements),
        EetfTerm::ByteList(byte_list) if byte_list.bytes.is_empty() => Some(&[]),
        _ => None,
    }
}

fn term_map(term: &EetfTerm) -> Option<&std::collections::HashMap<EetfTerm, EetfTerm>> {
    match term {
        EetfTerm::Map(map) => Some(&map.map),
        _ => None,
    }
}

fn atom_name(term: &EetfTerm) -> Option<&str> {
    match term {
        EetfTerm::Atom(atom) => Some(atom.name.as_str()),
        _ => None,
    }
}

fn term_string(term: &EetfTerm) -> Option<String> {
    match term {
        EetfTerm::Atom(atom) => Some(atom.name.clone()),
        EetfTerm::Binary(binary) => String::from_utf8(binary.bytes.clone()).ok(),
        EetfTerm::ByteList(byte_list) => String::from_utf8(byte_list.bytes.clone()).ok(),
        _ => None,
    }
}

fn term_i32(term: &EetfTerm) -> Option<i32> {
    match term {
        EetfTerm::FixInteger(value) => Some(value.value),
        EetfTerm::BigInteger(value) => value.value.to_string().parse().ok(),
        _ => None,
    }
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

#[derive(Clone, Debug, Default)]
struct SourceTreeClassification {
    files: BTreeMap<String, SourceFileClass>,
    protocol_impls: Vec<ProtocolImpl>,
    tags: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct SourceFileClass {
    generated: bool,
    obligations: Vec<CodeObligation>,
    test: bool,
}

#[derive(Clone, Debug)]
struct CodeObligation {
    kind: &'static str,
    line: u32,
    text: String,
}

#[derive(Clone, Debug)]
struct ProtocolImpl {
    file: String,
    line: u32,
    protocol: String,
    target: Option<String>,
}

impl SourceTreeClassification {
    fn scan(
        root: &Utf8Path,
        source_root: &Utf8Path,
        extensions: &[String],
    ) -> Result<Self, SourceError> {
        let joined = root.join(source_root);
        // Canonicalize so a climbing source root ("..") yields a stable base;
        // handle ids are relative to this base — the path space citations use.
        let source_abs = joined.canonicalize_utf8().unwrap_or(joined);
        let mut out = Self {
            files: BTreeMap::new(),
            protocol_impls: Vec::new(),
            tags: git_version_tags(&source_abs),
        };
        scan_source_dir(&source_abs, &source_abs, extensions, &mut out)?;
        Ok(out)
    }

    fn project(
        &self,
        batch: &mut FactBatch,
        root: &Utf8Path,
        source_root: &Utf8Path,
        revision: Option<&str>,
    ) {
        let revision = Revision::from(
            revision
                .filter(|value| !value.is_empty())
                .unwrap_or(relation_value::ARTIFACT_REVISION_UNKNOWN)
                .to_string(),
        );
        let package_handle = package_root_file(root, source_root);
        self.ensure_source_file_handles(batch, root, &revision);

        let visibility_by_handle = meta_values(batch, meta_key::VISIBILITY);
        let mut public_files = BTreeSet::new();
        for handle in &batch.handles {
            if handle.kind == "section"
                && visibility_by_handle
                    .get(&handle.id)
                    .is_some_and(|visibility| visibility == "public")
            {
                public_files.insert(handle.file.clone());
            }
        }

        let handle_shapes = batch
            .handles
            .iter()
            .map(|handle| (handle.id.clone(), handle.file.clone(), handle.kind.clone()))
            .collect::<Vec<_>>();
        for (id, file, kind) in handle_shapes {
            if kind != "file" && kind != "section" {
                continue;
            }
            let class = self.class_for_handle(
                &id,
                &file,
                &kind,
                &package_handle,
                &public_files,
                &visibility_by_handle,
            );
            push_code_meta(batch, root, &revision, &id, &file, meta_key::CLASS, class);
        }

        self.emit_obligations(batch, root, &revision);
        self.emit_protocol_impls(batch, root, &revision);
        self.emit_version_tags(batch, root, &revision, &package_handle);
    }

    fn ensure_source_file_handles(
        &self,
        batch: &mut FactBatch,
        root: &Utf8Path,
        revision: &Revision,
    ) {
        let existing = batch
            .handles
            .iter()
            .map(|handle| handle.id.clone())
            .collect::<BTreeSet<_>>();
        for file in self.files.keys() {
            if existing.contains(file) {
                continue;
            }
            batch.handles.push(HandleFact {
                identity: code_identity(batch, root, revision, file, file),
                id: file.clone(),
                kind: "file".to_string(),
                status: None,
                namespace: String::new(),
                file: file.clone(),
                line: 1,
                date: None,
                area: area_for(file),
                summary: file.clone(),
            });
        }
    }

    fn class_for_handle(
        &self,
        id: &str,
        file: &str,
        kind: &str,
        package_handle: &str,
        public_files: &BTreeSet<String>,
        visibility_by_handle: &BTreeMap<String, String>,
    ) -> &'static str {
        if let Some(scan) = self.files.get(file) {
            if scan.generated {
                return relation_value::CLASS_GENERATED;
            }
            if scan.test {
                return relation_value::CLASS_TEST;
            }
        }
        if kind == "file" {
            if id == package_handle || public_files.contains(id) {
                relation_value::CLASS_PUBLIC_API
            } else {
                relation_value::CLASS_PRIVATE
            }
        } else if visibility_by_handle
            .get(id)
            .is_some_and(|visibility| visibility == "public")
        {
            relation_value::CLASS_PUBLIC_API
        } else {
            relation_value::CLASS_PRIVATE
        }
    }

    fn emit_obligations(&self, batch: &mut FactBatch, root: &Utf8Path, revision: &Revision) {
        for (file, scan) in &self.files {
            if scan.obligations.is_empty() {
                continue;
            }
            push_code_meta(
                batch,
                root,
                revision,
                file,
                file,
                meta_key::OBLIGATION_COUNT,
                &scan.obligations.len().to_string(),
            );
            for (idx, obligation) in scan.obligations.iter().enumerate() {
                let native_id = format!("{file}::code-obligation::{idx}");
                let identity = code_identity(batch, root, revision, &native_id, file);
                let concern = if obligation.kind == "TODO" {
                    concern_name::CODE_TODO
                } else {
                    concern_name::CODE_FIXME
                };
                batch.concerns.push(ConcernFact {
                    identity: identity.clone(),
                    name: concern.to_string(),
                    member: file.clone(),
                });
                push_meta_fact(
                    batch,
                    &identity,
                    file,
                    meta_key::OBLIGATION,
                    &format!(
                        "{}:{}:{}",
                        obligation.kind, obligation.line, obligation.text
                    ),
                );
            }
        }
    }

    fn emit_protocol_impls(&self, batch: &mut FactBatch, root: &Utf8Path, revision: &Revision) {
        for (idx, impl_) in self.protocol_impls.iter().enumerate() {
            let from = impl_.target.as_ref().map_or_else(
                || impl_.file.clone(),
                |target| format!("elixir://{}", stable_fragment(target)),
            );
            ensure_external_code_handle(
                batch,
                root,
                revision,
                &impl_.file,
                &from,
                impl_.target.as_deref().unwrap_or(&impl_.file),
            );
            let to = format!("elixir://{}", stable_fragment(&impl_.protocol));
            ensure_external_code_handle(batch, root, revision, &impl_.file, &to, &impl_.protocol);
            let native_id = format!(
                "{}::edge::protocol_impl::{idx}::{}::{}",
                impl_.file, from, to
            );
            let identity = code_identity(batch, root, revision, &native_id, &impl_.file);
            batch.edges.push(EdgeFact {
                identity: identity.clone(),
                from: from.clone(),
                to: to.clone(),
                kind: edge_kind::IMPLEMENTS.to_string(),
                file: impl_.file.clone(),
                line: impl_.line,
                assertion_date: None,
                assertion_revision: None,
            });
            push_meta_fact(
                batch,
                &identity,
                &native_id,
                meta_key::IMPLEMENTS_KIND,
                "protocol_impl",
            );
            let signature = impl_.target.as_ref().map_or_else(
                || format!("defimpl {}", impl_.protocol),
                |target| format!("defimpl {}, for: {target}", impl_.protocol),
            );
            push_meta_fact(
                batch,
                &identity,
                &native_id,
                meta_key::IMPLEMENTS_SIGNATURE,
                &signature,
            );
        }
    }

    fn emit_version_tags(
        &self,
        batch: &mut FactBatch,
        root: &Utf8Path,
        revision: &Revision,
        package_handle: &str,
    ) {
        for (idx, tag) in self.tags.iter().enumerate() {
            let tag_handle = version_handle_id(tag);
            if !batch.handles.iter().any(|handle| handle.id == tag_handle) {
                batch.handles.push(HandleFact {
                    identity: code_identity(batch, root, revision, &tag_handle, package_handle),
                    id: tag_handle.clone(),
                    kind: "version".to_string(),
                    status: None,
                    namespace: SOURCE_NAME.to_string(),
                    file: package_handle.to_string(),
                    line: 1,
                    date: None,
                    area: area_for(package_handle),
                    summary: format!("code version tag {tag}"),
                });
            }
            push_code_meta(
                batch,
                root,
                revision,
                package_handle,
                package_handle,
                meta_key::VERSION_TAG,
                tag,
            );
            let native_id = format!("{package_handle}::edge::version::{idx}::{tag}");
            batch.edges.push(EdgeFact {
                identity: code_identity(batch, root, revision, &native_id, package_handle),
                from: package_handle.to_string(),
                to: tag_handle,
                kind: edge_kind::CONTAINS.to_string(),
                file: package_handle.to_string(),
                line: 1,
                assertion_date: None,
                assertion_revision: None,
            });
        }
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
        push_meta_fact(batch, identity, handle, key, value);
    }

    fn identity_for(&self, batch: &FactBatch, native_id: &str, file: &str) -> FactIdentity {
        code_identity(batch, self.root, &self.revision, native_id, file)
    }
}

struct Eep48Projector {
    root: Utf8PathBuf,
    artifact: Utf8PathBuf,
    revision: Revision,
    revision_text: String,
    package: String,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
    docs: Eep48ModuleDocs,
    parsed: Eep48ParsedDocs,
    budget_override: Option<ContentBudgetReport>,
    module_handle: String,
    module_file: String,
    package_handle: String,
    budget: ContentBudgetReport,
}

struct Eep48ProjectorInput<'a> {
    root: &'a Utf8Path,
    source_root: &'a Utf8Path,
    artifact: &'a Utf8Path,
    revision: String,
    package: String,
    content_budget_bytes: usize,
    member_doc_budget_bytes: usize,
    docs: Eep48ModuleDocs,
    parsed: Eep48ParsedDocs,
    budget_override: Option<ContentBudgetReport>,
}

impl Eep48Projector {
    fn new(input: Eep48ProjectorInput<'_>) -> Self {
        let parsed = input.parsed;
        let package_handle = package_root_file(input.root, input.source_root);
        let module_file = parsed
            .metadata
            .source_path
            .as_deref()
            .and_then(|path| normalize_code_source_path(input.root, input.source_root, path))
            .unwrap_or_else(|| package_handle.clone());
        let module_handle = format!("{}#{}", module_file, input.docs.module);
        Self {
            root: input.root.to_path_buf(),
            artifact: input.artifact.to_path_buf(),
            revision: Revision::from(input.revision.clone()),
            revision_text: input.revision,
            package: input.package,
            content_budget_bytes: input.content_budget_bytes,
            member_doc_budget_bytes: input.member_doc_budget_bytes,
            docs: input.docs,
            parsed,
            budget_override: input.budget_override,
            module_handle,
            module_file,
            package_handle,
            budget: ContentBudgetReport::default(),
        }
    }

    fn project(&mut self, batch: &mut FactBatch) {
        self.emit_file_handle(batch);
        self.emit_module_handle(batch);
        self.emit_member_handles(batch);
        self.emit_package_meta(batch);
        self.emit_structure_edges(batch);
        self.emit_behaviour_edges(batch);
        self.emit_doc_link_edges(batch);
        self.emit_content(batch);
        if self.budget_override.is_none() {
            self.emit_budget_meta(batch);
        }
    }

    fn emit_file_handle(&self, batch: &mut FactBatch) {
        if batch
            .handles
            .iter()
            .any(|handle| handle.id == self.module_file)
        {
            return;
        }
        batch.handles.push(HandleFact {
            identity: self.identity_for(batch, &self.module_file, &self.module_file),
            id: self.module_file.clone(),
            kind: "file".to_string(),
            status: None,
            namespace: String::new(),
            file: self.module_file.clone(),
            line: 1,
            date: None,
            area: area_for(&self.module_file),
            summary: self.module_file.clone(),
        });
    }

    fn emit_module_handle(&self, batch: &mut FactBatch) {
        let line = 1;
        let doc_state = if self.parsed.metadata.hidden {
            relation_value::DOC_STATE_HIDDEN
        } else {
            self.parsed.module_doc.state.as_str()
        };
        batch.handles.push(HandleFact {
            identity: self.identity_for(batch, &self.module_handle, &self.module_file),
            id: self.module_handle.clone(),
            kind: "section".to_string(),
            status: self
                .parsed
                .metadata
                .deprecated
                .as_ref()
                .map(|_| "deprecated".to_string()),
            namespace: "module".to_string(),
            file: self.module_file.clone(),
            line,
            date: None,
            area: area_for(&self.module_file),
            summary: format!("module {}", self.docs.module),
        });
        let identity = self.identity_for(batch, &self.module_handle, &self.module_file);
        Self::push_meta(
            batch,
            &identity,
            &self.module_handle,
            meta_key::QUALIFIED_NAME,
            &self.docs.module,
        );
        Self::push_meta(
            batch,
            &identity,
            &self.module_handle,
            meta_key::KIND,
            "module",
        );
        Self::push_meta(
            batch,
            &identity,
            &self.module_handle,
            meta_key::VISIBILITY,
            if doc_state == relation_value::DOC_STATE_HIDDEN {
                "hidden"
            } else {
                "public"
            },
        );
        Self::push_meta(
            batch,
            &identity,
            &self.module_handle,
            meta_key::DOC_STATE,
            doc_state,
        );
        if self.parsed.metadata.hidden || doc_state == relation_value::DOC_STATE_HIDDEN {
            Self::push_meta(
                batch,
                &identity,
                &self.module_handle,
                meta_key::HIDDEN,
                "true",
            );
        }
        Self::emit_deprecation_meta(batch, &identity, &self.module_handle, &self.parsed.metadata);
    }

    fn emit_member_handles(&self, batch: &mut FactBatch) {
        for entry in &self.parsed.entries {
            let handle = self.member_handle(entry);
            let qualified = self.member_qualified_name(entry);
            let doc_state = if entry.metadata.hidden {
                relation_value::DOC_STATE_HIDDEN
            } else {
                entry.doc.state.as_str()
            };
            batch.handles.push(HandleFact {
                identity: self.identity_for(batch, &handle, &self.module_file),
                id: handle.clone(),
                kind: "section".to_string(),
                status: entry
                    .metadata
                    .deprecated
                    .as_ref()
                    .map(|_| "deprecated".to_string()),
                namespace: entry.kind.clone(),
                file: self.module_file.clone(),
                line: entry.line,
                date: None,
                area: area_for(&self.module_file),
                summary: format!("{} {qualified}", entry.kind),
            });
            let identity = self.identity_for(batch, &handle, &self.module_file);
            Self::push_meta(
                batch,
                &identity,
                &handle,
                meta_key::QUALIFIED_NAME,
                &qualified,
            );
            Self::push_meta(batch, &identity, &handle, meta_key::KIND, &entry.kind);
            Self::push_meta(
                batch,
                &identity,
                &handle,
                meta_key::VISIBILITY,
                if doc_state == relation_value::DOC_STATE_HIDDEN {
                    "hidden"
                } else {
                    "public"
                },
            );
            Self::push_meta(batch, &identity, &handle, meta_key::DOC_STATE, doc_state);
            if entry.metadata.hidden || doc_state == relation_value::DOC_STATE_HIDDEN {
                Self::push_meta(batch, &identity, &handle, meta_key::HIDDEN, "true");
            }
            Self::emit_deprecation_meta(batch, &identity, &handle, &entry.metadata);
        }
    }

    fn emit_package_meta(&self, batch: &mut FactBatch) {
        let identity = self.identity_for(batch, &self.package_handle, &self.package_handle);
        if !batch
            .handles
            .iter()
            .any(|handle| handle.id == self.package_handle)
        {
            batch.handles.push(HandleFact {
                identity: identity.clone(),
                id: self.package_handle.clone(),
                kind: "file".to_string(),
                status: None,
                namespace: SOURCE_NAME.to_string(),
                file: self.package_handle.clone(),
                line: 1,
                date: None,
                area: area_for(&self.package_handle),
                summary: format!("{} package root", self.package),
            });
        }
        for (key, value) in [
            (meta_key::PACKAGE, self.package.as_str()),
            (meta_key::ARTIFACT_PATH, self.artifact.as_str()),
            (meta_key::ARTIFACT_FORMAT, "eep48"),
            (
                meta_key::ARTIFACT_FORMAT_VERSION,
                self.parsed.doc_format.as_deref().unwrap_or("unknown"),
            ),
            (meta_key::ARTIFACT_REVISION, self.revision_text.as_str()),
            (meta_key::DOCS_SOURCE, self.docs.docs_source.as_str()),
        ] {
            Self::push_meta(batch, &identity, &self.package_handle, key, value);
        }
        if self.revision_text == relation_value::ARTIFACT_REVISION_UNKNOWN {
            Self::push_meta(
                batch,
                &identity,
                &self.package_handle,
                meta_key::ARTIFACT_REVISION_STATE,
                relation_value::ARTIFACT_REVISION_UNKNOWN,
            );
        }
    }

    fn emit_structure_edges(&self, batch: &mut FactBatch) {
        let mut ordinal = 0usize;
        self.push_edge(
            batch,
            &self.module_file,
            &self.module_handle,
            edge_kind::CONTAINS,
            1,
            ordinal,
        );
        ordinal += 1;
        for entry in &self.parsed.entries {
            self.push_edge(
                batch,
                &self.module_handle,
                &self.member_handle(entry),
                edge_kind::CONTAINS,
                entry.line,
                ordinal,
            );
            ordinal += 1;
        }
    }

    fn emit_behaviour_edges(&self, batch: &mut FactBatch) {
        for (ordinal, behaviour) in self.parsed.metadata.behaviours.iter().enumerate() {
            let target = self.external_handle(batch, behaviour);
            let edge = self.push_edge(
                batch,
                &self.module_handle,
                &target,
                edge_kind::IMPLEMENTS,
                1,
                ordinal,
            );
            let identity = self.identity_for(batch, &edge, &self.module_file);
            Self::push_meta(
                batch,
                &identity,
                &edge,
                meta_key::IMPLEMENTS_KIND,
                "behaviour",
            );
            Self::push_meta(
                batch,
                &identity,
                &edge,
                meta_key::IMPLEMENTS_SIGNATURE,
                &format!("@behaviour {behaviour}"),
            );
        }
    }

    fn emit_doc_link_edges(&self, batch: &mut FactBatch) {
        let mut ordinal = 0usize;
        for target in markdown_links(&self.parsed.module_doc.text) {
            let external = self.external_handle(batch, &target);
            self.push_edge(
                batch,
                &self.module_handle,
                &external,
                edge_kind::CITES,
                1,
                ordinal,
            );
            ordinal += 1;
        }
        for entry in &self.parsed.entries {
            let from = self.member_handle(entry);
            for target in markdown_links(&entry.doc.text) {
                let external = self.external_handle(batch, &target);
                self.push_edge(
                    batch,
                    &from,
                    &external,
                    edge_kind::CITES,
                    entry.line,
                    ordinal,
                );
                ordinal += 1;
            }
            for target in signature_type_refs(&entry.signatures) {
                let external = self.external_handle(batch, &target);
                self.push_edge(
                    batch,
                    &from,
                    &external,
                    edge_kind::USES_TYPE,
                    entry.line,
                    ordinal,
                );
                ordinal += 1;
            }
        }
    }

    fn emit_content(&mut self, batch: &mut FactBatch) {
        self.budget = self
            .budget_override
            .clone()
            .unwrap_or_else(|| self.content_budget_report());
        self.emit_one_content(
            batch,
            &self.module_handle,
            &self.module_file,
            1,
            &format!("defmodule {}", self.docs.module),
            &self.parsed.module_doc.text,
        );
        for entry in &self.parsed.entries {
            let handle = self.member_handle(entry);
            let signature = Self::member_signature(entry);
            let (docs, disposition) = self.budgeted_docs(&entry.doc.text, true);
            self.emit_one_content(
                batch,
                &handle,
                &self.module_file,
                entry.line,
                &signature,
                docs,
            );
            if let Some(disposition) = disposition {
                let identity = self.identity_for(batch, &handle, &self.module_file);
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

    fn emit_one_content(
        &self,
        batch: &mut FactBatch,
        handle: &str,
        file: &str,
        line: u32,
        signature: &str,
        docs: &str,
    ) {
        let text = content_text(signature, docs);
        if text.trim().is_empty() {
            return;
        }
        let span_id = format!("{handle}#docs");
        let lines = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
        let identity = self.identity_for(batch, &span_id, file);
        batch.spans.push(SpanFact {
            identity: identity.clone(),
            id: span_id.clone(),
            handle: handle.to_string(),
            start_line: line,
            end_line: line.saturating_add(lines.saturating_sub(1)),
            summary: signature
                .lines()
                .next()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(handle)
                .to_string(),
        });
        batch.content.push(ContentFact {
            identity,
            handle: handle.to_string(),
            span_id,
            lines,
            tokens: token_count(&text),
            text,
        });
    }

    fn content_budget_report(&self) -> ContentBudgetReport {
        let mut report = ContentBudgetReport {
            content_budget_bytes: self.content_budget_bytes,
            member_doc_budget_bytes: self.member_doc_budget_bytes,
            ..ContentBudgetReport::default()
        };
        report.signature_bytes += format!("defmodule {}", self.docs.module).len();
        report.doc_bytes += self.parsed.module_doc.text.len();
        report.structural_doc_bytes += self.parsed.module_doc.text.len();
        for entry in &self.parsed.entries {
            report.signature_bytes += Self::member_signature(entry).len();
            report.doc_bytes += entry.doc.text.len();
            report.member_doc_bytes += entry.doc.text.len();
        }
        report.disposition =
            if report.doc_bytes + report.signature_bytes > self.content_budget_bytes {
                relation_value::BUDGET_TRUNCATED.to_string()
            } else {
                relation_value::BUDGET_COMPLETE.to_string()
            };
        report
    }

    fn budgeted_docs<'b>(&self, docs: &'b str, member: bool) -> (&'b str, Option<&'static str>) {
        if docs.is_empty() || self.budget.disposition == relation_value::BUDGET_COMPLETE || !member
        {
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

    fn emit_deprecation_meta(
        batch: &mut FactBatch,
        identity: &FactIdentity,
        handle: &str,
        metadata: &Eep48Metadata,
    ) {
        if let Some(deprecated) = &metadata.deprecated {
            Self::push_meta(
                batch,
                identity,
                handle,
                meta_key::DEPRECATED_NOTE,
                deprecated,
            );
        }
        if let Some(since) = &metadata.since {
            Self::push_meta(batch, identity, handle, meta_key::SINCE, since);
        }
    }

    fn member_handle(&self, entry: &Eep48Entry) -> String {
        format!(
            "{}#{}.{}/{}",
            self.module_file, self.docs.module, entry.name, entry.arity
        )
    }

    fn member_qualified_name(&self, entry: &Eep48Entry) -> String {
        format!("{}.{}/{}", self.docs.module, entry.name, entry.arity)
    }

    fn member_signature(entry: &Eep48Entry) -> String {
        if entry.signatures.is_empty() {
            return format!("{}({})", entry.name, entry.arity);
        }
        entry.signatures.join("\n")
    }

    fn external_handle(&self, batch: &mut FactBatch, qualified: &str) -> String {
        let handle = format!("elixir://{}", stable_fragment(qualified));
        ensure_external_code_handle(
            batch,
            &self.root,
            &self.revision,
            &self.module_file,
            &handle,
            qualified,
        );
        handle
    }

    fn push_edge(
        &self,
        batch: &mut FactBatch,
        from: &str,
        to: &str,
        kind: &str,
        line: u32,
        ordinal: usize,
    ) -> String {
        let native_id = format!("{from}::edge::{ordinal}::{kind}::{to}::{line}");
        batch.edges.push(EdgeFact {
            identity: self.identity_for(batch, &native_id, &self.module_file),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file: self.module_file.clone(),
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
        push_meta_fact(batch, identity, handle, key, value);
    }

    fn identity_for(&self, batch: &FactBatch, native_id: &str, file: &str) -> FactIdentity {
        code_identity(batch, &self.root, &self.revision, native_id, file)
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
    if root.join("mix.exs").is_file() {
        return "mix.exs".to_string();
    }
    if root.join(source_root).join("mix.exs").is_file() {
        let path = source_root.join("mix.exs");
        return normalize_relative_path(
            path.as_str(),
            anneal_core::RelativePathPolicy::ALLOW_EMPTY,
        )
        .map_or_else(|| path.to_string(), |path| path.to_string());
    }
    if root.join("Cargo.toml").is_file() {
        return "Cargo.toml".to_string();
    }
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

fn scan_source_dir(
    base: &Utf8Path,
    dir: &Utf8Path,
    extensions: &[String],
    out: &mut SourceTreeClassification,
) -> Result<(), SourceError> {
    let entries = fs::read_dir(dir).map_err(|source| SourceError::io(dir, source))?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| SourceError::io(dir, source))
                .and_then(|path| {
                    Utf8PathBuf::from_path_buf(path).map_err(|path| {
                        SourceError::Other(format!("source path is not UTF-8: {}", path.display()))
                    })
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();

    for path in paths {
        let Some(name) = path.file_name() else {
            continue;
        };
        if should_skip_source_dir(name) {
            continue;
        }
        if path.is_dir() {
            scan_source_dir(base, &path, extensions, out)?;
            continue;
        }
        if !path
            .extension()
            .is_some_and(|ext| extensions.iter().any(|allowed| allowed == ext))
        {
            continue;
        }
        let Some(relative) = normalize_path_inside_root(base, &path) else {
            continue;
        };
        let relative = normalize_relative_path(
            relative.as_str(),
            anneal_core::RelativePathPolicy::STRICT_NON_EMPTY,
        )
        .map_or_else(|| relative.to_string(), |path| path.to_string());
        let text = fs::read_to_string(&path).map_err(|source| SourceError::io(&path, source))?;
        let test_path = is_test_path(Utf8Path::new("."), &relative);
        out.protocol_impls.extend(protocol_impls(&relative, &text));
        out.files
            .insert(relative, classify_source_file(&text, test_path));
    }
    Ok(())
}

fn should_skip_source_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".jj" | ".svn" | "target" | "_build" | "deps" | "node_modules" | ".direnv"
    )
}

fn is_test_path(source_root: &Utf8Path, relative: &str) -> bool {
    let path = if source_root == Utf8Path::new(".") {
        Utf8PathBuf::from(relative)
    } else {
        Utf8Path::new(relative)
            .strip_prefix(source_root)
            .map_or_else(|_| Utf8PathBuf::from(relative), Utf8Path::to_path_buf)
    };
    path.components()
        .any(|component| matches!(component.as_str(), "test" | "tests"))
        || path
            .file_stem()
            .is_some_and(|stem| stem.ends_with("_test") || stem.ends_with("_tests"))
}

fn classify_source_file(text: &str, test_path: bool) -> SourceFileClass {
    SourceFileClass {
        generated: has_generated_marker(text),
        obligations: code_obligations(text),
        test: test_path || has_test_marker(text),
    }
}

fn has_generated_marker(text: &str) -> bool {
    text.lines().take(80).any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.contains("@generated")
            || line.contains("automatically generated")
            || line.contains("auto-generated")
            || line.contains("do not edit")
            || line.contains("generated by")
    })
}

fn has_test_marker(text: &str) -> bool {
    text.contains("#[cfg(test)]")
        || text.contains("#[test]")
        || text.contains("mod tests")
        || text.contains("ExUnit.Case")
}

fn code_obligations(text: &str) -> Vec<CodeObligation> {
    let mut obligations = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let Some(kind) = obligation_kind(line) else {
            continue;
        };
        obligations.push(CodeObligation {
            kind,
            line: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            text: truncate_at_char_boundary(line.trim(), 180).to_string(),
        });
    }
    obligations
}

fn obligation_kind(line: &str) -> Option<&'static str> {
    if line.contains("FIXME") {
        Some("FIXME")
    } else if line.contains("TODO") {
        Some("TODO")
    } else {
        None
    }
}

fn protocol_impls(file: &str, text: &str) -> Vec<ProtocolImpl> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let Some(after) = trimmed.strip_prefix("defimpl ") else {
            continue;
        };
        let mut parts = after.splitn(2, ',');
        let protocol = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string();
        if protocol.is_empty() {
            continue;
        }
        let target = parts
            .next()
            .and_then(|rest| rest.split("for:").nth(1))
            .map(|value| {
                value
                    .trim()
                    .trim_matches(|ch: char| matches!(ch, '[' | ']' | '{' | '}'))
                    .split(|ch: char| ch == ',' || ch.is_whitespace())
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .filter(|value| !value.is_empty());
        out.push(ProtocolImpl {
            file: file.to_string(),
            line: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            protocol,
            target,
        });
    }
    out
}

fn git_version_tags(source_abs: &Utf8Path) -> Vec<String> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(source_abs)
        .args(["tag", "--points-at", "HEAD", "--sort=refname"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn version_handle_id(tag: &str) -> String {
    format!("code-version:{tag}")
}

fn meta_values(batch: &FactBatch, key: &str) -> BTreeMap<String, String> {
    batch
        .meta
        .iter()
        .filter(|meta| meta.key == key)
        .map(|meta| (meta.handle.clone(), meta.value.clone()))
        .collect()
}

fn push_code_meta(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    handle: &str,
    file: &str,
    key: &str,
    value: &str,
) {
    let identity = code_identity(batch, root, revision, handle, file);
    push_meta_fact(batch, &identity, handle, key, value);
}

fn push_meta_fact(
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

fn ensure_external_code_handle(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    file: &str,
    handle: &str,
    qualified: &str,
) {
    if batch.handles.iter().any(|existing| existing.id == handle) {
        return;
    }
    let identity = code_identity(batch, root, revision, handle, file);
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
    push_meta_fact(
        batch,
        &identity,
        handle,
        meta_key::QUALIFIED_NAME,
        qualified,
    );
    push_meta_fact(
        batch,
        &identity,
        handle,
        meta_key::EXTERNAL_CLASS,
        SOURCE_NAME,
    );
}

fn emit_content_budget_meta(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    package_handle: &str,
    budget: &ContentBudgetReport,
) {
    let identity = code_identity(batch, root, revision, package_handle, package_handle);
    let values = [
        (
            meta_key::CONTENT_BUDGET_ROOT_DISPOSITION,
            budget.disposition.as_str(),
        ),
        (
            meta_key::CONTENT_BUDGET_BYTES,
            &budget.content_budget_bytes.to_string(),
        ),
        (
            meta_key::MEMBER_DOC_ITEM_BYTES,
            &budget.member_doc_budget_bytes.to_string(),
        ),
        (meta_key::DOC_BYTES, &budget.doc_bytes.to_string()),
        (
            meta_key::MEMBER_DOC_BYTES,
            &budget.member_doc_bytes.to_string(),
        ),
        (
            meta_key::STRUCTURAL_DOC_BYTES,
            &budget.structural_doc_bytes.to_string(),
        ),
        (
            meta_key::SIGNATURE_BYTES,
            &budget.signature_bytes.to_string(),
        ),
    ];
    for (key, value) in values {
        push_meta_fact(batch, &identity, package_handle, key, value);
    }
}

fn code_identity(
    batch: &FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    native_id: &str,
    file: &str,
) -> FactIdentity {
    let origin_uri = if file.is_empty() {
        format!("rustdoc://{native_id}")
    } else {
        format!("file://{}", root.join(file))
    };
    FactIdentity::new(
        batch.corpus.clone(),
        batch.source.clone(),
        NativeId::from(native_id.to_string()),
        OriginUri::from(origin_uri),
        revision.clone(),
        batch.generation,
    )
}

fn normalize_code_source_path(
    root: &Utf8Path,
    source_root: &Utf8Path,
    raw_path: &str,
) -> Option<String> {
    let path = Utf8PathBuf::from(raw_path);
    let relative = if path.is_absolute() {
        normalize_path_inside_root(root, &path)?
    } else if path.starts_with(source_root) {
        path
    } else {
        source_root.join(path)
    };
    normalize_relative_path(
        relative.as_str(),
        anneal_core::RelativePathPolicy::STRICT_NON_EMPTY,
    )
    .map(|path| path.to_string())
}

fn markdown_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("](") {
        let after = &rest[open + 2..];
        let Some(close) = after.find(')') else {
            break;
        };
        let target = after[..close].trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
        rest = &after[close + 1..];
    }
    out.sort();
    out.dedup();
    out
}

fn signature_type_refs(signatures: &[String]) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for signature in signatures {
        for token in signature
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':')))
        {
            if token.contains(".t") || token.contains(':') {
                refs.insert(token.trim_matches(':').to_string());
            }
        }
    }
    refs.into_iter().filter(|value| !value.is_empty()).collect()
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
    use std::collections::HashMap;
    use std::fs;

    use anneal_core::{
        ActorContext, CancellationToken, ConfigEntry, ConfigFacts, CorpusId, Generation,
    };
    use beam_file::chunk::RawChunk;
    use eetf::{Atom, Binary, FixInteger, List, Map, Term, Tuple};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn context<'a>(root: &'a Utf8PathBuf, config: &'a ConfigFacts) -> SourceContext<'a> {
        context_for_roots(std::slice::from_ref(root), config)
    }

    fn context_for_roots<'a>(
        roots: &'a [Utf8PathBuf],
        config: &'a ConfigFacts,
    ) -> SourceContext<'a> {
        SourceContext {
            corpus: CorpusId::from("test"),
            roots,
            config_facts: config,
            probe_code_target_history: false,
            read_code_drift_evidence: false,
            refresh_code_drift_evidence: false,
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
        fs::create_dir_all(root.join("tests")).expect("create tests");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("write cargo");
        fs::write(
            root.join("src/lib.rs"),
            "pub struct Widget;\npub trait Other {}\npub fn make() -> Widget { Widget }\n",
        )
        .expect("write lib");
        fs::write(
            root.join("src/private.rs"),
            "fn helper() {}\n// TODO: tighten private helper\n",
        )
        .expect("write private");
        fs::write(
            root.join("src/generated.rs"),
            "// @generated by fixture\n// DO NOT EDIT\npub fn generated() {}\n",
        )
        .expect("write generated");
        fs::write(
            root.join("tests/integration.rs"),
            "#[test]\nfn smoke() {}\n",
        )
        .expect("write integration test");
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

    fn atom(value: &str) -> Term {
        Term::from(Atom::from(value))
    }

    fn binary(value: &str) -> Term {
        Term::from(Binary::from(value.as_bytes()))
    }

    fn int(value: i32) -> Term {
        Term::from(FixInteger::from(value))
    }

    fn list(values: Vec<Term>) -> Term {
        Term::from(List::from(values))
    }

    fn tuple(values: Vec<Term>) -> Term {
        Term::from(Tuple::from(values))
    }

    fn map(values: Vec<(Term, Term)>) -> Term {
        Term::from(Map::from(values.into_iter().collect::<HashMap<_, _>>()))
    }

    fn doc(text: &str) -> Term {
        map(vec![(binary("en"), binary(text))])
    }

    fn metadata(values: Vec<(&str, Term)>) -> Term {
        map(values
            .into_iter()
            .map(|(key, value)| (atom(key), value))
            .collect())
    }

    fn eep48_docs_term() -> Term {
        let module_metadata = metadata(vec![
            ("source_path", binary("lib/herald/agent.ex")),
            ("behaviours", list(vec![atom("Herald.AgentBehaviour")])),
        ]);
        let entry_metadata = metadata(vec![
            ("deprecated", binary("use start/1")),
            ("since", binary("1.0")),
        ]);
        let entry = tuple(vec![
            tuple(vec![atom("function"), atom("run"), int(2)]),
            int(12),
            list(vec![binary(
                "run(agent :: Herald.Agent.t, opts :: Keyword.t) :: {:ok, String.t}",
            )]),
            doc("Run the agent. See [Guide](guides/agent.md)."),
            entry_metadata,
        ]);
        tuple(vec![
            atom("docs_v1"),
            int(1),
            atom("elixir"),
            atom("markdown"),
            doc("Agent module docs."),
            module_metadata,
            list(vec![entry]),
        ])
    }

    fn write_eep48_fixture(root: &Utf8Path) {
        fs::create_dir_all(root.join("lib/herald")).expect("create lib");
        fs::create_dir_all(root.join("_build/dev/lib/herald/ebin")).expect("create ebin");
        fs::write(
            root.join("mix.exs"),
            "defmodule Herald.MixProject do\nend\n",
        )
        .expect("write mix");
        fs::write(
            root.join("lib/herald/agent.ex"),
            "defmodule Herald.Agent do\n  @behaviour Herald.AgentBehaviour\n  def run(agent, opts), do: {:ok, agent}\nend\n\ndefimpl Herald.Protocol, for: Herald.Agent do\nend\n",
        )
        .expect("write elixir source");
        let mut docs = Vec::new();
        eep48_docs_term().encode(&mut docs).expect("encode docs");
        RawBeamFile {
            chunks: vec![RawChunk {
                id: *b"Docs",
                data: docs,
            }],
        }
        .to_file(root.join("_build/dev/lib/herald/ebin/Elixir.Herald.Agent.beam"))
        .expect("write beam");
    }

    fn code_meta<'a>(batch: &'a FactBatch, handle: &str, key: &str) -> Vec<&'a str> {
        batch
            .meta
            .iter()
            .filter(|meta| meta.handle == handle && meta.key == key)
            .map(|meta| meta.value.as_str())
            .collect()
    }

    fn handle_class(batch: &FactBatch, handle: &str) -> Option<String> {
        batch
            .meta
            .iter()
            .find(|meta| meta.handle == handle && meta.key == meta_key::CLASS)
            .map(|meta| meta.value.clone())
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

    #[test]
    fn rustdoc_source_classifies_source_tree_files_and_obligations() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("target/doc")).expect("create target doc");
        write_fixture(&root);
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(config_key::RUSTDOC_JSON, "target/doc/demo.json"),
            ConfigEntry::scalar(config_key::ARTIFACT_REVISION, "abc123"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");
        let widget = batch
            .handles
            .iter()
            .find(|handle| {
                handle.kind == "section" && handle.id.starts_with("src/lib.rs#demo::Widget@")
            })
            .expect("widget handle");

        assert_eq!(
            handle_class(&batch, "src/lib.rs").as_deref(),
            Some(relation_value::CLASS_PUBLIC_API)
        );
        assert_eq!(
            handle_class(&batch, &widget.id).as_deref(),
            Some(relation_value::CLASS_PUBLIC_API)
        );
        assert_eq!(
            handle_class(&batch, "src/private.rs").as_deref(),
            Some(relation_value::CLASS_PRIVATE)
        );
        assert_eq!(
            handle_class(&batch, "tests/integration.rs").as_deref(),
            Some(relation_value::CLASS_TEST)
        );
        assert_eq!(
            handle_class(&batch, "src/generated.rs").as_deref(),
            Some(relation_value::CLASS_GENERATED)
        );
        assert!(batch.handles.iter().any(|handle| {
            handle.kind == "file"
                && handle.id == "src/private.rs"
                && handle.summary == "src/private.rs"
        }));
        assert_eq!(
            code_meta(&batch, "src/private.rs", meta_key::OBLIGATION_COUNT),
            vec!["1"]
        );
        assert!(
            code_meta(&batch, "src/private.rs", meta_key::OBLIGATION)
                .iter()
                .any(|value| value.starts_with("TODO:2:"))
        );
        assert!(batch.concerns.iter().any(|concern| {
            concern.name == concern_name::CODE_TODO && concern.member == "src/private.rs"
        }));
    }

    #[test]
    fn source_root_alone_activates_file_level_extraction() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("target/doc")).expect("create target doc");
        write_fixture(&root);
        let config =
            ConfigFacts::try_from_entries(vec![ConfigEntry::scalar(config_key::SOURCE_ROOT, ".")])
                .expect("config facts");
        assert!(CodeSource::is_configured(&config));

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");
        assert!(
            batch
                .handles
                .iter()
                .any(|handle| handle.kind == "file" && handle.id == "src/lib.rs"),
            "standalone mode emits file handles from the source tree"
        );
        assert!(
            !batch.handles.iter().any(|handle| handle.kind == "section"),
            "no artifact means no item handles"
        );
        assert_eq!(
            handle_class(&batch, "src/private.rs").as_deref(),
            Some(relation_value::CLASS_PRIVATE)
        );
        assert_eq!(
            handle_class(&batch, "tests/integration.rs").as_deref(),
            Some(relation_value::CLASS_TEST)
        );
        assert!(batch.concerns.iter().any(|concern| {
            concern.name == concern_name::CODE_TODO && concern.member == "src/private.rs"
        }));
    }

    #[test]
    fn source_extension_config_widens_the_file_level_scan() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("scripts")).expect("create scripts");
        fs::write(root.join("scripts/tool.py"), "# TODO: port me\n").expect("write py");
        fs::write(root.join("scripts/build.rs"), "fn main() {}\n").expect("write rs");
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(config_key::SOURCE_ROOT, "."),
            ConfigEntry::scalar(config_key::SOURCE_EXTENSION, "py"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");
        assert!(
            batch
                .handles
                .iter()
                .any(|handle| handle.id == "scripts/tool.py"),
            "configured extension is scanned"
        );
        assert!(
            !batch
                .handles
                .iter()
                .any(|handle| handle.id == "scripts/build.rs"),
            "explicit extension config replaces the default set"
        );
    }

    #[test]
    fn rustdoc_source_projects_git_version_tags_as_package_metadata() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        fs::create_dir_all(root.join("target/doc")).expect("create target doc");
        write_fixture(&root);
        for args in [
            &["init"][..],
            &["config", "user.email", "anneal@example.test"],
            &["config", "user.name", "Anneal Test"],
            &["add", "."],
            &["commit", "-m", "fixture"],
            &["tag", "demo-0.1.0"],
        ] {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "git {:?} failed: {}{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let config = ConfigFacts::try_from_entries(vec![ConfigEntry::scalar(
            config_key::RUSTDOC_JSON,
            "target/doc/demo.json",
        )])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");

        assert!(batch.handles.iter().any(|handle| {
            handle.id == "code-version:demo-0.1.0"
                && handle.kind == "version"
                && handle.namespace == SOURCE_NAME
        }));
        assert_eq!(
            code_meta(&batch, "Cargo.toml", meta_key::VERSION_TAG),
            vec!["demo-0.1.0"]
        );
        assert!(batch.edges.iter().any(|edge| {
            edge.from == "Cargo.toml"
                && edge.to == "code-version:demo-0.1.0"
                && edge.kind == edge_kind::CONTAINS
        }));
    }

    #[test]
    fn rustdoc_source_projects_each_root_independently() {
        let dir = tempdir().expect("tempdir");
        let root_a = Utf8PathBuf::from_path_buf(dir.path().join("a")).expect("utf8 tempdir");
        let root_b = Utf8PathBuf::from_path_buf(dir.path().join("b")).expect("utf8 tempdir");
        for root in [&root_a, &root_b] {
            fs::create_dir_all(root.join("target/doc")).expect("create target doc");
            write_fixture(root);
        }
        let roots = vec![root_a, root_b];
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(config_key::RUSTDOC_JSON, "target/doc/demo.json"),
            ConfigEntry::scalar(config_key::ARTIFACT_REVISION, "abc123"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context_for_roots(&roots, &config))
            .expect("code extraction");

        assert_eq!(
            batch
                .handles
                .iter()
                .filter(|handle| handle.kind == "file" && handle.id == "src/private.rs")
                .count(),
            2
        );
        assert_eq!(
            batch
                .meta
                .iter()
                .filter(|meta| {
                    meta.handle == "src/private.rs"
                        && meta.key == meta_key::CLASS
                        && meta.value == relation_value::CLASS_PRIVATE
                })
                .count(),
            2
        );
    }

    #[test]
    fn eep48_source_projects_elixir_docs_and_metadata() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        write_eep48_fixture(&root);
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(
                config_key::EEP48_BEAM,
                "_build/dev/lib/herald/ebin/Elixir.Herald.Agent.beam",
            ),
            ConfigEntry::scalar(config_key::SOURCE_ROOT, "."),
            ConfigEntry::scalar(config_key::PACKAGE, "herald"),
            ConfigEntry::scalar(config_key::ARTIFACT_REVISION, "abc123"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");
        let module = "lib/herald/agent.ex#Herald.Agent";
        let member = "lib/herald/agent.ex#Herald.Agent.run/2";

        assert!(batch.handles.iter().any(|handle| {
            handle.id == module && handle.kind == "section" && handle.namespace == "module"
        }));
        assert!(batch.handles.iter().any(|handle| {
            handle.id == member
                && handle.status.as_deref() == Some("deprecated")
                && handle.line == 12
        }));
        assert_eq!(
            code_meta(&batch, module, meta_key::QUALIFIED_NAME),
            vec!["Herald.Agent"]
        );
        assert_eq!(
            code_meta(&batch, member, meta_key::QUALIFIED_NAME),
            vec!["Herald.Agent.run/2"]
        );
        assert_eq!(
            code_meta(&batch, member, meta_key::DEPRECATED_NOTE),
            vec!["use start/1"]
        );
        assert_eq!(code_meta(&batch, member, meta_key::SINCE), vec!["1.0"]);
        assert!(batch.edges.iter().any(|edge| {
            edge.from == module
                && edge.kind == edge_kind::IMPLEMENTS
                && edge.to.contains("Herald.AgentBehaviour")
        }));
        assert!(batch.edges.iter().any(|edge| {
            edge.from == member && edge.kind == edge_kind::CITES && edge.to.contains("guides")
        }));
        assert!(batch.edges.iter().any(|edge| {
            edge.from == member && edge.kind == edge_kind::USES_TYPE && edge.to.contains("String.t")
        }));
        assert!(
            batch.content.iter().any(|content| {
                content.handle == member && content.text.contains("Run the agent")
            })
        );
        assert!(batch.edges.iter().any(|edge| {
            edge.file == "lib/herald/agent.ex"
                && edge.kind == edge_kind::IMPLEMENTS
                && edge.to.contains("Herald.Protocol")
        }));
    }

    #[test]
    fn eep48_source_declares_member_doc_budget_truncation() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
        write_eep48_fixture(&root);
        let config = ConfigFacts::try_from_entries(vec![
            ConfigEntry::scalar(
                config_key::EEP48_BEAM,
                "_build/dev/lib/herald/ebin/Elixir.Herald.Agent.beam",
            ),
            ConfigEntry::scalar(config_key::CONTENT_BUDGET_BYTES, "1"),
            ConfigEntry::scalar(config_key::MEMBER_DOC_BUDGET_BYTES, "8"),
        ])
        .expect("config facts");

        let batch = CodeSource
            .extract(&context(&root, &config))
            .expect("code extraction");

        assert!(batch.meta.iter().any(|meta| {
            meta.handle == "mix.exs"
                && meta.key == meta_key::CONTENT_BUDGET_ROOT_DISPOSITION
                && meta.value == relation_value::BUDGET_TRUNCATED
        }));
        assert!(batch.meta.iter().any(|meta| {
            meta.handle == "lib/herald/agent.ex#Herald.Agent.run/2"
                && meta.key == meta_key::CONTENT_BUDGET_DISPOSITION
                && meta.value == relation_value::PER_ITEM_CAP
        }));
    }
}
