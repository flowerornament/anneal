//! Renders describe cards and derives output-column annotations from examples.

use super::DescribeKind;

#[derive(Default)]
/// Structured teaching-card inputs before their canonical prose projection.
pub(super) struct DescribeCard<'a> {
    pub(super) summary: &'a str,
    pub(super) kind: Option<DescribeKind>,
    pub(super) signature: Option<&'a str>,
    pub(super) relationship: Option<&'a str>,
    pub(super) common_joins: &'a [&'a str],
    pub(super) requires: &'a [&'a str],
    pub(super) see_also: &'a [&'a str],
    pub(super) examples: Vec<&'a str>,
    pub(super) extra_lines: Vec<String>,
}

/// Renders one card and derives output columns from executable query examples.
pub(super) fn describe_card(card: DescribeCard<'_>) -> String {
    // `describe(name, doc)` is the prose teaching surface. Machine callers should
    // use schema/source_of/examples for the same facts as structured relations.
    let mut lines = Vec::new();
    lines.push(card.summary.trim().to_string());
    if let Some(kind) = card.kind {
        lines.push(format!("Kind: {}.", kind.label()));
    }
    if let Some(signature) = card.signature {
        lines.push(format!("Signature: {signature}."));
    }
    if let Some(relationship) = card.relationship {
        lines.push(format!("Relationship: {relationship}"));
    }
    if !card.common_joins.is_empty() {
        lines.push("Common joins:".to_string());
        for join in card.common_joins {
            lines.push(format!("- {}", with_output_shape(join)));
        }
    }
    lines.extend(card.extra_lines);
    for requirement in card.requires {
        lines.push(format!("Requires: {requirement}"));
    }
    if !card.see_also.is_empty() {
        lines.push(format!("See also: {}.", card.see_also.join(", ")));
    }
    for example in card.examples {
        lines.push(format!("Example: {}", with_output_shape(example)));
    }
    lines.join("\n")
}

fn with_output_shape(text: &str) -> String {
    let columns = projected_columns(text);
    if columns.is_empty() {
        return text.to_string();
    }
    format!("{text} -> Output: {}", columns.join(", "))
}

fn projected_columns(text: &str) -> Vec<String> {
    let fragment = query_fragment(text).trim();
    if !is_output_shape_candidate(fragment) {
        return Vec::new();
    }
    let fragment = strip_string_literals(fragment);
    let chars = fragment.chars().collect::<Vec<_>>();
    let mut columns = Vec::<String>::new();
    let mut index = 0;
    while index < chars.len() {
        if !is_ident_start(chars[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < chars.len() && is_ident_continue(chars[index]) {
            index += 1;
        }
        let token = chars[start..index].iter().collect::<String>();
        let next = next_non_ws(&chars, index);
        if matches!(next, Some(':' | '(' | '{')) || is_reserved_token(&token) {
            continue;
        }
        if !columns.iter().any(|column| column == &token) {
            columns.push(token);
        }
    }
    columns
}

fn is_output_shape_candidate(fragment: &str) -> bool {
    !fragment.starts_with("anneal ")
        && (fragment.starts_with('?')
            || fragment.starts_with('*')
            || fragment.contains(":=")
            || fragment.contains('{')
            || fragment.contains('('))
}

fn query_fragment(text: &str) -> &str {
    if let Some(start) = text.find('`')
        && let Some(end) = text[start + 1..].find('`')
    {
        return &text[start + 1..start + 1 + end];
    }
    text
}

fn strip_string_literals(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            out.push(' ');
        } else if ch == '"' {
            in_string = true;
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn next_non_ws(chars: &[char], index: usize) -> Option<char> {
    chars
        .iter()
        .skip(index)
        .copied()
        .find(|ch| !ch.is_whitespace())
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_reserved_token(token: &str) -> bool {
    matches!(
        token,
        "not"
            | "in"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "matches"
            | "true"
            | "false"
            | "null"
    )
}
