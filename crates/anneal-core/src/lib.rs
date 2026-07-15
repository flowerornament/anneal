//! Core substrate contracts for anneal.
//!
//! This crate owns source-agnostic runtime types: source extraction,
//! stored relation facts, actor/cancellation context, and generation
//! merge semantics. Adapters depend on this crate; this crate must not
//! depend on any adapter.

pub mod config_schema;
pub mod driver;
pub mod facts;
pub mod hash;
pub mod history;
pub mod ids;
pub(crate) mod ir;
pub mod lifecycle;
pub mod metadata;
pub mod path_policy;
pub mod policy;
pub mod project;
pub mod ranking;
pub mod retrieval;
pub mod runtime;
pub mod source;
pub mod store;
pub mod target_probe;
mod time;
pub mod trail;
pub mod verbs;
pub mod visibility;
pub(crate) mod vm;

pub use config_schema::{
    RUNTIME_CONFIG_DECLARATIONS, RuntimeConfigDeclaration, RuntimeConfigEntryError,
    RuntimeConfigKey, RuntimeConfigLifecycle, RuntimeConfigValueMode,
    runtime_config_declaration_by_key, runtime_config_declaration_for,
};
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
    HistoryError, HistoryWarning, SnapshotAppendOutcome, SnapshotEntry, SnapshotEntryFact,
    SnapshotHistory, append_snapshot_entry, append_snapshot_entry_capped, read_snapshot_history,
    repo_history_path,
};
pub use ids::{CorpusId, Generation, NativeId, OriginUri, Revision, SourceName};
pub use lifecycle::is_terminal_status;
pub use metadata::CodeTargetMeta;
pub use path_policy::{RelativePathPolicy, normalize_path_inside_root, normalize_relative_path};
pub use policy::{
    Action, ActionKind, AllowAllPolicy, AuthorizationError, Policy, PolicyDecision,
    authorize_action, authorize_capability_action, authorize_trail_private,
};
pub use project::{
    InferredCorpusRoot, PROJECT_RULE_FILE, ProjectExtension, ProjectLoadError, ShadowWarning,
    infer_corpus_root, load_project_extension, merge_program_layers,
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
pub use source::{
    ActorCapability, ActorContext, CancellationToken, ConfigEntry, ConfigFacts, ConfigKey,
    ConfigValueShape, Pattern, RuntimeCapability, SearchInfo, Source, SourceCapabilities,
    SourceContext, SourceError, SourceInfo, TimeRef,
};
pub use store::{FactStore, GenerationFact, StoreError};
pub use target_probe::{
    CodeDriftRefreshProgress, CodeDriftRefreshProgressSink, CodeTargetProbe, CodeTargetProbeCache,
    TargetExistence, TargetHistoryStatus, enclosing_project_root, probe_code_target,
};
pub use trail::{
    DEFAULT_TRAIL_QUERY_LIMIT, DefaultTrailRecorder, DefaultTrailRedactor, DefaultTrailSummarizer,
    JsonlTrailStore, TrailContext, TrailEntryInProgress, TrailEntryRedacted, TrailError,
    TrailGeneration, TrailQuery, TrailRecorder, TrailRedactor, TrailRefKind, TrailReference,
    TrailSessionId, TrailSessionIdError, TrailStore, TrailSummarizer, TrailSummary,
    summarize_trail_session,
};
pub use verbs::{
    VerbArg, VerbArgKind, VerbBuiltinPermission, VerbCapability, VerbDispatchError, VerbEntry,
    VerbLayer, VerbName, VerbRegistry, VerbRegistryError, VerbRunPlan, VerbSource,
    render_verb_arg_fact, render_verb_arg_facts, validate_project_verb_query_program,
};
pub use visibility::FactVisibility;
