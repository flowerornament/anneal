use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::facts::FactIdentity;
use crate::store::FactStore;

/// Evaluation visibility envelope for source-derived facts.
///
/// This value is runtime metadata, not a stored-relation field. Missing
/// visibility defaults to `Public`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FactVisibility {
    #[default]
    Public,
    Team,
    Private,
}

impl FactVisibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Team => "team",
            Self::Private => "private",
        }
    }
}

pub(crate) fn hidden_handles<F>(store: &FactStore, fact_visible: &F) -> BTreeSet<String>
where
    F: Fn(&FactIdentity) -> bool,
{
    store
        .handles()
        .iter()
        .filter(|fact| !fact_visible(&fact.identity))
        .map(|fact| fact.id.clone())
        .collect()
}
