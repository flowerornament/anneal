use super::test_support::{row, status_output};
use super::{CommandOutput, EMPTY_ROWS_DIAGNOSTIC, RowView, search_zero_result_hint};
use crate::app::command::OutputMode;

#[test]
fn empty_row_outputs_report_zero_rows_to_stderr() {
    assert_eq!(
        CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Json),
        Some(EMPTY_ROWS_DIAGNOSTIC)
    );
    assert_eq!(
        CommandOutput::rows(Vec::new(), RowView::Eval).empty_rows_diagnostic(OutputMode::Human),
        None
    );
    assert_eq!(
        CommandOutput::rows(
            Vec::new(),
            RowView::Handle {
                handle: "missing.md".to_string(),
                impact: false,
                lineage: false,
                missing: true,
            },
        )
        .empty_rows_diagnostic(OutputMode::Human),
        None
    );
    assert_eq!(
        CommandOutput::rows(Vec::new(), RowView::Broken).empty_rows_diagnostic(OutputMode::Human),
        None
    );
    assert_eq!(
        status_output(Vec::new()).empty_rows_diagnostic(OutputMode::Json),
        Some(EMPTY_ROWS_DIAGNOSTIC)
    );
    assert_eq!(
        status_output(Vec::new()).empty_rows_diagnostic(OutputMode::Human),
        None
    );
}

#[test]
fn zero_result_hints_render_inline_but_keep_machine_stdout_clean() {
    let hint = search_zero_result_hint(false);
    let output =
        CommandOutput::rows(Vec::new(), RowView::Search).with_zero_result_hint(Some(hint.clone()));
    assert_eq!(
        output.stderr_diagnostic(OutputMode::Json),
        Some(format!("{hint}\n{EMPTY_ROWS_DIAGNOSTIC}"))
    );

    let output = CommandOutput::rows(Vec::new(), RowView::Search).with_zero_result_hint(Some(hint));
    let mut rendered = Vec::new();
    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render rows");
    assert_eq!(
        String::from_utf8(rendered).expect("utf8"),
        "Search (0)\n(0 rows)\nhint: search returned 0 rows after excluding low-confidence matches; retry with --include-low-confidence or broader terms.\n"
    );
}

#[test]
fn empty_binding_rows_emit_a_human_hint() {
    let output = CommandOutput::rows_with_empty_binding_hint(
        vec![row(&[]), row(&[])],
        RowView::Eval,
        Some(r#"? diagnostic{severity: "error", code: code}."#.to_string()),
    );
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render rows");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Results (2)"));
    assert!(rendered.contains("hint: matched 2 rows but no fields are bound for output."));
    assert!(rendered.contains(r#"? diagnostic{severity: "error", code: code}."#));
}

#[test]
fn empty_binding_rows_emit_a_json_stderr_hint() {
    let output = CommandOutput::rows_with_empty_binding_hint(
        vec![row(&[])],
        RowView::Eval,
        Some("? settled(h).".to_string()),
    );

    assert_eq!(
        output.stderr_diagnostic(OutputMode::Json),
        Some(
            "hint: matched 1 rows but no fields are bound for output.\nAdd a variable to extract values, e.g.:\n  ? settled(h)."
                .to_string()
        )
    );
}
