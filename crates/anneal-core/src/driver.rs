//! Runtime-owned source refresh orchestration.

use camino::Utf8PathBuf;
use thiserror::Error;

use crate::facts::FactBatch;
use crate::ids::{CorpusId, Generation, SourceName};
use crate::source::{
    ActorContext, CancellationToken, ConfigFacts, Source, SourceContext, SourceError, SourceInfo,
    TimeRef,
};
use crate::store::{FactStore, StoreError};

/// Long-running surface boundary for refreshing a source into a fact store.
pub trait SourceDriver: Send + Sync {
    /// Describe the source this driver refreshes.
    fn describe(&self) -> SourceInfo;

    /// Produce a refresh batch for the supplied extraction context.
    fn refresh(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError>;
}

/// Refresh `driver` and atomically merge its batch into `store`.
pub fn refresh_source(
    driver: &(impl SourceDriver + ?Sized),
    request: &SourceRefreshRequest<'_>,
    store: &mut FactStore,
) -> Result<SourceRefreshReport, SourceDriverError> {
    let source = SourceName::from(driver.describe().name);
    let previous_generation = store.generation_for(&request.corpus, &source);
    let cx = request.source_context(previous_generation);
    let batch = driver.refresh(&cx)?;
    if batch.corpus != request.corpus || batch.source != source {
        return Err(SourceDriverError::MismatchedBatchScope {
            expected_corpus: request.corpus.clone(),
            expected_source: source,
            actual_corpus: batch.corpus,
            actual_source: batch.source,
        });
    }
    let report = SourceRefreshReport::from_batch(&batch, previous_generation);
    store.merge(batch)?;
    Ok(report)
}

/// Request context for one driver-owned source refresh.
pub struct SourceRefreshRequest<'a> {
    corpus: CorpusId,
    roots: &'a [Utf8PathBuf],
    config_facts: &'a ConfigFacts,
    time_ref: Option<TimeRef>,
    actor: ActorContext,
    cancellation: CancellationToken,
}

impl<'a> SourceRefreshRequest<'a> {
    pub fn new(
        corpus: impl Into<CorpusId>,
        roots: &'a [Utf8PathBuf],
        config_facts: &'a ConfigFacts,
    ) -> Self {
        Self {
            corpus: corpus.into(),
            roots,
            config_facts,
            time_ref: None,
            actor: ActorContext::anonymous_cli(),
            cancellation: CancellationToken::new(),
        }
    }

    pub fn with_time_ref(mut self, time_ref: TimeRef) -> Self {
        self.time_ref = Some(time_ref);
        self
    }

    pub fn with_actor(mut self, actor: ActorContext) -> Self {
        self.actor = actor;
        self
    }

    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn corpus(&self) -> &CorpusId {
        &self.corpus
    }

    pub fn roots(&self) -> &'a [Utf8PathBuf] {
        self.roots
    }

    pub fn config_facts(&self) -> &'a ConfigFacts {
        self.config_facts
    }

    pub fn time_ref(&self) -> Option<&TimeRef> {
        self.time_ref.as_ref()
    }

    pub fn actor(&self) -> &ActorContext {
        &self.actor
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    fn source_context(&self, previous_generation: Option<Generation>) -> SourceContext<'_> {
        SourceContext {
            corpus: self.corpus.clone(),
            roots: self.roots,
            config_facts: self.config_facts,
            time_ref: self.time_ref.clone(),
            previous_generation,
            actor: self.actor.clone(),
            cancellation: self.cancellation.clone(),
        }
    }
}

/// Summary of a committed source refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRefreshReport {
    pub corpus: CorpusId,
    pub source: SourceName,
    pub previous_generation: Option<Generation>,
    pub current_generation: Generation,
}

impl SourceRefreshReport {
    fn from_batch(batch: &FactBatch, previous_generation: Option<Generation>) -> Self {
        Self {
            corpus: batch.corpus.clone(),
            source: batch.source.clone(),
            previous_generation,
            current_generation: batch.generation,
        }
    }
}

/// Default driver for one-shot extraction surfaces.
#[derive(Clone, Debug)]
pub struct OneShotSourceDriver<S> {
    source: S,
    info: SourceInfo,
}

impl<S> OneShotSourceDriver<S>
where
    S: Source,
{
    pub fn new(source: S) -> Self {
        let info = source.describe();
        Self { source, info }
    }
}

impl<S> OneShotSourceDriver<S> {
    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn into_source(self) -> S {
        self.source
    }
}

impl<S> SourceDriver for OneShotSourceDriver<S>
where
    S: Source,
{
    fn describe(&self) -> SourceInfo {
        self.info.clone()
    }

    fn refresh(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
        validate_required_config(&self.info, cx.config_facts)?;
        self.source.extract(cx)
    }
}

fn validate_required_config(info: &SourceInfo, facts: &ConfigFacts) -> Result<(), SourceError> {
    for key in &info.config_keys {
        if key.required_flag() && facts.first(key.key()).is_none() {
            return Err(SourceError::InvalidConfig(format!(
                "source {} requires config fact {}",
                info.name,
                key.key()
            )));
        }
    }
    Ok(())
}

/// Failure while refreshing a source into a store.
#[derive(Debug, Error)]
pub enum SourceDriverError {
    #[error("source extraction failed: {0}")]
    Source(#[from] SourceError),
    #[error(
        "source returned batch for {actual_corpus}/{actual_source}, expected {expected_corpus}/{expected_source}"
    )]
    MismatchedBatchScope {
        expected_corpus: CorpusId,
        expected_source: SourceName,
        actual_corpus: CorpusId,
        actual_source: SourceName,
    },
    #[error("source merge failed: {0}")]
    Store(#[from] StoreError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use crate::facts::{FactBatch, FactBatchMode, FactIdentity, HandleFact};
    use crate::ids::{NativeId, OriginUri, Revision};
    use crate::source::{ConfigKey, Pattern, SourceCapabilities};

    use super::*;

    const SOURCE_NAME: &str = "test-source";

    #[derive(Debug)]
    struct EditableSource {
        status: Mutex<String>,
        seen_previous_generations: Mutex<Vec<Option<Generation>>>,
    }

    impl EditableSource {
        fn new(status: &str) -> Self {
            Self {
                status: Mutex::new(status.to_string()),
                seen_previous_generations: Mutex::new(Vec::new()),
            }
        }

        fn set_status(&self, status: &str) {
            *self.status.lock().expect("status lock") = status.to_string();
        }

        fn seen_previous_generations(&self) -> Vec<Option<Generation>> {
            self.seen_previous_generations
                .lock()
                .expect("seen lock")
                .clone()
        }
    }

    impl Source for EditableSource {
        fn describe(&self) -> SourceInfo {
            SourceInfo {
                name: SOURCE_NAME,
                recognizes: vec![Pattern::new("**/*.md")],
                doc: "test source",
                config_keys: vec![ConfigKey::optional("test.option")],
                capabilities: SourceCapabilities::default(),
                search: None,
            }
        }

        fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
            self.seen_previous_generations
                .lock()
                .expect("seen lock")
                .push(cx.previous_generation);
            let generation = cx.next_generation();
            let status = self.status.lock().expect("status lock").clone();
            let mut batch = FactBatch::new(
                cx.corpus.clone(),
                SourceName::from(SOURCE_NAME),
                FactBatchMode::FullSnapshot,
                generation,
            );
            batch.handles.push(HandleFact {
                identity: FactIdentity::new(
                    cx.corpus.clone(),
                    SourceName::from(SOURCE_NAME),
                    NativeId::from("doc.md"),
                    OriginUri::from("file://doc.md"),
                    Revision::from("r1"),
                    generation,
                ),
                id: "doc.md".to_string(),
                kind: "file".to_string(),
                status: Some(status),
                namespace: String::new(),
                file: "doc.md".to_string(),
                line: 1,
                date: None,
                area: String::new(),
                summary: String::new(),
            });
            Ok(batch)
        }
    }

    #[derive(Debug)]
    struct WrongScopeSource;

    impl Source for WrongScopeSource {
        fn describe(&self) -> SourceInfo {
            SourceInfo {
                name: SOURCE_NAME,
                recognizes: Vec::new(),
                doc: "wrong scope test source",
                config_keys: Vec::new(),
                capabilities: SourceCapabilities::default(),
                search: None,
            }
        }

        fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
            Ok(FactBatch::new(
                cx.corpus.clone(),
                SourceName::from("other-source"),
                FactBatchMode::FullSnapshot,
                cx.next_generation(),
            ))
        }
    }

    struct RequiredConfigSource;

    impl Source for RequiredConfigSource {
        fn describe(&self) -> SourceInfo {
            SourceInfo {
                name: SOURCE_NAME,
                recognizes: vec![Pattern::new("**/*.md")],
                doc: "test source",
                config_keys: vec![ConfigKey::required("test.required")],
                capabilities: SourceCapabilities::default(),
                search: None,
            }
        }

        fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
            Ok(FactBatch::new(
                cx.corpus.clone(),
                SourceName::from(SOURCE_NAME),
                FactBatchMode::FullSnapshot,
                cx.next_generation(),
            ))
        }
    }

    #[test]
    fn one_shot_driver_threads_generation_and_commits_snapshot_swap() {
        let roots = Vec::new();
        let config = ConfigFacts::default();
        let request = SourceRefreshRequest::new("test", &roots, &config).with_actor(ActorContext {
            actor: "test".to_string(),
            capabilities: BTreeSet::new(),
        });
        let driver = OneShotSourceDriver::new(EditableSource::new("draft"));
        let mut store = FactStore::default();

        let first = refresh_source(&driver, &request, &mut store).expect("first refresh");
        driver.source().set_status("active");
        let second = refresh_source(&driver, &request, &mut store).expect("second refresh");

        assert_eq!(first.previous_generation, None);
        assert_eq!(first.current_generation, Generation::new(1));
        assert_eq!(second.previous_generation, Some(Generation::new(1)));
        assert_eq!(second.current_generation, Generation::new(2));
        assert_eq!(
            driver.source().seen_previous_generations(),
            vec![None, Some(Generation::new(1))]
        );
        assert_eq!(store.handles().len(), 1);
        assert_eq!(store.handles()[0].status.as_deref(), Some("active"));
        assert_eq!(
            store.generation_for(&CorpusId::from("test"), &SourceName::from(SOURCE_NAME)),
            Some(Generation::new(2))
        );
    }

    #[test]
    fn driver_rejects_batches_outside_the_source_scope() {
        let roots = Vec::new();
        let config = ConfigFacts::default();
        let request = SourceRefreshRequest::new("test", &roots, &config);
        let driver = OneShotSourceDriver::new(WrongScopeSource);
        let mut store = FactStore::default();

        let err = refresh_source(&driver, &request, &mut store)
            .expect_err("wrong source scope should fail");

        assert!(matches!(
            err,
            SourceDriverError::MismatchedBatchScope {
                expected_source,
                actual_source,
                ..
            } if expected_source == SourceName::from(SOURCE_NAME)
                && actual_source == SourceName::from("other-source")
        ));
        assert!(store.generations().is_empty());
    }

    #[test]
    fn one_shot_driver_rejects_missing_required_config() {
        let roots = Vec::new();
        let config = ConfigFacts::default();
        let request = SourceRefreshRequest::new("test", &roots, &config);
        let driver = OneShotSourceDriver::new(RequiredConfigSource);
        let mut store = FactStore::default();

        let err = refresh_source(&driver, &request, &mut store)
            .expect_err("missing required config rejects");

        assert!(err.to_string().contains("test.required"));
        assert!(store.generations().is_empty());
    }
}
