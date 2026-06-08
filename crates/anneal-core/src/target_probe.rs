//! Code-target existence and history probes for source references.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

use crate::path_policy::{RelativePathPolicy, normalize_relative_path};

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
        run_git(&repo, &["config", "user.name", "Anneal Test"]);
        run_git(&repo, &["config", "user.email", "anneal@example.test"]);
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

    fn run_git(root: &Utf8Path, args: &[&str]) {
        let output = std::process::Command::new("git")
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
}
