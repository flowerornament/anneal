//! Effective edge-kind policy for impact traversal.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use crate::config_schema::{RuntimeConfigKey, runtime_config_declaration_by_key};
use crate::facts::ConfigFact;

const DEFAULT_EDGE_KINDS: &[&str] = &["DependsOn", "Supersedes", "Verifies"];
static CONFIG_KEY: LazyLock<String> = LazyLock::new(|| {
    runtime_config_declaration_by_key(RuntimeConfigKey::ImpactTraverse)
        .expect("impact traversal config declaration is built in")
        .config_key()
});

/// The effective edge kinds traversed by impact queries and command surfaces.
///
/// A non-empty project configuration replaces the built-in relation set. With
/// no configured relations, the policy follows the built-in defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImpactTraversalPolicy {
    configured_edge_kinds: BTreeSet<String>,
}

impl ImpactTraversalPolicy {
    /// Build the effective policy from stored runtime configuration facts.
    #[must_use]
    pub fn from_config_facts(configs: &[ConfigFact]) -> Self {
        Self {
            configured_edge_kinds: configs
                .iter()
                .filter(|fact| fact.key == *CONFIG_KEY)
                .map(|fact| fact.value.clone())
                .collect(),
        }
    }

    /// Return whether impact traversal follows `edge_kind`.
    #[must_use]
    pub fn traverses(&self, edge_kind: &str) -> bool {
        if self.configured_edge_kinds.is_empty() {
            DEFAULT_EDGE_KINDS.contains(&edge_kind)
        } else {
            self.configured_edge_kinds.contains(edge_kind)
        }
    }

    pub(crate) fn insert_config(&mut self, key: &str, value: &str) {
        if key == *CONFIG_KEY {
            self.configured_edge_kinds.insert(value.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::CorpusId;

    use super::*;

    fn config(key: &str, value: &str) -> ConfigFact {
        ConfigFact {
            corpus: CorpusId::from("test"),
            key: key.to_string(),
            value: value.to_string(),
            ordinal: None,
        }
    }

    #[test]
    fn defaults_cover_the_builtin_dependency_relations() {
        let policy = ImpactTraversalPolicy::default();

        assert!(policy.traverses("DependsOn"));
        assert!(policy.traverses("Supersedes"));
        assert!(policy.traverses("Verifies"));
        assert!(!policy.traverses("Cites"));
    }

    #[test]
    fn configured_relations_replace_the_defaults() {
        let key = CONFIG_KEY.as_str();
        let policy = ImpactTraversalPolicy::from_config_facts(&[
            config(key, "DependsOn"),
            config(key, "Synthesizes"),
            config("unrelated", "Cites"),
        ]);

        assert!(policy.traverses("DependsOn"));
        assert!(policy.traverses("Synthesizes"));
        assert!(!policy.traverses("Supersedes"));
        assert!(!policy.traverses("Cites"));
    }

    #[test]
    fn incremental_runtime_ingestion_matches_fact_construction() {
        let key = CONFIG_KEY.as_str();
        let configs = [
            config(key, "DependsOn"),
            config(key, "Synthesizes"),
            config("unrelated", "Cites"),
        ];
        let from_facts = ImpactTraversalPolicy::from_config_facts(&configs);
        let mut incremental = ImpactTraversalPolicy::default();
        for fact in &configs {
            incremental.insert_config(&fact.key, &fact.value);
        }

        assert_eq!(incremental, from_facts);
    }
}
