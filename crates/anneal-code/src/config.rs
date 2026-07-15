use super::{
    ConfigFacts, DEFAULT_CONTENT_BUDGET_BYTES, DEFAULT_MEMBER_DOC_BUDGET_BYTES,
    DEFAULT_SOURCE_EXTENSIONS, JsonValue, SourceError, Utf8Path, Utf8PathBuf, config_key, fs,
    normalize_relative_path,
};

#[derive(Clone, Debug)]
pub(super) struct CodeDiscoveryConfig {
    pub(super) artifacts: Vec<Utf8PathBuf>,
    pub(super) eep48_beams: Vec<Utf8PathBuf>,
    pub(super) eep48_beam_dirs: Vec<Utf8PathBuf>,
    pub(super) eep48_doc_chunks: Vec<Utf8PathBuf>,
    pub(super) source_root: Utf8PathBuf,
    pub(super) source_extensions: Vec<String>,
    pub(super) package: Option<String>,
    pub(super) manifest: Option<Utf8PathBuf>,
    pub(super) artifact_revision: Option<String>,
    pub(super) content_budget_bytes: usize,
    pub(super) member_doc_budget_bytes: usize,
}

impl CodeDiscoveryConfig {
    pub(super) fn from_facts(facts: &ConfigFacts) -> Result<Self, SourceError> {
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

pub(super) fn parse_usize_config(value: &str) -> Result<usize, SourceError> {
    value.parse::<usize>().map_err(|source| {
        SourceError::InvalidConfig(format!("code budget values must be byte counts: {source}"))
    })
}

pub(super) fn valid_relative_path(value: &str) -> Result<Utf8PathBuf, SourceError> {
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
pub(super) fn valid_source_root(value: &str) -> Result<Utf8PathBuf, SourceError> {
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

pub(super) fn ensure_source_root_within_project(
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

pub(super) fn read_manifest(
    root: &Utf8Path,
    manifest: &Utf8Path,
) -> Result<ArtifactManifest, SourceError> {
    let path = root.join(manifest);
    let text = fs::read_to_string(&path).map_err(|source| SourceError::io(&path, source))?;
    let value = serde_json::from_str::<JsonValue>(&text)
        .map_err(|source| SourceError::Other(format!("{path}: {source}")))?;
    Ok(ArtifactManifest { value })
}

#[derive(Clone, Debug)]
pub(super) struct ArtifactManifest {
    value: JsonValue,
}

impl ArtifactManifest {
    pub(super) fn source_revision(&self) -> Option<String> {
        self.string_at(&[
            &["source_revision"],
            &["source", "revision"],
            &["artifact", "source_revision"],
        ])
    }

    pub(super) fn package_name(&self) -> Option<String> {
        self.string_at(&[
            &["package"],
            &["package_name"],
            &["package", "name"],
            &["crate"],
            &["crate_name"],
        ])
    }

    pub(super) fn string_at(&self, paths: &[&[&str]]) -> Option<String> {
        paths.iter().find_map(|path| {
            let mut value = &self.value;
            for key in *path {
                value = value.get(*key)?;
            }
            value.as_str().map(ToOwned::to_owned)
        })
    }
}
