use std::collections::BTreeMap;
use std::fs;

use anneal_core::runtime::Value;
use anneal_core::runtime::{ExplainOptions, NumberValue};
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::{OutputMode, RuntimeCommand};
use crate::app::output::test_support::row;
use crate::app::output::{CommandOutput, RankedAnchorEnrichment, RowView};
use crate::app::session::RuntimeSession;

#[test]
fn generic_rows_human_render_is_readable() {
    let output = CommandOutput::rows(
        vec![row(&[
            ("category", Value::String("status".to_string())),
            ("value", Value::String("open question".to_string())),
            ("count", Value::Number(NumberValue::Int(2))),
        ])],
        RowView::Eval,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render rows");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.starts_with("Results (1)\n 1."));
    assert!(rendered.contains("category=status"));
    assert!(rendered.contains(r#"value="open question""#));
    assert!(rendered.contains("count=2"));
}

#[test]
fn eval_empty_binding_hint_uses_query_schema() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "---\ndepends-on: missing.md\n---\n# A\n").expect("write file");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? diagnostic{severity: "error"}."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows {
        rows,
        empty_binding_hint,
        ..
    } = output
    else {
        panic!("eval should emit rows");
    };

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.fields.is_empty()));
    assert_eq!(
        empty_binding_hint,
        Some(r#"? diagnostic{severity: "error", code: code}."#.to_string())
    );

    assert_eq!(
        session.empty_binding_hint_for_query(r#"? examples("diagnostic", "noop")."#, &rows),
        None
    );
}

#[test]
fn read_human_render_shows_content_blocks() {
    let output = CommandOutput::rows(
        vec![row(&[
            ("span_id", Value::String("plan.md#full".to_string())),
            ("start_line", Value::Number(NumberValue::Int(10))),
            ("end_line", Value::Number(NumberValue::Int(12))),
            ("tokens", Value::Number(NumberValue::Int(8))),
            (
                "text",
                Value::String("Release blocker details.\nNext line.".to_string()),
            ),
        ])],
        RowView::Read {
            missing_handle: None,
        },
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render read");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.starts_with("Read (1)\n 1. plan.md#full  lines=10-12  tokens=8"));
    assert!(rendered.contains("\n  Release blocker details.\n  Next line.\n"));
    assert!(!rendered.contains("text="));
}

#[test]
fn read_human_render_hints_when_span_is_truncated() {
    let output = CommandOutput::rows(
        vec![row(&[
            ("span_id", Value::String("plan.md#h/long".to_string())),
            ("start_line", Value::Number(NumberValue::Int(10))),
            ("end_line", Value::Number(NumberValue::Int(40))),
            ("tokens", Value::Number(NumberValue::Int(12))),
            ("total_tokens", Value::Number(NumberValue::Int(80))),
            (
                "text",
                Value::String("Release blocker details.".to_string()),
            ),
        ])],
        RowView::Read {
            missing_handle: None,
        },
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render read");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("showing first 12 tokens of span (80 total)"));
    assert!(rendered.contains("use --budget 80"));
}

#[test]
fn describe_human_render_shows_all_doc_cards() {
    let output = CommandOutput::rows(
        vec![
            row(&[(
                "doc",
                Value::String("Search primitive internals.\nKind: engine primitive.".to_string()),
            )]),
            row(&[(
                "doc",
                Value::String("Search command surface.\nKind: verb.".to_string()),
            )]),
        ],
        RowView::Describe,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render describe");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert_eq!(
        rendered,
        "Search primitive internals.\nKind: engine primitive.\n\nSearch command surface.\nKind: verb.\n"
    );
}

#[test]
fn describe_auto_json_mode_still_renders_teaching_cards() {
    let output = CommandOutput::rows(
        vec![row(&[(
            "doc",
            Value::String("Search command surface.\nKind: verb.".to_string()),
        )])],
        RowView::Describe,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Json)
        .expect("render describe");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert_eq!(rendered, "Search command surface.\nKind: verb.\n");
}

#[test]
fn describe_explicit_json_preserves_ndjson() {
    let output = CommandOutput::rows(
        vec![row(&[(
            "doc",
            Value::String("Search command surface.\nKind: verb.".to_string()),
        )])],
        RowView::Describe,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::JsonExplicit)
        .expect("render describe");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert_eq!(
        rendered,
        "{\"doc\":\"Search command surface.\\nKind: verb.\"}\n"
    );
}

#[test]
fn search_json_rounds_score_precision() {
    let output = CommandOutput::rows(
        vec![row(&[
            ("h", Value::String("plan.md".to_string())),
            (
                "score",
                Value::Number(NumberValue::Float(0.989_999_949_932_098_4)),
            ),
        ])],
        RowView::Search,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::JsonExplicit)
        .expect("render search json");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert_eq!(rendered, "{\"h\":\"plan.md\",\"score\":0.99}\n");
}

#[test]
fn eval_json_preserves_non_search_score_precision() {
    let output = CommandOutput::rows(
        vec![row(&[(
            "score",
            Value::Number(NumberValue::Float(0.989_999_949_932_098_4)),
        )])],
        RowView::Eval,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::JsonExplicit)
        .expect("render eval json");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert_eq!(rendered, "{\"score\":0.9899999499320984}\n");
}

#[test]
fn ranked_anchor_json_adds_ordered_signal_set() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("README.md"),
        "---\nstatus: current\n---\n# Project\n\nDurable overview.\n",
    )
    .expect("write readme");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: "? ranked_anchor(handle, r, s, w).".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("ranked_anchor eval runs");

    let mut rendered = Vec::new();
    output
        .write(&mut rendered, OutputMode::JsonExplicit)
        .expect("render ranked_anchor json");
    let rendered = String::from_utf8(rendered).expect("utf8");
    let rows = rendered
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json row"))
        .collect::<Vec<_>>();
    let readme = rows
        .iter()
        .find(|row| row["handle"] == "README.md")
        .expect("README anchor row");

    assert_eq!(readme["r"], 1);
    assert_eq!(readme["s"], 215);
    assert_eq!(readme["w"], "authoritative_status");
    assert_eq!(
        readme["signals"],
        serde_json::json!([
            {"why": "authoritative_status", "score": 150},
            {"why": "curated_name", "score": 65}
        ])
    );
}

#[test]
fn ranked_anchor_text_teaches_signal_drill_down() {
    let output = CommandOutput::rows_with_ranked_anchor_enrichment(
        vec![row(&[
            ("handle", Value::String("README.md".to_string())),
            ("r", Value::Number(NumberValue::Int(1))),
            ("s", Value::Number(NumberValue::Int(215))),
            ("w", Value::String("authoritative_status".to_string())),
        ])],
        RowView::RankedAnchor {
            handle_field: "handle".to_string(),
        },
        None,
        Vec::new(),
        Some(RankedAnchorEnrichment {
            handle_field: "handle".to_string(),
            signals_by_handle: BTreeMap::new(),
        }),
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render ranked_anchor text");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("handle=README.md r=1 s=215 w=authoritative_status"));
    assert!(rendered.contains("Follow-up: anneal -e '? anchor_signal(h, s, prio, why).'"));
}
