use super::*;
use std::ffi::OsString;

use anneal_core::InferredCorpusRoot;
use camino::{Utf8Path, Utf8PathBuf};

use crate::app::command::{Invocation, OutputMode, RootSelection, RuntimeCommand};

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn routes_only_runtime_commands() {
    assert!(should_handle_args(&os(&["anneal"])));
    assert!(should_handle_args(&os(&["anneal", "--help"])));
    assert!(should_handle_args(&os(&["anneal", "-h"])));
    assert!(should_handle_args(&os(&["anneal", "--root", ".design"])));
    assert!(should_handle_args(&os(&[
        "anneal", "--root", ".design", "context", "goal"
    ])));
    assert!(should_handle_args(&os(&[
        "anneal",
        "-e",
        "? *handle{id: h}."
    ])));
    assert!(should_handle_args(&os(&["anneal", "help", "context"])));
    assert!(should_handle_args(&os(&["anneal", "help", "agent"])));
    assert!(should_handle_args(&os(&[
        "anneal",
        "--root",
        ".design",
        "release-blockers"
    ])));
    assert!(should_handle_args(&os(&[
        "anneal",
        "help",
        "release-blockers"
    ])));
    assert!(should_handle_args(&os(&["anneal", "anneal"])));
    assert!(should_handle_args(&os(&[
        "anneal", "--root", ".design", "status"
    ])));
    assert!(should_handle_args(&os(&[
        "anneal", "--format", "text", "work"
    ])));
    assert!(should_handle_args(&os(&[
        "anneal",
        "--format=text",
        "vocab"
    ])));
    assert!(should_handle_args(&os(&["anneal", "areas"])));
    assert!(should_handle_args(&os(&["anneal", "help", "areas"])));
    assert!(should_handle_args(&os(&[
        "anneal", "--area", "compiler", "status"
    ])));
    assert!(should_handle_args(&os(&["anneal", "--pretty", "status"])));
    assert!(should_handle_args(&os(&[
        "anneal", "--root", ".design", "health"
    ])));
    for retired in [
        "work",
        "blocked",
        "diagnostics",
        "broken",
        "areas",
        "trend",
        "sources",
        "impact",
        "find",
        "get",
        "map",
        "health",
        "diff",
        "obligations",
        "garden",
        "orient",
        "query",
        "explain",
    ] {
        assert!(
            should_handle_args(&os(&["anneal", retired])),
            "retired command {retired:?} should route to runtime recovery"
        );
        assert!(
            should_handle_args(&os(&["anneal", "help", retired])),
            "retired help topic {retired:?} should route to runtime recovery"
        );
    }
    assert!(should_handle_args(&os(&["anneal", "check"])));
    assert!(should_handle_args(&os(&[
        "anneal", "--area", "compiler", "check"
    ])));
    assert!(should_handle_args(&os(&["anneal", "init"])));
    assert!(should_handle_args(&os(&["anneal", "prime"])));
    assert!(should_handle_args(&os(&["anneal", "help", "check"])));
    assert!(should_handle_args(&os(&["anneal", "--version"])));
    assert!(should_handle_args(&os(&["anneal", "--help"])));
    assert!(should_handle_args(&os(&["anneal", "check", "--json"])));
    assert!(!should_handle_args(&os(&["anneal", "--mcp"])));
}

#[test]
fn parses_version_without_loading_corpus() {
    let parsed = Invocation::parse(os(&["anneal", "--version"])).expect("parse version");
    assert_eq!(parsed.command, RuntimeCommand::Version);

    let parsed = Invocation::parse(os(&["anneal", "version"])).expect("parse version command");
    assert_eq!(parsed.command, RuntimeCommand::Version);

    let err = Invocation::parse(os(&["anneal", "--version", "status"]))
        .expect_err("version accepts no args");
    assert!(err.to_string().contains("accepts no arguments"), "{err}");
}

#[test]
fn marked_root_is_reported_for_json_or_empty_outputs() {
    let root = Utf8PathBuf::from("/tmp/corpus/.design");

    assert_eq!(
        RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
            .diagnostic(OutputMode::Json, true),
        Some("resolved root: /tmp/corpus/.design".to_string())
    );
    assert_eq!(
        RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
            .diagnostic(OutputMode::Human, false),
        Some("resolved root: /tmp/corpus/.design".to_string())
    );
    assert_eq!(
        RootSelection::Inferred(InferredCorpusRoot::Marked(root.clone()))
            .diagnostic(OutputMode::Human, true),
        None
    );
    assert_eq!(
        RootSelection::Explicit(root).diagnostic(OutputMode::Json, true),
        None
    );
}

#[test]
fn unmarked_root_is_rejected_before_runtime_output() {
    let root = Utf8PathBuf::from("/tmp/stray");
    let selection = RootSelection::Inferred(InferredCorpusRoot::Unmarked(root.clone()));

    assert_eq!(
        selection.implicit_unmarked_root(),
        Some(Utf8Path::new("/tmp/stray"))
    );
    assert_eq!(selection.diagnostic(OutputMode::Human, true), None);

    let explicit = RootSelection::Explicit(root);
    assert_eq!(explicit.implicit_unmarked_root(), None);
}
