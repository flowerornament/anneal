//! Runtime repository provider and operation availability.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

/// Repository operation whose availability depends on the concrete workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryOperation {
    ChangeHistory,
    AssertionBlame,
    TargetHistory,
    IgnoreIndex,
}

impl RepositoryOperation {
    const fn index(self) -> usize {
        match self {
            Self::ChangeHistory => 0,
            Self::AssertionBlame => 1,
            Self::TargetHistory => 2,
            Self::IgnoreIndex => 3,
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
    availability: [RepositoryAvailability; 4],
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
                let availability = if direct_git_root.is_some() {
                    [RepositoryAvailability::Available; 4]
                } else {
                    [RepositoryAvailability::Unavailable; 4]
                };
                return Self {
                    discovery_root,
                    direct_git_root,
                    availability,
                };
            }
            if boundary.join(".jj").exists() {
                return Self {
                    discovery_root,
                    direct_git_root: None,
                    availability: [RepositoryAvailability::Unavailable; 4],
                };
            }
        }
        Self {
            discovery_root,
            direct_git_root: None,
            availability: [RepositoryAvailability::Unavailable; 4],
        }
    }

    pub(crate) const fn is_available(&self, operation: RepositoryOperation) -> bool {
        self.availability[operation.index()].is_available()
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

fn validated_git_root(root: &Utf8Path, boundary: &Utf8Path) -> Option<Utf8PathBuf> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .arg("-C")
        .arg(root.as_std_path())
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
    fn direct_git_worktree_earns_every_operation() {
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
        assert_eq!(context.availability, [RepositoryAvailability::Available; 4]);
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
            [RepositoryAvailability::Unavailable; 4]
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
            [RepositoryAvailability::Unavailable; 4]
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
