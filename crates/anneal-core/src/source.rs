use std::collections::BTreeSet;
use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

use crate::visibility::FactVisibility;

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
    entries: Vec<ConfigEntry>,
}

/// One adapter-visible discovery/config fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub ordinal: Option<u32>,
}

impl ConfigEntry {
    pub fn scalar(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ordinal: None,
        }
    }

    pub fn ordered(key: impl Into<String>, value: impl Into<String>, ordinal: u32) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ordinal: Some(ordinal),
        }
    }
}

impl From<(String, String)> for ConfigEntry {
    fn from((key, value): (String, String)) -> Self {
        Self::scalar(key, value)
    }
}

impl ConfigFacts {
    pub fn new(entries: Vec<(String, String)>) -> Self {
        Self {
            entries: entries.into_iter().map(ConfigEntry::from).collect(),
        }
    }

    pub fn from_entries(entries: Vec<ConfigEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[ConfigEntry] {
        &self.entries
    }

    pub fn values<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.entries
            .iter()
            .filter(move |entry| entry.key == key)
            .map(|entry| entry.value.as_str())
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
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

/// Built-in runtime capabilities recognized by the substrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RuntimeCapability {
    ReadFull,
    Eval,
    TrailPrivate,
}

impl RuntimeCapability {
    pub const ALL: [Self; 3] = [Self::ReadFull, Self::Eval, Self::TrailPrivate];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadFull => "read_full",
            Self::Eval => "eval",
            Self::TrailPrivate => "trail_private",
        }
    }
}

/// Typed actor capability understood by the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActorCapability {
    Runtime(RuntimeCapability),
    FactVisibility(FactVisibility),
}

impl ActorCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime(capability) => capability.as_str(),
            Self::FactVisibility(FactVisibility::Public) => "visibility:public",
            Self::FactVisibility(FactVisibility::Team) => {
                ActorContext::TEAM_FACT_VISIBILITY_CAPABILITY
            }
            Self::FactVisibility(FactVisibility::Private) => {
                ActorContext::PRIVATE_FACT_VISIBILITY_CAPABILITY
            }
        }
    }
}

impl fmt::Display for ActorCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for RuntimeCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ActorContext {
    pub const TEAM_FACT_VISIBILITY_CAPABILITY: &'static str = "visibility:team";
    pub const PRIVATE_FACT_VISIBILITY_CAPABILITY: &'static str = "visibility:private";

    pub fn anonymous_cli() -> Self {
        Self {
            actor: "anonymous-cli".to_string(),
            capabilities: BTreeSet::new(),
        }
    }

    pub fn trusted_cli() -> Self {
        Self::anonymous_cli()
            .with_runtime_capability(RuntimeCapability::ReadFull)
            .with_runtime_capability(RuntimeCapability::Eval)
            .with_runtime_capability(RuntimeCapability::TrailPrivate)
            .with_fact_visibility_capability(FactVisibility::Private)
    }

    pub fn anonymous_mcp() -> Self {
        Self {
            actor: "anonymous-mcp".to_string(),
            capabilities: BTreeSet::new(),
        }
    }

    pub fn with_actor_capability(mut self, capability: ActorCapability) -> Self {
        if capability != ActorCapability::FactVisibility(FactVisibility::Public) {
            self.capabilities.insert(capability.as_str().to_string());
        }
        self
    }

    pub fn with_runtime_capability(self, capability: RuntimeCapability) -> Self {
        self.with_actor_capability(ActorCapability::Runtime(capability))
    }

    pub fn has_runtime_capability(&self, capability: RuntimeCapability) -> bool {
        self.has_actor_capability(ActorCapability::Runtime(capability))
    }

    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }

    pub fn has_actor_capability(&self, capability: ActorCapability) -> bool {
        capability == ActorCapability::FactVisibility(FactVisibility::Public)
            || self.has_capability(capability.as_str())
    }

    pub fn with_fact_visibility_capability(self, visibility: FactVisibility) -> Self {
        self.with_actor_capability(ActorCapability::FactVisibility(visibility))
    }

    pub fn can_see_fact_visibility(&self, visibility: FactVisibility) -> bool {
        match visibility {
            FactVisibility::Public => true,
            FactVisibility::Team => {
                self.has_actor_capability(ActorCapability::FactVisibility(FactVisibility::Team))
                    || self.has_actor_capability(ActorCapability::FactVisibility(
                        FactVisibility::Private,
                    ))
            }
            FactVisibility::Private => {
                self.has_actor_capability(ActorCapability::FactVisibility(FactVisibility::Private))
            }
        }
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

impl TimeRef {
    pub fn parse(reference: impl Into<String>) -> Self {
        let reference = reference.into();
        if reference == "snapshot:last"
            || (reference.starts_with("snapshot:") && reference != "snapshot:")
        {
            Self::Snapshot(reference)
        } else if is_date_reference(&reference) {
            Self::Date(reference)
        } else {
            Self::GitRef(reference)
        }
    }
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

impl SourceCapabilities {
    pub fn supports_time_ref(&self, time_ref: &TimeRef) -> bool {
        match time_ref {
            TimeRef::Snapshot(_) | TimeRef::Date(_) => self.supports_time_snapshot,
            TimeRef::GitRef(_) => self.supports_git_ref,
        }
    }
}

/// Search scoring metadata advertised by a source.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchInfo {
    pub reason_vocabulary: Vec<&'static str>,
    pub fields: Vec<&'static str>,
    pub low_confidence_threshold: f32,
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

fn is_date_reference(reference: &str) -> bool {
    if let Some(days) = reference
        .strip_prefix("--")
        .and_then(|value| value.strip_suffix("days"))
    {
        return !days.is_empty() && days.bytes().all(|byte| byte.is_ascii_digit());
    }
    parse_iso_date_prefix(reference).is_some()
}

fn parse_iso_date_prefix(reference: &str) -> Option<(i32, u32, u32)> {
    let date = reference.get(0.."YYYY-MM-DD".len())?;
    let bytes = date.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !bytes
            .iter()
            .enumerate()
            .all(|(idx, byte)| matches!(idx, 4 | 7) || byte.is_ascii_digit())
    {
        return None;
    }
    let year = date.get(0..4)?.parse::<i32>().ok()?;
    let month = date.get(5..7)?.parse::<u32>().ok()?;
    let day = date.get(8..10)?.parse::<u32>().ok()?;
    (1..=12).contains(&month).then_some(()).and_then(|()| {
        (1..=days_in_month(year, month))
            .contains(&day)
            .then_some(())
    })?;
    Some((year, month, day))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_ref_parse_classifies_snapshot_date_and_git_refs() {
        assert!(matches!(
            TimeRef::parse("snapshot:last"),
            TimeRef::Snapshot(_)
        ));
        assert!(matches!(TimeRef::parse("snapshot:"), TimeRef::GitRef(_)));
        assert!(matches!(TimeRef::parse("2026-05-13"), TimeRef::Date(_)));
        assert!(matches!(TimeRef::parse("--7days"), TimeRef::Date(_)));
        assert!(matches!(TimeRef::parse("HEAD~3"), TimeRef::GitRef(_)));
        assert!(matches!(TimeRef::parse("--days"), TimeRef::GitRef(_)));
        assert!(matches!(TimeRef::parse("2026-99-99"), TimeRef::GitRef(_)));
    }

    #[test]
    fn source_capabilities_report_time_ref_support() {
        let caps = SourceCapabilities {
            supports_git_ref: true,
            supports_time_snapshot: false,
            supports_incremental: false,
            live_only: false,
        };
        assert!(caps.supports_time_ref(&TimeRef::GitRef("HEAD~3".to_string())));
        assert!(!caps.supports_time_ref(&TimeRef::Snapshot("snapshot:last".to_string())));
    }

    #[test]
    fn actor_capabilities_have_typed_runtime_and_visibility_helpers() {
        let actor = ActorContext::anonymous_mcp()
            .with_actor_capability(ActorCapability::Runtime(RuntimeCapability::ReadFull))
            .with_actor_capability(ActorCapability::FactVisibility(FactVisibility::Team));

        assert!(actor.has_runtime_capability(RuntimeCapability::ReadFull));
        assert!(actor.can_see_fact_visibility(FactVisibility::Public));
        assert!(actor.can_see_fact_visibility(FactVisibility::Team));
        assert!(!actor.can_see_fact_visibility(FactVisibility::Private));
        assert!(ActorContext::trusted_cli().has_runtime_capability(RuntimeCapability::Eval));
        assert!(
            ActorContext::trusted_cli().has_runtime_capability(RuntimeCapability::TrailPrivate)
        );
        assert!(ActorContext::trusted_cli().can_see_fact_visibility(FactVisibility::Private));
    }

    #[test]
    fn config_facts_preserve_explicit_ordinals() {
        let facts = ConfigFacts::from_entries(vec![
            ConfigEntry::ordered("convergence.ordering", "draft", 1),
            ConfigEntry::scalar("md.file_extension", ".md"),
        ]);

        assert_eq!(facts.first("md.file_extension"), Some(".md"));
        assert_eq!(
            facts
                .entries()
                .iter()
                .find(|entry| entry.key == "convergence.ordering")
                .and_then(|entry| entry.ordinal),
            Some(1)
        );
    }
}
