//! Shared substrate and extension contracts for anneal.
//!
//! The crate root is the shared substrate and adapter/provider facade. It owns:
//!
//! - corpus, source, generation, and handle identities;
//! - stored facts, source extraction, refresh, and generation merge;
//! - retrieval, ranking, policy, trails, verbs, and shared persistence.
//!
//! Query hosts additionally use [`runtime`] for grammar, analysis, evaluation,
//! and row rendering. Root admission is transitive: a shared type may not
//! expose a host-only type through a method, field, associated type, or trait
//! bound. Each supported item has one canonical facade path; implementation
//! modules remain private.
//!
//! Errors stay phase-local. Adapters return [`SourceError`], refresh and store
//! transactions return [`SourceDriverError`] and [`StoreError`], and query
//! language failures are exposed through [`runtime`]. The full admission rule
//! and boundary fixtures are specified in
//! `.design/2026-07-29-anneal-core-public-api-altitude.md`.
//!
//! This crate must not depend on any adapter.

mod config_schema;
mod driver;
mod facts;
mod hash;
mod history;
mod ids;
mod impact;
pub(crate) mod ir;
mod lifecycle;
mod metadata;
mod path_policy;
mod policy;
mod project;
mod ranking;
mod repository;
mod retrieval;
pub mod runtime;
mod source;
mod store;
mod target_probe;
mod time;
mod trail;
mod verbs;
mod visibility;
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
    HandleFact, MetaFact, MetaRole, SpanFact,
};
pub use hash::fnv1a_64;
pub use ids::{
    CorpusId, Generation, HandleId, HandleIdError, NativeId, OriginUri, Revision, SourceName,
};
pub use impact::ImpactTraversalPolicy;
pub use lifecycle::is_terminal_status;
pub use metadata::CodeTargetMeta;
pub use path_policy::{RelativePathPolicy, normalize_path_inside_root, normalize_relative_path};
pub use policy::{
    Action, ActionKind, AllowAllPolicy, AuthorizationError, Policy, PolicyDecision,
    authorize_action, authorize_capability_action, authorize_trail_private,
};
pub use project::{
    InferredCorpusRoot, PROJECT_RULE_FILE, ProgramLayerError, ProjectExtension, ProjectLoadError,
    ShadowWarning, infer_corpus_root, load_project_extension, merge_program_layers,
};
pub use ranking::{
    DefaultRanker, REASON_PARENT_CLUSTER, Ranker, RankingContext, SearchHit, SearchScore,
    default_lexical_search_info,
};
pub use repository::{RepositoryContext, RepositoryOperation};
pub use retrieval::{
    ContentProvider, ReadChunk, ReadContext, ReadError, ReadFullContent, ReadFullRequest,
    ReadRequest, RetrievalContext, SearchContext, SearchError, SearchProvider, SearchRequest,
    SearchSpanScope,
};
pub use source::{
    ActorCapability, ActorContext, CancellationToken, ConfigEntry, ConfigFacts, ConfigKey,
    ConfigValueShape, DuplicateConfigOrdinal, Pattern, RuntimeCapability, SearchInfo, Source,
    SourceCapabilities, SourceContext, SourceError, SourceInfo, TimeRef,
};
pub use store::{DuplicateHandleIdConflict, FactStore, GenerationFact, StoreError};
pub use target_probe::{
    CodeDriftEvidence, CodeDriftEvidenceCache, CodeDriftEvidenceMode, CodeDriftEvidenceRequest,
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
    VerbArg, VerbArgKind, VerbArgValueError, VerbBuiltinPermission, VerbCapability,
    VerbDispatchError, VerbEntry, VerbLayer, VerbName, VerbRegistry, VerbRegistryError,
    VerbRunPlan, VerbSource, render_verb_arg_fact, render_verb_arg_facts,
    validate_project_verb_query_program,
};
pub use visibility::FactVisibility;
