//! Trail capture, redaction, and persistence contracts.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::hash::fnv1a_64;
use crate::ids::{CorpusId, Generation, SourceName};
use crate::policy::{AllowAllPolicy, AuthorizationError, Policy, authorize_trail_private};
use crate::source::ActorContext;
use crate::visibility::FactVisibility;

static DEFAULT_TRAIL_POLICY: AllowAllPolicy = AllowAllPolicy;

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
    pub session_id: String,
    pub step: u64,
    pub timestamp: String,
    pub actor: String,
    pub corpus: CorpusId,
    pub verb: String,
    pub expr: String,
    pub surfaced_refs: Vec<TrailReference>,
    pub consumed_refs: Vec<TrailReference>,
    pub prelude_hash: String,
    pub source_generations: Vec<TrailGeneration>,
    pub visibility: FactVisibility,
    pub retention: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailEntryRedacted {
    pub session_id: String,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrailQuery {
    pub session_id: Option<String>,
    pub include_private: bool,
}

impl TrailQuery {
    pub fn for_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            include_private: false,
        }
    }

    pub const fn include_private(mut self, include_private: bool) -> Self {
        self.include_private = include_private;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailSummary {
    pub session_id: String,
    pub steps: usize,
    pub consumed_refs: usize,
}

pub struct TrailContext<'a> {
    actor: &'a ActorContext,
    policy: &'a dyn Policy,
}

impl<'a> TrailContext<'a> {
    pub const fn new(actor: &'a ActorContext, policy: &'a dyn Policy) -> Self {
        Self { actor, policy }
    }

    pub const fn actor(&self) -> &'a ActorContext {
        self.actor
    }

    pub const fn policy(&self) -> &'a dyn Policy {
        self.policy
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

    fn note_consumed(&self, reference: TrailReference, ctx: &TrailContext<'_>);
}

pub trait TrailRedactor {
    fn redact(&self, entry: TrailEntryInProgress, ctx: &TrailContext<'_>) -> TrailEntryRedacted;
}

pub trait TrailSummarizer {
    fn summarize(&self, entries: &[TrailEntryRedacted], ctx: &TrailContext<'_>) -> TrailSummary;
}

pub trait TrailStore {
    fn append(&self, entry: TrailEntryRedacted, ctx: &TrailContext<'_>) -> Result<(), TrailError>;

    fn query(
        &self,
        request: TrailQuery,
        ctx: &TrailContext<'_>,
    ) -> Result<Vec<TrailEntryRedacted>, TrailError>;
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
    fn redact(&self, entry: TrailEntryInProgress, _ctx: &TrailContext<'_>) -> TrailEntryRedacted {
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
            actor: entry.actor,
            corpus: entry.corpus,
            verb: entry.verb,
            redacted_expr,
            input_hash: stable_input_hash(&entry.expr),
            surfaced_refs: entry.surfaced_refs,
            consumed_refs: entry.consumed_refs,
            prelude_hash: entry.prelude_hash,
            source_generations: entry.source_generations,
            visibility: entry.visibility,
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

    fn note_consumed(&self, _reference: TrailReference, _ctx: &TrailContext<'_>) {}
}

#[derive(Clone, Debug)]
pub struct DefaultTrailSummarizer;

impl TrailSummarizer for DefaultTrailSummarizer {
    fn summarize(&self, entries: &[TrailEntryRedacted], _ctx: &TrailContext<'_>) -> TrailSummary {
        TrailSummary {
            session_id: entries
                .first()
                .map(|entry| entry.session_id.clone())
                .unwrap_or_default(),
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

    fn session_path(&self, session_id: &str) -> Utf8PathBuf {
        self.dir.join(format!("{session_id}.jsonl"))
    }
}

impl TrailStore for JsonlTrailStore {
    fn append(&self, entry: TrailEntryRedacted, _ctx: &TrailContext<'_>) -> Result<(), TrailError> {
        fs::create_dir_all(&self.dir)?;
        let path = self.session_path(&entry.session_id);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, &entry)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn query(
        &self,
        request: TrailQuery,
        ctx: &TrailContext<'_>,
    ) -> Result<Vec<TrailEntryRedacted>, TrailError> {
        let mut entries = Vec::new();
        for path in self.query_paths(request.session_id.as_deref())? {
            read_jsonl_entries(&path, &mut entries)?;
        }
        entries.retain(|entry| {
            request
                .session_id
                .as_deref()
                .is_none_or(|id| entry.session_id == id)
        });
        if request.include_private
            && entries
                .iter()
                .any(|entry| entry.visibility == FactVisibility::Private)
        {
            authorize_trail_private(ctx.actor(), ctx.policy())?;
        }
        entries.retain(|entry| match entry.visibility {
            FactVisibility::Public => true,
            FactVisibility::Team => ctx.actor().can_see_fact_visibility(FactVisibility::Team),
            FactVisibility::Private => {
                request.include_private
                    && ctx.actor().can_see_fact_visibility(FactVisibility::Private)
            }
        });
        entries.sort_by(|left, right| {
            left.session_id
                .cmp(&right.session_id)
                .then(left.step.cmp(&right.step))
        });
        Ok(entries)
    }
}

impl JsonlTrailStore {
    fn query_paths(&self, session_id: Option<&str>) -> Result<Vec<Utf8PathBuf>, TrailError> {
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
        Ok(path.exists().then_some(path).into_iter().collect())
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
}

fn read_jsonl_entries(
    path: &Utf8Path,
    entries: &mut Vec<TrailEntryRedacted>,
) -> Result<(), TrailError> {
    let file = fs::File::open(path)?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        entries.push(serde_json::from_str(&line)?);
    }
    Ok(())
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

    fn trail_entry(expr: &str, visibility: FactVisibility) -> TrailEntryInProgress {
        TrailEntryInProgress {
            session_id: "session-1".to_string(),
            step: 1,
            timestamp: "2026-05-16T00:00:00Z".to_string(),
            actor: "tester".to_string(),
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
            visibility,
            retention: Some("P30D".to_string()),
        }
    }

    #[test]
    fn default_redactor_removes_string_literals_and_hashes_raw_input() {
        let actor = ActorContext::trusted_cli();
        let ctx = TrailContext::from(&actor);
        let redactor = DefaultTrailRedactor::default();
        let redacted = redactor.redact(
            trail_entry(
                r#"? secret("hunter2", "customer-7")."#,
                FactVisibility::Public,
            ),
            &ctx,
        );

        assert_eq!(redacted.redacted_expr, "<redacted>");
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
        let ctx = TrailContext::from(&actor);

        recorder
            .record(
                trail_entry(
                    r#"? read("secret-token", 10, span, text)."#,
                    FactVisibility::Public,
                ),
                &ctx,
            )
            .expect("trail record persists");

        let rows = store
            .query(TrailQuery::for_session("session-1"), &ctx)
            .expect("query persisted trail");
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].redacted_expr.contains("secret-token"));
        assert!(
            !fs::read_to_string(store.session_path("session-1"))
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
                    trail_entry(
                        r#"? read("visible", 10, span, text)."#,
                        FactVisibility::Private,
                    ),
                    &cli_ctx,
                ),
                &cli_ctx,
            )
            .expect("append private trail");

        let mcp_actor = ActorContext::anonymous_mcp();
        let mcp_ctx = TrailContext::from(&mcp_actor);
        assert!(
            store
                .query(TrailQuery::for_session("session-1"), &mcp_ctx)
                .expect("public query skips private rows")
                .is_empty()
        );
        let err = store
            .query(
                TrailQuery::for_session("session-1").include_private(true),
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
                TrailQuery::for_session("session-1").include_private(true),
                &privileged_ctx,
            )
            .expect("capable actor can query private trails");
        assert_eq!(private_rows.len(), 1);
    }

    #[test]
    fn trail_store_filters_team_entries_by_actor_visibility() {
        let dir = tempdir().expect("tempdir");
        let store = JsonlTrailStore::new(
            Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("tempdir path is utf-8"),
        );
        let cli_actor = ActorContext::trusted_cli();
        let cli_ctx = TrailContext::from(&cli_actor);
        store
            .append(
                DefaultTrailRedactor::default()
                    .redact(trail_entry(r"? work(h).", FactVisibility::Team), &cli_ctx),
                &cli_ctx,
            )
            .expect("append team trail");

        let mcp_actor = ActorContext::anonymous_mcp();
        let mcp_ctx = TrailContext::from(&mcp_actor);
        assert!(
            store
                .query(TrailQuery::for_session("session-1"), &mcp_ctx)
                .expect("actor without team visibility cannot read team trail rows")
                .is_empty()
        );

        let team_actor =
            ActorContext::anonymous_mcp().with_fact_visibility_capability(FactVisibility::Team);
        let team_ctx = TrailContext::from(&team_actor);
        let rows = store
            .query(TrailQuery::for_session("session-1"), &team_ctx)
            .expect("actor with team visibility can read team trail rows");
        assert_eq!(rows.len(), 1);
    }
}
