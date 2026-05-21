---
name: anneal
description: "Orient in knowledge corpora, retrieve relevant context, inspect graph structure, track convergence, and ask Datalog queries over anneal facts. Use when a repo has `.design/`, `docs/`, or `anneal.dl`, or the user asks about convergence, blockers, broken refs, what changed, or what depends on X."
metadata:
  short-description: Query knowledge corpora with anneal
---

# Anneal

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
facts, loads the standard library and project `anneal.dl`, and exposes a small
ladder: arrive, discover the language, retrieve evidence, then use Datalog for
precise composite questions.

Run `anneal help <command>` for exact flags. Do not guess CLI details from
memory when a command matters.

Runtime commands render readable text at a terminal and JSON/NDJSON when piped.
In pipe-only agent harnesses, add `--format=text` when you want to read the
answer directly.

If this skill is not preloaded, run `anneal prime` to print the shipped
briefing from the installed binary.

## First Moves

Pick the smallest surface that can answer the next agent question.

### Arriving Cold

```bash
anneal context "<goal>" --hits 5 --budget 8000 --format=text
anneal status --format=text
```

Use `context` when the user gives a concrete goal and you need search hits,
graph neighborhood, and read spans in one call. Its `--budget` derives a
per-hit read cap that is applied independently to each winning hit. Use
`--hits` to choose the number of search winners; `--limit` is also accepted as
an alias. Use `anneal status` when the question is corpus state.

### Discovering The Language

```bash
anneal schema --format=text
anneal describe search --format=text
anneal cookbook --format=text
anneal verbs --format=text
anneal vocab --format=text
anneal sources --format=text
```

Use these before inventing names. `schema` shows queryable relations and
signatures. `describe` explains one primitive, predicate, or verb. `cookbook`
shows worked join recipes by question shape. `verbs` shows saved query examples
from the prelude and project. `vocab` shows observed status values, edge kinds,
namespaces, and frontmatter fields. `sources` shows linked adapters and
capabilities.

### Finding and Reading

```bash
anneal search "<text>" --limit 5 --format=text
anneal read <handle> --budget 4000 --format=text
anneal handle <handle> --format=text
```

Use `search` for content retrieval. It handles light stemming and common
planning abbreviations such as OQ/open question, ADR, and RFC. Use `read` after
search or when the handle is known. Use `handle` when relationship shape
matters; `H` is a short alias.
Empty NDJSON row streams emit `(0 rows)` on stderr while leaving stdout empty
for pipes.

### Asking a Precise Question

```bash
anneal -e '? *handle{id: h, kind: "file", status: s}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? upstream("formal-model/v17.md", h).'
anneal save broken-area '? diagnostic{subject: h}, area_of{h: h, area: area}.' \
  --args area:String --doc 'Diagnostics in one area.'
```

Use `anneal save` when an eval query becomes a reusable project move. It writes
a normal `@verb` declaration to `anneal.dl`, so the next session can call it as
`anneal broken-area language` and inspect it with `anneal verbs` or
`anneal describe broken-area`. If the saved query is wrong, remove the
generated `@verb(...)` block from `anneal.dl` or rerun `anneal save ... --force`
to replace it.

Use raw Datalog when the built-in verbs are too broad. Stored relations use
`*` prefixes; prelude and project predicates do not. `anneal -e -` reads a
query from stdin when a scratch file is clearer than a one-liner.

Use `--explain` when you need provenance for why a row exists. It explains the
first 3 rows by default; use `--explain-first N` for a different cap or
`--explain-all` only when you really want every row's derivation tree.

### Config and Extensions

The ladder is:

- built-in prelude: automatic standard rules and verbs
- `anneal.dl`: repo-local project declarations for adapter discovery, statuses,
  lattice, namespaces, frontmatter edges, excludes, rules, and `@verb`s
- user config: machine-local preferences under XDG config

Label namespaces are inferred from corpus evidence. Do not maintain a manual
namespace inventory. Project config carries namespace policy only:

- `linear([...])`: obligation prefixes whose labels must be discharged exactly
  once
- `rejected([...])`: false-positive prefixes such as hashes or all-caps words
- `force([...])`: rare sparse prefixes that should count as labels before they
  have enough examples

Do not copy the built-in prelude into a project. Use `anneal init --dry-run` to
inspect the current `anneal.dl` scaffold. `anneal init` refuses to overwrite an
existing config unless `--force` is passed; for older installs, `--force`
writes unified `anneal.dl` and moves `anneal.toml` to `anneal.toml.legacy`.
Legacy `confirmed` namespace inventories are dropped during conversion. If an
existing `anneal.dl` still contains `confirmed(...)`, rerun `anneal init
--dry-run` to preview the cleaned config and `anneal init --force` to rewrite
it.

### Working The Convergence Frontier

```bash
anneal broken
anneal work
anneal trend  # emits rows when snapshot history exists; otherwise zero rows
```

Prefer runtime verbs for new workflows. Use `broken` for diagnostic blockers,
`work` for ranked active candidates, and `trend` when snapshot history exists.

## Command Map

### Arrive

- `anneal context GOAL`: grouped cold-agent context from search, read, and
  neighborhood
- `anneal status`: compact corpus status
- `anneal prime`: bundled agent briefing from the installed binary

### Discover The Language

- `anneal schema`: predicates, primitives, stored relations, and signatures
- `anneal describe NAME`: docs for one runtime name
- `anneal cookbook`: worked recipes for common question shapes
- `anneal verbs`: saved query examples from the prelude and project
- `anneal vocab`: observed status, edge, namespace, and metadata vocabulary
- `anneal sources`: linked adapters and capabilities

### Retrieve Evidence

- `anneal search TEXT`: ranked content hits with handle, span, score, reason,
  field, and low-confidence marker
- `anneal read HANDLE`: bounded content spans
- `anneal handle HANDLE`: handle neighborhood

### Work The Convergence Frontier

- `anneal work`: ranked work candidates
- `anneal areas`: per-area health grades and frontier work
- `anneal blocked HANDLE`: blockers for one handle
- `anneal broken`: diagnostic gate
- `anneal trend`: convergence movement rows when snapshot history exists; no-history
  corpora emit zero rows

Project `@verb` declarations in `anneal.dl` appear beside these in
`anneal verbs` and are callable by name. Use `anneal -e` for custom composition
when you need a parameterized or one-off query. Use `anneal save` to promote a
working query into a reusable project verb.

### Raw Query Surface

```bash
anneal -e '? *handle{id: h, kind: "label", status: "open"}.'
anneal -e '? *edge{from: src, to: dst, kind: "DependsOn"}.'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
anneal -e '? read("formal-model/v17.md", 4000, span, text, start, end, tokens).'
anneal save stale-active '? freshness(h, days), days > 30, active(h).' \
  --doc 'Old active handles.'
```

Common stored relations:

- `*handle`
- `*edge`
- `*meta`
- `*content`
- `*span`
- `*concern`
- `*config`
- `*snapshot`
- `*generation`

Common prelude families:

- graph: `upstream`, `downstream`, `impact`, `neighborhood`
- convergence: lifecycle position, entropy, blocked, advancing
- checks: `diagnostic`
- ranking: `search`, `top_work`, `top_k` helpers
- views: callable starter verbs

## Agent Rules

- Start with `anneal context "<goal>"` for goal-oriented work.
- Use `search` then `read` when you need tighter control over retrieval.
- Use `sources`, `verbs`, `schema`, and `describe` before writing a custom
  query against unfamiliar vocabulary.
- Use `anneal -e` for composite questions. Keep queries narrow and project only
  fields you need. Add `--limit N` while exploring broad predicates.
- Use `--json` or NDJSON streams for tool consumption. Runtime commands render
  readable text at a terminal; use `--format=text` to force that renderer
  through pipe-only harnesses.
- Use `--root` for the corpus path. Use `--area` only for an area name inside
  that corpus, usually a top-level directory or configured concern group.
- Use hidden compatibility commands such as `health`, `check`, `get`, `find`,
  `map`, `impact`, `diff`, `garden`, and `obligations` only when exact
  pre-0.11.0 behavior is required.
- After editing corpus files, run `anneal broken`. Use `anneal check
  --scope=active` only when you are deliberately exercising the compatibility
  surface or CI gate.
- If a command returns too much, rerun with a lower `--limit`, smaller
  `--budget`, or a more specific query.

## Project Extension

`anneal.dl` at the corpus root can add discovery facts, project predicates, and
verbs.

```dl
source md {
  file_extension(".md").
  scan_root(".").
  scan_exclude("node_modules").
}

release_blocker(h, "broken_ref") :=
  diagnostic("E001", severity, h, file, line, evidence).

@verb(
  name: "release-blockers",
  query: "release_row(h, why, milestone) :=
    verb_arg(\"milestone\", milestone),
    release_blocker(h, why),
    *meta{handle: h, key: \"milestone\", value: milestone}.

    ? release_row(h, why, milestone).",
  doc: "Open blockers for the next release.",
  output_schema: "{\"h\":\"HandleId\",\"why\":\"String\",\"milestone\":\"String\"}",
  args: ["milestone:String"],
  capabilities: ["read"]
).
```

Discovery facts are consumed by adapters before extraction. Rules and verbs are
loaded after source facts exist. Project predicates shadow standard-library
predicates by name and arity.

## Mental Model

- `handle`: a file, section, label, version, or external reference
- `source`: an adapter such as markdown, code, host runtime, or issue tracker
- `relation`: a stored row emitted by a source or a derived row produced by rules
- `verb`: a named query with documentation, schema, and capabilities
- `generation`: a source refresh epoch used for atomic fact replacement
- `visibility`: fact-level access envelope applied before derivation

You do not need the full language in your head. Query the runtime first; extend
it only when a goal needs vocabulary the corpus does not yet have.
