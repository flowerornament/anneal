use std::fs;
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::facts::SnapshotFact;
use crate::ids::CorpusId;
use crate::time::snapshot_days_since_epoch;

/// One append-only v2 snapshot entry in `.anneal/history.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub snapshot: String,
    pub at: String,
    pub corpus: CorpusId,
    pub facts: Vec<SnapshotEntryFact>,
}

impl SnapshotEntry {
    pub fn new(
        snapshot: impl Into<String>,
        at: impl Into<String>,
        corpus: CorpusId,
        facts: Vec<SnapshotEntryFact>,
    ) -> Self {
        Self {
            snapshot: snapshot.into(),
            at: at.into(),
            corpus,
            facts,
        }
    }

    pub fn to_snapshot_facts(&self) -> Vec<SnapshotFact> {
        self.facts
            .iter()
            .map(|fact| SnapshotFact {
                corpus: self.corpus.clone(),
                snapshot: self.snapshot.clone(),
                at: self.at.clone(),
                id: fact.id.clone(),
                key: fact.key.clone(),
                value: fact.value.clone(),
            })
            .collect()
    }
}

/// One key/value fact captured for a handle in a snapshot entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntryFact {
    pub id: String,
    pub key: String,
    pub value: String,
}

impl SnapshotEntryFact {
    pub fn new(id: impl Into<String>, key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Parsed snapshot history plus recoverable read warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotHistory {
    entries: Vec<SnapshotEntry>,
    warnings: Vec<HistoryWarning>,
}

impl SnapshotHistory {
    pub fn from_entries(entries: Vec<SnapshotEntry>) -> Self {
        let mut history = Self::default();
        for entry in entries {
            match validate_snapshot_entry(&entry) {
                Ok(()) => history.entries.push(entry),
                Err(message) => history.warnings.push(HistoryWarning { line: 0, message }),
            }
        }
        history
    }

    pub fn entries(&self) -> &[SnapshotEntry] {
        &self.entries
    }

    pub fn warnings(&self) -> &[HistoryWarning] {
        &self.warnings
    }

    pub fn snapshot_facts(&self) -> Vec<SnapshotFact> {
        self.entries
            .iter()
            .flat_map(SnapshotEntry::to_snapshot_facts)
            .collect()
    }
}

/// Non-fatal history read warning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryWarning {
    pub line: usize,
    pub message: String,
}

/// Repository-local v2 history path.
pub fn repo_history_path(root: &Utf8Path) -> Utf8PathBuf {
    root.join(".anneal/history.jsonl")
}

/// Append one v2 snapshot entry as a single JSON line.
pub fn append_snapshot_entry(root: &Utf8Path, entry: &SnapshotEntry) -> Result<(), HistoryError> {
    let path = repo_history_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent.as_std_path()).map_err(|source| HistoryError::Io {
            path: parent.to_string(),
            source,
        })?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())
        .map_err(|source| HistoryError::Io {
            path: path.to_string(),
            source,
        })?;
    let mut buf = serde_json::to_vec(entry).map_err(HistoryError::Encode)?;
    buf.push(b'\n');
    file.write_all(&buf).map_err(|source| HistoryError::Io {
        path: path.to_string(),
        source,
    })?;
    Ok(())
}

/// Read repository-local v2 snapshot history.
///
/// Missing history returns an empty history. Unparseable lines are skipped and
/// reported as structured warnings so a truncated append cannot poison all
/// future time-travel queries.
pub fn read_snapshot_history(root: &Utf8Path) -> Result<SnapshotHistory, HistoryError> {
    let path = repo_history_path(root);
    let file = match fs::File::open(path.as_std_path()) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SnapshotHistory::default());
        }
        Err(source) => {
            return Err(HistoryError::Io {
                path: path.to_string(),
                source,
            });
        }
    };

    let reader = BufReader::new(file);
    let mut history = SnapshotHistory::default();
    for (line_index, line) in reader.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|source| HistoryError::Io {
            path: path.to_string(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<SnapshotEntry>(&line) {
            Ok(entry) => match validate_snapshot_entry(&entry) {
                Ok(()) => history.entries.push(entry),
                Err(message) => history.warnings.push(HistoryWarning {
                    line: line_number,
                    message,
                }),
            },
            Err(err) => history.warnings.push(HistoryWarning {
                line: line_number,
                message: err.to_string(),
            }),
        }
    }
    Ok(history)
}

fn validate_snapshot_entry(entry: &SnapshotEntry) -> Result<(), String> {
    if entry.snapshot.is_empty() {
        return Err("snapshot id is empty".to_string());
    }
    if snapshot_days_since_epoch(&entry.at).is_none() {
        return Err(format!(
            "snapshot timestamp {:?} is not parseable",
            entry.at
        ));
    }
    for fact in &entry.facts {
        if fact.id.is_empty() {
            return Err("snapshot fact id is empty".to_string());
        }
        if fact.key.is_empty() {
            return Err(format!("snapshot fact for {:?} has empty key", fact.id));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("could not encode snapshot history entry: {0}")]
    Encode(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_snapshot_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let first = SnapshotEntry::new(
            "s1",
            "2026-05-13T10:00:00Z",
            CorpusId::from("test"),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );
        let second = SnapshotEntry::new(
            "s2",
            "2026-05-14T10:00:00Z",
            CorpusId::from("test"),
            vec![SnapshotEntryFact::new("a.md", "status", "current")],
        );

        append_snapshot_entry(&root, &first).expect("append first");
        append_snapshot_entry(&root, &second).expect("append second");

        let history = read_snapshot_history(&root).expect("read history");
        assert_eq!(history.entries(), &[first, second]);
        assert!(history.warnings().is_empty());
        assert_eq!(history.snapshot_facts().len(), 2);
    }

    #[test]
    fn read_snapshot_history_skips_bad_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let entry = SnapshotEntry::new(
            "s1",
            "2026-05-13",
            CorpusId::from("test"),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );
        append_snapshot_entry(&root, &entry).expect("append first");
        let path = repo_history_path(&root);
        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(path.as_std_path())
                .expect("open history");
            writeln!(file, "{{not-json").expect("append bad line");
        }

        let history = read_snapshot_history(&root).expect("read history");
        assert_eq!(history.entries(), &[entry]);
        assert_eq!(history.warnings().len(), 1);
        assert_eq!(history.warnings()[0].line, 2);
    }

    #[test]
    fn read_snapshot_history_skips_invalid_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        append_snapshot_entry(
            &root,
            &SnapshotEntry::new(
                "s1",
                "2026-05-13T10:00:00Z",
                CorpusId::from("test"),
                vec![SnapshotEntryFact::new("a.md", "status", "draft")],
            ),
        )
        .expect("append valid entry");
        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(repo_history_path(&root).as_std_path())
                .expect("open history");
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "snapshot": "bad",
                    "at": "2026-99-99",
                    "corpus": "test",
                    "facts": [{"id": "a.md", "key": "status", "value": "current"}]
                })
            )
            .expect("append invalid entry");
        }

        let history = read_snapshot_history(&root).expect("read history");

        assert_eq!(history.entries().len(), 1);
        assert_eq!(history.warnings().len(), 1);
        assert_eq!(history.warnings()[0].line, 2);
    }

    #[test]
    fn from_entries_validates_snapshot_entries() {
        let history = SnapshotHistory::from_entries(vec![
            SnapshotEntry::new(
                "",
                "2026-05-13",
                CorpusId::from("test"),
                vec![SnapshotEntryFact::new("a.md", "status", "draft")],
            ),
            SnapshotEntry::new(
                "s1",
                "2026-05-13",
                CorpusId::from("test"),
                vec![SnapshotEntryFact::new("a.md", "status", "draft")],
            ),
        ]);

        assert_eq!(history.entries().len(), 1);
        assert_eq!(history.warnings().len(), 1);
        assert_eq!(history.warnings()[0].line, 0);
    }

    #[test]
    fn missing_history_reads_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");

        let history = read_snapshot_history(&root).expect("read missing history");

        assert!(history.entries().is_empty());
        assert!(history.warnings().is_empty());
    }
}
