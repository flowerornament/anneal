use super::{
    BTreeMap, BTreeSet, Command, ConcernFact, EdgeFact, FactBatch, HandleFact, Revision,
    SOURCE_NAME, SourceError, Utf8Path, Utf8PathBuf, area_for, code_identity, concern_name,
    edge_kind, ensure_external_code_handle, fs, git_version_tags, meta_key, meta_values,
    normalize_path_inside_root, normalize_relative_path, package_root_file, push_code_meta,
    push_meta_fact, relation_value, stable_fragment, truncate_at_char_boundary, version_handle_id,
};

#[derive(Clone, Debug, Default)]
pub(super) struct SourceTreeClassification {
    files: BTreeMap<String, SourceFileClass>,
    protocol_impls: Vec<ProtocolImpl>,
    tags: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct SourceFileClass {
    generated: bool,
    obligations: Vec<CodeObligation>,
    test: bool,
}

#[derive(Clone, Debug)]
pub(super) struct CodeObligation {
    kind: &'static str,
    line: u32,
    text: String,
}

#[derive(Clone, Debug)]
pub(super) struct ProtocolImpl {
    file: String,
    line: u32,
    protocol: String,
    target: Option<String>,
}

impl SourceTreeClassification {
    pub(super) fn scan(
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
        // Prefer git-tracked files: on a real repo the working tree can carry
        // gigabytes of ignored assets/build output (herald's priv/ is 7.9G),
        // and a raw recursive walk drowns in it. Tracked files ARE the source,
        // and drift already reasons over git history, so this is the honest
        // boundary. Fall back to a filesystem walk when git can't answer.
        match git_tracked_files(&source_abs, extensions) {
            Some(files) => {
                for relative in files {
                    classify_source_path(&source_abs, &relative, &mut out)?;
                }
            }
            None => scan_source_dir(&source_abs, &source_abs, extensions, &mut out)?,
        }
        Ok(out)
    }

    pub(super) fn project(
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

    pub(super) fn ensure_source_file_handles(
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

    pub(super) fn class_for_handle(
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

    pub(super) fn emit_obligations(
        &self,
        batch: &mut FactBatch,
        root: &Utf8Path,
        revision: &Revision,
    ) {
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

    pub(super) fn emit_protocol_impls(
        &self,
        batch: &mut FactBatch,
        root: &Utf8Path,
        revision: &Revision,
    ) {
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

    pub(super) fn emit_version_tags(
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

pub(super) fn scan_source_dir(
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
        classify_source_path(base, relative.as_str(), out)?;
    }
    Ok(())
}

/// Read, classify, and record one source file. The base-relative path is the
/// handle id — the path space citations resolve against.
pub(super) fn classify_source_path(
    base: &Utf8Path,
    relative: &str,
    out: &mut SourceTreeClassification,
) -> Result<(), SourceError> {
    let relative =
        normalize_relative_path(relative, anneal_core::RelativePathPolicy::STRICT_NON_EMPTY)
            .map_or_else(|| relative.to_string(), |path| path.to_string());
    let path = base.join(&relative);
    let text = fs::read_to_string(&path).map_err(|source| SourceError::io(&path, source))?;
    let test_path = is_test_path(Utf8Path::new("."), &relative);
    out.protocol_impls.extend(protocol_impls(&relative, &text));
    out.files
        .insert(relative, classify_source_file(&text, test_path));
    Ok(())
}

/// Git-tracked source files under `base`, filtered to the configured
/// extensions and sorted. `None` when `base` is not a git working tree (or
/// git is unavailable) — the caller falls back to a filesystem walk.
pub(super) fn git_tracked_files(base: &Utf8Path, extensions: &[String]) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(base)
        .args(["ls-files", "-z", "--cached", "--exclude-standard"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            Utf8Path::new(entry)
                .extension()
                .is_some_and(|ext| extensions.iter().any(|allowed| allowed == ext))
        })
        .map(ToOwned::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Some(files)
}

pub(super) fn should_skip_source_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".jj" | ".svn" | "target" | "_build" | "deps" | "node_modules" | ".direnv"
    )
}

pub(super) fn is_test_path(source_root: &Utf8Path, relative: &str) -> bool {
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

pub(super) fn classify_source_file(text: &str, test_path: bool) -> SourceFileClass {
    SourceFileClass {
        generated: has_generated_marker(text),
        obligations: code_obligations(text),
        test: test_path || has_test_marker(text),
    }
}

pub(super) fn has_generated_marker(text: &str) -> bool {
    text.lines().take(80).any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.contains("@generated")
            || line.contains("automatically generated")
            || line.contains("auto-generated")
            || line.contains("do not edit")
            || line.contains("generated by")
    })
}

pub(super) fn has_test_marker(text: &str) -> bool {
    text.contains("#[cfg(test)]")
        || text.contains("#[test]")
        || text.contains("mod tests")
        || text.contains("ExUnit.Case")
}

pub(super) fn code_obligations(text: &str) -> Vec<CodeObligation> {
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

pub(super) fn obligation_kind(line: &str) -> Option<&'static str> {
    if line.contains("FIXME") {
        Some("FIXME")
    } else if line.contains("TODO") {
        Some("TODO")
    } else {
        None
    }
}

pub(super) fn protocol_impls(file: &str, text: &str) -> Vec<ProtocolImpl> {
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
