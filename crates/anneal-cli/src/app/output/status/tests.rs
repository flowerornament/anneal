use std::collections::BTreeMap;
use std::fs;

use anneal_core::runtime::ExplainOptions;
use anneal_core::runtime::{NumberValue, Row, Value};
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::{OutputMode, RuntimeCommand};
use crate::app::output::CommandOutput;
use crate::app::output::test_support::{
    row, status_metric, status_metric_counts, status_output, status_output_with_baseline,
};
use crate::app::output::value::required_string;
use crate::app::session::RuntimeSession;

#[test]
fn status_human_render_shows_aggregate_dashboard_and_pointers() {
    let output = status_output(vec![
        status_metric("scale", "handles", 10),
        status_metric("scale", "file_handles", 8),
        status_metric("scale", "file_handles_with_status", 2),
        status_metric("scale", "statusless_file_handles", 6),
        status_metric("convergence", "broken", 1),
        status_metric("convergence", "blocked", 2),
        status_metric("convergence", "open", 3),
        status_metric("convergence", "advancing", 4),
        status_metric("convergence", "holding", 5),
        status_metric("convergence", "drifting", 6),
        status_metric("health", "errors", 1),
        status_metric("health", "blockers", 2),
        status_metric("health", "spec_code_drift", 1),
        status_metric("diagnostics", "total", 20),
        status_metric("diagnostics", "error", 1),
        status_metric("diagnostics", "warning", 17),
        status_metric("diagnostics", "suggestion", 1),
        status_metric("diagnostics", "info", 1),
        status_metric("vocabulary", "description", 21),
        status_metric("vocabulary", "author", 13),
        status_metric("vocabulary", "authors", 30),
        status_metric("drift", "cold", 3),
    ]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.starts_with("Status\n"));
    assert!(rendered.contains("Scale        10 handles, 8 files, 25% lifecycle coverage"));
    assert!(rendered.contains(
        "Coverage     25% of file handles carry lifecycle status; orientation is graph+recency-led"
    ));
    assert!(
        rendered.contains(
            "Convergence  broken=1, blocked=2, open=3, advancing=4, holding=5, drifting=6"
        )
    );
    assert!(
        rendered.contains(
            "Health       errors=1, blockers=2, spec_code_drift=1 distinct source handles"
        )
    );
    assert!(rendered.contains("Diagnostics  20 total, 1 error, 17 warning, 1 suggestion, 1 info"));
    assert!(rendered.contains(
        "Vocabulary   top 3 unmodeled authored keys by distinct file handles: authors 30, description 21, author 13; query `unmodeled_frontmatter_key`"
    ));
    assert!(rendered.contains(
        "Code refs    drift evidence not built for 3 refs; run `anneal check --refresh-drift`"
    ));
    assert!(rendered.contains("Read first"));
    assert!(rendered.contains("recent_frontier(h, rank, recency)"));
    assert!(rendered.contains(
        "? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc."
    ));
    assert!(rendered.contains("ranked_anchor(h, rank, score, why)"));
    assert!(rendered.contains(
        "? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc."
    ));
    assert!(rendered.contains("follow-up: anneal -e '? anchor_signal(h, s, prio, why).'"));
    assert!(rendered.contains("Work"));
    assert!(rendered.contains("diagnostic{code: code, severity: severity"));
    assert!(!rendered.contains("bad.md"));
    assert_dashboard_summary_separator_contract(&rendered);
}

#[test]
fn status_human_render_omits_vocabulary_line_when_no_unmodeled_keys_exist() {
    let output = status_output(vec![
        status_metric("scale", "handles", 1),
        status_metric("scale", "file_handles", 1),
        status_metric("scale", "file_handles_with_status", 1),
        status_metric("scale", "statusless_file_handles", 0),
    ]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(!rendered.contains("Vocabulary"));
}

#[test]
fn status_human_render_preserves_vocabulary_signal_tie_break() {
    let output = status_output(vec![
        status_metric("vocabulary", "notes", 1),
        row(&[
            ("category", Value::String("vocabulary".to_string())),
            ("name", Value::String("source-link".to_string())),
            ("count", Value::Number(NumberValue::Int(1))),
            ("detail", Value::String("reference_name_signal".to_string())),
        ]),
    ]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("source-link 1, notes 1"));
}

#[test]
fn status_human_render_marks_flow_pending_without_snapshot_baseline() {
    let output = status_output_with_baseline(
        vec![
            status_metric("scale", "handles", 1),
            status_metric("scale", "file_handles", 1),
            status_metric("scale", "file_handles_with_status", 1),
            status_metric("scale", "statusless_file_handles", 0),
            status_metric("convergence", "broken", 0),
            status_metric("convergence", "blocked", 0),
            status_metric("convergence", "open", 1),
        ],
        false,
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(
        rendered.contains(
            "Convergence  broken=0, blocked=0, open=1, advancing=-, holding=-, drifting=-"
        )
    );
    assert!(rendered.contains("Note: flow signals empty until snapshot baseline accumulates."));
    assert!(rendered.contains("Run `anneal status` again to populate."));
    assert_dashboard_summary_separator_contract(&rendered);
}

#[test]
fn status_human_render_orders_pipeline_rows_by_status_name() {
    let output = status_output(vec![
        status_metric("scale", "handles", 1),
        status_metric("scale", "file_handles", 1),
        status_metric("scale", "file_handles_with_status", 1),
        status_metric("scale", "statusless_file_handles", 0),
        status_metric("pipeline", "stable", 2),
        status_metric("pipeline", "draft", 3),
    ]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(
        rendered.contains("Pipeline     draft 3, stable 2"),
        "pipeline should render deterministically:\n{rendered}"
    );
    assert_dashboard_summary_separator_contract(&rendered);
}

#[test]
fn status_human_render_splits_warm_code_reference_tally() {
    let output = status_output(vec![
        status_metric("drift", "intact", 1),
        status_metric("drift", "drifted", 2),
        status_metric("drift", "moved", 3),
        status_metric("drift", "moved_ambiguous", 4),
        status_metric("drift", "gone", 5),
        status_metric("drift", "unknown", 6),
        status_metric("drift", "dirty", 7),
        status_metric("drift", "cold", 8),
    ]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Code refs    1 intact, 2 drifted, 3 moved, 4 moved?, 5 gone\n"));
    assert!(
        rendered.contains(
            "             6 unknown, 7 dirty, 8 cold (run `anneal check --refresh-drift`)"
        )
    );
    assert_dashboard_summary_separator_contract(&rendered);
}

#[test]
fn status_renderer_source_contains_no_chunking_separator() {
    let source = include_str!("../status.rs");

    assert!(
        !source.contains('·'),
        "status dashboard renderer must use commas or split overloaded lines"
    );
}

fn assert_dashboard_summary_separator_contract(rendered: &str) {
    const LABELS: [&str; 8] = [
        "Scale",
        "Coverage",
        "Pipeline",
        "Convergence",
        "Health",
        "Diagnostics",
        "Vocabulary",
        "Code refs",
    ];

    for line in rendered.lines().take_while(|line| !line.is_empty()) {
        let content = if LABELS.iter().any(|label| line.starts_with(label))
            || line.starts_with("             ")
        {
            line.get(13..).expect("status label column is ASCII")
        } else {
            continue;
        };
        assert!(
            !content.contains('·') && !content.contains("  "),
            "status dashboard summary must use commas or split overloaded lines: {line}"
        );
    }
}

#[test]
fn status_json_render_preserves_ndjson() {
    let output = status_output(vec![status_metric("convergence", "open", 42)]);
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Json)
        .expect("render status");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.starts_with(
        "{\"category\":\"convergence\",\"count\":42,\"detail\":null,\"name\":\"open\"}\n"
    ));
}

#[test]
fn status_human_render_rejects_schema_drift() {
    let output = status_output(vec![row(&[
        ("section", Value::String("work".to_string())),
        ("h", Value::String("plan.md".to_string())),
        ("why", Value::String("potential".to_string())),
    ])]);
    let mut rendered = Vec::new();

    let error = output
        .write(&mut rendered, OutputMode::Human)
        .expect_err("missing score should fail");

    assert!(error.to_string().contains("status row missing field"));
}

fn status_item_section_counts(rows: &[Row]) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for row in rows {
        let section = required_string(row, "section").expect("status_item row has section");
        *counts.entry(section.to_string()).or_insert(0) += 1;
    }
    counts
}

#[test]
fn status_dashboard_counts_match_status_item_sections() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("a.md"),
        "---\nstatus: draft\n---\n# A\n\nThis cites MISSING-REF.\n",
    )
    .expect("write a");
    fs::write(root.join("b.md"), "---\nstatus: draft\n---\n# B\n").expect("write b");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let status = session.run(RuntimeCommand::Status).expect("status runs");
    let CommandOutput::Status(status) = status else {
        panic!("status should emit status output");
    };
    let metrics = status_metric_counts(&status.rows, "convergence");

    let item_rows = session
        .run(RuntimeCommand::Eval {
            query: "? status_item(section, h, score, why).".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("status_item eval runs");
    let CommandOutput::Rows { rows, .. } = item_rows else {
        panic!("eval should emit rows");
    };
    let section_counts = status_item_section_counts(&rows);

    for (metric, section) in [
        ("broken", "broken"),
        ("blocked", "blocked"),
        ("open", "work"),
        ("advancing", "advancing"),
        ("holding", "holding"),
        ("drifting", "drifting"),
    ] {
        assert_eq!(
            metrics.get(metric).copied().unwrap_or_default(),
            section_counts.get(section).copied().unwrap_or_default(),
            "{metric} dashboard count should match status_item({section})"
        );
    }
}

#[test]
fn status_vocabulary_metrics_select_top_three_without_overlapping_w007() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("a.md"),
        "---\nauthors: [Ada, Grace]\ndescription: A\nsource-link: x.md\nnotes: free\nreferences: x.md\n---\n# A\n",
    )
    .expect("write a");
    fs::write(
        root.join("b.md"),
        "---\nauthors: Ada\ndescription: B\n---\n# B\n",
    )
    .expect("write b");
    fs::write(root.join("c.md"), "---\nauthors: Ada\n---\n# C\n").expect("write c");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let status = session.run(RuntimeCommand::Status).expect("status runs");
    let CommandOutput::Status(status) = status else {
        panic!("status should emit status output");
    };
    let vocabulary = status_metric_counts(&status.rows, "vocabulary");

    assert_eq!(
        vocabulary,
        BTreeMap::from([
            ("authors".to_string(), 3),
            ("description".to_string(), 2),
            ("source-link".to_string(), 1),
        ])
    );
    assert!(
        !vocabulary.contains_key("notes"),
        "the lexical signal may break the equal-count boundary"
    );
    assert!(
        !vocabulary.contains_key("references"),
        "W007 aliases must not double-report in inverse discovery"
    );
}
