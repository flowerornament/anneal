use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;
use tempfile::TempDir;

fn tempdir() -> TempDir {
    tempfile::Builder::new()
        .prefix("anneal-test")
        .tempdir()
        .expect("tempdir")
}

fn anneal_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(path) = std::env::var_os("CARGO_BIN_EXE_anneal") {
            return PathBuf::from(path);
        }

        let exe = std::env::current_exe().expect("test executable path");
        let target_dir = exe
            .ancestors()
            .nth(2)
            .expect("test executable lives under target/debug/deps");
        let binary = target_dir.join(format!("anneal{}", std::env::consts::EXE_SUFFIX));
        let status = Command::new("cargo")
            .args(["build", "-q", "-p", "anneal"])
            .status()
            .expect("build anneal binary");
        assert!(status.success(), "cargo build -p anneal failed");
        binary
    })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run(args: &[&str]) -> Output {
    run_in(repo_root(), args)
}

fn run_in(cwd: impl AsRef<Path>, args: &[&str]) -> Output {
    Command::new(anneal_bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run anneal")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn json_rows(output: &Output) -> Vec<Value> {
    assert_success(output);
    text(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("valid ndjson row"))
        .collect()
}

fn write_file(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

fn write_config(root: &Path, body: &str) {
    write_file(
        root,
        "anneal.dl",
        &format!(
            r#"source md {{
  file_extension(".md").
  scan_root(".").
}}

{body}
"#
        ),
    );
}

fn lifecycle_config(active: &[&str], terminal: &[&str], ordering: &[&str]) -> String {
    format!(
        r"config convergence {{
  ordering([{}]).
  active([{}]).
  terminal([{}]).
}}",
        quoted_list(ordering),
        quoted_list(active),
        quoted_list(terminal)
    )
}

fn quoted_list(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!(r#""{value}""#))
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_markdown(root: &Path, path: &str, status: &str, body: &str) {
    write_file(
        root,
        path,
        &format!(
            r"---
status: {status}
---
{body}
"
        ),
    );
}

#[test]
fn empty_corpus_status_explains_zero_rows() {
    let dir = tempdir();

    let output = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "status",
        "--format=text",
    ]);

    assert_success(&output);
    let stdout = text(&output.stdout);
    assert!(stdout.contains("(0 rows)"), "{stdout}");
    assert!(
        stdout.contains("no corpus facts found; root may be empty or unresolved"),
        "{stdout}"
    );
}

#[test]
fn no_marker_directory_with_markdown_signals_fallback_scan() {
    let dir = tempdir();
    write_markdown(
        dir.path(),
        "stray.md",
        "draft",
        "# Stray\n\naccidental corpus\n",
    );

    let output = run_in(dir.path(), &["status", "--format=text"]);

    assert_success(&output);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("no marked corpus root found above"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("scanning current directory"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn deep_subdir_invocation_walks_to_marked_root() {
    let dir = tempdir();
    let root = dir.path().join("corpus");
    write_config(
        &root,
        &lifecycle_config(&["draft"], &["done"], &["draft", "done"]),
    );
    write_markdown(&root, "a.md", "draft", "# A\n\nmarked root document\n");
    let nested = root.join("subdir/deep/nested");
    std::fs::create_dir_all(&nested).expect("create nested cwd");

    let output = run_in(&nested, &["-e", "? *handle{id: h}.", "--format=json"]);

    let rows = json_rows(&output);
    assert!(!rows.is_empty(), "eval should use the marked root");
    let stderr = text(&output.stderr);
    assert!(stderr.contains("resolved root:"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(root.to_str().expect("utf8 root")),
        "stderr:\n{stderr}"
    );
}

#[test]
fn unclassified_status_emits_lifecycle_config_gap() {
    let dir = tempdir();
    write_config(
        dir.path(),
        &format!(
            r#"config frontmatter {{
  field("depends-on", "DependsOn", "forward").
}}

{}"#,
            lifecycle_config(&["draft"], &["done"], &["draft", "done"])
        ),
    );
    write_markdown(
        dir.path(),
        "paused.md",
        "paused",
        "# Paused\n\nNot partitioned.\n",
    );

    let output = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "-e",
        r#"? diagnostic(code, severity, subject, file, line, evidence), code = "W005"."#,
        "--format=json",
    ]);

    let rows = json_rows(&output);
    assert!(
        rows.iter().any(|row| row["subject"] == "paused"
            && row["evidence"]
                .to_string()
                .contains("used_status_unpartitioned")),
        "{rows:#?}"
    );
}

#[test]
fn non_terminating_ordering_lattice_emits_lifecycle_config_gap() {
    let dir = tempdir();
    write_config(
        dir.path(),
        &lifecycle_config(&["draft", "review"], &["archived"], &["draft", "review"]),
    );
    write_markdown(
        dir.path(),
        "draft.md",
        "draft",
        "# Draft\n\nNo terminal tail.\n",
    );

    let output = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "-e",
        r#"? diagnostic(code, severity, subject, file, line, evidence), code = "W005"."#,
        "--format=json",
    ]);

    let rows = json_rows(&output);
    assert!(
        rows.iter().any(|row| row["subject"] == "review"
            && row["evidence"]
                .to_string()
                .contains("ordering_not_terminal")),
        "{rows:#?}"
    );
}

#[test]
fn no_snapshot_history_does_not_emit_false_pipeline_stall() {
    let dir = tempdir();
    write_config(
        dir.path(),
        &lifecycle_config(&["draft"], &["done"], &["draft", "done"]),
    );
    for index in 0..4 {
        write_file(
            dir.path(),
            &format!("draft-{index}.md"),
            &format!(
                "---\nstatus: draft\ndepends-on: missing-{index}.md\n---\n# Draft {index}\n\nNo snapshot baseline yet.\n"
            ),
        );
    }

    let diagnostics = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "-e",
        r#"? diagnostic(code, severity, subject, file, line, evidence), code = "S003"."#,
        "--format=text",
    ]);
    assert_success(&diagnostics);
    assert!(text(&diagnostics.stdout).contains("(0 rows)"));

    let status = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "status",
        "--format=text",
    ]);
    assert_success(&status);
    let stdout = text(&status.stdout);
    assert!(
        stdout.contains("flow signals empty until snapshot baseline accumulates"),
        "{stdout}"
    );
}

#[test]
fn tie_saturated_context_still_surfaces_canonical_section() {
    let dir = tempdir();
    write_config(
        dir.path(),
        &format!(
            "{}\n\nconfig handles {{ force([\"C\"]). }}",
            lifecycle_config(&["draft"], &["done"], &["draft", "done"])
        ),
    );
    write_markdown(
        dir.path(),
        "canonical.md",
        "draft",
        "# Error Model and Load Shedding\n\nGraceful overrun load shedding protects audio degradation during overload.\n",
    );
    write_markdown(
        dir.path(),
        "LABELS.md",
        "draft",
        "# Labels\n\n- C-12: graceful overrun\n- C-21: audio degradation\n- C-22: load shedding\n- C-23: overrun audio\n- C-24: degradation graceful\n- C-25: load audio\n",
    );

    let output = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "context",
        "graceful overrun load shedding audio degradation",
        "--hits=5",
        "--format=json",
    ]);

    let rows = json_rows(&output);
    let hits = rows
        .iter()
        .filter(|row| row["section"] == "hit")
        .collect::<Vec<_>>();
    assert!(
        hits.iter().any(|row| row["handle"] == "canonical.md"
            && row["heading_path"] == "Error Model and Load Shedding"),
        "{hits:#?}"
    );
    assert_eq!(hits.first().expect("first hit")["handle"], "canonical.md");
}

#[test]
fn context_default_is_compact_and_read_spans_expands_bodies() {
    let root = repo_root().join(".fixtures/sample-corpus");
    let compact = run(&[
        "--root",
        root.to_str().expect("utf8 fixture root"),
        "context",
        "v17 conformance audit",
        "--hits=3",
        "--format=json",
    ]);
    let compact_rows = json_rows(&compact);
    assert!(
        compact_rows.iter().all(|row| row.get("text").is_none()),
        "{compact_rows:#?}"
    );

    let expanded = run(&[
        "--root",
        root.to_str().expect("utf8 fixture root"),
        "context",
        "v17 conformance audit",
        "--hits=3",
        "--read-spans",
        "--format=json",
    ]);
    let expanded_rows = json_rows(&expanded);
    assert!(
        expanded_rows.iter().any(|row| row.get("text").is_some()),
        "{expanded_rows:#?}"
    );
}

#[test]
fn context_read_spans_escapes_control_characters_in_json() {
    let dir = tempdir();
    write_config(
        dir.path(),
        &lifecycle_config(&["draft"], &["done"], &["draft", "done"]),
    );
    write_markdown(
        dir.path(),
        "control.md",
        "draft",
        "# Control\n\nNeedle before \u{0007} after.\n",
    );

    let output = run(&[
        "--root",
        dir.path().to_str().expect("utf8 tempdir"),
        "context",
        "needle",
        "--hits=1",
        "--read-spans",
        "--format=json",
    ]);

    let stdout = text(&output.stdout);
    let rows = json_rows(&output);
    assert!(stdout.contains(r"\u0007"), "{stdout}");
    assert!(rows.iter().any(|row| {
        row["section"] == "span"
            && row
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains('\u{0007}'))
    }));
}

#[test]
fn status_sections_are_mutually_exclusive() {
    let output = run(&["--root", ".design", "status", "--format=json"]);
    let rows = json_rows(&output);
    let mut sections_by_handle = BTreeMap::<String, BTreeSet<String>>::new();
    for row in rows {
        let Some(handle) = row.get("h").and_then(Value::as_str) else {
            continue;
        };
        let Some(section) = row.get("section").and_then(Value::as_str) else {
            continue;
        };
        sections_by_handle
            .entry(handle.to_string())
            .or_default()
            .insert(section.to_string());
    }

    let duplicates = sections_by_handle
        .iter()
        .filter(|(_, sections)| sections.len() > 1)
        .collect::<Vec<_>>();
    assert!(duplicates.is_empty(), "{duplicates:#?}");
}

#[test]
fn live_spec_code_refs_warn_only_for_confident_missing_targets() {
    let dir = tempdir();
    let repo = dir.path().join("repo");
    let design = repo.join(".design");
    std::fs::create_dir_all(&repo).expect("create repo");
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.name", "Anneal Test"]);
    run_git(&repo, &["config", "user.email", "anneal@example.test"]);
    std::fs::create_dir_all(repo.join("lib")).expect("create lib");
    std::fs::create_dir_all(&design).expect("create design root");
    write_file(&repo, "lib/live.rs", "pub fn live() {}\n");
    write_file(&repo, "lib/missing.rs", "pub fn old() {}\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "seed code history"]);
    std::fs::remove_file(repo.join("lib/missing.rs")).expect("remove historical code");
    write_config(
        &design,
        &lifecycle_config(
            &["draft", "plan"],
            &["superseded"],
            &["draft", "plan", "superseded"],
        ),
    );
    write_markdown(
        &design,
        "live-missing.md",
        "draft",
        "# Live Missing\n\nStill points at `lib/missing.rs`.\n",
    );
    write_markdown(
        &design,
        "live-existing.md",
        "draft",
        "# Live Existing\n\nStill points at `lib/live.rs`.\n",
    );
    write_markdown(
        &design,
        "superseded-missing.md",
        "superseded",
        "# Historical Missing\n\nHistorical note points at `lib/missing.rs`.\n",
    );
    write_markdown(
        &design,
        "plan-missing.md",
        "plan",
        "# Forward Plan\n\nForward plan points at future code `lib/missing.rs`.\n",
    );
    write_markdown(
        &design,
        "illustrative.md",
        "draft",
        "# Example\n\nIllustrative prose quotes never-tracked code `lib/never.rs`.\n",
    );

    let diagnostics = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "-e",
        r#"? diagnostic(code, severity, subject, file, line, evidence), code = "W006"."#,
        "--format=json",
    ]);
    let rows = json_rows(&diagnostics);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0]["subject"], "live-missing.md");
    assert_eq!(rows[0]["severity"], "warning");
    assert!(
        rows[0]["evidence"].to_string().contains("spec_code_drift"),
        "{rows:#?}"
    );

    let meta = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "-e",
        r#"? *meta{handle: h, key: "target_exists", value: exists}, *meta{handle: h, key: "target_probe_base", value: base}, *meta{handle: h, key: "target_in_history", value: in_history}."#,
        "--format=json",
    ]);
    let meta_rows = json_rows(&meta);
    let repo = repo.to_string_lossy().into_owned();
    assert!(
        meta_rows.iter().any(|row| {
            row["h"] == "lib/live.rs"
                && row["exists"] == "true"
                && row["in_history"] == "false"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
    assert!(
        meta_rows.iter().any(|row| {
            row["h"] == "lib/missing.rs"
                && row["exists"] == "false"
                && row["in_history"] == "true"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
    assert!(
        meta_rows.iter().any(|row| {
            row["h"] == "lib/never.rs"
                && row["exists"] == "unknown"
                && row["in_history"] == "false"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}
