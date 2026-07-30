use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anneal_core::runtime::{ExplainOptions, NumberValue};
use anneal_core::{
    CorpusId, EdgeFact, FactBatch, FactBatchMode, FactIdentity, FactStore, Generation, HandleFact,
    MetaFact, MetaRole, NativeId, OriginUri, Revision, SourceName,
};
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::{OutputMode, RuntimeCommand};
use crate::app::navigation::{RESOLVED_FILE_META_KEY, SUPERSEDES_EDGE_KIND, handle_lineage_rows};
use crate::app::output::{CommandOutput, RowView};
use crate::app::session::RuntimeSession;

fn handle_id(value: &str) -> anneal_core::HandleId {
    anneal_core::HandleId::new(value).expect("fixture handle is nonempty")
}

fn string_field<'a>(row: &'a anneal_core::runtime::Row, field: &str) -> anyhow::Result<&'a str> {
    match row.fields.get(field) {
        Some(anneal_core::runtime::Value::String(value)) => Ok(value),
        value => anyhow::bail!("expected string field {field}, got {value:?}"),
    }
}

fn number_field<'a>(
    row: &'a anneal_core::runtime::Row,
    field: &str,
) -> anyhow::Result<&'a NumberValue> {
    match row.fields.get(field) {
        Some(anneal_core::runtime::Value::Number(value)) => Ok(value),
        value => anyhow::bail!("expected number field {field}, got {value:?}"),
    }
}

fn test_identity(native_id: &str) -> FactIdentity {
    FactIdentity::new(
        CorpusId::from("test"),
        SourceName::from("test-source"),
        NativeId::from(native_id),
        OriginUri::from(format!("file:///{native_id}")),
        Revision::from("test-revision"),
        Generation::new(1),
    )
}

fn test_handle(id: &str, kind: &str, status: Option<&str>, file: &str) -> HandleFact {
    HandleFact {
        identity: test_identity(id),
        id: handle_id(id),
        kind: kind.to_string(),
        status: status.map(str::to_string),
        namespace: String::new(),
        file: file.to_string(),
        line: 1,
        date: None,
        area: String::new(),
        summary: String::new(),
    }
}

fn test_edge(from: &str, to: &str, kind: &str) -> EdgeFact {
    EdgeFact {
        identity: test_identity(from),
        from: handle_id(from),
        to: handle_id(to),
        kind: kind.to_string(),
        file: from.to_string(),
        line: 1,
        assertion_date: None,
        assertion_revision: None,
    }
}

fn test_meta(handle: &str, key: &str, value: &str) -> MetaFact {
    MetaFact {
        identity: test_identity(handle),
        handle: handle_id(handle),
        key: key.to_string(),
        value: value.to_string(),
        role: MetaRole::Derived,
    }
}

fn lineage_store() -> FactStore {
    let mut batch = FactBatch::new(
        CorpusId::from("test"),
        SourceName::from("test-source"),
        FactBatchMode::FullSnapshot,
        Generation::new(1),
    );
    batch.handles.extend([
        test_handle(
            "implementation/2026-05-30-unified.md",
            "file",
            Some("superseded"),
            "implementation/2026-05-30-unified.md",
        ),
        test_handle(
            "compiler/2026-03-30-cell-graph.md",
            "file",
            Some("superseded"),
            "compiler/2026-03-30-cell-graph.md",
        ),
        test_handle(
            "implementation/2026-05-31-program-space.md",
            "file",
            Some("active"),
            "implementation/2026-05-31-program-space.md",
        ),
        test_handle(
            "formal-model/history/sample-formal-model-v14.md",
            "file",
            Some("superseded"),
            "formal-model/history/sample-formal-model-v14.md",
        ),
        test_handle(
            "formal-model/sample-formal-model-v17.md",
            "file",
            Some("authoritative"),
            "formal-model/sample-formal-model-v17.md",
        ),
        test_handle("sample-formal-model-v14", "version", None, ""),
        test_handle("sample-formal-model-v17", "version", None, ""),
        test_handle("raw-v14", "version", None, ""),
        test_handle("raw-v17", "version", None, ""),
    ]);
    batch.edges.extend([
        test_edge(
            "implementation/2026-05-30-unified.md",
            "implementation/2026-05-31-program-space.md",
            SUPERSEDES_EDGE_KIND,
        ),
        test_edge(
            "compiler/2026-03-30-cell-graph.md",
            "implementation/2026-05-31-program-space.md",
            SUPERSEDES_EDGE_KIND,
        ),
        test_edge(
            "formal-model/history/sample-formal-model-v14.md",
            "formal-model/sample-formal-model-v17.md",
            SUPERSEDES_EDGE_KIND,
        ),
        test_edge(
            "sample-formal-model-v17",
            "sample-formal-model-v14",
            SUPERSEDES_EDGE_KIND,
        ),
        test_edge("raw-v17", "raw-v14", SUPERSEDES_EDGE_KIND),
    ]);
    batch.meta.push(test_meta(
        "sample-formal-model-v14",
        RESOLVED_FILE_META_KEY,
        "formal-model/history/sample-formal-model-v14.md",
    ));
    let mut store = FactStore::default();
    store.merge(batch).expect("merge lineage batch");
    store
}

#[test]
fn lineage_normalizes_short_handles_before_walking_file_edges() {
    let store = lineage_store();
    let rows = handle_lineage_rows(&store, "sample-formal-model-v14");
    let lineage = rows
        .iter()
        .filter(|row| string_field(row, "relation").is_ok_and(|value| value == "lineage"))
        .collect::<Vec<_>>();

    assert!(lineage.iter().all(|row| {
        string_field(row, "normalized_root")
            .is_ok_and(|root| root == "formal-model/history/sample-formal-model-v14.md")
    }));
    assert!(lineage.iter().any(|row| {
        string_field(row, "other")
            .is_ok_and(|other| other == "formal-model/sample-formal-model-v17.md")
            && string_field(row, "role").is_ok_and(|role| role == "successor")
            && string_field(row, "disposition")
                .is_ok_and(|disposition| disposition == "current_head")
    }));
    assert!(
        handle_lineage_rows(&store, "raw-v14").is_empty(),
        "raw reversed short-id edges must not be walked without file normalization"
    );
}

#[test]
fn lineage_renderer_shows_merge_predecessors_and_heads() {
    let store = lineage_store();
    let rows = handle_lineage_rows(&store, "implementation/2026-05-31-program-space.md");
    let mut rendered = Vec::new();

    CommandOutput::rows(
        rows,
        RowView::Handle {
            handle: "implementation/2026-05-31-program-space.md".to_string(),
            impact: false,
            lineage: true,
            missing: false,
        },
    )
    .write(&mut rendered, OutputMode::Human)
    .expect("render lineage");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Lineage (file supersession)"));
    assert!(rendered.contains("Current head(s) (1)"));
    assert!(rendered.contains("implementation/2026-05-31-program-space.md"));
    assert!(rendered.contains("Older (2)"));
    assert!(rendered.contains("implementation/2026-05-30-unified.md"));
    assert!(rendered.contains("compiler/2026-03-30-cell-graph.md"));
}

#[test]
fn handle_impact_projects_configured_reverse_dependencies() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        config frontmatter {
          field("synthesizes", "Synthesizes", "forward").
          field("references", "Cites", "forward").
        }

        config impact {
          traverse(["DependsOn", "Synthesizes"]).
        }
        "#,
    )
    .expect("write project rules");
    fs::write(root.join("b.md"), "# B\n").expect("write b");
    fs::write(root.join("a.md"), "---\ndepends-on: b.md\n---\n# A\n").expect("write a");
    fs::write(root.join("c.md"), "---\nsynthesizes: b.md\n---\n# C\n").expect("write c");
    fs::write(root.join("d.md"), "---\nreferences: b.md\n---\n# D\n").expect("write d");
    fs::write(root.join("e.md"), "---\ndepends-on: a.md\n---\n# E\n").expect("write e");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
        })
        .expect("handle runs");
    let CommandOutput::Rows { rows, view, .. } = output else {
        panic!("handle should emit rows");
    };
    assert_eq!(
        view,
        RowView::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
            missing: false,
        }
    );

    let impacted = rows
        .iter()
        .filter(|row| string_field(row, "relation").is_ok_and(|value| value == "impact"))
        .map(|row| {
            (
                string_field(row, "other").expect("other").to_string(),
                *number_field(row, "depth").expect("depth"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert_eq!(impacted.get("a.md"), Some(&NumberValue::Int(1)));
    assert_eq!(impacted.get("c.md"), Some(&NumberValue::Int(1)));
    assert_eq!(impacted.get("e.md"), Some(&NumberValue::Int(2)));
    assert!(!impacted.contains_key("d.md"));

    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? impact("b.md", affected, depth), depth = 1."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("impact eval runs");
    let CommandOutput::Rows {
        rows: eval_rows, ..
    } = output
    else {
        panic!("impact eval should emit rows");
    };
    let direct_eval = eval_rows
        .iter()
        .filter_map(|row| string_field(row, "affected").ok().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let direct_handle = impacted
        .iter()
        .filter_map(|(handle, depth)| (depth == &NumberValue::Int(1)).then_some(handle.clone()))
        .collect::<BTreeSet<_>>();

    assert_eq!(direct_handle, direct_eval);

    let mut rendered = Vec::new();
    CommandOutput::rows(
        rows,
        RowView::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
            missing: false,
        },
    )
    .write(&mut rendered, OutputMode::Human)
    .expect("render handle");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Impact (configured reverse traversal)\nDirect (2)"));
    assert!(rendered.contains("Indirect (1)"));
    assert!(rendered.contains("a.md"));
    assert!(rendered.contains("c.md"));
    assert!(rendered.contains("e.md"));
    let impact_text = rendered
        .split("Impact (configured reverse traversal)\n")
        .nth(1)
        .expect("impact section");
    assert!(!impact_text.contains("d.md"));
}

#[test]
fn handle_impact_and_primitive_share_the_default_policy() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("b.md"), "# B\n").expect("write b");
    fs::write(root.join("a.md"), "---\ndepends-on: b.md\n---\n# A\n").expect("write a");
    fs::write(root.join("c.md"), "---\nverifies: b.md\n---\n# C\n").expect("write c");
    fs::write(root.join("d.md"), "---\nsuperseded-by: b.md\n---\n# D\n").expect("write d");
    fs::write(root.join("e.md"), "---\ndischarges: b.md\n---\n# E\n").expect("write e");
    fs::write(root.join("f.md"), "---\ndepends-on: a.md\n---\n# F\n").expect("write f");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let handle_output = session
        .run(RuntimeCommand::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
        })
        .expect("handle runs");
    let CommandOutput::Rows {
        rows: handle_rows, ..
    } = handle_output
    else {
        panic!("handle should emit rows");
    };
    let handle_impact = handle_rows
        .iter()
        .filter(|row| string_field(row, "relation").is_ok_and(|value| value == "impact"))
        .map(|row| {
            (
                string_field(row, "other").expect("other").to_string(),
                *number_field(row, "depth").expect("depth"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let eval_output = session
        .run(RuntimeCommand::Eval {
            query: r#"? impact("b.md", affected, depth)."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("impact eval runs");
    let CommandOutput::Rows {
        rows: eval_rows, ..
    } = eval_output
    else {
        panic!("impact eval should emit rows");
    };
    let primitive_impact = eval_rows
        .iter()
        .map(|row| {
            (
                string_field(row, "affected").expect("affected").to_string(),
                *number_field(row, "depth").expect("depth"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert_eq!(handle_impact, primitive_impact);
    assert_eq!(handle_impact.get("a.md"), Some(&NumberValue::Int(1)));
    assert_eq!(handle_impact.get("c.md"), Some(&NumberValue::Int(1)));
    assert_eq!(handle_impact.get("d.md"), Some(&NumberValue::Int(1)));
    assert_eq!(handle_impact.get("f.md"), Some(&NumberValue::Int(2)));
    assert!(!handle_impact.contains_key("e.md"));
}
