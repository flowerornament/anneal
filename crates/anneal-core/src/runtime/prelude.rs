//! Embedded prelude declarations that the runtime exposes to surfaces.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Serialize;

use super::ast::{Program, Statement};
use super::parser::{ParseError, parse_prelude_program};
use crate::hash::Fnv1a64;

pub const ANNEAL_PRELUDE_PATH_ENV: &str = "ANNEAL_PRELUDE_PATH";
pub const STANDARD_PRELUDE_VERSION: &str = "v2.0";
pub const CONTEXT_VERB_NAME: &str = "context";
pub const CONTEXT_VERB_DOC: &str = "Find the most relevant handles for a goal, read bounded spans, and include a small neighborhood so a cold agent can localize work in one call.";
pub const CONTEXT_OUTPUT_SCHEMA: &str = r#"{"goal":"String","hits":[{"handle":"HandleId","span_id":"String|null","score":"Number","reason":"String","field":"String"}],"spans":[{"handle":"HandleId","span_id":"String","start_line":"Number","end_line":"Number","tokens":"Number","text":"String"}],"neighborhood":[{"handle":"HandleId","neighbor":"HandleId"}]}"#;
pub const CONTEXT_DEFAULT_ARGS: &[&str] = &["goal", "budget", "neighborhood_depth", "hits"];
pub const CONTEXT_CAPABILITIES: &[&str] = &["read"];
pub const VIEWS_PRELUDE_DOC: &str = "Saved verb declarations and lifecycle profile examples for the runtime surface. Verbs are project-extensible templates over the same Datalog runtime as the prelude.";
pub const GRAPH_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/graph.dl";
pub const CONVERGENCE_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/convergence.dl";
pub const CHECKS_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/checks.dl";
pub const RANKING_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/ranking.dl";
pub const VIEWS_PRELUDE_SOURCE: &str = "crates/anneal-core/src/prelude/views.dl";
pub const GRAPH_PRELUDE: &str = include_str!("../prelude/graph.dl");
pub const CONVERGENCE_PRELUDE: &str = include_str!("../prelude/convergence.dl");
pub const CHECKS_PRELUDE: &str = include_str!("../prelude/checks.dl");
pub const RANKING_PRELUDE: &str = include_str!("../prelude/ranking.dl");
pub const VIEWS_PRELUDE: &str = include_str!("../prelude/views.dl");
static CONTEXT_QUERY_TEMPLATE: LazyLock<String> = LazyLock::new(|| {
    let program = parse_prelude_program(VIEWS_PRELUDE_SOURCE, VIEWS_PRELUDE)
        .expect("checked-in views prelude should parse");
    program
        .statements
        .iter()
        .find_map(|statement| match statement {
            Statement::Verb(verb) if verb.string_arg("name") == Some(CONTEXT_VERB_NAME) => {
                verb.string_arg("query").map(str::to_string)
            }
            _ => None,
        })
        .expect("checked-in views prelude should declare the context verb")
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedPreludeFile {
    pub source_name: &'static str,
    pub contents: &'static str,
}

pub const STANDARD_PRELUDE_FILES: &[EmbeddedPreludeFile] = &[
    EmbeddedPreludeFile {
        source_name: GRAPH_PRELUDE_SOURCE,
        contents: GRAPH_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: CONVERGENCE_PRELUDE_SOURCE,
        contents: CONVERGENCE_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: CHECKS_PRELUDE_SOURCE,
        contents: CHECKS_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: RANKING_PRELUDE_SOURCE,
        contents: RANKING_PRELUDE,
    },
    EmbeddedPreludeFile {
        source_name: VIEWS_PRELUDE_SOURCE,
        contents: VIEWS_PRELUDE,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeSet {
    compatibility: PreludeCompatibility,
    files: Vec<PreludeFile>,
    hash: PreludeHash,
    source_map: PreludeSourceMap,
}

impl PreludeSet {
    pub fn standard() -> Self {
        Self::new(
            PreludeCompatibility::CheckedIn {
                version: STANDARD_PRELUDE_VERSION,
            },
            STANDARD_PRELUDE_FILES
                .iter()
                .map(PreludeFile::from_embedded)
                .collect(),
        )
    }

    fn custom(files: Vec<PreludeFile>) -> Self {
        Self::new(PreludeCompatibility::Custom, files)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, PreludeLoadError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|source| PreludeLoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.is_file() {
            return Ok(Self::custom(vec![PreludeFile::from_disk_file(
                path,
                path.display().to_string(),
                PreludePathKey::single_file(),
            )?]));
        }
        if metadata.is_dir() {
            return Self::from_directory(path);
        }
        Err(PreludeLoadError::UnsupportedPath {
            path: path.to_path_buf(),
        })
    }

    pub fn program(&self) -> Result<Program, ParseError> {
        let mut statements = Vec::new();
        for file in &self.files {
            let program = parse_prelude_program(file.source_name(), file.contents())?;
            reject_unresolved_load_directives(file, &program)?;
            statements.extend(program.statements);
        }
        Ok(Program::new(statements))
    }

    pub fn compatibility(&self) -> PreludeCompatibility {
        self.compatibility
    }

    pub fn version(&self) -> Option<&'static str> {
        self.compatibility.version()
    }

    pub fn files(&self) -> &[PreludeFile] {
        &self.files
    }

    pub fn hash(&self) -> &PreludeHash {
        &self.hash
    }

    pub fn source_map(&self) -> &PreludeSourceMap {
        &self.source_map
    }

    fn new(compatibility: PreludeCompatibility, files: Vec<PreludeFile>) -> Self {
        let hash = PreludeHash::for_files(&files);
        let source_map = PreludeSourceMap::from_files(&files);
        Self {
            compatibility,
            files,
            hash,
            source_map,
        }
    }

    fn from_directory(root: &Path) -> Result<Self, PreludeLoadError> {
        let mut paths = Vec::new();
        collect_prelude_paths(root, root, &mut paths)?;
        if paths.is_empty() {
            return Err(PreludeLoadError::EmptyDirectory {
                path: root.to_path_buf(),
            });
        }
        paths.sort_by(|left, right| left.key.cmp(&right.key));
        let files = paths
            .into_iter()
            .map(|path| {
                PreludeFile::from_disk_file(&path.path, path.key.as_str().to_string(), path.key)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::custom(files))
    }
}

#[derive(Clone, Debug)]
pub struct LoadedPrelude {
    set: PreludeSet,
    program: Program,
}

impl LoadedPrelude {
    pub fn load_active() -> Result<Self, PreludeError> {
        if std::env::var_os(ANNEAL_PRELUDE_PATH_ENV).is_none() {
            return Ok(Self {
                set: standard_prelude_set().clone(),
                program: standard_prelude_program()?,
            });
        }
        let set = active_prelude_set()?;
        let program = set.program()?;
        Ok(Self { set, program })
    }

    pub fn set(&self) -> &PreludeSet {
        &self.set
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn into_program(self) -> Program {
        self.program
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreludeCompatibility {
    CheckedIn { version: &'static str },
    Custom,
}

impl PreludeCompatibility {
    pub fn version(self) -> Option<&'static str> {
        match self {
            Self::CheckedIn { version } => Some(version),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeFile {
    source_name: String,
    hash_key: PreludePathKey,
    contents: String,
}

impl PreludeFile {
    pub fn new(source_name: impl Into<String>, contents: impl Into<String>) -> Self {
        let source_name = source_name.into();
        Self {
            hash_key: PreludePathKey::new(source_name.clone()),
            source_name,
            contents: contents.into(),
        }
    }

    fn with_hash_key(
        source_name: impl Into<String>,
        hash_key: PreludePathKey,
        contents: impl Into<String>,
    ) -> Self {
        Self {
            source_name: source_name.into(),
            hash_key,
            contents: contents.into(),
        }
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn hash_key(&self) -> &str {
        self.hash_key.as_str()
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }

    fn from_embedded(file: &EmbeddedPreludeFile) -> Self {
        Self::new(file.source_name, file.contents)
    }

    fn from_disk_file(
        path: &Path,
        source_name: String,
        hash_key: PreludePathKey,
    ) -> Result<Self, PreludeLoadError> {
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("dl") {
            return Err(PreludeLoadError::UnsupportedPath {
                path: path.to_path_buf(),
            });
        }
        let contents = fs::read_to_string(path).map_err(|source| PreludeLoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self::with_hash_key(source_name, hash_key, contents))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PreludePathKey(String);

impl PreludePathKey {
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    fn single_file() -> Self {
        Self::new("prelude.dl")
    }

    fn from_relative(root: &Path, path: &Path) -> Result<Self, PreludeLoadError> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PreludeLoadError::PathOutsideRoot {
                path: path.to_path_buf(),
                root: root.to_path_buf(),
            })?;
        let mut parts = Vec::new();
        for component in relative.components() {
            let std::path::Component::Normal(part) = component else {
                return Err(PreludeLoadError::UnsupportedPath {
                    path: path.to_path_buf(),
                });
            };
            let Some(part) = part.to_str() else {
                return Err(PreludeLoadError::NonUtf8Path {
                    path: path.to_path_buf(),
                });
            };
            parts.push(part);
        }
        Ok(Self::new(parts.join("/")))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeHash(String);

impl PreludeHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn for_files(files: &[PreludeFile]) -> Self {
        let mut hasher = Fnv1a64::new();
        for file in files {
            hasher = write_hash_part(hasher, file.hash_key().as_bytes());
            hasher = write_hash_part(hasher, file.contents().as_bytes());
        }
        Self(format!("fnv1a64:{:016x}", hasher.finish()))
    }
}

impl std::fmt::Display for PreludeHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeSourceMap {
    files: Vec<PreludeSourceFile>,
}

impl PreludeSourceMap {
    pub fn files(&self) -> &[PreludeSourceFile] {
        &self.files
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn from_files(files: &[PreludeFile]) -> Self {
        Self {
            files: files.iter().map(PreludeSourceFile::from_file).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeSourceFile {
    source_name: String,
    line_count: usize,
}

impl PreludeSourceFile {
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn line_count(&self) -> usize {
        self.line_count
    }

    fn from_file(file: &PreludeFile) -> Self {
        Self {
            source_name: file.source_name().to_string(),
            line_count: file.contents().lines().count(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PreludeLoadError {
    #[error("{path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path:?} is not a .dl file or directory")]
    UnsupportedPath { path: PathBuf },
    #[error("{path:?} contains no .dl files")]
    EmptyDirectory { path: PathBuf },
    #[error("{path:?} is not a UTF-8 prelude path")]
    NonUtf8Path { path: PathBuf },
    #[error("{path:?} is outside prelude root {root:?}")]
    PathOutsideRoot { path: PathBuf, root: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub enum PreludeError {
    #[error(transparent)]
    Load(#[from] PreludeLoadError),
    #[error(transparent)]
    Parse(#[from] ParseError),
}

static STANDARD_PRELUDE_SET: LazyLock<PreludeSet> = LazyLock::new(PreludeSet::standard);
static STANDARD_PRELUDE_PROGRAM: LazyLock<Result<Program, ParseError>> =
    LazyLock::new(parse_standard_prelude_program);

pub fn standard_prelude_set() -> &'static PreludeSet {
    &STANDARD_PRELUDE_SET
}

pub fn active_prelude_set() -> Result<PreludeSet, PreludeLoadError> {
    match std::env::var_os(ANNEAL_PRELUDE_PATH_ENV) {
        Some(path) => PreludeSet::from_path(path),
        None => Ok(standard_prelude_set().clone()),
    }
}

pub fn active_prelude() -> Result<LoadedPrelude, PreludeError> {
    LoadedPrelude::load_active()
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct QueryEchoMeta<'a> {
    pub query: &'a str,
    pub prelude_hash: &'a str,
}

impl<'a> QueryEchoMeta<'a> {
    pub fn new(query: &'a str, prelude: &'a PreludeSet) -> Self {
        Self {
            query,
            prelude_hash: prelude.hash().as_str(),
        }
    }
}

pub fn standard_prelude_program() -> Result<Program, ParseError> {
    match &*STANDARD_PRELUDE_PROGRAM {
        Ok(program) => Ok(program.clone()),
        Err(err) => Err(err.clone()),
    }
}

fn parse_standard_prelude_program() -> Result<Program, ParseError> {
    standard_prelude_set().program()
}

fn write_hash_part(hasher: Fnv1a64, part: &[u8]) -> Fnv1a64 {
    hasher
        .write(part.len().to_string().as_bytes())
        .write(&[0])
        .write(part)
        .write(&[0xff])
}

#[derive(Debug)]
struct PreludePath {
    key: PreludePathKey,
    path: PathBuf,
}

fn collect_prelude_paths(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<PreludePath>,
) -> Result<(), PreludeLoadError> {
    for entry in fs::read_dir(directory).map_err(|source| PreludeLoadError::Io {
        path: directory.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PreludeLoadError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| PreludeLoadError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_prelude_paths(root, &path, paths)?;
        } else if file_type.is_file()
            && path.extension().and_then(std::ffi::OsStr::to_str) == Some("dl")
        {
            paths.push(PreludePath {
                key: PreludePathKey::from_relative(root, &path)?,
                path,
            });
        }
    }
    Ok(())
}

fn reject_unresolved_load_directives(
    file: &PreludeFile,
    program: &Program,
) -> Result<(), ParseError> {
    for statement in &program.statements {
        let (location, directive) = match statement {
            Statement::Include(directive) => (&directive.location, "include"),
            Statement::Import(directive) => (&directive.location, "import"),
            _ => continue,
        };
        return Err(ParseError {
            location: location.clone(),
            message: format!(
                "{directive} is not allowed inside PreludeSet file {:?}; use ANNEAL_PRELUDE_PATH directory package ordering instead",
                file.source_name()
            ),
        });
    }
    Ok(())
}

pub struct ContextQueryArgs<'a> {
    pub goal: &'a str,
    pub hits: usize,
    pub per_hit_read_budget: i64,
    pub neighborhood_depth: i64,
    pub include_low_confidence: bool,
}

pub fn render_context_query(args: &ContextQueryArgs<'_>) -> String {
    render_context_query_terms(
        &datalog_string_literal(args.goal),
        &args.hits.to_string(),
        &args.per_hit_read_budget.to_string(),
        &args.neighborhood_depth.to_string(),
        args.include_low_confidence,
    )
}

pub fn low_confidence_filter(include_low_confidence: bool) -> &'static str {
    if include_low_confidence {
        ""
    } else {
        ",\n        low_confidence = false"
    }
}

pub fn datalog_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn render_context_query_terms(
    goal_term: &str,
    hits_term: &str,
    read_budget_term: &str,
    neighborhood_depth_term: &str,
    include_low_confidence: bool,
) -> String {
    let mut query = CONTEXT_QUERY_TEMPLATE.clone();
    replace_required(
        &mut query,
        "context_goal(goal).",
        &format!("context_goal({goal_term})."),
    );
    replace_required(
        &mut query,
        "context_hits(hits).",
        &format!("context_hits({hits_term})."),
    );
    replace_required(
        &mut query,
        "context_read_budget(per_hit_budget).",
        &format!("context_read_budget({read_budget_term})."),
    );
    replace_required(
        &mut query,
        "context_neighborhood_depth(depth).",
        &format!("context_neighborhood_depth({neighborhood_depth_term})."),
    );
    if include_low_confidence {
        replace_required(&mut query, low_confidence_filter(false), "");
    }
    query
}

fn replace_required(query: &mut String, from: &str, to: &str) {
    let replaced = query.replacen(from, to, 1);
    assert_ne!(replaced, *query, "context query template missing {from:?}");
    *query = replaced;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;

    use crate::facts::{
        ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity, HandleFact,
        MetaFact, SnapshotFact, SpanFact,
    };
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::runtime::QueryOutput;
    use crate::runtime::ast::Statement;
    use crate::runtime::ast::{Expr, Literal, NumberLiteral, Program, RuleLayer};
    use crate::runtime::eval::NumberValue;
    use crate::runtime::{Database, Evaluator, Value, analyze, parse_program};
    use crate::source::{ConfigKey, Pattern, SourceCapabilities, SourceInfo};
    use crate::store::FactStore;

    const REQUIRED_VIEW_VERBS: &[&str] = &[
        "status",
        "handle",
        "find",
        "search",
        CONTEXT_VERB_NAME,
        "read",
        "work",
        "blocked",
        "trend",
        "broken",
        "vocab",
        "schema",
        "predicates",
        "verbs",
        "describe",
        "source-of",
        "examples",
        "sources",
    ];

    #[test]
    fn standard_prelude_file_set_matches_spec_layout() {
        let source_names = STANDARD_PRELUDE_FILES
            .iter()
            .map(|file| file.source_name)
            .collect::<Vec<_>>();
        let prelude = standard_prelude_set();

        assert_eq!(
            source_names,
            vec![
                GRAPH_PRELUDE_SOURCE,
                CONVERGENCE_PRELUDE_SOURCE,
                CHECKS_PRELUDE_SOURCE,
                RANKING_PRELUDE_SOURCE,
                VIEWS_PRELUDE_SOURCE,
            ]
        );
        assert!(STANDARD_PRELUDE_FILES.iter().all(|file| {
            parse_program(file.source_name, file.contents).is_ok()
                && !file.contents.trim().is_empty()
        }));
        assert_eq!(prelude.version(), Some(STANDARD_PRELUDE_VERSION));
        assert_eq!(
            prelude
                .files()
                .iter()
                .map(PreludeFile::source_name)
                .collect::<Vec<_>>(),
            source_names
        );
        assert!(prelude.hash().as_str().starts_with("fnv1a64:"));
        assert_eq!(prelude.hash().as_str().len(), "fnv1a64:".len() + 16);
        assert_eq!(prelude.source_map().files().len(), source_names.len());
        assert!(
            prelude.source_map().files().iter().all(|file| {
                source_names.contains(&file.source_name()) && file.line_count() > 0
            })
        );
        prelude.program().expect("standard PreludeSet parses");
    }

    #[test]
    fn custom_prelude_set_has_deterministic_order_and_custom_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).expect("nested dir");
        std::fs::write(nested.join("z.dl"), r#"z("ok")."#).expect("write z");
        std::fs::write(dir.path().join("a.dl"), r#"a("ok")."#).expect("write a");
        std::fs::write(dir.path().join("ignore.txt"), "ignored").expect("write ignored");

        let prelude = PreludeSet::from_path(dir.path()).expect("custom prelude loads");

        assert_eq!(prelude.compatibility(), PreludeCompatibility::Custom);
        assert_eq!(prelude.version(), None);
        assert_eq!(
            prelude
                .files()
                .iter()
                .map(PreludeFile::source_name)
                .collect::<Vec<_>>(),
            vec!["a.dl", "nested/z.dl"]
        );
        assert_eq!(
            prelude
                .files()
                .iter()
                .map(PreludeFile::hash_key)
                .collect::<Vec<_>>(),
            vec!["a.dl", "nested/z.dl"]
        );
        assert_ne!(prelude.hash(), standard_prelude_set().hash());
        assert_eq!(prelude.source_map().files().len(), 2);
        prelude.program().expect("custom PreludeSet parses");
    }

    #[test]
    fn single_file_custom_prelude_is_the_whole_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("custom.dl");
        std::fs::write(&file, r#"custom("ok")."#).expect("write custom prelude");

        let prelude = PreludeSet::from_path(&file).expect("single file prelude loads");

        assert_eq!(prelude.version(), None);
        assert_eq!(prelude.files().len(), 1);
        assert_eq!(prelude.files()[0].source_name(), file.display().to_string());
        assert_eq!(prelude.files()[0].hash_key(), "prelude.dl");
        prelude.program().expect("single file PreludeSet parses");
    }

    #[test]
    fn single_file_custom_prelude_hash_is_location_independent() {
        let left = tempfile::tempdir().expect("left tempdir");
        let right = tempfile::tempdir().expect("right tempdir");
        let left_file = left.path().join("custom.dl");
        let right_file = right.path().join("renamed.dl");
        std::fs::write(&left_file, r#"custom("ok")."#).expect("write left");
        std::fs::write(&right_file, r#"custom("ok")."#).expect("write right");

        let left_prelude = PreludeSet::from_path(&left_file).expect("left prelude loads");
        let right_prelude = PreludeSet::from_path(&right_file).expect("right prelude loads");

        assert_eq!(left_prelude.hash(), right_prelude.hash());
        assert_ne!(
            left_prelude.files()[0].source_name(),
            right_prelude.files()[0].source_name()
        );
    }

    #[test]
    fn custom_prelude_rejects_unresolved_load_directives() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("custom.dl");
        std::fs::write(&file, r#"include "other.dl"."#).expect("write custom prelude");

        let prelude = PreludeSet::from_path(&file).expect("custom prelude loads");
        let err = prelude.program().expect_err("include is rejected");

        assert!(err.message.contains("include is not allowed"));
    }

    #[test]
    fn query_echo_meta_uses_prelude_set_hash() {
        let prelude = standard_prelude_set();
        let meta = QueryEchoMeta::new("? blocked(h).", prelude);

        assert_eq!(meta.prelude_hash, prelude.hash().as_str());
    }

    #[test]
    fn views_prelude_declares_required_verbs_and_profiles() {
        let program = parse_program(VIEWS_PRELUDE_SOURCE, VIEWS_PRELUDE).expect("views.dl parses");
        let verbs = program
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Verb(verb) => {
                    let name = verb.string_arg("name").expect("@verb has name");
                    Some((name, verb))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            verbs.keys().copied().collect::<BTreeSet<_>>(),
            REQUIRED_VIEW_VERBS.iter().copied().collect::<BTreeSet<_>>()
        );

        for &name in REQUIRED_VIEW_VERBS {
            let verb = verbs.get(name).expect("required verb present");
            let query = verb.string_arg("query").expect("@verb has query");
            let schema = verb
                .string_arg("output_schema")
                .expect("@verb has output_schema");
            serde_json::from_str::<serde_json::Value>(schema)
                .unwrap_or_else(|err| panic!("{name} output_schema should be json: {err}"));

            let executable_query = lower_verb_query(name, query);
            let mut loaded = standard_prelude_program().expect("standard prelude parses");
            loaded.statements.extend(executable_query.statements);
            analyze(loaded).unwrap_or_else(|err| panic!("{name} query should analyze: {err}"));
        }

        let context = verbs.get(CONTEXT_VERB_NAME).expect("context verb present");
        assert_eq!(
            context.string_arg("output_schema"),
            Some(CONTEXT_OUTPUT_SCHEMA)
        );
        assert_eq!(context.string_arg("doc"), Some(CONTEXT_VERB_DOC));

        let verb_rows = evaluate_standard_prelude_queries(
            "? verbs(name, query, doc, output_schema).",
            Database::from_store(&FactStore::default()),
        );
        let introspected_names = verb_rows[0]
            .rows
            .iter()
            .filter_map(|row| match row.fields.get("name") {
                Some(Value::String(name)) => Some(name.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            introspected_names,
            REQUIRED_VIEW_VERBS.iter().copied().collect::<BTreeSet<_>>()
        );

        for profile in [
            "profile_doc_corpus",
            "profile_code_corpus",
            "profile_issue_corpus",
        ] {
            let query = format!("? {profile}(snippet).");
            let outputs = evaluate_standard_prelude_queries(
                &query,
                Database::from_store(&FactStore::default()),
            );
            assert_eq!(outputs[0].rows.len(), 1, "{profile} should be queryable");
            assert!(
                matches!(
                    outputs[0].rows[0].fields.get("snippet"),
                    Some(Value::String(snippet)) if snippet.contains("pipeline_position_for")
                ),
                "{profile} should return a copyable lifecycle snippet"
            );
            let snippet = string_row_field(&outputs[0], "snippet");
            let mut loaded = standard_prelude_program().expect("standard prelude parses");
            let profile_program = parse_program(profile, &snippet)
                .unwrap_or_else(|err| panic!("{profile} snippet should parse: {err}"));
            loaded.statements.extend(profile_program.statements);
            analyze(loaded).unwrap_or_else(|err| panic!("{profile} snippet should analyze: {err}"));
        }
    }

    #[test]
    fn views_prelude_verbs_evaluate_against_declared_schemas() {
        let verbs = views_verb_declarations();
        let database = standard_library_database();

        for &name in REQUIRED_VIEW_VERBS {
            let verb = verbs.get(name).expect("verb is declared");
            let query = verb.string_arg("query").expect("@verb has query");
            let output = evaluate_verb_query(name, query, database.clone());
            assert!(
                !output.rows.is_empty(),
                "{name} should produce at least one fixture row"
            );
            if name == CONTEXT_VERB_NAME {
                assert_output_fields(
                    &output,
                    &[
                        "field",
                        "h",
                        "hit_span_id",
                        "neighbor",
                        "reason",
                        "score",
                        "section",
                        "span_id",
                        "start_line",
                        "end_line",
                        "text",
                        "tokens",
                    ],
                );
            } else {
                assert_output_matches_schema(verb, &output);
            }
        }
    }

    #[test]
    fn trend_verb_returns_empty_without_snapshot_history() {
        let verbs = views_verb_declarations();
        let trend = verbs.get("trend").expect("trend verb is declared");
        let query = trend.string_arg("query").expect("@verb has query");

        let output = evaluate_verb_query(
            "trend",
            query,
            standard_library_database_without_snapshots(),
        );

        assert!(output.rows.is_empty());
    }

    fn views_verb_declarations() -> BTreeMap<String, crate::runtime::ast::VerbDecl> {
        let program = parse_program(VIEWS_PRELUDE_SOURCE, VIEWS_PRELUDE).expect("views.dl parses");
        program
            .statements
            .into_iter()
            .filter_map(|statement| match statement {
                Statement::Verb(verb) => {
                    let name = verb.string_arg("name").expect("@verb has name").to_string();
                    Some((name, verb))
                }
                _ => None,
            })
            .collect()
    }

    fn lower_verb_query(name: &str, query: &str) -> Program {
        let mut program = parse_program(&format!("views.dl:{name}.query"), query)
            .unwrap_or_else(|err| panic!("{name} query should parse: {err}"));
        match name {
            "handle" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("handle_focus", "h", "ticket-1"),
            ),
            "find" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("find_text", "text", "ticket"),
            ),
            "search" => {
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::string("search_query", "query", "ticket"),
                );
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::int("search_limit", "limit", 10),
                );
            }
            CONTEXT_VERB_NAME => {
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::string("context_goal", "goal", "ticket"),
                );
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::int("context_hits", "hits", 3),
                );
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::int("context_read_budget", "per_hit_budget", 2400),
                );
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::int("context_neighborhood_depth", "depth", 1),
                );
            }
            "read" => {
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::string("read_handle", "h", "ticket-1"),
                );
                bind_parameter_fact(
                    &mut program,
                    ParameterBinding::int("read_budget", "budget", 4000),
                );
            }
            "blocked" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("blocked_focus", "h", "ticket-1"),
            ),
            "describe" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("describe_name", "name", "runtime"),
            ),
            "source-of" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("source_of_name", "name", "ranked_work"),
            ),
            "examples" => bind_parameter_fact(
                &mut program,
                ParameterBinding::string("examples_name", "name", "search"),
            ),
            _ => {}
        }
        program
    }

    #[derive(Clone, Debug)]
    struct ParameterBinding {
        predicate: &'static str,
        variable: &'static str,
        value: Literal,
    }

    impl ParameterBinding {
        fn string(predicate: &'static str, variable: &'static str, value: &str) -> Self {
            Self {
                predicate,
                variable,
                value: Literal::String(value.to_string()),
            }
        }

        fn int(predicate: &'static str, variable: &'static str, value: i64) -> Self {
            Self {
                predicate,
                variable,
                value: Literal::Number(NumberLiteral::Int(value)),
            }
        }
    }

    fn bind_parameter_fact(program: &mut Program, binding: ParameterBinding) {
        let ParameterBinding {
            predicate,
            variable,
            value,
        } = binding;
        let mut matched = false;
        for statement in &mut program.statements {
            let Statement::Fact(head) = statement else {
                continue;
            };
            if head.predicate.module.is_some() || head.predicate.name.as_str() != predicate {
                continue;
            }
            assert_eq!(head.terms.len(), 1, "{predicate} arity");
            let Some(expr) = head.terms[0].expr_mut() else {
                panic!("{predicate} parameter fact must bind a variable");
            };
            match expr {
                Expr::Var(var) if var.as_str() == variable => {
                    *expr = Expr::Literal(value.clone());
                    matched = true;
                }
                Expr::Var(var) => panic!("{predicate} expected variable {variable}, found {var}"),
                Expr::Literal(_)
                | Expr::FunctionCall { .. }
                | Expr::Binary { .. }
                | Expr::Tuple(_) => {
                    panic!("{predicate} parameter fact was already lowered")
                }
            }
        }
        assert!(matched, "missing parameter fact {predicate}");
    }

    fn evaluate_verb_query(name: &str, query: &str, database: Database) -> QueryOutput {
        let query_program = lower_verb_query(name, query);
        let mut program = standard_prelude_program().expect("standard prelude parses");
        program.statements.extend(query_program.statements);
        let analyzed = analyze(program).unwrap_or_else(|err| panic!("{name} analyzes: {err}"));
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .unwrap_or_else(|| panic!("{name} should contain a query"));
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator
            .run_fixpoint()
            .unwrap_or_else(|err| panic!("{name} fixpoint runs: {err}"));
        evaluator
            .eval_query(&query)
            .unwrap_or_else(|err| panic!("{name} evaluates: {err}"))
    }

    fn assert_output_matches_schema(verb: &crate::runtime::ast::VerbDecl, output: &QueryOutput) {
        let schema: serde_json::Value = serde_json::from_str(
            verb.string_arg("output_schema")
                .expect("@verb schema is present"),
        )
        .expect("schema parses");
        let fields = schema
            .as_object()
            .expect("simple verb output_schema is an object");
        let expected = fields.keys().map(String::as_str).collect::<Vec<_>>();
        assert_output_fields(output, &expected);

        let row = &output.rows[0];
        for (field, expected_type) in fields {
            let value = row
                .fields
                .get(field)
                .unwrap_or_else(|| panic!("row missing field {field}"));
            assert_schema_type(
                field,
                value,
                expected_type.as_str().expect("type is a string"),
            );
        }
    }

    fn assert_output_fields(output: &QueryOutput, expected: &[&str]) {
        let actual = output.rows[0]
            .fields
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected = expected.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
    }

    fn assert_schema_type(field: &str, value: &Value, expected: &str) {
        match expected {
            "Bool" => assert!(matches!(value, Value::Bool(_)), "{field} should be bool"),
            "HandleId" | "String" => {
                assert!(
                    matches!(value, Value::String(_)),
                    "{field} should be string"
                );
            }
            "Number" => assert!(
                matches!(value, Value::Number(_)),
                "{field} should be numeric"
            ),
            "String|null" => assert!(
                matches!(value, Value::String(_) | Value::Null),
                "{field} should be string or null"
            ),
            "Number|null" => assert!(
                matches!(value, Value::Number(_) | Value::Null),
                "{field} should be numeric or null"
            ),
            "Value" => {}
            "List<String>" => assert!(
                matches!(value, Value::List(values) if values.iter().all(|v| matches!(v, Value::String(_)))),
                "{field} should be a string list"
            ),
            other => panic!("unsupported schema type {other:?} for field {field}"),
        }
    }

    fn string_row_field(output: &QueryOutput, field: &str) -> String {
        match output.rows[0].fields.get(field) {
            Some(Value::String(value)) => value.clone(),
            other => panic!("expected string field {field}, got {other:?}"),
        }
    }

    #[test]
    fn standard_prelude_exposes_source_backed_convergence_doc() {
        let mut program = standard_prelude_program().expect("prelude parses");
        let query = parse_program(
            "describe",
            r#"
            ? describe("convergence", doc).
            ? source_of("convergence", file, lines).
            "#,
        )
        .expect("describe query parses");
        program.statements.extend(query.statements);

        let analyzed = analyze(program).expect("prelude with describe query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&FactStore::default()));
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        assert!(matches!(
            outputs[0].rows[0].fields.get("doc"),
            Some(Value::String(doc)) if doc.contains("potential") && doc.contains("blocked")
        ));
        assert_eq!(
            outputs[1].rows[0].fields.get("file"),
            Some(&Value::String(CONVERGENCE_PRELUDE_SOURCE.to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("lines"),
            Some(&Value::String("3".to_string()))
        );
    }

    #[test]
    fn standard_prelude_exposes_source_backed_topic_docs() {
        let topic_sources = [
            ("graph", GRAPH_PRELUDE_SOURCE),
            ("convergence", CONVERGENCE_PRELUDE_SOURCE),
            ("checks", CHECKS_PRELUDE_SOURCE),
            ("ranking", RANKING_PRELUDE_SOURCE),
            ("views", VIEWS_PRELUDE_SOURCE),
        ];
        let mut program = standard_prelude_program().expect("prelude parses");
        let mut query_source = String::new();
        for (topic, _) in &topic_sources {
            writeln!(
                query_source,
                "? describe({}, doc).\n? source_of({}, file, lines).",
                datalog_string_literal(topic),
                datalog_string_literal(topic),
            )
            .expect("write query");
        }
        let query = parse_program("describe-topics", &query_source).expect("topic query parses");
        program.statements.extend(query.statements);

        let analyzed = analyze(program).expect("prelude topic query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&FactStore::default()));
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        for (idx, (topic, source_name)) in topic_sources.iter().enumerate() {
            let describe = &outputs[idx * 2];
            assert_eq!(describe.rows.len(), 1, "describe({topic})");
            assert!(
                matches!(describe.rows[0].fields.get("doc"), Some(Value::String(doc)) if !doc.is_empty()),
                "describe({topic}) should have doc text"
            );

            let source = &outputs[idx * 2 + 1];
            assert_eq!(
                source.rows[0].fields.get("file"),
                Some(&Value::String((*source_name).to_string())),
                "source_of({topic})"
            );
            assert_ne!(
                source.rows[0].fields.get("lines"),
                Some(&Value::String("unknown".to_string())),
                "source_of({topic}) should have concrete lines"
            );
        }
    }

    #[test]
    fn standard_prelude_derives_graph_convergence_and_ranking_rules() {
        let outputs = evaluate_standard_prelude_cases(
            &[
                ("area_of", r#"? area_of("ticket-1", area)."#),
                ("status_of", r#"? status_of("ticket-1", status)."#),
                (
                    "incoming_edge",
                    r#"? incoming_edge("closed-issue", from, kind)."#,
                ),
                ("outgoing_edge", r#"? outgoing_edge("ticket-1", to, kind)."#),
                ("orphan", r#"? orphan("REQ-1")."#),
                ("stub", r#"? stub("stub.md")."#),
                (
                    "diagnostic",
                    r#"? diagnostic("E001", severity, "ticket-1", file, line, evidence)."#,
                ),
                ("entropy", r#"? entropy("ticket-1", source)."#),
                ("potential", r#"? potential("ticket-1", energy)."#),
                ("blocked", r#"? blocked("ticket-1")."#),
                ("advancing", r#"? advancing("ticket-2")."#),
                ("ranked_work", r"? ranked_work(h, energy, rank)."),
                ("top_work", r"? top_work(h, energy)."),
                ("describe", r#"? describe("potential", doc)."#),
                ("source_of", r#"? source_of("ranked_work", file, lines)."#),
            ],
            standard_library_database(),
        );

        assert!(has_row(
            output(&outputs, "area_of"),
            &[("area", string("host"))]
        ));
        assert!(has_row(
            output(&outputs, "status_of"),
            &[("status", string("open"))]
        ));
        assert!(has_row(
            output(&outputs, "incoming_edge"),
            &[("from", string("ticket-1")), ("kind", string("DependsOn"))]
        ));
        assert!(has_row(
            output(&outputs, "outgoing_edge"),
            &[
                ("to", string("closed-issue")),
                ("kind", string("DependsOn"))
            ]
        ));
        assert_eq!(
            output(&outputs, "orphan").rows.len(),
            1,
            "REQ-1 is orphaned"
        );
        assert_eq!(
            output(&outputs, "stub").rows.len(),
            1,
            "stub.md is a content stub"
        );
        assert!(has_row(
            output(&outputs, "diagnostic"),
            &[
                ("severity", string("error")),
                ("file", string("ticket-1.md")),
                ("line", int(7)),
                (
                    "evidence",
                    list(vec![string("broken_ref"), string("ghost")])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "entropy"),
            &[("source", string("broken_ref"))]
        ));
        assert!(has_row(
            output(&outputs, "entropy"),
            &[("source", string("stale_dep"))]
        ));
        assert!(has_row(
            output(&outputs, "potential"),
            &[("energy", int(7))]
        ));
        assert_eq!(
            output(&outputs, "blocked").rows.len(),
            1,
            "ticket-1 is blocked"
        );
        assert_eq!(
            output(&outputs, "advancing").rows.len(),
            1,
            "ticket-2 advanced"
        );
        assert!(
            has_row(
                output(&outputs, "ranked_work"),
                &[
                    ("h", string("ticket-1")),
                    ("energy", int(7)),
                    ("rank", int(1))
                ]
            ),
            "ranked_work rows: {:?}",
            output(&outputs, "ranked_work").rows
        );
        assert!(has_row(
            output(&outputs, "top_work"),
            &[("h", string("REQ-1")), ("energy", int(6))]
        ));
        assert!(matches!(
            output(&outputs, "describe").rows[0].fields.get("doc"),
            Some(Value::String(doc)) if doc.contains("convergence energy")
        ));
        assert_eq!(
            output(&outputs, "source_of").rows[0].fields.get("file"),
            Some(&Value::String(RANKING_PRELUDE_SOURCE.to_string()))
        );
    }

    #[test]
    fn standard_prelude_derives_v1_diagnostic_catalog_relations() {
        let outputs = evaluate_standard_prelude_cases(
            &[
                (
                    "E001",
                    r#"? diagnostic("E001", severity, "broken.md", file, line, evidence)."#,
                ),
                (
                    "I001",
                    r#"? diagnostic("I001", severity, subject, file, line, evidence)."#,
                ),
                (
                    "W004",
                    r#"? diagnostic("W004", severity, "implausible.md", file, line, evidence)."#,
                ),
                (
                    "W001",
                    r#"? diagnostic("W001", severity, "stale-src.md", file, line, evidence)."#,
                ),
                (
                    "W002",
                    r#"? diagnostic("W002", severity, "stable-src.md", file, line, evidence)."#,
                ),
                (
                    "E002",
                    r#"? diagnostic("E002", severity, "OQ-1", file, line, evidence)."#,
                ),
                (
                    "I002",
                    r#"? diagnostic("I002", severity, "OQ-2", file, line, evidence)."#,
                ),
                (
                    "W003",
                    r#"? diagnostic("W003", severity, "team/missing.md", file, line, evidence)."#,
                ),
                (
                    "S001",
                    r#"? diagnostic("S001", severity, "ORPH-1", file, line, evidence)."#,
                ),
                (
                    "S003",
                    r#"? diagnostic("S003", severity, "draft", file, line, evidence)."#,
                ),
                (
                    "S004",
                    r#"? diagnostic("S004", severity, "OLD", file, line, evidence)."#,
                ),
                (
                    "S005",
                    r#"? diagnostic("S005", severity, "AA", file, line, evidence)."#,
                ),
            ],
            diagnostic_catalog_database(),
        );

        assert!(
            has_row(
                output(&outputs, "E001"),
                &[
                    ("severity", string("error")),
                    ("file", string("broken.md")),
                    ("line", int(3)),
                    (
                        "evidence",
                        list(vec![string("broken_ref"), string("missing.md")])
                    )
                ]
            ),
            "E001 rows: {:?}",
            output(&outputs, "E001").rows
        );
        assert!(has_row(
            output(&outputs, "I001"),
            &[
                ("severity", string("info")),
                ("subject", string("corpus")),
                ("file", Value::Null),
                ("evidence", list(vec![string("section_refs"), int(1)]))
            ]
        ));
        assert!(has_row(
            output(&outputs, "W004"),
            &[
                ("severity", string("warning")),
                ("file", string("implausible.md")),
                (
                    "evidence",
                    list(vec![
                        string("implausible_ref"),
                        string(r#"{"value":"/tmp/foo","reason":"absolute path","line":4}"#)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W001"),
            &[
                ("severity", string("warning")),
                ("file", string("stale-src.md")),
                (
                    "evidence",
                    list(vec![
                        string("stale_ref"),
                        string("draft"),
                        string("archived")
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W002"),
            &[
                ("severity", string("warning")),
                ("file", string("stable-src.md")),
                (
                    "evidence",
                    list(vec![
                        string("confidence_gap"),
                        string("stable"),
                        int(1),
                        string("draft"),
                        int(0)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "E002"),
            &[
                ("severity", string("error")),
                ("file", string("OQ-1.md")),
                ("evidence", string("undischarged"))
            ]
        ));
        assert!(has_row(
            output(&outputs, "I002"),
            &[
                ("severity", string("info")),
                ("file", string("OQ-2.md")),
                (
                    "evidence",
                    list(vec![string("multiple_discharges"), int(2)])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "W003"),
            &[
                ("severity", string("warning")),
                ("file", string("team/missing.md")),
                ("evidence", Value::Null)
            ]
        ));
        assert!(has_row(
            output(&outputs, "S001"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![string("orphaned_handle"), string("ORPH-1")])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S003"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("pipeline_stall"),
                        string("draft"),
                        int(20),
                        string("stable"),
                        Value::Bool(false)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S004"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("abandoned_namespace"),
                        string("OLD"),
                        int(2),
                        int(2),
                        int(0)
                    ])
                )
            ]
        ));
        assert!(has_row(
            output(&outputs, "S005"),
            &[
                ("severity", string("suggestion")),
                (
                    "evidence",
                    list(vec![
                        string("concern_group_candidate"),
                        string("AA"),
                        string("BB"),
                        int(3)
                    ])
                )
            ]
        ));
    }

    #[test]
    fn doc_declarations_replace_and_document_predicates() {
        let program = parse_program(
            "docs.dl",
            r#"@doc(name: "topic", doc: "first").
@doc(name: "topic", doc: "second").
fact("a").
topic(x) := fact(x).
? describe("topic", doc).
? predicates("topic", doc, file, lines).
? source_of("topic", file, lines).
"#,
        )
        .expect("doc program parses");
        let analyzed = analyze(program).expect("doc program analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, Database::default());
        evaluator.run_fixpoint().expect("doc program fixpoint runs");
        let outputs = queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect::<Vec<_>>();

        assert_eq!(outputs[0].rows.len(), 1);
        assert_eq!(
            outputs[0].rows[0].fields.get("doc"),
            Some(&Value::String("second".to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("doc"),
            Some(&Value::String("second".to_string()))
        );
        assert_eq!(
            outputs[1].rows[0].fields.get("lines"),
            Some(&Value::String("4".to_string()))
        );

        let source_lines = outputs[2]
            .rows
            .iter()
            .map(|row| row.fields.get("lines").cloned())
            .collect::<Vec<_>>();
        assert_eq!(
            source_lines,
            vec![
                Some(Value::String("2".to_string())),
                Some(Value::String("4".to_string())),
            ]
        );
    }

    #[test]
    fn assign_rule_layer_marks_nested_rules_as_prelude() {
        let program = parse_prelude_program(
            "layers.dl",
            r#"root(x) := fact(x).
? where local(x) := root(x). local(x).
at("snapshot:last") { historical(h) := *handle{id: h}. }
"#,
        );
        let program = program.expect("layer fixture parses");
        assert_prelude_layers(&program.statements);
    }

    fn assert_prelude_layers(statements: &[Statement]) {
        for statement in statements {
            match statement {
                Statement::Rule(rule) => {
                    assert_eq!(rule.origin().layer(), RuleLayer::Prelude);
                }
                Statement::Query(query) => {
                    assert!(
                        query
                            .local_rules
                            .iter()
                            .all(|rule| rule.origin().layer() == RuleLayer::Inline)
                    );
                }
                Statement::AtBlock { statements, .. } => assert_prelude_layers(statements),
                Statement::Fact(_)
                | Statement::ConfigBlock(_)
                | Statement::SourceBlock(_)
                | Statement::OptionalFact(_)
                | Statement::Include(_)
                | Statement::Import(_)
                | Statement::Verb(_)
                | Statement::Doc(_) => {}
            }
        }
    }

    fn evaluate_standard_prelude_queries(source: &str, database: Database) -> Vec<QueryOutput> {
        let mut program = standard_prelude_program().expect("prelude parses");
        let query = parse_program("stdlib-test", source).expect("query parses");
        program.statements.extend(query.statements);
        let analyzed = analyze(program).expect("prelude query analyzes");
        let queries = analyzed.queries().cloned().collect::<Vec<_>>();
        let mut evaluator = Evaluator::new(analyzed, database);
        evaluator.run_fixpoint().expect("prelude fixpoint runs");
        queries
            .iter()
            .map(|query| evaluator.eval_query(query).expect("query evaluates"))
            .collect()
    }

    fn evaluate_standard_prelude_cases(
        cases: &[(&'static str, &str)],
        database: Database,
    ) -> BTreeMap<&'static str, QueryOutput> {
        let mut source = String::new();
        for (_, query) in cases {
            writeln!(&mut source, "{query}").expect("write query case");
        }

        let outputs = evaluate_standard_prelude_queries(&source, database);
        assert_eq!(outputs.len(), cases.len(), "one output per query case");
        cases.iter().map(|(name, _)| *name).zip(outputs).collect()
    }

    fn output<'a>(outputs: &'a BTreeMap<&'static str, QueryOutput>, name: &str) -> &'a QueryOutput {
        outputs.get(name).expect("query case exists")
    }

    fn standard_library_database() -> Database {
        standard_library_database_with_snapshots(true)
    }

    fn standard_library_database_without_snapshots() -> Database {
        standard_library_database_with_snapshots(false)
    }

    fn standard_library_database_with_snapshots(include_snapshots: bool) -> Database {
        let corpus = CorpusId::from("test");
        let source = SourceName::from("host");
        let generation = Generation::initial();
        let scope = FixtureScope {
            corpus: &corpus,
            source: &source,
            generation,
        };
        let mut batch = FactBatch::new(
            corpus.clone(),
            source.clone(),
            FactBatchMode::FullSnapshot,
            generation,
        );
        batch.handles = vec![
            handle(&scope, "ticket-1", "issue", Some("open"), "", "host"),
            handle(&scope, "closed-issue", "issue", Some("closed"), "", "host"),
            handle(&scope, "ticket-2", "issue", Some("review"), "", "host"),
            handle(&scope, "REQ-1", "label", Some("open"), "REQ", "host"),
            handle(&scope, "stub.md", "file", Some("open"), "", "host"),
        ];
        batch.content = vec![
            content(
                &scope,
                "ticket-1",
                "intro",
                "ticket-1 urgent broken reference to ghost",
                6,
            ),
            content(
                &scope,
                "ticket-2",
                "intro",
                "ticket-2 recently advanced review work",
                5,
            ),
        ];
        batch.spans = vec![
            span(&scope, "ticket-1", "intro", 1, 3),
            span(&scope, "ticket-2", "intro", 1, 2),
        ];
        batch.edges = vec![
            edge(&scope, "ticket-1", "closed-issue", "DependsOn", 4),
            edge(&scope, "ticket-1", "ghost", "Cites", 7),
        ];

        let mut store = FactStore::default();
        store.merge(batch).expect("merge stdlib fixture");
        store
            .replace_configs(
                &corpus,
                vec![
                    config(&corpus, "convergence.active", "open", None),
                    config(&corpus, "convergence.active", "review", None),
                    config(&corpus, "convergence.terminal", "closed", None),
                    config(&corpus, "convergence.ordering", "open", Some(0)),
                    config(&corpus, "convergence.ordering", "review", Some(1)),
                    config(&corpus, "convergence.ordering", "closed", Some(2)),
                    config(&corpus, "handles.linear", "REQ", None),
                ],
            )
            .expect("replace stdlib fixture config");
        if include_snapshots {
            store
                .replace_snapshots(
                    &corpus,
                    vec![SnapshotFact {
                        corpus: corpus.clone(),
                        snapshot: "s1".to_string(),
                        at: "2026-05-01".to_string(),
                        id: "ticket-2".to_string(),
                        key: "status".to_string(),
                        value: "open".to_string(),
                    }],
                )
                .expect("replace stdlib fixture snapshots");
        }
        Database::from_store(&store).with_sources([fixture_source_info()])
    }

    fn fixture_source_info() -> SourceInfo {
        SourceInfo {
            name: "fixture",
            recognizes: vec![Pattern::new("*.md")],
            doc: "Fixture source used by standard-library tests.",
            config_keys: vec![
                ConfigKey::required("md.file_extension"),
                ConfigKey::optional("md.scan_exclude"),
            ],
            capabilities: SourceCapabilities {
                supports_git_ref: false,
                supports_time_snapshot: true,
                supports_incremental: true,
                live_only: false,
            },
            search: Some(crate::ranking::default_lexical_search_info()),
        }
    }

    fn diagnostic_catalog_database() -> Database {
        let corpus = CorpusId::from("diagnostics");
        let source = SourceName::from("host");
        let generation = Generation::initial();
        let scope = FixtureScope {
            corpus: &corpus,
            source: &source,
            generation,
        };
        let mut batch = FactBatch::new(
            corpus.clone(),
            source.clone(),
            FactBatchMode::FullSnapshot,
            generation,
        );
        batch.handles = vec![
            handle(&scope, "broken.md", "file", Some("draft"), "", ""),
            handle(&scope, "section.md", "file", Some("draft"), "", ""),
            handle(&scope, "implausible.md", "file", Some("draft"), "", ""),
            handle(&scope, "stale-src.md", "file", Some("draft"), "", ""),
            handle(&scope, "terminal.md", "file", Some("archived"), "", ""),
            handle(&scope, "stable-src.md", "file", Some("stable"), "", ""),
            handle(&scope, "draft-target.md", "file", Some("draft"), "", ""),
            handle(&scope, "team/with-a.md", "file", Some("draft"), "", "team"),
            handle(&scope, "team/with-b.md", "file", Some("draft"), "", "team"),
            handle(&scope, "team/missing.md", "file", None, "", "team"),
            handle(&scope, "OQ-1", "label", Some("draft"), "OQ", ""),
            handle(&scope, "OQ-2", "label", Some("draft"), "OQ", ""),
            handle(&scope, "impl-1.md", "file", Some("draft"), "", ""),
            handle(&scope, "impl-2.md", "file", Some("draft"), "", ""),
            handle(&scope, "ORPH-1", "label", Some("draft"), "ORPH", ""),
            handle(&scope, "NEW-1", "label", Some("draft"), "NEW", ""),
            handle(&scope, "NEW-2", "label", Some("draft"), "NEW", ""),
            handle(&scope, "NEW-3", "label", Some("draft"), "NEW", ""),
            handle(&scope, "OLD-1", "label", Some("archived"), "OLD", ""),
            handle(&scope, "OLD-2", "label", Some("archived"), "OLD", ""),
            handle(&scope, "AA-1", "label", Some("draft"), "AA", ""),
            handle(&scope, "BB-1", "label", Some("draft"), "BB", ""),
            handle(&scope, "co1.md", "file", Some("draft"), "", ""),
            handle(&scope, "co2.md", "file", Some("draft"), "", ""),
            handle(&scope, "co3.md", "file", Some("draft"), "", ""),
        ];
        batch.edges = vec![
            edge(&scope, "broken.md", "missing.md", "Cites", 3),
            edge(&scope, "section.md", "section:intro", "Cites", 9),
            edge(&scope, "stale-src.md", "terminal.md", "DependsOn", 1),
            edge(&scope, "stable-src.md", "draft-target.md", "DependsOn", 2),
            edge(&scope, "impl-1.md", "OQ-2", "Discharges", 1),
            edge(&scope, "impl-2.md", "OQ-2", "Discharges", 1),
            edge(&scope, "co1.md", "AA-1", "Cites", 1),
            edge(&scope, "co1.md", "BB-1", "Cites", 2),
            edge(&scope, "co2.md", "AA-1", "Cites", 1),
            edge(&scope, "co2.md", "BB-1", "Cites", 2),
            edge(&scope, "co3.md", "AA-1", "Cites", 1),
            edge(&scope, "co3.md", "BB-1", "Cites", 2),
        ];
        batch.meta = vec![
            meta(
                &scope,
                "implausible.md",
                "md.implausible_ref",
                r#"{"value":"/tmp/foo","reason":"absolute path","line":4}"#,
            ),
            meta(&scope, "team/with-a.md", "md.parent_dir", "team"),
            meta(&scope, "team/with-b.md", "md.parent_dir", "team"),
            meta(&scope, "team/missing.md", "md.parent_dir", "team"),
        ];

        let mut store = FactStore::default();
        store.merge(batch).expect("merge diagnostic fixture");
        store
            .replace_configs(
                &corpus,
                vec![
                    config(&corpus, "convergence.active", "draft", None),
                    config(&corpus, "convergence.active", "stable", None),
                    config(&corpus, "convergence.terminal", "archived", None),
                    config(&corpus, "convergence.ordering", "draft", Some(0)),
                    config(&corpus, "convergence.ordering", "stable", Some(1)),
                    config(&corpus, "convergence.ordering", "archived", Some(2)),
                    config(&corpus, "handles.linear", "OQ", None),
                    config(&corpus, "handles.force", "OLD", None),
                    config(&corpus, "handles.force", "AA", None),
                    config(&corpus, "handles.force", "BB", None),
                ],
            )
            .expect("replace diagnostic fixture config");
        Database::from_store(&store)
    }

    struct FixtureScope<'a> {
        corpus: &'a CorpusId,
        source: &'a SourceName,
        generation: Generation,
    }

    fn handle(
        scope: &FixtureScope<'_>,
        id: &str,
        kind: &str,
        status: Option<&str>,
        namespace: &str,
        area: &str,
    ) -> HandleFact {
        let file = fixture_file_for(id);
        HandleFact {
            identity: identity(scope, id),
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.map(str::to_string),
            namespace: namespace.to_string(),
            file,
            line: 1,
            date: None,
            area: area.to_string(),
            summary: String::new(),
        }
    }

    fn edge(scope: &FixtureScope<'_>, from: &str, to: &str, kind: &str, line: u32) -> EdgeFact {
        let file = fixture_file_for(from);
        EdgeFact {
            identity: identity(scope, &format!("{from}->{to}")),
            from: from.to_string(),
            to: to.to_string(),
            kind: kind.to_string(),
            file,
            line,
        }
    }

    fn meta(scope: &FixtureScope<'_>, handle: &str, key: &str, value: &str) -> MetaFact {
        MetaFact {
            identity: identity(scope, &format!("{handle}:{key}:{value}")),
            handle: handle.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn content(
        scope: &FixtureScope<'_>,
        handle: &str,
        span_id: &str,
        text: &str,
        tokens: u32,
    ) -> ContentFact {
        ContentFact {
            identity: identity(scope, &format!("{handle}#{span_id}")),
            handle: handle.to_string(),
            span_id: span_id.to_string(),
            lines: 1,
            text: text.to_string(),
            tokens,
        }
    }

    fn span(
        scope: &FixtureScope<'_>,
        handle: &str,
        span_id: &str,
        start_line: u32,
        end_line: u32,
    ) -> SpanFact {
        SpanFact {
            identity: identity(scope, &format!("{handle}#{span_id}")),
            id: span_id.to_string(),
            handle: handle.to_string(),
            start_line,
            end_line,
            summary: String::new(),
        }
    }

    fn fixture_file_for(id: &str) -> String {
        if std::path::Path::new(id)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            id.to_string()
        } else {
            format!("{id}.md")
        }
    }

    fn identity(scope: &FixtureScope<'_>, native_id: &str) -> FactIdentity {
        FactIdentity::new(
            scope.corpus.clone(),
            scope.source.clone(),
            NativeId::from(native_id),
            OriginUri::from(format!("fixture://{native_id}")),
            Revision::from("test"),
            scope.generation,
        )
    }

    fn config(corpus: &CorpusId, key: &str, value: &str, ordinal: Option<u32>) -> ConfigFact {
        ConfigFact {
            corpus: corpus.clone(),
            key: key.to_string(),
            value: value.to_string(),
            ordinal,
        }
    }

    fn has_row(output: &QueryOutput, expected: &[(&str, Value)]) -> bool {
        output.rows.iter().any(|row| {
            expected
                .iter()
                .all(|(field, value)| row.fields.get(*field) == Some(value))
        })
    }

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn int(value: i64) -> Value {
        Value::Number(NumberValue::Int(value))
    }

    fn list(values: Vec<Value>) -> Value {
        Value::List(values)
    }
}
