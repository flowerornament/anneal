---
name: anneal
description: "Orient in knowledge corpora with anneal. Use for markdown corpora, docs directories, or repos with anneal.dl; retrieving context, checking convergence, tracing handles, blockers, broken refs, changes, impact, or Datalog facts."
---

# Anneal

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
facts, loads the standard library plus project `anneal.dl`, and gives agents a
small ladder: arrive, discover vocabulary, retrieve evidence, then ask precise
Datalog questions.

Run `anneal help <command>` for exact flags. Do not guess CLI details when a
command matters. Runtime commands render readable text at a terminal and
JSON/NDJSON when piped; add `--format=text` in pipe-only harnesses when you
want to read the answer directly.

If this skill is not preloaded, run `anneal help agent` to print this briefing
from the installed binary. `anneal prime` remains a hidden compatibility alias.

## First Moves

Pick the smallest surface that can answer the next question.

```bash
anneal context "<goal>" --hits 5 --budget 8000 --format=text
anneal status --format=text
anneal schema --format=text
anneal describe runtime --format=text
```

Use `context` for goal-oriented orientation: ranked span hits, matched excerpts,
and graph neighborhood in one call. Use `status` for corpus state. Use `schema`
and `describe NAME` before inventing predicate or field names; `describe`
includes signatures, examples, common joins, and output columns.

## Retrieval

```bash
anneal search "<text>" --limit 5 --format=text
anneal read <handle> --budget 4000 --format=text
anneal read <handle> --span-id <span-id> --format=text
anneal handle <handle> --format=text
anneal handle <handle> --impact --format=text
```

Use `search` for content retrieval. Span hits include `heading_path`; pass the
hit's `span_id` to `read` for the matched heading span. Use `handle` when
relationships matter; add `--impact` before edits that need reverse
dependencies. Empty NDJSON streams emit `(0 rows)` on stderr while keeping
stdout empty for pipes.

Tool choice:

- `anneal context "X"`: find the section that defines X, with evidence
- `grep -rn "X"`: find every literal occurrence with line numbers
- `anneal -e '? ...'`: ask structural graph questions

## Query Surface

Use raw Datalog when a built-in verb is too broad. Stored relations use `*`
prefixes; prelude and project predicates do not. `anneal -e -` reads a query
from stdin. Use `--explain` for provenance.

```bash
anneal -e '? *handle{id: h, kind: "file", status: s}.' --limit 20
anneal -e '? *edge{from: src, to: dst, kind: kind}.' --limit 20
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
anneal -e '? read("formal-model/v17.md", 4000, span, text, start, end, tokens).'
```

Common stored relations: `*handle`, `*edge`, `*meta`, `*content`, `*span`,
`*config`, `*snapshot`, `*generation`, `*concern`.

Common predicate families:

- graph: `upstream`, `downstream`, `impact`, `neighborhood`
- retrieval: `search`, `read`, `top_k` helpers
- convergence: `entropy`, `potential`, `frontier`, `blocker`, `flow`
- change history: `changed_within`, `git_mtime`, `at("snapshot:last")`
- checks: `diagnostic`

## Convergence

```bash
anneal -e '? diagnostic{code: code, severity: "error", subject: h, file: file, line: line}.'
anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? blocker(h, energy, source), h = "HANDLE".'
anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'
anneal -e '? flow(h, direction), *handle{id: h, status: status}.'
anneal -e '? area_health(area, grade, files, errors, cross_edges).'
anneal -e '? changed_within(h, 7), *handle{id: h, kind: "file", summary: summary}.'
```

Use `describe convergence`, `describe potential`, `describe entropy`,
`describe blocker`, `describe flow`, and `describe area_health` to learn the
joins. `check` is a hidden CI gate alias for the error-only diagnostic view and
exits 1 when error-severity diagnostics exist.

## Configuration

`anneal.dl` can add discovery facts, project rules, and project `@verb`s.
Discovery facts are consumed before extraction; rules and verbs load after
source facts exist. Project predicates shadow standard-library predicates by
name and arity.

Useful project config examples:

```dl
source md { scan_root("."). scan_exclude("node_modules"). }
linear(["CR", "REQ"])
rejected(["TODO", "NOTE"])
force(["ADR"])
config potential_weight { freshness_decay(0). }
config search_boost { status("authoritative", 0.08). hub(0.01). }
config code_path_root { root(["web"]). }
```

When a query becomes a reusable corpus move, promote it into a project verb:

```dl
@verb(
  name: "area-diagnostics",
  query: "area_diagnostic(h, code, file) :=
    verb_arg(\"area\", area),
    diagnostic{subject: h, code: code, file: file},
    area_of{h: h, area: area}.

    ? area_diagnostic(h, code, file).",
  doc: "Diagnostics in one area.",
  output_schema: "{\"h\":\"HandleId\",\"code\":\"String\",\"file\":\"String|null\"}",
  args: ["area:String"],
  capabilities: ["read"]
).
```

Project verbs appear in `schema` and are callable by name, for example
`anneal area-diagnostics language --format=text`. Use `anneal describe <verb>`
for a loaded verb's teaching card. Do not copy the built-in prelude into a
project. Use `anneal init --dry-run` to inspect the current scaffold before
writing config.

## Agent Rules

- Start with `anneal context "<goal>"` for goal-oriented work.
- Use `search` then `read` when you need tighter retrieval control.
- Use `schema` and `describe NAME` before querying unfamiliar vocabulary.
- Use `anneal -e` for composite questions; project only fields you need.
- Add `--limit N`, smaller `--budget`, or stricter pattern filters when broad
  predicates return too much.
- Use `--root` for the corpus path. Filter inside the query rather than
  reaching for retired global flags.
- After editing corpus files, run the error diagnostic query or `anneal check`.
- Let retired-command recovery messages teach old-workflow replacements instead
  of memorizing compatibility surfaces.

## Mental Model

- `handle`: file, label, version, or external reference; headings are `*span`
  rows, and in-repo code refs are external handles with `external_class="code"`
- `source`: adapter such as markdown, code, host runtime, or issue tracker
- `relation`: stored row from a source or derived row from rules
- `verb`: named query with docs, schema, args, and capabilities
- `generation`: source refresh epoch for atomic fact replacement
- `trail`: per-query provenance for surfaced and consumed facts

You do not need the full language in your head. Query the runtime first; extend
it only when a goal needs vocabulary the corpus does not yet have.
