//! Runtime configuration declaration schema.

use crate::source::ConfigEntry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConfigValueMode {
    Scalar,
    OrderedList,
    UnorderedSet,
    Tuple {
        expected: &'static str,
        arity: usize,
    },
    TupleList {
        expected: &'static str,
        minimum: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConfigLifecycle {
    Active,
    ObsoleteConfirmedNamespace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConfigKey {
    CorpusRoot,
    CorpusExclude,
    ConvergenceOrdering,
    ConvergenceActive,
    ConvergenceTerminal,
    ConvergenceAssertsCode,
    ConvergenceDescription,
    HandlesForce,
    HandlesRejected,
    HandlesLinear,
    HandlesConfirmed,
    FrontmatterField,
    FreshnessWarn,
    FreshnessError,
    StateHistoryMode,
    CheckDefaultFilter,
    SuppressCode,
    SuppressRule,
    ConcernsGroup,
    ImpactTraverse,
    AreasOrphanThreshold,
    TemporalRecentDays,
    OrientEdgeWeight,
    OrientLabelWeight,
    OrientRecencyWeight,
    OrientRecencyHalfLifeDays,
    OrientBudget,
    OrientDepth,
    OrientPin,
    OrientExclude,
    OrientStubBytes,
    OrientCuratedHubWeight,
    CodePathRoot,
    SearchBoostStatus,
    SearchBoostHub,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeConfigDeclaration {
    key: RuntimeConfigKey,
    section: &'static str,
    name: &'static str,
    mode: RuntimeConfigValueMode,
    lifecycle: RuntimeConfigLifecycle,
}

impl RuntimeConfigDeclaration {
    #[must_use]
    pub const fn key(self) -> RuntimeConfigKey {
        self.key
    }

    #[must_use]
    pub const fn section(self) -> &'static str {
        self.section
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    #[must_use]
    pub const fn mode(self) -> RuntimeConfigValueMode {
        self.mode
    }

    #[must_use]
    pub const fn lifecycle(self) -> RuntimeConfigLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub fn config_key(self) -> String {
        format!("{}.{}", self.section, self.name)
    }

    pub fn validate_values(self, values: &[String]) -> Result<(), RuntimeConfigEntryError> {
        if self.lifecycle != RuntimeConfigLifecycle::Active {
            return Err(RuntimeConfigEntryError::Obsolete(self.key));
        }
        match self.mode {
            RuntimeConfigValueMode::Scalar if values.len() != 1 => {
                Err(RuntimeConfigEntryError::InvalidArity {
                    key: self.config_key(),
                    expected: "exactly one scalar value",
                    actual: values.len(),
                })
            }
            RuntimeConfigValueMode::Scalar
            | RuntimeConfigValueMode::OrderedList
            | RuntimeConfigValueMode::UnorderedSet => Ok(()),
            RuntimeConfigValueMode::Tuple { expected, arity } => {
                validate_tuple_arity(self, values.len(), expected, arity)
            }
            RuntimeConfigValueMode::TupleList { expected, minimum } => {
                if values.len() < minimum {
                    Err(RuntimeConfigEntryError::InvalidArity {
                        key: self.config_key(),
                        expected,
                        actual: values.len(),
                    })
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn entries(self, values: Vec<String>) -> Result<Vec<ConfigEntry>, RuntimeConfigEntryError> {
        self.validate_values(&values)?;
        match self.key {
            RuntimeConfigKey::ConvergenceDescription => {
                let [status, description]: [String; 2] =
                    expect_exact_tuple(self, values, "status and description")?;
                Ok(vec![ConfigEntry::scalar(
                    format!("convergence.description.{status}"),
                    description,
                )])
            }
            RuntimeConfigKey::FrontmatterField => {
                let [field, edge_kind, direction]: [String; 3] =
                    expect_exact_tuple(self, values, "field, edge kind, and direction")?;
                Ok(vec![
                    ConfigEntry::scalar(format!("frontmatter.field.{field}.edge_kind"), edge_kind),
                    ConfigEntry::scalar(format!("frontmatter.field.{field}.direction"), direction),
                ])
            }
            RuntimeConfigKey::SuppressRule => {
                let [code, target]: [String; 2] =
                    expect_exact_tuple(self, values, "code and target")?;
                Ok(vec![ConfigEntry::scalar(
                    format!("suppress.rule.{code}"),
                    target,
                )])
            }
            RuntimeConfigKey::ConcernsGroup => {
                if values.len() < 2 {
                    return Err(RuntimeConfigEntryError::InvalidArity {
                        key: self.config_key(),
                        expected: "name and one or more patterns",
                        actual: values.len(),
                    });
                }
                let mut values = values;
                let name = values.remove(0);
                Ok(values
                    .into_iter()
                    .map(|value| ConfigEntry::scalar(format!("concerns.group.{name}"), value))
                    .collect())
            }
            RuntimeConfigKey::SearchBoostStatus => {
                let [status, boost]: [String; 2] =
                    expect_exact_tuple(self, values, "status and boost")?;
                Ok(vec![ConfigEntry::scalar(
                    format!("{SEARCH_BOOST_STATUS_ENTRY_PREFIX}{status}"),
                    boost,
                )])
            }
            RuntimeConfigKey::SearchBoostHub => {
                let [boost]: [String; 1] = expect_exact_tuple(self, values, "exactly one boost")?;
                Ok(vec![ConfigEntry::scalar(SEARCH_BOOST_HUB_KEY, boost)])
            }
            _ => match self.mode {
                RuntimeConfigValueMode::Scalar => {
                    let [value]: [String; 1] =
                        values.try_into().map_err(|values: Vec<String>| {
                            RuntimeConfigEntryError::InvalidArity {
                                key: self.config_key(),
                                expected: "exactly one scalar value",
                                actual: values.len(),
                            }
                        })?;
                    Ok(vec![ConfigEntry::scalar(self.config_key(), value)])
                }
                RuntimeConfigValueMode::OrderedList => ordered_entries(&self.config_key(), values),
                RuntimeConfigValueMode::UnorderedSet => {
                    let key = self.config_key();
                    Ok(values
                        .into_iter()
                        .map(|value| ConfigEntry::scalar(key.clone(), value))
                        .collect())
                }
                RuntimeConfigValueMode::Tuple { .. } | RuntimeConfigValueMode::TupleList { .. } => {
                    Err(RuntimeConfigEntryError::MissingLowering {
                        key: self.config_key(),
                    })
                }
            },
        }
    }

    pub fn entries_for_name(
        self,
        _declared_name: &str,
        values: Vec<String>,
    ) -> Result<Vec<ConfigEntry>, RuntimeConfigEntryError> {
        self.entries(values)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeConfigEntryError {
    #[error("config declaration '{key}' expects {expected}; got {actual} values")]
    InvalidArity {
        key: String,
        expected: &'static str,
        actual: usize,
    },
    #[error("ordered config declaration '{key}' overflowed u32 ordinals")]
    OrderedConfigIndexOverflow { key: String },
    #[error("config declaration '{key}' is tuple-shaped but has no fact lowering")]
    MissingLowering { key: String },
    #[error("obsolete config declaration {0:?}")]
    Obsolete(RuntimeConfigKey),
}

pub const SEARCH_BOOST_STATUS_ENTRY_PREFIX: &str = "search_boost.status.";
pub const SEARCH_BOOST_HUB_KEY: &str = "search_boost.hub";
#[must_use]
pub fn parse_search_boost_value(value: &str) -> Option<f32> {
    let boost = value.parse::<f32>().ok()?;
    (boost.is_finite() && (0.0..=1.0).contains(&boost)).then_some(boost)
}

const fn runtime_config_declaration(
    key: RuntimeConfigKey,
    section: &'static str,
    name: &'static str,
    mode: RuntimeConfigValueMode,
) -> RuntimeConfigDeclaration {
    RuntimeConfigDeclaration {
        key,
        section,
        name,
        mode,
        lifecycle: RuntimeConfigLifecycle::Active,
    }
}

const fn obsolete_runtime_config_declaration(
    key: RuntimeConfigKey,
    section: &'static str,
    name: &'static str,
    mode: RuntimeConfigValueMode,
    lifecycle: RuntimeConfigLifecycle,
) -> RuntimeConfigDeclaration {
    RuntimeConfigDeclaration {
        key,
        section,
        name,
        mode,
        lifecycle,
    }
}

pub const RUNTIME_CONFIG_DECLARATIONS: &[RuntimeConfigDeclaration] = &[
    runtime_config_declaration(
        RuntimeConfigKey::CorpusRoot,
        "corpus",
        "root",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::CorpusExclude,
        "corpus",
        "exclude",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConvergenceOrdering,
        "convergence",
        "ordering",
        RuntimeConfigValueMode::OrderedList,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConvergenceActive,
        "convergence",
        "active",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConvergenceTerminal,
        "convergence",
        "terminal",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConvergenceAssertsCode,
        "convergence",
        "asserts_code",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConvergenceDescription,
        "convergence",
        "description",
        RuntimeConfigValueMode::Tuple {
            expected: "status and description",
            arity: 2,
        },
    ),
    runtime_config_declaration(
        RuntimeConfigKey::HandlesForce,
        "handles",
        "force",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::HandlesRejected,
        "handles",
        "rejected",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::HandlesLinear,
        "handles",
        "linear",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    obsolete_runtime_config_declaration(
        RuntimeConfigKey::HandlesConfirmed,
        "handles",
        "confirmed",
        RuntimeConfigValueMode::UnorderedSet,
        RuntimeConfigLifecycle::ObsoleteConfirmedNamespace,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::FrontmatterField,
        "frontmatter",
        "field",
        RuntimeConfigValueMode::Tuple {
            expected: "field, edge kind, and direction",
            arity: 3,
        },
    ),
    runtime_config_declaration(
        RuntimeConfigKey::FreshnessWarn,
        "freshness",
        "warn",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::FreshnessError,
        "freshness",
        "error",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::StateHistoryMode,
        "state",
        "history_mode",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::CheckDefaultFilter,
        "check",
        "default_filter",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::SuppressCode,
        "suppress",
        "code",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::SuppressRule,
        "suppress",
        "rule",
        RuntimeConfigValueMode::Tuple {
            expected: "code and target",
            arity: 2,
        },
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ConcernsGroup,
        "concerns",
        "group",
        RuntimeConfigValueMode::TupleList {
            expected: "name and one or more patterns",
            minimum: 2,
        },
    ),
    runtime_config_declaration(
        RuntimeConfigKey::ImpactTraverse,
        "impact",
        "traverse",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::AreasOrphanThreshold,
        "areas",
        "orphan_threshold",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::TemporalRecentDays,
        "temporal",
        "recent_days",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientEdgeWeight,
        "orient",
        "edge_weight",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientLabelWeight,
        "orient",
        "label_weight",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientRecencyWeight,
        "orient",
        "recency_weight",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientRecencyHalfLifeDays,
        "orient",
        "recency_half_life_days",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientBudget,
        "orient",
        "budget",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientDepth,
        "orient",
        "depth",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientPin,
        "orient",
        "pin",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientExclude,
        "orient",
        "exclude",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientStubBytes,
        "orient",
        "stub_bytes",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::OrientCuratedHubWeight,
        "orient",
        "curated_hub_weight",
        RuntimeConfigValueMode::Scalar,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::CodePathRoot,
        "code_path_root",
        "root",
        RuntimeConfigValueMode::UnorderedSet,
    ),
    runtime_config_declaration(
        RuntimeConfigKey::SearchBoostStatus,
        "search_boost",
        "status",
        RuntimeConfigValueMode::Tuple {
            expected: "status and boost",
            arity: 2,
        },
    ),
    runtime_config_declaration(
        RuntimeConfigKey::SearchBoostHub,
        "search_boost",
        "hub",
        RuntimeConfigValueMode::Scalar,
    ),
];

#[must_use]
pub fn runtime_config_declaration_for(
    section: &str,
    name: &str,
) -> Option<RuntimeConfigDeclaration> {
    RUNTIME_CONFIG_DECLARATIONS
        .iter()
        .copied()
        .find(|declaration| declaration.section == section && declaration.name == name)
        .or_else(|| {
            RUNTIME_CONFIG_DECLARATIONS
                .iter()
                .copied()
                .find(|declaration| declaration.section == section && declaration.name == "*")
        })
}

#[must_use]
pub fn runtime_config_declaration_by_key(
    key: RuntimeConfigKey,
) -> Option<RuntimeConfigDeclaration> {
    RUNTIME_CONFIG_DECLARATIONS
        .iter()
        .copied()
        .find(|declaration| declaration.key == key)
}

fn ordered_entries(
    key: &str,
    values: Vec<String>,
) -> Result<Vec<ConfigEntry>, RuntimeConfigEntryError> {
    values
        .into_iter()
        .enumerate()
        .map(|(idx, value)| {
            let ordinal = u32::try_from(idx).map_err(|_| {
                RuntimeConfigEntryError::OrderedConfigIndexOverflow { key: key.into() }
            })?;
            Ok(ConfigEntry::ordered(key, value, ordinal))
        })
        .collect()
}

fn validate_tuple_arity(
    declaration: RuntimeConfigDeclaration,
    actual: usize,
    expected: &'static str,
    arity: usize,
) -> Result<(), RuntimeConfigEntryError> {
    if actual == arity {
        Ok(())
    } else {
        Err(RuntimeConfigEntryError::InvalidArity {
            key: declaration.config_key(),
            expected,
            actual,
        })
    }
}

fn expect_exact_tuple<const N: usize>(
    declaration: RuntimeConfigDeclaration,
    values: Vec<String>,
    expected: &'static str,
) -> Result<[String; N], RuntimeConfigEntryError> {
    values.try_into().map_err(
        |values: Vec<String>| RuntimeConfigEntryError::InvalidArity {
            key: declaration.config_key(),
            expected,
            actual: values.len(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_schema_lowers_grouped_declarations() {
        let description =
            runtime_config_declaration_for("convergence", "description").expect("schema");
        assert_eq!(
            description
                .entries(vec!["draft".to_string(), "unsettled".to_string()])
                .expect("entries"),
            vec![ConfigEntry::scalar(
                "convergence.description.draft",
                "unsettled"
            )]
        );

        let frontmatter = runtime_config_declaration_for("frontmatter", "field").expect("schema");
        assert_eq!(
            frontmatter
                .entries(vec![
                    "depends-on".to_string(),
                    "DependsOn".to_string(),
                    "forward".to_string(),
                ])
                .expect("entries"),
            vec![
                ConfigEntry::scalar("frontmatter.field.depends-on.edge_kind", "DependsOn"),
                ConfigEntry::scalar("frontmatter.field.depends-on.direction", "forward"),
            ]
        );

        let search_status =
            runtime_config_declaration_for("search_boost", "status").expect("schema");
        assert_eq!(
            search_status
                .entries(vec!["authoritative".to_string(), "0.08".to_string()])
                .expect("entries"),
            vec![ConfigEntry::scalar(
                "search_boost.status.authoritative",
                "0.08"
            )]
        );

        let search_hub = runtime_config_declaration_for("search_boost", "hub").expect("schema");
        assert_eq!(
            search_hub
                .entries(vec!["0.01".to_string()])
                .expect("entries"),
            vec![ConfigEntry::scalar("search_boost.hub", "0.01")]
        );

        assert!(
            runtime_config_declaration_for("potential_weight", "freshness_decay").is_none(),
            "potential_weight overrides are retired; retune by shadowing potential_weight rules"
        );
    }

    #[test]
    fn runtime_config_schema_marks_confirmed_obsolete() {
        let confirmed = runtime_config_declaration_for("handles", "confirmed").expect("schema");
        assert_eq!(
            confirmed.lifecycle(),
            RuntimeConfigLifecycle::ObsoleteConfirmedNamespace
        );
        assert!(matches!(
            confirmed.entries(vec!["OQ".to_string()]),
            Err(RuntimeConfigEntryError::Obsolete(
                RuntimeConfigKey::HandlesConfirmed
            ))
        ));
    }

    #[test]
    fn runtime_config_schema_indexes_every_declaration_both_ways() {
        for declaration in RUNTIME_CONFIG_DECLARATIONS {
            assert_eq!(
                runtime_config_declaration_for(declaration.section(), declaration.name()),
                Some(*declaration)
            );
            assert_eq!(
                runtime_config_declaration_by_key(declaration.key()),
                Some(*declaration)
            );
        }
    }
}
