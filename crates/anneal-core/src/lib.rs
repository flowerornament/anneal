//! Core substrate contracts for anneal v2.
//!
//! This crate owns source-agnostic runtime types: source extraction,
//! stored relation facts, actor/cancellation context, and generation
//! merge semantics. Adapters depend on this crate; this crate must not
//! depend on any adapter.

pub mod facts;
pub mod hash;
pub mod history;
pub mod ids;
pub mod runtime;
pub mod source;
pub mod store;
mod time;

pub use facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
    HandleFact, MetaFact, SnapshotFact, SpanFact,
};
pub use hash::fnv1a_64;
pub use history::{
    HistoryError, HistoryWarning, SnapshotEntry, SnapshotEntryFact, SnapshotHistory,
    append_snapshot_entry, read_snapshot_history, repo_history_path,
};
pub use ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
pub use source::{
    Action, ActorContext, CancellationToken, ConfigEntry, ConfigFacts, ConfigKey, Pattern,
    SearchInfo, Source, SourceCapabilities, SourceContext, SourceError, SourceInfo, TimeRef,
};
pub use store::{FactStore, GenerationFact, StoreError};
