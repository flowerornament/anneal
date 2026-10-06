//! Cold-start honesty integration tests for the bundled agent briefing.

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

#[test]
fn project_diagnostic_clause_cannot_disable_builtin_check_errors() {
    let dir = tempdir();
    write_config(dir.path(), "");
    write_file(
        dir.path(),
        "a.md",
        "---\nstatus: active\n---\n# A\n\n[Missing](nope.md)\n",
    );

    let baseline = run_in(dir.path(), &["check", "--format=json"]);
    assert!(!baseline.status.success(), "broken reference must fail");
    assert!(text(&baseline.stdout).contains(r#""code":"E001""#));

    write_config(
        dir.path(),
        r#"diagnostic("P001", "info", "project", null, null, "project-row")."#,
    );
    let protected = run_in(dir.path(), &["check", "--format=json"]);
    assert!(!protected.status.success(), "protected shadow must fail");
    let stderr = text(&protected.stderr);
    assert!(stderr.contains("diagnostic/6"), "{stderr}");
    assert!(stderr.contains("can make a broken corpus pass"), "{stderr}");
    assert!(stderr.contains("separately named predicate"), "{stderr}");
    assert!(stderr.contains("ANNEAL_PRELUDE_PATH"), "{stderr}");
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
    assert!(stdout.contains("Scale        0 handles"), "{stdout}");
    assert!(
        stdout.contains("no corpus facts found; root may be empty or unresolved"),
        "{stdout}"
    );
}

#[test]
fn no_marker_directory_with_markdown_signals_errors_before_stdout_report() {
    let dir = tempdir();
    write_markdown(
        dir.path(),
        "stray.md",
        "draft",
        "# Stray\n\naccidental corpus\n",
    );

    let output = run_in(dir.path(), &["status", "--format=text"]);

    assert!(
        !output.status.success(),
        "unmarked implicit roots should fail\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "wrong-root failures must not emit a plausible report\nstdout:\n{}",
        text(&output.stdout)
    );
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("no marked corpus root found above"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("refusing implicit scan"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("anneal init --dry-run"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("--root <path>"), "stderr:\n{stderr}");
}

#[test]
fn init_dry_run_still_recovers_unmarked_roots() {
    let dir = tempdir();
    write_markdown(
        dir.path(),
        "stray.md",
        "draft",
        "# Stray\n\naccidental corpus\n",
    );

    let output = run_in(dir.path(), &["init", "--dry-run", "--format=text"]);

    assert_success(&output);
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("source md") && stdout.contains("scan_root"),
        "stdout:\n{stdout}"
    );
    let stderr = text(&output.stderr);
    assert!(
        !stderr.contains("refusing implicit scan"),
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
fn frontmatter_mapping_gap_works_without_config_and_mapping_closes_it() {
    let dir = tempdir();
    let lifecycle = format!(
        "{}\n\nconfig dependency {{ valid([\"done\"]). }}",
        lifecycle_config(&["draft"], &["done"], &["draft", "done"])
    );
    write_config(dir.path(), &lifecycle);
    write_file(
        dir.path(),
        "a.md",
        "---\nstatus: draft\nrefs:\n  - target.md\n  - second.md\n---\n# A\n",
    );
    write_file(
        dir.path(),
        "b.md",
        "---\nstatus: draft\nrefs: target.md\n---\n# B\n",
    );
    write_markdown(dir.path(), "target.md", "done", "# Target\n");
    write_markdown(dir.path(), "second.md", "done", "# Second\n");

    let root = dir.path().to_str().expect("utf8 tempdir");
    let gap = run(&[
        "--root",
        root,
        "-e",
        r"? frontmatter_mapping_gap(key, distinct_handle_count, suggested_field, edge_kind, direction).",
        "--format=json",
    ]);
    let rows = json_rows(&gap);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0]["key"], "refs");
    assert_eq!(rows[0]["distinct_handle_count"], 2);
    assert_eq!(rows[0]["suggested_field"], "references");
    assert_eq!(rows[0]["edge_kind"], "Cites");
    assert_eq!(rows[0]["direction"], "forward");

    let check = run(&["--root", root, "check", "--format=json"]);
    assert_success(&check);
    assert!(
        check.stdout.is_empty(),
        "warning-only check must keep machine stdout clean: {}",
        text(&check.stdout)
    );
    assert!(
        text(&check.stderr).contains("1 non-error diagnostic rows remain"),
        "warning count should motivate the diagnostic drill-down: {}",
        text(&check.stderr)
    );

    let edges_before = json_rows(&run(&[
        "--root",
        root,
        "-e",
        r#"? *edge{from: src, to: target, kind: "Cites"}."#,
        "--format=json",
    ]));
    assert!(
        edges_before.is_empty(),
        "unmapped frontmatter must not manufacture edges: {edges_before:#?}"
    );

    write_config(
        dir.path(),
        &format!(
            r#"config frontmatter {{
  field("refs", "Cites", "forward").
}}

{lifecycle}"#
        ),
    );

    let gaps_after = json_rows(&run(&[
        "--root",
        root,
        "-e",
        "? frontmatter_mapping_gap(key, count, field, kind, direction).",
        "--format=json",
    ]));
    assert!(
        gaps_after.is_empty(),
        "configured aliases must leave the gap relation: {gaps_after:#?}"
    );

    let edges_after = json_rows(&run(&[
        "--root",
        root,
        "-e",
        r#"? *edge{from: src, to: target, kind: "Cites"}."#,
        "--format=json",
    ]));
    assert_eq!(edges_after.len(), 3, "{edges_after:#?}");
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
            && row["summary"] == "Error Model and Load Shedding"),
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
        "harbor ledger conformance audit",
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
        "harbor ledger conformance audit",
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
        r#"? *meta{handle: h, key: "target_exists", value: exists}, *meta{handle: h, key: "target_probe_base", value: base}, *meta{handle: h, key: "target_history_status", value: history}, *meta{handle: h, key: "target_path", value: path}."#,
        "--format=json",
    ]);
    let meta_rows = json_rows(&meta);
    let repo = repo.to_string_lossy().into_owned();
    assert!(
        meta_rows.iter().any(|row| {
            row["h"]
                .as_str()
                .is_some_and(|h| h.starts_with("external:code:"))
                && row["path"] == "lib/live.rs"
                && row["exists"] == "true"
                && row["history"] == "present"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
    assert!(
        meta_rows.iter().any(|row| {
            row["h"]
                .as_str()
                .is_some_and(|h| h.starts_with("external:code:"))
                && row["path"] == "lib/missing.rs"
                && row["exists"] == "false"
                && row["history"] == "present"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
    assert!(
        meta_rows.iter().any(|row| {
            row["h"]
                .as_str()
                .is_some_and(|h| h.starts_with("external:code:"))
                && row["path"] == "lib/never.rs"
                && row["exists"] == "unknown"
                && row["history"] == "absent"
                && row["base"] == repo
        }),
        "{meta_rows:#?}"
    );
}

#[test]
fn refresh_drift_builds_cache_and_handle_reads_annotations() {
    let dir = tempdir();
    let repo = dir.path().join("repo");
    let design = repo.join(".design");
    std::fs::create_dir_all(repo.join("src")).expect("create src");
    std::fs::create_dir_all(&design).expect("create design root");
    run_git(&repo, &["init"]);
    write_file(&repo, "src/cli.rs", "pub fn run() {}\n");
    write_config(
        &design,
        &lifecycle_config(&["active"], &["archived"], &["active", "archived"]),
    );
    write_markdown(
        &design,
        "spec.md",
        "active",
        "# Spec\n\nThe CLI lived in `src/cli.rs`.\n",
    );
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add cli and spec"]);
    std::fs::remove_file(repo.join("src/cli.rs")).expect("remove cli");
    write_file(&repo, "src/cli/main.rs", "pub fn main() {}\n");
    write_file(&repo, "src/cli/app.rs", "pub fn app() {}\n");
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "split cli"]);

    let cold = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "handle",
        "spec.md",
        "--format=text",
    ]);
    assert_success(&cold);
    let cold_stdout = text(&cold.stdout);
    assert!(
        cold_stdout.contains("drift evidence not built; run `anneal check --refresh-drift`"),
        "stdout:\n{cold_stdout}"
    );

    let refresh = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "check",
        "--refresh-drift",
        "--format=text",
    ]);
    assert_success(&refresh);
    assert!(
        repo.join(".anneal/drift-evidence.json").exists(),
        "refresh writes the evidence cache"
    );

    let warm = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "handle",
        "spec.md",
        "--format=text",
    ]);
    assert_success(&warm);
    let warm_stdout = text(&warm.stdout);
    assert!(
        warm_stdout.contains("src/cli.rs  [referent-moved-ambiguous · 2 candidates]"),
        "stdout:\n{warm_stdout}"
    );
    assert!(
        !warm_stdout.contains("drift evidence not built"),
        "stdout:\n{warm_stdout}"
    );

    let warning = run(&[
        "--root",
        design.to_str().expect("utf8 design root"),
        "-e",
        r#"? diagnostic(code, severity, subject, file, line, evidence), code = "W006"."#,
        "--format=json",
    ]);
    let rows = json_rows(&warning);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0]["subject"], "spec.md");
    assert_eq!(rows[0]["severity"], "warning");
}

#[test]
fn status_discloses_included_gitignored_markdown_files_without_diagnosing_them() {
    let dir = tempdir();
    write_config(dir.path(), "");
    write_file(dir.path(), ".gitignore", "generated/\n");
    write_file(dir.path(), "real.md", "# Real\n");
    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["add", ".gitignore", "anneal.dl", "real.md"]);
    run_git(dir.path(), &["commit", "-m", "add corpus"]);
    write_file(dir.path(), "generated/artifact.md", "# Generated\n");

    let query = run_in(
        dir.path(),
        &["-e", "? gitignored_scanned_file(h, file).", "--format=json"],
    );
    let rows = json_rows(&query);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0]["h"], "generated/artifact.md");
    assert_eq!(rows[0]["file"], "generated/artifact.md");

    let status = run_in(dir.path(), &["status", "--format=text"]);
    assert_success(&status);
    assert!(
        text(&status.stdout).contains(
            "Scope        1 Git-ignored Markdown file handle included; query `gitignored_scanned_file`"
        ),
        "stdout:\n{}",
        text(&status.stdout)
    );

    let check = run_in(dir.path(), &["check", "--format=json"]);
    assert_success(&check);
    assert!(
        text(&check.stdout).is_empty(),
        "Git-ignore membership is a scope observation, not a diagnostic: {}",
        text(&check.stdout)
    );

    let describe = run_in(dir.path(), &["describe", "gitignored_scanned_file"]);
    assert_success(&describe);
    let description = text(&describe.stdout);
    assert!(description.contains("configured filesystem scanning"));
    assert!(description.contains("no rows do not establish that files are tracked"));
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
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

#[test]
fn structured_frontmatter_warns_with_queryable_evidence_and_preserves_scalars() {
    let dir = tempdir();
    write_config(dir.path(), "");
    write_file(
        dir.path(),
        "a.md",
        "---\nwork: {owner: Ada}\nmixed: [keep, {owner: Grace}, also]\n---\n# A\n",
    );
    write_file(
        dir.path(),
        "flow.md",
        "---\n{work: {owner: Ada}}\n---\n# Flow\n",
    );
    let root = dir.path().to_str().expect("utf8 tempdir");
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic{code: "W008", severity: severity, subject: h, file: file, line: line, evidence: evidence}."#,
    ]));
    assert_eq!(rows.len(), 3, "{rows:#?}");
    let mut locations = BTreeMap::new();
    for row in &rows {
        assert_eq!(row["severity"], "warning");
        assert!(row["line"].is_null());
        let evidence = row["evidence"].as_array().expect("evidence tuple");
        assert_eq!(evidence[0], "unmodeled_frontmatter_shape");
        let shape: Value = serde_json::from_str(evidence[1].as_str().expect("JSON string"))
            .expect("shape evidence");
        locations.insert(
            (
                row["file"].as_str().expect("file").to_owned(),
                shape["key"].as_str().expect("key").to_owned(),
            ),
            (
                shape["line"].as_u64().expect("line"),
                shape["line_exact"].as_bool().expect("exact"),
            ),
        );
    }
    assert_eq!(
        locations,
        BTreeMap::from([
            (("a.md".to_owned(), "work".to_owned()), (2, true)),
            (("a.md".to_owned(), "mixed".to_owned()), (3, true)),
            (("flow.md".to_owned(), "work".to_owned()), (1, false)),
        ])
    );
    let scalars = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? *meta{key: "mixed", value: value, role: role}."#,
    ]));
    assert_eq!(scalars.len(), 2);
    assert_eq!(
        scalars
            .iter()
            .map(|row| row["value"].as_str().expect("scalar"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["keep", "also"])
    );
    assert!(
        scalars
            .iter()
            .all(|row| row["role"] == "authored_unmodeled")
    );
    assert_success(&run(&["--root", root, "--json", "check"]));
    let card = run(&["describe", "W008"]);
    assert_success(&card);
    assert!(text(&card.stdout).contains("authorial absence"));
}

fn fixture_vcs(root: &Path, program: &str, args: &[&str]) -> String {
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
    let output = command.args(args).output().expect("fixture vcs command");
    assert_success(&output);
    text(&output.stdout).trim().to_string()
}

#[test]
fn real_jj_desk_uses_own_pin_mount_origins_and_ignore_rules_with_null_provenance() {
    if Command::new("jj").arg("--version").output().is_err() {
        eprintln!("real jj fixture skipped: jj executable unavailable");
        return;
    }
    let dir = tempdir();
    let ancestor = dir.path().join("ancestor");
    std::fs::create_dir_all(&ancestor).expect("mkdir");
    fixture_vcs(&ancestor, "git", &["init", "--quiet"]);
    write_file(&ancestor, ".git/info/exclude", "*.md\n");
    let anchor = ancestor.join("anchor");
    std::fs::create_dir_all(&anchor).expect("mkdir");
    fixture_vcs(&anchor, "git", &["init", "--quiet"]);
    write_file(
        &anchor,
        ".gitignore",
        ".design/ignored.md\n.design/tracked.md\n",
    );
    write_file(
        &anchor,
        ".design/anneal.dl",
        "source md { file_extension(\".md\"). scan_root(\".\"). external_root(\"../docs\"). }\n",
    );
    write_file(
        &anchor,
        ".design/spec.md",
        "---\nstatus: active\n---\n# Spec\n\nSee `lib/old.rs`.\n",
    );
    write_file(
        &anchor,
        ".design/tracked.md",
        "# Tracked despite ignore rule\n",
    );
    write_file(&anchor, "docs/external.md", "# External\n");
    write_file(&anchor, "lib/old.rs", "pub fn old() {}\n");
    fixture_vcs(&anchor, "git", &["add", "-f", "."]);
    fixture_vcs(&anchor, "git", &["commit", "--quiet", "-m", "baseline"]);
    fixture_vcs(&anchor, "jj", &["git", "init", "--colocate"]);
    let desk = ancestor.join("desk");
    fixture_vcs(
        &anchor,
        "jj",
        &["workspace", "add", desk.to_str().expect("utf8")],
    );
    std::fs::remove_file(desk.join("lib/old.rs")).expect("remove target only at desk");
    fixture_vcs(&desk, "jj", &["describe", "-m", "remove target"]);
    // These are unsnapshotted: the pin still supplies tracked-ness and history.
    write_file(&desk, ".design/ignored.md", "# Ignored\n");
    write_file(
        &desk,
        ".design/private.md",
        "# Private excludes must not apply\n",
    );
    write_file(&anchor, ".git/info/exclude", ".design/private.md\n");
    let root = desk.join(".design");
    let root = root.to_str().expect("utf8");
    let capabilities = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? repository_operation_capability(operation, availability, provider, reason).",
    ]));
    for operation in ["change_history", "target_history", "ignore_index"] {
        assert!(
            capabilities
                .iter()
                .any(|row| row["operation"] == operation && row["availability"] == "available"),
            "{capabilities:#?}"
        );
    }
    assert!(
        capabilities.iter().any(
            |row| row["operation"] == "assertion_blame" && row["availability"] == "unavailable"
        )
    );
    let recency = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? git_mtime(file, instant).",
    ]));
    assert!(
        recency.iter().any(
            |row| row["file"] == "docs/external.md" && row["instant"] == "2001-01-01T00:00:00Z"
        ),
        "{recency:#?}"
    );
    let ignored = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? *meta{key: "md.scan_git_disposition", handle: h, value: value}."#,
    ]));
    assert_eq!(ignored.len(), 1, "{ignored:#?}");
    assert_eq!(ignored[0]["h"], "ignored.md");
    let target = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? *meta{key: "target_exists", value: value}."#,
    ]));
    assert!(
        target.iter().any(|row| row["value"] == "false"),
        "{target:#?}"
    );
    assert!(
        anchor.join("lib/old.rs").exists(),
        "anchor must disagree with desk"
    );
    let edges = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? *edge{assertion_date: date, assertion_revision: revision}.",
    ]));
    assert!(!edges.is_empty());
    assert!(
        edges
            .iter()
            .all(|row| row["date"].is_null() && row["revision"].is_null())
    );
    let status = run(&["--root", root, "--format=text", "status"]);
    assert_success(&status);
    assert!(text(&status.stdout).contains(
        "recency available (change author time), W006 available, assertion provenance unavailable"
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.path().join("fault-bin");
        std::fs::create_dir_all(&bin).expect("wrapper directory");
        let wrapper = bin.join("git");
        std::fs::write(&wrapper, "#!/bin/sh\nfor arg do\n  if [ \"$arg\" = '--format=%x00%aI' ]; then exit 1; fi\ndone\nexec \"$ANNEAL_TEST_REAL_GIT\" \"$@\"\n").expect("fault wrapper");
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .expect("executable wrapper");
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("resolve Git");
        assert_success(&real_git);
        let path = std::env::var_os("PATH").expect("PATH");
        let paths = std::iter::once(bin).chain(std::env::split_paths(&path));
        let fault = Command::new(anneal_bin())
            .current_dir(&desk)
            .env("PATH", std::env::join_paths(paths).expect("joined PATH"))
            .env("ANNEAL_TEST_REAL_GIT", text(&real_git.stdout).trim())
            .args([
                "--root",
                root,
                "--json",
                "-e",
                "? repository_operation_capability(operation, availability, provider, reason).",
            ])
            .output()
            .expect("fault probe");
        let rows = json_rows(&fault);
        assert!(
            rows.iter().any(|row| row["operation"] == "change_history"
                && row["availability"] == "unavailable"
                && row["reason"] == "jj-change-history-probe-failed"),
            "{rows:#?}"
        );
        for operation in ["target_history", "ignore_index"] {
            assert!(
                rows.iter()
                    .any(|row| row["operation"] == operation && row["availability"] == "available"),
                "{rows:#?}"
            );
        }
    }
}

#[test]
fn dependency_gap_cards_teach_distinct_direct_reach_and_zero() {
    for name in ["S006", "dependency_config_gap"] {
        let result = run(&["describe", name]);
        assert_success(&result);
        let card = text(&result.stdout);
        for teaching in [
            "active_dependents",
            "distinct active source handles",
            "non-null status",
            "direct DependsOn",
            "including zero",
            "transitive dependents",
            "Cites",
        ] {
            assert!(
                card.contains(teaching),
                "{name} must teach {teaching}: {card}"
            );
        }
    }
}

#[test]
fn soft_lifecycle_replacements_reach_cli_queries_verbs_and_diagnostics() {
    let dir = tempdir();
    write_file(
        dir.path(),
        "a.md",
        "---\nstatus: draft\ndepends-on: c.md\n---\n# A\n",
    );
    write_file(dir.path(), "b.md", "# B\n");
    write_file(dir.path(), "c.md", "---\nstatus: archived\n---\n# C\n");
    let config = r#"config convergence { active(["draft"]). terminal(["archived"]). }"#;
    let project = format!(
        r#"{config}
        active(h) := eligible(h).
        eligible(h) := *handle{{id: h}}, h != "c.md".
        selected(h) := active(h).
        @verb(name: "census_active", query: "? active(h).", doc: "Active override",
          output_schema: "{{\"h\":\"String\"}}", args: [], capabilities: ["read"]).
    "#
    );
    write_file(dir.path(), "anneal.dl", &project);
    let selected = vec![
        serde_json::json!({"h": "a.md"}),
        serde_json::json!({"h": "b.md"}),
    ];
    for (query, expected) in [
        ("? active(h).", selected.clone()),
        ("? selected(h).", selected.clone()),
        (
            "? n = Count{ h : active(h) }.",
            vec![serde_json::json!({"n": 2})],
        ),
        (
            "? *handle{id: h}, not active(h).",
            vec![serde_json::json!({"h": "c.md"})],
        ),
        (
            "? where selected(h) := active(h). selected(h).",
            selected.clone(),
        ),
    ] {
        assert_eq!(
            json_rows(&run_in(dir.path(), &["--json", "-e", query])),
            expected,
            "{query}"
        );
    }
    assert_eq!(
        json_rows(&run_in(dir.path(), &["--json", "census_active"])),
        selected
    );
    let warnings = json_rows(&run_in(
        dir.path(),
        &[
            "--json",
            "-e",
            r#"? diagnostic("W001", severity, subject, file, line, evidence)."#,
        ],
    ));
    assert_eq!(
        warnings,
        vec![
            serde_json::json!({"severity":"warning", "subject":"a.md", "file":"a.md", "line":null, "evidence":["stale_ref","draft","archived"]})
        ]
    );

    write_file(dir.path(), "anneal.dl", config);
    assert_eq!(
        json_rows(&run_in(dir.path(), &["--json", "-e", "? active(h)."])),
        selected
    );
    assert_eq!(
        json_rows(&run_in(
            dir.path(),
            &[
                "--json",
                "-e",
                r#"active(h) := *handle{id: h}, h != "c.md". ? active(h)."#
            ]
        )),
        selected
    );
    write_file(
        dir.path(),
        "anneal.dl",
        &format!("{config} active(\"b.md\")."),
    );
    assert_eq!(
        json_rows(&run_in(dir.path(), &["--json", "-e", "? active(h)."])),
        vec![serde_json::json!({"h":"b.md"})]
    );
    write_file(
        dir.path(),
        "anneal.dl",
        &format!("{config} upstream(h,t) := *edge{{from:h,to:t}}."),
    );
    let sealed = run_in(dir.path(), &["--json", "-e", "? upstream(h,t)."]);
    assert!(!sealed.status.success());
    assert!(text(&sealed.stderr).contains("cannot be defined by corpus rules"));
}

#[test]
fn diagnostic_suppression_matches_only_code_and_exact_subject() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    for path in ["a.md", "b.md"] {
        write_file(
            corpus.path(),
            path,
            "---\nstatus: draft\nreferences: missing.md\n---\n# Document\n",
        );
    }
    let query = r#"? diagnostic("E001", severity, subject, file, line, evidence)."#;
    let baseline = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert_eq!(baseline.len(), 2);
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));

    for rule in [
        r#"rule("E001", "missing.md")."#,
        r#"rule("E001", "*.md")."#,
        r#"rule("W001", "a.md")."#,
    ] {
        write_file(
            corpus.path(),
            "anneal.dl",
            &format!("config suppress {{ {rule} }}"),
        );
        assert_eq!(
            json_rows(&run(&["--root", root, "--json", "-e", query])),
            baseline
        );
        assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    }
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config suppress { rule("E001", "a.md"). }"#,
    );
    let remaining = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert_eq!(
        remaining,
        baseline
            .iter()
            .filter(|row| row["subject"] == "b.md")
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config suppress { rule("E001", "a.md"). rule("E001", "b.md"). }"#,
    );
    assert!(json_rows(&run(&["--root", root, "--json", "-e", query])).is_empty());
    assert_success(&run(&["--root", root, "check"]));
    assert_eq!(
        json_rows(&run(&[
            "--root",
            root,
            "--json",
            "-e",
            "? broken_reference(src, target, file, line)."
        ]))
        .len(),
        2
    );
}

#[test]
fn blanket_diagnostic_suppression_refuses_with_instance_remedy() {
    let corpus = tempdir();
    write_file(corpus.path(), "a.md", "# A\n");
    for declaration in [r#"code(["E001"])."#, "code([])."] {
        write_file(
            corpus.path(),
            "anneal.dl",
            &format!("config suppress {{ {declaration} }}"),
        );
        let output = run(&[
            "--root",
            corpus.path().to_str().expect("UTF-8 temp path"),
            "check",
        ]);
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(stderr.contains("suppress.code"), "{stderr}");
        assert!(stderr.contains("rule(CODE, target)"), "{stderr}");
    }
}

#[test]
fn aggregate_diagnostic_suppression_uses_subject_not_file_location() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    write_file(
        corpus.path(),
        "a.md",
        "---\nstatus: unmapped-test-status\n---\n# A\n",
    );
    let query = r#"? diagnostic("W005", severity, subject, file, line, evidence)."#;
    let baseline = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert!(!baseline.is_empty());
    assert!(
        baseline
            .iter()
            .all(|row| row["subject"] == "unmapped-test-status")
    );
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config suppress { rule("W005", "a.md"). }"#,
    );
    assert_eq!(
        json_rows(&run(&["--root", root, "--json", "-e", query])),
        baseline
    );
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config suppress { rule("W005", "unmapped-test-status"). }"#,
    );
    assert!(json_rows(&run(&["--root", root, "--json", "-e", query])).is_empty());
    let card = run(&["--root", root, "describe", "suppress"]);
    assert_success(&card);
    assert!(text(&card.stdout).contains("subject identity exactly"));
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"diagnostic_subject_suppressed(key, subject) := *handle{id: subject}, key = "suppress.rule.E001"."#,
    );
    let output = run(&["--root", root, "check"]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("protected standard-library relation"));
}

#[test]
fn label_diagnostic_suppression_does_not_match_its_file_location() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    write_file(
        corpus.path(),
        "a.md",
        "---\nstatus: draft\n---\n# A\n## OQ-1 An obligation\n\nWhat is owed?\n",
    );
    let handles = r#"config handles { force(["OQ"]). linear(["OQ"]). }"#;
    let query = r#"? diagnostic("E002", severity, subject, file, line, evidence)."#;
    write_file(corpus.path(), "anneal.dl", handles);
    let baseline = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0]["subject"], "OQ-1");
    assert_eq!(baseline[0]["file"], "a.md");
    write_file(
        corpus.path(),
        "anneal.dl",
        &format!(r#"{handles} config suppress {{ rule("E002", "a.md"). }}"#),
    );
    assert_eq!(
        json_rows(&run(&["--root", root, "--json", "-e", query])),
        baseline
    );
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    write_file(
        corpus.path(),
        "anneal.dl",
        &format!(r#"{handles} config suppress {{ rule("E002", "OQ-1"). }}"#),
    );
    assert!(json_rows(&run(&["--root", root, "--json", "-e", query])).is_empty());
    assert_success(&run(&["--root", root, "check"]));
}

#[test]
fn diagnostic_escalation_regrades_gate_and_teaches_effective_policy() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    write_file(corpus.path(), "a.md", "---\nstatus: unfamiliar\n---\n# A\n");
    let query = "? diagnostic(code, severity, subject, file, line, evidence).";
    let baseline = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert!(baseline.iter().any(|row| row["code"] == "W005"));
    assert_success(&run(&["--root", root, "check"]));
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config diagnostics { escalate("W005", "error"). escalate("W005", "error"). }"#,
    );
    let after = json_rows(&run(&["--root", root, "--json", "-e", query]));
    let expected = baseline
        .into_iter()
        .map(|mut row| {
            if row["code"] == "W005" {
                row["severity"] = "error".into();
            }
            row
        })
        .collect::<Vec<_>>();
    assert_eq!(after, expected);
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    let policy = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? diagnostic_policy(code, declared_severity, effective_severity, origin).",
    ]));
    assert_eq!(policy.len(), 18);
    assert!(policy.iter().any(|row| row["code"] == "W005"
        && row["declared_severity"] == "warning"
        && row["effective_severity"] == "error"
        && row["origin"] == "project"));
    let status = run(&["--root", root, "status", "--format=text"]);
    assert_success(&status);
    assert!(text(&status.stdout).contains("1 codes escalated by project"));
    let card = run(&["--root", root, "describe", "W005"]);
    assert_success(&card);
    assert!(text(&card.stdout).contains("Declared severity: warning."));
    assert!(text(&card.stdout).contains("Effective severity: error."));
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? describe("W005", doc)."#,
    ]));
    assert!(rows.iter().any(|row| {
        row["doc"]
            .as_str()
            .is_some_and(|doc| doc.contains("Effective severity: error."))
    }));
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"config diagnostics { escalate("W005", "error"). } config suppress { rule("W005", "unfamiliar"). }"#,
    );
    assert_success(&run(&["--root", root, "check"]));
    let status = run(&["--root", root, "status", "--format=text"]);
    assert_success(&status);
    assert!(text(&status.stdout).contains("1 codes escalated by project"));
}

#[test]
fn diagnostic_escalation_refuses_unknown_downgrade_and_conflicting_codes() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    write_file(corpus.path(), "a.md", "# A\n");
    for body in [
        r#"escalate("P001", "error")."#,
        r#"escalate("W005", "banana")."#,
        r#"escalate("E001", "warning")."#,
        r#"escalate("W005", "suggestion")."#,
        r#"escalate("I001", "suggestion")."#,
        r#"escalate("S001", "info")."#,
        r#"escalate("S001", "warning"). escalate("S001", "error")."#,
        r#"escalate("S001", "error"). escalate("S001", "warning")."#,
    ] {
        write_file(
            corpus.path(),
            "anneal.dl",
            &format!("config diagnostics {{ {body} }}"),
        );
        let output = run(&["--root", root, "check"]);
        assert!(!output.status.success(), "{body}");
        assert!(
            text(&output.stderr).contains("diagnostics.escalate"),
            "{}",
            text(&output.stderr)
        );
    }
}

#[test]
fn diagnostic_policy_handles_noops_absent_instances_and_each_promotion() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("UTF-8 temp path");
    write_file(corpus.path(), "a.md", "---\nstatus: draft\n---\n# A\n");
    let query = "? diagnostic_policy(code, declared_severity, effective_severity, origin).";
    let baseline = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert_eq!(baseline.len(), 18);
    assert!(
        baseline.iter().all(|row| row["origin"] == "stdlib"
            && row["declared_severity"] == row["effective_severity"])
    );
    for (code, declared, effective) in [
        ("I001", "info", "warning"),
        ("I001", "info", "error"),
        ("S006", "suggestion", "warning"),
        ("S006", "suggestion", "error"),
        ("W006", "warning", "error"),
        ("I001", "info", "info"),
        ("S006", "suggestion", "suggestion"),
        ("W006", "warning", "warning"),
        ("E001", "error", "error"),
    ] {
        write_file(
            corpus.path(),
            "policy.dl",
            &format!(r#"config diagnostics {{ escalate("{code}", "{effective}"). }}"#),
        );
        write_file(corpus.path(), "anneal.dl", "include \"policy.dl\".\n");
        let rows = json_rows(&run(&["--root", root, "--json", "-e", query]));
        assert_eq!(rows.len(), 18);
        let policy = rows
            .iter()
            .find(|row| row["code"] == code)
            .expect("policy row");
        assert_eq!(policy["declared_severity"], declared);
        assert_eq!(policy["effective_severity"], effective);
        assert_eq!(
            policy["origin"],
            if declared == effective {
                "stdlib"
            } else {
                "project"
            }
        );
        let status = run(&["--root", root, "status", "--format=text"]);
        assert_success(&status);
        assert_eq!(
            text(&status.stdout).contains("codes escalated by project"),
            declared != effective
        );
        assert_success(&run(&["--root", root, "check"]));
    }
    for head in [
        r#"diagnostic_policy("W005", "warning", "info", "project")."#,
        r#"builtin_diagnostic("W005", "info", "project", null, null, null)."#,
        r#"builtin_diagnostic_policy("W005", "info", "diagnostics.escalate.W005")."#,
        r#"diagnostic_severity_promotion("error", "info")."#,
        r#"project_diagnostic_escalation("E001", "error", "info")."#,
    ] {
        write_file(corpus.path(), "anneal.dl", head);
        let output = run(&["--root", root, "check"]);
        assert!(!output.status.success());
        assert!(
            text(&output.stderr).contains("protected standard-library relation"),
            "{}",
            text(&output.stderr)
        );
    }
}

fn project_card(severity: &str) -> String {
    format!(
        r#"@diagnostic(code: "P001", severity: "{severity}", doc: "Synthetic project finding.", rule: project_diagnostic, evidence: ["tag", "value"])."#
    )
}

#[test]
fn project_diagnostic_policy_composes_with_builtins_and_cards() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("root");
    write_file(corpus.path(), "a.md", "---\nstatus: unknown\n---\n# A\n");
    let query = "? diagnostic(code, severity, subject, file, line, evidence).";
    let builtin = json_rows(&run(&["--root", root, "--json", "-e", query]));
    assert!(!builtin.is_empty());
    for (declared, effective) in [
        ("info", "warning"),
        ("info", "error"),
        ("suggestion", "warning"),
        ("suggestion", "error"),
        ("warning", "error"),
        ("warning", "warning"),
    ] {
        let rules = format!(
            r#"{} project_diagnostic("P001", "{declared}", "a.md", "a.md", 4, ("tag", "value")). config diagnostics {{ escalate("P001", "{effective}"). }}"#,
            project_card(declared)
        );
        write_file(corpus.path(), "anneal.dl", &rules);
        let rows = json_rows(&run(&["--root", root, "--json", "-e", query]));
        let project = rows
            .iter()
            .filter(|row| row["code"] == "P001")
            .collect::<Vec<_>>();
        assert_eq!(project.len(), 1);
        assert_eq!(project[0]["severity"], effective);
        assert_eq!(
            rows.into_iter()
                .filter(|row| row["code"] != "P001")
                .collect::<Vec<_>>(),
            builtin
        );
        assert_eq!(
            run(&["--root", root, "check"]).status.code(),
            Some(i32::from(effective == "error"))
        );
        let card = run(&["--root", root, "describe", "P001"]);
        assert_success(&card);
        assert!(text(&card.stdout).contains(&format!("Severity: {effective}.")));
        assert!(text(&card.stdout).contains("Declaration ownership: project."));
        let policy = json_rows(&run(&[
            "--root",
            root,
            "--json",
            "-e",
            r#"? diagnostic_policy("P001", declared, effective, origin)."#,
        ]));
        assert_eq!(policy.len(), 1);
        assert_eq!(
            policy[0]["origin"],
            if declared == effective {
                "stdlib"
            } else {
                "project"
            }
        );
        for target in ["a*", "value", "a.md"] {
            write_file(
                corpus.path(),
                "anneal.dl",
                &format!(r#"{rules} config suppress {{ rule("P001", "{target}"). }}"#),
            );
            let rows = json_rows(&run(&["--root", root, "--json", "-e", query]));
            assert_eq!(
                rows.iter().filter(|row| row["code"] == "P001").count(),
                usize::from(target != "a.md")
            );
            assert_eq!(
                json_rows(&run(&[
                    "--root",
                    root,
                    "--json",
                    "-e",
                    r"? project_diagnostic(code, severity, subject, file, line, evidence)."
                ]))
                .len(),
                1
            );
            assert_success(&run(&["--root", root, "describe", "P001"]));
        }
    }
}

#[test]
fn project_diagnostic_dynamic_contract_errors_name_each_clause() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("root");
    write_file(corpus.path(), "a.md", "# A\n");
    let rules = format!(
        r#"{}
input("W001", "warning").
input("P999", "warning").
input(42, "warning").
input("P001", "error").
project_diagnostic(code, severity, "a.md", null, null, null) := input(code, severity).
project_diagnostic(code, severity, "a.md", null, null, null) := input(code, severity).
"#,
        project_card("warning")
    );
    write_file(corpus.path(), "anneal.dl", &rules);
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic("E003", severity, subject, file, line, evidence)."#,
    ]));
    assert_eq!(rows.len(), 8, "four failures from each of two clauses");
    let origins = rows
        .iter()
        .map(|row| row["evidence"][1].as_str().expect("producer"))
        .collect::<BTreeSet<_>>();
    assert_eq!(origins.len(), 2);
    assert!(
        origins
            .iter()
            .all(|origin| origin.contains("anneal.dl:6:") || origin.contains("anneal.dl:7:"))
    );
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    assert!(
        json_rows(&run(&[
            "--root",
            root,
            "--json",
            "-e",
            r#"? diagnostic("P001", severity, subject, file, line, evidence)."#
        ]))
        .is_empty()
    );
}

#[test]
fn project_diagnostic_declarations_and_internal_relations_are_guarded() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("root");
    write_file(corpus.path(), "a.md", "# A\n");
    let card = project_card("warning");
    for rules in [
        format!(r#"{card} project_diagnostic("W001", "warning", "a", null, null, null)."#),
        format!(r#"{card} project_diagnostic(42, "warning", "a", null, null, null)."#),
        format!(r#"{card} project_diagnostic("P001", "error", "a", null, null, null)."#),
        format!(r#"{card} project_diagnostic("P001", 42, "a", null, null, null)."#),
        format!(r#"{card} {card} project_diagnostic("P001", "warning", "a", null, null, null)."#),
        card.replace("rule: project_diagnostic", "rule: missing"),
        card.replace("severity: \"warning\",", ""),
        card.replace("evidence: [\"tag\", \"value\"]", "evidence: []"),
        r#"project_diagnostic_producer("P001", "warning", "a", null, null, null, "forged")."#.to_owned(),
        r#"project_diagnostic_declaration("P001", "warning", "key", "key")."#.to_owned(),
        r#"validated_project_diagnostic("P001", "warning", "a", null, null, null)."#.to_owned(),
        r#"project_diagnostic_contract_failure("a", null, null, null)."#.to_owned(),
        r#"diagnostic_declaration("P001", "warning", "key")."#.to_owned(),
        r"diagnostic(code, severity, subject, file, line, evidence) := project_diagnostic(code, severity, subject, file, line, evidence).".to_owned(),
    ] {
        write_file(corpus.path(), "anneal.dl", &rules);
        let result = run(&["--root", root, "check"]);
        assert!(!result.status.success(), "accepted {rules}");
        assert!(!text(&result.stderr).is_empty());
    }
    // Literal P codes without a declaration are emitted contract errors, not dropped.
    write_file(
        corpus.path(),
        "anneal.dl",
        r#"project_diagnostic("P999", "warning", "a", null, null, null)."#,
    );
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic("E003", severity, subject, file, line, evidence)."#,
    ]));
    assert_eq!(rows.len(), 1);
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
}

#[test]
fn project_diagnostic_frontmatter_vocabulary_fixture_selects_deepest_spans() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("root");
    write_file(
        corpus.path(),
        "stale.md",
        "---\nstatus: draft\nretired_terms: [RatioUpdate]\n---\n# Contract\n\n## Carrier\n\n### Old table\n\n| Name | Value |\n| --- | --- |\n| RatioUpdate | stale |\n",
    );
    write_file(
        corpus.path(),
        "clean.md",
        "---\nstatus: draft\nretired_terms: [RatioUpdate]\n---\n# Clean\n\nParentRatios only.\n",
    );
    write_file(
        corpus.path(),
        "unscoped.md",
        "---\nstatus: draft\n---\n# Unscoped\n\nRatioUpdate is permitted without a local declaration.\n",
    );
    write_file(
        corpus.path(),
        "mixed.md",
        "---\nstatus: draft\nretired_terms: [MixedTerm]\n---\n# Parent\n\nMixedTerm in parent prose.\n\n## Child\n\nMixedTerm in child prose.\n",
    );
    write_file(
        corpus.path(),
        "anneal.dl",
        include_str!("fixtures/project-diagnostic-vocabulary.dl"),
    );
    let raw = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? retired_hit(h, term, span, start, end).",
    ]));
    assert_eq!(
        raw.len(),
        5,
        "three ancestors for stale and two mixed spans"
    );
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic("P001", severity, subject, file, line, evidence)."#,
    ]));
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["severity"] == "error"));
    assert!(rows.iter().any(|row| row["subject"] == "stale.md"));
    assert!(rows.iter().any(|row| row["subject"] == "mixed.md"));
    assert_eq!(run(&["--root", root, "check"]).status.code(), Some(1));
    // Remove the deliberately stale table row; only mixed's deepest child remains.
    write_file(
        corpus.path(),
        "stale.md",
        "---\nstatus: draft\nretired_terms: [RatioUpdate]\n---\n# Contract\n\nParentRatios.\n",
    );
    let after = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic("P001", severity, subject, file, line, evidence)."#,
    ]));
    assert_eq!(after.len(), 1);
    assert_eq!(after[0]["subject"], "mixed.md");
}

#[test]
fn project_diagnostic_imports_empty_populations_and_refused_grades() {
    let corpus = tempdir();
    let root = corpus.path().to_str().expect("root");
    write_file(corpus.path(), "a.md", "---\nstatus: draft\n---\n# A\n");
    write_file(
        corpus.path(),
        "library.dl",
        &format!(
            r#"{} project_diagnostic("P001", "warning", "a.md", null, null, null)."#,
            project_card("warning")
        ),
    );
    let import = "import library from \"library.dl\".";
    write_file(corpus.path(), "anneal.dl", import);
    assert_success(&run(&["--root", root, "check"]));
    assert!(
        json_rows(&run(&[
            "--root",
            root,
            "--json",
            "-e",
            r#"? diagnostic("P001", severity, subject, file, line, evidence)."#
        ]))
        .is_empty()
    );
    let card = run(&["--root", root, "describe", "P001"]);
    assert_success(&card);
    assert!(text(&card.stdout).contains("Rule predicate: library.project_diagnostic."));
    let policy = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic_policy("P001", declared, effective, origin)."#,
    ]));
    assert_eq!(policy.len(), 1);
    write_file(
        corpus.path(),
        "anneal.dl",
        &format!(
            "{import}\nproject_diagnostic(code, severity, subject, file, line, evidence) := library.project_diagnostic(code, severity, subject, file, line, evidence)."
        ),
    );
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        r#"? diagnostic("P001", severity, subject, file, line, evidence)."#,
    ]));
    assert_eq!(rows.len(), 1);
    for grade in [
        r#"escalate("P001", "info")."#,
        r#"escalate("P001", "suggestion")."#,
        r#"escalate("P001", "banana")."#,
        r#"escalate("P001", "warning"). escalate("P001", "error")."#,
    ] {
        write_file(
            corpus.path(),
            "anneal.dl",
            &format!("{import} config diagnostics {{ {grade} }}"),
        );
        let output = run(&["--root", root, "check"]);
        assert!(!output.status.success());
        assert!(text(&output.stderr).contains("diagnostics.escalate"));
    }
    // Query-local input rules affect that query only, never a later check.
    write_file(corpus.path(), "anneal.dl", import);
    let query = r"project_diagnostic(code, severity, subject, file, line, evidence) := library.project_diagnostic(code, severity, subject, file, line, evidence). ? project_diagnostic(code, severity, subject, file, line, evidence).";
    assert_eq!(
        json_rows(&run(&["--root", root, "--json", "-e", query])).len(),
        1
    );
    assert_success(&run(&["--root", root, "check"]));
}

fn committed_git_fixture(root: &Path, committer_date: &str) {
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
        vec!["add", "."],
        vec!["commit", "--quiet", "-m", "fixture"],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_AUTHOR_DATE", "2019-05-20T12:00:00+00:00")
            .env("GIT_COMMITTER_DATE", committer_date)
            .output()
            .expect("fixture Git");
        assert_success(&output);
    }
}

#[test]
fn direct_git_history_ignores_inherited_roots_and_keeps_committer_dates() {
    let dir = tempdir();
    let root = dir.path().join("repo");
    let foreign = dir.path().join("foreign");
    for path in [&root, &foreign] {
        write_file(
            path,
            ".design/a.md",
            "---\nstatus: draft\nreferences: code.rs\n---\n# A\n",
        );
        write_file(path, "code.rs", "fn target() {}\n");
    }
    committed_git_fixture(&root, "2020-05-20T12:00:00+00:00");
    committed_git_fixture(&foreign, "2025-05-20T12:00:00+00:00");
    let corpus = root.join(".design");
    let corpus = corpus.to_str().expect("root");
    for query in [
        "? git_mtime(file, instant).",
        "? repository_operation_capability(operation, availability, provider, reason).",
        "? *meta{handle: h, key: key, value: value}.",
    ] {
        let args = ["--root", corpus, "--json", "-e", query];
        let neutral = Command::new(anneal_bin())
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .output()
            .expect("neutral query");
        assert_success(&neutral);
        let expected = json_rows(&neutral);
        assert!(!expected.is_empty(), "{query} must exercise a population");
        if query.contains("git_mtime") {
            assert_eq!(
                expected,
                vec![serde_json::json!({"file":"a.md", "instant":"2020-05-20T12:00:00Z"})]
            );
        }
        for (git_dir, work_tree, common_dir) in [
            (
                PathBuf::from("/does/not/exist"),
                PathBuf::from("/wrong/worktree"),
                PathBuf::from("/wrong/common"),
            ),
            (foreign.join(".git"), root.clone(), foreign.join(".git")),
        ] {
            let actual = Command::new(anneal_bin())
                .args(args)
                .env("GIT_DIR", git_dir)
                .env("GIT_WORK_TREE", work_tree)
                .env("GIT_COMMON_DIR", common_dir)
                .output()
                .expect("injected query");
            assert_success(&actual);
            assert_eq!(json_rows(&actual), expected, "{query}");
        }
    }
}

#[test]
fn unborn_git_history_reports_only_change_history_unavailable() {
    let corpus = tempdir();
    write_file(corpus.path(), "a.md", "# A\n");
    let output = Command::new("git")
        .current_dir(corpus.path())
        .args(["init", "--quiet"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .expect("init");
    assert_success(&output);
    let root = corpus.path().to_str().expect("root");
    let rows = json_rows(&run(&[
        "--root",
        root,
        "--json",
        "-e",
        "? repository_operation_capability(operation, availability, provider, reason).",
    ]));
    assert_eq!(rows.len(), 5);
    let history = rows
        .iter()
        .find(|row| row["operation"] == "change_history")
        .expect("history row");
    assert_eq!(history["availability"], "unavailable");
    assert_eq!(history["reason"], "git-change-history-probe-failed");
    for operation in ["assertion_blame", "target_history", "ignore_index"] {
        assert!(
            rows.iter()
                .any(|row| row["operation"] == operation && row["availability"] == "available")
        );
    }
    assert!(
        json_rows(&run(&[
            "--root",
            root,
            "--json",
            "-e",
            "? git_mtime(file, instant)."
        ]))
        .is_empty()
    );
}

#[test]
fn intents_enumerates_all_declared_goals_and_project_cards_without_scores() {
    let dir = tempdir();
    write_file(dir.path(), "a.md", "# A\nCorpusquokkaonly evidence.\n");
    write_config(
        dir.path(),
        r#"
@diagnostic(code: "P991", severity: "warning", doc: "A declared project check.", rule: project_diagnostic, evidence: ["custom", "value"]).
@doc(name: "P991", doc: "Cannot replace sealed summary.", intents: ["Find marmot governance."]).
project_diagnostic("P991", "warning", h, file, 1, ("custom", value)) := *meta{handle: h, key: "nonexistent", value: value}, *handle{id: h, file: file}.
@verb(name: "project-card", query: "? sources(name, recognizes, capabilities, doc).", doc: "A project command.", output_schema: "{\"name\":\"String\",\"recognizes\":\"List<String>\",\"capabilities\":\"List<String>\",\"doc\":\"String\"}", args: [], capabilities: [], intents: ["Find marmot commands."]).
"#,
    );
    let rows = json_rows(&run_in(dir.path(), &["intents", "--json"]));
    assert_eq!(rows.len(), 257);
    let cards = rows
        .iter()
        .map(|row| {
            (
                row["name"].clone().to_string(),
                row["kind"].clone().to_string(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(cards.len(), 82);
    assert!(rows.iter().all(|row| {
        row.as_object()
            .expect("row")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["name", "kind", "intent"])
    }));
    assert!(
        rows.iter()
            .any(|row| row["name"] == "P991" && row["kind"] == "runtime topic")
    );
    assert!(
        rows.iter()
            .any(|row| row["name"] == "project-card" && row["kind"] == "verb")
    );
    assert!(rows.iter().all(|row| {
        !row["intent"]
            .as_str()
            .expect("intent")
            .contains("Corpusquokkaonly")
    }));
    assert_eq!(rows, json_rows(&run_in(dir.path(), &["intents", "--json"])));
    assert!(
        json_rows(&run_in(
            dir.path(),
            &[
                "--json",
                "-e",
                "? diagnostic(\"P991\", severity, h, file, line, evidence)."
            ]
        ))
        .is_empty()
    );
    let rendered = run_in(dir.path(), &["intents", "--format=text"]);
    assert_success(&rendered);
    let rendered = text(&rendered.stdout);
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.starts_with("- "))
            .count(),
        257
    );
    assert_eq!(
        rendered
            .lines()
            .filter(|line| !line.starts_with("- "))
            .count(),
        82
    );
    assert_eq!(rendered.matches("P991 [runtime topic]").count(), 1);
    assert!(!rendered.contains("score="));
    let removed = run_in(
        dir.path(),
        &["--json", "-e", "? card_search(\"goal\", n, k, d, s, l, r)."],
    );
    assert!(!removed.status.success());
}

#[test]
fn project_intents_override_keeps_generic_rendering() {
    let dir = tempdir();
    write_config(
        dir.path(),
        r#"@verb(name: "intents", query: "? sources(name, recognizes, capabilities, doc).", doc: "Project source list.", output_schema: "{\"name\":\"String\",\"recognizes\":\"List<String>\",\"capabilities\":\"List<String>\",\"doc\":\"String\"}", args: [], capabilities: [])."#,
    );
    let output = run_in(dir.path(), &["intents", "--format=text"]);
    assert_success(&output);
    assert!(
        text(&output.stdout).contains("name=markdown"),
        "{}",
        text(&output.stdout)
    );
}
