use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::{ConfigFact, CorpusId};

#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error("failed to read runtime config {path}: {source}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse runtime config {path}: {source}")]
    Parse {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("{field} contains a non-string value")]
    NonStringArrayValue { field: String },
    #[error("{field} contains a non-integer value")]
    NonIntegerValue { field: String },
    #[error("ordered config index overflowed u32")]
    OrderedIndexOverflow,
}

pub fn load_runtime_configs(
    root: &Utf8Path,
    corpus: &CorpusId,
) -> Result<Vec<ConfigFact>, RuntimeConfigError> {
    let path = root.join("anneal.toml");
    let text = fs::read_to_string(&path).map_err(|source| RuntimeConfigError::Read {
        path: path.clone(),
        source,
    })?;
    parse_runtime_configs(&path, &text, corpus)
}

pub fn load_runtime_configs_if_present(
    root: &Utf8Path,
    corpus: &CorpusId,
) -> Result<Vec<ConfigFact>, RuntimeConfigError> {
    match load_runtime_configs(root, corpus) {
        Ok(configs) => Ok(configs),
        Err(RuntimeConfigError::Read { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn parse_runtime_configs(
    path: &Utf8Path,
    text: &str,
    corpus: &CorpusId,
) -> Result<Vec<ConfigFact>, RuntimeConfigError> {
    let value = text
        .parse::<toml::Value>()
        .map_err(|source| RuntimeConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    let mut configs = Vec::new();
    if let Some(convergence) = value.get("convergence").and_then(toml::Value::as_table) {
        push_string_array(
            &mut configs,
            corpus,
            convergence,
            "active",
            "convergence.active",
        )?;
        push_string_array(
            &mut configs,
            corpus,
            convergence,
            "terminal",
            "convergence.terminal",
        )?;
        push_ordered_string_array(
            &mut configs,
            corpus,
            convergence,
            "ordering",
            "convergence.ordering",
        )?;
    }
    if let Some(handles) = value.get("handles").and_then(toml::Value::as_table) {
        push_string_array(
            &mut configs,
            corpus,
            handles,
            "confirmed",
            "handles.confirmed",
        )?;
        push_string_array(
            &mut configs,
            corpus,
            handles,
            "rejected",
            "handles.rejected",
        )?;
        push_string_array(&mut configs, corpus, handles, "linear", "handles.linear")?;
    }
    if let Some(freshness) = value.get("freshness").and_then(toml::Value::as_table) {
        push_integer_config(&mut configs, corpus, freshness, "warn", "freshness.warn")?;
        push_integer_config(&mut configs, corpus, freshness, "error", "freshness.error")?;
    }
    Ok(configs)
}

fn push_string_array(
    configs: &mut Vec<ConfigFact>,
    corpus: &CorpusId,
    table: &toml::value::Table,
    field: &str,
    key: &str,
) -> Result<(), RuntimeConfigError> {
    let Some(values) = table.get(field).and_then(toml::Value::as_array) else {
        return Ok(());
    };
    for value in values {
        let value = value
            .as_str()
            .ok_or_else(|| RuntimeConfigError::NonStringArrayValue {
                field: field.to_string(),
            })?;
        configs.push(ConfigFact {
            corpus: corpus.clone(),
            key: key.to_string(),
            value: value.to_string(),
            ordinal: None,
        });
    }
    Ok(())
}

fn push_ordered_string_array(
    configs: &mut Vec<ConfigFact>,
    corpus: &CorpusId,
    table: &toml::value::Table,
    field: &str,
    key: &str,
) -> Result<(), RuntimeConfigError> {
    let Some(values) = table.get(field).and_then(toml::Value::as_array) else {
        return Ok(());
    };
    for (idx, value) in values.iter().enumerate() {
        let value = value
            .as_str()
            .ok_or_else(|| RuntimeConfigError::NonStringArrayValue {
                field: field.to_string(),
            })?;
        configs.push(ConfigFact {
            corpus: corpus.clone(),
            key: key.to_string(),
            value: value.to_string(),
            ordinal: Some(
                u32::try_from(idx).map_err(|_| RuntimeConfigError::OrderedIndexOverflow)?,
            ),
        });
    }
    Ok(())
}

fn push_integer_config(
    configs: &mut Vec<ConfigFact>,
    corpus: &CorpusId,
    table: &toml::value::Table,
    field: &str,
    key: &str,
) -> Result<(), RuntimeConfigError> {
    let Some(value) = table.get(field) else {
        return Ok(());
    };
    let value = value
        .as_integer()
        .ok_or_else(|| RuntimeConfigError::NonIntegerValue {
            field: field.to_string(),
        })?;
    configs.push(ConfigFact {
        corpus: corpus.clone(),
        key: key.to_string(),
        value: value.to_string(),
        ordinal: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_ordered_and_scalar_runtime_config_facts() {
        let root = tempfile::tempdir().expect("tempdir");
        fs::write(
            root.path().join("anneal.toml"),
            r#"
            [convergence]
            active = ["draft", "active"]
            terminal = ["settled"]
            ordering = ["draft", "active", "settled"]

            [handles]
            linear = ["OQ"]

            [freshness]
            warn = 30
            "#,
        )
        .expect("write config");
        let root = Utf8Path::from_path(root.path()).expect("utf8 tempdir");
        let corpus = CorpusId::from("test");

        let configs = load_runtime_configs(root, &corpus).expect("configs load");

        assert!(configs.iter().any(|fact| fact.key == "convergence.active"
            && fact.value == "draft"
            && fact.ordinal.is_none()));
        assert!(configs.iter().any(|fact| fact.key == "handles.linear"
            && fact.value == "OQ"
            && fact.ordinal.is_none()));
        assert!(configs.iter().any(|fact| fact.key == "convergence.ordering"
            && fact.value == "active"
            && fact.ordinal == Some(1)));
        assert!(configs.iter().any(|fact| fact.key == "freshness.warn"
            && fact.value == "30"
            && fact.ordinal.is_none()));
    }

    #[test]
    fn optional_runtime_config_returns_empty_when_missing() {
        let root = tempfile::tempdir().expect("tempdir");
        let root = Utf8Path::from_path(root.path()).expect("utf8 tempdir");
        let corpus = CorpusId::from("test");

        let configs = load_runtime_configs_if_present(root, &corpus).expect("optional config");

        assert!(configs.is_empty());
    }
}
