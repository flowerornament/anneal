use std::ffi::OsString;
use std::fs;

use anneal_core::runtime::ExplainOptions;
use anneal_core::runtime::parse_program;
use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::app::command::{Invocation, RuntimeCommand};
use crate::app::help::{HelpTopic, SKILL_MARKDOWN, skill_briefing_body, skill_section};
use crate::app::output::required_string;
use crate::app::run_args;
use crate::app::session::RuntimeSession;

mod executable_docs;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn help_agent_renders_shipped_skill_briefing() {
    let rendered = HelpTopic::Agent.render();

    assert_eq!(rendered, skill_briefing_body(SKILL_MARKDOWN));
    assert!(rendered.contains("# Anneal"));
    assert!(rendered.contains("## First Moves"));
    assert!(rendered.contains("## Agent Rules"));
    assert!(rendered.contains("outside the corpus root"));
    assert!(rendered.contains("Git-project-relative handles"));
    assert!(rendered.contains("collide on a handle fail loudly"));
    assert!(!rendered.starts_with("---"));

    let thesis = skill_section(SKILL_MARKDOWN, "Product Thesis").expect("product thesis");
    assert!(HelpTopic::Top.render().contains(thesis));
    assert!(rendered.contains(thesis));
}

#[test]
fn top_and_agent_help_project_the_same_product_thesis() {
    fn thesis_paragraph<'a>(rendered: &'a str, canonical: &str) -> &'a str {
        rendered
            .split("\n\n")
            .find(|paragraph| *paragraph == canonical)
            .expect("rendered help contains the product thesis")
    }

    let top = HelpTopic::Top.render();
    let agent = HelpTopic::Agent.render();
    let canonical = skill_section(SKILL_MARKDOWN, "Product Thesis").expect("product thesis");

    assert_eq!(
        thesis_paragraph(&top, canonical),
        thesis_paragraph(&agent, canonical)
    );
}

#[test]
fn semantic_help_names_parse_for_corpus_resolution() {
    for name in ["runtime", "convergence", "frontier", "banana"] {
        let parsed = Invocation::parse(os(&["anneal", "help", name])).expect("parse help");
        assert_eq!(
            parsed.command,
            RuntimeCommand::HelpName {
                name: name.to_string()
            }
        );
    }
}

#[test]
fn unknown_help_topic_points_to_runtime_discovery() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
    let err = run_args(vec![
        OsString::from("anneal"),
        OsString::from("--root"),
        OsString::from(root.as_str()),
        OsString::from("help"),
        OsString::from("banana"),
    ])
    .expect_err("unknown help topic should error");

    assert!(
        err.to_string().contains("anneal schema")
            && err.to_string().contains("anneal describe runtime"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn static_command_help_has_examples_links_and_collision_disclosure() {
    for topic in [
        HelpTopic::Init,
        HelpTopic::Status,
        HelpTopic::Context,
        HelpTopic::Search,
        HelpTopic::Read,
        HelpTopic::Handle,
        HelpTopic::Check,
        HelpTopic::Describe,
        HelpTopic::Schema,
        HelpTopic::Eval,
    ] {
        let rendered = topic.render();
        assert!(rendered.contains("Examples:"), "{topic:?} lacks examples");
        assert!(rendered.contains("See also:"), "{topic:?} lacks links");
    }
    for topic in [
        HelpTopic::Status,
        HelpTopic::Context,
        HelpTopic::Search,
        HelpTopic::Read,
        HelpTopic::Handle,
        HelpTopic::Check,
        HelpTopic::Describe,
        HelpTopic::Schema,
    ] {
        assert!(
            topic.render().contains("Also: `anneal describe"),
            "{topic:?} hides its runtime-name collision"
        );
    }
}

#[test]
fn retired_teaching_commands_point_to_describe_and_eval() {
    for (command, expected) in [
        ("cookbook", "folded into `anneal describe NAME`"),
        ("vocab", "folded into Code Mode queries"),
        ("verbs", "folded into introspection"),
        ("examples", "folded into `anneal describe NAME`"),
        ("save", "edit anneal.dl directly"),
        ("impact", "handle <HANDLE> --impact"),
        ("find", "h contains \"TEXT\""),
        ("get", "anneal handle <HANDLE>"),
        ("map", "*edge{from: src, to: dst, kind: kind}"),
        (
            "health",
            "diagnostic{code: code, severity: severity, subject: h, file: file, line: line}",
        ),
        ("diff", "at(\"snapshot:last\")"),
        (
            "obligations",
            "undischarged(h), obligation(h), *handle{id: h, file: file, status: status}",
        ),
        ("garden", "primary_entropy"),
        ("orient", "recent_frontier"),
        ("query", "use the language directly"),
        (
            "explain",
            "diagnostic{code: code, subject: h, file: file, line: line}",
        ),
        ("work", "frontier(h, energy)"),
        ("blocked", "blocker(h, energy, source)"),
        (
            "diagnostics",
            "diagnostic(code, severity, subject, file, line, evidence)",
        ),
        (
            "broken",
            "diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}",
        ),
        (
            "areas",
            "area_health(area, grade, files, errors, cross_edges)",
        ),
        ("trend", "at(\"snapshot:last\")"),
        ("sources", "sources(name, recognizes, capabilities, doc)"),
    ] {
        let err = Invocation::parse(os(&["anneal", command]))
            .expect_err("retired command should teach replacement");
        assert!(err.to_string().contains(expected), "{command}: {err}");

        let err = Invocation::parse(os(&["anneal", "help", command]))
            .expect_err("retired help topic should teach same replacement");
        assert!(err.to_string().contains(expected), "help {command}: {err}");
    }
}
