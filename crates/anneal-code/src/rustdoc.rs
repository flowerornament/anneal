use super::*;

pub(super) fn extract_rustdoc(
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

pub(super) struct RustdocProjector<'a> {
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

pub(super) struct ProjectorInput<'a> {
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
    pub(super) fn new(input: ProjectorInput<'a>) -> Self {
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

    pub(super) fn project(&mut self, batch: &mut FactBatch) -> Result<(), SourceError> {
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

    pub(super) fn index_handles(&mut self) {
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

    pub(super) fn local_items(&self) -> impl Iterator<Item = (&Id, &Item)> {
        self.rustdoc
            .index
            .iter()
            .filter(|(_, item)| item.crate_id == 0)
    }

    pub(super) fn unique_handle(&mut self, base: String) -> String {
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

    pub(super) fn item_file(&self, item: &Item) -> Option<String> {
        item.span
            .as_ref()
            .and_then(|span| normalize_span_filename(self.root, &self.source_root, &span.filename))
    }

    pub(super) fn emit_file_handles(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_package_meta(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_item_handles(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_structure_edges(&self, batch: &mut FactBatch) {
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

    pub(super) fn emit_type_edges(&mut self, batch: &mut FactBatch) -> Result<(), SourceError> {
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

    pub(super) fn emit_doc_link_edges(&mut self, batch: &mut FactBatch) {
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

    pub(super) fn emit_impl_edges(&mut self, batch: &mut FactBatch) {
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

    pub(super) fn emit_content(&mut self, batch: &mut FactBatch) {
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

    pub(super) fn budgeted_docs<'b>(
        &self,
        item: &Item,
        docs: &'b str,
    ) -> (&'b str, Option<&'static str>) {
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

    pub(super) fn content_budget_report(&self) -> ContentBudgetReport {
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

    pub(super) fn handle_for_target(&mut self, batch: &mut FactBatch, id: Id) -> String {
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

    pub(super) fn handle_for_impl_type(&mut self, batch: &mut FactBatch, type_: &Type) -> String {
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

    pub(super) fn emit_external_handle(
        &self,
        batch: &mut FactBatch,
        handle: &str,
        qualified: &str,
    ) {
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

    pub(super) fn push_edge(
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
        code_identity(batch, self.root, &self.revision, native_id, file)
    }
}

pub(super) fn crate_name_from_root(rustdoc: &RustdocCrate) -> Option<String> {
    rustdoc.index.get(&rustdoc.root).and_then(|item| {
        item.name
            .clone()
            .or_else(|| qualified_name(rustdoc, rustdoc.root))
    })
}

pub(super) fn markdown_links(text: &str) -> Vec<String> {
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

pub(super) fn signature_type_refs(signatures: &[String]) -> Vec<String> {
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

pub(super) fn normalize_span_filename(
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

pub(super) fn qualified_name(rustdoc: &RustdocCrate, id: Id) -> Option<String> {
    rustdoc
        .paths
        .get(&id)
        .and_then(|summary| (!summary.path.is_empty()).then(|| summary.path.join("::")))
}

pub(super) fn adapter_local_id(rustdoc: &RustdocCrate, id: Id, item: &Item, file: &str) -> String {
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

pub(super) fn stable_fragment(value: &str) -> String {
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

pub(super) fn item_summary(item: &Item, qualified: &str) -> String {
    let kind = item_kind_name(item.inner.item_kind());
    item.docs
        .as_deref()
        .and_then(|docs| docs.lines().find(|line| !line.trim().is_empty()))
        .map(str::trim)
        .map_or_else(|| format!("{kind} {qualified}"), ToOwned::to_owned)
}

pub(super) fn item_signature(item: &Item, qualified: &str) -> String {
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

pub(super) fn function_signature(qualified: &str, signature: &FunctionSignature) -> String {
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

pub(super) fn content_text(signature: &str, docs: &str) -> String {
    match (signature.trim().is_empty(), docs.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => signature.to_string(),
        (true, false) => docs.to_string(),
        (false, false) => format!("{signature}\n\n{docs}"),
    }
}

pub(super) fn type_label(type_: &Type) -> String {
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

pub(super) fn type_target_id(type_: &Type) -> Option<Id> {
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

pub(super) fn direct_children(inner: &ItemEnum) -> Vec<Id> {
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

pub(super) fn resolved_path_ids(inner: &ItemEnum) -> Result<Vec<Id>, SourceError> {
    let value = serde_json::to_value(inner)
        .map_err(|source| SourceError::Other(format!("rustdoc type traversal failed: {source}")))?;
    let mut out = Vec::new();
    collect_resolved_path_ids(&value, &mut out);
    Ok(out)
}

pub(super) fn collect_resolved_path_ids(value: &JsonValue, out: &mut Vec<Id>) {
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

pub(super) fn json_id(value: Option<&JsonValue>) -> Option<Id> {
    value
        .and_then(JsonValue::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .map(Id)
}

pub(super) fn impl_kind(impl_: &Impl) -> &'static str {
    if impl_.blanket_impl.is_some() {
        "blanket_impl"
    } else {
        "trait_impl"
    }
}

pub(super) fn impl_signature(impl_: &Impl) -> String {
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

pub(super) fn is_member_item(kind: ItemKind) -> bool {
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
