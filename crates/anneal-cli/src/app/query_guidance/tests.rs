use std::fs;

use anneal_core::runtime::ExplainOptions;
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::RuntimeCommand;
use crate::app::output::CommandOutput;
use crate::app::query_guidance::{
    query_demands_code_drift_evidence, query_demands_code_target_history,
    query_demands_edge_assertions, ranked_anchor_handle_field,
};
use crate::app::session::RuntimeSession;

#[test]
fn ranked_anchor_detector_uses_predicate_identity() {
    assert_eq!(
        ranked_anchor_handle_field("? ranked_anchor(handle, r, s, w).").as_deref(),
        Some("handle")
    );
    assert_eq!(
        ranked_anchor_handle_field(
            "? ranked_anchor(handle, r, s, w), *handle{id: handle, file: file}."
        )
        .as_deref(),
        Some("handle")
    );
    assert_eq!(
        ranked_anchor_handle_field("? *handle{id: handle, kind: r, status: s, file: w}."),
        None
    );
}

#[test]
fn eval_warns_when_query_filters_retired_section_kind() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? *handle{id: h, kind: "section"}."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows { rows, warnings, .. } = output else {
        panic!("eval should emit rows");
    };

    assert!(rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("section handle kind was retired in v0.14"));
    assert!(warnings[0].contains("*span"));
}

#[test]
fn eval_does_not_warn_for_code_section_handles() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n\nBody.\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? *handle{source: "code", id: h, kind: "section"}."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows { warnings, .. } = output else {
        panic!("eval should emit rows");
    };

    assert!(warnings.is_empty());
}

#[test]
fn transitive_convergence_queries_demand_code_target_history() {
    for query in [
        "? status_item(section, h, score, why).",
        "? holding(h).",
        "? flow(h, direction).",
        "? ranked_work(h, energy, rank).",
        "? area_frontier(area, h, score, why).",
        "? primary_entropy(h, source).",
    ] {
        assert!(
            query_demands_code_target_history(query),
            "{query} should demand target-history facts through potential/entropy"
        );
    }
    assert!(query_demands_code_target_history(
        "? *meta{handle: h, key: \"target_exists\", value: exists}."
    ));
    assert!(query_demands_code_target_history(
        "? frontier(h, energy), *handle{id: h}."
    ));
    assert!(!query_demands_code_target_history("? *handle{id: h}."));
    assert!(!query_demands_code_target_history(
        "? recent_frontier(h, rank, recency), *handle{id: h}."
    ));
}

#[test]
fn edge_assertion_queries_demand_edge_assertion_probe_only_when_explicit() {
    assert!(query_demands_edge_assertions(
        "? *edge{from: a, to: b, assertion_date: date}."
    ));
    assert!(query_demands_edge_assertions(
        "? *edge{from: a, to: b, assertion_revision: rev}."
    ));
    assert!(!query_demands_edge_assertions(
        "? *edge{from: a, to: b, file: file, line: line}."
    ));
    assert!(!query_demands_edge_assertions(
        "? recent_frontier(h, rank, recency)."
    ));
}

#[test]
fn code_reference_queries_demand_drift_evidence() {
    assert!(query_demands_code_drift_evidence(
        "? code_ref(spec, ref, path, code_handle, disposition)."
    ));
    assert!(query_demands_code_drift_evidence(
        "? drift_profile(bucket, count)."
    ));
    assert!(!query_demands_code_drift_evidence("? *handle{id: h}."));
}
