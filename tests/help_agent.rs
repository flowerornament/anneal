use std::process::Command;

fn anneal(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_anneal"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("anneal {args:?} failed to run: {err}"));

    assert!(
        output.status.success(),
        "anneal {args:?} failed with status {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("anneal {args:?} emitted non-utf8 stdout: {err}"))
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
