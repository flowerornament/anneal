use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativePathPolicy {
    allow_empty: bool,
}

impl RelativePathPolicy {
    pub const STRICT_NON_EMPTY: Self = Self { allow_empty: false };
    pub const ALLOW_EMPTY: Self = Self { allow_empty: true };
}

#[must_use]
pub fn normalize_relative_path(value: &str, policy: RelativePathPolicy) -> Option<Utf8PathBuf> {
    let path = Utf8Path::new(value);
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
    if normalized.as_str().is_empty() && !policy.allow_empty {
        return None;
    }
    if normalized.as_str().is_empty() && value == "." {
        return Some(Utf8PathBuf::from("."));
    }
    Some(normalized)
}

#[must_use]
pub fn normalize_path_inside_root(root: &Utf8Path, path: &Utf8Path) -> Option<Utf8PathBuf> {
    normalize_path(path)
        .strip_prefix(root)
        .ok()
        .map(Utf8Path::to_path_buf)
}

fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                components.pop();
            }
            Utf8Component::RootDir => {
                components.clear();
                components.push("/");
            }
            Utf8Component::Prefix(prefix) => {
                components.clear();
                components.push(prefix.as_str());
            }
            Utf8Component::Normal(part) => components.push(part),
        }
    }
    if components.is_empty() {
        return Utf8PathBuf::from(".");
    }
    let mut result = Utf8PathBuf::new();
    for (idx, component) in components.iter().enumerate() {
        if idx == 0 && *component == "/" {
            result.push("/");
        } else {
            result.push(component);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_relative_path_rejects_empty_absolute_and_parent() {
        assert_eq!(
            normalize_relative_path("src/lib.rs", RelativePathPolicy::STRICT_NON_EMPTY),
            Some(Utf8PathBuf::from("src/lib.rs"))
        );
        assert_eq!(
            normalize_relative_path("./src/lib.rs", RelativePathPolicy::STRICT_NON_EMPTY),
            Some(Utf8PathBuf::from("src/lib.rs"))
        );
        assert_eq!(
            normalize_relative_path("", RelativePathPolicy::STRICT_NON_EMPTY),
            None
        );
        assert_eq!(
            normalize_relative_path("../lib.rs", RelativePathPolicy::STRICT_NON_EMPTY),
            None
        );
        assert_eq!(
            normalize_relative_path("/tmp/lib.rs", RelativePathPolicy::STRICT_NON_EMPTY),
            None
        );
    }

    #[test]
    fn scan_root_policy_preserves_current_directory() {
        assert_eq!(
            normalize_relative_path(".", RelativePathPolicy::ALLOW_EMPTY),
            Some(Utf8PathBuf::from("."))
        );
    }

    #[test]
    fn normalize_path_inside_root_rejects_escape() {
        let root = Utf8Path::new("/repo/.design");
        assert_eq!(
            normalize_path_inside_root(root, Utf8Path::new("/repo/.design/a/../b.md")),
            Some(Utf8PathBuf::from("b.md"))
        );
        assert_eq!(
            normalize_path_inside_root(root, Utf8Path::new("/repo/outside.md")),
            None
        );
    }
}
