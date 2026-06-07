use std::fs;
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::facts::SnapshotFact;
use crate::ids::CorpusId;
use crate::runtime::prelude::PreludeSet;
use crate::time::snapshot_days_since_epoch;

/// One append-only snapshot entry in `.anneal/history.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub snapshot: String,
    pub at: String,
    pub corpus: CorpusId,
    #[serde(default = "unknown_prelude_hash")]
    pub prelude_hash: String,
    pub facts: Vec<SnapshotEntryFact>,
}

impl SnapshotEntry {
    pub fn new(
        snapshot: impl Into<String>,
        at: impl Into<String>,
        corpus: CorpusId,
        prelude: &PreludeSet,
        facts: Vec<SnapshotEntryFact>,
    ) -> Self {
        Self::with_prelude_hash(snapshot, at, corpus, prelude.hash().to_string(), facts)
    }

    pub fn with_prelude_hash(
        snapshot: impl Into<String>,
        at: impl Into<String>,
        corpus: CorpusId,
        prelude_hash: impl Into<String>,
        facts: Vec<SnapshotEntryFact>,
    ) -> Self {
        Self {
            snapshot: snapshot.into(),
            at: at.into(),
            corpus,
            prelude_hash: prelude_hash.into(),
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

fn unknown_prelude_hash() -> String {
    "unknown".to_string()
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

/// Repository-local snapshot history path.
pub fn repo_history_path(root: &Utf8Path) -> Utf8PathBuf {
    root.join(".anneal/history.jsonl")
}

/// Append one snapshot entry as a single JSON line.
pub fn append_snapshot_entry(root: &Utf8Path, entry: &SnapshotEntry) -> Result<(), HistoryError> {
    validate_snapshot_entry(entry).map_err(HistoryError::InvalidEntry)?;

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

/// Outcome from a capped snapshot-history write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotAppendOutcome {
    Appended,
    SkippedDuplicate,
}

/// Append one snapshot entry while keeping only the latest `max_entries`.
///
/// If the latest valid entry already captures the same corpus/prelude/facts,
/// the new timestamp is ignored. This lets frequent automatic snapshots avoid
/// turning unchanged `status` reads into noisy history churn.
pub fn append_snapshot_entry_capped(
    root: &Utf8Path,
    entry: &SnapshotEntry,
    max_entries: usize,
) -> Result<SnapshotAppendOutcome, HistoryError> {
    validate_snapshot_entry(entry).map_err(HistoryError::InvalidEntry)?;

    let mut file = read_snapshot_history_file(root)?;
    if file
        .history
        .entries
        .last()
        .is_some_and(|latest| snapshot_state_matches(latest, entry))
    {
        return Ok(SnapshotAppendOutcome::SkippedDuplicate);
    }

    file.history.entries.push(entry.clone());
    let keep = max_entries.max(1);
    let start = file.history.entries.len().saturating_sub(keep);
    write_snapshot_entries(root, &file.preserved_lines, &file.history.entries[start..])?;
    Ok(SnapshotAppendOutcome::Appended)
}

fn snapshot_state_matches(left: &SnapshotEntry, right: &SnapshotEntry) -> bool {
    left.corpus == right.corpus
        && left.prelude_hash == right.prelude_hash
        && left.facts == right.facts
}

fn write_snapshot_entries(
    root: &Utf8Path,
    preserved_lines: &[String],
    entries: &[SnapshotEntry],
) -> Result<(), HistoryError> {
    let path = repo_history_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent.as_std_path()).map_err(|source| HistoryError::Io {
            path: parent.to_string(),
            source,
        })?;
    }

    let mut buf = Vec::new();
    for line in preserved_lines {
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
    }
    for entry in entries {
        validate_snapshot_entry(entry).map_err(HistoryError::InvalidEntry)?;
        serde_json::to_writer(&mut buf, entry).map_err(HistoryError::Encode)?;
        buf.push(b'\n');
    }
    fs::write(path.as_std_path(), buf).map_err(|source| HistoryError::Io {
        path: path.to_string(),
        source,
    })
}

/// Read repository-local snapshot history.
///
/// Missing history returns an empty history. Unparseable lines are skipped and
/// reported as structured warnings so a truncated append cannot poison all
/// future time-travel queries.
pub fn read_snapshot_history(root: &Utf8Path) -> Result<SnapshotHistory, HistoryError> {
    Ok(read_snapshot_history_file(root)?.history)
}

struct SnapshotHistoryFile {
    history: SnapshotHistory,
    preserved_lines: Vec<String>,
}

fn read_snapshot_history_file(root: &Utf8Path) -> Result<SnapshotHistoryFile, HistoryError> {
    let path = repo_history_path(root);
    let file = match fs::File::open(path.as_std_path()) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SnapshotHistoryFile {
                history: SnapshotHistory::default(),
                preserved_lines: Vec::new(),
            });
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
    let mut preserved_lines = Vec::new();
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
            Err(err) => {
                if is_legacy_snapshot_line(&line) {
                    preserved_lines.push(line);
                } else {
                    history.warnings.push(HistoryWarning {
                        line: line_number,
                        message: err.to_string(),
                    });
                }
            }
        }
    }
    Ok(SnapshotHistoryFile {
        history,
        preserved_lines,
    })
}

fn is_legacy_snapshot_line(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("timestamp")
        && object.contains_key("handles")
        && object.contains_key("edges")
        && object.contains_key("states")
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
    #[error("invalid snapshot history entry: {0}")]
    InvalidEntry(String),
    #[error("could not encode snapshot history entry: {0}")]
    Encode(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::prelude::standard_prelude_set;

    #[test]
    fn append_and_read_snapshot_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let first = SnapshotEntry::new(
            "s1",
            "2026-05-13T10:00:00Z",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );
        let second = SnapshotEntry::new(
            "s2",
            "2026-05-14T10:00:00Z",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "current")],
        );

        append_snapshot_entry(&root, &first).expect("append first");
        append_snapshot_entry(&root, &second).expect("append second");

        let history = read_snapshot_history(&root).expect("read history");
        assert_eq!(history.entries(), &[first, second]);
        assert!(history.warnings().is_empty());
        assert_eq!(history.snapshot_facts().len(), 2);
        assert_eq!(
            history.entries()[0].prelude_hash,
            standard_prelude_set().hash().to_string()
        );
    }

    #[test]
    fn snapshot_entry_can_record_custom_prelude_hash() {
        let entry = SnapshotEntry::with_prelude_hash(
            "s1",
            "2026-05-13T10:00:00Z",
            CorpusId::from("test"),
            "custom-hash",
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );

        assert_eq!(entry.prelude_hash, "custom-hash");
    }

    #[test]
    fn missing_snapshot_prelude_hash_deserializes_as_unknown() {
        let entry: SnapshotEntry = serde_json::from_str(
            r#"{"snapshot":"s1","at":"2026-05-13T10:00:00Z","corpus":"test","facts":[]}"#,
        )
        .expect("legacy snapshot entry decodes");

        assert_eq!(entry.prelude_hash, "unknown");
    }

    #[test]
    fn read_snapshot_history_skips_bad_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let entry = SnapshotEntry::new(
            "s1",
            "2026-05-13",
            CorpusId::from("test"),
            standard_prelude_set(),
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
                standard_prelude_set(),
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
    fn read_snapshot_history_skips_timestamp_with_valid_date_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        append_snapshot_entry(
            &root,
            &SnapshotEntry::new(
                "s1",
                "2026-05-13T10:00:00Z",
                CorpusId::from("test"),
                standard_prelude_set(),
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
                    "at": "2026-05-13junk",
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
    fn append_snapshot_entry_rejects_invalid_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let entry = SnapshotEntry::new(
            "s1",
            "2026-05-13junk",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );

        let err = append_snapshot_entry(&root, &entry).expect_err("entry rejected");

        assert!(
            matches!(err, HistoryError::InvalidEntry(message) if message.contains("timestamp"))
        );
        let history = read_snapshot_history(&root).expect("read missing history");
        assert!(history.entries().is_empty());
    }

    #[test]
    fn from_entries_validates_snapshot_entries() {
        let history = SnapshotHistory::from_entries(vec![
            SnapshotEntry::new(
                "",
                "2026-05-13",
                CorpusId::from("test"),
                standard_prelude_set(),
                vec![SnapshotEntryFact::new("a.md", "status", "draft")],
            ),
            SnapshotEntry::new(
                "s1",
                "2026-05-13",
                CorpusId::from("test"),
                standard_prelude_set(),
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

    #[test]
    fn capped_append_retains_latest_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        for idx in 1..=3 {
            append_snapshot_entry_capped(
                &root,
                &SnapshotEntry::new(
                    format!("s{idx}"),
                    format!("2026-05-1{idx}T10:00:00Z"),
                    CorpusId::from("test"),
                    standard_prelude_set(),
                    vec![SnapshotEntryFact::new("a.md", "status", format!("s{idx}"))],
                ),
                2,
            )
            .expect("append capped");
        }

        let history = read_snapshot_history(&root).expect("read history");
        let snapshots = history
            .entries()
            .iter()
            .map(|entry| entry.snapshot.as_str())
            .collect::<Vec<_>>();

        assert_eq!(snapshots, ["s2", "s3"]);
    }

    #[test]
    fn capped_append_preserves_legacy_snapshot_lines_without_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let path = repo_history_path(&root);
        fs::create_dir_all(path.parent().expect("history parent").as_std_path())
            .expect("create history parent");
        let legacy = serde_json::json!({
            "timestamp": "2026-03-29T20:33:58.159171+00:00",
            "handles": {"total": 67, "active": 67, "frozen": 0},
            "edges": {"total": 0},
            "states": {"draft": 1},
            "obligations": {"outstanding": 0, "discharged": 0, "mooted": 0},
            "diagnostics": {"errors": 1, "warnings": 0},
            "namespaces": {}
        })
        .to_string();
        fs::write(path.as_std_path(), format!("{legacy}\n")).expect("write legacy history");

        let entry = SnapshotEntry::new(
            "s1",
            "2026-05-13T10:00:00Z",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );

        append_snapshot_entry_capped(&root, &entry, 100).expect("append capped");

        let contents = fs::read_to_string(path.as_std_path()).expect("read history file");
        assert!(contents.starts_with(&legacy));

        let history = read_snapshot_history(&root).expect("read history");
        assert_eq!(history.entries(), &[entry]);
        assert!(history.warnings().is_empty());
    }

    #[test]
    fn capped_append_skips_duplicate_latest_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 root");
        let first = SnapshotEntry::new(
            "s1",
            "2026-05-13T10:00:00Z",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );
        let duplicate = SnapshotEntry::new(
            "s2",
            "2026-05-13T10:01:00Z",
            CorpusId::from("test"),
            standard_prelude_set(),
            vec![SnapshotEntryFact::new("a.md", "status", "draft")],
        );

        assert_eq!(
            append_snapshot_entry_capped(&root, &first, 100).expect("append first"),
            SnapshotAppendOutcome::Appended
        );
        assert_eq!(
            append_snapshot_entry_capped(&root, &duplicate, 100).expect("skip duplicate"),
            SnapshotAppendOutcome::SkippedDuplicate
        );

        let history = read_snapshot_history(&root).expect("read history");

        assert_eq!(history.entries(), &[first]);
    }
}
