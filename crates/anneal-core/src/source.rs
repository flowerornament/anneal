use std::collections::BTreeSet;
use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

use crate::facts::FactBatch;
use crate::ids::{CorpusId, Generation};

/// Adapter contract for turning a data source into stored relation facts.
pub trait Source: Send + Sync {
    /// Describe recognized inputs, consumed discovery facts, and capabilities.
    fn describe(&self) -> SourceInfo;

    /// Extract facts without mutating runtime state.
    ///
    /// The runtime owns atomic merge and generation retraction semantics.
    fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError>;
}

/// Per-extraction context supplied by the runtime.
pub struct SourceContext<'a> {
    pub corpus: CorpusId,
    pub roots: &'a [camino::Utf8PathBuf],
    pub config_facts: &'a ConfigFacts,
    pub time_ref: Option<TimeRef>,
    pub previous_generation: Option<Generation>,
    pub actor: ActorContext,
    pub cancellation: CancellationToken,
}

impl SourceContext<'_> {
    pub fn next_generation(&self) -> Generation {
        self.previous_generation
            .map_or_else(Generation::initial, Generation::next)
    }
}

/// Discovery facts available to sources during Phase B extraction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFacts {
    entries: Vec<(String, String)>,
}

impl ConfigFacts {
    pub fn new(entries: Vec<(String, String)>) -> Self {
        Self { entries }
    }

    pub fn values<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries
            .iter()
            .filter(move |(entry_key, _)| entry_key == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(entry_key, _)| entry_key == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Actor identity and granted runtime capabilities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorContext {
    pub actor: String,
    pub capabilities: BTreeSet<String>,
}

impl ActorContext {
    pub fn anonymous_cli() -> Self {
        Self {
            actor: "anonymous-cli".to_string(),
            capabilities: BTreeSet::new(),
        }
    }

    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Cooperative cancellation flag passed from surfaces into extraction.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> Result<(), SourceError> {
        if self.is_cancelled() {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Historical extraction reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeRef {
    Snapshot(String),
    GitRef(String),
    Date(String),
}

/// Source self-description.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SourceInfo {
    pub name: &'static str,
    pub recognizes: Vec<Pattern>,
    pub doc: &'static str,
    pub config_keys: Vec<ConfigKey>,
    pub capabilities: SourceCapabilities,
    pub search: Option<SearchInfo>,
}

/// Glob-like recognition pattern.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Pattern(pub String);

impl Pattern {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Adapter-qualified discovery fact key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConfigKey {
    pub key: String,
    pub required: bool,
}

impl ConfigKey {
    pub fn required(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            required: true,
        }
    }

    pub fn optional(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            required: false,
        }
    }
}

/// Historical and incremental extraction capabilities.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SourceCapabilities {
    pub supports_git_ref: bool,
    pub supports_time_snapshot: bool,
    pub supports_incremental: bool,
    pub live_only: bool,
}

/// Search scoring metadata advertised by a source.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchInfo {
    pub reason_vocabulary: Vec<&'static str>,
    pub fields: Vec<&'static str>,
    pub low_confidence_threshold: f32,
}

/// Policy action target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Read,
    Search,
    Eval,
    Extract { source: String },
}

/// Source extraction error.
#[derive(Debug)]
pub enum SourceError {
    Cancelled,
    UnsupportedTimeRef(TimeRef),
    Io {
        path: Option<String>,
        source: std::io::Error,
    },
    InvalidConfig(String),
    Other(String),
}

impl SourceError {
    pub fn io(path: &Utf8Path, source: std::io::Error) -> Self {
        Self::Io {
            path: Some(path.to_string()),
            source,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("source extraction cancelled"),
            Self::UnsupportedTimeRef(time_ref) => {
                write!(f, "source does not support time ref {time_ref:?}")
            }
            Self::Io { path, source } => {
                if let Some(path) = path {
                    write!(f, "{path}: {source}")
                } else {
                    write!(f, "{source}")
                }
            }
            Self::InvalidConfig(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
