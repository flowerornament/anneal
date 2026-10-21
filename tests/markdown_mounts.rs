//! Cross-repository mounts exercise the shipped config, extraction, graph and read surfaces.
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .expect("git fixture");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8").trim().into()
}
fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.test"]);
}

fn git_input(root: &Path, args: &[&str], input: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .current_dir(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["rev-parse", "HEAD"])
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anneal"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn query(root: &Path, query: &str) -> Vec<Value> {
    let out = run(root, &["--root", root.to_str().unwrap(), "-e", query]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("query row"))
        .collect()
}
fn config(host: &Path, alias: &str, revision: Option<&str>) {
    fs::write(host.join("anneal.dl"), format!("source md {{ scan_root(\".design\"). mount \"{alias}\" {{ path(\"../foreign\"). scan_root(\"pages\"). {} }} }}\n", revision.map_or_else(String::new, |r| format!("revision(\"{r}\").")))).unwrap();
}

#[test]
fn mounts_join_body_and_frontmatter_links_without_merging_local_names() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/same.md"), "---\ndepends-on: other:pages/same.md\n---\n# Host\n[foreign](other:pages/same.md#details)\n[absent](other:pages/missing.md)\n[URI](other://example/path.md)\n[News](news:comp.lang.rust)\n").unwrap();
    fs::write(
        foreign.join("pages/same.md"),
        "# Foreign\n[local](next.md)\n## Details\nMounted unique payload.\n",
    )
    .unwrap();
    fs::write(foreign.join("pages/next.md"), "# Next\n").unwrap();
    commit(&foreign);
    config(&host, "other", None);
    let rows = query(&host, "? *handle{id: id, kind: \"file\"}.");
    let text = serde_json::to_string(&rows).unwrap();
    assert!(text.contains(".design/same.md"));
    assert!(text.contains("other:pages/same.md"));
    let edges = query(&host, "? *edge{from: from, to: to, kind: kind}.");
    let text = serde_json::to_string(&edges).unwrap();
    assert!(text.contains("other:pages/next.md"));
    assert!(text.contains("DependsOn"));
    let diagnostics = query(
        &host,
        "? diagnostic(\"E001\", severity, subject, file, line, evidence).",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        serde_json::to_string(&diagnostics)
            .unwrap()
            .contains("other:pages/missing.md")
    );
    let read = run(
        &host,
        &[
            "--root",
            host.to_str().unwrap(),
            "read",
            "other:pages/same.md",
            "--format=text",
        ],
    );
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert!(String::from_utf8_lossy(&read.stdout).contains("Mounted unique payload"));
}

#[test]
fn pinned_mount_reads_exact_tree_with_stable_handles_and_no_checkout_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/host.md"), "# Host\n").unwrap();
    fs::write(foreign.join("pages/guide.md"), "# Guide\nFirst revision.\n").unwrap();
    let base = commit(&foreign);
    // Git trees can hold byte paths that the host filesystem cannot create.
    let pages_tree = git(&foreign, &["rev-parse", "HEAD:pages"]);
    let blob = git(&foreign, &["rev-parse", "HEAD:pages/guide.md"]);
    let mut tree = format!("040000 tree {pages_tree}\tpages\0").into_bytes();
    tree.extend_from_slice(format!("100644 blob {blob}\tunrelated-").as_bytes());
    tree.extend_from_slice(b"\xff.bin\0");
    let tree = git_input(&foreign, &["mktree", "-z"], &tree);
    let pin = git(
        &foreign,
        &[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "pin with unrelated byte path",
        ],
    );
    fs::write(
        foreign.join("pages/guide.md"),
        "# Guide\nSecond revision.\n",
    )
    .unwrap();
    commit(&foreign);
    fs::write(
        foreign.join("pages/guide.md"),
        "# Guide\nDirty workspace.\n",
    )
    .unwrap();
    let before = git(&foreign, &["status", "--porcelain=v1"]);
    let head = git(&foreign, &["rev-parse", "HEAD"]);
    config(&host, "other", None);
    let live = query(
        &host,
        "? *content{handle: \"other:pages/guide.md\", text: text}.",
    );
    assert!(
        serde_json::to_string(&live)
            .unwrap()
            .contains("Dirty workspace")
    );
    let ids_live = query(&host, "? *handle{id: id}.");
    config(&host, "other", Some(&pin[..12]));
    let pinned = query(
        &host,
        "? *content{handle: \"other:pages/guide.md\", text: text}.",
    );
    assert!(
        serde_json::to_string(&pinned)
            .unwrap()
            .contains("First revision")
    );
    assert_eq!(ids_live, query(&host, "? *handle{id: id}."));
    assert_eq!(query(&host, "? *meta{handle: \"other:pages/guide.md\", key: \"md.mount.skipped_non_utf8_paths\", value: \"1\"}.").len(), 1);
    let origin = query(
        &host,
        "? *handle{id: \"other:pages/guide.md\", origin_uri: uri}.",
    );
    assert!(serde_json::to_string(&origin).unwrap().contains(&pin));
    assert_eq!(head, git(&foreign, &["rev-parse", "HEAD"]));
    assert_eq!(before, git(&foreign, &["status", "--porcelain=v1"]));
    assert_eq!(
        fs::read_to_string(foreign.join("pages/guide.md")).unwrap(),
        "# Guide\nDirty workspace.\n"
    );
    assert!(query(&host, "? git_mtime(\"other:pages/guide.md\", instant).").is_empty());
    config(&host, "other", Some("deadbeef"));
    let absent = run(&host, &["--root", host.to_str().unwrap(), "status"]);
    assert!(!absent.status.success());
    assert!(String::from_utf8_lossy(&absent.stderr).contains("absent or ambiguous"));
    config(&host, "other", Some("zzzzzzzz"));
    let change = run(&host, &["--root", host.to_str().unwrap(), "status"]);
    assert!(!change.status.success());
    assert!(String::from_utf8_lossy(&change.stderr).contains("Git commit id"));
}

#[test]
fn reserved_aliases_and_invalid_settings_refuse_before_scanning() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    init(&host);
    fs::create_dir(host.join(".design")).unwrap();
    fs::write(host.join(".design/x.md"), "# X\n").unwrap();
    for alias in [
        "external",
        "asset",
        "file",
        "label",
        "section",
        "version",
        "code",
        "snapshot",
        "https",
        "c",
        "t",
        "m",
        "KB",
        "KB-D",
        "bad:name",
        "cli",
        "ssh",
        "edge",
        "news",
        "tel",
        "git",
        "javascript",
        "HTTP",
    ] {
        config(&host, alias, None);
        let out = run(&host, &["--root", host.to_str().unwrap(), "status"]);
        assert!(!out.status.success(), "alias {alias} accepted");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("alias"),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for setting in [
        "path(\"../foreign\"). revision(\"master\").",
        "path(\"../foreign\"). scan_root(\"../escape\").",
        "path(\"../foreign\"). typo(\"x\").",
        "path(\"../foreign\"). path(\"../elsewhere\").",
    ] {
        fs::write(
            host.join("anneal.dl"),
            format!("source md {{ mount \"other\" {{ {setting} }} }}"),
        )
        .unwrap();
        assert!(
            !run(&host, &["--root", host.to_str().unwrap(), "status"])
                .status
                .success()
        );
    }
}

#[cfg(unix)]
#[test]
fn mounted_scan_root_and_file_symlink_escapes_refuse() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/secret.md"), "# Secret\n").unwrap();
    fs::write(foreign.join("pages/guide.md"), "# Guide\n").unwrap();
    commit(&foreign);
    config(&host, "other", None);
    symlink(
        host.join(".design/secret.md"),
        foreign.join("pages/escape.md"),
    )
    .unwrap();
    let out = run(&host, &["--root", host.to_str().unwrap(), "status"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("outside"));
    fs::remove_file(foreign.join("pages/escape.md")).unwrap();
    fs::remove_dir_all(foreign.join("pages")).unwrap();
    symlink(host.join(".design"), foreign.join("pages")).unwrap();
    let out = run(&host, &["--root", host.to_str().unwrap(), "status"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("escapes"));
    symlink(host.join("anneal.dl"), foreign.join("anneal.dl")).unwrap();
    let out = run(&host, &["--root", host.to_str().unwrap(), "status"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("foreign anneal.dl"));
}

#[test]
fn workspace_mount_probes_its_own_code_and_ignores_foreign_rules_and_policies() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::create_dir(foreign.join("src")).unwrap();
    fs::write(host.join(".design/host.md"), "# Host\n").unwrap();
    fs::write(
        foreign.join("pages/guide.md"),
        "---\nstatus: current\n---\n# Guide\n`src/owned.rs`\n[asset](payload.tsv)\n",
    )
    .unwrap();
    fs::write(foreign.join("pages/payload.tsv"), "key\tvalue\na\tb\n").unwrap();
    fs::write(foreign.join("pages/excluded.md"), "# Excluded\n").unwrap();
    fs::write(foreign.join("src/owned.rs"), "// owned by foreign\n").unwrap();
    fs::write(foreign.join("unselected.md"), "# Unselected\n").unwrap();
    fs::write(foreign.join("anneal.dl"), "source md { scan_exclude([\"pages/excluded.md\"]). mount \"nested\" { path(\"../absent\"). } }\nconfig diagnostics { escalate(\"W006\", \"error\"). }\nforeign_rule(\"must not load\").\n").unwrap();
    let pin = commit(&foreign);
    config(&host, "other", None);
    commit(&host);
    let rows = query(
        &host,
        "? *meta{handle: h, key: \"target_exists\", value: exists}, *meta{handle: h, key: \"external_class\", value: \"code\"}.",
    );
    assert_eq!(rows.len(), 1);
    assert!(serde_json::to_string(&rows).unwrap().contains("true"));
    assert!(query(&host, "? git_mtime(\"other:unselected.md\", instant).").is_empty());
    let handles = query(&host, "? *handle{id: id}.");
    assert!(
        !serde_json::to_string(&handles)
            .unwrap()
            .contains("excluded.md")
    );
    assert!(
        serde_json::to_string(&handles)
            .unwrap()
            .contains("other:external:asset:pages/payload.tsv")
    );
    assert!(
        !run(
            &host,
            &[
                "--root",
                host.to_str().unwrap(),
                "-e",
                "? foreign_rule(value)."
            ]
        )
        .status
        .success()
    );
    assert!(!query(&host, "? *meta{handle: \"other:pages/guide.md\", key: \"md.mount.config_ignored\", value: value}.").is_empty());
    assert_eq!(query(&host, "? mount_repository_operation_capability(\"other:pages/guide.md\", operation, available, reason).").len(), 5);
    fs::remove_file(foreign.join("src/owned.rs")).unwrap();
    let rows = query(
        &host,
        "? *meta{handle: h, key: \"target_exists\", value: exists}, *meta{handle: h, key: \"external_class\", value: \"code\"}.",
    );
    assert_eq!(rows.len(), 1);
    assert!(serde_json::to_string(&rows).unwrap().contains("false"));
    config(&host, "other", Some(&pin));
    let rows = query(
        &host,
        "? *meta{handle: h, key: \"target_exists\", value: exists}, *meta{handle: h, key: \"external_class\", value: \"code\"}.",
    );
    assert_eq!(rows.len(), 1);
    assert!(serde_json::to_string(&rows).unwrap().contains("true"));
    let metadata = query(&host, "? *meta{key: key, value: value}.");
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("anneal-mount-"),
        "temporary paths leaked into pinned metadata"
    );
}

fn jj(root: &Path, args: &[&str]) -> String {
    let out = Command::new("jj")
        .current_dir(root)
        .args([
            "--config",
            "git.colocate=false",
            "--config",
            "user.name='Fixture'",
            "--config",
            "user.email='fixture@example.test'",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn noncolocated_jj_mount_is_read_without_snapshotting_or_using_host_history() {
    if !Command::new("jj")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
    {
        assert!(
            std::env::var_os("ANNEAL_REQUIRE_JJ").is_none(),
            "jj is required"
        );
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let anchor = temp.path().join("anchor");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&anchor);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(anchor.join("pages")).unwrap();
    fs::write(host.join(".design/host.md"), "# Host\n").unwrap();
    fs::write(anchor.join("pages/guide.md"), "# Guide\nCommitted body.\n").unwrap();
    let pin = commit(&anchor);
    jj(&anchor, &["git", "init", "--colocate"]);
    jj(&anchor, &["workspace", "add", foreign.to_str().unwrap()]);
    assert!(!foreign.join(".git").exists());
    // Snapshot once before the read-only window. Subsequent reads must not advance operations.
    jj(&foreign, &["status"]);
    let op = jj(
        &foreign,
        &[
            "--ignore-working-copy",
            "--at-operation=@",
            "op",
            "log",
            "--limit",
            "1",
            "--no-graph",
            "-T",
            "id",
        ],
    );
    fs::write(
        foreign.join("pages/guide.md"),
        "# Guide\nUnsnapshotted body.\n",
    )
    .unwrap();
    config(&host, "other", None);
    let live = query(
        &host,
        "? *content{handle: \"other:pages/guide.md\", text: text}.",
    );
    assert!(
        serde_json::to_string(&live)
            .unwrap()
            .contains("Unsnapshotted body")
    );
    let capabilities = query(
        &host,
        "? mount_repository_operation_capability(\"other:pages/guide.md\", \"assertion_blame\", available, reason).",
    );
    assert!(
        serde_json::to_string(&capabilities)
            .unwrap()
            .contains("jj-assertion-blame-not-defined")
    );
    config(&host, "other", Some(&pin));
    let pinned = query(
        &host,
        "? *content{handle: \"other:pages/guide.md\", text: text}.",
    );
    assert!(
        serde_json::to_string(&pinned)
            .unwrap()
            .contains("Committed body")
    );
    assert_eq!(
        op,
        jj(
            &foreign,
            &[
                "--ignore-working-copy",
                "--at-operation=@",
                "op",
                "log",
                "--limit",
                "1",
                "--no-graph",
                "-T",
                "id"
            ]
        )
    );
    assert!(
        fs::read_to_string(foreign.join("pages/guide.md"))
            .unwrap()
            .contains("Unsnapshotted body")
    );
}

#[test]
fn mount_paths_are_based_on_project_boundary_when_config_is_inside_design() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/host.md"), "# Host\n").unwrap();
    fs::write(foreign.join("pages/guide.md"), "# Foreign\n").unwrap();
    commit(&foreign);
    fs::write(host.join(".design/anneal.dl"), "source md { scan_root(\".\"). mount \"other\" { path(\"../foreign\"). scan_root(\"pages\"). } }\n").unwrap();
    assert_eq!(
        query(
            &host.join(".design"),
            "? *handle{id: \"other:pages/guide.md\"}."
        )
        .len(),
        1
    );
}

#[test]
fn multiple_mounts_preserve_declared_foreign_links_and_scoped_duplicate_labels() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(
        host.join(".design/host.md"),
        "# Host\n[one](one:pages/guide.md)\n[two](two:pages/guide.md)\n",
    )
    .unwrap();
    for (name, text) in [
        ("guide.md", "# OQ-1\n[other](two:pages/guide.md)\n"),
        ("second.md", "# OQ-2\nOQ-3\n"),
    ] {
        fs::write(foreign.join("pages").join(name), text).unwrap();
    }
    commit(&foreign);
    fs::write(host.join("anneal.dl"), "source md { scan_root(\".design\"). mount \"one\" { path(\"../foreign\"). scan_root(\"pages\"). } } source md { mount \"two\" { path(\"../foreign\"). scan_root(\"pages\"). } }\n").unwrap();
    let handles = query(&host, "? *handle{id: id}.");
    let text = serde_json::to_string(&handles).unwrap();
    for id in [
        "one:pages/guide.md",
        "two:pages/guide.md",
        "one:OQ-1",
        "two:OQ-1",
    ] {
        assert!(text.contains(id), "{id}: {text}");
    }
    assert!(
        query(
            &host,
            "? diagnostic(\"E001\", severity, subject, file, line, evidence)."
        )
        .is_empty()
    );
    let edges = query(
        &host,
        "? *edge{from: \"one:pages/guide.md\", to: \"two:pages/guide.md\"}.",
    );
    assert_eq!(edges.len(), 1);
}

#[cfg(unix)]
#[test]
fn named_assets_use_repository_window_and_exact_pin_existence() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/host.md"), "# Host\n[data](other:payload.tsv) [same](other:./payload.tsv)\n[excluded](other:pages/excluded.md)\n[later](other:later.csv)\n[escape](other:escape.csv)\n").unwrap();
    fs::write(host.join("secret.csv"), "private\n").unwrap();
    fs::write(foreign.join("pages/guide.md"), "# Guide\n").unwrap();
    fs::write(
        foreign.join("pages/excluded.md"),
        "---\nstatus: current\n---\n# Opaque\nMust stay opaque.\n",
    )
    .unwrap();
    fs::write(foreign.join("payload.tsv"), "a\tb\n").unwrap();
    fs::write(
        foreign.join("anneal.dl"),
        "source md { scan_exclude(\"pages/excluded.md\"). }\n",
    )
    .unwrap();
    let pin = commit(&foreign);
    fs::write(foreign.join("later.csv"), "added after pin\n").unwrap();
    symlink(host.join("secret.csv"), foreign.join("escape.csv")).unwrap();
    config(&host, "other", None);
    let live = query(
        &host,
        "? *handle{id: id, kind: \"external\", file: file, status: status}.",
    );
    let text = serde_json::to_string(&live).unwrap();
    for path in ["payload.tsv", "pages/excluded.md", "later.csv"] {
        assert!(
            text.contains(&format!("other:external:asset:{path}")),
            "{text}"
        );
    }
    assert!(!text.contains("escape.csv"));
    assert_eq!(
        query(
            &host,
            "? *edge{to: \"other:external:asset:payload.tsv\", native_id: id}."
        )
        .len(),
        2,
        "repeated synonymous references must keep distinct ids"
    );
    assert!(
        query(
            &host,
            "? *content{handle: \"other:external:asset:pages/excluded.md\", text: text}."
        )
        .is_empty()
    );
    assert_eq!(
        query(
            &host,
            "? diagnostic(\"E001\", severity, subject, file, line, evidence)."
        )
        .len(),
        1
    );
    assert!(!query(&host, "? git_mtime(\"other:payload.tsv\", instant).").is_empty());
    config(&host, "other", Some(&pin));
    let pinned = query(
        &host,
        "? *handle{id: id, kind: \"external\", file: file, status: status}.",
    );
    let text = serde_json::to_string(&pinned).unwrap();
    for path in ["payload.tsv", "pages/excluded.md"] {
        assert!(
            text.contains(&format!("other:external:asset:{path}")),
            "{text}"
        );
    }
    assert!(!text.contains("later.csv"));
    assert!(!text.contains("escape.csv"));
    let missing = query(
        &host,
        "? diagnostic(\"E001\", severity, subject, file, line, evidence).",
    );
    assert_eq!(missing.len(), 2);
    assert!(
        serde_json::to_string(&missing)
            .unwrap()
            .contains("other:later.csv")
    );
    let origin = query(
        &host,
        "? *handle{id: \"other:external:asset:payload.tsv\", origin_uri: uri}.",
    );
    assert!(serde_json::to_string(&origin).unwrap().contains(&pin));
}

#[test]
fn mounted_selection_preserves_native_subdirectory_corpus_keys_and_config() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir_all(foreign.join(".design/formal-model")).unwrap();
    fs::create_dir_all(foreign.join("crates/fm")).unwrap();
    fs::write(
        foreign.join(".design/anneal.dl"),
        "source md { scan_root(\".\"). external_root(\"../crates/fm\"). }\n",
    )
    .unwrap();
    fs::write(foreign.join(".design/formal-model/norms.md"), "# Norms\n").unwrap();
    fs::write(
        foreign.join("crates/fm/atoms.md"),
        "# Atoms\n[corpus](formal-model/norms.md)\n",
    )
    .unwrap();
    fs::write(
        host.join(".design/a.md"),
        "# Host\n[atoms](other:crates/fm/atoms.md)\n",
    )
    .unwrap();
    let pin = commit(&foreign);
    let native = query(&foreign.join(".design"), "? *handle{id: id}.");
    assert!(
        native
            .iter()
            .any(|row| row["id"] == "formal-model/norms.md")
    );
    assert!(native.iter().any(|row| row["id"] == "crates/fm/atoms.md"));
    for revision in [None, Some(pin.as_str())] {
        fs::write(host.join("anneal.dl"), format!("source md {{ scan_root(\".design\"). mount \"other\" {{ path(\"../foreign\"). scan_root(\"crates/fm\"). {} }} }}\n", revision.map_or_else(String::new, |r| format!("revision(\"{r}\").")))).unwrap();
        let handles = query(&host, "? *handle{id: id, file: file}.");
        assert!(handles.iter().any(|row| row["id"]
            == "other:external:asset:formal-model/norms.md"
            && row["file"] == "other:formal-model/norms.md"));
        assert!(
            query(
                &host,
                "? diagnostic(code, severity, subject, file, line, evidence), code = \"E001\"."
            )
            .is_empty()
        );
        assert!(
            !query(
                &host,
                "? *meta{key: \"md.mount.config_ignored\", value: value}."
            )
            .is_empty()
        );
    }
    // Omitted selection preserves the owning corpus's primary and sibling roots.
    for revision in [None, Some(pin.as_str())] {
        fs::write(host.join("anneal.dl"), format!("source md {{ scan_root(\".design\"). mount \"other\" {{ path(\"../foreign\"). {} }} }}\n", revision.map_or_else(String::new, |r| format!("revision(\"{r}\").")))).unwrap();
        let handles = query(&host, "? *handle{id: id}.");
        assert!(
            handles
                .iter()
                .any(|row| row["id"] == "other:formal-model/norms.md")
        );
        assert!(
            handles
                .iter()
                .any(|row| row["id"] == "other:crates/fm/atoms.md")
        );
    }
    // Internal selections keep corpus-relative IDs; the selection itself is repository-relative.
    fs::write(host.join("anneal.dl"), "source md { scan_root(\".design\"). mount \"other\" { path(\"../foreign\"). scan_root(\".design/formal-model\"). } }\n").unwrap();
    assert!(
        query(&host, "? *handle{id: id}.")
            .iter()
            .any(|row| row["id"] == "other:formal-model/norms.md")
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(foreign.join("crates/fm"), foreign.join("fm-alias")).unwrap();
        fs::write(host.join("anneal.dl"), "source md { scan_root(\".design\"). mount \"other\" { path(\"../foreign\"). scan_root(\"fm-alias\"). } }\n").unwrap();
        assert!(
            query(&host, "? *handle{id: id}.")
                .iter()
                .any(|row| row["id"] == "other:crates/fm/atoms.md")
        );
    }
    // Native sibling-root overlap refusals still apply when discovery supplies the selection.
    fs::write(
        foreign.join(".design/anneal.dl"),
        "source md { external_root(\"..\"). }\n",
    )
    .unwrap();
    let bad_pin = commit(&foreign);
    for revision in [None, Some(bad_pin.as_str())] {
        fs::write(host.join("anneal.dl"), format!("source md {{ scan_root(\".design\"). mount \"other\" {{ path(\"../foreign\"). {} }} }}\n", revision.map_or_else(String::new, |r| format!("revision(\"{r}\").")))).unwrap();
        let out = run(
            &host,
            &["--root", host.to_str().unwrap(), "-e", "? *handle{id: id}."],
        );
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("overlaps the corpus root"));
    }
}

#[test]
fn mounted_section_placeholders_keep_their_native_non_identity_meaning() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir(foreign.join("pages")).unwrap();
    fs::write(host.join(".design/a.md"), "# Host\n").unwrap();
    fs::write(
        foreign.join("pages/a.md"),
        "# Foreign\nSee §13.1 and §4.2.\n",
    )
    .unwrap();
    let pin = commit(&foreign);
    for revision in [None, Some(pin.as_str())] {
        config(&host, "other", revision);
        let edges = query(
            &host,
            "? *edge{to: target}, target starts_with \"section:\".",
        );
        assert_eq!(edges.len(), 2);
        assert!(
            query(
                &host,
                "? diagnostic(code, severity, subject, file, line, evidence), code = \"E001\"."
            )
            .is_empty()
        );
    }
}

#[test]
fn pinned_code_targets_use_repository_then_native_corpus_coordinates() {
    let temp = tempfile::tempdir().unwrap();
    let host = temp.path().join("host");
    let foreign = temp.path().join("foreign");
    init(&host);
    init(&foreign);
    fs::create_dir(host.join(".design")).unwrap();
    fs::create_dir_all(foreign.join(".design/src")).unwrap();
    fs::create_dir(foreign.join("src")).unwrap();
    fs::write(host.join(".design/a.md"), "# Host\n").unwrap();
    fs::write(
        foreign.join(".design/anneal.dl"),
        "source md { scan_root(\".\"). }\n",
    )
    .unwrap();
    fs::write(
        foreign.join(".design/a.md"),
        "# Document\nSee src/owned.rs and src/corpus_only.rs.\n",
    )
    .unwrap();
    fs::write(foreign.join("src/owned.rs"), "// project source\n").unwrap();
    fs::write(
        foreign.join(".design/src/owned.rs"),
        "// distinct corpus source\n",
    )
    .unwrap();
    fs::write(
        foreign.join(".design/src/corpus_only.rs"),
        "// corpus fallback\n",
    )
    .unwrap();
    let pin = commit(&foreign);
    fs::write(host.join("anneal.dl"), format!("source md {{ scan_root(\".design\"). mount \"other\" {{ path(\"../foreign\"). scan_root(\".design\"). revision(\"{pin}\"). }} }}\n")).unwrap();
    let meta = query(
        &host,
        "? *meta{handle: h, key: \"target_resolved_path\", value: value}.",
    );
    let owned = meta
        .iter()
        .find(|row| row["h"].as_str().unwrap().ends_with(":src/owned.rs"))
        .unwrap();
    assert!(
        owned["value"].as_str().unwrap().ends_with("#src/owned.rs"),
        "{owned}"
    );
    let corpus = meta
        .iter()
        .find(|row| row["h"].as_str().unwrap().ends_with(":src/corpus_only.rs"))
        .unwrap();
    assert!(
        corpus["value"]
            .as_str()
            .unwrap()
            .ends_with("#.design/src/corpus_only.rs"),
        "{corpus}"
    );
    assert!(
        !serde_json::to_string(&meta)
            .unwrap()
            .contains("anneal-mount-")
    );
}
