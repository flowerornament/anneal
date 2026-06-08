//! Deterministic in-memory fact store used to build runtime databases.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
    HandleFact, MetaFact, SnapshotFact, SpanFact,
};
use crate::history::SnapshotHistory;
use crate::ids::{CorpusId, Generation, NativeId, SourceName};
use crate::visibility::FactVisibility;

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
    visibility: BTreeMap<VisibilityKey, FactVisibility>,
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
        self.visibility
            .extend(batch.visibility.into_iter().map(|(native_id, visibility)| {
                (
                    VisibilityKey::new(
                        validated.scope.corpus.clone(),
                        validated.scope.source.clone(),
                        native_id,
                    ),
                    visibility,
                )
            }));
        self.set_generation(
            validated.scope.corpus,
            validated.scope.source,
            batch.generation,
        );
        self.canonicalize_source_relations();
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

    pub fn generation_for(&self, corpus: &CorpusId, source: &SourceName) -> Option<Generation> {
        self.generation_index(corpus, source)
            .map(|index| self.generations[index].current)
    }

    pub fn visibility_for(&self, identity: &FactIdentity) -> FactVisibility {
        self.visibility
            .get(&VisibilityKey::from_identity(identity))
            .copied()
            .unwrap_or_default()
    }

    /// Replace runtime snapshot facts for one corpus.
    ///
    /// Snapshot rows are runtime-owned historical state, so source generation
    /// swaps do not retract them.
    pub fn replace_snapshots(
        &mut self,
        corpus: &CorpusId,
        snapshots: Vec<SnapshotFact>,
    ) -> Result<(), StoreError> {
        if snapshots.iter().any(|fact| &fact.corpus != corpus) {
            return Err(StoreError::MixedSnapshotCorpus);
        }
        self.snapshots.retain(|fact| &fact.corpus != corpus);
        self.snapshots.extend(snapshots);
        sort_snapshots(&mut self.snapshots);
        Ok(())
    }

    /// Load parsed history entries into runtime `*snapshot` rows.
    ///
    /// One history file may contain entries for multiple corpora, so this
    /// replaces every corpus represented in the parsed history atomically
    /// within the in-memory store.
    pub fn replace_snapshot_history(&mut self, history: &SnapshotHistory) {
        let snapshots = history.snapshot_facts();
        let corpora = snapshots
            .iter()
            .map(|fact| fact.corpus.clone())
            .collect::<BTreeSet<_>>();
        self.snapshots
            .retain(|fact| !corpora.contains(&fact.corpus));
        self.snapshots.extend(snapshots);
        sort_snapshots(&mut self.snapshots);
    }

    /// Replace runtime config facts for one corpus.
    ///
    /// Config rows are runtime-owned, so source generation swaps do not
    /// retract them.
    pub fn replace_configs(
        &mut self,
        corpus: &CorpusId,
        configs: Vec<ConfigFact>,
    ) -> Result<(), StoreError> {
        if configs.iter().any(|fact| &fact.corpus != corpus) {
            return Err(StoreError::MixedConfigCorpus);
        }
        self.configs.retain(|fact| &fact.corpus != corpus);
        self.configs.extend(configs);
        sort_configs(&mut self.configs);
        Ok(())
    }

    /// Keep stored relation vectors in deterministic, relation-local order.
    ///
    /// Source extraction may discover files or graph nodes through orderings that
    /// are not guaranteed across processes. The runtime preserves these vectors
    /// when materializing stored relations, so canonicalizing once at the store
    /// boundary makes raw stored queries, search indexing, and derived rule input
    /// deterministic by construction.
    fn canonicalize_source_relations(&mut self) {
        self.handles.sort_by(compare_handle_facts);
        self.edges.sort_by(compare_edge_facts);
        self.content.sort_by(compare_content_facts);
        self.spans.sort_by(compare_span_facts);
        self.meta.sort_by(compare_meta_facts);
        self.concerns.sort_by(compare_concern_facts);
        self.generations.sort_by(compare_generation_facts);
    }

    fn set_generation(&mut self, corpus: CorpusId, source: SourceName, current: Generation) {
        if let Some(index) = self.generation_index(&corpus, &source) {
            self.generations[index].current = current;
        } else {
            self.generations.push(GenerationFact {
                corpus,
                source,
                current,
            });
        }
    }

    fn generation_index(&self, corpus: &CorpusId, source: &SourceName) -> Option<usize> {
        self.generations
            .iter()
            .position(|row| &row.corpus == corpus && &row.source == source)
    }

    fn remove_scope(&mut self, scope: &BatchScope) {
        self.handles.retain(|fact| !scope.matches(&fact.identity));
        self.edges.retain(|fact| !scope.matches(&fact.identity));
        self.content.retain(|fact| !scope.matches(&fact.identity));
        self.spans.retain(|fact| !scope.matches(&fact.identity));
        self.meta.retain(|fact| !scope.matches(&fact.identity));
        self.concerns.retain(|fact| !scope.matches(&fact.identity));
        self.visibility.retain(|key, _| !scope.matches_key(key));
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
        self.visibility
            .retain(|key, _| !scope.matches_native_key(key, native_ids));
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VisibilityKey {
    corpus: CorpusId,
    source: SourceName,
    native_id: NativeId,
}

impl VisibilityKey {
    fn new(corpus: CorpusId, source: SourceName, native_id: NativeId) -> Self {
        Self {
            corpus,
            source,
            native_id,
        }
    }

    fn from_identity(identity: &FactIdentity) -> Self {
        Self::new(
            identity.corpus.clone(),
            identity.source.clone(),
            identity.native_id.clone(),
        )
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
        let mut fact_native_ids = BTreeSet::new();
        let collect_native_ids = matches!(batch.mode, FactBatchMode::Delta);
        for identity in all_identities(batch) {
            if !scope.matches(identity) {
                return Err(StoreError::MixedSourceBatch);
            }
            if identity.generation != batch.generation {
                return Err(StoreError::MismatchedGeneration);
            }
            fact_native_ids.insert(identity.native_id.clone());
            if collect_native_ids {
                native_ids.insert(identity.native_id.clone());
            }
        }
        if batch
            .visibility
            .keys()
            .any(|native_id| !fact_native_ids.contains(native_id))
        {
            return Err(StoreError::VisibilityWithoutFact);
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

    fn matches_key(&self, key: &VisibilityKey) -> bool {
        key.corpus == self.corpus && key.source == self.source
    }

    fn matches_native(&self, identity: &FactIdentity, native_ids: &BTreeSet<NativeId>) -> bool {
        self.matches(identity) && native_ids.contains(&identity.native_id)
    }

    fn matches_native_key(&self, key: &VisibilityKey, native_ids: &BTreeSet<NativeId>) -> bool {
        self.matches_key(key) && native_ids.contains(&key.native_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    MixedSourceBatch,
    MismatchedGeneration,
    VisibilityWithoutFact,
    MixedConfigCorpus,
    MixedSnapshotCorpus,
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
            Self::VisibilityWithoutFact => {
                f.write_str("FactBatch visibility references a native id without an emitted fact")
            }
            Self::MixedConfigCorpus => f.write_str("config facts contain multiple corpus scopes"),
            Self::MixedSnapshotCorpus => {
                f.write_str("snapshot facts contain multiple corpus scopes")
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

fn compare_identity(left: &FactIdentity, right: &FactIdentity) -> Ordering {
    left.corpus
        .cmp(&right.corpus)
        .then_with(|| left.source.cmp(&right.source))
        .then_with(|| left.native_id.cmp(&right.native_id))
        .then_with(|| left.origin_uri.cmp(&right.origin_uri))
        .then_with(|| left.revision.cmp(&right.revision))
        .then_with(|| left.generation.cmp(&right.generation))
}

fn compare_handle_facts(left: &HandleFact, right: &HandleFact) -> Ordering {
    left.id
        .cmp(&right.id)
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.namespace.cmp(&right.namespace))
        .then_with(|| left.file.cmp(&right.file))
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.status.cmp(&right.status))
        .then_with(|| left.date.cmp(&right.date))
        .then_with(|| left.area.cmp(&right.area))
        .then_with(|| left.summary.cmp(&right.summary))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn compare_edge_facts(left: &EdgeFact, right: &EdgeFact) -> Ordering {
    left.from
        .cmp(&right.from)
        .then_with(|| left.to.cmp(&right.to))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.file.cmp(&right.file))
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn compare_content_facts(left: &ContentFact, right: &ContentFact) -> Ordering {
    left.handle
        .cmp(&right.handle)
        .then_with(|| left.span_id.cmp(&right.span_id))
        .then_with(|| left.lines.cmp(&right.lines))
        .then_with(|| left.tokens.cmp(&right.tokens))
        .then_with(|| left.text.cmp(&right.text))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn compare_span_facts(left: &SpanFact, right: &SpanFact) -> Ordering {
    left.id
        .cmp(&right.id)
        .then_with(|| left.handle.cmp(&right.handle))
        .then_with(|| left.start_line.cmp(&right.start_line))
        .then_with(|| left.end_line.cmp(&right.end_line))
        .then_with(|| left.summary.cmp(&right.summary))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn compare_meta_facts(left: &MetaFact, right: &MetaFact) -> Ordering {
    left.handle
        .cmp(&right.handle)
        .then_with(|| left.key.cmp(&right.key))
        .then_with(|| left.value.cmp(&right.value))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn compare_concern_facts(left: &ConcernFact, right: &ConcernFact) -> Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.member.cmp(&right.member))
        .then_with(|| compare_identity(&left.identity, &right.identity))
}

fn sort_configs(configs: &mut [ConfigFact]) {
    configs.sort_by(|left, right| {
        left.corpus
            .cmp(&right.corpus)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.ordinal.cmp(&right.ordinal))
            .then_with(|| left.value.cmp(&right.value))
    });
}

fn sort_snapshots(snapshots: &mut [SnapshotFact]) {
    snapshots.sort_by(|left, right| {
        left.corpus
            .cmp(&right.corpus)
            .then_with(|| left.snapshot.cmp(&right.snapshot))
            .then_with(|| left.at.cmp(&right.at))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.value.cmp(&right.value))
    });
}

fn compare_generation_facts(left: &GenerationFact, right: &GenerationFact) -> Ordering {
    left.corpus
        .cmp(&right.corpus)
        .then_with(|| left.source.cmp(&right.source))
        .then_with(|| left.current.cmp(&right.current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{
        ConcernFact, ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
        HandleFact, MetaFact, SpanFact,
    };
    use crate::history::{SnapshotEntry, SnapshotEntryFact};
    use crate::ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
    use crate::runtime::prelude::standard_prelude_set;
    use crate::visibility::FactVisibility;

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
        meta_with_value(native_id, generation, key, "value")
    }

    fn meta_with_value(
        native_id: &str,
        generation: Generation,
        key: &str,
        value: &str,
    ) -> MetaFact {
        MetaFact {
            identity: identity(native_id, generation),
            handle: native_id.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn edge(from: &str, to: &str, generation: Generation) -> EdgeFact {
        EdgeFact {
            identity: identity(from, generation),
            from: from.to_string(),
            to: to.to_string(),
            kind: "DependsOn".to_string(),
            file: from.to_string(),
            line: 1,
        }
    }

    fn content(native_id: &str, span_id: &str, generation: Generation) -> ContentFact {
        ContentFact {
            identity: identity(native_id, generation),
            handle: native_id.to_string(),
            span_id: span_id.to_string(),
            lines: 1,
            text: format!("content for {native_id}"),
            tokens: 3,
        }
    }

    fn span(native_id: &str, span_id: &str, generation: Generation) -> SpanFact {
        SpanFact {
            identity: identity(native_id, generation),
            id: span_id.to_string(),
            handle: native_id.to_string(),
            start_line: 1,
            end_line: 2,
            summary: format!("span for {native_id}"),
        }
    }

    fn concern(name: &str, member: &str, generation: Generation) -> ConcernFact {
        ConcernFact {
            identity: identity(member, generation),
            name: name.to_string(),
            member: member.to_string(),
        }
    }

    fn config(corpus: &str, key: &str, value: &str) -> ConfigFact {
        config_with_ordinal(corpus, key, value, None)
    }

    fn config_with_ordinal(
        corpus: &str,
        key: &str,
        value: &str,
        ordinal: Option<u32>,
    ) -> ConfigFact {
        ConfigFact {
            corpus: CorpusId::from(corpus),
            key: key.to_string(),
            value: value.to_string(),
            ordinal,
        }
    }

    fn snapshot(corpus: &str, snapshot: &str, id: &str) -> SnapshotFact {
        snapshot_with_key(corpus, snapshot, "2026-05-13", id, "status", "draft")
    }

    fn snapshot_with_key(
        corpus: &str,
        snapshot: &str,
        at: &str,
        id: &str,
        key: &str,
        value: &str,
    ) -> SnapshotFact {
        SnapshotFact {
            corpus: CorpusId::from(corpus),
            snapshot: snapshot.to_string(),
            at: at.to_string(),
            id: id.to_string(),
            key: key.to_string(),
            value: value.to_string(),
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
    fn merge_canonicalizes_relation_vectors() {
        let mut store = FactStore::default();
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        batch
            .handles
            .push(handle("b.md", Generation::new(1), "draft"));
        batch
            .handles
            .push(handle("a.md", Generation::new(1), "draft"));
        batch.edges.push(edge("b.md", "a.md", Generation::new(1)));
        batch.edges.push(edge("a.md", "b.md", Generation::new(1)));
        batch
            .content
            .push(content("b.md", "b.md#body", Generation::new(1)));
        batch
            .content
            .push(content("a.md", "a.md#body", Generation::new(1)));
        batch
            .spans
            .push(span("b.md", "b.md#body", Generation::new(1)));
        batch
            .spans
            .push(span("a.md", "a.md#body", Generation::new(1)));
        batch.meta.push(meta("b.md", Generation::new(1), "purpose"));
        batch.meta.push(meta("a.md", Generation::new(1), "purpose"));
        batch
            .meta
            .push(meta_with_value("a.md", Generation::new(1), "zeta", "2"));
        batch
            .meta
            .push(meta_with_value("a.md", Generation::new(1), "alpha", "1"));
        batch
            .concerns
            .push(concern("runtime", "b.md", Generation::new(1)));
        batch
            .concerns
            .push(concern("runtime", "a.md", Generation::new(1)));

        store.merge(batch).expect("merge");

        assert_eq!(
            store
                .handles()
                .iter()
                .map(|fact| fact.id.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        assert_eq!(
            store
                .edges()
                .iter()
                .map(|fact| fact.from.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        assert_eq!(
            store
                .content()
                .iter()
                .map(|fact| fact.handle.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        assert_eq!(
            store
                .spans()
                .iter()
                .map(|fact| fact.handle.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        assert_eq!(
            store
                .meta()
                .iter()
                .map(|fact| (fact.handle.as_str(), fact.key.as_str(), fact.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("a.md", "alpha", "1"),
                ("a.md", "purpose", "value"),
                ("a.md", "zeta", "2"),
                ("b.md", "purpose", "value"),
            ]
        );
        assert_eq!(
            store
                .concerns()
                .iter()
                .map(|fact| fact.member.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
    }

    #[test]
    fn runtime_owned_relations_are_canonicalized_on_replace() {
        let mut store = FactStore::default();

        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![
                    config_with_ordinal("test", "convergence.ordering", "stable", Some(2)),
                    config_with_ordinal("test", "convergence.ordering", "draft", Some(1)),
                    config("test", "convergence.active", "draft"),
                ],
            )
            .expect("replace configs");
        assert_eq!(
            store
                .configs()
                .iter()
                .map(|fact| (fact.key.as_str(), fact.ordinal, fact.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("convergence.active", None, "draft"),
                ("convergence.ordering", Some(1), "draft"),
                ("convergence.ordering", Some(2), "stable"),
            ]
        );

        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![
                    snapshot_with_key("test", "s2", "2026-05-14", "b.md", "status", "stable"),
                    snapshot_with_key("test", "s1", "2026-05-13", "b.md", "status", "draft"),
                    snapshot_with_key("test", "s1", "2026-05-13", "a.md", "status", "draft"),
                ],
            )
            .expect("replace snapshots");
        assert_eq!(
            store
                .snapshots()
                .iter()
                .map(|fact| {
                    (
                        fact.snapshot.as_str(),
                        fact.at.as_str(),
                        fact.id.as_str(),
                        fact.key.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                ("s1", "2026-05-13", "a.md", "status"),
                ("s1", "2026-05-13", "b.md", "status"),
                ("s2", "2026-05-14", "b.md", "status"),
            ]
        );
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
    fn visibility_envelope_tracks_source_generation_swaps() {
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
        first
            .handles
            .push(handle("b.md", Generation::new(1), "draft"));
        first.set_visibility(NativeId::from("a.md"), FactVisibility::Private);
        store.merge(first).expect("merge first");

        assert_eq!(
            store.visibility_for(&identity("a.md", Generation::new(1))),
            FactVisibility::Private
        );
        assert_eq!(
            store.visibility_for(&identity("b.md", Generation::new(1))),
            FactVisibility::Public
        );

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

        assert_eq!(
            store.visibility_for(&identity("a.md", Generation::new(2))),
            FactVisibility::Public
        );
    }

    #[test]
    fn visibility_envelope_requires_an_emitted_fact() {
        let mut store = FactStore::default();
        let mut batch = FactBatch::new(
            CorpusId::from("test"),
            SourceName::from("test-source"),
            FactBatchMode::FullSnapshot,
            Generation::new(1),
        );
        batch.set_visibility(NativeId::from("missing.md"), FactVisibility::Private);

        let err = store.merge(batch).expect_err("orphan visibility rejected");

        assert_eq!(err, StoreError::VisibilityWithoutFact);
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

    #[test]
    fn replace_configs_updates_one_corpus_scope() {
        let mut store = FactStore::default();
        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![config("test", "convergence.ordering", "draft")],
            )
            .expect("initial config replace");
        store
            .replace_configs(
                &CorpusId::from("other"),
                vec![config("other", "convergence.ordering", "raw")],
            )
            .expect("other config replace");
        store
            .replace_configs(
                &CorpusId::from("test"),
                vec![config("test", "convergence.ordering", "current")],
            )
            .expect("second config replace");

        assert_eq!(store.configs().len(), 2);
        assert!(
            store
                .configs()
                .iter()
                .any(|fact| fact.corpus == CorpusId::from("test") && fact.value == "current")
        );
        assert!(
            store
                .configs()
                .iter()
                .any(|fact| fact.corpus == CorpusId::from("other") && fact.value == "raw")
        );
    }

    #[test]
    fn replace_configs_rejects_mixed_corpus_rows() {
        let mut store = FactStore::default();
        let err = store
            .replace_configs(
                &CorpusId::from("test"),
                vec![config("other", "convergence.ordering", "raw")],
            )
            .expect_err("mixed config corpus rejected");
        assert_eq!(err, StoreError::MixedConfigCorpus);
    }

    #[test]
    fn replace_snapshots_updates_one_corpus_scope() {
        let mut store = FactStore::default();
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot("test", "s1", "a.md")],
            )
            .expect("initial snapshot replace");
        store
            .replace_snapshots(
                &CorpusId::from("other"),
                vec![snapshot("other", "s1", "b.md")],
            )
            .expect("other snapshot replace");
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot("test", "s2", "c.md")],
            )
            .expect("second snapshot replace");

        assert_eq!(store.snapshots().len(), 2);
        assert!(
            store
                .snapshots()
                .iter()
                .any(|fact| fact.corpus == CorpusId::from("test") && fact.snapshot == "s2")
        );
        assert!(
            store
                .snapshots()
                .iter()
                .any(|fact| fact.corpus == CorpusId::from("other") && fact.id == "b.md")
        );
    }

    #[test]
    fn replace_snapshots_rejects_mixed_corpus_rows() {
        let mut store = FactStore::default();
        let err = store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot("other", "s1", "a.md")],
            )
            .expect_err("mixed snapshot corpus rejected");
        assert_eq!(err, StoreError::MixedSnapshotCorpus);
    }

    #[test]
    fn replace_snapshot_history_loads_all_history_corpora() {
        let mut store = FactStore::default();
        store
            .replace_snapshots(
                &CorpusId::from("test"),
                vec![snapshot("test", "old", "old.md")],
            )
            .expect("initial snapshots");
        let history = SnapshotHistory::from_entries(vec![
            SnapshotEntry::new(
                "s1",
                "2026-05-13",
                CorpusId::from("test"),
                standard_prelude_set(),
                vec![SnapshotEntryFact::new("a.md", "status", "draft")],
            ),
            SnapshotEntry::new(
                "s1",
                "2026-05-13",
                CorpusId::from("other"),
                standard_prelude_set(),
                vec![SnapshotEntryFact::new("b.md", "status", "current")],
            ),
        ]);

        store.replace_snapshot_history(&history);

        assert_eq!(store.snapshots().len(), 2);
        assert!(!store.snapshots().iter().any(|fact| fact.snapshot == "old"));
        assert!(
            store
                .snapshots()
                .iter()
                .any(|fact| fact.corpus == CorpusId::from("other") && fact.id == "b.md")
        );
    }
}
