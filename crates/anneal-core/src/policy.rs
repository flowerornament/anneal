//! Authorization policy contracts for runtime actions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::source::{ActorContext, RuntimeCapability};

/// Coarse action category for policy decisions and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    Read,
    ReadFull,
    Search,
    Match,
    Eval,
    TrailPrivateRead,
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
            Self::TrailPrivateRead => "trail_private",
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
    TrailPrivateRead,
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
            Self::TrailPrivateRead => ActionKind::TrailPrivateRead,
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

/// Authorization failure produced by the shared policy/capability path.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationError {
    #[error("action '{action}' requires capability '{capability}'")]
    CapabilityRequired {
        action: ActionKind,
        capability: RuntimeCapability,
    },
    #[error("policy denied action '{action}' for actor '{actor}'")]
    PolicyDenied { actor: String, action: Action },
}

pub fn authorize_action(
    actor: &ActorContext,
    policy: &dyn Policy,
    action: Action,
) -> Result<(), AuthorizationError> {
    match policy.check(actor, &action) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny => Err(AuthorizationError::PolicyDenied {
            actor: actor.actor.clone(),
            action,
        }),
    }
}

pub fn authorize_capability_action(
    actor: &ActorContext,
    policy: &dyn Policy,
    action: Action,
    capability: RuntimeCapability,
) -> Result<(), AuthorizationError> {
    if !actor.has_runtime_capability(capability) {
        return Err(AuthorizationError::CapabilityRequired {
            action: action.kind(),
            capability,
        });
    }
    authorize_action(actor, policy, action)
}

pub fn authorize_trail_private(
    actor: &ActorContext,
    policy: &dyn Policy,
) -> Result<(), AuthorizationError> {
    authorize_capability_action(
        actor,
        policy,
        Action::TrailPrivateRead,
        RuntimeCapability::TrailPrivate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct DenyTrailPrivatePolicy;

    impl Policy for DenyTrailPrivatePolicy {
        fn check(&self, _actor: &ActorContext, action: &Action) -> PolicyDecision {
            if action.kind() == ActionKind::TrailPrivateRead {
                PolicyDecision::Deny
            } else {
                PolicyDecision::Allow
            }
        }
    }

    #[test]
    fn trail_private_authorization_requires_capability() {
        let err = authorize_trail_private(&ActorContext::anonymous_mcp(), &AllowAllPolicy)
            .expect_err("non-cli actor has no trail_private capability by default");
        assert!(matches!(
            err,
            AuthorizationError::CapabilityRequired {
                action: ActionKind::TrailPrivateRead,
                capability: RuntimeCapability::TrailPrivate,
            }
        ));
    }

    #[test]
    fn trail_private_authorization_allows_capable_actor() {
        let actor =
            ActorContext::anonymous_mcp().with_runtime_capability(RuntimeCapability::TrailPrivate);

        authorize_trail_private(&actor, &AllowAllPolicy)
            .expect("trail_private capability and allow policy authorize private trail reads");
    }

    #[test]
    fn trail_private_authorization_still_consults_policy() {
        let actor =
            ActorContext::anonymous_mcp().with_runtime_capability(RuntimeCapability::TrailPrivate);
        let err = authorize_trail_private(&actor, &DenyTrailPrivatePolicy)
            .expect_err("policy can deny private trail reads after capability passes");

        assert!(matches!(
            err,
            AuthorizationError::PolicyDenied {
                action: Action::TrailPrivateRead,
                ..
            }
        ));
    }
}
