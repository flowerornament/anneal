---
name: anneal
description: "Orient in knowledge corpora with anneal. Use for markdown corpora, docs directories, or repos with anneal.dl; retrieving context, checking convergence, tracing handles, blockers, broken refs, changes, impact, or Datalog facts."
---

# Anneal

## Product Thesis

Anneal is a convergence assistant for knowledge corpora. It helps disconnected
intelligences recover what matters, expose uncertainty, and push shared
knowledge toward settledness.

## What This Actually Is

Anneal is not a search tool with extra features. It is a **Datalog runtime over
a typed knowledge graph** built from your corpus. Files become handles, headings
become spans, references become typed edges, and lifecycle status becomes a
partition you can reason over. You ask questions in a language, not through a
fixed menu of commands.

That buys three things grep and embeddings cannot give you:

- **Structural questions.** "Which live specs cite code paths that moved?" is a
  join over edges, status, and git history — not a string match.
- **Composition.** Any two predicates can be joined. The verbs you see are
  named queries over the same substrate you can query directly.
- **Provenance.** Every derived row can explain which stored facts produced it.

The corpus is the database. You are expected to interrogate it, and to extend
its vocabulary when a goal needs a distinction the corpus does not yet carry.

## How Anneal Earns Trust

This is the part most tools skip, and the part you should rely on. Anneal is
built so that **no result carries more authority than its oracle earns.** Learn
these and you can calibrate how far to trust any answer.

- **It reports and teaches; it does not advise.** You will never be told
  "read this first" or "this document is probably stale." You will be told what
  was measured and by what rule. A topology-profile surface was fully designed
  and then dropped rather than shipped, because measurement did not support the
  premise it rested on.
- **Author-declared, machine-checked — never inferred.** Edges come from what
  someone wrote, validated against reality. Anneal will *flag* that a newer
  sibling may exist on a topic; it will never assert a supersession nobody
  declared. Currency flags, it does not assert.
- **Absence and `unknown` are different claims.** `unknown` means a claim
  applies but was not earned. Absence means the claim does not apply at all. A
  non-Git corpus makes no Git-ignore claim rather than reporting zero.
- **Counts name their unit and population.** `spec_code_drift=6 distinct source
  handles` rather than a bare 6, because the rows are per citation and the
  metric is per handle.
- **Reductions admit they reduced.** `read --budget` tells you it showed the
  first N of M tokens and how to get the rest. Take that literally: it is a
  prefix, not a summary, and exact-span provenance proves quotation fidelity,
  never coverage. Nothing here yet claims a reduced view is representative.
- **Axes are separate questions with separate oracles.** Relevance is not
  currency is not lifecycle is not recency. Collapsing them is how a confident,
  relevant, four-month-stale document gets read as current.
- **Policy is queryable data, not hidden defaults.** What counts as terminal,
  which dependency statuses are dead, which frontmatter anneal models — all of
  it is a relation you can read, carrying whether the answer came from project
  config or builtin policy. When behavior surprises you, query the policy
  rather than guessing at it.

```bash
anneal -e '? lifecycle_status_classification(status, classification, origin).' --format=text
anneal -e '? dependency_status_classification(status, classification, origin).' --format=text
```

The practical consequence: when anneal hedges, the hedge is load-bearing. Treat
a `REPORT` disposition as a hint to verify, not as a weak assertion.

## Agent Briefing

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
facts, loads the standard library plus project `anneal.dl`, and gives you a
ladder: arrive, discover vocabulary, retrieve evidence, then ask precise
questions.

Do not memorize the surface. Ask the binary:

```bash
anneal help
anneal help agent
anneal schema --format=text
anneal describe runtime --format=text
```

`anneal help <command-or-runtime-name>` gives exact flags or a runtime teaching
card. `anneal describe <name>` gives signatures, examples, common joins, and
output columns for any predicate, axis, diagnostic code, or verb. Reach for
these instead of guessing names — the runtime is self-describing, and that is
the intended way to use it.

Commands render readable text at a terminal and JSON/NDJSON when piped; add
`--format=text` in pipe-only harnesses when you want to read the answer
yourself.

If this skill is not preloaded, run `anneal help agent` to print this briefing
from the installed binary. `anneal prime` remains a hidden compatibility alias.

## First Moves

Pick the smallest surface that can answer the next question.

```bash
anneal status --format=text
anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.' --limit 12 --format=text
anneal -e '? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.' --limit 12 --format=text
anneal context "<goal>" --hits 5 --budget 8000 --format=text
```

`status` is the arrival surface: corpus vital signs plus copy-runnable
orientation and work queries. When you have no goal yet, run its
`recent_frontier` query for recent live files and `ranked_anchor` for the
durable spine. Both end with `order by rank asc` so the list reads top-down.
`order by <expr> [asc|desc]` sorts at the projection boundary, and with
`--limit N` it is a true top-N.

Use `context` once you can name a goal: ranked span hits, compact span
metadata, and graph neighborhood in one call. Add `--read-spans` only when
inline matched bodies are worth the output.

## Retrieval

```bash
anneal search "<text>" --limit 5 --format=text
anneal read <handle> --budget 4000 --format=text
anneal handle <handle> --format=text
anneal handle <handle> --impact --format=text
anneal handle <handle> --lineage --format=text
```

Search hits annotate disposition (`current`, `current_head`, `superseded`),
lifecycle status, and age — read those, not just the rank. Pass a hit's
`span_id` to `read` for the matched heading span. Use `handle` when
relationships matter: `--impact` before edits that need reverse dependencies,
`--lineage` when Supersedes history and current heads matter.

**Search matches every containing span, not only the deepest one.** A term
inside a nested section returns the leaf, its parents, and the document root.
Span ids are hierarchical slugs, so keep only spans with no matching descendant
when you need one row per occurrence.

Tool choice:

- `anneal context "X"`: find the section that defines X, with compact evidence
- `grep -rn "X"`: find every literal occurrence with line numbers
- `anneal -e '? ...'`: ask structural graph questions

## Query Surface

Use raw Datalog when a built-in verb is too broad. Stored relations use `*`
prefixes; prelude and project predicates do not. `anneal -e -` reads from
stdin. Add `--explain` for provenance.

```bash
anneal -e '? *handle{id: h, kind: "file", status: s}.' --limit 20
anneal -e '? *edge{from: src, to: dst, kind: kind}.' --limit 20
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
```

Common stored relations: `*handle`, `*edge`, `*meta`, `*content`, `*span`,
`*config`, `*snapshot`, `*generation`, `*concern`.

Predicate families, each with a `describe` card:

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
anneal -e '? diagnostic{code: code, severity: "error", subject: h, file: file, line: line}.'
anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? blocker(h, energy, source), h = "HANDLE".'
anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'
anneal -e '? area_health(area, grade, files, errors, cross_edges).'
```

`status` keeps the live dispositions visible: advancing, holding, drifting, or
broken on the way toward settledness. Run `anneal describe convergence` for the
joins. `check` is a hidden CI gate over the error-only diagnostic view and
exits 1 when error-severity diagnostics exist.

## Ask By Axis

When vocabulary feels blurry, ask the axis before guessing a predicate:

```bash
anneal -e '? axis(name, question, oracle, disposition).' --format=text
anneal -e '? axis_of(predicate, "currency").' --format=text
anneal describe currency --format=text
```

Axes: relevance, currency, lifecycle, dependency-validity, recency, importance,
convergence, structure, obligations, topic. Each `describe <axis>` states the
question, the oracle, the disposition, entry predicates, and common joins.

## Where Agents Go Wrong

Failures that have cost real time, and what to do instead.

- **Reading rank as recency.** A relevance match cannot distinguish the current
  authoritative spec from a superseded one that matches better. Read the
  disposition and age on every hit.
- **Trusting a walked path as the corpus.** What the tool walks, what the repo
  tracks, and what config mounts are three different sets. When you measure a
  corpus, measure what anneal emitted, not what `find` returned.
- **Reading `check`'s count as the diagnostic total.** `check` filters to error
  severity. Its hint names the query for the rest; run that query rather than
  inferring.
- **Extending by redefining.** A project rule with a standard predicate's name
  and arity *replaces* it rather than adding to it. Gate-output relations
  refuse this outright.
- **Inventing vocabulary.** If a name is not in `schema` or `describe`, it does
  not exist. Guessing produces a static-analysis error, not a silent empty
  result — but the round trip is wasted.

## Configuration

`anneal.dl` adds discovery facts, project rules, and project `@verb`s.
Discovery facts are consumed before extraction; rules and verbs load after
source facts exist.

```bash
anneal init --dry-run
anneal help init
anneal describe W005 --format=text
```

Project predicates shadow standard-library predicates by name and arity, except
gate-output relations declared `shadow: "forbid"`. `diagnostic/6` cannot be
replaced from `anneal.dl`, because doing so could make a broken corpus pass
`anneal check`; use a separately named predicate for direct evaluation, or
`ANNEAL_PRELUDE_PATH` to replace the whole prelude package intentionally.

```dl
source md {
  scan_root(".").
  scan_exclude("node_modules").
}
config convergence { settled(["project-settled"]). }
config dependency { dead(["custom-retired"]). valid(["custom-current"]). }
```

Configuration answers to queryable policy relations rather than hidden
defaults. `describe` each before configuring it: settled does not imply
terminal, dependency validity is separate from lifecycle convergence,
`unmodeled_frontmatter_key` never recommends an edge kind, and
`gitignored_scanned_file` exists because discovery follows configured mounts
rather than Git tracking — untracked drafts are not treated as ignored.

`external_root` additively mounts a sibling directory outside the corpus root
but inside the same Git repository. External files use
Git-project-relative handles such as `formal/models/prism.md`, so references
resolve across directories; mounts that escape the repository, overlap, or
collide on a handle fail loudly.

When a query becomes a reusable corpus move, promote it into a project `@verb`
with docs, schema, args, and capabilities. Verbs then appear in `schema` and
are callable by name. Run `anneal describe <verb>` for a loaded verb's teaching
card, and `anneal init --dry-run` to inspect the current scaffold. Do not copy
the built-in prelude into a project.

## Agent Rules

- Run `anneal status` first; use its printed queries when you have no goal yet.
- Run from a marked corpus (`.design`, `docs`, or `anneal.dl`) or pass
  `--root <path>`.
- Run `anneal describe <name>` before using vocabulary you have not used before.
- Run `anneal schema` when you need the callable surface rather than one card.
- Use `anneal context "<goal>"` once you can name the goal; `search` then `read`
  when you need tighter control.
- Use `anneal -e` for composite questions, and project only the fields you need.
- Add `--limit N` or a smaller `--budget` when a broad predicate returns too
  much; filter inside the query rather than reaching for retired global flags.
- Read disposition, status, and age on retrieval hits before acting on rank.
- Run `anneal check` or the error diagnostic query after editing corpus files.
- Extend the corpus vocabulary when a goal needs a distinction it does not
  carry; do not work around a missing predicate in your head.

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
