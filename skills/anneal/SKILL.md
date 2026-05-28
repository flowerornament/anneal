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

If this skill is not preloaded, run `anneal help agent` to print the shipped
briefing from the installed binary. The hidden `anneal prime` alias remains for
installed skill loaders and muscle memory.

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
anneal describe runtime --format=text
```

Use these before inventing names. `schema` shows queryable relations and
signatures. `describe NAME` explains one primitive, predicate, or verb,
including examples and common joins. `describe runtime` is the compact command
map plus vocabulary recipes. Query observed vocabulary directly with `-e`, for
example `? *handle{status: status}.`, `? *edge{kind: kind}.`, or
`? *handle{namespace: ns}.`. Adapter information is queryable through
`? sources(name, recognizes, capabilities, doc).`.

### Finding and Reading

```bash
anneal search "<text>" --limit 5 --format=text
anneal read <handle> --budget 4000 --format=text
anneal handle <handle> --format=text
anneal handle <handle> --impact --format=text
```

Use `search` for content retrieval. It handles light stemming and common
planning abbreviations such as OQ/open question, ADR, and RFC. Use `read` after
search or when the handle is known. Use `handle` when relationship shape
matters. Add `--impact` before editing when you need direct and indirect
reverse dependencies.
Empty NDJSON row streams emit `(0 rows)` on stderr while leaving stdout empty
for pipes.

### Asking a Precise Question

```bash
anneal -e '? *handle{id: h, kind: "file", status: s}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? upstream("formal-model/v17.md", h).'
anneal -e '? diagnostic{subject: h}, area_of{h: h, area: "language"}.'
```

When an eval query becomes a reusable project move, edit `anneal.dl` and add a
normal `@verb` declaration. The next session can call it by name and inspect it
with `anneal describe <name>`.

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
anneal -e '? diagnostic{severity: "error"}.'                                          # blockers
anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'      # ranked work
anneal -e '? area_health(area, grade, files, errors, cross_edges).'                   # area drill-down
```

Convergence is composed via the prelude vocabulary. Use `describe potential`,
`describe entropy`, `describe blocked`, `describe area_health` to learn the
Common joins, then compose with `-e`. `check` remains as a hidden CI gate
alias for the error-only filtered view.

## Command Map

### Arrive

- `anneal context GOAL`: grouped cold-agent context from search, read, and
  neighborhood
- `anneal status`: compact corpus status
- `anneal help agent`: bundled agent briefing from the installed binary
- `anneal prime`: hidden legacy alias for skill loader compatibility

### Discover The Language

- `anneal schema`: predicates, primitives, stored relations, and signatures
- `anneal describe NAME`: docs for one runtime name (includes Common joins)
- `anneal describe runtime`: compact command map and vocabulary recipes

### Retrieve Evidence

- `anneal search TEXT`: ranked content hits with handle, span, score, reason,
  field, and low-confidence marker
- `anneal read HANDLE`: bounded content spans
- `anneal handle HANDLE`: handle neighborhood
- `anneal handle HANDLE --impact`: handle neighborhood plus reverse dependencies

### Work The Convergence Frontier

Compose with `anneal -e` over prelude vocabulary:

- `? diagnostic{severity: "error"}.`: blockers (error-only filtered view)
- `? work_candidate(h, energy), entropy(h, source).`: raw work energy and cause
- `? frontier(h, energy), *handle{id: h, file: file}.`: ranked active work
- `? area_health(area, grade, files, errors, cross_edges).`: per-area health
- `? blocker(h, energy, source), h = "HANDLE".`: why one handle is stalled
- `? changed_within(h, 7), *handle{id: h, summary: summary}.`: handles changed in the last week
- `? *handle{id: h, file: f}, git_mtime(f, t).`: git-backed file change time

Project `@verb` declarations in `anneal.dl` appear in `schema` and are callable
by name. Edit `anneal.dl` directly to promote a working query into a reusable
project verb. The `check` command remains as a hidden CI gate alias for
the error-only diagnostic view.

Retired compatibility commands return teaching recovery messages. Translate old
workflows into Code Mode directly:

- `find TEXT`: `? *handle{id: h, kind: kind, status: status}, h contains "TEXT".`
- `get H`: `anneal handle H` or `anneal read H`
- `map`: `? *edge{from: src, to: dst, kind: kind}.`
- `health`: `anneal status` plus `? diagnostic{severity: severity, subject: h}.`
- `diff`: `? at("snapshot:last") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.`
- `obligations`: `? undischarged(h), obligation(h).`
- `garden`: `anneal status` plus `? frontier(h, energy), entropy(h, source).`
- `orient`: `anneal context "GOAL"` or `anneal handle H --impact`
- `impact H`: `anneal handle H --impact`
- `work`: `anneal status` or `? frontier(h, energy), *handle{id: h, file: file, summary: summary}.`
- `blocked H`: `anneal handle H` or `? blocker(h, energy, source), h = "H".`
- `diagnostics`: `? diagnostic(code, severity, subject, file, line, evidence).`
- `broken`: `? diagnostic{severity: "error"}.` or `anneal check`
- `areas`: `? area_health(area, grade, files, errors, cross_edges).`
- `trend`: `? at("snapshot:last") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.`
- `sources`: `? sources(name, recognizes, capabilities, doc).`
- `query`: `anneal -e`
- `explain`: `anneal -e '...' --explain`

### Raw Query Surface

```bash
anneal -e '? *handle{id: h, kind: "label", status: "open"}.'
anneal -e '? *edge{from: src, to: dst, kind: "DependsOn"}.'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
anneal -e '? read("formal-model/v17.md", 4000, span, text, start, end, tokens).'
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
- convergence: lifecycle position, entropy, frontier, blockers, advancing, recent changes
- checks: `diagnostic`
- ranking: `search`, `work_candidate`, `frontier`, `top_k` helpers
- views: callable starter verbs

## Agent Rules

- Start with `anneal context "<goal>"` for goal-oriented work.
- Use `search` then `read` when you need tighter control over retrieval.
- Use `schema` and `describe NAME` before writing a custom query against
  unfamiliar vocabulary. `describe NAME` shows Common joins inline.
- Use `anneal -e` for composite questions. Keep queries narrow and project only
  fields you need. Add `--limit N` while exploring broad predicates.
- Use `--json` or NDJSON streams for tool consumption. Runtime commands render
  readable text at a terminal; use `--format=text` to force that renderer
  through pipe-only harnesses.
- Use `--root` for the corpus path. Filter inside the query (`area_of{h: h,
  area: "X"}`) rather than reaching for flags.
- After editing corpus files, run `anneal -e '? diagnostic{severity: "error"}.'`
  to see new blockers. `anneal check` is a hidden CI gate alias for the same
  filtered diagnostic view and exits 1 if any error-severity diagnostic exists.
- If a query returns too much, add `--limit N`, smaller `--budget`, or a more
  specific pattern brace filter.

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
