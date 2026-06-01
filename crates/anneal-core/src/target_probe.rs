use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTargetProbe {
    pub exists: TargetExistence,
    pub probe_base: Option<Utf8PathBuf>,
    pub resolved_path: Option<Utf8PathBuf>,
}

impl CodeTargetProbe {
    fn unknown() -> Self {
        Self {
            exists: TargetExistence::Unknown,
            probe_base: None,
            resolved_path: None,
        }
    }
}

#[must_use]
pub fn probe_code_target(corpus_root: &Utf8Path, target_path: &str) -> CodeTargetProbe {
    let Some(normalized) = normalize_relative_target(target_path) else {
        return CodeTargetProbe::unknown();
    };

    if let Some(project_root) = enclosing_project_root(corpus_root) {
        if let Some(found) = existing_target(&project_root, &normalized) {
            return CodeTargetProbe {
                exists: TargetExistence::True,
                probe_base: Some(project_root),
                resolved_path: Some(found),
            };
        }
        if project_root != corpus_root
            && let Some(found) = existing_target(corpus_root, &normalized)
        {
            return CodeTargetProbe {
                exists: TargetExistence::True,
                probe_base: Some(corpus_root.to_path_buf()),
                resolved_path: Some(found),
            };
        }
        return CodeTargetProbe {
            exists: TargetExistence::False,
            probe_base: Some(project_root),
            resolved_path: None,
        };
    }

    if let Some(found) = existing_target(corpus_root, &normalized) {
        return CodeTargetProbe {
            exists: TargetExistence::True,
            probe_base: Some(corpus_root.to_path_buf()),
            resolved_path: Some(found),
        };
    }
    CodeTargetProbe {
        exists: TargetExistence::False,
        probe_base: Some(corpus_root.to_path_buf()),
        resolved_path: None,
    }
}

fn normalize_relative_target(target_path: &str) -> Option<Utf8PathBuf> {
    let path = Utf8Path::new(target_path);
    if path.is_absolute() {
        return None;
    }
    let mut normalized = Utf8PathBuf::new();
    for component in path.components() {
        match component {
            Utf8Component::Normal(part) => normalized.push(part),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir | Utf8Component::RootDir | Utf8Component::Prefix(_) => {
                return None;
            }
        }
    }
    (!normalized.as_str().is_empty()).then_some(normalized)
}

fn existing_target(base: &Utf8Path, target: &Utf8Path) -> Option<Utf8PathBuf> {
    let candidate = base.join(target);
    candidate.exists().then_some(candidate)
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
        fs::create_dir_all(repo.join(".git")).expect("create marker");
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

        assert_eq!(probe.exists, TargetExistence::False);
        assert_eq!(probe.probe_base.as_deref(), Some(repo.as_path()));
        assert_eq!(probe.resolved_path, None);
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
}
