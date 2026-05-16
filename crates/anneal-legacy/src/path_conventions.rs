use camino::Utf8Path;

pub(crate) const TERMINAL_DIRS: &[&str] = &["archive", "history", "prior"];

pub(crate) fn has_terminal_directory(path: &Utf8Path) -> bool {
    path.components()
        .any(|component| is_terminal_directory_name(component.as_str()))
}

pub(crate) fn has_terminal_directory_str(path: &str) -> bool {
    path.split('/').any(is_terminal_directory_name)
}

pub(crate) fn is_terminal_directory_name(name: &str) -> bool {
    TERMINAL_DIRS.contains(&name)
}
