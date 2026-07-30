use std::fs;

use anneal_core::runtime::NumberValue;
use anneal_core::runtime::Value;
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::DEFAULT_READ_BUDGET;
use crate::app::command::{OutputMode, RuntimeCommand};
use crate::app::output::EMPTY_ROWS_DIAGNOSTIC;
use crate::app::output::handle::write_handle_text;
use crate::app::output::test_support::row;
use crate::app::session::RuntimeSession;

#[test]
fn handle_recovers_retired_section_handle_shape() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let Err(error) = session.run(RuntimeCommand::Handle {
        handle: "a.md#A".to_string(),
        impact: false,
        lineage: false,
    }) else {
        panic!("retired section handle should recover");
    };
    let message = error.to_string();

    assert!(message.contains("section handles were retired in v0.14"));
    assert!(message.contains(r#"? *span{handle: "a.md""#));
}

#[test]
fn missing_handle_surfaces_teach_recovery_without_mislabeling_empty_reads() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let hint = "hint: handle \"missing.md\" not found; try `anneal search \"missing.md\"` or `anneal status`";

    let output = session
        .run(RuntimeCommand::Handle {
            handle: "missing.md".to_string(),
            impact: false,
            lineage: false,
        })
        .expect("missing handle remains guidance, not failure");
    assert_eq!(output.stderr_diagnostic(OutputMode::Human), None);
    assert_eq!(
        output.stderr_diagnostic(OutputMode::Json).as_deref(),
        Some(format!("{hint}\n{EMPTY_ROWS_DIAGNOSTIC}").as_str())
    );
    let mut rendered = Vec::new();
    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render missing handle");
    assert_eq!(
        String::from_utf8(rendered).expect("utf8"),
        format!("Handle missing.md (0 edges)\n{EMPTY_ROWS_DIAGNOSTIC}\n{hint}\n")
    );

    let output = session
        .run(RuntimeCommand::Read {
            handle: "missing.md".to_string(),
            budget: DEFAULT_READ_BUDGET,
            span_id: None,
        })
        .expect("missing read remains guidance, not failure");
    assert_eq!(output.stderr_diagnostic(OutputMode::Human), None);
    assert_eq!(
        output.stderr_diagnostic(OutputMode::Json).as_deref(),
        Some(format!("{hint}\n{EMPTY_ROWS_DIAGNOSTIC}").as_str())
    );
    let mut rendered = Vec::new();
    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render missing read");
    assert_eq!(
        String::from_utf8(rendered).expect("utf8"),
        format!("Read (0)\n{EMPTY_ROWS_DIAGNOSTIC}\n{hint}\n")
    );

    let output = session
        .run(RuntimeCommand::Read {
            handle: "a.md".to_string(),
            budget: 0,
            span_id: None,
        })
        .expect("known handle with empty read runs");
    assert_eq!(
        output.stderr_diagnostic(OutputMode::Json),
        Some(EMPTY_ROWS_DIAGNOSTIC.to_string())
    );
    let mut rendered = Vec::new();
    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render known empty read");
    assert_eq!(
        String::from_utf8(rendered).expect("utf8"),
        format!("Read (0)\n{EMPTY_ROWS_DIAGNOSTIC}\n")
    );
}

#[test]
fn handle_human_render_groups_edges_and_code_refs() {
    let rows = vec![
        row(&[
            ("h", Value::String("doc.md".to_string())),
            ("relation", Value::String("self".to_string())),
            ("other", Value::String("doc.md".to_string())),
            ("kind", Value::String("file".to_string())),
            ("status", Value::String("draft".to_string())),
            ("file", Value::String("doc.md".to_string())),
            ("line", Value::Number(NumberValue::Int(1))),
            ("summary", Value::String(String::new())),
        ]),
        row(&[
            ("h", Value::String("doc.md".to_string())),
            ("relation", Value::String("out".to_string())),
            ("other", Value::String("plan.md".to_string())),
            ("kind", Value::String("DependsOn".to_string())),
            ("status", Value::Null),
            ("file", Value::String("doc.md".to_string())),
            ("line", Value::Number(NumberValue::Int(4))),
            ("summary", Value::String(String::new())),
        ]),
        row(&[
            ("h", Value::String("doc.md".to_string())),
            ("relation", Value::String("code_ref".to_string())),
            (
                "other",
                Value::String("lib/example/admission.rs:142-167".to_string()),
            ),
            ("kind", Value::String("Cites".to_string())),
            ("status", Value::Null),
            ("file", Value::String("doc.md".to_string())),
            ("line", Value::Number(NumberValue::Int(8))),
            (
                "summary",
                Value::String("lib/example/admission.rs".to_string()),
            ),
        ]),
    ];
    let mut rendered = Vec::new();

    write_handle_text(&mut rendered, "doc.md", false, false, false, &rows).expect("render handle");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Outgoing\nDependsOn (1)"));
    assert!(rendered.contains(" 1. -> plan.md  at=doc.md:4"));
    assert!(rendered.contains("Code references (1)"));
    assert!(rendered.contains(" 1. lib/example/admission.rs  at=doc.md:8"));
    assert!(rendered.contains("drift evidence not built; run `anneal check --refresh-drift`"));
    assert!(
        rendered.contains("follow-up: anneal -e '? assertion_drift(\"doc.md\", target, commits).'")
    );
}

#[test]
fn handle_human_render_annotates_code_ref_drift() {
    let rows = vec![
        row(&[
            ("h", Value::String("doc.md".to_string())),
            ("relation", Value::String("self".to_string())),
            ("other", Value::String("doc.md".to_string())),
            ("kind", Value::String("file".to_string())),
            ("status", Value::String("draft".to_string())),
            ("file", Value::String("doc.md".to_string())),
            ("line", Value::Number(NumberValue::Int(1))),
            ("summary", Value::String(String::new())),
        ]),
        row(&[
            ("h", Value::String("doc.md".to_string())),
            ("relation", Value::String("code_ref".to_string())),
            (
                "other",
                Value::String("external:code:doc.md:8:src/cli.rs".to_string()),
            ),
            ("kind", Value::String("Cites".to_string())),
            ("status", Value::Null),
            ("file", Value::String("doc.md".to_string())),
            ("line", Value::Number(NumberValue::Int(8))),
            ("summary", Value::String("src/cli.rs".to_string())),
            (
                "disposition",
                Value::String("referent-moved-ambiguous".to_string()),
            ),
            ("candidate_count", Value::String("11".to_string())),
            ("moved_to", Value::Null),
        ]),
    ];
    let mut rendered = Vec::new();

    write_handle_text(&mut rendered, "doc.md", false, false, false, &rows).expect("render handle");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(
        rendered.contains("src/cli.rs  [referent-moved-ambiguous · 11 candidates]  at=doc.md:8")
    );
    assert!(!rendered.contains("drift evidence not built"));
}
