//! Runtime repository provider and operation availability.

use std::process::Command;
use std::sync::Arc;

mod jj;

use camino::{Utf8Path, Utf8PathBuf};

/// Repository operation whose availability depends on the concrete workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryOperation {
    ChangeHistory,
    AssertionBlame,
    TargetHistory,
    IgnoreIndex,
    VersionTags,
}

impl RepositoryOperation {
    pub(crate) const ALL: [Self; 5] = [
        Self::ChangeHistory,
        Self::AssertionBlame,
        Self::TargetHistory,
        Self::IgnoreIndex,
        Self::VersionTags,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ChangeHistory => "change_history",
            Self::AssertionBlame => "assertion_blame",
            Self::TargetHistory => "target_history",
            Self::IgnoreIndex => "ignore_index",
            Self::VersionTags => "version_tags",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::ChangeHistory => 0,
            Self::AssertionBlame => 1,
            Self::TargetHistory => 2,
            Self::IgnoreIndex => 3,
            Self::VersionTags => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepositoryProvider {
    Git,
    Jj,
    None,
}

impl RepositoryProvider {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Jj => "jj",
            Self::None => "none",
        }
    }
}

/// Whether one repository operation has earned a runtime implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepositoryAvailability {
    Available,
    Unavailable,
}

impl RepositoryAvailability {
    pub(crate) const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Nearest VCS workspace and the operations available from it.
///
/// Fields stay private so callers cannot pair a jj boundary with an ancestor
/// Git root or manufacture capabilities independently of discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryContext {
    discovery_root: Utf8PathBuf,
    direct_git_root: Option<Utf8PathBuf>,
    provider: RepositoryProvider,
    availability: [RepositoryAvailability; 5],
    reasons: [&'static str; 5],
    jj: Option<Arc<jj::JjEvidence>>,
    tags: Option<Vec<String>>,
}

impl RepositoryContext {
    /// Discover the nearest VCS workspace without crossing a jj-only boundary.
    #[must_use]
    pub fn discover(root: &Utf8Path) -> Self {
        let discovery_root = root
            .canonicalize_utf8()
            .unwrap_or_else(|_| root.to_path_buf());
        for boundary in root.ancestors() {
            // A colocated jj main workspace is also a real Git worktree. Git
            // wins only at the same boundary, never beyond a nearer .jj.
            if boundary.join(".git").exists() {
                let direct_git_root = validated_git_root(root, boundary);
                let mut availability = if direct_git_root.is_some() {
                    [RepositoryAvailability::Available; 5]
                } else {
                    [RepositoryAvailability::Unavailable; 5]
                };
                let tags = direct_git_root.as_deref().and_then(|root| {
                    command_stdout(git_command(root).args([
                        "tag",
                        "--points-at",
                        "HEAD",
                        "--sort=refname",
                    ]))
                    .map(|raw| parse_tags(&raw))
                });
                availability[RepositoryOperation::VersionTags.index()] = if tags.is_some() {
                    RepositoryAvailability::Available
                } else {
                    RepositoryAvailability::Unavailable
                };
                let mut reasons = [if availability[0].is_available() {
                    "direct-git-worktree"
                } else {
                    "git-worktree-unavailable"
                }; 5];
                reasons[RepositoryOperation::VersionTags.index()] = if tags.is_some() {
                    "git-head-version-tags"
                } else if direct_git_root.is_some() {
                    "git-version-tags-probe-failed"
                } else {
                    "git-worktree-unavailable"
                };
                return Self {
                    discovery_root,
                    jj: None,
                    tags,
                    direct_git_root,
                    provider: RepositoryProvider::Git,
                    availability,
                    reasons,
                };
            }
            if boundary.join(".jj").exists() {
                return Self {
                    discovery_root,
                    jj: Some(Arc::new(jj::JjEvidence::discover(boundary))),
                    tags: None,
                    direct_git_root: None,
                    provider: RepositoryProvider::Jj,
                    availability: [RepositoryAvailability::Unavailable; 5],
                    reasons: [
                        "jj-change-history-not-implemented",
                        "jj-assertion-blame-not-implemented",
                        "jj-target-history-not-implemented",
                        "jj-workspace-index-unavailable",
                        "jj-version-tags-unavailable",
                    ],
                };
            }
        }
        Self {
            discovery_root,
            jj: None,
            direct_git_root: None,
            provider: RepositoryProvider::None,
            availability: [RepositoryAvailability::Unavailable; 5],
            reasons: ["no-vcs-workspace"; 5],
            tags: None,
        }
    }

    pub(crate) fn is_available(&self, operation: RepositoryOperation) -> bool {
        self.jj.as_ref().map_or_else(
            || self.availability[operation.index()].is_available(),
            |jj| jj.available(operation),
        )
    }

    pub(crate) fn capability_rows(
        &self,
    ) -> impl Iterator<Item = (&'static str, &'static str, &'static str, &'static str)> + '_ {
        RepositoryOperation::ALL.into_iter().map(|operation| {
            (
                operation.as_str(),
                if self.is_available(operation) {
                    "available"
                } else {
                    "unavailable"
                },
                self.provider.as_str(),
                self.jj
                    .as_ref()
                    .map_or(self.reasons[operation.index()], |jj| jj.reason(operation)),
            )
        })
    }

    /// Whether the nearest VCS boundary is a jj-only added workspace.
    pub const fn is_jj_workspace(&self) -> bool {
        matches!(self.provider, RepositoryProvider::Jj)
    }

    /// Whether one operation is available in this concrete workspace.
    pub fn operation_available(&self, operation: RepositoryOperation) -> bool {
        self.is_available(operation)
    }

    /// Reason associated with this operation's current availability.
    pub fn operation_reason(&self, operation: RepositoryOperation) -> &'static str {
        self.jj
            .as_ref()
            .map_or(self.reasons[operation.index()], |jj| jj.reason(operation))
    }

    /// Tags at Git HEAD or the generation's exact jj pin; None means unavailable.
    pub fn version_tags(&self) -> Option<&[String]> {
        if !self.is_available(RepositoryOperation::VersionTags) {
            return None;
        }
        self.jj
            .as_ref()
            .map_or(self.tags.as_deref(), |jj| Some(jj.tags.as_slice()))
    }

    /// Repository-selected paths relative to a contained source subtree.
    ///
    /// Only a genuinely non-VCS root returns None (intentional filesystem discovery).
    /// A failed VCS operation returns a reason, never an empty or fallback population.
    pub fn tracked_files_under(
        &self,
        base: &Utf8Path,
    ) -> Result<Option<Vec<String>>, &'static str> {
        if self.provider == RepositoryProvider::None {
            return Ok(None);
        }
        if !self.is_available(RepositoryOperation::IgnoreIndex) {
            return Err(self.operation_reason(RepositoryOperation::IgnoreIndex));
        }
        let base = base
            .canonicalize_utf8()
            .map_err(|_| "source-root-unreadable")?;
        let (root, paths) = if let Some(jj) = &self.jj {
            (
                jj.root.as_path(),
                jj.tracked.iter().cloned().collect::<Vec<_>>(),
            )
        } else {
            let root = self
                .direct_git_root
                .as_deref()
                .ok_or("git-worktree-unavailable")?;
            let raw = command_stdout(git_command(root).args([
                "ls-files",
                "-z",
                "--cached",
                "--exclude-standard",
            ]))
            .ok_or("git-tracked-files-probe-failed")?;
            (
                root,
                raw.split('\0')
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect(),
            )
        };
        let prefix = base
            .strip_prefix(root)
            .map_err(|_| "source-root-outside-repository")?;
        let mut selected = paths
            .into_iter()
            .filter_map(|path| {
                Utf8Path::new(&path)
                    .strip_prefix(prefix)
                    .ok()
                    .filter(|p| !p.as_str().is_empty())
                    .map(|p| p.as_str().to_owned())
            })
            .collect::<Vec<_>>();
        selected.sort();
        selected.dedup();
        Ok(Some(selected))
    }

    /// Resolve jj backing for containment only; this earns no operation capability.
    pub fn jj_git_backing(project_root: &Utf8Path) -> Result<Utf8PathBuf, String> {
        jj::resolve_backing(project_root)
    }

    /// Read-only Git command bound to a jj workspace, never its anchor checkout.
    pub fn jj_git_command(&self) -> Option<Command> {
        self.jj.as_ref()?.command()
    }

    pub fn jj_pin(&self) -> Option<&str> {
        self.jj.as_ref()?.pin.as_deref()
    }

    pub fn jj_workspace_root(&self) -> Option<&Utf8Path> {
        self.jj.as_ref().map(|jj| jj.root.as_path())
    }

    /// Last selected file-change author timestamp, keyed by physical origin.
    pub fn jj_file_time(&self, origin: &Utf8Path) -> Option<String> {
        let jj = self.jj.as_ref()?;
        if !jj.available(RepositoryOperation::ChangeHistory) {
            return None;
        }
        let origin = origin
            .canonicalize_utf8()
            .unwrap_or_else(|_| origin.to_path_buf());
        let path = origin.strip_prefix(&jj.root).ok()?;
        jj.times.get(path.as_str()).cloned()
    }

    pub fn jj_tracked_paths(&self) -> Option<&std::collections::BTreeSet<String>> {
        let jj = self.jj.as_ref()?;
        jj.available(RepositoryOperation::IgnoreIndex)
            .then_some(&jj.tracked)
    }

    pub fn jj_target_history(&self, base: &Utf8Path, target: &Utf8Path) -> Option<bool> {
        let jj = self.jj.as_ref()?;
        if !jj.available(RepositoryOperation::TargetHistory) {
            return None;
        }
        let path = base.canonicalize_utf8().ok()?.join(target);
        let relative = path.strip_prefix(&jj.root).ok()?;
        Some(jj.history.contains(relative.as_str()))
    }

    /// Invalidate just the failed operation; clones share this generation's result.
    pub fn fail_jj_operation(&self, operation: RepositoryOperation, reason: &'static str) {
        if let Some(jj) = &self.jj {
            jj.fail(operation, reason);
        }
    }

    /// Re-read recorded @ without snapshotting. Movement invalidates dependent evidence.
    pub fn finish_jj_generation(&self) -> bool {
        self.jj.as_ref().is_none_or(|jj| jj.finish())
    }

    /// Whether this context was discovered for this extraction root.
    pub fn applies_to(&self, root: &Utf8Path) -> bool {
        root.canonicalize_utf8()
            .unwrap_or_else(|_| root.to_path_buf())
            == self.discovery_root
    }

    /// Return a Git root only when this specific operation is available.
    pub fn direct_git_root(&self, operation: RepositoryOperation) -> Option<&Utf8Path> {
        if self.is_available(operation) {
            self.direct_git_root.as_deref()
        } else {
            None
        }
    }
}

fn git_command(root: &Utf8Path) -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .arg("-C")
        .arg(root)
        .current_dir(root);
    command
}

fn command_stdout(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn parse_tags(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn validated_git_root(root: &Utf8Path, boundary: &Utf8Path) -> Option<Utf8PathBuf> {
    let output = git_command(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let reported = String::from_utf8(output.stdout).ok()?;
    let reported = Utf8Path::new(reported.trim()).canonicalize_utf8().ok()?;
    let boundary = boundary.canonicalize_utf8().ok()?;
    (reported == boundary).then_some(reported)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn utf8(path: std::path::PathBuf) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(path).expect("utf8 temp path")
    }

    fn init_git(root: &Utf8Path) {
        fs::create_dir_all(root).expect("create git root");
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.as_std_path())
            .status()
            .expect("run git init");
        assert!(status.success());
    }

    #[test]
    fn direct_git_worktree_earns_history_but_unborn_head_has_no_tags() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        init_git(&repo);
        fs::create_dir_all(&corpus).expect("create corpus");

        let context = RepositoryContext::discover(&corpus);

        assert_eq!(
            context.direct_git_root.as_deref(),
            Some(repo.canonicalize_utf8().expect("canonical repo").as_path())
        );
        assert!(
            RepositoryOperation::ALL[..4]
                .iter()
                .all(|&op| context.operation_available(op))
        );
        assert!(!context.operation_available(RepositoryOperation::VersionTags));
    }

    #[test]
    fn jj_boundary_blocks_an_unrelated_ancestor_git_repository() {
        let dir = tempdir().expect("tempdir");
        let ancestor = utf8(dir.path().join("ancestor"));
        let workspace = ancestor.join("desk");
        let corpus = workspace.join(".design");
        init_git(&ancestor);
        fs::create_dir_all(workspace.join(".jj")).expect("create jj marker");
        fs::create_dir_all(&corpus).expect("create corpus");

        let context = RepositoryContext::discover(&corpus);

        assert_eq!(context.direct_git_root, None);
        assert_eq!(
            context.availability,
            [RepositoryAvailability::Unavailable; 5]
        );
        assert_eq!(
            context.direct_git_root(RepositoryOperation::IgnoreIndex),
            None
        );
    }

    #[test]
    fn colocated_git_and_jj_uses_the_direct_git_worktree() {
        let dir = tempdir().expect("tempdir");
        let repo = utf8(dir.path().join("repo"));
        let corpus = repo.join(".design");
        init_git(&repo);
        fs::create_dir_all(repo.join(".jj")).expect("create jj marker");
        fs::create_dir_all(&corpus).expect("create corpus");

        let context = RepositoryContext::discover(&corpus);

        assert_eq!(
            context.direct_git_root.as_deref(),
            Some(repo.canonicalize_utf8().expect("canonical repo").as_path())
        );
        assert!(
            context
                .direct_git_root(RepositoryOperation::TargetHistory)
                .is_some()
        );
    }

    #[test]
    fn non_vcs_root_reports_each_operation_unavailable() {
        let dir = tempdir().expect("tempdir");
        let corpus = utf8(dir.path().join("corpus"));
        fs::create_dir_all(&corpus).expect("create corpus");

        let context = RepositoryContext::discover(&corpus);

        assert_eq!(context.direct_git_root, None);
        assert_eq!(
            context.availability,
            [RepositoryAvailability::Unavailable; 5]
        );
    }

    #[test]
    fn context_is_scoped_to_its_discovery_root() {
        let dir = tempdir().expect("tempdir");
        let first = utf8(dir.path().join("first"));
        let second = utf8(dir.path().join("second"));
        fs::create_dir_all(&first).expect("create first root");
        fs::create_dir_all(&second).expect("create second root");

        let context = RepositoryContext::discover(&first);

        assert!(context.applies_to(&first));
        assert!(!context.applies_to(&second));
    }
}
