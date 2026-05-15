use serde::{Deserialize, Serialize};

use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};

/// Origin tuple carried by every source-derived stored fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactIdentity {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub native_id: NativeId,
    pub origin_uri: OriginUri,
    pub revision: Revision,
    pub generation: Generation,
}

impl FactIdentity {
    pub fn new(
        corpus: CorpusId,
        source: SourceName,
        native_id: NativeId,
        origin_uri: OriginUri,
        revision: Revision,
        generation: Generation,
    ) -> Self {
        Self {
            corpus,
            source,
            native_id,
            origin_uri,
            revision,
            generation,
        }
    }
}

/// Stored `*handle` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleFact {
    pub identity: FactIdentity,
    pub id: String,
    pub kind: String,
    pub status: Option<String>,
    pub namespace: String,
    pub file: String,
    pub line: u32,
    pub date: Option<String>,
    pub area: String,
    pub summary: String,
}

/// Stored `*edge` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeFact {
    pub identity: FactIdentity,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
}

/// Stored `*meta` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaFact {
    pub identity: FactIdentity,
    pub handle: String,
    pub key: String,
    pub value: String,
}

/// Stored `*content` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFact {
    pub identity: FactIdentity,
    pub handle: String,
    pub span_id: String,
    pub lines: u32,
    pub text: String,
    pub tokens: u32,
}

/// Stored `*span` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanFact {
    pub identity: FactIdentity,
    pub id: String,
    pub handle: String,
    pub start_line: u32,
    pub end_line: u32,
    pub summary: String,
}

/// Stored `*concern` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConcernFact {
    pub identity: FactIdentity,
    pub name: String,
    pub member: String,
}

/// Runtime-populated `*config` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFact {
    pub corpus: CorpusId,
    pub key: String,
    pub value: String,
}

/// Runtime-populated `*snapshot` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFact {
    pub corpus: CorpusId,
    pub snapshot: String,
    pub at: String,
    pub id: String,
    pub key: String,
    pub value: String,
}

/// Source extraction mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactBatchMode {
    /// Replace all current facts for `(corpus, source)`.
    FullSnapshot,
    /// Upsert returned facts and retract the listed native ids.
    Delta,
}

/// Facts returned by a `Source`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactBatch {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub mode: FactBatchMode,
    pub generation: Generation,
    pub handles: Vec<HandleFact>,
    pub edges: Vec<EdgeFact>,
    pub content: Vec<ContentFact>,
    pub spans: Vec<SpanFact>,
    pub meta: Vec<MetaFact>,
    pub concerns: Vec<ConcernFact>,
    pub retractions: Vec<NativeId>,
}

impl FactBatch {
    pub fn new(
        corpus: CorpusId,
        source: SourceName,
        mode: FactBatchMode,
        generation: Generation,
    ) -> Self {
        Self {
            corpus,
            source,
            mode,
            generation,
            handles: Vec::new(),
            edges: Vec::new(),
            content: Vec::new(),
            spans: Vec::new(),
            meta: Vec::new(),
            concerns: Vec::new(),
            retractions: Vec::new(),
        }
    }
}
