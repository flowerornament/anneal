//! Read-only, generation-pinned jj history (CR-D113).
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::OnceLock;

use camino::{Utf8Path, Utf8PathBuf};

use super::RepositoryOperation;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct JjEvidence {
    pub(super) root: Utf8PathBuf,
    git_dir: Option<Utf8PathBuf>,
    pub(super) pin: Option<String>,
    pub(super) times: BTreeMap<String, String>,
    pub(super) history: BTreeSet<String>,
    pub(super) tracked: BTreeSet<String>,
    pub(super) tags: Vec<String>,
    failures: [OnceLock<&'static str>; 5],
}

fn jj_command(root: &Utf8Path) -> Command {
    let mut command = Command::new("jj");
    command.args([
        "--ignore-working-copy",
        "--color=never",
        "--no-pager",
        "--repository",
        root.as_str(),
    ]);
    command
}

fn stdout(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn read_pin(root: &Utf8Path) -> Result<String, &'static str> {
    let current = stdout(jj_command(root).args([
        "log",
        "-r",
        "@",
        "--no-graph",
        "-T",
        "commit_id ++ ' ' ++ conflict",
    ]))
    .ok_or("jj-pin-unreadable")?;
    let mut fields = current.split_whitespace();
    let pin = fields.next().ok_or("jj-pin-unreadable")?;
    if fields.next() != Some("false") {
        return Err("jj-conflicted-working-copy");
    }
    if fields.next().is_some()
        || !matches!(pin.len(), 40 | 64)
        || !pin.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("jj-pin-unreadable");
    }
    // Compare the workspace's recorded operation with today's view of its @.
    // Debug metadata is read-only; unsupported layouts fail closed.
    let working = stdout(jj_command(root).args(["debug", "working-copy"]))
        .ok_or("jj-workspace-state-unreadable")?;
    let operation = working_copy_operation(&working)?;
    let recorded = stdout(jj_command(root).args([
        "--at-operation",
        operation,
        "log",
        "-r",
        "@",
        "--no-graph",
        "-T",
        "commit_id",
    ]))
    .ok_or("jj-workspace-state-unreadable")?;
    if recorded.trim() != pin {
        return Err("jj-stale-working-copy");
    }
    Ok(pin.to_string())
}

// jj debug output is unstable. Accept only the operation field exercised by
// our captured layouts and real fixtures; do not guess after a format change.
fn working_copy_operation(working: &str) -> Result<&str, &'static str> {
    let incompatible = "jj-workspace-state-incompatible";
    let mut fields = working
        .lines()
        .filter_map(|line| line.strip_prefix("Current operation:"));
    let field = fields.next().ok_or(incompatible)?;
    if fields.next().is_some() {
        return Err(incompatible);
    }
    let operation = field
        .strip_prefix(" OperationId(\"")
        .and_then(|value| value.strip_suffix("\")"))
        .ok_or(incompatible)?;
    if operation.len() != 128 || !operation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(incompatible);
    }
    Ok(operation)
}

fn resolve_pointer(path: &Utf8Path) -> Result<Utf8PathBuf, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("cannot read jj pointer {path}: {err}"))?;
    if raw.trim().is_empty() {
        return Err(format!("empty jj pointer {path}"));
    }
    path.parent()
        .ok_or_else(|| format!("jj pointer lacks parent: {path}"))?
        .join(raw.trim())
        .canonicalize_utf8()
        .map_err(|err| format!("cannot resolve jj pointer {path}: {err}"))
}

pub(super) fn resolve_backing(root: &Utf8Path) -> Result<Utf8PathBuf, String> {
    let marker = root.join(".jj/repo");
    let repo = if marker.is_dir() {
        marker.canonicalize_utf8().map_err(|err| err.to_string())?
    } else {
        resolve_pointer(&marker)?
    };
    let git_dir = resolve_pointer(&repo.join("store/git_target"))?;
    let mut command = backing_command(root, &git_dir);
    stdout(command.args(["rev-parse", "--git-dir"]))
        .ok_or_else(|| format!("Git did not recognize jj backing {git_dir}"))?;
    Ok(git_dir)
}

fn backing_command(root: &Utf8Path, git_dir: &Utf8Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .args(["--git-dir", git_dir.as_str(), "--work-tree", root.as_str()]);
    command
}

impl JjEvidence {
    pub(super) fn discover(root: &Utf8Path) -> Self {
        let root = root
            .canonicalize_utf8()
            .unwrap_or_else(|_| root.to_path_buf());
        let mut evidence = Self {
            root,
            git_dir: None,
            pin: None,
            times: BTreeMap::new(),
            history: BTreeSet::new(),
            tracked: BTreeSet::new(),
            tags: Vec::new(),
            failures: std::array::from_fn(|_| OnceLock::new()),
        };
        evidence.fail(
            RepositoryOperation::AssertionBlame,
            "jj-assertion-blame-not-defined",
        );
        let Ok(git_dir) = resolve_backing(&evidence.root) else {
            evidence.fail_dependent("jj-backing-unavailable");
            return evidence;
        };
        evidence.git_dir = Some(git_dir);
        let pin = match read_pin(&evidence.root) {
            Ok(pin) => pin,
            Err(reason) => {
                evidence.fail_dependent(reason);
                return evidence;
            }
        };
        let Some(mut command) = evidence.command() else {
            return evidence;
        };
        if stdout(command.args(["cat-file", "-t", &pin]))
            .as_deref()
            .map(str::trim)
            != Some("commit")
        {
            evidence.fail_dependent("jj-pin-not-in-git-store");
            return evidence;
        }
        evidence.pin = Some(pin.clone());
        // Independent probes: a successful empty stream earns capability.
        if let Some(log) = evidence.output(&[
            "log",
            "--format=%x00%aI",
            "--name-only",
            "-z",
            &pin,
            "--",
            ".",
        ]) {
            for record in log.split("\0\0") {
                let mut fields = record.trim_start_matches('\0').split('\0');
                let Some(instant) = fields.next().filter(|value| !value.is_empty()) else {
                    continue;
                };
                for (index, path) in fields.enumerate() {
                    let path = if index == 0 {
                        path.strip_prefix('\n').unwrap_or(path)
                    } else {
                        path
                    };
                    if !path.is_empty() {
                        evidence
                            .times
                            .entry(path.to_string())
                            .or_insert_with(|| instant.to_string());
                    }
                }
            }
        } else {
            evidence.fail(
                RepositoryOperation::ChangeHistory,
                "jj-change-history-probe-failed",
            );
        }
        if let Some(log) = evidence.output(&["log", "--format=", "--name-only", "-z", &pin]) {
            evidence.history = log
                .split('\0')
                .map(|s| s.trim_start_matches('\n'))
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        } else {
            evidence.fail(
                RepositoryOperation::TargetHistory,
                "jj-target-history-probe-failed",
            );
        }
        if let Some(tree) = evidence.output(&["ls-tree", "-r", "-z", "--name-only", &pin]) {
            evidence.tracked = tree
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        } else {
            evidence.fail(
                RepositoryOperation::IgnoreIndex,
                "jj-tracked-tree-probe-failed",
            );
        }
        if let Some(raw) = evidence.output(&["tag", "--points-at", &pin, "--sort=refname"]) {
            evidence.tags = super::parse_tags(&raw);
        } else {
            evidence.fail(
                RepositoryOperation::VersionTags,
                "jj-version-tags-probe-failed",
            );
        }
        evidence
    }

    pub(super) fn command(&self) -> Option<Command> {
        Some(backing_command(&self.root, self.git_dir.as_deref()?))
    }
    fn output(&self, args: &[&str]) -> Option<String> {
        stdout(self.command()?.args(args))
    }
    pub(super) fn available(&self, operation: RepositoryOperation) -> bool {
        self.pin.is_some() && self.failures[operation.index()].get().is_none()
    }
    pub(super) fn reason(&self, operation: RepositoryOperation) -> &'static str {
        self.failures[operation.index()]
            .get()
            .copied()
            .unwrap_or(match operation {
                RepositoryOperation::ChangeHistory => "jj-author-history",
                RepositoryOperation::TargetHistory => "jj-pinned-target-history",
                RepositoryOperation::IgnoreIndex => "jj-workspace-ignore-and-tree",
                RepositoryOperation::AssertionBlame => "jj-assertion-blame-not-defined",
                RepositoryOperation::VersionTags => "jj-pinned-version-tags",
            })
    }
    pub(super) fn fail(&self, operation: RepositoryOperation, reason: &'static str) {
        let _ = self.failures[operation.index()].set(reason);
    }
    fn fail_dependent(&self, reason: &'static str) {
        for operation in [
            RepositoryOperation::ChangeHistory,
            RepositoryOperation::TargetHistory,
            RepositoryOperation::IgnoreIndex,
            RepositoryOperation::VersionTags,
        ] {
            self.fail(operation, reason);
        }
    }
    pub(super) fn finish(&self) -> bool {
        let result = read_pin(&self.root);
        match result {
            Ok(pin) if self.pin.as_deref() == Some(&pin) => {
                self.available(RepositoryOperation::TargetHistory)
                    || self.available(RepositoryOperation::ChangeHistory)
                    || self.available(RepositoryOperation::IgnoreIndex)
                    || self.available(RepositoryOperation::VersionTags)
            }
            Ok(_) => {
                self.fail_dependent("jj-pin-moved-during-extraction");
                false
            }
            Err(reason) => {
                self.fail_dependent(reason);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::RepositoryContext;

    struct Desk {
        _temp: tempfile::TempDir,
        anchor: Utf8PathBuf,
        root: Utf8PathBuf,
        base: String,
    }

    fn run(root: &Utf8Path, program: &str, args: &[&str]) -> String {
        let mut command = Command::new(program);
        command
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR");
        if program == "git" {
            command
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .env("GIT_AUTHOR_DATE", "2001-01-01T00:00:00Z")
                .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z");
        } else {
            command.args([
                "--config",
                "user.name='Test'",
                "--config",
                "user.email='test@example.com'",
            ]);
        }
        let output = command.args(args).output().expect("fixture command");
        assert!(
            output.status.success(),
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf8 output")
            .trim()
            .to_string()
    }

    fn desk() -> Option<Desk> {
        let available = Command::new("jj")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !available {
            assert!(
                std::env::var_os("ANNEAL_REQUIRE_JJ").is_none(),
                "real jj history fixture requires a working jj executable (ANNEAL_REQUIRE_JJ)"
            );
            eprintln!("real jj history fixture skipped: jj executable unavailable");
            return None;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let anchor = Utf8PathBuf::from_path_buf(temp.path().join("anchor")).expect("utf8");
        let root = Utf8PathBuf::from_path_buf(temp.path().join("desk")).expect("utf8");
        std::fs::create_dir_all(anchor.join(".design")).expect("mkdir");
        std::fs::write(anchor.join(".design/carried.md"), "# Carried\n").expect("write");
        std::fs::write(anchor.join(".design/ timestamp.md"), "# Whitespace\n").expect("write");
        run(&anchor, "git", &["init", "--quiet"]);
        run(&anchor, "git", &["add", "."]);
        run(&anchor, "git", &["commit", "--quiet", "-m", "baseline"]);
        let base = run(&anchor, "git", &["rev-parse", "HEAD"]);
        run(&anchor, "jj", &["git", "init", "--colocate"]);
        run(&anchor, "jj", &["workspace", "add", root.as_str()]);
        Some(Desk {
            _temp: temp,
            anchor,
            root,
            base,
        })
    }

    #[test]
    fn working_copy_layouts_for_locked_and_desk_jj_are_compatible() {
        for layout in [
            include_str!("../../tests/fixtures/jj/working-copy-0.39.0.txt"),
            include_str!("../../tests/fixtures/jj/working-copy-0.45.1.txt"),
        ] {
            assert_eq!(
                working_copy_operation(layout)
                    .expect("supported layout")
                    .len(),
                128
            );
        }
    }

    #[test]
    fn incompatible_working_copy_layouts_fail_closed() {
        let valid = include_str!("../../tests/fixtures/jj/working-copy-0.39.0.txt");
        let line = valid
            .lines()
            .find(|line| line.starts_with("Current operation: "))
            .expect("operation line");
        for layout in [
            String::new(),
            valid.replace("Current operation:", "Recorded operation:"),
            format!("{valid}\n{line}\n"),
            format!("{valid}\nCurrent operation:garbled\n"),
            valid.replace("OperationId(\"", "OperationId("),
            "Current operation: OperationId(\"\")".to_string(),
            "Current operation: OperationId(\"abc123\")".to_string(),
            format!("Current operation: OperationId(\"{}\")", "z".repeat(128)),
        ] {
            assert_eq!(
                working_copy_operation(&layout),
                Err("jj-workspace-state-incompatible"),
                "{layout}"
            );
        }
    }

    #[test]
    fn required_jj_cannot_skip_a_missing_executable() {
        let empty_path = tempfile::tempdir().expect("empty executable search path");
        let fixture = format!(
            "{}::pinned_author_history_survives_real_rebase_and_does_not_read_anchor",
            module_path!()
                .split_once("::")
                .expect("crate-qualified module path")
                .1
        );
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .env("PATH", empty_path.path())
            .env("ANNEAL_REQUIRE_JJ", "1")
            .args(["--exact", fixture.as_str(), "--nocapture"])
            .output()
            .expect("child fixture runs");
        assert!(!output.status.success(), "missing required jj must fail");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            diagnostic.contains("real jj history fixture requires a working jj executable"),
            "{diagnostic}"
        );
    }

    #[test]
    fn pinned_author_history_survives_real_rebase_and_does_not_read_anchor() {
        let Some(desk) = desk() else {
            return;
        };
        std::fs::write(desk.root.join(".design/edited.md"), "# Old change\n").expect("write");
        run(
            &desk.root,
            "jj",
            &[
                "metaedit",
                "--author-timestamp",
                "2002-01-01T00:00:00Z",
                "-m",
                "old edit",
            ],
        );
        let before = RepositoryContext::discover(&desk.root.join(".design"));
        assert_eq!(
            before
                .jj_file_time(&desk.root.join(".design/edited.md"))
                .as_deref(),
            Some("2002-01-01T00:00:00Z")
        );
        assert_eq!(
            before
                .jj_file_time(&desk.root.join(".design/ timestamp.md"))
                .as_deref(),
            Some("2001-01-01T00:00:00Z")
        );
        run(&desk.anchor, "jj", &["new", &desk.base]);
        std::fs::write(desk.anchor.join(".design/upstream.md"), "# Upstream\n").expect("write");
        run(&desk.anchor, "jj", &["describe", "-m", "upstream"]);
        let upstream = run(
            &desk.anchor,
            "jj",
            &["log", "--no-graph", "-r", "@", "-T", "commit_id"],
        );
        run(&desk.root, "jj", &["rebase", "-r", "@", "-o", &upstream]);
        let after = RepositoryContext::discover(&desk.root.join(".design"));
        assert_ne!(before.jj_pin(), after.jj_pin());
        assert_eq!(
            before.jj_file_time(&desk.root.join(".design/edited.md")),
            after.jj_file_time(&desk.root.join(".design/edited.md"))
        );
        assert_eq!(
            after
                .jj_file_time(&desk.root.join(".design/carried.md"))
                .as_deref(),
            Some("2001-01-01T00:00:00Z")
        );
        assert!(after.operation_available(RepositoryOperation::TargetHistory));
        assert_eq!(
            after.jj_target_history(&desk.root, Utf8Path::new(".design/edited.md")),
            Some(true)
        );
        assert_eq!(
            after.jj_target_history(&desk.root.join(".design"), Utf8Path::new("edited.md")),
            Some(true),
            "nested and root-prefixed paths name the same recorded target"
        );
        assert_eq!(
            after.jj_target_history(&desk.root, Utf8Path::new("edited.md")),
            Some(false),
            "the selected base remains part of history membership"
        );
        assert!(!desk.anchor.join(".design/edited.md").exists());
        assert!(!after.operation_available(RepositoryOperation::AssertionBlame));
        assert!(after.finish_jj_generation());
    }

    #[test]
    fn pin_movement_invalidates_clones_without_snapshotting_pending_edits() {
        let Some(desk) = desk() else {
            return;
        };
        let context = RepositoryContext::discover(&desk.root.join(".design"));
        let clone = context.clone();
        let pin = context.jj_pin().expect("pin").to_string();
        std::fs::write(desk.root.join(".design/unsnapshotted.md"), "# Pending\n").expect("write");
        assert!(context.finish_jj_generation());
        assert_eq!(
            run(
                &desk.root,
                "jj",
                &[
                    "--ignore-working-copy",
                    "log",
                    "--no-graph",
                    "-r",
                    "@",
                    "-T",
                    "commit_id"
                ]
            ),
            pin
        );
        assert!(
            context
                .jj_file_time(&desk.root.join(".design/unsnapshotted.md"))
                .is_none()
        );
        run(&desk.root, "jj", &["new"]);
        assert!(!context.finish_jj_generation());
        for operation in [
            RepositoryOperation::ChangeHistory,
            RepositoryOperation::TargetHistory,
            RepositoryOperation::IgnoreIndex,
            RepositoryOperation::VersionTags,
        ] {
            assert!(!clone.operation_available(operation));
        }
        assert!(
            clone
                .jj_file_time(&desk.root.join(".design/carried.md"))
                .is_none()
        );
    }

    #[test]
    fn stale_workspace_and_one_operation_failure_are_disclosed_independently() {
        let Some(desk) = desk() else {
            return;
        };
        let context = RepositoryContext::discover(&desk.root.join(".design"));
        context.fail_jj_operation(RepositoryOperation::IgnoreIndex, "fixture-ignore-failure");
        assert!(!context.operation_available(RepositoryOperation::IgnoreIndex));
        assert!(context.operation_available(RepositoryOperation::ChangeHistory));
        assert!(context.operation_available(RepositoryOperation::TargetHistory));
        let pin = context.jj_pin().expect("pin");
        run(
            &desk.anchor,
            "jj",
            &["metaedit", pin, "-m", "rewrite other desk"],
        );
        let stale = RepositoryContext::discover(&desk.root.join(".design"));
        assert!(!stale.operation_available(RepositoryOperation::ChangeHistory));
        assert!(
            stale
                .capability_rows()
                .any(|(_, _, _, reason)| reason == "jj-stale-working-copy")
        );
    }
    #[test]
    fn conflicted_pin_is_unavailable_without_materializing_or_resolving_it() {
        let Some(desk) = desk() else {
            return;
        };
        std::fs::write(desk.root.join(".design/carried.md"), "# Desk edit\n").expect("write");
        run(&desk.root, "jj", &["describe", "-m", "desk"]);
        run(&desk.anchor, "jj", &["new", &desk.base]);
        std::fs::write(desk.anchor.join(".design/carried.md"), "# Anchor edit\n").expect("write");
        run(&desk.anchor, "jj", &["describe", "-m", "anchor"]);
        let upstream = run(
            &desk.anchor,
            "jj",
            &["log", "--no-graph", "-r", "@", "-T", "commit_id"],
        );
        run(&desk.root, "jj", &["rebase", "-r", "@", "-o", &upstream]);
        let bytes = std::fs::read(desk.root.join(".design/carried.md")).expect("conflict bytes");
        let context = RepositoryContext::discover(&desk.root.join(".design"));
        assert!(!context.operation_available(RepositoryOperation::TargetHistory));
        assert!(
            context
                .capability_rows()
                .any(|(_, _, _, reason)| reason == "jj-conflicted-working-copy")
        );
        assert_eq!(
            std::fs::read(desk.root.join(".design/carried.md")).expect("unchanged conflict"),
            bytes
        );
    }
}
