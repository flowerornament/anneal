//! Core substrate contracts for anneal v2.
//!
//! This crate owns source-agnostic runtime types: source extraction,
//! stored relation facts, actor/cancellation context, and generation
//! merge semantics. Adapters depend on this crate; this crate must not
//! depend on any adapter.

pub mod facts;
pub mod ids;
pub mod source;
pub mod store;

pub use facts::{
    ConcernFact, ConfigFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity,
    HandleFact, MetaFact, SnapshotFact, SpanFact,
};
pub use ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
pub use source::{
    Action, ActorContext, CancellationToken, ConfigFacts, ConfigKey, Pattern, SearchInfo, Source,
    SourceCapabilities, SourceContext, SourceError, SourceInfo, TimeRef,
};
pub use store::{FactStore, GenerationFact, StoreError};
