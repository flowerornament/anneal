//! Code-target existence and history probes for source references.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::path_policy::{RelativePathPolicy, normalize_relative_path};

const DRIFT_CACHE_SCHEMA_VERSION: u32 = 1;
const PATH_POLICY_VERSION: u32 = 1;
const DRIFT_CACHE_RELATIVE_PATH: &str = ".anneal/drift-evidence.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetExistence {
    True,
    False,
    Unknown,
}

impl TargetExistence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetHistoryStatus {
    Present,
    Absent,
    Unavailable,
}

impl TargetHistoryStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTargetProbe {
    pub exists: TargetExistence,
    pub history_status: TargetHistoryStatus,
    pub probe_base: Option<Utf8PathBuf>,
    pub resolved_path: Option<Utf8PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeDriftEvidenceRequest {
    pub ref_handle: String,
    pub target_path: String,
    pub edge_file: String,
    pub assertion_date: Option<String>,
    pub assertion_revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeDriftEvidence {
    pub disposition: String,
    pub commits_since_assertion: Option<u32>,
    pub moved_to: Option<String>,
    pub move_candidates: Vec<String>,
    pub evidence_head: String,
    pub assertion_premise: String,
    pub cost_ms: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeDriftEvidenceMode {
    Disabled,
    ReadCache,
    Refresh,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct DriftEvidenceFile {
    schema_version: u32,
    path_policy_version: u32,
    entries: BTreeMap<String, CachedCodeDriftEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedCodeDriftEvidence {
    ref_handle: String,
    target_path: String,
    assertion_premise: String,
    repo_root: String,
    head: String,
    disposition: String,
    commits_since_assertion: Option<u32>,
    moved_to: Option<String>,
    move_candidates: Vec<String>,
    cost_ms: u128,
}

#[derive(Debug)]
pub struct CodeDriftEvidenceCache {
    mode: CodeDriftEvidenceMode,
    repo_root: Utf8PathBuf,
    cache_path: Utf8PathBuf,
    head: Option<String>,
    entries: BTreeMap<String, CachedCodeDriftEvidence>,
    changed: bool,
}

impl CodeDriftEvidenceCache {
    #[must_use]
    pub fn open(corpus_root: &Utf8Path, mode: CodeDriftEvidenceMode) -> Self {
        let repo_root =
            enclosing_project_root(corpus_root).unwrap_or_else(|| corpus_root.to_path_buf());
        let cache_path = repo_root.join(DRIFT_CACHE_RELATIVE_PATH);
        let head = git_head(&repo_root);
        let entries = if matches!(mode, CodeDriftEvidenceMode::Disabled) {
            BTreeMap::new()
        } else {
            read_drift_cache(&cache_path, head.as_deref(), &repo_root)
        };
        Self {
            mode,
            repo_root,
            cache_path,
            head,
            entries,
            changed: false,
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, CodeDriftEvidenceMode::Disabled)
    }

    pub fn evidence_for(
        &mut self,
        request: &CodeDriftEvidenceRequest,
    ) -> Option<CodeDriftEvidence> {
        if !self.is_enabled() {
            return None;
        }
        let Some(head) = self.head.clone() else {
            return Some(CodeDriftEvidence {
                disposition: "referent-unknown".to_string(),
                commits_since_assertion: None,
                moved_to: None,
                move_candidates: Vec::new(),
                evidence_head: "unknown".to_string(),
                assertion_premise: assertion_premise(request),
                cost_ms: 0,
            });
        };
        let key = drift_cache_key(&self.repo_root, &head, request);
        if let Some(entry) = self.entries.get(&key) {
            return Some(entry.to_evidence());
        }
        if matches!(self.mode, CodeDriftEvidenceMode::ReadCache)
            && assertion_premise(request) == "assertion_date_unknown"
            && let Some(entry) = self.entries.values().find(|entry| {
                entry.head == head
                    && entry.ref_handle == request.ref_handle
                    && entry.target_path == request.target_path
            })
        {
            return Some(entry.to_evidence());
        }
        if !matches!(self.mode, CodeDriftEvidenceMode::Refresh) {
            return None;
        }
        let evidence = compute_drift_evidence(&self.repo_root, &head, request);
        self.entries.insert(
            key,
            CachedCodeDriftEvidence::from_evidence(&self.repo_root, request, &evidence),
        );
        self.changed = true;
        Some(evidence)
    }

    pub fn save(&self) -> std::io::Result<()> {
        if !matches!(self.mode, CodeDriftEvidenceMode::Refresh) || !self.changed {
            return Ok(());
        }
        if let Some(parent) = self.cache_path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }
        let file = DriftEvidenceFile {
            schema_version: DRIFT_CACHE_SCHEMA_VERSION,
            path_policy_version: PATH_POLICY_VERSION,
            entries: self.entries.clone(),
        };
        let body = serde_json::to_string_pretty(&file)?;
        std::fs::write(self.cache_path.as_std_path(), body)
    }
}

impl CodeTargetProbe {
    fn unknown() -> Self {
        Self {
            exists: TargetExistence::Unknown,
            history_status: TargetHistoryStatus::Unavailable,
            probe_base: None,
            resolved_path: None,
        }
    }
}

#[derive(Default)]
pub struct CodeTargetProbeCache {
    history_by_base: BTreeMap<Utf8PathBuf, Option<BTreeSet<String>>>,
}

impl CodeTargetProbeCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn probe(&mut self, corpus_root: &Utf8Path, target_path: &str) -> CodeTargetProbe {
        probe_code_target_with_cache(corpus_root, target_path, self)
    }

    pub fn probe_without_history(
        &mut self,
        corpus_root: &Utf8Path,
        target_path: &str,
    ) -> CodeTargetProbe {
        probe_code_target_without_history(corpus_root, target_path)
    }

    fn history_contains_target(&mut self, base: &Utf8Path, target: &Utf8Path) -> Option<bool> {
        let history = self
            .history_by_base
            .entry(base.to_path_buf())
            .or_insert_with(|| read_head_history_paths(base));
        history
            .as_ref()
            .map(|paths| paths.contains(target.as_str()))
    }

    fn target_history_status(&mut self, base: &Utf8Path, target: &Utf8Path) -> TargetHistoryStatus {
        match self.history_contains_target(base, target) {
            Some(true) => TargetHistoryStatus::Present,
            Some(false) => TargetHistoryStatus::Absent,
            None => TargetHistoryStatus::Unavailable,
        }
    }
}

impl CachedCodeDriftEvidence {
    fn to_evidence(&self) -> CodeDriftEvidence {
        CodeDriftEvidence {
            disposition: self.disposition.clone(),
            commits_since_assertion: self.commits_since_assertion,
            moved_to: self.moved_to.clone(),
            move_candidates: self.move_candidates.clone(),
            evidence_head: self.head.clone(),
            assertion_premise: self.assertion_premise.clone(),
            cost_ms: self.cost_ms,
        }
    }

    fn from_evidence(
        repo_root: &Utf8Path,
        request: &CodeDriftEvidenceRequest,
        evidence: &CodeDriftEvidence,
    ) -> Self {
        Self {
            ref_handle: request.ref_handle.clone(),
            target_path: request.target_path.clone(),
            assertion_premise: evidence.assertion_premise.clone(),
            repo_root: repo_root.to_string(),
            head: evidence.evidence_head.clone(),
            disposition: evidence.disposition.clone(),
            commits_since_assertion: evidence.commits_since_assertion,
            moved_to: evidence.moved_to.clone(),
            move_candidates: evidence.move_candidates.clone(),
            cost_ms: evidence.cost_ms,
        }
    }
}

fn read_drift_cache(
    cache_path: &Utf8Path,
    head: Option<&str>,
    repo_root: &Utf8Path,
) -> BTreeMap<String, CachedCodeDriftEvidence> {
    let Some(head) = head else {
        return BTreeMap::new();
    };
    let Ok(body) = std::fs::read_to_string(cache_path.as_std_path()) else {
        return BTreeMap::new();
    };
    let Ok(file) = serde_json::from_str::<DriftEvidenceFile>(&body) else {
        return BTreeMap::new();
    };
    if file.schema_version != DRIFT_CACHE_SCHEMA_VERSION
        || file.path_policy_version != PATH_POLICY_VERSION
    {
        return BTreeMap::new();
    }
    file.entries
        .into_iter()
        .filter(|(_, entry)| {
            entry.head == head
                && entry.repo_root == repo_root.as_str()
                && revision_exists(repo_root, &entry.head)
                && assertion_revision_valid(repo_root, &entry.assertion_premise)
        })
        .collect()
}

fn drift_cache_key(repo_root: &Utf8Path, head: &str, request: &CodeDriftEvidenceRequest) -> String {
    [
        repo_root.as_str(),
        head,
        request.ref_handle.as_str(),
        request.target_path.as_str(),
        assertion_premise(request).as_str(),
        "schema=1",
        "path_policy=1",
    ]
    .join("|")
}

fn assertion_premise(request: &CodeDriftEvidenceRequest) -> String {
    if let Some(revision) = request
        .assertion_revision
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return format!("assertion_revision:{revision}");
    }
    if let Some(date) = request
        .assertion_date
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return format!("assertion_date:{date}");
    }
    "assertion_date_unknown".to_string()
}

fn assertion_revision_valid(repo_root: &Utf8Path, assertion_premise: &str) -> bool {
    let Some(revision) = assertion_premise.strip_prefix("assertion_revision:") else {
        return true;
    };
    revision_exists(repo_root, revision)
}

fn compute_drift_evidence(
    repo_root: &Utf8Path,
    head: &str,
    request: &CodeDriftEvidenceRequest,
) -> CodeDriftEvidence {
    let started = Instant::now();
    let assertion_premise = assertion_premise(request);
    let Some(target) = normalize_probe_target(repo_root, &request.target_path) else {
        return CodeDriftEvidence {
            disposition: "referent-unknown".to_string(),
            commits_since_assertion: None,
            moved_to: None,
            move_candidates: Vec::new(),
            evidence_head: head.to_string(),
            assertion_premise,
            cost_ms: started.elapsed().as_millis(),
        };
    };
    if relevant_worktree_dirty(repo_root, &target, &request.edge_file) {
        return CodeDriftEvidence {
            disposition: "evidence_dirty_worktree".to_string(),
            commits_since_assertion: None,
            moved_to: None,
            move_candidates: Vec::new(),
            evidence_head: head.to_string(),
            assertion_premise,
            cost_ms: started.elapsed().as_millis(),
        };
    }

    let exists_now = repo_root.join(&target).is_file();
    let history_status = target_history_status(repo_root, &target);
    let commits_since_assertion = commits_since_assertion(repo_root, &target, request);
    let mut move_candidates = Vec::new();
    let mut moved_to = None;
    let disposition = if exists_now {
        match commits_since_assertion {
            None => "referent-present-undated".to_string(),
            Some(0) => "referent-intact".to_string(),
            Some(count) => format!("referent-drifted({count})"),
        }
    } else if history_status == TargetHistoryStatus::Present {
        move_candidates = find_move_candidates(repo_root, &target);
        match move_candidates.as_slice() {
            [candidate] => {
                moved_to = Some(candidate.clone());
                "referent-moved".to_string()
            }
            [] => "referent-gone".to_string(),
            _ => "referent-moved-ambiguous".to_string(),
        }
    } else {
        "referent-unknown".to_string()
    };

    CodeDriftEvidence {
        disposition,
        commits_since_assertion,
        moved_to,
        move_candidates,
        evidence_head: head.to_string(),
        assertion_premise,
        cost_ms: started.elapsed().as_millis(),
    }
}

fn normalize_probe_target(repo_root: &Utf8Path, raw_path: &str) -> Option<Utf8PathBuf> {
    let path = Utf8Path::new(raw_path);
    if path.is_absolute() {
        return path.strip_prefix(repo_root).ok().map(Utf8Path::to_path_buf);
    }
    normalize_relative_path(raw_path, RelativePathPolicy::STRICT_NON_EMPTY)
}

fn git_head(repo_root: &Utf8Path) -> Option<String> {
    git_output(repo_root, &["rev-parse", "HEAD"]).and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn revision_exists(repo_root: &Utf8Path, revision: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root.as_std_path())
        .args(["cat-file", "-e", revision])
        .status()
        .is_ok_and(|status| status.success())
}

fn target_history_status(repo_root: &Utf8Path, target: &Utf8Path) -> TargetHistoryStatus {
    git_output(
        repo_root,
        &[
            "log",
            "--all",
            "--format=%H",
            "--max-count=1",
            "--",
            target.as_str(),
        ],
    )
    .map_or(TargetHistoryStatus::Unavailable, |output| {
        if output.trim().is_empty() {
            TargetHistoryStatus::Absent
        } else {
            TargetHistoryStatus::Present
        }
    })
}

fn commits_since_assertion(
    repo_root: &Utf8Path,
    target: &Utf8Path,
    request: &CodeDriftEvidenceRequest,
) -> Option<u32> {
    if let Some(revision) = request
        .assertion_revision
        .as_deref()
        .filter(|value| !value.is_empty() && revision_exists(repo_root, value))
    {
        return git_output(
            repo_root,
            &[
                "log",
                "--all",
                "--format=%H",
                &format!("{revision}..HEAD"),
                "--",
                target.as_str(),
            ],
        )
        .map(|output| nonempty_line_count(&output));
    }
    request
        .assertion_date
        .as_deref()
        .filter(|value| !value.is_empty())
        .and_then(|date| {
            git_output(
                repo_root,
                &[
                    "log",
                    "--all",
                    "--format=%H",
                    &format!("--since={date}"),
                    "--",
                    target.as_str(),
                ],
            )
        })
        .map(|output| nonempty_line_count(&output))
}

fn relevant_worktree_dirty(repo_root: &Utf8Path, target: &Utf8Path, edge_file: &str) -> bool {
    let mut args = vec!["status", "--porcelain", "--", target.as_str()];
    let edge_path = Utf8Path::new(edge_file);
    let edge_string;
    if !edge_file.is_empty() {
        edge_string = if edge_path.is_absolute() {
            edge_path
                .strip_prefix(repo_root)
                .map_or_else(|_| edge_file.to_string(), ToString::to_string)
        } else {
            edge_file.to_string()
        };
        args.push(edge_string.as_str());
    }
    git_output(repo_root, &args).is_some_and(|output| !output.trim().is_empty())
}

fn find_move_candidates(repo_root: &Utf8Path, target: &Utf8Path) -> Vec<String> {
    let Some(deleting_commits) = git_output(
        repo_root,
        &[
            "log",
            "--all",
            "--format=%H",
            "--diff-filter=D",
            "--",
            target.as_str(),
        ],
    ) else {
        return Vec::new();
    };
    let mut candidates = BTreeSet::new();
    for commit in deleting_commits
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some(show) = git_output(
            repo_root,
            &[
                "show",
                "--name-status",
                "--find-renames",
                "--find-copies",
                "--format=",
                commit,
            ],
        ) else {
            continue;
        };
        let mut deleted_in_commit = false;
        let mut added_paths = Vec::new();
        for line in show.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let fields = line.split('\t').collect::<Vec<_>>();
            let Some(code) = fields.first().copied() else {
                continue;
            };
            if code == "D" && fields.get(1).is_some_and(|path| *path == target.as_str()) {
                deleted_in_commit = true;
            } else if (code.starts_with('R') || code.starts_with('C'))
                && fields.get(1).is_some_and(|path| *path == target.as_str())
                && let Some(candidate) = fields.get(2)
            {
                candidates.insert((*candidate).to_string());
            } else if code == "A"
                && let Some(path) = fields.get(1)
            {
                added_paths.push((*path).to_string());
            }
        }
        if deleted_in_commit && candidates.is_empty() {
            candidates.extend(
                added_paths
                    .into_iter()
                    .filter(|path| looks_like_split_candidate(target.as_str(), path)),
            );
        }
    }
    candidates.into_iter().take(64).collect()
}

fn looks_like_split_candidate(old_path: &str, new_path: &str) -> bool {
    let old = Utf8Path::new(old_path);
    let new = Utf8Path::new(new_path);
    let Some(stem) = old.file_stem() else {
        return false;
    };
    let parent = old.parent().map_or("", Utf8Path::as_str);
    new_path.starts_with(&format!("{parent}/{stem}/"))
        || new_path.starts_with(&format!("{parent}/{stem}_"))
        || new
            .file_stem()
            .is_some_and(|new_stem| new_stem.starts_with(&format!("{stem}_")))
}

fn git_output(repo_root: &Utf8Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root.as_std_path())
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn nonempty_line_count(output: &str) -> u32 {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[must_use]
pub fn probe_code_target(corpus_root: &Utf8Path, target_path: &str) -> CodeTargetProbe {
    let mut cache = CodeTargetProbeCache::new();
    probe_code_target_with_cache(corpus_root, target_path, &mut cache)
}

fn probe_code_target_with_cache(
    corpus_root: &Utf8Path,
    target_path: &str,
    cache: &mut CodeTargetProbeCache,
) -> CodeTargetProbe {
    let Some(normalized) =
        normalize_relative_path(target_path, RelativePathPolicy::STRICT_NON_EMPTY)
    else {
        return CodeTargetProbe::unknown();
    };

    if let Some(project_root) = enclosing_project_root(corpus_root) {
        if let Some(found) = existing_target(&project_root, &normalized) {
            return CodeTargetProbe {
                exists: TargetExistence::True,
                history_status: cache.target_history_status(&project_root, &normalized),
                probe_base: Some(project_root),
                resolved_path: Some(found),
            };
        }
        if project_root != corpus_root
            && let Some(found) = existing_target(corpus_root, &normalized)
        {
            return CodeTargetProbe {
                exists: TargetExistence::True,
                history_status: cache.target_history_status(corpus_root, &normalized),
                probe_base: Some(corpus_root.to_path_buf()),
                resolved_path: Some(found),
            };
        }
        return missing_target_probe(cache, project_root, &normalized);
    }

    if let Some(found) = existing_target(corpus_root, &normalized) {
        return CodeTargetProbe {
            exists: TargetExistence::True,
            history_status: cache.target_history_status(corpus_root, &normalized),
            probe_base: Some(corpus_root.to_path_buf()),
            resolved_path: Some(found),
        };
    }
    missing_target_probe(cache, corpus_root.to_path_buf(), &normalized)
}

fn probe_code_target_without_history(corpus_root: &Utf8Path, target_path: &str) -> CodeTargetProbe {
    let Some(normalized) =
        normalize_relative_path(target_path, RelativePathPolicy::STRICT_NON_EMPTY)
    else {
        return CodeTargetProbe::unknown();
    };

    if let Some(project_root) = enclosing_project_root(corpus_root) {
        if let Some(found) = existing_target(&project_root, &normalized) {
            return existing_target_probe(project_root, found);
        }
        if project_root != corpus_root
            && let Some(found) = existing_target(corpus_root, &normalized)
        {
            return existing_target_probe(corpus_root.to_path_buf(), found);
        }
        return unknown_missing_target_probe(project_root, &normalized);
    }

    if let Some(found) = existing_target(corpus_root, &normalized) {
        return existing_target_probe(corpus_root.to_path_buf(), found);
    }
    unknown_missing_target_probe(corpus_root.to_path_buf(), &normalized)
}

fn existing_target_probe(base: Utf8PathBuf, found: Utf8PathBuf) -> CodeTargetProbe {
    CodeTargetProbe {
        exists: TargetExistence::True,
        history_status: TargetHistoryStatus::Unavailable,
        probe_base: Some(base),
        resolved_path: Some(found),
    }
}

fn unknown_missing_target_probe(base: Utf8PathBuf, _normalized: &Utf8Path) -> CodeTargetProbe {
    CodeTargetProbe {
        exists: TargetExistence::Unknown,
        history_status: TargetHistoryStatus::Unavailable,
        probe_base: Some(base),
        resolved_path: None,
    }
}

fn missing_target_probe(
    cache: &mut CodeTargetProbeCache,
    base: Utf8PathBuf,
    normalized: &Utf8Path,
) -> CodeTargetProbe {
    match cache.history_contains_target(&base, normalized) {
        Some(true) => CodeTargetProbe {
            exists: TargetExistence::False,
            history_status: TargetHistoryStatus::Present,
            probe_base: Some(base),
            resolved_path: None,
        },
        Some(false) => CodeTargetProbe {
            exists: TargetExistence::Unknown,
            history_status: TargetHistoryStatus::Absent,
            probe_base: Some(base),
            resolved_path: None,
        },
        None => CodeTargetProbe {
            exists: TargetExistence::Unknown,
            history_status: TargetHistoryStatus::Unavailable,
            probe_base: Some(base),
            resolved_path: None,
        },
    }
}

fn existing_target(base: &Utf8Path, target: &Utf8Path) -> Option<Utf8PathBuf> {
    let candidate = base.join(target);
    candidate.exists().then_some(candidate)
}

fn read_head_history_paths(base: &Utf8Path) -> Option<BTreeSet<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(base.as_std_path())
        .args(["log", "--name-only", "--format="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn enclosing_project_root(corpus_root: &Utf8Path) -> Option<Utf8PathBuf> {
    let mut current = Some(corpus_root);
    while let Some(path) = current {
        if is_project_root(path) {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn is_project_root(path: &Utf8Path) -> bool {
    path.join(".git").exists()
        || cargo_workspace_marker(path)
        || path.join("mix.exs").exists()
        || path.join("package.json").exists()
        || path.join("pyproject.toml").exists()
        || path.join("go.mod").exists()
}

fn cargo_workspace_marker(path: &Utf8Path) -> bool {
    let manifest = path.join("Cargo.toml");
    std::fs::read_to_string(manifest)
        .is_ok_and(|contents| contents.lines().any(|line| line.trim() == "[workspace]"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;

    fn utf8(path: std::path::PathBuf) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(path).expect("utf8 temp path")
    }

    #[test]
    fn probe_prefers_enclosing_repo_for_nested_corpus() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        fs::create_dir_all(&repo).expect("create repo");
        run_git(&repo, &["init"]);
        fs::create_dir_all(repo.join("lib")).expect("create lib");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(repo.join("lib/live.rs"), "").expect("write code");

        let probe = probe_code_target(&corpus, "lib/live.rs");

        assert_eq!(probe.exists, TargetExistence::True);
        assert_eq!(probe.probe_base.as_deref(), Some(repo.as_path()));
        assert_eq!(
            probe.resolved_path.as_deref(),
            Some(repo.join("lib/live.rs").as_path())
        );
    }

    #[test]
    fn probe_reports_missing_against_confident_repo_base() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        fs::create_dir_all(repo.join(".git")).expect("create marker");
        fs::create_dir_all(&corpus).expect("create corpus");

        let probe = probe_code_target(&corpus, "lib/missing.rs");

        assert_eq!(probe.exists, TargetExistence::Unknown);
        assert_eq!(probe.history_status, TargetHistoryStatus::Unavailable);
        assert_eq!(probe.probe_base.as_deref(), Some(repo.as_path()));
        assert_eq!(probe.resolved_path, None);
    }

    #[test]
    fn probe_reports_drift_only_when_missing_target_has_head_history() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        fs::create_dir_all(&repo).expect("create repo");
        run_git(&repo, &["init"]);
        fs::create_dir_all(repo.join("lib")).expect("create lib");
        fs::create_dir_all(&corpus).expect("create corpus");
        fs::write(repo.join("lib/old.rs"), "pub fn old() {}\n").expect("write old");
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "add old"]);
        fs::remove_file(repo.join("lib/old.rs")).expect("remove old");

        let mut cache = CodeTargetProbeCache::new();
        let drift = cache.probe(&corpus, "lib/old.rs");
        let illustrative = cache.probe(&corpus, "lib/never.rs");

        assert_eq!(drift.exists, TargetExistence::False);
        assert_eq!(drift.history_status, TargetHistoryStatus::Present);
        assert_eq!(drift.probe_base.as_deref(), Some(repo.as_path()));
        assert_eq!(illustrative.exists, TargetExistence::Unknown);
        assert_eq!(illustrative.history_status, TargetHistoryStatus::Absent);
        assert_eq!(illustrative.probe_base.as_deref(), Some(repo.as_path()));
    }

    #[test]
    fn probe_returns_unknown_for_escaping_or_absolute_targets() {
        let dir = tempdir().expect("tempdir");
        let root = utf8(dir.path().join("corpus"));
        fs::create_dir_all(&root).expect("create corpus");

        assert_eq!(
            probe_code_target(&root, "../outside.rs").exists,
            TargetExistence::Unknown
        );
        assert_eq!(
            probe_code_target(&root, "/tmp/outside.rs").exists,
            TargetExistence::Unknown
        );
    }

    #[test]
    fn drift_evidence_classifies_split_paths_as_moved_ambiguous_and_caches() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::create_dir_all(&corpus).expect("create corpus");
        run_git(&repo, &["init"]);
        fs::write(repo.join("src/cli.rs"), "pub fn run() {}\n").expect("write cli");
        fs::write(
            corpus.join("spec.md"),
            "---\nstatus: active\n---\n# Spec\n\nSee `src/cli.rs`.\n",
        )
        .expect("write spec");
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "add cli and spec"]);
        let assertion_revision = git_stdout(&repo, &["rev-parse", "HEAD"]);

        fs::remove_file(repo.join("src/cli.rs")).expect("remove cli");
        fs::create_dir_all(repo.join("src/cli")).expect("create split dir");
        fs::write(repo.join("src/cli/main.rs"), "pub fn main() {}\n").expect("write main");
        fs::write(repo.join("src/cli/app.rs"), "pub fn app() {}\n").expect("write app");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-m", "split cli"]);

        let request = CodeDriftEvidenceRequest {
            ref_handle: "external:code:spec.md:6:src/cli.rs".to_string(),
            target_path: "src/cli.rs".to_string(),
            edge_file: ".design/spec.md".to_string(),
            assertion_date: None,
            assertion_revision: Some(assertion_revision),
        };
        let mut refresh = CodeDriftEvidenceCache::open(&corpus, CodeDriftEvidenceMode::Refresh);
        let evidence = refresh
            .evidence_for(&request)
            .expect("refresh computes evidence");

        assert_eq!(evidence.disposition, "referent-moved-ambiguous");
        assert_eq!(
            evidence.move_candidates,
            vec!["src/cli/app.rs".to_string(), "src/cli/main.rs".to_string()]
        );
        refresh.save().expect("save cache");

        let read_request = CodeDriftEvidenceRequest {
            assertion_revision: None,
            ..request
        };
        let mut read = CodeDriftEvidenceCache::open(&corpus, CodeDriftEvidenceMode::ReadCache);
        let cached = read
            .evidence_for(&read_request)
            .expect("read cache evidence without re-blaming assertions");
        assert_eq!(cached, evidence);
    }

    fn run_git(root: &Utf8Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_CONFIG_GLOBAL", root.join(".anneal-test-gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-c")
            .arg("user.name=Anneal Test")
            .arg("-c")
            .arg("user.email=anneal@example.test")
            .arg("-C")
            .arg(root.as_std_path())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_stdout(root: &Utf8Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_CONFIG_GLOBAL", root.join(".anneal-test-gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-c")
            .arg("user.name=Anneal Test")
            .arg("-c")
            .arg("user.email=anneal@example.test")
            .arg("-C")
            .arg(root.as_std_path())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf8 git output")
            .trim()
            .to_string()
    }
}
