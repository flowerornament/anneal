use super::*;
use anyhow::Context;
use std::collections::BTreeSet;

#[test]
fn shipped_help_and_describe_examples_are_executable() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().join("corpus")).expect("utf8 tempdir");
    fs::create_dir(&root).expect("create corpus root");
    fs::write(root.join("a.md"), "# A\n").expect("write corpus document");
    let session = RuntimeSession::load_for_test(&root).expect("session loads");

    let mut failures = validate_static_help(&session);
    failures.extend(validate_describe_cards(&session));
    failures.sort();
    failures.dedup();

    assert!(
        failures.is_empty(),
        "help/describe executable documentation drifted; fix every taught command, \
         query, or config example.\nFull failures:\n{failures:#?}"
    );
}

fn validate_static_help(session: &RuntimeSession) -> Vec<String> {
    let mut failures = Vec::new();
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
        validate_program_fragments(&format!("help {topic:?}"), &rendered, &mut failures);
        for command in command_examples(&rendered) {
            validate_command(session, &format!("help {topic:?}"), command, &mut failures);
        }
    }

    let agent = HelpTopic::Agent.render();
    validate_program_fragments("help agent", &agent, &mut failures);
    for command in fenced_lines(&agent, "bash") {
        if command.starts_with("anneal ") {
            validate_command(session, "help agent", command, &mut failures);
        }
    }
    for (index, program) in fenced_blocks(&agent, "dl").into_iter().enumerate() {
        let source = format!("help agent config example {}", index + 1);
        if let Err(error) = validate_project_example(session, program) {
            failures.push(format!(
                "{source} does not load against the runtime vocabulary: {error:#}"
            ));
        }
    }
    failures
}

fn validate_project_example(session: &RuntimeSession, program: &str) -> anyhow::Result<()> {
    let project_file = session.root().join(anneal_core::PROJECT_RULE_FILE);
    fs::write(&project_file, program).context("write project example")?;
    let result = RuntimeSession::load_for_test(session.root()).map(|_| ());
    fs::remove_file(project_file).context("remove project example")?;
    result
}

fn validate_describe_cards(session: &RuntimeSession) -> Vec<String> {
    let output = session
        .eval("? describe(name, doc).", ExplainOptions::disabled())
        .expect("describe catalog evaluates");
    let cards = output
        .rows
        .iter()
        .map(|row| {
            (
                required_string(row, "name")
                    .expect("describe name")
                    .to_string(),
                required_string(row, "doc")
                    .expect("describe doc")
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    let described_names = cards
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    let mut failures = Vec::new();

    for (name, doc) in &cards {
        validate_program_fragments(&format!("describe {name}"), doc, &mut failures);

        for line in doc.lines() {
            let trimmed = line.trim();
            if let Some(example) = trimmed.strip_prefix("Example: ") {
                let example = strip_output_shape(example);
                if example.starts_with("anneal ") {
                    validate_command(
                        session,
                        &format!("describe {name} example"),
                        example,
                        &mut failures,
                    );
                } else {
                    validate_query(
                        session,
                        &format!("describe {name} example"),
                        example,
                        &mut failures,
                    );
                }
            } else if let Some(join) = trimmed.strip_prefix("- `")
                && let Some((fragment, _)) = join.split_once('`')
                && (fragment.contains('(') || fragment.contains('{'))
            {
                let query = format!("? {}.", fragment.trim_end_matches('.'));
                validate_query(
                    session,
                    &format!("describe {name} common join"),
                    &query,
                    &mut failures,
                );
            }

            if let Some(rest) = trimmed.split_once("wrap it in `").map(|(_, rest)| rest)
                && let Some((callable, _)) = rest.split_once('`')
                && !described_names.contains(callable)
            {
                failures.push(format!(
                    "describe {name}: `{callable}` is taught as a callable runtime name"
                ));
            }
        }
    }
    failures
}

fn validate_command(
    session: &RuntimeSession,
    source: &str,
    command: &str,
    failures: &mut Vec<String>,
) {
    let args = match shell_words(command) {
        Ok(args) => args,
        Err(error) => {
            failures.push(format!(
                "{source}: `{command}` cannot be tokenized: {error}"
            ));
            return;
        }
    };
    let parsed = match Invocation::parse(args.into_iter().map(OsString::from).collect()) {
        Ok(parsed) => parsed,
        Err(error) => {
            failures.push(format!("{source}: `{command}` does not parse: {error}"));
            return;
        }
    };
    if let RuntimeCommand::Eval { query, .. } = parsed.command
        && query != "-"
    {
        validate_query(session, source, &query, failures);
    }
}

fn validate_query(session: &RuntimeSession, source: &str, query: &str, failures: &mut Vec<String>) {
    if let Err(error) = session.analyze_query_for_test(query) {
        failures.push(format!(
            "{source}: `{query}` does not analyze against the runtime vocabulary: {error:#}"
        ));
    }
}

fn validate_program_fragments(source: &str, text: &str, failures: &mut Vec<String>) {
    for query in query_fragments(text) {
        if let Err(error) = parse_program(source, &query) {
            failures.push(format!("{source}: `{query}` does not parse: {error}"));
        }
    }
}

fn command_examples(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("anneal "))
        .filter(|line| !line.starts_with("anneal [OPTIONS]"))
        .filter(|line| !line.contains(" remains "))
        .collect()
}

fn query_fragments(text: &str) -> Vec<String> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut queries = Vec::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        let (start, ch) = chars[cursor];
        if ch != '?' || !text[start..].starts_with("? ") {
            cursor += 1;
            continue;
        }
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;
        for &(index, current) in &chars[cursor + 1..] {
            if in_string {
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == '"' {
                    in_string = false;
                }
            } else if current == '"' {
                in_string = true;
            } else if current == '.' {
                end = Some(index + current.len_utf8());
                break;
            }
        }
        let Some(end) = end else {
            break;
        };
        queries.push(text[start..end].to_string());
        cursor = chars.partition_point(|(index, _)| *index < end);
    }
    queries
}

fn fenced_blocks<'a>(markdown: &'a str, language: &str) -> Vec<&'a str> {
    let marker = format!("```{language}\n");
    let mut rest = markdown;
    let mut blocks = Vec::new();
    while let Some((_, after_marker)) = rest.split_once(&marker) {
        let Some((block, after_block)) = after_marker.split_once("\n```") else {
            break;
        };
        blocks.push(block);
        rest = after_block;
    }
    blocks
}

fn fenced_lines<'a>(markdown: &'a str, language: &str) -> Vec<&'a str> {
    fenced_blocks(markdown, language)
        .into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn strip_output_shape(text: &str) -> &str {
    text.split_once(" -> Output:")
        .map_or(text, |(query, _)| query)
}

fn shell_words(command: &str) -> Result<Vec<String>, &'static str> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        match (quote, ch) {
            (Some('"'), '\\') => escaped = true,
            (Some(expected), current) if current == expected => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, current) if current.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (Some(_) | None, current) => word.push(current),
        }
    }
    if quote.is_some() {
        return Err("unclosed quote");
    }
    if escaped {
        return Err("trailing escape");
    }
    if !word.is_empty() {
        words.push(word);
    }
    if let Some(redirection) = words.iter().position(|word| word == "<") {
        words.truncate(redirection);
    }
    Ok(words)
}
