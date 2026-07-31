use std::fs;

use anneal_core::runtime::ExplainOptions;
use anneal_core::runtime::standard_prelude_program;
use anneal_core::{VerbLayer, VerbRegistry};
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
    let program = standard_prelude_program().expect("prelude parses");
    let registry = VerbRegistry::default();
    for query in [
        "? status_item(section, h, score, why).",
        "? holding(h).",
        "? flow(h, direction).",
        "? ranked_work(h, energy, rank).",
        "? area_frontier(area, h, score, why).",
        "? primary_entropy(h, source).",
    ] {
        let demands = RuntimeCommand::Eval {
            query: query.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        }
        .evidence_demands(&program, &registry);
        assert!(
            demands.code_target_history,
            "{query} should demand target-history facts through potential/entropy"
        );
    }
    assert!(query_demands_code_target_history(
        "? *meta{handle: h, key: \"target_exists\", value: exists}."
    ));
    let frontier = RuntimeCommand::Eval {
        query: "? frontier(h, energy), *handle{id: h}.".to_string(),
        explain: ExplainOptions::disabled(),
        limit: None,
    }
    .evidence_demands(&program, &registry);
    assert!(frontier.code_target_history);
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
    let program = standard_prelude_program().expect("prelude parses");
    let registry = VerbRegistry::default();
    for query in [
        "? code_ref(spec, ref, path, code_handle, disposition).",
        "? drift_profile(bucket, count).",
        "? diagnostic(code, severity, subject, file, line, evidence).",
        r"
        project_diagnostic(code, severity, subject, file, line, evidence) :=
          diagnostic(code, severity, subject, file, line, evidence).
        ? project_diagnostic(code, severity, subject, file, line, evidence).
        ",
    ] {
        let demands = RuntimeCommand::Eval {
            query: query.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        }
        .evidence_demands(&program, &registry);
        assert!(
            demands.code_drift,
            "{query} should transitively demand drift evidence"
        );
    }
    assert!(!query_demands_code_drift_evidence("? *handle{id: h}."));
}

#[test]
fn check_and_its_taught_diagnostic_query_load_the_same_evidence() {
    let program = standard_prelude_program().expect("prelude parses");
    let registry = VerbRegistry::default();
    let check = RuntimeCommand::Check {
        refresh_drift: false,
    }
    .evidence_demands(&program, &registry);
    let drill_down = RuntimeCommand::Eval {
        query: "? diagnostic(code, severity, subject, file, line, evidence).".to_string(),
        explain: ExplainOptions::disabled(),
        limit: None,
    }
    .evidence_demands(&program, &registry);

    assert_eq!(drill_down, check);
}

#[test]
fn unqueried_drift_rule_does_not_change_eval_evidence_demand() {
    let program = standard_prelude_program().expect("prelude parses");
    let registry = VerbRegistry::default();
    let baseline = RuntimeCommand::Eval {
        query: "? diagnostic(code, severity, subject, file, line, evidence).".to_string(),
        explain: ExplainOptions::disabled(),
        limit: None,
    }
    .evidence_demands(&program, &registry);
    let with_unqueried_rule = RuntimeCommand::Eval {
        query: r#"
            unqueried(ref) := referent_disposition(ref, "referent-intact").
            ? diagnostic(code, severity, subject, file, line, evidence).
        "#
        .to_string(),
        explain: ExplainOptions::disabled(),
        limit: None,
    }
    .evidence_demands(&program, &registry);

    assert_eq!(baseline, with_unqueried_rule);
    assert!(baseline.code_drift);
}

#[test]
fn project_verb_derives_drift_evidence_demand_from_its_query() {
    let prelude = standard_prelude_program().expect("prelude parses");
    let project = anneal_core::runtime::parse_program(
        "anneal.dl",
        r#"
        @verb(
          name: "drift-audit",
          query: "? project_drift(ref, disposition).",
          doc: "Project drift audit.",
          output_schema: "{\"ref\":\"String\",\"disposition\":\"String\"}",
          args: [],
          capabilities: ["read"]
        ).
        project_drift(ref, disposition) := referent_disposition(ref, disposition).
        "#,
    )
    .expect("project program parses");
    let registry = VerbRegistry::from_layers(&[
        (VerbLayer::Prelude, &prelude),
        (VerbLayer::Project, &project),
    ])
    .expect("verb registry builds");
    let mut program = prelude;
    program.statements.extend(project.statements);

    let demands = RuntimeCommand::Verb {
        name: "drift-audit".to_string(),
        args: Vec::new(),
    }
    .evidence_demands(&program, &registry);

    assert!(demands.code_drift);
}
