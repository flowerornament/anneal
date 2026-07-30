use std::ffi::OsString;
use std::num::NonZeroUsize;

use anneal_core::runtime::ExplainRowLimit;
use camino::Utf8PathBuf;

use crate::app::command::{Invocation, OutputPreference, RootSelection, RuntimeCommand};
use crate::app::help::HelpTopic;
use crate::app::session::{
    drift_refresh_announcement, drift_refresh_progress_for, edge_assertion_refresh_progress_for,
};
use crate::{DEFAULT_READ_BUDGET, DEFAULT_SEARCH_LIMIT};

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn runtime_rejects_compatibility_dialect_flags() {
    let err = Invocation::parse(os(&["anneal", "--area=compiler", "status"]))
        .expect_err("runtime should reject compatibility filters");
    assert!(err.to_string().contains("compatibility filter"), "{err}");

    let err = Invocation::parse(os(&["anneal", "--pretty", "status"]))
        .expect_err("runtime should reject compatibility render flags");
    assert!(
        err.to_string().contains("compatibility rendering flag"),
        "{err}"
    );

    let err = Invocation::parse(os(&["anneal", "status", "--area=compiler"]))
        .expect_err("standard runtime verbs should reject compatibility filters");
    assert!(
        err.to_string()
            .contains("does not accept retired compatibility filter"),
        "{err}"
    );

    let parsed = Invocation::parse(os(&["anneal", "release-blockers", "--area", "compiler"]))
        .expect("dynamic verbs may declare their own area argument");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec!["--area".to_string(), "compiler".to_string()],
        }
    );
}

#[test]
fn parses_context_options() {
    let parsed = Invocation::parse(os(&[
        "anneal",
        "--root=.design",
        "context",
        "v17 audit",
        "--budget",
        "1200",
        "--hits=2",
        "--depth=3",
        "--read-spans",
    ]))
    .expect("parse");
    assert_eq!(
        parsed.root,
        RootSelection::Explicit(Utf8PathBuf::from(".design"))
    );
    assert_eq!(
        parsed.command,
        RuntimeCommand::Context {
            goal: "v17 audit".to_string(),
            budget: 1200,
            hits: 2,
            depth: 3,
            include_low_confidence: false,
            read_spans: true,
        }
    );
}

#[test]
fn parses_read_span_id_option() {
    let parsed = Invocation::parse(os(&[
        "anneal",
        "read",
        "docs/a.md",
        "--budget=1200",
        "--span-id",
        "docs/a.md#h/target",
    ]))
    .expect("parse read");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Read {
            handle: "docs/a.md".to_string(),
            budget: 1200,
            span_id: Some("docs/a.md#h/target".to_string()),
        }
    );
}

#[test]
fn rejects_empty_read_span_id() {
    let err = Invocation::parse(os(&["anneal", "read", "docs/a.md", "--span-id="]))
        .expect_err("empty span id should fail");

    assert!(err.to_string().contains("--span-id must not be empty"));
}

#[test]
fn parses_check_gate_alias() {
    let parsed = Invocation::parse(os(&["anneal", "check"])).expect("parse check");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Check {
            refresh_drift: false
        }
    );

    let parsed = Invocation::parse(os(&["anneal", "check", "--json"])).expect("parse check");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Check {
            refresh_drift: false
        }
    );
    assert_eq!(parsed.output, OutputPreference::Json);

    let parsed =
        Invocation::parse(os(&["anneal", "check", "--refresh-drift"])).expect("parse check");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Check {
            refresh_drift: true
        }
    );
    assert!(drift_refresh_progress_for(&parsed.command).is_some());
    assert!(edge_assertion_refresh_progress_for(&parsed.command).is_some());
    assert!(drift_refresh_announcement(&parsed.command).is_some());
    assert!(drift_refresh_progress_for(&RuntimeCommand::Status).is_none());
    assert!(edge_assertion_refresh_progress_for(&RuntimeCommand::Status).is_none());
    assert!(drift_refresh_announcement(&RuntimeCommand::Status).is_none());
    assert!(
        drift_refresh_progress_for(&RuntimeCommand::Check {
            refresh_drift: false
        })
        .is_none()
    );
    assert!(
        edge_assertion_refresh_progress_for(&RuntimeCommand::Check {
            refresh_drift: false
        })
        .is_none()
    );
    assert!(
        drift_refresh_announcement(&RuntimeCommand::Check {
            refresh_drift: false
        })
        .is_none()
    );

    let err = Invocation::parse(os(&["anneal", "check", "--active-only"]))
        .expect_err("check no longer accepts compatibility filters");
    assert!(
        err.to_string().contains("retired compatibility filter"),
        "{err}"
    );

    let err = Invocation::parse(os(&["anneal", "diagnostics", "--gate"]))
        .expect_err("diagnostics is retired");
    assert!(
        err.to_string().contains("diagnostics has been retired"),
        "{err}"
    );
}

#[test]
fn rejects_context_limit_alias() {
    let err = Invocation::parse(os(&["anneal", "context", "v17 audit", "--limit=4"]))
        .expect_err("context has hits, not a generic limit");
    assert!(err.to_string().contains("context uses --hits"), "{err}");
}

#[test]
fn parses_eval_explain_depth() {
    let parsed = Invocation::parse(os(&[
        "anneal",
        "-e",
        "? diagnostic(code, severity, subject, file, line, evidence).",
        "--explain-depth",
        "4",
    ]))
    .expect("parse");
    let RuntimeCommand::Eval {
        query,
        explain,
        limit,
    } = parsed.command
    else {
        panic!("expected eval command");
    };
    assert_eq!(
        query,
        "? diagnostic(code, severity, subject, file, line, evidence)."
    );
    assert!(explain.is_enabled());
    assert_eq!(explain.depth().get(), 4);
    assert!(explain.explicit_depth());
    assert_eq!(explain.row_limit(), ExplainRowLimit::default());
    assert_eq!(limit, None);
}

#[test]
fn parses_eval_explain_row_limit_options() {
    let parsed = Invocation::parse(os(&[
        "anneal",
        "-e",
        "? blocked(h).",
        "--explain-first=2",
        "--explain-depth",
        "4",
    ]))
    .expect("parse explain first");
    let RuntimeCommand::Eval { query, explain, .. } = parsed.command else {
        panic!("expected eval command");
    };
    assert_eq!(query, "? blocked(h).");
    assert!(explain.is_enabled());
    assert_eq!(explain.depth().get(), 4);
    assert_eq!(
        explain.row_limit(),
        ExplainRowLimit::First(NonZeroUsize::new(2).expect("nonzero"))
    );

    let parsed = Invocation::parse(os(&["anneal", "-e", "? blocked(h).", "--explain-all"]))
        .expect("parse explain all");
    let RuntimeCommand::Eval { query, explain, .. } = parsed.command else {
        panic!("expected eval command");
    };
    assert_eq!(query, "? blocked(h).");
    assert!(explain.is_enabled());
    assert_eq!(explain.row_limit(), ExplainRowLimit::All);
}

#[test]
fn parses_runtime_subcommand_help_without_loading_corpus() {
    for (command, topic, expected_output) in [
        ("agent", HelpTopic::Agent, "# Anneal"),
        ("context", HelpTopic::Context, "Output: human summary"),
        ("search", HelpTopic::Search, "Output: readable rows"),
        ("read", HelpTopic::Read, "Output: readable rows"),
        (
            "check",
            HelpTopic::Check,
            "Hidden CI gate for error-severity diagnostics",
        ),
    ] {
        let parsed = Invocation::parse(os(&["anneal", "--root=.design", command, "--help"]))
            .expect("parse command help");

        assert_eq!(parsed.command, RuntimeCommand::Help { topic });
        assert!(topic.render().contains(expected_output));
        if !matches!(topic, HelpTopic::Agent) {
            assert!(topic.render().contains("Usage: anneal"));
        }
    }
    assert!(
        HelpTopic::Context
            .render()
            .contains(&format!("default: {}", crate::DEFAULT_CONTEXT_HITS))
    );
    assert!(HelpTopic::Context.render().contains(&format!(
        "default: {}",
        crate::DEFAULT_CONTEXT_NEIGHBORHOOD_DEPTH
    )));
    assert!(
        HelpTopic::Search
            .render()
            .contains(&format!("default: {DEFAULT_SEARCH_LIMIT}"))
    );
    assert!(
        HelpTopic::Read
            .render()
            .contains(&format!("default: {DEFAULT_READ_BUDGET}"))
    );
    assert!(
        HelpTopic::Status.render().contains("arrival command")
            && !HelpTopic::Status.render().contains("0.10 and earlier"),
        "status help should teach the current arrival surface"
    );
}

#[test]
fn parses_top_level_help_without_loading_corpus() {
    for help_flag in ["--help", "-h"] {
        let parsed =
            Invocation::parse(os(&["anneal", "--root=.design", help_flag])).expect("parse");

        assert_eq!(
            parsed.command,
            RuntimeCommand::Help {
                topic: HelpTopic::Top
            }
        );
    }

    let parsed = Invocation::parse(os(&["anneal", "help"])).expect("parse help");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Help {
            topic: HelpTopic::Top
        }
    );

    let rendered = HelpTopic::Top.render();
    assert!(rendered.contains("Usage: anneal [OPTIONS] [COMMAND]"));
    assert!(rendered.contains("Anneal is a convergence assistant"));
    assert!(rendered.contains("disconnected\nintelligences"));
    assert!(rendered.contains("convergence frontier"));
    assert!(rendered.contains("settledness"));
    assert!(rendered.contains("anneal help agent"));
    assert!(rendered.contains("anneal help <command-or-runtime-name>"));
    assert!(rendered.lines().count() <= 60);
    assert!(rendered.lines().all(|line| line.len() <= 80));
    for command in [
        "anneal status",
        "anneal context",
        "anneal search",
        "anneal read",
        "anneal handle",
        "anneal schema",
        "anneal describe",
        "anneal -e",
        "anneal init",
    ] {
        assert!(rendered.contains(command), "top help omits {command}");
    }
}

#[test]
fn parses_eval_help_aliases() {
    let parsed = Invocation::parse(os(&["anneal", "-e", "--help"])).expect("parse eval help");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Help {
            topic: HelpTopic::Eval
        }
    );
    let rendered = HelpTopic::Eval.render();
    assert!(rendered.contains("--explain-depth"));
    assert!(rendered.contains("--explain-first"));
    assert!(rendered.contains("--explain-all"));
    assert!(rendered.contains("Discover before guessing"));
    assert!(rendered.contains("source_of"));
    assert!(rendered.contains("anneal -e - < query.dl"));
    assert!(rendered.contains("at(\"snapshot:last\")"));
    assert!(rendered.contains("at(\"HEAD~5\") remain pending"));
    assert!(rendered.contains("Migration recipes"));
    assert!(rendered.contains("severity: \"error\""));
    assert!(rendered.contains("undischarged(h), obligation(h)"));
    assert!(!rendered.contains('\t'));
}

#[test]
fn parses_eval_stdin_marker() {
    let parsed = Invocation::parse(os(&["anneal", "-e", "-"])).expect("parse stdin eval");

    let RuntimeCommand::Eval {
        query,
        explain,
        limit,
    } = parsed.command
    else {
        panic!("expected eval command");
    };
    assert_eq!(query, "-");
    assert!(!explain.is_enabled());
    assert_eq!(limit, None);
}

#[test]
fn parses_eval_limit() {
    let parsed = Invocation::parse(os(&["anneal", "-e", "? *handle{id: h}.", "--limit=7"]))
        .expect("parse eval limit");

    let RuntimeCommand::Eval { limit, .. } = parsed.command else {
        panic!("expected eval command");
    };
    assert_eq!(limit, Some(7));
}

#[test]
fn fixed_search_and_eval_reject_zero_limits() {
    for args in [
        &["anneal", "search", "runtime", "--limit", "0"][..],
        &["anneal", "search", "runtime", "--limit=0"][..],
        &["anneal", "-e", "? *handle{id: h}.", "--limit", "0"][..],
        &["anneal", "-e", "? *handle{id: h}.", "--limit=0"][..],
    ] {
        let error = Invocation::parse(os(args)).expect_err("zero limit should fail");
        assert_eq!(
            error.to_string(),
            "--limit value \"0\" must be greater than zero"
        );
    }

    let parsed = Invocation::parse(os(&["anneal", "custom-verb", "--limit", "0"]))
        .expect("dynamic verb owns its limit semantics");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Verb {
            name: "custom-verb".to_string(),
            args: vec!["--limit".to_string(), "0".to_string()],
        }
    );
}

#[test]
fn parses_dynamic_verb_projection_options() {
    let parsed = Invocation::parse(os(&[
        "anneal",
        "release-blockers",
        "--rows=5",
        "--explain-first=2",
    ]))
    .expect("parse dynamic verb");

    let RuntimeCommand::Verb { name, args } = parsed.command else {
        panic!("expected dynamic verb command");
    };
    assert_eq!(name, "release-blockers");
    assert_eq!(args, ["--rows=5", "--explain-first=2"]);

    let parsed =
        Invocation::parse(os(&["anneal", "release-blockers", "--help"])).expect("parse help");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec!["--help".to_string()],
        }
    );
}

#[test]
fn standard_verb_explain_routes_through_dynamic_projection() {
    let parsed = Invocation::parse(os(&["anneal", "handle", "OQ-1", "--explain"]))
        .expect("parse standard explain");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Verb {
            name: "handle".to_string(),
            args: vec!["OQ-1".to_string(), "--explain".to_string()],
        }
    );
}

#[test]
fn dynamic_verb_preserves_positional_arguments_for_registry_parse() {
    let parsed = Invocation::parse(os(&["anneal", "release-blockers", "v0.11"]))
        .expect("parse dynamic verb args");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Verb {
            name: "release-blockers".to_string(),
            args: vec!["v0.11".to_string()],
        }
    );
}

#[test]
fn parses_help_subcommand_for_runtime_topics() {
    let parsed = Invocation::parse(os(&["anneal", "help", "context"])).expect("parse help context");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Help {
            topic: HelpTopic::Context
        }
    );
    assert!(HelpTopic::Context.render().contains("<GOAL>"));
}

#[test]
fn bare_invocation_defaults_to_status() {
    let parsed = Invocation::parse(os(&["anneal", "--root=.design"])).expect("parse");

    assert_eq!(parsed.command, RuntimeCommand::Status);
    assert_eq!(parsed.output, OutputPreference::Auto);
}

#[test]
fn parses_json_output_preference() {
    let parsed = Invocation::parse(os(&["anneal", "--json", "status"])).expect("parse status");

    assert_eq!(parsed.command, RuntimeCommand::Status);
    assert_eq!(parsed.output, OutputPreference::Json);
}

#[test]
fn parses_text_output_preference() {
    let parsed =
        Invocation::parse(os(&["anneal", "--format=text", "status"])).expect("parse status");

    assert_eq!(parsed.command, RuntimeCommand::Status);
    assert_eq!(parsed.output, OutputPreference::Human);

    let parsed =
        Invocation::parse(os(&["anneal", "schema", "--format", "json"])).expect("parse schema");

    assert_eq!(parsed.command, RuntimeCommand::Schema);
    assert_eq!(parsed.output, OutputPreference::Json);
}

#[test]
fn parses_ndjson_as_the_json_output_alias() {
    for args in [
        &["anneal", "--format=ndjson", "status"][..],
        &["anneal", "status", "--format", "ndjson"][..],
    ] {
        let parsed = Invocation::parse(os(args)).expect("parse ndjson output");
        assert_eq!(parsed.command, RuntimeCommand::Status);
        assert_eq!(parsed.output, OutputPreference::Json);
    }

    assert!(HelpTopic::Top.render().contains("<text|json|ndjson>"));
}

#[test]
fn parses_handle_impact_flag() {
    let parsed = Invocation::parse(os(&["anneal", "handle", "b.md", "--impact"])).expect("parse");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
        }
    );

    let parsed = Invocation::parse(os(&["anneal", "H", "--impact", "b.md"])).expect("parse");
    assert_eq!(
        parsed.command,
        RuntimeCommand::Handle {
            handle: "b.md".to_string(),
            impact: true,
            lineage: false,
        }
    );

    assert!(HelpTopic::Handle.render().contains("--impact"));
    assert!(HelpTopic::Handle.render().contains("--lineage"));
}

#[test]
fn parses_handle_lineage_flag() {
    let parsed = Invocation::parse(os(&["anneal", "handle", "b.md", "--lineage"])).expect("parse");

    assert_eq!(
        parsed.command,
        RuntimeCommand::Handle {
            handle: "b.md".to_string(),
            impact: false,
            lineage: true,
        }
    );
}

#[test]
fn describe_rejects_extra_names() {
    let error = Invocation::parse(os(&["anneal", "describe", "runtime", "extra"]))
        .expect_err("extra describe args should fail");

    assert!(
        error
            .to_string()
            .contains("describe accepts at most one name")
    );
}

#[test]
fn search_rejects_empty_query() {
    let error =
        Invocation::parse(os(&["anneal", "search", "   "])).expect_err("empty search fails");

    assert!(error.to_string().contains("search query must not be empty"));
}
