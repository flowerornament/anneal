//! Trail capture, redaction, and persistence contracts.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::facts::StoredRelationDescriptor;
use crate::hash::fnv1a_64;
use crate::ids::{CorpusId, Generation, SourceName};
use crate::policy::{AllowAllPolicy, AuthorizationError, Policy, authorize_trail_private};
use crate::source::ActorContext;
use crate::visibility::FactVisibility;

static DEFAULT_TRAIL_POLICY: AllowAllPolicy = AllowAllPolicy;
pub const DEFAULT_TRAIL_QUERY_LIMIT: usize = 1_000;
pub const TRAIL_RELATION: &str = "trail";
pub const TRAIL_REF_RELATION: &str = "trail_ref";
pub const TRAIL_GENERATION_RELATION: &str = "trail_generation";

pub(crate) const TRAIL_RELATION_DESCRIPTORS: &[StoredRelationDescriptor] = &[
    StoredRelationDescriptor {
        name: TRAIL_RELATION,
        fields: &[
            "session_id",
            "step",
            "timestamp",
            "actor",
            "corpus",
            "verb",
            "redacted_expr",
            "input_hash",
            "prelude_hash",
            "visibility",
            "retention",
        ],
        doc: "Runtime-populated redacted trail audit rows for agent/session paths.",
        provenance: "runtime",
    },
    StoredRelationDescriptor {
        name: TRAIL_REF_RELATION,
        fields: &[
            "session_id",
            "step",
            "kind",
            "ordinal",
            "corpus",
            "source",
            "handle",
            "span_id",
            "score",
        ],
        doc: "Runtime-populated normalized surfaced and consumed references for trail steps.",
        provenance: "runtime",
    },
    StoredRelationDescriptor {
        name: TRAIL_GENERATION_RELATION,
        fields: &["session_id", "step", "corpus", "source", "generation"],
        doc: "Runtime-populated source generation stamps for trail steps.",
        provenance: "runtime",
    },
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TrailSessionId(String);

impl TrailSessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, TrailSessionIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(TrailSessionIdError::Empty);
        }
        if value == "." || value == ".." {
            return Err(TrailSessionIdError::Reserved(value));
        }
        if value.len() > 128 {
            return Err(TrailSessionIdError::TooLong { len: value.len() });
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(TrailSessionIdError::InvalidCharacters(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TrailSessionId {
    type Error = TrailSessionIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TrailSessionId> for String {
    fn from(value: TrailSessionId) -> Self {
        value.0
    }
}

impl std::fmt::Display for TrailSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TrailSessionIdError {
    #[error("trail session id cannot be empty")]
    Empty,
    #[error("trail session id {0:?} is reserved")]
    Reserved(String),
    #[error("trail session id is too long: {len} bytes")]
    TooLong { len: usize },
    #[error("trail session id contains invalid characters: {0:?}")]
    InvalidCharacters(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailRefKind {
    Surfaced,
    Consumed,
}

impl TrailRefKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Surfaced => "surfaced",
            Self::Consumed => "consumed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailReference {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub handle: String,
    pub span_id: Option<String>,
    pub score: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailGeneration {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub generation: Generation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailEntryInProgress {
    pub session_id: TrailSessionId,
    pub step: u64,
    pub timestamp: String,
    pub corpus: CorpusId,
    pub verb: String,
    pub expr: String,
    pub surfaced_refs: Vec<TrailReference>,
    pub consumed_refs: Vec<TrailReference>,
    pub prelude_hash: String,
    pub source_generations: Vec<TrailGeneration>,
    pub retention: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailEntryRedacted {
    pub session_id: TrailSessionId,
    pub step: u64,
    pub timestamp: String,
    pub actor: String,
    pub corpus: CorpusId,
    pub verb: String,
    pub redacted_expr: String,
    pub input_hash: String,
    pub surfaced_refs: Vec<TrailReference>,
    pub consumed_refs: Vec<TrailReference>,
    pub prelude_hash: String,
    pub source_generations: Vec<TrailGeneration>,
    pub visibility: FactVisibility,
    pub retention: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrailQuery {
    pub session_id: Option<TrailSessionId>,
    pub include_private: bool,
    pub min_step: Option<u64>,
    pub max_step: Option<u64>,
    pub limit: usize,
}

impl Default for TrailQuery {
    fn default() -> Self {
        Self {
            session_id: None,
            include_private: false,
            min_step: None,
            max_step: None,
            limit: DEFAULT_TRAIL_QUERY_LIMIT,
        }
    }
}

impl TrailQuery {
    pub fn for_session(session_id: impl Into<String>) -> Result<Self, TrailSessionIdError> {
        Ok(Self {
            session_id: Some(TrailSessionId::new(session_id)?),
            include_private: false,
            min_step: None,
            max_step: None,
            limit: DEFAULT_TRAIL_QUERY_LIMIT,
        })
    }

    pub fn for_valid_session(session_id: TrailSessionId) -> Self {
        Self {
            session_id: Some(session_id),
            include_private: false,
            min_step: None,
            max_step: None,
            limit: DEFAULT_TRAIL_QUERY_LIMIT,
        }
    }

    pub const fn include_private(mut self, include_private: bool) -> Self {
        self.include_private = include_private;
        self
    }

    pub const fn with_step_window(mut self, min_step: Option<u64>, max_step: Option<u64>) -> Self {
        self.min_step = min_step;
        self.max_step = max_step;
        self
    }

    pub const fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailSummary {
    pub session_id: TrailSessionId,
    pub steps: usize,
    pub consumed_refs: usize,
}

pub struct TrailContext<'a> {
    actor: &'a ActorContext,
    policy: &'a dyn Policy,
    visibility: FactVisibility,
}

impl<'a> TrailContext<'a> {
    pub const fn new(actor: &'a ActorContext, policy: &'a dyn Policy) -> Self {
        Self {
            actor,
            policy,
            visibility: FactVisibility::Private,
        }
    }

    pub const fn actor(&self) -> &'a ActorContext {
        self.actor
    }

    pub const fn policy(&self) -> &'a dyn Policy {
        self.policy
    }

    pub const fn visibility(&self) -> FactVisibility {
        self.visibility
    }

    pub const fn with_visibility(mut self, visibility: FactVisibility) -> Self {
        self.visibility = visibility;
        self
    }
}

impl<'a> From<&'a ActorContext> for TrailContext<'a> {
    fn from(actor: &'a ActorContext) -> Self {
        Self::new(actor, &DEFAULT_TRAIL_POLICY)
    }
}

pub trait TrailRecorder {
    fn record(&self, entry: TrailEntryInProgress, ctx: &TrailContext<'_>)
    -> Result<(), TrailError>;

    fn note_consumed(
        &self,
        session_id: &TrailSessionId,
        step: u64,
        reference: TrailReference,
        ctx: &TrailContext<'_>,
    ) -> Result<(), TrailError>;
}

pub trait TrailRedactor {
    fn redact(&self, entry: TrailEntryInProgress, ctx: &TrailContext<'_>) -> TrailEntryRedacted;
}

pub trait TrailSummarizer {
    fn summarize(
        &self,
        session_id: &TrailSessionId,
        entries: &[TrailEntryRedacted],
        ctx: &TrailContext<'_>,
    ) -> TrailSummary;
}

pub trait TrailStore {
    fn append(&self, entry: TrailEntryRedacted, ctx: &TrailContext<'_>) -> Result<(), TrailError>;

    fn query(
        &self,
        request: TrailQuery,
        ctx: &TrailContext<'_>,
    ) -> Result<Vec<TrailEntryRedacted>, TrailError>;
}

pub fn summarize_trail_session(
    store: &dyn TrailStore,
    session_id: &TrailSessionId,
    include_private: bool,
    summarizer: &dyn TrailSummarizer,
    ctx: &TrailContext<'_>,
) -> Result<TrailSummary, TrailError> {
    let request =
        TrailQuery::for_valid_session(session_id.clone()).include_private(include_private);
    let entries = store.query(request, ctx)?;
    Ok(summarizer.summarize(session_id, &entries, ctx))
}

#[derive(Clone, Debug)]
pub struct DefaultTrailRedactor {
    sensitive_patterns: Vec<String>,
}

impl Default for DefaultTrailRedactor {
    fn default() -> Self {
        Self {
            sensitive_patterns: vec![
                "secret".to_string(),
                "password".to_string(),
                "customer".to_string(),
            ],
        }
    }
}

impl DefaultTrailRedactor {
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            sensitive_patterns: patterns
                .into_iter()
                .map(Into::into)
                .map(|pattern| pattern.to_ascii_lowercase())
                .collect(),
        }
    }
}

impl TrailRedactor for DefaultTrailRedactor {
    fn redact(&self, entry: TrailEntryInProgress, ctx: &TrailContext<'_>) -> TrailEntryRedacted {
        let mut redacted_expr = redact_string_literals(&entry.expr);
        let lower = redacted_expr.to_ascii_lowercase();
        if self
            .sensitive_patterns
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            redacted_expr = "<redacted>".to_string();
        }
        TrailEntryRedacted {
            session_id: entry.session_id,
            step: entry.step,
            timestamp: entry.timestamp,
            actor: ctx.actor().actor.clone(),
            corpus: entry.corpus,
            verb: entry.verb,
            redacted_expr,
            input_hash: stable_input_hash(&entry.expr),
            surfaced_refs: entry.surfaced_refs,
            consumed_refs: entry.consumed_refs,
            prelude_hash: entry.prelude_hash,
            source_generations: entry.source_generations,
            visibility: ctx.visibility(),
            retention: entry.retention,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DefaultTrailRecorder<R, S> {
    redactor: R,
    store: S,
}

impl<R, S> DefaultTrailRecorder<R, S> {
    pub const fn new(redactor: R, store: S) -> Self {
        Self { redactor, store }
    }
}

impl<R, S> TrailRecorder for DefaultTrailRecorder<R, S>
where
    R: TrailRedactor,
    S: TrailStore,
{
    fn record(
        &self,
        entry: TrailEntryInProgress,
        ctx: &TrailContext<'_>,
    ) -> Result<(), TrailError> {
        let redacted = self.redactor.redact(entry, ctx);
        self.store.append(redacted, ctx)
    }

    fn note_consumed(
        &self,
        _session_id: &TrailSessionId,
        _step: u64,
        _reference: TrailReference,
        _ctx: &TrailContext<'_>,
    ) -> Result<(), TrailError> {
        Err(TrailError::Unsupported(
            "default trail recorder cannot update consumed refs without runtime integration",
        ))
    }
}

#[derive(Clone, Debug)]
pub struct DefaultTrailSummarizer;

impl TrailSummarizer for DefaultTrailSummarizer {
    fn summarize(
        &self,
        session_id: &TrailSessionId,
        entries: &[TrailEntryRedacted],
        _ctx: &TrailContext<'_>,
    ) -> TrailSummary {
        TrailSummary {
            session_id: session_id.clone(),
            steps: entries.len(),
            consumed_refs: entries
                .iter()
                .map(|entry| entry.consumed_refs.len())
                .sum::<usize>(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JsonlTrailStore {
    dir: Utf8PathBuf,
}

impl JsonlTrailStore {
    pub fn new(dir: impl Into<Utf8PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Utf8Path {
        &self.dir
    }

    fn session_path(&self, session_id: &TrailSessionId) -> Utf8PathBuf {
        self.dir.join(format!("{session_id}.jsonl"))
    }
}

impl TrailStore for JsonlTrailStore {
    fn append(&self, entry: TrailEntryRedacted, _ctx: &TrailContext<'_>) -> Result<(), TrailError> {
        fs::create_dir_all(&self.dir)?;
        let path = self.session_path(&entry.session_id);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let mut record = serde_json::to_vec(&entry)?;
        record.push(b'\n');
        file.write_all(&record)?;
        Ok(())
    }

    fn query(
        &self,
        request: TrailQuery,
        ctx: &TrailContext<'_>,
    ) -> Result<Vec<TrailEntryRedacted>, TrailError> {
        if request.include_private {
            authorize_trail_private(ctx.actor(), ctx.policy())?;
        }
        let mut entries = Vec::new();
        if request.limit == 0 {
            return Ok(entries);
        }
        for path in self.query_paths(request.session_id.as_ref())? {
            read_matching_jsonl_entries(&path, &request, ctx, &mut entries)?;
            if entries.len() >= request.limit {
                break;
            }
        }
        Ok(entries)
    }
}

impl JsonlTrailStore {
    fn query_paths(
        &self,
        session_id: Option<&TrailSessionId>,
    ) -> Result<Vec<Utf8PathBuf>, TrailError> {
        let Some(session_id) = session_id else {
            let Ok(entries) = fs::read_dir(&self.dir) else {
                return Ok(Vec::new());
            };
            let mut paths = Vec::new();
            for entry in entries {
                let path = Utf8PathBuf::from_path_buf(entry?.path())
                    .map_err(|path| TrailError::NonUtf8Path(path.display().to_string()))?;
                if path.extension() == Some("jsonl") {
                    paths.push(path);
                }
            }
            paths.sort();
            return Ok(paths);
        };
        let path = self.session_path(session_id);
        Ok(vec![path])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrailError {
    #[error("trail path is not utf-8: {0}")]
    NonUtf8Path(String),
    #[error("trail io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("trail json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
    #[error("invalid trail session id: {0}")]
    InvalidSessionId(#[from] TrailSessionIdError),
    #[error("unsupported trail operation: {0}")]
    Unsupported(&'static str),
}

fn read_matching_jsonl_entries(
    path: &Utf8Path,
    request: &TrailQuery,
    ctx: &TrailContext<'_>,
    entries: &mut Vec<TrailEntryRedacted>,
) -> Result<(), TrailError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(TrailError::Io(err)),
    };
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str(&line)?;
        if trail_entry_matches_request(&entry, request, ctx) {
            entries.push(entry);
        }
        if entries.len() >= request.limit {
            break;
        }
    }
    Ok(())
}

fn trail_entry_matches_request(
    entry: &TrailEntryRedacted,
    request: &TrailQuery,
    ctx: &TrailContext<'_>,
) -> bool {
    if request
        .session_id
        .as_ref()
        .is_some_and(|id| entry.session_id != *id)
    {
        return false;
    }
    if request.min_step.is_some_and(|min| entry.step < min) {
        return false;
    }
    if request.max_step.is_some_and(|max| entry.step > max) {
        return false;
    }
    match entry.visibility {
        FactVisibility::Public => true,
        FactVisibility::Team => ctx.actor().can_see_fact_visibility(FactVisibility::Team),
        FactVisibility::Private => {
            request.include_private && ctx.actor().can_see_fact_visibility(FactVisibility::Private)
        }
    }
}

fn stable_input_hash(input: &str) -> String {
    format!("{:016x}", fnv1a_64(input.as_bytes()))
}

fn redact_string_literals(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut emitted_marker = false;
    for ch in input.chars() {
        if !in_string {
            output.push(ch);
            if ch == '"' {
                in_string = true;
                escaped = false;
                emitted_marker = false;
            }
            continue;
        }

        if !emitted_marker {
            output.push_str("<redacted>");
            emitted_marker = true;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            output.push(ch);
            in_string = false;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::source::RuntimeCapability;

    fn session_id() -> TrailSessionId {
        TrailSessionId::new("session-1").expect("valid session id")
    }

    fn trail_entry(expr: &str) -> TrailEntryInProgress {
        TrailEntryInProgress {
            session_id: session_id(),
            step: 1,
            timestamp: "2026-05-16T00:00:00Z".to_string(),
            corpus: CorpusId::from("test"),
            verb: "-e".to_string(),
            expr: expr.to_string(),
            surfaced_refs: vec![TrailReference {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                handle: "alpha.md".to_string(),
                span_id: Some("body".to_string()),
                score: Some(0.9),
            }],
            consumed_refs: Vec::new(),
            prelude_hash: "prelude".to_string(),
            source_generations: vec![TrailGeneration {
                corpus: CorpusId::from("test"),
                source: SourceName::from("md"),
                generation: Generation::initial(),
            }],
            retention: Some("P30D".to_string()),
        }
    }

    #[test]
    fn trail_session_id_rejects_path_escape_names() {
        assert!(TrailSessionId::new("../outside").is_err());
        assert!(TrailSessionId::new("/tmp/session").is_err());
        assert!(TrailSessionId::new("..").is_err());
        assert!(TrailSessionId::new("session-1").is_ok());
    }

    #[test]
    fn default_redactor_removes_string_literals_and_hashes_raw_input() {
        let actor = ActorContext::trusted_cli();
        let ctx = TrailContext::from(&actor);
        let redactor = DefaultTrailRedactor::default();
        let redacted = redactor.redact(trail_entry(r#"? secret("hunter2", "customer-7")."#), &ctx);

        assert_eq!(redacted.redacted_expr, "<redacted>");
        assert_eq!(redacted.actor, "anonymous-cli");
        assert_eq!(redacted.visibility, FactVisibility::Private);
        assert!(!redacted.redacted_expr.contains("hunter2"));
        assert!(!redacted.redacted_expr.contains("customer-7"));
        assert_eq!(
            redacted.input_hash,
            stable_input_hash(r#"? secret("hunter2", "customer-7")."#)
        );
    }

    #[test]
    fn recorder_persists_redacted_entries_by_default() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let recorder = DefaultTrailRecorder::new(DefaultTrailRedactor::default(), store.clone());
        let actor = ActorContext::trusted_cli();
        let ctx = TrailContext::from(&actor).with_visibility(FactVisibility::Public);

        recorder
            .record(
                trail_entry(r#"? read("secret-token", 10, span, text)."#),
                &ctx,
            )
            .expect("trail record persists");

        let rows = store
            .query(
                TrailQuery::for_session("session-1").expect("valid query"),
                &ctx,
            )
            .expect("query persisted trail");
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].redacted_expr.contains("secret-token"));
        assert!(
            !fs::read_to_string(store.session_path(&session_id()))
                .expect("trail file")
                .contains("secret-token")
        );
    }

    #[test]
    fn trail_store_requires_hook_for_private_entries() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let cli_actor = ActorContext::trusted_cli();
        let cli_ctx = TrailContext::from(&cli_actor);
        store
            .append(
                DefaultTrailRedactor::default().redact(
                    trail_entry(r#"? read("visible", 10, span, text)."#),
                    &cli_ctx,
                ),
                &cli_ctx,
            )
            .expect("append private trail");

        let mcp_actor = ActorContext::anonymous_mcp();
        let mcp_ctx = TrailContext::from(&mcp_actor);
        assert!(
            store
                .query(
                    TrailQuery::for_session("session-1").expect("valid query"),
                    &mcp_ctx
                )
                .expect("public query skips private rows")
                .is_empty()
        );
        let err = store
            .query(
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .include_private(true),
                &mcp_ctx,
            )
            .expect_err("private trail read requires capability");
        assert!(matches!(
            err,
            TrailError::Authorization(AuthorizationError::CapabilityRequired { .. })
        ));

        let privileged_actor = ActorContext::anonymous_mcp()
            .with_runtime_capability(RuntimeCapability::TrailPrivate)
            .with_fact_visibility_capability(FactVisibility::Private);
        let privileged_ctx = TrailContext::from(&privileged_actor);
        let private_rows = store
            .query(
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .include_private(true),
                &privileged_ctx,
            )
            .expect("capable actor can query private trails");
        assert_eq!(private_rows.len(), 1);
    }

    #[test]
    fn default_recorder_reports_consumed_ref_updates_as_unsupported() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let recorder = DefaultTrailRecorder::new(DefaultTrailRedactor::default(), store);
        let actor = ActorContext::trusted_cli();
        let ctx = TrailContext::from(&actor);
        let err = recorder
            .note_consumed(
                &session_id(),
                1,
                TrailReference {
                    corpus: CorpusId::from("test"),
                    source: SourceName::from("md"),
                    handle: "alpha.md".to_string(),
                    span_id: None,
                    score: None,
                },
                &ctx,
            )
            .expect_err("default recorder does not silently drop consumed refs");

        assert!(matches!(err, TrailError::Unsupported(_)));
    }

    #[test]
    fn private_queries_authorize_before_reading_files() {
        let dir = tempdir().expect("tempdir");
        let trail_dir =
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8");
        fs::create_dir_all(&trail_dir).expect("create trail dir");
        fs::write(trail_dir.join("session-1.jsonl"), b"{not json}\n").expect("write bad json");
        let store = JsonlTrailStore::new(trail_dir);
        let actor = ActorContext::anonymous_mcp();
        let ctx = TrailContext::from(&actor);

        let err = store
            .query(
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .include_private(true),
                &ctx,
            )
            .expect_err("private read should fail before json parsing");

        assert!(matches!(
            err,
            TrailError::Authorization(AuthorizationError::CapabilityRequired { .. })
        ));
    }

    #[test]
    fn trail_query_streams_with_step_window_and_limit() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let actor = ActorContext::trusted_cli();
        let ctx = TrailContext::from(&actor).with_visibility(FactVisibility::Public);

        for step in 1..=3 {
            let mut entry = trail_entry(r"? work(h).");
            entry.step = step;
            store
                .append(DefaultTrailRedactor::default().redact(entry, &ctx), &ctx)
                .expect("append trail row");
        }

        let rows = store
            .query(
                TrailQuery::for_session("session-1")
                    .expect("valid query")
                    .with_step_window(Some(2), None)
                    .with_limit(1),
                &ctx,
            )
            .expect("query bounded trail");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].step, 2);
    }

    #[test]
    fn trail_store_filters_team_entries_by_actor_visibility() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let cli_actor = ActorContext::trusted_cli();
        let cli_ctx = TrailContext::from(&cli_actor).with_visibility(FactVisibility::Team);
        store
            .append(
                DefaultTrailRedactor::default().redact(trail_entry(r"? work(h)."), &cli_ctx),
                &cli_ctx,
            )
            .expect("append team trail");

        let mcp_actor = ActorContext::anonymous_mcp();
        let mcp_ctx = TrailContext::from(&mcp_actor);
        assert!(
            store
                .query(
                    TrailQuery::for_session("session-1").expect("valid query"),
                    &mcp_ctx
                )
                .expect("actor without team visibility cannot read team trail rows")
                .is_empty()
        );

        let team_actor =
            ActorContext::anonymous_mcp().with_fact_visibility_capability(FactVisibility::Team);
        let team_ctx = TrailContext::from(&team_actor);
        let rows = store
            .query(
                TrailQuery::for_session("session-1").expect("valid query"),
                &team_ctx,
            )
            .expect("actor with team visibility can read team trail rows");
        assert_eq!(rows.len(), 1);
    }
}
