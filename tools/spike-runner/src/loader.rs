//! Load a real corpus into spike-runner data structures by shelling out
//! to the anneal v1.1 binary's `query --json` surface.
//!
//! The spike's `fixture` module is hand-authored static data; this module
//! produces the same shape from real markdown corpora. Strings are leaked
//! to `'static` because every downstream type carries `&'static str` for
//! zero-cost relation tuples; the spike is short-lived so the leak is
//! acceptable as a benchmark fixture.

use crate::fixture::{Edge, Handle};
use crate::types::{Area, EdgeKind, FilePath, HandleId, HandleKind, Namespace, Status};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to spawn `anneal {kind}`: {source}")]
    Spawn { kind: &'static str, #[source] source: std::io::Error },

    #[error("`anneal {kind}` exited {status}: {stderr}")]
    NonZero { kind: &'static str, status: std::process::ExitStatus, stderr: String },

    #[error("failed to parse `anneal {kind}` JSON: {source}")]
    Parse { kind: &'static str, #[source] source: serde_json::Error },

    #[error("unknown handle_kind {0:?}")]
    UnknownHandleKind(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Staging buffer holding the parsed corpus ready for the ascent program.
#[derive(Debug, Default)]
pub struct Corpus {
    pub handles: Vec<Handle>,
    pub edges: Vec<Edge>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    items: Vec<T>,
}

#[derive(Deserialize)]
struct HandleJson {
    id: String,
    handle_kind: String,
    status: Option<String>,
    file: Option<String>,
    namespace: Option<String>,
}

#[derive(Deserialize)]
struct EdgeJson {
    source: String,
    target: String,
    edge_kind: String,
    source_file: Option<String>,
}

/// Deduplicating interner: each unique input string is leaked exactly
/// once and returned as `&'static str`. Without dedup, a corpus where
/// every edge references the same handle id (Cites is 99% of large-corpus's
/// edges) would leak that id once per edge; with dedup, once per corpus.
#[derive(Default)]
struct Interner {
    cache: HashMap<String, &'static str>,
}

impl Interner {
    fn get(&mut self, s: &str) -> &'static str {
        if let Some(&v) = self.cache.get(s) { return v; }
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        self.cache.insert(leaked.to_string(), leaked);
        leaked
    }
}

fn run_anneal_json(root: &Path, kind: &'static str) -> Result<Vec<u8>, LoadError> {
    let root_str = root.to_string_lossy();
    let out = Command::new("anneal")
        .args(["--root", &root_str, "query", kind, "--json", "--full"])
        .output()
        .map_err(|source| LoadError::Spawn { kind, source })?;
    if !out.status.success() {
        return Err(LoadError::NonZero {
            kind,
            status: out.status,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(out.stdout)
}

/// First path component of a file handle's path. Mirrors anneal's
/// `area_of` heuristic at `src/area.rs`. Returns `(root)` for top-level
/// files.
fn area_of(file: &str, interner: &mut Interner) -> &'static str {
    file.split_once('/').map_or("(root)", |(first, _)| interner.get(first))
}

/// Load a corpus by shelling out to anneal's query surface. Requires the
/// `anneal` binary to be on PATH and the corpus to have an `anneal.toml`
/// at `root` (or a parent).
///
/// # Errors
/// Returns [`LoadError`] if anneal fails to spawn, exits non-zero, or
/// produces JSON the spike can't parse.
pub fn load_via_anneal(root: &Path) -> Result<Corpus, LoadError> {
    let handles_bytes = run_anneal_json(root, "handles")?;
    let handles_env: Envelope<HandleJson> = serde_json::from_slice(&handles_bytes)
        .map_err(|source| LoadError::Parse { kind: "handles", source })?;

    let edges_bytes = run_anneal_json(root, "edges")?;
    let edges_env: Envelope<EdgeJson> = serde_json::from_slice(&edges_bytes)
        .map_err(|source| LoadError::Parse { kind: "edges", source })?;

    let mut interner = Interner::default();
    let mut handles = Vec::with_capacity(handles_env.items.len());
    for h in handles_env.items {
        let kind = HandleKind::parse(&h.handle_kind)
            .ok_or_else(|| LoadError::UnknownHandleKind(h.handle_kind.clone()))?;
        let id = HandleId(interner.get(&h.id));
        let file = h.file.as_deref().map_or(FilePath(""), |f| FilePath(interner.get(f)));
        let area = Area(area_of(h.file.as_deref().unwrap_or(""), &mut interner));
        let namespace = h.namespace.as_deref()
            .map_or(Namespace::NONE, |n| Namespace(interner.get(n)));
        let status = h.status.as_deref()
            .map_or(Status::Other(""), |s| Status::parse(interner.get(s)));
        handles.push(Handle { id, kind, status, namespace, file, area, date: None });
    }

    let mut edges = Vec::with_capacity(edges_env.items.len());
    for e in edges_env.items {
        let kind = EdgeKind::parse(interner.get(&e.edge_kind));
        let file = e.source_file.as_deref().map_or(FilePath(""), |f| FilePath(interner.get(f)));
        edges.push(Edge {
            from: HandleId(interner.get(&e.source)),
            to: HandleId(interner.get(&e.target)),
            kind,
            file,
            line: 0,
        });
    }

    Ok(Corpus { handles, edges })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_of_returns_first_segment() {
        let mut i = Interner::default();
        assert_eq!(area_of("compiler/jit.md", &mut i), "compiler");
        assert_eq!(area_of("a/b/c.md", &mut i), "a");
    }

    #[test]
    fn area_of_returns_root_marker_for_top_level_files() {
        let mut i = Interner::default();
        assert_eq!(area_of("README.md", &mut i), "(root)");
    }

    #[test]
    fn interner_dedups_repeated_strings() {
        let mut i = Interner::default();
        let a = i.get("foo");
        let b = i.get("foo");
        assert!(std::ptr::eq(a, b),
            "second call must return the same leaked address as the first");
        assert_ne!(a, i.get("bar"));
    }
}
