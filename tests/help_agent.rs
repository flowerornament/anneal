use std::fs;
use std::process::{Command, Output};

const SKILL_MARKDOWN: &str = include_str!("../skills/anneal/SKILL.md");

fn anneal(args: &[&str]) -> String {
    let output = anneal_output(args, None);

    assert!(
        output.status.success(),
        "anneal {args:?} failed with status {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("anneal {args:?} emitted non-utf8 stdout: {err}"))
}

fn anneal_output(args: &[&str], current_dir: Option<&std::path::Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anneal"));
    command.args(args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    command
        .output()
        .unwrap_or_else(|err| panic!("anneal {args:?} failed to run: {err}"))
}

fn product_thesis() -> &'static str {
    let marker = "## Product Thesis\n";
    let section = SKILL_MARKDOWN
        .split_once(marker)
        .expect("skill has product thesis")
        .1;
    section
        .split("\n## ")
        .next()
        .expect("thesis section")
        .trim()
}

#[test]
fn help_agent_matches_prime_briefing() {
    let help_agent = anneal(&["help", "agent"]);
    let prime = anneal(&["prime"]);

    assert_eq!(help_agent, prime);
    assert!(help_agent.contains("# Anneal"));
    assert!(help_agent.contains("## First Moves"));
    assert!(help_agent.contains("## Agent Rules"));
    assert!(!help_agent.starts_with("---"));
}

#[test]
fn top_and_agent_help_project_the_canonical_product_language() {
    let top = anneal(&["help"]);
    let agent = anneal(&["help", "agent"]);

    assert!(top.contains(product_thesis()));
    assert!(agent.contains(product_thesis()));
    assert!(top.lines().count() <= 60);
    assert!(top.lines().all(|line| line.len() <= 80));
    for word in ["convergence", "frontier", "settledness"] {
        assert!(top.contains(word), "top help lost product word {word:?}");
    }
    for word in [
        "convergence",
        "frontier",
        "potential",
        "entropy",
        "obligation",
        "discharged",
        "flow",
        "drifting",
        "provenance",
        "trail",
        "disposition",
    ] {
        assert!(
            agent.contains(word),
            "agent help lost product word {word:?}"
        );
    }
}

#[test]
fn semantic_help_is_the_describe_projection() {
    for format in ["text", "json"] {
        let help = anneal(&["--root=.design", "--format", format, "help", "convergence"]);
        let describe = anneal(&[
            "--root=.design",
            "--format",
            format,
            "describe",
            "convergence",
        ]);
        assert_eq!(help, describe);
    }

    let status = anneal(&["--root=.design", "--format=text", "status"]);
    assert!(status.contains("Convergence"));
    assert!(status.contains("drifting="));
}

#[test]
fn semantic_help_outside_a_corpus_teaches_root_recovery() {
    let root = std::env::temp_dir().join(format!("anneal-help-unmarked-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("create unmarked directory");

    let output = anneal_output(&["help", "runtime"], Some(&root));
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(!output.status.success());
    assert!(stderr.contains("help for runtime name"));
    assert!(stderr.contains("corpus-scoped"));
    assert!(stderr.contains("anneal help top"));
    assert!(stderr.contains("--root <path>"));

    fs::remove_dir_all(root).expect("remove unmarked directory");
}
