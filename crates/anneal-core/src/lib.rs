//! Core substrate contracts for anneal v2.
//!
//! This crate owns source-agnostic runtime types: source extraction,
//! stored relation facts, actor/cancellation context, and generation
//! merge semantics. Adapters depend on this crate; this crate must not
//! depend on any adapter.

pub mod driver;
pub mod facts;
pub mod hash;
pub mod history;
pub mod ids;
pub mod policy;
pub mod project;
pub mod ranking;
pub mod retrieval;
pub mod runtime;
pub mod runtime_config;
pub mod source;
pub mod store;
mod time;
pub mod trail;
pub mod verbs;
pub mod visibility;

pub use driver::{
    OneShotSourceDriver, SourceDriver, SourceDriverError, SourceRefreshReport,
    SourceRefreshRequest, refresh_source,
};
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
pub use policy::{
    Action, ActionKind, AllowAllPolicy, AuthorizationError, Policy, PolicyDecision,
    authorize_action, authorize_capability_action, authorize_trail_private,
};
pub use project::{
    PROJECT_RULE_FILE, ProjectExtension, ProjectLoadError, ShadowWarning, load_project_extension,
    merge_program_layers,
};
pub use ranking::{
    DefaultRanker, REASON_PARENT_CLUSTER, Ranker, RankingContext, SearchHit, SearchScore,
    default_lexical_search_info,
};
pub use retrieval::{
    ContentProvider, ReadChunk, ReadChunkParts, ReadContext, ReadError, ReadFullContent,
    ReadFullRequest, ReadRequest, RetrievalContext, SearchContext, SearchError, SearchProvider,
    SearchRequest, SearchSpanScope,
};
pub use runtime_config::{
    RuntimeConfigError, load_runtime_configs, load_runtime_configs_if_present,
};
pub use source::{
    ActorCapability, ActorContext, CancellationToken, ConfigEntry, ConfigFacts, ConfigKey, Pattern,
    RuntimeCapability, SearchInfo, Source, SourceCapabilities, SourceContext, SourceError,
    SourceInfo, TimeRef,
};
pub use store::{FactStore, GenerationFact, StoreError};
pub use trail::{
    DEFAULT_TRAIL_QUERY_LIMIT, DefaultTrailRecorder, DefaultTrailRedactor, DefaultTrailSummarizer,
    JsonlTrailStore, TrailContext, TrailEntryInProgress, TrailEntryRedacted, TrailError,
    TrailGeneration, TrailQuery, TrailRecorder, TrailRedactor, TrailRefKind, TrailReference,
    TrailSessionId, TrailSessionIdError, TrailStore, TrailSummarizer, TrailSummary,
    summarize_trail_session,
};
pub use verbs::{
    VerbBuiltinPermission, VerbCapability, VerbDispatchError, VerbEntry, VerbLayer, VerbName,
    VerbRegistry, VerbRegistryError, VerbRunPlan, VerbSource,
};
pub use visibility::FactVisibility;
