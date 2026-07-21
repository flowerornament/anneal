---
name: anneal
description: "Orient in knowledge corpora with anneal. Use for markdown corpora, docs directories, or repos with anneal.dl; retrieving context, checking convergence, tracing handles, blockers, broken refs, changes, impact, or Datalog facts."
---

# Anneal

## Product Thesis

Anneal is a convergence assistant for knowledge corpora. It helps disconnected
intelligences recover what matters, expose uncertainty, and push shared
knowledge toward settledness.

## Agent Briefing

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
facts, loads the standard library plus project `anneal.dl`, and gives agents a
small ladder: arrive, discover vocabulary, retrieve evidence, then ask precise
Datalog questions.

Run `anneal help <command-or-runtime-name>` for exact flags or a runtime
teaching card. Runtime commands render readable text at a terminal and
JSON/NDJSON when piped; add `--format=text` in pipe-only harnesses when you want
to read the answer directly.

If this skill is not preloaded, run `anneal help agent` to print this briefing
from the installed binary. `anneal prime` remains a hidden compatibility alias.

## First Moves

Pick the smallest surface that can answer the next question.

```bash
anneal status --format=text
anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12 --format=text
anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12 --format=text
anneal context "<goal>" --hits 5 --budget 8000 --format=text
anneal schema --format=text
anneal describe runtime --format=text
anneal describe currency --format=text
anneal -e '? axis_of(predicate, "recency").' --format=text
```

Use `status` as the arrival surface: aggregate corpus vital signs plus
copy-runnable orientation and work queries. For goal-less reading, run
`recent_frontier` for recent live files and `ranked_anchor` for durable spine
files. Both end with `order by rank asc` so the list reads top-down — rank 1
first. `order by <expr> [asc|desc]` sorts any query's result at the projection
boundary, and `order by … --limit N` is a true top-N.
Use `context` only once you can name a goal: ranked span hits, compact span
metadata, and graph neighborhood in one call. Add `--read-spans` only when
inline matched bodies are worth the extra output. Use `schema` and
`describe NAME` before inventing predicate or field names; `describe` includes
signatures, examples, common joins, and output columns. Use `describe <axis>`
and `axis_of(predicate, axis)` when you need the runtime's dimensional map
before choosing predicates.

## Retrieval

```bash
anneal search "<text>" --limit 5 --format=text
anneal read <handle> --budget 4000 --format=text
anneal read <handle> --span-id <span-id> --format=text
anneal handle <handle> --format=text
anneal handle <handle> --impact --format=text
anneal handle <handle> --lineage --format=text
```

Use `search` for content retrieval. Span hits include `summary`, and
search/context hits annotate disposition (`current`, `current_head`,
`superseded`), lifecycle status, and age. Pass the hit's `span_id` to `read`
for the matched heading span. Use `handle` when
relationships matter; add `--impact` before edits that need reverse
dependencies or `--lineage` when Supersedes history and current heads matter.
If a context hit reports unmarked newer topical siblings, inspect it with
`anneal -e '? currency_suspect("HANDLE", newer), topic_sibling("HANDLE", newer, shared).' --format=text`.
Empty NDJSON streams emit `(0 rows)` on stderr while keeping stdout empty for
pipes.

Tool choice:

- `anneal context "X"`: find the section that defines X, with compact evidence
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
anneal -e '? read("docs/runtime-overview.md", 4000, span, text, start, end, tokens).'
```

Common stored relations: `*handle`, `*edge`, `*meta`, `*content`, `*span`,
`*config`, `*snapshot`, `*generation`, `*concern`.

Common predicate families:

- graph: `upstream`, `downstream`, `impact`, `neighborhood`
- retrieval: `search`, `read`, `top_k` helpers
- orientation: `recent_frontier`, `anchor`, `ranked_anchor`
- axes: `axis`, `axis_of`, `authored_age`, `changed_recently`,
  `currency_suspect`, `topic_sibling`
- convergence: `entropy`, `potential`, `frontier`, `blocker`, `flow`
- change history: `changed_within`, `git_mtime`, `at("snapshot:last")`
- checks: `diagnostic`

## Convergence

```bash
anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file, status: status} order by rank asc.' --limit 12
anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file, status: status} order by rank asc.' --limit 12
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
joins. Status keeps the live dispositions visible: advancing, holding,
drifting, or broken on the way toward settledness. `check` is a hidden CI gate
alias for the error-only diagnostic view and exits 1 when error-severity
diagnostics exist.

## Ask By Axis

When vocabulary feels blurry, ask the axis first:

```bash
anneal -e '? axis(name, question, oracle, disposition).' --format=text
anneal -e '? axis_of(predicate, "currency").' --format=text
anneal describe currency --format=text
anneal describe topic --format=text
```

Axes: relevance, currency, lifecycle, recency, importance, convergence,
structure, obligations, topic. Use `describe <axis>` for the question, oracle,
disposition, entry predicates, and common joins.

## Configuration

`anneal.dl` can add discovery facts, project rules, and project `@verb`s.
Discovery facts are consumed before extraction; rules and verbs load after
source facts exist. Project predicates shadow standard-library predicates by
name and arity.

Useful project config examples:

```dl
source md {
  scan_root(".").
  # external_root(["../formal"]).
  scan_exclude("node_modules").
}
linear(["CR", "REQ"])
rejected(["TODO", "NOTE"])
force(["ADR"])
potential_weight("freshness_decay", 0).
config search_boost { status("authoritative", 0.08). hub(0.01). }
config code_path_root { root(["web"]). }
```

`external_root` additively mounts a sibling directory outside the corpus root
but inside the same Git repository into the markdown graph. External files use
Git-project-relative handles such as `formal/models/prism.md`, so references
can resolve across directories; mounts that escape the repository, overlap, or
collide on a handle fail loudly.

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

- Start with `anneal status`; run its `recent_frontier` and `ranked_anchor` queries
  when you do not yet have a goal.
- Run from a marked corpus (`.design`, `docs`, or `anneal.dl`) or pass
  `--root <path>` for an explicit ad-hoc scan; use `anneal init --dry-run`
  when a directory should become a corpus.
- Use `anneal context "<goal>"` once you can name the goal.
- Use `search` then `read` when you need tighter retrieval control.
- Use `schema` and `describe NAME` before querying unfamiliar vocabulary.
- Use `describe <axis>` or `axis_of(predicate, axis)` when you need the
  dimensional map behind a predicate family.
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
  rows, and in-repo code refs are external handles with `external_class="code"`,
  `target_exists`, and `target_history_status`
- `asserts_code`: lifecycle statuses whose specs claim facts about this
  corpus's current code; W006 uses it to avoid warning on plans or research notes
- `source`: adapter such as markdown, code, host runtime, or issue tracker
- `relation`: stored row from a source or derived row from rules
- `verb`: named query with docs, schema, args, and capabilities
- `generation`: source refresh epoch for atomic fact replacement
- `trail`: per-query provenance for surfaced and consumed facts

You do not need the full language in your head. Query the runtime first; extend
it only when a goal needs vocabulary the corpus does not yet have.
