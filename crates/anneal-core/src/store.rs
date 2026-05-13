use std::collections::BTreeSet;
use std::fmt;

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
    HandleFact, MetaFact, SnapshotFact, SpanFact,
};
use crate::ids::{CorpusId, Generation, NativeId, SourceName};

/// In-memory stored-fact relation set with runtime-owned generation swaps.
#[derive(Clone, Debug, Default)]
pub struct FactStore {
    handles: Vec<HandleFact>,
    edges: Vec<EdgeFact>,
    content: Vec<ContentFact>,
    spans: Vec<SpanFact>,
    meta: Vec<MetaFact>,
    concerns: Vec<ConcernFact>,
    configs: Vec<ConfigFact>,
    snapshots: Vec<SnapshotFact>,
    generations: Vec<GenerationFact>,
}

/// Stored `*generation` row.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GenerationFact {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub current: Generation,
}

impl FactStore {
    pub fn merge(&mut self, batch: FactBatch) -> Result<(), StoreError> {
        let mut validated = ValidatedBatch::from_batch(&batch)?;

        if matches!(batch.mode, FactBatchMode::FullSnapshot) {
            self.remove_scope(&validated.scope);
        } else {
            validated
                .native_ids
                .extend(batch.retractions.iter().cloned());
            self.remove_native_ids(&validated.scope, &validated.native_ids);
        }

        self.handles.extend(batch.handles);
        self.edges.extend(batch.edges);
        self.content.extend(batch.content);
        self.spans.extend(batch.spans);
        self.meta.extend(batch.meta);
        self.concerns.extend(batch.concerns);
        self.set_generation(
            validated.scope.corpus,
            validated.scope.source,
            batch.generation,
        );
        Ok(())
    }

    pub fn handles(&self) -> &[HandleFact] {
        &self.handles
    }

    pub fn edges(&self) -> &[EdgeFact] {
        &self.edges
    }

    pub fn content(&self) -> &[ContentFact] {
        &self.content
    }

    pub fn spans(&self) -> &[SpanFact] {
        &self.spans
    }

    pub fn meta(&self) -> &[MetaFact] {
        &self.meta
    }

    pub fn concerns(&self) -> &[ConcernFact] {
        &self.concerns
    }

    pub fn configs(&self) -> &[ConfigFact] {
        &self.configs
    }

    pub fn snapshots(&self) -> &[SnapshotFact] {
        &self.snapshots
    }

    pub fn generations(&self) -> &[GenerationFact] {
        &self.generations
    }

    fn set_generation(&mut self, corpus: CorpusId, source: SourceName, current: Generation) {
        if let Some(existing) = self
            .generations
            .iter_mut()
            .find(|row| row.corpus == corpus && row.source == source)
        {
            existing.current = current;
        } else {
            self.generations.push(GenerationFact {
                corpus,
                source,
                current,
            });
        }
    }

    fn remove_scope(&mut self, scope: &BatchScope) {
        self.handles.retain(|fact| !scope.matches(&fact.identity));
        self.edges.retain(|fact| !scope.matches(&fact.identity));
        self.content.retain(|fact| !scope.matches(&fact.identity));
        self.spans.retain(|fact| !scope.matches(&fact.identity));
        self.meta.retain(|fact| !scope.matches(&fact.identity));
        self.concerns.retain(|fact| !scope.matches(&fact.identity));
    }

    fn remove_native_ids(&mut self, scope: &BatchScope, native_ids: &BTreeSet<NativeId>) {
        self.handles
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
        self.edges
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
        self.content
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
        self.spans
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
        self.meta
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
        self.concerns
            .retain(|fact| !scope.matches_native(&fact.identity, native_ids));
    }
}

#[derive(Clone)]
struct BatchScope {
    corpus: CorpusId,
    source: SourceName,
}

struct ValidatedBatch {
    scope: BatchScope,
    native_ids: BTreeSet<NativeId>,
}

impl ValidatedBatch {
    fn from_batch(batch: &FactBatch) -> Result<Self, StoreError> {
        let scope = BatchScope::from_batch(batch);
        let mut native_ids = BTreeSet::new();
        let collect_native_ids = matches!(batch.mode, FactBatchMode::Delta);
        for identity in all_identities(batch) {
            if !scope.matches(identity) {
                return Err(StoreError::MixedSourceBatch);
            }
            if identity.generation != batch.generation {
                return Err(StoreError::MismatchedGeneration);
            }
            if collect_native_ids {
                native_ids.insert(identity.native_id.clone());
            }
        }
        Ok(Self { scope, native_ids })
    }
}

impl BatchScope {
    fn from_batch(batch: &FactBatch) -> Self {
        Self {
            corpus: batch.corpus.clone(),
            source: batch.source.clone(),
        }
    }

    fn matches(&self, identity: &FactIdentity) -> bool {
        identity.corpus == self.corpus && identity.source == self.source
    }

    fn matches_native(&self, identity: &FactIdentity, native_ids: &BTreeSet<NativeId>) -> bool {
        self.matches(identity) && native_ids.contains(&identity.native_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    MixedSourceBatch,
    MismatchedGeneration,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedSourceBatch => {
                f.write_str("FactBatch contains multiple corpus/source scopes")
            }
            Self::MismatchedGeneration => {
                f.write_str("FactBatch fact identity generation does not match batch generation")
            }
        }
    }
}

impl std::error::Error for StoreError {}

fn all_identities(batch: &FactBatch) -> impl Iterator<Item = &FactIdentity> {
    batch
        .handles
        .iter()
        .map(|fact| &fact.identity)
        .chain(batch.edges.iter().map(|fact| &fact.identity))
        .chain(batch.content.iter().map(|fact| &fact.identity))
        .chain(batch.spans.iter().map(|fact| &fact.identity))
        .chain(batch.meta.iter().map(|fact| &fact.identity))
        .chain(batch.concerns.iter().map(|fact| &fact.identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{FactBatch, FactBatchMode, FactIdentity, HandleFact, MetaFact};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};

    fn identity(native_id: &str, generation: Generation) -> FactIdentity {
        FactIdentity::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            NativeId::from(native_id),
            OriginUri::from(format!("file://{native_id}")),
            Revision::from("r1"),
            generation,
        )
    }

    fn handle(native_id: &str, generation: Generation, status: &str) -> HandleFact {
        HandleFact {
            identity: identity(native_id, generation),
            id: native_id.to_string(),
            kind: "file".to_string(),
            status: Some(status.to_string()),
            namespace: String::new(),
            file: native_id.to_string(),
            line: 1,
            date: None,
            area: String::new(),
            summary: String::new(),
        }
    }

    fn meta(native_id: &str, generation: Generation, key: &str) -> MetaFact {
        MetaFact {
            identity: identity(native_id, generation),
            handle: native_id.to_string(),
            key: key.to_string(),
            value: "value".to_string(),
        }
    }

    #[test]
    fn full_snapshot_replaces_existing_scope() {
        let mut store = FactStore::default();
        let mut first = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        first
            .handles
            .push(handle("a.md", Generation::new(1), "draft"));
        store.merge(first).expect("merge first");

        let mut second = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(2),
        );
        second
            .handles
            .push(handle("b.md", Generation::new(2), "current"));
        store.merge(second).expect("merge second");

        assert_eq!(store.handles().len(), 1);
        assert_eq!(store.handles()[0].id, "b.md");
        assert_eq!(store.generations()[0].current, Generation::new(2));
    }

    #[test]
    fn delta_retracts_all_facts_for_native_id() {
        let mut store = FactStore::default();
        let mut first = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        first
            .handles
            .push(handle("a.md", Generation::new(1), "draft"));
        first.meta.push(meta("a.md", Generation::new(1), "purpose"));
        first
            .handles
            .push(handle("b.md", Generation::new(1), "draft"));
        store.merge(first).expect("merge first");

        let mut delta = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::Delta,
            Generation::new(2),
        );
        delta.retractions.push(NativeId::from("a.md"));
        store.merge(delta).expect("merge delta");

        assert_eq!(store.handles().len(), 1);
        assert_eq!(store.handles()[0].id, "b.md");
        assert!(store.meta().is_empty());
        assert_eq!(store.generations()[0].current, Generation::new(2));
    }

    #[test]
    fn delta_upsert_replaces_prior_native_id_rows() {
        let mut store = FactStore::default();
        let mut first = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        first
            .handles
            .push(handle("a.md", Generation::new(1), "draft"));
        store.merge(first).expect("merge first");

        let mut delta = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::Delta,
            Generation::new(2),
        );
        delta
            .handles
            .push(handle("a.md", Generation::new(2), "current"));
        store.merge(delta).expect("merge delta");

        assert_eq!(store.handles().len(), 1);
        assert_eq!(store.handles()[0].status.as_deref(), Some("current"));
    }

    #[test]
    fn empty_full_snapshot_clears_source_scope() {
        let mut store = FactStore::default();
        let mut first = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        first
            .handles
            .push(handle("a.md", Generation::new(1), "draft"));
        store.merge(first).expect("merge first");

        let empty = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(2),
        );
        store.merge(empty).expect("merge empty");

        assert!(store.handles().is_empty());
        assert_eq!(store.generations()[0].current, Generation::new(2));
    }
}
