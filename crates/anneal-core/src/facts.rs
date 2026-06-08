//! Stored fact types emitted by source adapters.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
use crate::visibility::FactVisibility;

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
    #[serde(default)]
    pub ordinal: Option<u32>,
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

/// Runtime schema metadata for a stored relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoredRelationDescriptor {
    pub(crate) name: &'static str,
    pub(crate) fields: &'static [&'static str],
    pub(crate) doc: &'static str,
    pub(crate) provenance: &'static str,
    pub(crate) example: &'static str,
}

pub(crate) const HANDLE_RELATION_NAME: &str = "handle";
pub(crate) const EDGE_RELATION_NAME: &str = "edge";
pub(crate) const META_RELATION_NAME: &str = "meta";
pub(crate) const CONFIG_RELATION_NAME: &str = "config";
pub(crate) const CONTENT_RELATION_NAME: &str = "content";
pub(crate) const SPAN_RELATION_NAME: &str = "span";
pub(crate) const CONCERN_RELATION_NAME: &str = "concern";
pub(crate) const SNAPSHOT_RELATION_NAME: &str = "snapshot";
pub(crate) const GENERATION_RELATION_NAME: &str = "generation";

pub(crate) const STORED_RELATION_DESCRIPTORS: &[StoredRelationDescriptor] = &[
    StoredRelationDescriptor {
        name: HANDLE_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "id",
            "kind",
            "status",
            "namespace",
            "file",
            "line",
            "date",
            "area",
            "summary",
        ],
        doc: "Stored corpus handles emitted by linked sources.",
        provenance: "source",
        example: r#"? *handle{id: h, kind: "file", status: status}."#,
    },
    StoredRelationDescriptor {
        name: EDGE_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "from",
            "to",
            "kind",
            "file",
            "line",
        ],
        doc: "Stored typed edges between corpus handles.",
        provenance: "source",
        example: r#"? *edge{from: src, to: dst, kind: "DependsOn"}."#,
    },
    StoredRelationDescriptor {
        name: META_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "handle",
            "key",
            "value",
        ],
        doc: "Stored key/value metadata attached to handles.",
        provenance: "source",
        example: r"? *meta{handle: h, key: key, value: value}.",
    },
    StoredRelationDescriptor {
        name: CONTENT_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "handle",
            "span_id",
            "lines",
            "text",
            "tokens",
        ],
        doc: "Stored retrievable content spans for handles.",
        provenance: "source",
        example: r"? *content{handle: h, span_id: span, tokens: tokens}.",
    },
    StoredRelationDescriptor {
        name: SPAN_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "id",
            "handle",
            "start_line",
            "end_line",
            "summary",
        ],
        doc: "Stored source spans with line ranges and summaries.",
        provenance: "source",
        example: r"? *span{id: span, handle: h, start_line: start, end_line: end}.",
    },
    StoredRelationDescriptor {
        name: CONCERN_RELATION_NAME,
        fields: &[
            "corpus",
            "source",
            "native_id",
            "origin_uri",
            "revision",
            "generation",
            "name",
            "member",
        ],
        doc: "Stored concern membership facts.",
        provenance: "source",
        example: r"? *concern{name: concern, member: h}.",
    },
    StoredRelationDescriptor {
        name: CONFIG_RELATION_NAME,
        fields: &["corpus", "key", "value", "ordinal"],
        doc: "Runtime-populated configuration facts.",
        provenance: "runtime",
        example: r"? *config{key: key, value: value, ordinal: ordinal}.",
    },
    StoredRelationDescriptor {
        name: SNAPSHOT_RELATION_NAME,
        fields: &["corpus", "snapshot", "at", "id", "key", "value"],
        doc: "Runtime-populated historical snapshot facts.",
        provenance: "runtime",
        example: r"? *snapshot{snapshot: snapshot, id: h, key: key, value: value}.",
    },
    StoredRelationDescriptor {
        name: GENERATION_RELATION_NAME,
        fields: &["corpus", "source", "current"],
        doc: "Runtime-populated current generation per source.",
        provenance: "runtime",
        example: r"? *generation{source: source, current: generation}.",
    },
];

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub visibility: BTreeMap<NativeId, FactVisibility>,
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
            visibility: BTreeMap::new(),
            handles: Vec::new(),
            edges: Vec::new(),
            content: Vec::new(),
            spans: Vec::new(),
            meta: Vec::new(),
            concerns: Vec::new(),
            retractions: Vec::new(),
        }
    }

    pub fn set_visibility(&mut self, native_id: NativeId, visibility: FactVisibility) {
        if visibility == FactVisibility::Public {
            self.visibility.remove(&native_id);
        } else {
            self.visibility.insert(native_id, visibility);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_fact_missing_ordinal_deserializes_as_none() {
        let fact: ConfigFact =
            serde_json::from_str(r#"{"corpus":"test","key":"convergence.active","value":"draft"}"#)
                .expect("missing ordinal defaults");

        assert_eq!(fact.ordinal, None);
    }

    #[test]
    fn config_fact_null_and_numeric_ordinals_deserialize() {
        let null_fact: ConfigFact = serde_json::from_str(
            r#"{"corpus":"test","key":"convergence.active","value":"draft","ordinal":null}"#,
        )
        .expect("null ordinal parses");
        let ordered_fact: ConfigFact = serde_json::from_str(
            r#"{"corpus":"test","key":"convergence.ordering","value":"draft","ordinal":1}"#,
        )
        .expect("numeric ordinal parses");

        assert_eq!(null_fact.ordinal, None);
        assert_eq!(ordered_fact.ordinal, Some(1));
    }
}
