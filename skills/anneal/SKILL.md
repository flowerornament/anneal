---
name: anneal
description: "Orient in knowledge corpora, retrieve relevant context, inspect graph structure, run corpus health checks, and ask Datalog queries over anneal facts. Use when a repo has `.design/`, `docs/`, or `anneal.toml`, or the user asks about convergence, blockers, broken refs, what changed, or what depends on X."
metadata:
  short-description: Query knowledge corpora with anneal
---

# Anneal

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
facts, loads the standard library and project `anneal.dl`, and exposes verbs,
retrieval primitives, and raw Datalog queries.

Run `anneal help <command>` for exact flags. Do not guess CLI details from
memory when a command matters.

If this skill is not preloaded, run `anneal prime` to print the shipped
briefing from the installed binary.

## First Moves

Pick the smallest command that can answer the next agent question.

### Arriving Cold

```bash
anneal context "<goal>"
anneal status
anneal garden
anneal describe runtime
```

Use `context` when the user gives a concrete goal and you need search hits,
graph neighborhood, and read spans in one call. Its `--budget` derives a
per-hit read cap that is applied independently to each winning hit. Use
`anneal status` when the question is corpus state. Use `anneal garden` when
you want ranked maintenance tasks with fix/context/verify hints. Use
`describe` for predicates, verbs, primitives, and runtime objects; use `vocab`
for observed status values, edge kinds, namespaces, and frontmatter fields.

### Finding and Reading

```bash
anneal search "<text>" --limit 5
anneal read <handle> --budget 4000
anneal H <handle>
```

Use `search` for content retrieval. It handles light stemming and common
planning abbreviations such as OQ/open question, ADR, and RFC. Use `read` after
search or when the handle is known. Use `H` when relationship shape matters.
Empty NDJSON row streams emit `(0 rows)` on stderr while leaving stdout empty
for pipes.

### Asking a Precise Question

```bash
anneal -e '? *handle{id: h, kind: "file", status: s}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? upstream("formal-model/v17.md", h).'
```

Use raw Datalog when the built-in verbs are too broad. Stored source facts use
`*` prefixes; prelude and project predicates do not.

Use `--explain` when you need provenance for why a row exists. It explains the
first 3 rows by default; use `--explain-first N` for a different cap or
`--explain-all` only when you really want every row's derivation tree.

### Discovering the Runtime

```bash
anneal sources
anneal verbs
anneal schema
anneal vocab
anneal describe convergence
```

Use these before inventing names. Agents should discover the active adapters,
verbs, predicates, corpus vocabulary, output contracts, and capability
requirements from the runtime.

### Health and Compatibility

```bash
anneal broken
anneal work
anneal trend  # emits rows when snapshot history exists; otherwise zero rows
anneal health --json --compact
anneal check --scope=active
anneal get <handle> --context
```

Prefer programmable-runtime verbs for new workflows. The older health commands
remain available during the migration window when exact compatibility matters.

## Command Map

### Retrieval

- `anneal context GOAL`: grouped cold-agent context from search, read, and
  neighborhood
- `anneal search TEXT`: ranked content hits with handle, span, score, reason,
  field, and low-confidence marker
- `anneal read HANDLE`: bounded content spans
- `anneal H HANDLE`: handle neighborhood

### Standard Verbs

- `anneal status`: compact corpus status
- `find`: identity-oriented handle lookup
- `work`: ranked work candidates
- `blocked`: blockers for one handle
- `broken`: diagnostic gate
- `trend`: convergence movement rows when snapshot history exists; no-history
  corpora emit zero rows
- `vocab`: observed status, edge, namespace, and metadata vocabulary
- `context`: cold-agent retrieval bundle

Project `@verb` declarations in `anneal.dl` appear beside these in
`anneal verbs` and are callable through the same surface.

### Raw Query Surface

```bash
anneal -e '? *handle{id: h, kind: "label", status: "open"}.'
anneal -e '? *edge{from: src, to: dst, kind: "DependsOn"}.'
anneal -e '? search("conformance", h, span, score, reason, field, low).'
anneal -e '? read("formal-model/v17.md", 4000, span, text, start, end, tokens).'
```

Common stored facts:

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
  fields you need.
- Use `--json` or NDJSON streams for tool consumption. Runtime commands render
  readable text at a terminal; use `--format=text` to force that renderer
  through pipe-only harnesses.
- Use `--root` for the corpus path. Use `--area` only for an area name inside
  that corpus, usually a top-level directory or configured concern group.
- Use legacy `health`, `check`, `get`, `find`, `map`, `impact`, `diff`, and
  `obligations` when exact pre-runtime behavior is required.
- After editing corpus files, run `anneal broken` or `anneal check
  --scope=active`, depending on whether you are exercising the programmable
  runtime or the compatibility surface.
- If a command returns too much, rerun with a lower `--limit`, smaller
  `--budget`, or a more specific query.

## Project Extension

`anneal.dl` at the corpus root can add discovery facts, project predicates, and
verbs.

```dl
md.file_extension(".md").
md.scan_root(".").
md.scan_exclude("node_modules").

release_blocker(h, "broken_ref") :=
  diagnostic("E001", severity, h, file, line, evidence).

@verb(
  name: "release-blockers",
  query: "? release_blocker(h, why).",
  doc: "Open blockers for the next release.",
  output_schema: "{\"h\":\"HandleId\",\"why\":\"String\"}",
  capabilities: ["read"]
)
```

Discovery facts are consumed by adapters before extraction. Rules and verbs are
loaded after source facts exist. Project predicates shadow standard-library
predicates by name and arity.

## Mental Model

- `handle`: a file, section, label, version, or external reference
- `source`: an adapter such as markdown, code, host runtime, or issue tracker
- `fact`: a stored row emitted by a source or a derived row produced by rules
- `verb`: a named query with documentation, schema, and capabilities
- `generation`: a source refresh epoch used for atomic fact replacement
- `visibility`: fact-level access envelope applied before derivation

You do not need the full language in your head. Query the runtime first; extend
it only when a goal needs vocabulary the corpus does not yet have.
