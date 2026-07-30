//! Canonical CLI help, agent briefing, and retirement guidance.

#[cfg(test)]
mod tests;

/// Shipped agent briefing projected by `anneal help agent`.
pub(super) const SKILL_MARKDOWN: &str = include_str!("../../../../skills/anneal/SKILL.md");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Static help pages available without loading a corpus.
pub(super) enum HelpTopic {
    Top,
    Agent,
    Init,
    Status,
    Context,
    Search,
    Read,
    Handle,
    Check,
    Describe,
    Schema,
    Eval,
}

impl HelpTopic {
    /// Resolve a static command or briefing name.
    pub(super) fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "top" => Self::Top,
            "agent" => Self::Agent,
            "init" => Self::Init,
            "status" => Self::Status,
            "context" => Self::Context,
            "search" => Self::Search,
            "read" => Self::Read,
            "handle" | "H" => Self::Handle,
            "check" => Self::Check,
            "describe" => Self::Describe,
            "schema" => Self::Schema,
            "eval" | "-e" | "--eval" => Self::Eval,
            _ => return None,
        })
    }

    /// Render the canonical static page for this topic.
    pub(super) fn render(self) -> String {
        if matches!(self, Self::Agent) {
            return skill_briefing_body(SKILL_MARKDOWN).to_string();
        }
        if matches!(self, Self::Top) {
            return render_top_help();
        }

        let body = match self {
            Self::Top => unreachable!("top help returns before static help rendering"),
            Self::Agent => unreachable!("agent help returns before static help rendering"),
            Self::Init => {
                "\
Usage: anneal [OPTIONS] init [OPTIONS]

Generate an anneal.dl project declaration from inferred markdown corpus
structure, or migrate an older anneal.toml to the unified runtime config.

Options:
      --dry-run                  Print the generated anneal.dl without writing
      --force                    Replace anneal.dl or migrate anneal.toml

Output: readable config preview at a terminal or with --format=text; JSON object when piped or with --json.

Use init when a directory is not yet marked. Runtime commands require either a
marked inferred root or an explicit --root PATH.

Examples:
  anneal init --dry-run
  anneal init

See also: `anneal help`, `anneal describe runtime`.
"
            }
            Self::Status => {
                "\
Usage: anneal [OPTIONS] status

Print compact corpus status from the programmable runtime.

Use this as the arrival command: it renders aggregate corpus vital signs and
copy-runnable orientation/work queries. For goal-less reading, run the
`recent_frontier` and `ranked_anchor` queries it prints; use `context GOAL`
once you have a specific goal.

Output: human summary at a terminal or with --format=text; NDJSON rows when piped or with --json.

Examples:
  anneal status --format=text
  anneal status --format=json

See also: `anneal context`, `anneal describe convergence`, `anneal help eval`.
Also: `anneal describe status` teaches the runtime verb with this name.
"
            }
            Self::Context => {
                "\
Usage: anneal [OPTIONS] context [OPTIONS] <GOAL>

Cold-agent orientation in one response. Composes summary-bearing span search,
bounded span metadata, and graph neighborhood. Use --read-spans to include matched
span bodies.

Arguments:
  <GOAL>                         Natural-language goal/query

Options:
      --budget <N>               Per-hit span selection cap; used for bodies with --read-spans
      --hits <N>                 Number of search winners (default: 3)
      --depth <N>                Alias for --neighborhood-depth
      --neighborhood-depth <N>   Graph distance around winners (default: 1)
      --include-low-confidence   Include low-confidence search hits
      --read-spans               Include matched span bodies in the output

Output: human summary at a terminal or with --format=text; NDJSON event rows when piped or with --json.

Examples:
  anneal context \"what should I read before changing releases?\"
  anneal context \"currency model\" --hits 5 --read-spans

See also: `anneal status`, `anneal search`, `anneal read`.
Also: `anneal describe context` teaches the runtime verb with this name.
"
            }
            Self::Search => {
                "\
Usage: anneal [OPTIONS] search [OPTIONS] <TEXT>

Ranked content search over handles and heading spans. Span hits include
summary metadata.

Arguments:
  <TEXT>                         Search query

Options:
      --limit <N>                Maximum rows (default: 25)
      --include-low-confidence   Include low-confidence hits

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.

Examples:
  anneal search \"convergence frontier\" --limit 5
  anneal search \"CR-D82\" --include-low-confidence

See also: `anneal context`, `anneal read`, `anneal describe relevance`.
Also: `anneal describe search` teaches the runtime verb and primitive with this name.
"
            }
            Self::Read => {
                "\
Usage: anneal [OPTIONS] read [OPTIONS] <HANDLE>

Read bounded content spans for a handle.

Arguments:
  <HANDLE>                       Handle id to read

Options:
      --budget <N>               Token budget (default: 4000)
      --span-id <ID>             Read one content span

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.

Examples:
  anneal read 2026-05-13-corpus-runtime.md --budget 4000
  anneal read 2026-05-13-corpus-runtime.md --span-id <SPAN_ID>

See also: `anneal search`, `anneal context`, `anneal handle`.
Also: `anneal describe read` teaches the runtime verb and primitive with this name.
"
            }
            Self::Handle => {
                "\
Usage: anneal [OPTIONS] handle [OPTIONS] <HANDLE>

Show one handle plus bounded incoming/outgoing references. Outgoing and
incoming edges are grouped by kind; in-repo code refs render in a dedicated
Code references section.

Arguments:
  <HANDLE>                       Handle id to inspect

Options:
      --impact                   Include direct/indirect reverse dependencies
      --lineage                  Include file supersession DAG and current head

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.

Examples:
  anneal handle 2026-05-13-corpus-runtime.md --impact
  anneal handle 2026-05-13-corpus-runtime.md --lineage

See also: `anneal read`, `anneal context`, `anneal describe structure`.
Also: `anneal describe handle` teaches the runtime verb with this name.
"
            }
            Self::Check { .. } => {
                "\
Usage: anneal [OPTIONS] check

Hidden CI gate for error-severity diagnostics.

Options:
      --refresh-drift            Refresh design-code drift evidence before checking

For filtered diagnostic questions, use eval:
  anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'
  anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'

Deprecation: hidden alias retained for CI muscle memory; prefer eval composition in agent-facing workflows.

Output: readable error diagnostics at a terminal or with --format=text; NDJSON rows when piped or with --json. Exits 1 when any error row exists.

Examples:
  anneal check
  anneal check --refresh-drift

See also: `anneal status`, `anneal describe diagnostic`, `anneal help eval`.
Also: `anneal describe check` teaches the hidden runtime verb with this name.
"
            }
            Self::Describe => {
                "\
Usage: anneal [OPTIONS] describe [NAME]

Describe a runtime primitive, predicate, or verb. Defaults to runtime.
Use `anneal describe runtime` for the compact map, then `anneal -e` for
composition.

Arguments:
  [NAME]                         Object to describe

Output: readable teaching cards by default, including when piped; use --json or --format=json for NDJSON rows.

Examples:
  anneal describe runtime
  anneal describe convergence

See also: `anneal schema`, `anneal help <name>`, `anneal help eval`.
Also: `anneal describe describe` teaches the runtime verb with this name.
"
            }
            Self::Schema => {
                "\
Usage: anneal [OPTIONS] schema

List runtime predicates, primitives, signatures, and provenance.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.

Examples:
  anneal schema --format=text
  anneal schema --format=json

See also: `anneal describe runtime`, `anneal help <name>`, `anneal help eval`.
Also: `anneal describe schema` teaches the runtime verb with this name.
"
            }
            Self::Eval => {
                "\
Usage: anneal [OPTIONS] -e [OPTIONS] <QUERY>
       anneal [OPTIONS] eval [OPTIONS] <QUERY>

Run a Datalog query against corpus facts. This is anneal's compositional
surface: use commands to orient, introspection to discover vocabulary, and
`-e` when you need a precise question.

Arguments:
  <QUERY>                        Query string

Options:
      --limit <N>                Cap returned rows after evaluation
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

Grammar tour:
  Queries ask for rows:
    ? predicate(arg), other(arg2).

  Stored relations are source/runtime facts. They use `*name{field: value}`:
    ? *handle{id: h, kind: \"file\", status: s}.
    ? *edge{from: src, to: dst, kind: \"DependsOn\"}.
    `id: h` binds a variable. `kind: \"file\"` filters to a literal.

  Derived predicates and primitives use complete call syntax:
    ? frontier(h, energy).
    ? search(query: \"conformance\", handle: h, span_id: span, score: score,
        reason: reason, field: field, low_confidence: low).

  Relation-pattern calls use braces when you only care about some fields:
    ? diagnostic{severity: \"error\", subject: h}.
    ? search{query: \"conformance\", handle: h, score: score}.
    ? diagnostic{subject: h}, area_of{h: h, area: \"language\"}.
    Omitted fields behave like hidden wildcards and are not output columns.

  Local rules name reusable subqueries before the final `?` query:
    open_file(h) := *handle{id: h, kind: \"file\"}, active(h).
    ? open_file(h).

  Negation uses `not` after variables are positively bound:
    missing_discharge(h) := obligation(h), not discharged(h).

  Aggregates bind tuples from grouped rows:
    area(area) := area_of(h, area).
    area_count(area, n) :=
      area(area),
      n = Count{ h : area_of(h, area) }.

    ? (h, energy) = TopK{ k: 10, key: energy :
        (h, energy) : potential(h, energy)
      }.

  Time blocks query supported historical references:
    ? at(\"snapshot:last\") { *handle{id: h, status: old} },
      *handle{id: h, status: now},
      old != now.
    Only snapshot references are supported today; git refs like at(\"HEAD~5\") remain pending.

  Stratification rule of thumb:
    recursive rules are fine; negation and aggregates must not depend on
    themselves through a cycle. If analysis rejects a query, split the negative
    or aggregate part into a later rule.

Migration recipes:
  Hidden CI gate:
    anneal check
    anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'
    `anneal check` exits 1 when any error row exists; use eval for filtered agent workflows.

  Retired obligations:
    anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'

  Retired diff:
    anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'

Goal-less orientation:
  Start with `anneal status`; it prints these copy-runnable queries:
    anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12
    anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12
  Use `anneal context \"GOAL\"` after you can name the goal.

Discover before guessing:
  anneal schema --format=text
  anneal describe runtime --format=text
  anneal describe search --format=text
  anneal -e '? source_of(\"frontier\", file, lines).'
  Unknown predicate and stored-field errors include nearby names and allowed fields.

Examples:
  anneal -e '? *handle{id: h, kind: \"file\", status: s}.' --limit 20
  anneal -e '? *edge{from: src, to: dst, kind: \"DependsOn\"}.'
  anneal -e '? search{query: \"conformance\", handle: h, span_id: span, score: score}, *span{handle: h, id: span, summary: summary}.' --limit 20
  anneal -e '? read{handle: \"docs/runtime-overview.md\", budget: 4000, text: text}.'
  anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12
  anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12
  anneal -e '? diagnostic{severity: \"error\", subject: h, file: file}.'
  anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
  anneal -e '? changed_within(h, 7), *handle{id: h, kind: \"file\"}, search{query: \"conformance\", handle: h}.'
  anneal -e '? source_of(\"frontier\", file, lines).'
  anneal -e - < query.dl

See also: `anneal schema`, `anneal describe runtime`, `anneal help agent`.

Output: readable rows at a terminal or with --format=text; NDJSON rows when piped or with --json.
"
            }
        };
        if matches!(self, Self::Eval | Self::Check { .. }) {
            format!("{body}{RUNTIME_HELP_OPTIONS}")
        } else {
            format!("{body}{RUNTIME_PROVENANCE_OPTIONS}{RUNTIME_HELP_OPTIONS}")
        }
    }
}

fn render_top_help() -> String {
    let thesis = skill_section(SKILL_MARKDOWN, "Product Thesis")
        .expect("shipped anneal skill must define the Product Thesis section");
    format!(
        "\
Usage: anneal [OPTIONS] [COMMAND]

{thesis}

Run `anneal help <name>` for command details or a runtime teaching card.

Commands by intent:
  Arrive
    anneal status                 Corpus vital signs and convergence frontier
    anneal context \"goal\"         Goal-shaped retrieval and graph neighborhood
  Retrieve
    anneal search \"text\"          Ranked handle and span search
    anneal read <handle>          Budgeted evidence for one handle
    anneal handle <handle>        Relationships, impact, and lineage
  Discover and program
    anneal schema                 Callable runtime vocabulary and signatures
    anneal describe <name>        Purpose, joins, examples, and requirements
    anneal -e '? predicate(args).' Compose a precise Datalog question
  Configure
    anneal init                   Preview or write an anneal.dl project file

Converge:
  anneal describe convergence
  anneal -e '? frontier(h, energy).'

More help:
  anneal help agent
  anneal help <command-or-runtime-name>

Root premise:
  Run from a marked corpus (.design, docs, or anneal.dl), pass --root PATH,
  or use anneal init --dry-run to preview a project file.

{TOP_HELP_OPTIONS}"
    )
}

const TOP_HELP_OPTIONS: &str = "\
Global options:
      --root <PATH>              Corpus root
                                 (default: nearest .design, docs,
                                 or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json|ndjson>
                                 Force readable text or JSON/NDJSON output
";

const RUNTIME_PROVENANCE_OPTIONS: &str = "\
Provenance options:
      --explain                  Include derivation trees for first 3 rows
      --explain-first <N>        Include derivation trees for first N rows
      --explain-all              Include derivation trees for every row
      --explain-depth <N>        Derivation expansion depth

";

const RUNTIME_HELP_OPTIONS: &str = "\
Global options:
      --root <PATH>              Corpus root (default: nearest .design, docs, or anneal.dl upward)
      --json                     Force JSON/NDJSON output
      --format <text|json|ndjson> Force readable text or JSON/NDJSON output
";

/// Teach the supported replacement for a retired command name.
pub(super) fn retired_command_message(command: &str) -> Option<&'static str> {
    match command {
        "cookbook" => Some(
            "anneal cookbook was folded into `anneal describe NAME`; use `anneal describe diagnostic` for worked joins or `anneal help eval` for query recipes",
        ),
        "vocab" => Some(
            "anneal vocab was folded into Code Mode queries; use `anneal describe runtime` for vocabulary recipes or `anneal -e '? *handle{status: status}.'`",
        ),
        "verbs" => Some(
            "anneal verbs was folded into introspection; use `anneal schema --format=text`, `anneal describe NAME`, or `anneal -e '? verbs(name, query, doc, output_schema).'",
        ),
        "examples" => Some(
            "anneal examples was folded into `anneal describe NAME`; use `anneal describe search` or query `examples(name, example)` with `anneal -e`",
        ),
        "save" => Some(retired_save_message()),
        "impact" => Some(
            "anneal impact has been retired; use `anneal handle <HANDLE> --impact` or compose `anneal -e '? impact(\"HANDLE\", affected, depth).'`",
        ),
        "find" => Some(
            "anneal find has been retired; use `anneal search TEXT` for content retrieval or compose `anneal -e '? *handle{id: h, kind: kind, status: status}, h contains \"TEXT\".'` for identity matching",
        ),
        "get" => Some(
            "anneal get has been retired; use `anneal handle <HANDLE>` for handle metadata and edges, or `anneal read <HANDLE>` for bounded content",
        ),
        "map" => Some(
            "anneal map has been retired; compose graph questions with `anneal -e '? *edge{from: src, to: dst, kind: kind}.'` or use `anneal handle <HANDLE>` for a local neighborhood",
        ),
        "health" => Some(
            "anneal health has been retired; use `anneal status` for the convergence header and compose diagnostics with `anneal -e '? diagnostic{code: code, severity: severity, subject: h, file: file, line: line}.'`",
        ),
        "diff" => Some(
            "anneal diff has been retired; use automatic status snapshots with `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`",
        ),
        "obligations" => Some(
            "anneal obligations has been retired; compose `anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'` or inspect `anneal describe undischarged`",
        ),
        "garden" => Some(
            "anneal garden has been retired; compose `frontier`, `primary_entropy`, and `*handle` with `anneal -e '? frontier(h, energy), primary_entropy(h, source), *handle{id: h, file: file, summary: summary}.'`, starting from `anneal status`",
        ),
        "orient" => Some(
            "anneal orient has been retired; start with `anneal status`, then run its `recent_frontier` and `ranked_anchor` queries for goal-less orientation or `anneal context \"GOAL\"` once you have a goal",
        ),
        "query" => Some(
            "anneal query has been retired; use the language directly with `anneal -e '? *handle{id: h}.'`",
        ),
        "explain" => Some(
            "anneal explain has been retired; use provenance on eval with `anneal -e '? diagnostic{code: code, subject: h, file: file, line: line}.' --explain`",
        ),
        "work" => Some(
            "anneal work has been retired; use `anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'` for ranked work, or `anneal status` for the convergence landing",
        ),
        "blocked" => Some(
            "anneal blocked has been retired; use `anneal -e '? blocker(h, energy, source), *handle{id: h, file: file, status: status}.'` or add `h = \"HANDLE\"` for a focused view",
        ),
        "diagnostics" => Some(
            "anneal diagnostics has been retired; use `anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'` for the full diagnostic stream or `anneal check` for the error-only CI gate",
        ),
        "broken" => Some(
            "anneal broken has been retired; use `anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'` for blockers or `anneal check` for the CI gate",
        ),
        "areas" => Some(
            "anneal areas has been retired; use `anneal -e '? area_health(area, grade, files, errors, cross_edges).'` or `anneal -e '? area_frontier(area, h, score, why).'`",
        ),
        "trend" => Some(
            "anneal trend has been retired; use `anneal -e '? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'` for status changes between snapshots",
        ),
        "sources" => Some(
            "anneal sources has been retired; use `anneal -e '? sources(name, recognizes, capabilities, doc).'`",
        ),
        _ => None,
    }
}

/// Strip the skill frontmatter from the shipped agent briefing.
pub(super) fn skill_briefing_body(markdown: &str) -> &str {
    let trimmed = markdown.trim_start_matches(['\u{feff}']);
    let Some(rest) = trimmed.strip_prefix("---\n") else {
        return trimmed;
    };
    let Some(end) = rest.find("\n---\n") else {
        return trimmed;
    };
    rest[end + "\n---\n".len()..].trim_start_matches('\n')
}

/// Select one second-level section from the shipped agent briefing.
pub(super) fn skill_section<'a>(markdown: &'a str, heading: &str) -> Option<&'a str> {
    let body = skill_briefing_body(markdown);
    let marker = format!("## {heading}\n");
    let section = body.split_once(&marker)?.1;
    Some(section.split("\n## ").next().unwrap_or(section).trim())
}

/// Teach the project-declaration replacement for the retired save command.
pub(super) fn retired_save_message() -> &'static str {
    "anneal save has been retired; edit anneal.dl directly and add an @verb(...) declaration, then verify with `anneal describe <name>` and a direct invocation"
}
