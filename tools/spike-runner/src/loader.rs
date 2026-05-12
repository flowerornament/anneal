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
use std::path::Path;
use std::process::Command;

/// Errors raised while loading from an anneal corpus.
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
}

/// Loaded corpus state, ready to push into an ascent program.
#[derive(Debug, Default)]
pub struct Corpus {
    pub handles: Vec<Handle>,
    pub edges: Vec<Edge>,
}

impl Corpus {
    pub fn handle_count(&self) -> usize { self.handles.len() }
    pub fn edge_count(&self)   -> usize { self.edges.len() }
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

/// Leak `s` into a `'static` byte slice. Each call allocates; the spike
/// uses ~O(handles × fields) leaked strings. Negligible at 13k handles.
fn intern(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
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

/// Derive the corpus-root-relative area (first path component) for a file
/// handle. Mirrors anneal's `area_of` heuristic. Returns `(root)` for
/// files at the corpus root.
fn area_of(file: &str) -> &'static str {
    file.split_once('/').map_or("(root)", |(first, _)| intern(first))
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

    let mut handles = Vec::with_capacity(handles_env.items.len());
    for h in handles_env.items {
        let kind = HandleKind::parse(&h.handle_kind)
            .ok_or_else(|| LoadError::UnknownHandleKind(h.handle_kind.clone()))?;
        let id = HandleId(intern(&h.id));
        let file = h.file.as_deref().map_or(FilePath(""), |f| FilePath(intern(f)));
        let area = Area(area_of(h.file.as_deref().unwrap_or("")));
        let namespace = h.namespace.as_deref()
            .map_or(Namespace::NONE, |n| Namespace(intern(n)));
        let status = h.status.as_deref()
            .map_or(Status::Other(""), |s| Status::parse(intern(s)));
        handles.push(Handle { id, kind, status, namespace, file, area, date: None });
    }

    let mut edges = Vec::with_capacity(edges_env.items.len());
    for e in edges_env.items {
        let kind = EdgeKind::parse(intern(&e.edge_kind));
        let file = e.source_file.as_deref().map_or(FilePath(""), |f| FilePath(intern(f)));
        edges.push(Edge {
            from: HandleId(intern(&e.source)),
            to: HandleId(intern(&e.target)),
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
        assert_eq!(area_of("compiler/jit.md"), "compiler");
        assert_eq!(area_of("a/b/c.md"), "a");
    }

    #[test]
    fn area_of_returns_root_marker_for_top_level_files() {
        assert_eq!(area_of("README.md"), "(root)");
    }
}
