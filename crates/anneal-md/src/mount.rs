//! Named Markdown corpora retain their repository boundary before joining the host facts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anneal_core::runtime::{CallArg, Expr, Literal, Statement, parse_program};
use anneal_core::runtime::{Declaration, SourceMount};
use anneal_core::{
    ConfigEntry, ConfigFacts, FactBatch, FactIdentity, HandleId, MetaFact, MetaRole, NativeId,
    OriginUri, RepositoryContext, RepositoryOperation, RepositorySnapshot, SourceContext,
};
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::extract::adapter::{
    MarkdownConfig, MarkdownExtractionOptions, extract_markdown_facts_with_markdown_config,
};

/// A declaration error is detected before any repository is read.
#[derive(Debug)]
pub(crate) enum MountConfigError {
    Alias { alias: String, reason: &'static str },
    Declaration(String),
}
impl fmt::Display for MountConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Alias { alias, reason } => {
                write!(f, "invalid Markdown mount alias {alias:?}: {reason}")
            }
            Self::Declaration(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for MountConfigError {}

#[derive(Debug)]
pub(crate) struct Mount {
    name: String,
    path: Utf8PathBuf,
    revision: Option<String>,
    roots: Vec<Utf8PathBuf>,
    link_prefixes: Vec<LinkPrefix>,
}

#[derive(Clone, Debug)]
struct LinkPrefix {
    exposed: String,
    native: String,
}

fn configured_path(
    path: &Utf8Path,
    boundary: &Utf8Path,
    home: Option<&Utf8Path>,
) -> Result<Utf8PathBuf> {
    if let Some(relative) = path.as_str().strip_prefix("~/") {
        let home = home
            .filter(|home| home.is_absolute())
            .context("mount ~/ path requires HOME to be set to an absolute path")?;
        return Ok(home.join(relative));
    }
    if path.as_str().starts_with('~') {
        anyhow::bail!("mount path supports only a leading ~/; ~user paths are not supported");
    }
    Ok(boundary.join(path))
}

/// Keep a pin's inventory and temporary selected files alive until cross-mount resolution ends.
pub(crate) struct MountRead {
    name: String,
    root: Utf8PathBuf,
    corpus_root: Utf8PathBuf,
    repository_root: Utf8PathBuf,
    snapshot: Option<RepositorySnapshot>,
    repository: RepositoryContext,
    foreign_present: bool,
    link_prefixes: Vec<LinkPrefix>,
    _temporary: Option<tempfile::TempDir>,
}

fn materialize(
    repository: &RepositoryContext,
    snapshot: &RepositorySnapshot,
    root: &Utf8Path,
    paths: &[String],
) -> Result<()> {
    for (path, bytes) in repository
        .read_snapshot_files(snapshot, paths)
        .map_err(anyhow::Error::msg)?
    {
        let destination = root.join(path);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, bytes)?;
    }
    Ok(())
}

pub(crate) fn is_reference(value: &str, aliases: &[String]) -> bool {
    value.split_once(':').is_some_and(|(alias, path)| {
        !path.starts_with("//") && aliases.iter().any(|name| name == alias)
    })
}

fn strings(declaration: &Declaration) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for arg in &declaration.args {
        let CallArg::Positional {
            expr: Expr::Literal(literal),
            ..
        } = arg
        else {
            return Err(MountConfigError::Declaration(
                "mount settings require literal strings".into(),
            )
            .into());
        };
        match literal {
            Literal::String(value) => values.push(value.clone()),
            Literal::List(items) => {
                for item in items {
                    let Literal::String(value) = item else {
                        anyhow::bail!("mount lists require strings");
                    };
                    values.push(value.clone());
                }
            }
            _ => anyhow::bail!("mount settings require strings"),
        }
    }
    Ok(values)
}

pub(crate) fn from_facts(facts: &ConfigFacts, host: &str) -> Result<Vec<Mount>> {
    let reserved = [
        "file",
        "label",
        "version",
        "section",
        "external",
        "asset",
        "code",
        "snapshot",
        "edge",
        "cli",
        "ssh",
        "ftp",
        "ftps",
        "sftp",
        "tel",
        "urn",
        "data",
        "qmd",
        "http",
        "https",
        "mailto",
        "news",
        "git",
        "javascript",
        "anneal",
        "elixir",
        "rust",
        "m",
        "c",
        "t",
    ];
    let mut names = BTreeSet::new();
    let mut mounts = Vec::new();
    for value in facts.values("md.mount") {
        let block: SourceMount =
            serde_json::from_str(value).context("invalid md.mount declaration")?;
        let name = block.name;
        let reason = if reserved
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        {
            Some("reserved handle family or URI scheme")
        } else if name.eq_ignore_ascii_case(host) {
            Some("equals the host corpus prefix")
        } else if !name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
        {
            Some("use lowercase ASCII letters, digits, '-' or '_', starting with a letter")
        } else if !names.insert(name.clone()) {
            Some("duplicate alias")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(MountConfigError::Alias {
                alias: name,
                reason,
            }
            .into());
        }
        let mut path = None;
        let mut revision = None;
        let mut roots = Vec::new();
        let mut link_prefixes = Vec::new();
        for declaration in block.declarations {
            let values = strings(&declaration)?;
            match declaration.name.as_str() {
                "path" | "revision" => {
                    let [value]: [String; 1] = values.try_into().map_err(|_| {
                        MountConfigError::Declaration(
                            "mount path/revision requires exactly one string".into(),
                        )
                    })?;
                    if value.is_empty() {
                        anyhow::bail!("mount path/revision cannot be empty");
                    }
                    let slot = if declaration.name.as_str() == "path" {
                        &mut path
                    } else {
                        &mut revision
                    };
                    if slot.replace(value).is_some() {
                        anyhow::bail!("duplicate mount {}", declaration.name);
                    }
                }
                "link_prefix" => {
                    let [exposed, native]: [String; 2] = values.try_into().map_err(|_| {
                        MountConfigError::Declaration(
                            "link_prefix requires exposed and native string prefixes".into(),
                        )
                    })?;
                    if !exposed.ends_with('/') || exposed.contains(':') {
                        anyhow::bail!(
                            "link_prefix exposed prefix must be a relative directory prefix ending in /, not a URI"
                        );
                    }
                    let exposed = anneal_core::normalize_relative_path(&exposed, anneal_core::RelativePathPolicy::STRICT_NON_EMPTY)
                        .filter(|path| path.as_str() != ".")
                        .context("link_prefix exposed prefix cannot be absolute, empty or contain parent traversal")?;
                    if native.contains("://")
                        || crate::extract::extraction::is_external_uri(&native)
                    {
                        anyhow::bail!(
                            "link_prefix native prefix must belong to its own mount, not a URI or another alias"
                        );
                    }
                    let normalized = anneal_core::normalize_relative_path(
                        &native,
                        anneal_core::RelativePathPolicy::ALLOW_EMPTY,
                    )
                    .context(
                        "link_prefix native prefix cannot be absolute or contain parent traversal",
                    )?;
                    let native = if native.ends_with('/') && !normalized.as_str().is_empty() {
                        format!("{normalized}/")
                    } else {
                        normalized.to_string()
                    };
                    link_prefixes.push(LinkPrefix {
                        exposed: format!("{exposed}/"),
                        native,
                    });
                }
                "scan_root" => {
                    if values.is_empty() {
                        anyhow::bail!("mount scan_root requires at least one path");
                    }
                    for value in values {
                        let root = anneal_core::normalize_relative_path(
                            &value,
                            anneal_core::RelativePathPolicy::ALLOW_EMPTY,
                        )
                        .ok_or_else(|| {
                            MountConfigError::Declaration(format!(
                                "mount scan_root must stay inside its repository: {value:?}"
                            ))
                        })?;
                        roots.push(if root.as_str().is_empty() {
                            Utf8PathBuf::from(".")
                        } else {
                            root
                        });
                    }
                }
                other => {
                    return Err(MountConfigError::Declaration(format!(
                        "unknown mount setting {other}"
                    ))
                    .into());
                }
            }
        }
        let path = Utf8PathBuf::from(path.ok_or_else(|| {
            MountConfigError::Declaration(format!("mount {name} requires path(...)"))
        })?);
        if path.as_str().starts_with('~') && !path.as_str().starts_with("~/") {
            anyhow::bail!("mount path supports only a leading ~/; ~user paths are not supported");
        }
        if let Some(revision) = &revision
            && (!(4..=64).contains(&revision.len())
                || !revision.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            anyhow::bail!(
                "mount revision must be a Git commit id, not a symbolic ref or jj change id"
            );
        }
        mounts.push(Mount {
            name,
            path,
            revision,
            roots,
            link_prefixes,
        });
    }
    let mut prefixes: Vec<&str> = Vec::new();
    for mount in &mounts {
        for prefix in &mount.link_prefixes {
            if names.contains(
                prefix
                    .exposed
                    .split('/')
                    .next()
                    .expect("validated nonempty prefix"),
            ) {
                anyhow::bail!("link_prefix exposed prefix collides with a declared mount alias");
            }
            if prefix
                .native
                .split_once(':')
                .is_some_and(|(alias, _)| names.contains(alias))
            {
                anyhow::bail!("link_prefix native prefix cannot refer to another mount alias");
            }
            if prefixes.iter().any(|previous| {
                previous.starts_with(&prefix.exposed) || prefix.exposed.starts_with(previous)
            }) {
                anyhow::bail!("overlapping link_prefix exposed prefixes are not allowed");
            }
            prefixes.push(&prefix.exposed);
        }
    }
    mounts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(mounts)
}

pub(crate) fn names(mounts: &[Mount]) -> Vec<String> {
    mounts.iter().map(|mount| mount.name.clone()).collect()
}

fn foreign_config(
    root: &Utf8Path,
    boundary: &Utf8Path,
) -> Result<(MarkdownConfig, MarkdownExtractionOptions, bool)> {
    let file = root.join("anneal.dl");
    let mut entries = Vec::new();
    let present = file.is_file();
    if present {
        let canonical = file.canonicalize_utf8()?;
        if !canonical.starts_with(boundary) {
            anyhow::bail!("foreign anneal.dl resolves outside the mount repository");
        }
        let content = std::fs::read_to_string(&canonical)?;
        let program = parse_program(file.as_str(), &content)?;
        for statement in program.statements {
            if let Statement::SourceBlock(block) = statement
                && block.source.as_str() == "md"
            {
                for declaration in block.declarations {
                    let key = declaration.name.as_str();
                    if matches!(
                        key,
                        "scan_root"
                            | "external_root"
                            | "scan_exclude"
                            | "file_extension"
                            | "linear_namespace"
                            | "label_pattern"
                            | "version_pattern"
                            | "section_min_depth"
                            | "section_max_depth"
                    ) {
                        for value in strings(&declaration)? {
                            entries.push(ConfigEntry::scalar(format!("md.{key}"), value));
                        }
                    }
                }
            }
        }
    }
    let facts = ConfigFacts::try_from_entries(entries)?;
    let discovery =
        crate::MarkdownDiscoveryConfig::from_facts(&facts).map_err(|e| anyhow::anyhow!(e))?;
    Ok((
        MarkdownConfig::from_runtime_facts(&ConfigFacts::default())?,
        discovery.options,
        present,
    ))
}

impl Mount {
    pub(crate) fn extract(
        &self,
        host_root: &Utf8Path,
        cx: &SourceContext<'_>,
        aliases: &[String],
    ) -> Result<(FactBatch, MountRead)> {
        let boundary = anneal_core::enclosing_project_root(host_root)
            .unwrap_or_else(|| host_root.to_path_buf());
        let home = std::env::var("HOME").ok().map(Utf8PathBuf::from);
        let repository_root = configured_path(&self.path, &boundary, home.as_deref())?
            .canonicalize_utf8()
            .context("cannot resolve mount repository path")?;
        let repository = RepositoryContext::discover(&repository_root);
        let actual_root = repository
            .jj_workspace_root()
            .or_else(|| repository.direct_git_root(RepositoryOperation::ChangeHistory));
        if actual_root != Some(repository_root.as_path()) {
            anyhow::bail!(
                "mount {} path must name a validated Git or jj repository root",
                self.name
            );
        }
        let snapshot = self
            .revision
            .as_deref()
            .map(|revision| {
                repository
                    .read_snapshot(revision)
                    .map_err(anyhow::Error::msg)
            })
            .transpose()?;
        let temporary = if snapshot.is_some() {
            Some(tempfile::Builder::new().prefix("anneal-mount-").tempdir()?)
        } else {
            None
        };
        let root = if let Some(temporary) = &temporary {
            Utf8Path::from_path(temporary.path())
                .context("non-UTF-8 snapshot directory")?
                .canonicalize_utf8()?
        } else {
            repository_root.clone()
        };
        if let Some(snapshot) = &snapshot {
            // Reproduce native root discovery from the immutable inventory, not current disk.
            for child in [".design", "docs"] {
                if snapshot
                    .files
                    .keys()
                    .any(|path| path.starts_with(&format!("{child}/")))
                {
                    std::fs::create_dir_all(root.join(child))?;
                }
            }
            materialize(
                &repository,
                snapshot,
                &root,
                &[
                    "anneal.dl".into(),
                    ".design/anneal.dl".into(),
                    "docs/anneal.dl".into(),
                ],
            )?;
        }
        let corpus_root = anneal_core::infer_corpus_root(&root)
            .into_path()
            .canonicalize_utf8()?;
        if !corpus_root.starts_with(&root) {
            anyhow::bail!("native corpus root resolves outside the mount repository");
        }
        let corpus_prefix = corpus_root.strip_prefix(&root)?;
        let commit = snapshot.as_ref().map(|tree| tree.commit.as_str());
        let (config, mut options, foreign_present) = foreign_config(&corpus_root, &root)?;
        let mut selection = if self.roots.is_empty() {
            let roots = if options.scan_roots.is_empty() {
                vec![Utf8PathBuf::from(".")]
            } else {
                options.scan_roots.clone()
            };
            roots
                .iter()
                .chain(&options.external_roots)
                .map(|path| {
                    anneal_core::normalize_path_inside_root(&root, &corpus_root.join(path))
                        .ok_or_else(|| anyhow::anyhow!("foreign scan root escapes its repository"))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            self.roots.clone()
        };
        if let Some(snapshot) = &snapshot {
            let mut parsing_config = config.config.clone();
            parsing_config
                .exclude
                .extend(options.exclude.iter().cloned());
            let paths = crate::extract::parse::snapshot_markdown_paths(
                snapshot.files.keys(),
                &parsing_config,
                &selection,
                corpus_prefix,
            );
            for scan_root in &selection {
                if snapshot
                    .files
                    .keys()
                    .any(|path| scan_root == "." || Utf8Path::new(path).starts_with(scan_root))
                {
                    std::fs::create_dir_all(root.join(scan_root))?;
                }
            }
            materialize(&repository, snapshot, &root, &paths)?;
            // Resolve only referenced assets/code outside the Markdown selection, without
            // fetching unrelated repository payloads. The normal adapter still owns resolution.
            let parsed = crate::extract::parse::build_graph_for_mount(
                &corpus_root,
                &root,
                &parsing_config,
                &selection,
            )?;
            let mut needed = BTreeSet::new();
            for reference in &parsed.files.code_refs {
                needed.insert(reference.path.clone());
                needed.insert(corpus_prefix.join(&reference.path).to_string());
            }
            for edge in &parsed.resolution.pending_edges {
                needed.insert(edge.target_identity.clone());
                needed.insert(corpus_prefix.join(&edge.target_identity).to_string());
                if let Some(file) = parsed.graph.node(edge.source).file_path.as_ref()
                    && let Some(parent) = file.parent()
                {
                    let source_origin = parsed.files.file_origins.get(file.as_str());
                    let parent = source_origin
                        .and_then(|origin| origin.parent())
                        .unwrap_or(parent);
                    if let Ok(relative) = parent.join(&edge.target_identity).strip_prefix(&root) {
                        needed.insert(relative.to_string());
                    }
                }
            }
            let paths = needed
                .into_iter()
                .filter_map(|path| {
                    anneal_core::normalize_relative_path(
                        &path,
                        anneal_core::RelativePathPolicy::ALLOW_EMPTY,
                    )
                    .map(|path| path.to_string())
                })
                .filter(|path| snapshot.files.contains_key(path) && !root.join(path).is_file())
                .collect::<Vec<_>>();
            materialize(&repository, snapshot, &root, &paths)?;
        }
        for scan_root in &mut selection {
            let canonical = root
                .join(&*scan_root)
                .canonicalize_utf8()
                .with_context(|| {
                    format!("mount {} scan root {scan_root} does not exist", self.name)
                })?;
            if !canonical.starts_with(&root) || !canonical.is_dir() {
                anyhow::bail!(
                    "mount {} scan root escapes its repository or is not a directory",
                    self.name
                );
            }
            *scan_root = canonical.strip_prefix(&root)?.to_path_buf();
        }
        if self.roots.is_empty() {
            crate::extract::adapter::resolve_external_scan_roots_in_project(
                &corpus_root,
                &options.external_roots,
                Some(&root),
            )?;
        }
        options.mount_names = aliases.to_vec();
        options.mounted_selection = Some((root.clone(), selection));
        options.probe_code_target_history = commit.is_none() && cx.probe_code_target_history;
        options.probe_edge_assertions = commit.is_none() && cx.probe_edge_assertions;
        // Mount changes are independent of the host pin. Never persist mount drift in the host cache.
        options.read_code_drift_evidence = false;
        options.refresh_code_drift_evidence = false;
        if commit.is_none() {
            options.repository = Some(repository.clone());
        }
        let mut batch = extract_markdown_facts_with_markdown_config(
            &corpus_root,
            cx.corpus.clone(),
            anneal_core::SourceName::from("markdown"),
            cx.next_generation(),
            &config,
            &options,
        )?;
        self.qualify(&mut batch, aliases, &root, &repository_root, commit);
        let read = MountRead {
            name: self.name.clone(),
            root,
            corpus_root,
            repository_root,
            snapshot,
            repository,
            foreign_present,
            link_prefixes: self.link_prefixes.clone(),
            _temporary: temporary,
        };
        Ok((batch, read))
    }

    fn qualify(
        &self,
        batch: &mut FactBatch,
        aliases: &[String],
        root: &Utf8Path,
        repository: &Utf8Path,
        commit: Option<&str>,
    ) {
        let qualify = |value: &str| -> String {
            if value.is_empty()
                || value.starts_with("section:")
                || value
                    .split_once(':')
                    .is_some_and(|(alias, _)| aliases.iter().any(|name| name == alias))
            {
                value.to_string()
            } else {
                format!("{}:{value}", self.name)
            }
        };
        let identity = |identity: &mut FactIdentity| {
            identity.native_id =
                NativeId::from(format!("{}:{}", self.name, identity.native_id.as_str()));
            if let Some(commit) = commit
                && let Some(path) = identity
                    .origin_uri
                    .as_str()
                    .strip_prefix(&format!("file://{root}/"))
            {
                identity.origin_uri =
                    OriginUri::from(format!("git+file://{repository}?rev={commit}#{path}"));
            }
        };
        for handle in &mut batch.handles {
            identity(&mut handle.identity);
            handle.id = qualified_handle(qualify(handle.id.as_str()));
            handle.file = qualify(&handle.file);
        }
        for edge in &mut batch.edges {
            identity(&mut edge.identity);
            edge.from = qualified_handle(qualify(edge.from.as_str()));
            edge.to = qualified_handle(qualify(edge.to.as_str()));
            edge.file = qualify(&edge.file);
        }
        for content in &mut batch.content {
            identity(&mut content.identity);
            content.handle = qualified_handle(qualify(content.handle.as_str()));
            content.span_id = qualify(&content.span_id);
        }
        for span in &mut batch.spans {
            identity(&mut span.identity);
            span.handle = qualified_handle(qualify(span.handle.as_str()));
            span.id = qualify(&span.id);
        }
        for meta in &mut batch.meta {
            identity(&mut meta.identity);
            meta.handle = qualified_handle(qualify(meta.handle.as_str()));
            if matches!(meta.key.as_str(), "md.resolved_file" | "md.parent_dir") {
                meta.value = qualify(&meta.value);
            }
            if let Some(commit) = commit
                && matches!(
                    meta.key.as_str(),
                    "target_probe_base" | "target_resolved_path"
                )
                && let Ok(path) = Utf8Path::new(&meta.value).strip_prefix(root)
            {
                meta.value = format!("git+file://{repository}?rev={commit}#{path}");
            }
        }
        for concern in &mut batch.concerns {
            identity(&mut concern.identity);
            concern.member = qualified_handle(qualify(concern.member.as_str()));
        }
    }
}

fn qualified_handle(value: String) -> HandleId {
    HandleId::new(value).expect("a mount prefix preserves a nonempty validated handle")
}

impl MountRead {
    pub(crate) fn annotate(&self, batch: &mut FactBatch) {
        let repository_root = &self.repository_root;
        let repository = &self.repository;
        let commit = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.commit.as_str());
        let foreign_present = self.foreign_present;
        let prefix = format!("{}:", self.name);
        for handle in batch
            .handles
            .iter()
            .filter(|handle| handle.id.as_str().starts_with(&prefix))
        {
            let mut values = vec![
                ("md.mount".to_string(), self.name.clone()),
                ("md.mount.repository".into(), repository_root.to_string()),
                (
                    "md.mount.mode".into(),
                    if commit.is_some() {
                        "revision"
                    } else {
                        "workspace"
                    }
                    .into(),
                ),
            ];
            if let Some(commit) = commit {
                values.push(("md.mount.commit".into(), commit.into()));
            }
            if let Some(snapshot) = &self.snapshot
                && snapshot.skipped_non_utf8_paths > 0
            {
                values.push((
                    "md.mount.skipped_non_utf8_paths".into(),
                    snapshot.skipped_non_utf8_paths.to_string(),
                ));
            }
            if foreign_present {
                values.push(("md.mount.config_ignored".into(), "foreign rules, runtime policies, prelude extensions and nested mounts; only source md scan_root, external_root, scan_exclude, file_extension, linear_namespace, label_pattern, version_pattern, section_min_depth, section_max_depth are read; explicit mount scan_root overrides foreign scan_root".into()));
            }
            for (operation, key) in [
                (RepositoryOperation::ChangeHistory, "change_history"),
                (RepositoryOperation::AssertionBlame, "assertion_blame"),
                (RepositoryOperation::TargetHistory, "target_history"),
                (RepositoryOperation::IgnoreIndex, "ignore_index"),
                (RepositoryOperation::VersionTags, "version_tags"),
            ] {
                let available = commit.is_none() && repository.operation_available(operation);
                values.push((format!("md.mount.{key}.available"), available.to_string()));
                values.push((
                    format!("md.mount.{key}.reason"),
                    if commit.is_some() {
                        "pinned-mount-history-unavailable"
                    } else {
                        repository.operation_reason(operation)
                    }
                    .into(),
                ));
            }
            for (key, value) in values {
                batch.meta.push(MetaFact {
                    identity: handle.identity.clone(),
                    handle: handle.id.clone(),
                    key,
                    value,
                    role: MetaRole::Derived,
                });
            }
        }
    }
}

impl MountRead {
    pub(crate) fn times(&mut self, batch: &FactBatch) -> BTreeMap<String, String> {
        if self.snapshot.is_some() {
            return BTreeMap::new();
        }
        let prefix = format!("{}:", self.name);
        let files: BTreeMap<_, _> = batch
            .handles
            .iter()
            .filter_map(|handle| {
                let logical = handle.file.strip_prefix(&prefix)?;
                let physical = Utf8Path::new(
                    handle
                        .identity
                        .origin_uri
                        .as_str()
                        .strip_prefix("file://")?,
                );
                let relative = physical.strip_prefix(&self.root).ok()?;
                (!logical.is_empty()).then(|| (relative.to_string(), logical.to_string()))
            })
            .collect();
        let times = if self.repository.is_jj_workspace() {
            files
                .keys()
                .filter_map(|file| {
                    self.repository
                        .jj_file_time(&self.root.join(file))
                        .map(|time| (file.clone(), time))
                })
                .collect()
        } else {
            crate::git_file_times(
                &self.root,
                &mut self.repository,
                files.keys().map(String::as_str),
            )
        };
        times
            .into_iter()
            .filter_map(|(file, time)| {
                files
                    .get(&file)
                    .map(|logical| (format!("{prefix}{logical}"), time))
            })
            .collect()
    }

    pub(crate) fn finish(&self) -> Result<()> {
        if self.snapshot.is_none()
            && self.repository.jj_pin().is_some()
            && !self.repository.finish_jj_generation()
        {
            anyhow::bail!(
                "mount {} recorded jj pin moved during generation",
                self.name
            );
        }
        Ok(())
    }

    pub(crate) fn resolve_assets(&self, batch: &mut FactBatch) -> Result<()> {
        let count = batch.edges.len();
        self.resolve_assets_for_edges(batch, 0..count)
    }

    fn resolve_assets_for_edges(
        &self,
        batch: &mut FactBatch,
        indices: impl Iterator<Item = usize> + Clone,
    ) -> Result<()> {
        let prefix = format!("{}:", self.name);
        let requested: BTreeSet<_> = indices
            .clone()
            .filter_map(|index| batch.edges[index].to.as_str().strip_prefix(&prefix))
            .filter_map(|path| {
                anneal_core::normalize_relative_path(
                    path,
                    anneal_core::RelativePathPolicy::ALLOW_EMPTY,
                )
            })
            .filter(|path| {
                !path
                    .components()
                    .any(|component| matches!(component.as_str(), ".git" | ".jj"))
            })
            .map(|path| path.to_string())
            .collect();
        if let Some(snapshot) = &self.snapshot {
            let paths = requested
                .iter()
                .filter_map(|path| {
                    let primary = self
                        .corpus_root
                        .join(path)
                        .strip_prefix(&self.root)
                        .ok()?
                        .to_string();
                    let selected = if snapshot.files.contains_key(&primary) {
                        primary
                    } else {
                        path.clone()
                    };
                    (snapshot.files.contains_key(&selected) && !self.root.join(&selected).is_file())
                        .then_some(selected)
                })
                .collect::<Vec<_>>();
            materialize(&self.repository, snapshot, &self.root, &paths)?;
        }
        let mut known: BTreeSet<_> = batch
            .handles
            .iter()
            .map(|handle| handle.id.as_str().to_string())
            .collect();
        let mut changed = BTreeSet::new();
        for index in indices {
            let original = batch.edges[index].to.as_str();
            if known.contains(original) {
                continue;
            }
            let Some(path) = original.strip_prefix(&prefix) else {
                continue;
            };
            let Some(path) = anneal_core::normalize_relative_path(
                path,
                anneal_core::RelativePathPolicy::ALLOW_EMPTY,
            ) else {
                continue;
            };
            if !requested.contains(path.as_str()) {
                continue;
            }
            // Pin existence comes from the regular-blob inventory, never current disk.
            let physical = [self.corpus_root.join(&path), self.root.join(&path)]
                .into_iter()
                .find_map(|candidate| {
                    if self.snapshot.as_ref().is_some_and(|snapshot| {
                        candidate
                            .strip_prefix(&self.root)
                            .ok()
                            .is_none_or(|relative| !snapshot.files.contains_key(relative.as_str()))
                    }) {
                        return None;
                    }
                    let physical = candidate.canonicalize_utf8().ok()?;
                    (physical.is_file() && physical.starts_with(&self.root)).then_some(physical)
                });
            let Some(physical) = physical else {
                continue;
            };
            let logical = physical
                .strip_prefix(&self.corpus_root)
                .or_else(|_| physical.strip_prefix(&self.root))?
                .to_string();
            // Nested repositories are separate owners, including submodule worktrees.
            let mut parent = physical.parent();
            let mut nested = false;
            while let Some(directory) = parent.filter(|directory| *directory != self.root) {
                if directory.join(".git").exists() || directory.join(".jj").exists() {
                    nested = true;
                    break;
                }
                parent = directory.parent();
            }
            if nested {
                continue;
            }
            let document = format!("{}:{logical}", self.name);
            let target = if known.contains(&document) {
                document
            } else {
                let id = format!("{}:external:asset:{logical}", self.name);
                if !known.contains(&id) {
                    let bytes = std::fs::read(&physical)?;
                    let origin = if let Some(snapshot) = &self.snapshot {
                        format!(
                            "git+file://{}?rev={}#{}",
                            self.repository_root,
                            snapshot.commit,
                            physical.strip_prefix(&self.root)?
                        )
                    } else {
                        format!("file://{physical}")
                    };
                    let identity = FactIdentity::new(
                        batch.corpus.clone(),
                        batch.source.clone(),
                        NativeId::from(format!("{}:{logical}", self.name)),
                        OriginUri::from(origin.clone()),
                        anneal_core::Revision::from(format!(
                            "{:016x}",
                            anneal_core::fnv1a_64(&bytes)
                        )),
                        batch.generation,
                    );
                    let handle = qualified_handle(id.clone());
                    batch.handles.push(anneal_core::HandleFact {
                        identity: identity.clone(),
                        id: handle.clone(),
                        kind: "external".into(),
                        status: None,
                        namespace: String::new(),
                        file: document,
                        line: 0,
                        date: None,
                        area: crate::extract::area::area_of(&logical).to_string(),
                        summary: String::new(),
                    });
                    for (key, value) in [
                        ("external_class", "asset".to_string()),
                        ("target_path", logical.clone()),
                        ("target_exists", "true".into()),
                        (
                            "target_resolved_path",
                            if self.snapshot.is_some() {
                                origin
                            } else {
                                physical.to_string()
                            },
                        ),
                    ] {
                        batch.meta.push(MetaFact {
                            identity: identity.clone(),
                            handle: handle.clone(),
                            key: key.into(),
                            value,
                            role: MetaRole::Derived,
                        });
                    }
                    known.insert(id.clone());
                }
                id
            };
            let edge = &mut batch.edges[index];
            edge.to = qualified_handle(target);
            changed.insert(edge_key(edge));
        }
        rekey_edges(batch, &changed)
    }
}

type EdgeKey = (String, String, String, u32);

fn edge_key(edge: &anneal_core::EdgeFact) -> EdgeKey {
    (
        edge.from.as_str().to_string(),
        edge.kind.clone(),
        edge.to.as_str().to_string(),
        edge.line,
    )
}

fn rekey_edges(batch: &mut FactBatch, changed: &BTreeSet<EdgeKey>) -> Result<()> {
    let mut occurrences = BTreeMap::new();
    for edge in &mut batch.edges {
        let key = edge_key(edge);
        if !changed.contains(&key) {
            continue;
        }
        let occurrence = occurrences.entry(key).or_insert(0usize);
        let raw = edge.identity.native_id.as_str();
        let owner = if raw.starts_with("edge:") {
            String::new()
        } else if let Some((owner, _)) = raw.split_once(":edge:") {
            format!("{owner}:")
        } else {
            anyhow::bail!("Markdown mount encountered a non-Markdown edge identity");
        };
        edge.identity.native_id = NativeId::from(format!(
            "{owner}{}",
            anneal_core::encode_native_id(
                "edge",
                &(
                    edge.from.as_str(),
                    edge.kind.as_str(),
                    edge.to.as_str(),
                    edge.line,
                    *occurrence
                )
            )?
        ));
        *occurrence += 1;
    }
    Ok(())
}

/// Retry unresolved host attempts once; neither imported references nor misses are rewritten.
pub(crate) fn resolve_link_prefixes(
    host_root: &Utf8Path,
    host_edge_count: usize,
    reads: &[MountRead],
    batch: &mut FactBatch,
) -> Result<()> {
    if reads.iter().all(|read| read.link_prefixes.is_empty()) {
        return Ok(());
    }
    let known: BTreeSet<_> = batch
        .handles
        .iter()
        .map(|handle| handle.id.clone())
        .collect();
    let mut attempts = Vec::new();
    let mut asset_attempts: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, edge) in batch.edges[..host_edge_count].iter_mut().enumerate() {
        if known.contains(&edge.to) || Utf8Path::new(edge.to.as_str()).is_absolute() {
            continue;
        }
        let direct =
            anneal_core::normalize_path_inside_root(host_root, &host_root.join(edge.to.as_str()));
        let relative = crate::extract::resolve::resolve_file_path(
            edge.to.as_str(),
            Utf8Path::new(&edge.file),
            host_root,
        );
        let target = direct.iter().chain(relative.iter()).find_map(|logical| {
            reads.iter().enumerate().find_map(|(read_index, read)| {
                read.link_prefixes.iter().find_map(|prefix| {
                    logical
                        .as_str()
                        .strip_prefix(&prefix.exposed)
                        .map(|suffix| {
                            (
                                read_index,
                                format!("{}:{}{suffix}", read.name, prefix.native),
                            )
                        })
                })
            })
        });
        if let Some((read_index, target)) = target {
            attempts.push((index, edge.to.clone()));
            edge.to = qualified_handle(target);
            if !known.contains(&edge.to) {
                asset_attempts.entry(read_index).or_default().push(index);
            }
        }
    }
    if attempts.is_empty() {
        return Ok(());
    }
    for (read_index, indices) in asset_attempts {
        reads[read_index].resolve_assets_for_edges(batch, indices.into_iter())?;
    }
    let known: BTreeSet<_> = batch
        .handles
        .iter()
        .map(|handle| handle.id.clone())
        .collect();
    let mut changed = BTreeSet::new();
    for (index, original) in attempts {
        let edge = &mut batch.edges[index];
        if known.contains(&edge.to) {
            changed.insert(edge_key(edge));
        } else {
            edge.to = original;
        }
    }
    rekey_edges(batch, &changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_path_forms_are_explicit_and_bounded_by_home() {
        let boundary = Utf8Path::new("/desk/host");
        let home = Some(Utf8Path::new("/home/user"));
        for (value, expected) in [
            ("../foreign", "/desk/host/../foreign"),
            ("/repos/foreign", "/repos/foreign"),
            ("~/code/foreign", "/home/user/code/foreign"),
            ("$REPO/*", "/desk/host/$REPO/*"),
        ] {
            assert_eq!(
                configured_path(Utf8Path::new(value), boundary, home).unwrap(),
                Utf8Path::new(expected)
            );
        }
        for value in ["~", "~someone/code"] {
            assert!(configured_path(Utf8Path::new(value), boundary, home).is_err());
        }
        assert!(configured_path(Utf8Path::new("~/code"), boundary, None).is_err());
        assert!(
            configured_path(
                Utf8Path::new("~/code"),
                boundary,
                Some(Utf8Path::new("relative/home"))
            )
            .is_err()
        );
    }
}
