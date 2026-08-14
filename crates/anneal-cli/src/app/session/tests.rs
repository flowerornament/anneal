use std::collections::BTreeSet;
use std::fs;

use anneal_core::CorpusId;
use anneal_core::runtime::{
    ExplainOptions, NumberValue, Program, SnapshotEntry, SnapshotEntryFact, SnapshotTime,
    Statement, analyze, append_snapshot_entry, parse_program, query_dependencies,
    read_snapshot_history, standard_prelude_program,
};
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::{OutputMode, RuntimeCommand};
use crate::app::output::{
    CommandOutput, RowView, render_dynamic_verb_help, render_dynamic_verb_help_with_collision,
    required_string,
};
use crate::app::session::{
    CHECK_DIAGNOSTIC_QUERY, DEFAULT_CORPUS, RuntimeRegistry, RuntimeSession, handle_query,
};

fn git(root: &camino::Utf8Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(args)
        .status()
        .unwrap_or_else(|err| panic!("git {args:?} failed to run: {err}"));
    assert!(status.success(), "git {args:?} failed: {status}");
}

#[test]
fn check_gate_roots_are_structurally_protected_from_project_shadowing() {
    let query_program = parse_program("check-gate", CHECK_DIAGNOSTIC_QUERY).expect("query parses");
    let query = query_program.queries().next().expect("check query");
    let roots = query_dependencies(&Program::new(Vec::new()), query);
    assert!(
        !roots.is_empty(),
        "check must directly query a gate relation"
    );

    let prelude = standard_prelude_program().expect("prelude parses");
    for root in roots {
        assert!(
            prelude.statements.iter().any(|statement| {
                let Statement::Predicate(decl) = statement else {
                    return false;
                };
                decl.predicate_ref().is_some_and(|predicate| {
                    predicate.is_ok_and(|predicate| predicate == root)
                        && decl.string_arg("shadow") == Some("forbid")
                })
            }),
            "gate root {root} must declare shadow: forbid"
        );
    }
}

#[test]
fn jj_workspace_projects_repository_capabilities_and_teaches_missing_evidence() {
    let dir = tempdir().expect("tempdir");
    let ancestor = Utf8PathBuf::from_path_buf(dir.path().join("ancestor")).expect("utf8 tempdir");
    let root = ancestor.join("desk/.design");
    fs::create_dir_all(root.parent().expect("desk parent").join(".jj")).expect("create jj marker");
    fs::create_dir_all(&root).expect("create corpus root");
    git(&ancestor, &["init"]);
    fs::write(
        root.join("anneal.dl"),
        r#"config frontmatter { field("references", "Cites", "forward"). }"#,
    )
    .expect("write project rules");
    fs::write(root.join("a.md"), "---\nreferences: b.md\n---\n# A\n").expect("write source");
    fs::write(root.join("b.md"), "# B\n").expect("write target");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let capabilities = session
        .run(RuntimeCommand::Eval {
            query: "? repository_operation_capability(operation, availability, provider, reason)."
                .to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("capability query runs");
    let CommandOutput::Rows { rows, .. } = capabilities else {
        panic!("capability query should emit rows");
    };
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| {
        required_string(row, "availability").is_ok_and(|value| value == "unavailable")
            && required_string(row, "provider").is_ok_and(|value| value == "jj")
    }));
    let operation_reasons = rows
        .iter()
        .map(|row| {
            (
                required_string(row, "operation")
                    .expect("operation")
                    .to_string(),
                required_string(row, "reason").expect("reason").to_string(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        operation_reasons,
        BTreeSet::from([
            (
                "assertion_blame".to_string(),
                "jj-assertion-blame-not-implemented".to_string(),
            ),
            (
                "change_history".to_string(),
                "jj-change-history-not-implemented".to_string(),
            ),
            (
                "ignore_index".to_string(),
                "jj-workspace-index-unavailable".to_string(),
            ),
            (
                "target_history".to_string(),
                "jj-target-history-not-implemented".to_string(),
            ),
        ])
    );

    let history = session
        .run(RuntimeCommand::Eval {
            query: "? git_mtime(file, instant).".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("history query runs");
    let CommandOutput::Rows {
        rows,
        zero_result_hint,
        ..
    } = history
    else {
        panic!("history query should emit rows");
    };
    assert!(rows.is_empty());
    assert_eq!(
        zero_result_hint.as_deref(),
        Some(
            "hint: Git change history is unavailable in this jj workspace; query `repository_operation_capability` for runtime availability."
        )
    );

    let assertions = session
        .run(RuntimeCommand::Eval {
            query: "? *edge{assertion_date: date, assertion_revision: revision}.".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("assertion query runs");
    let CommandOutput::Rows { rows, warnings, .. } = assertions else {
        panic!("assertion query should emit rows");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].fields.get("date"),
        Some(&anneal_core::runtime::Value::Null)
    );
    assert_eq!(
        rows[0].fields.get("revision"),
        Some(&anneal_core::runtime::Value::Null)
    );
    assert_eq!(
        warnings,
        vec!["hint: assertion provenance is unavailable in this jj workspace; null fields may mean unavailable provenance or no per-edge assertion evidence. Query `repository_operation_capability`.".to_string()]
    );

    let check = session.run_check_gate().expect("check runs");
    let CommandOutput::Rows {
        zero_result_hint, ..
    } = check
    else {
        panic!("check should emit rows");
    };
    assert!(zero_result_hint.as_deref().is_some_and(|hint| {
        hint.contains("observed non-error diagnostic rows; W006 is unavailable")
    }));
}

#[test]
fn project_discovery_facts_affect_markdown_extraction() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::create_dir(root.join("included")).expect("create included");
    fs::write(
        root.join("anneal.dl"),
        r#"source md { scan_root("included"). }"#,
    )
    .expect("write project rules");
    fs::write(
        root.join("a.md"),
        "---\nstatus: draft\n---\n# Excluded\nshared marker\n",
    )
    .expect("write excluded doc");
    fs::write(
        root.join("included").join("b.md"),
        "---\nstatus: draft\n---\n# Included\nshared marker\n",
    )
    .expect("write included doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Search {
            query: "shared marker".to_string(),
            limit: 10,
            include_low_confidence: false,
        })
        .expect("search runs");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("search should emit rows");
    };

    assert!(rows.iter().any(|row| {
        row.fields.get("h")
            == Some(&anneal_core::runtime::Value::String(
                "included/b.md".to_string(),
            ))
    }));
    assert!(!rows.iter().any(|row| {
        row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
    }));
}

#[test]
fn markdown_external_root_and_code_source_keep_distinct_source_identities() {
    let dir = tempdir().expect("tempdir");
    let repo = Utf8PathBuf::from_path_buf(dir.path().join("repo")).expect("utf8 tempdir");
    let root = repo.join(".design");
    fs::create_dir_all(repo.join("formal/models")).expect("create source trees");
    fs::create_dir_all(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        source md {
          scan_root(".").
          external_root(["../formal"]).
        }
        source code { source_root(".."). }
        config frontmatter {
          field("references", "Cites", "forward").
        }
        "#,
    )
    .expect("write project rules");
    fs::write(
        root.join("spec.md"),
        "---\nreferences: formal/models/prism.md\n---\n# Spec\n",
    )
    .expect("write spec");
    fs::write(repo.join("formal/models/prism.md"), "# Prism model\n")
        .expect("write markdown model");
    fs::write(repo.join("formal/models/prism.rs"), "pub struct Prism;\n")
        .expect("write code model");
    git(&repo, &["init"]);
    git(&repo, &["add", "."]);

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: "? *handle{id: h, source: source}.".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("eval should emit rows");
    };
    let identities = rows
        .iter()
        .filter(|row| {
            required_string(row, "h").is_ok_and(|handle| handle.starts_with("formal/models/prism."))
        })
        .map(|row| {
            (
                required_string(row, "h").expect("handle field"),
                required_string(row, "source").expect("source field"),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        identities,
        BTreeSet::from([
            ("formal/models/prism.md", "markdown"),
            ("formal/models/prism.rs", "code"),
        ])
    );

    let cites = session
        .run(RuntimeCommand::Eval {
            query: r#"? *edge{from: "spec.md", to: "formal/models/prism.md", kind: "Cites"}."#
                .to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("Cites query runs");
    let CommandOutput::Rows { rows, .. } = cites else {
        panic!("Cites query should emit rows");
    };
    assert_eq!(rows.len(), 1, "cross-corpus Cites edge must resolve once");

    let broken = session
        .run(RuntimeCommand::Eval {
            query: r#"? diagnostic{code: "E001", subject: h}."#.to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("diagnostic query runs");
    let CommandOutput::Rows { rows, .. } = broken else {
        panic!("diagnostic query should emit rows");
    };
    assert!(rows.is_empty(), "resolved external root must emit no E001");
}

#[test]
fn project_potential_weight_rule_changes_energy() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        config frontmatter {
          field("depends-on", "DependsOn", "forward").
        }

        potential_weight("broken_ref", 1).
        "#,
    )
    .expect("write project rules");
    fs::write(
        root.join("a.md"),
        "---\nstatus: draft\ndepends-on: missing.md\n---\n# A\n",
    )
    .expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? potential_weight("broken_ref", weight), potential("a.md", energy)."#
                .to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("eval should emit rows");
    };

    assert!(rows.iter().any(|row| {
        row.fields.get("weight") == Some(&anneal_core::runtime::Value::Number(NumberValue::Int(1)))
    }));
    assert!(rows.iter().any(|row| {
        row.fields.get("energy") == Some(&anneal_core::runtime::Value::Number(NumberValue::Int(1)))
    }));
}

#[test]
fn search_boost_project_config_changes_rank_order() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        config search_boost {
          status("draft", 0.09).
          status("authoritative", 0).
          hub(0).
        }
        "#,
    )
    .expect("write project rules");
    fs::write(
        root.join("draft.md"),
        "---\nstatus: draft\n---\n# Draft\n\nlease protocol\n",
    )
    .expect("write draft doc");
    fs::write(
        root.join("authority.md"),
        "---\nstatus: authoritative\n---\n# Authority\n\nlease protocol\n",
    )
    .expect("write authoritative doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Search {
            query: "lease protocol".to_string(),
            limit: 2,
            include_low_confidence: false,
        })
        .expect("search runs");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("search should emit rows");
    };

    let first = rows.first().expect("first search row");
    assert_eq!(
        first.fields.get("h"),
        Some(&anneal_core::runtime::Value::String("draft.md".to_string()))
    );
}

#[test]
fn status_writes_capped_automatic_snapshot_history() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "---\nstatus: draft\n---\n# A\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let first = session.run(RuntimeCommand::Status).expect("status runs");
    let CommandOutput::Status(first) = first else {
        panic!("status should emit status output");
    };
    assert!(!first.flow_baseline_ready);

    let session = RuntimeSession::load_for_test(&root).expect("session reloads");
    let second = session
        .run(RuntimeCommand::Status)
        .expect("unchanged status runs");
    let CommandOutput::Status(second) = second else {
        panic!("status should emit status output");
    };
    assert!(second.flow_baseline_ready);

    let history = read_snapshot_history(&root).expect("read history");

    assert_eq!(history.entries().len(), 1);
    assert!(history.entries()[0].facts.iter().any(|fact| {
        fact.id.as_str() == "a.md" && fact.key == "status" && fact.value == "draft"
    }));
}

#[test]
fn runtime_loads_snapshot_history_for_eval_at_blocks() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "---\nstatus: current\n---\n# A\n").expect("write doc");
    append_snapshot_entry(
        &root,
        &SnapshotEntry::with_prelude_hash(
            "s1",
            SnapshotTime::parse("2026-05-13T10:00:00Z").expect("fixture timestamp parses"),
            CorpusId::from(DEFAULT_CORPUS),
            "test-prelude",
            vec![SnapshotEntryFact::new(
                anneal_core::HandleId::new("a.md").expect("fixture handle is nonempty"),
                "status",
                "draft",
            )],
        ),
    )
    .expect("append history");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: r#"? at("snapshot:last") { *handle{id: h, status: prior_status} }, *handle{id: h, status: current_status}, prior_status != current_status."#
                .to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval at block runs");
    let CommandOutput::Rows { rows, warnings, .. } = output else {
        panic!("eval should emit rows");
    };

    assert!(rows.iter().any(|row| {
        row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
            && row.fields.get("prior_status")
                == Some(&anneal_core::runtime::Value::String("draft".to_string()))
            && row.fields.get("current_status")
                == Some(&anneal_core::runtime::Value::String("current".to_string()))
    }));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("at(\"snapshot:last\") used snapshot fallback")),
        "expected partial-history warning, got {warnings:?}"
    );

    let quiet_output = session
        .run(RuntimeCommand::Eval {
            query: "? *handle{id: h}.".to_string(),
            explain: ExplainOptions::disabled(),
            limit: Some(1),
        })
        .expect("ordinary eval runs");
    let CommandOutput::Rows {
        warnings: quiet_warnings,
        ..
    } = quiet_output
    else {
        panic!("eval should emit rows");
    };
    assert!(
        quiet_warnings.is_empty(),
        "ordinary eval should not inherit prelude flow warnings: {quiet_warnings:?}"
    );
}

#[test]
fn eval_git_mtime_uses_git_history() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "---\nstatus: draft\n---\n# A\n").expect("write doc");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "anneal@example.test"]);
    git(&root, &["config", "user.name", "Anneal Test"]);
    git(&root, &["add", "a.md"]);
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["commit", "-m", "initial"])
        .env("GIT_AUTHOR_DATE", "2026-05-20T12:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-05-20T12:00:00+00:00")
        .status()
        .expect("git commit runs");
    assert!(status.success(), "git commit failed: {status}");

    fs::write(root.join("notes.txt"), "unrelated\n").expect("write unrelated file");
    git(&root, &["add", "notes.txt"]);
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["commit", "-m", "unrelated"])
        .env("GIT_AUTHOR_DATE", "2026-05-21T12:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-05-21T12:00:00+00:00")
        .status()
        .expect("unrelated git commit runs");
    assert!(status.success(), "unrelated git commit failed: {status}");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Eval {
            query: "? *handle{id: h, file: file}, git_mtime(file, instant).".to_string(),
            explain: ExplainOptions::disabled(),
            limit: None,
        })
        .expect("eval runs");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("eval should emit rows");
    };

    assert!(rows.iter().any(|row| {
        row.fields.get("h") == Some(&anneal_core::runtime::Value::String("a.md".to_string()))
            && row.fields.get("instant")
                == Some(&anneal_core::runtime::Value::String(
                    "2026-05-20T12:00:00Z".to_string(),
                ))
    }));
}

#[test]
fn describe_cards_teach_common_join_patterns() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let runtime = session
        .run(RuntimeCommand::Describe {
            name: "runtime".to_string(),
        })
        .expect("describe runtime runs");
    let CommandOutput::Rows { rows, .. } = runtime else {
        panic!("describe runtime should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("Visible commands: status, context, search, read, handle, schema, describe, eval, init")
                    && doc.contains("Hidden support commands: check, prime.")
                    && !doc.contains("Hidden support commands: work")
                    && doc.contains("Dimensional map: axis(name, question, oracle, disposition)")
                    && doc.contains("? axis_of(\"currency_suspect\", axis). -> Output: axis")
                    && doc.contains("Observed vocabulary recipes")
                    && doc.contains("? *handle{id: h, file: file}, git_mtime(file, instant). -> Output: h, file, instant")
                    && doc.contains("? changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}. -> Output: h, summary")
            })
        }),
        "describe runtime should fold the command map and vocabulary recipes into the teaching card"
    );

    for name in [
        "diagnostic",
        "search",
        "handle",
        "upstream",
        "downstream",
        "frontier",
        "blocker",
        "broken_reference",
        "blocked",
        "entropy",
        "undischarged",
        "obligation",
        "snapshot",
        "check",
        "E001",
        "W005",
        "W007",
        "lifecycle_config_gap",
        "frontmatter_mapping_gap",
        "*meta",
        "external_class",
        "target_path",
    ] {
        let output = session
            .run(RuntimeCommand::Describe {
                name: name.to_string(),
            })
            .unwrap_or_else(|err| panic!("describe {name} runs: {err}"));
        let CommandOutput::Rows { rows, .. } = output else {
            panic!("describe should emit rows");
        };
        assert!(
            rows.iter().any(|row| {
                required_string(row, "doc").is_ok_and(|doc| doc.contains("Common joins:"))
            }),
            "describe {name} should teach common joins: {rows:?}"
        );
    }

    let diagnostic = session
        .run(RuntimeCommand::Describe {
            name: "diagnostic".to_string(),
        })
        .expect("describe diagnostic runs");
    let CommandOutput::Rows { rows, .. } = diagnostic else {
        panic!("describe diagnostic should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("diagnostic{subject: h}, area_of")
                    && doc.contains("Example: ? diagnostic{code: \"E001\"")
                    && doc.contains("Output: h")
            })
        }),
        "describe diagnostic should carry the folded recipe and example"
    );

    let diagnostic_code = session
        .run(RuntimeCommand::Describe {
            name: "E001".to_string(),
        })
        .expect("describe E001 runs");
    let CommandOutput::Rows { rows, .. } = diagnostic_code else {
        panic!("describe E001 should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("Diagnostic code: E001")
                    && doc.contains("Rule predicate: broken_reference")
                    && doc.contains("Common joins:")
                    && doc.contains("Output: src, target, file, line")
            })
        }),
        "describe E001 should route to the diagnostic catalog and rule predicate"
    );

    let frontmatter_gap = session
        .run(RuntimeCommand::Describe {
            name: "W007".to_string(),
        })
        .expect("describe W007 runs");
    let CommandOutput::Rows { rows, .. } = frontmatter_gap else {
        panic!("describe W007 should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("distinct markdown file handles")
                    && doc.contains("*meta{handle: h, key: key}")
                    && doc.contains("field(\"KEY\", \"EDGE_KIND\", \"DIRECTION\")")
            })
        }),
        "describe W007 should teach the aggregate unit and handle drill-down"
    );

    let handle = session
        .run(RuntimeCommand::Describe {
            name: "handle".to_string(),
        })
        .expect("describe handle runs");
    let CommandOutput::Rows { rows, .. } = handle else {
        panic!("describe handle should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("anneal handle H --impact")
                    && doc.contains("*edge{to: h, from: src}")
                    && doc.contains("Output: h, src, kind")
                    && !doc.contains("Output: anneal")
            })
        }),
        "describe handle should teach --impact and reverse dependency shape"
    );

    let meta = session
        .run(RuntimeCommand::Describe {
            name: "*meta".to_string(),
        })
        .expect("describe *meta runs");
    let CommandOutput::Rows { rows, .. } = meta else {
        panic!("describe *meta should emit rows");
    };
    assert!(
        rows.iter().any(|row| {
            required_string(row, "doc").is_ok_and(|doc| {
                doc.contains("STANDARD (defined by anneal")
                    && doc.contains("SOURCE (produced by a specific source adapter")
                    && doc.contains("FRONTMATTER (passed through from YAML")
                    && doc.contains("external_class")
                    && doc.contains("target_path")
            })
        }),
        "describe *meta should teach metadata key categories"
    );
}

#[test]
fn project_verbs_are_callable_from_cli_projection() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write doc");
    fs::write(
        root.join("anneal.dl"),
        r#"
        @verb(
          name: "release-blockers",
          query: "release_blocker(\"ok\", \"v0.11\", false).\nrelease_blocker(\"strict\", \"v0.11\", true).\nrelease_row(h, milestone, strict) :=\n  verb_arg(\"milestone\", milestone),\n  verb_arg(\"strict\", strict),\n  release_blocker(h, milestone, strict).\n\n? release_row(h, milestone, strict).",
          doc: "Project-specific blockers.",
          output_schema: "{\"h\":\"String\",\"milestone\":\"String\",\"strict\":\"Bool\"}",
          args: ["milestone:String", "strict:Bool=false"],
          capabilities: ["read"]
        ).
        "#,
    )
    .expect("write project rules");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec!["v0.11".to_string()],
        })
        .expect("project verb runs");
    let CommandOutput::Rows { rows, view, .. } = output else {
        panic!("project verb should emit rows");
    };
    assert_eq!(
        view,
        RowView::Verb {
            name: "release-blockers".to_string(),
        }
    );
    assert_eq!(
        rows[0].fields.get("h"),
        Some(&anneal_core::runtime::Value::String("ok".to_string()))
    );
    assert_eq!(
        rows[0].fields.get("milestone"),
        Some(&anneal_core::runtime::Value::String("v0.11".to_string()))
    );
    assert_eq!(
        rows[0].fields.get("strict"),
        Some(&anneal_core::runtime::Value::Bool(false))
    );

    let output = session
        .run(RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec![
                "--milestone".to_string(),
                "v0.11".to_string(),
                "--strict".to_string(),
            ],
        })
        .expect("project verb named args run");
    let CommandOutput::Rows { rows, .. } = output else {
        panic!("project verb should emit rows");
    };
    assert_eq!(
        rows[0].fields.get("h"),
        Some(&anneal_core::runtime::Value::String("strict".to_string()))
    );
    assert_eq!(
        rows[0].fields.get("strict"),
        Some(&anneal_core::runtime::Value::Bool(true))
    );
}

#[test]
fn project_verb_help_discloses_a_same_named_runtime_topic() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.dl"),
        r#"
        @verb(
          name: "convergence",
          query: "? *handle{id: h}.",
          doc: "Project convergence view.",
          output_schema: "{\"h\":\"String\"}",
          args: [],
          capabilities: ["read"]
        ).
        @verb(
          name: "local_view",
          query: "? *handle{id: h}.",
          doc: "Project-only view.",
          output_schema: "{\"h\":\"String\"}",
          args: [],
          capabilities: ["read"]
        ).
        "#,
    )
    .expect("write project rules");

    let registry = RuntimeRegistry::load(&root).expect("registry loads");
    let convergence = registry
        .resolve("convergence")
        .expect("project verb resolves");
    assert!(registry.has_described_name("convergence"));
    assert!(
        render_dynamic_verb_help_with_collision(convergence, true)
            .contains("anneal describe convergence")
    );

    let local = registry
        .resolve("local_view")
        .expect("project verb resolves");
    assert!(!registry.has_described_name("local_view"));
    assert_eq!(
        render_dynamic_verb_help_with_collision(local, false),
        render_dynamic_verb_help(local),
        "non-colliding project help must remain byte-identical"
    );
}

#[test]
fn semantic_help_delegates_byte_identically_to_describe() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write doc");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    for name in ["runtime", "convergence", "frontier", "markdown"] {
        for mode in [OutputMode::Human, OutputMode::JsonExplicit] {
            let direct = session
                .run(RuntimeCommand::Describe {
                    name: name.to_string(),
                })
                .expect("describe runs");
            let projected = session
                .run(RuntimeCommand::HelpName {
                    name: name.to_string(),
                })
                .expect("help projection runs");
            let mut direct_bytes = Vec::new();
            direct
                .write(&mut direct_bytes, mode)
                .expect("render describe");
            let mut projected_bytes = Vec::new();
            projected
                .write(&mut projected_bytes, mode)
                .expect("render help projection");
            assert_eq!(projected_bytes, direct_bytes, "help {name} in {mode:?}");
        }
    }
}

#[test]
fn project_verb_named_arg_rejects_option_as_missing_value() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write doc");
    fs::write(
        root.join("anneal.dl"),
        r#"
        @verb(
          name: "release-blockers",
          query: "release_blocker(\"ok\").\nrelease_row(h, milestone, strict) :=\n  verb_arg(\"milestone\", milestone),\n  verb_arg(\"strict\", strict),\n  release_blocker(h).\n\n? release_row(h, milestone, strict).",
          doc: "Project-specific blockers.",
          output_schema: "{\"h\":\"String\",\"milestone\":\"String\",\"strict\":\"Bool\"}",
          args: ["milestone:String", "strict:Bool=false"],
          capabilities: ["read"]
        ).
        "#,
    )
    .expect("write project rules");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let Err(err) = session.run(RuntimeCommand::Verb {
        name: "release-blockers".to_string(),
        args: vec!["--milestone".to_string(), "--strict".to_string()],
    }) else {
        panic!("missing value should fail");
    };

    assert!(err.to_string().contains("--milestone requires a value"));
}

#[test]
fn project_verb_help_uses_resolved_registry_entry() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write doc");
    fs::write(
        root.join("anneal.dl"),
        r#"
        @verb(
          name: "project-pulse",
          query: "? pulse(h).",
          doc: "Project-specific pulse.",
          output_schema: "{\"h\":\"String\"}",
          args: [],
          capabilities: ["read"]
        ).
        pulse("ok").
        "#,
    )
    .expect("write project rules");

    let session = RuntimeSession::load_for_test(&root).expect("session loads");
    let output = session
        .run(RuntimeCommand::Verb {
            name: "project-pulse".to_string(),
            args: vec!["--help".to_string()],
        })
        .expect("project verb help runs");
    let CommandOutput::Text(text) = output else {
        panic!("project verb help should emit text");
    };
    assert!(text.contains("Usage: anneal [OPTIONS] project-pulse"));
    assert!(text.contains("Project-specific pulse."));
    assert!(text.contains("Output schema:"));
    assert!(text.contains("? pulse(h)."));
}

#[test]
fn runtime_rejects_legacy_toml_config() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(
        root.join("anneal.toml"),
        "[convergence]\nactive = [\"draft\"]\n",
    )
    .expect("write legacy config");

    let Err(err) = RuntimeSession::load(&root, &RuntimeCommand::Schema) else {
        panic!("legacy TOML should be migration-only");
    };

    assert!(
        err.to_string()
            .contains("anneal.toml is a legacy config file")
    );
    assert!(err.to_string().contains("anneal init --force"));
}

#[test]
fn handle_query_escapes_literals() {
    let query = handle_query("notes/\"quoted\".md");
    assert!(query.contains(r#""notes/\"quoted\".md""#));
    let mut program = standard_prelude_program().expect("prelude parses");
    program.statements.extend(
        parse_program("handle-test", &query)
            .expect("query parses")
            .statements,
    );
    analyze(program).expect("query analyzes");
}
