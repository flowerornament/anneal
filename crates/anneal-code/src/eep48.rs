use super::{
    ArtifactManifest, CodeDiscoveryConfig, ContentBudgetReport, ContentFact, Cursor, EdgeFact,
    EetfTerm, FactBatch, FactBatchMode, FactIdentity, HandleFact, RawBeamFile, Revision,
    SOURCE_NAME, SourceContext, SourceError, SourceName, SpanFact, Utf8Path, Utf8PathBuf, area_for,
    code_identity, content_text, edge_kind, emit_content_budget_meta, ensure_external_code_handle,
    first_paragraph, fs, markdown_links, meta_key, normalize_code_source_path,
    normalize_path_inside_root, package_root_file, push_meta_fact, relation_value,
    signature_type_refs, stable_fragment, token_count, truncate_at_char_boundary,
};

pub(super) fn extract_eep48_set(
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

pub(super) fn eep48_artifacts(
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

pub(super) fn collect_eep48_beams(
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

pub(super) fn eep48_docs_from_artifact(path: &Utf8Path) -> Result<Eep48ModuleDocs, SourceError> {
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

pub(super) fn eep48_docs_from_beam(path: &Utf8Path) -> Result<Eep48ModuleDocs, SourceError> {
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

pub(super) fn external_doc_chunk_for_beam(path: &Utf8Path) -> Option<Utf8PathBuf> {
    let module = path.file_stem()?;
    let ebin = path.parent()?;
    let app = ebin.parent()?;
    Some(
        app.join("doc")
            .join("chunks")
            .join(format!("{module}.chunk")),
    )
}

pub(super) fn module_name_from_artifact(path: &Utf8Path) -> String {
    path.file_stem()
        .unwrap_or("unknown")
        .strip_prefix("Elixir.")
        .unwrap_or_else(|| path.file_stem().unwrap_or("unknown"))
        .to_string()
}

#[derive(Clone, Debug)]
pub(super) struct Eep48ModuleDocs {
    module: String,
    docs_source: String,
    term: Option<EetfTerm>,
}

pub(super) struct Eep48ArtifactDocs {
    artifact: Utf8PathBuf,
    docs: Eep48ModuleDocs,
    parsed: Eep48ParsedDocs,
}

#[derive(Clone, Debug)]
pub(super) struct Eep48Entry {
    kind: String,
    name: String,
    arity: i32,
    line: u32,
    signatures: Vec<String>,
    doc: Eep48Doc,
    metadata: Eep48Metadata,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Eep48Metadata {
    behaviours: Vec<String>,
    deprecated: Option<String>,
    hidden: bool,
    since: Option<String>,
    source_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Eep48ParsedDocs {
    doc_format: Option<String>,
    module_doc: Eep48Doc,
    metadata: Eep48Metadata,
    entries: Vec<Eep48Entry>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Eep48Doc {
    state: String,
    text: String,
}

impl Eep48Doc {
    pub(super) fn documented(text: String) -> Self {
        Self {
            state: relation_value::DOC_STATE_DOCUMENTED.to_string(),
            text,
        }
    }

    pub(super) fn hidden() -> Self {
        Self {
            state: relation_value::DOC_STATE_HIDDEN.to_string(),
            text: String::new(),
        }
    }

    pub(super) fn missing() -> Self {
        Self {
            state: relation_value::DOC_STATE_MISSING.to_string(),
            text: String::new(),
        }
    }

    pub(super) fn none() -> Self {
        Self {
            state: relation_value::DOC_STATE_NONE.to_string(),
            text: String::new(),
        }
    }
}

pub(super) fn parse_eep48_docs(docs: &Eep48ModuleDocs) -> Result<Eep48ParsedDocs, SourceError> {
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

pub(super) fn eep48_content_budget_report(
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

pub(super) fn eep48_entry(term: &EetfTerm) -> Option<Eep48Entry> {
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

pub(super) fn eep48_doc(term: &EetfTerm) -> Eep48Doc {
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

pub(super) fn eep48_metadata(term: &EetfTerm) -> Eep48Metadata {
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

pub(super) fn metadata_strings(term: &EetfTerm) -> Vec<String> {
    list_elements(term).map_or_else(
        || term_string(term).into_iter().collect(),
        |items| items.iter().filter_map(term_string).collect(),
    )
}

pub(super) fn bool_metadata(term: &EetfTerm) -> Option<bool> {
    match atom_name(term) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

pub(super) fn annotation_line(term: &EetfTerm) -> u32 {
    if let Some(line) = term_i32(term).and_then(|line| u32::try_from(line).ok()) {
        return line;
    }
    tuple_elements(term)
        .and_then(|items| items.first())
        .and_then(term_i32)
        .and_then(|line| u32::try_from(line).ok())
        .unwrap_or(1)
}

pub(super) fn tuple_elements(term: &EetfTerm) -> Option<&[EetfTerm]> {
    match term {
        EetfTerm::Tuple(tuple) => Some(&tuple.elements),
        _ => None,
    }
}

pub(super) fn list_elements(term: &EetfTerm) -> Option<&[EetfTerm]> {
    match term {
        EetfTerm::List(list) => Some(&list.elements),
        EetfTerm::ByteList(byte_list) if byte_list.bytes.is_empty() => Some(&[]),
        _ => None,
    }
}

pub(super) fn term_map(term: &EetfTerm) -> Option<&std::collections::HashMap<EetfTerm, EetfTerm>> {
    match term {
        EetfTerm::Map(map) => Some(&map.map),
        _ => None,
    }
}

pub(super) fn atom_name(term: &EetfTerm) -> Option<&str> {
    match term {
        EetfTerm::Atom(atom) => Some(atom.name.as_str()),
        _ => None,
    }
}

pub(super) fn term_string(term: &EetfTerm) -> Option<String> {
    match term {
        EetfTerm::Atom(atom) => Some(atom.name.clone()),
        EetfTerm::Binary(binary) => String::from_utf8(binary.bytes.clone()).ok(),
        EetfTerm::ByteList(byte_list) => String::from_utf8(byte_list.bytes.clone()).ok(),
        _ => None,
    }
}

pub(super) fn term_i32(term: &EetfTerm) -> Option<i32> {
    match term {
        EetfTerm::FixInteger(value) => Some(value.value),
        EetfTerm::BigInteger(value) => value.value.to_string().parse().ok(),
        _ => None,
    }
}

pub(super) struct Eep48Projector {
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

pub(super) struct Eep48ProjectorInput<'a> {
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
    pub(super) fn new(input: Eep48ProjectorInput<'_>) -> Self {
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

    pub(super) fn project(&mut self, batch: &mut FactBatch) {
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

    pub(super) fn emit_file_handle(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_module_handle(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_member_handles(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_package_meta(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_structure_edges(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_behaviour_edges(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_doc_link_edges(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_content(&mut self, batch: &mut FactBatch) {
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

    pub(super) fn emit_one_content(
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

    pub(super) fn content_budget_report(&self) -> ContentBudgetReport {
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

    pub(super) fn budgeted_docs<'b>(
        &self,
        docs: &'b str,
        member: bool,
    ) -> (&'b str, Option<&'static str>) {
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

    pub(super) fn emit_budget_meta(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_deprecation_meta(
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

    pub(super) fn member_handle(&self, entry: &Eep48Entry) -> String {
        format!(
            "{}#{}.{}/{}",
            self.module_file, self.docs.module, entry.name, entry.arity
        )
    }

    pub(super) fn member_qualified_name(&self, entry: &Eep48Entry) -> String {
        format!("{}.{}/{}", self.docs.module, entry.name, entry.arity)
    }

    pub(super) fn member_signature(entry: &Eep48Entry) -> String {
        if entry.signatures.is_empty() {
            return format!("{}({})", entry.name, entry.arity);
        }
        entry.signatures.join("\n")
    }

    pub(super) fn external_handle(&self, batch: &mut FactBatch, qualified: &str) -> String {
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

    pub(super) fn push_edge(
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

    pub(super) fn push_meta(
        batch: &mut FactBatch,
        identity: &FactIdentity,
        handle: &str,
        key: &str,
        value: &str,
    ) {
        push_meta_fact(batch, identity, handle, key, value);
    }

    pub(super) fn identity_for(
        &self,
        batch: &FactBatch,
        native_id: &str,
        file: &str,
    ) -> FactIdentity {
        code_identity(batch, &self.root, &self.revision, native_id, file)
    }
}
