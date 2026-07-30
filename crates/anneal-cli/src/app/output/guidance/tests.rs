use std::collections::BTreeSet;
use std::fs;

use anneal_core::runtime::eval::ExplainOptions;
use anneal_core::runtime::{Query, Statement, parse_program};
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::RuntimeCommand;
use crate::app::output::test_support::status_metric_counts;
use crate::app::output::value::required_string;
use crate::app::output::{
    CommandOutput, StatusOutput, eval_zero_result_hint, search_zero_result_hint,
};
use crate::app::session::{CHECK_DIAGNOSTIC_QUERY, RuntimeSession};

fn parsed_query(source: &str) -> Query {
    parse_program("test-query", source)
        .expect("query parses")
        .statements
        .into_iter()
        .find_map(|statement| match statement {
            Statement::Query(query) => Some(query),
            _ => None,
        })
        .expect("query statement")
}

#[test]
fn zero_result_hints_preserve_query_authority() {
    let bare = parsed_query("? diagnostic(code, severity, subject, file, line, evidence).");
    assert_eq!(
        eval_zero_result_hint(&bare),
        "hint: diagnostic currently has no rows; run `anneal describe diagnostic` for requirements and common joins."
    );

    let filtered = parsed_query(
        r#"? diagnostic(code, severity, subject, file, line, evidence), severity = "warning"."#,
    );
    assert_eq!(
        eval_zero_result_hint(&filtered),
        "hint: this filtered or joined query returned 0 rows; that does not establish its predicates are empty. Relax one constraint at a time or run `anneal describe diagnostic`."
    );

    let joined = parsed_query(
        "? diagnostic(code, severity, subject, file, line, evidence), area_of(subject, area).",
    );
    assert_eq!(
        eval_zero_result_hint(&joined),
        eval_zero_result_hint(&filtered)
    );
}

#[test]
fn search_zero_result_hint_reflects_confidence_selection() {
    assert_eq!(
        search_zero_result_hint(false),
        "hint: search returned 0 rows after excluding low-confidence matches; retry with --include-low-confidence or broader terms."
    );
    assert_eq!(
        search_zero_result_hint(true),
        "hint: search returned 0 rows including low-confidence matches; retry with broader terms."
    );
}

#[test]
fn check_zero_errors_names_the_adjacent_non_error_set() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("anneal.dl"), "source md { scan_root(\".\"). }\n").expect("write project");
    fs::write(root.join("a.md"), "---\nstatus: unpartitioned\n---\n# A\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let expected = session
        .eval(CHECK_DIAGNOSTIC_QUERY, ExplainOptions::disabled())
        .expect("diagnostics run")
        .rows
        .iter()
        .filter(|row| required_string(row, "severity").is_ok_and(|value| value != "error"))
        .count();
    assert!(expected > 0, "fixture should have a non-error diagnostic");
    let output = session
        .run(RuntimeCommand::Check {
            refresh_drift: false,
        })
        .expect("check runs");
    let CommandOutput::Rows {
        rows,
        gate_failed,
        zero_result_hint,
        ..
    } = output
    else {
        panic!("check should emit rows");
    };
    assert!(rows.is_empty());
    assert!(!gate_failed);
    assert_eq!(
        zero_result_hint,
        Some(format!(
            "hint: check filters to error severity; {expected} non-error diagnostic rows remain. Run `anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'`"
        ))
    );
}

#[test]
fn check_clean_corpus_truthfully_names_zero_adjacent_rows() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    assert!(
        session
            .eval(CHECK_DIAGNOSTIC_QUERY, ExplainOptions::disabled())
            .expect("diagnostics run")
            .rows
            .is_empty()
    );
    let output = session
        .run(RuntimeCommand::Check {
            refresh_drift: false,
        })
        .expect("check runs");
    let CommandOutput::Rows {
        zero_result_hint, ..
    } = output
    else {
        panic!("check should emit rows");
    };
    assert!(
        zero_result_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("0 non-error diagnostic rows remain"))
    );
}

#[test]
fn check_with_errors_keeps_gate_and_omits_zero_result_guidance() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "---\ndepends-on: missing.md\n---\n# A\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Check {
            refresh_drift: false,
        })
        .expect("check runs");
    let CommandOutput::Rows {
        rows,
        gate_failed,
        zero_result_hint,
        ..
    } = output
    else {
        panic!("check should emit rows");
    };
    assert!(!rows.is_empty());
    assert!(gate_failed);
    assert_eq!(zero_result_hint, None);
}

#[test]
fn status_histogram_counts_diagnostic_rows_not_distinct_codes() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("anneal.dl"), "source md { scan_root(\".\"). }\n").expect("write project");
    fs::write(root.join("a.md"), "---\nstatus: alpha\n---\n# A\n").expect("write a");
    fs::write(root.join("b.md"), "---\nstatus: beta\n---\n# B\n").expect("write b");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let diagnostics = session
        .eval(CHECK_DIAGNOSTIC_QUERY, ExplainOptions::disabled())
        .expect("diagnostics run")
        .rows;
    let distinct_codes = diagnostics
        .iter()
        .map(|row| required_string(row, "code").expect("code"))
        .collect::<BTreeSet<_>>();
    assert!(
        diagnostics.len() > distinct_codes.len(),
        "fixture must carry repeated diagnostic codes"
    );

    let output = session.run_status().expect("status runs");
    let CommandOutput::Status(StatusOutput { rows, .. }) = output else {
        panic!("status should emit metrics");
    };
    let counts = status_metric_counts(&rows, "diagnostics");
    let diagnostic_count = i64::try_from(diagnostics.len()).expect("fixture count fits i64");
    assert_eq!(
        counts.get("total"),
        Some(&diagnostic_count),
        "histogram total must count relation rows"
    );
    let severity_total = ["error", "warning", "suggestion", "info"]
        .iter()
        .map(|severity| counts.get(*severity).copied().unwrap_or_default())
        .sum::<i64>();
    assert_eq!(severity_total, diagnostic_count);
}

#[test]
fn explicit_eval_attaches_zero_result_guidance_but_project_verbs_do_not() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        @verb(
          name: "empty_report",
          query: "? diagnostic(\"NOPE\", severity, _, _, _, _).",
          doc: "An intentionally empty author-defined report.",
          output_schema: "{\"severity\":\"String\"}",
          args: [],
          capabilities: ["read"]
        ).
        "#,
    )
    .expect("write project verb");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: "? diagnostic(code, severity, subject, file, line, evidence).".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows {
        zero_result_hint, ..
    } = output
    else {
        panic!("eval should emit rows");
    };
    assert_eq!(
        zero_result_hint.as_deref(),
        Some(
            "hint: diagnostic currently has no rows; run `anneal describe diagnostic` for requirements and common joins."
        )
    );

    let output = session
        .run_dynamic_verb("empty_report", &[])
        .expect("project verb path runs");
    let CommandOutput::Rows {
        rows,
        zero_result_hint,
        ..
    } = output
    else {
        panic!("verb should emit rows");
    };
    assert!(rows.is_empty());
    assert_eq!(zero_result_hint, None);
}
