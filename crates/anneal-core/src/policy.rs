//! Authorization policy contracts for runtime actions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::source::ActorContext;

/// Coarse action category for policy decisions and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    Read,
    ReadFull,
    Search,
    Match,
    Eval,
    Extract,
}

impl ActionKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadFull => "read_full",
            Self::Search => "search",
            Self::Match => "match",
            Self::Eval => "eval",
            Self::Extract => "extract",
        }
    }
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Policy action target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Read {
        handle: String,
    },
    ReadFull {
        handle: String,
    },
    Search {
        query: String,
        handle: Option<String>,
    },
    Match {
        pattern: String,
        handle: Option<String>,
    },
    Eval,
    Extract {
        source: String,
    },
}

impl Action {
    #[must_use]
    pub const fn kind(&self) -> ActionKind {
        match self {
            Self::Read { .. } => ActionKind::Read,
            Self::ReadFull { .. } => ActionKind::ReadFull,
            Self::Search { .. } => ActionKind::Search,
            Self::Match { .. } => ActionKind::Match,
            Self::Eval => ActionKind::Eval,
            Self::Extract { .. } => ActionKind::Extract,
        }
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.kind().as_str()
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Host/project authorization decision for a runtime action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    Deny,
}

impl PolicyDecision {
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Actor/action authorization hook installed by surfaces or hosts.
pub trait Policy: fmt::Debug + Send + Sync {
    fn check(&self, actor: &ActorContext, action: &Action) -> PolicyDecision;
}

/// Compatibility policy used when no host/project policy is configured.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllPolicy;

impl Policy for AllowAllPolicy {
    fn check(&self, _actor: &ActorContext, _action: &Action) -> PolicyDecision {
        PolicyDecision::Allow
    }
}
