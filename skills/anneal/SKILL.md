---
name: anneal
description: "Orient in knowledge corpora, recover relevant repo context quickly, inspect structure, check health, and assess edit impact. Use when a repo has `.design/`, `docs/`, or `anneal.toml`, or the user asks about convergence, broken refs, graph structure, what changed, or what depends on X."
metadata:
  short-description: Orient in knowledge corpora with anneal
---

# Anneal

Use `anneal` to orient in a markdown knowledge corpus, recover relevant context quickly, and validate structural assumptions before making claims or edits.

`anneal` treats a corpus as a convergence system rather than a pile of files: handles move through degrees of settledness, obligations are either discharged or left hanging, and snapshot history shows whether the body of knowledge is advancing, holding, or drifting.

Use `anneal help <command>` for exact flags and edge cases. Do not guess the CLI from memory.

## Scope

Use this skill for knowledge-corpus structure, health, impact, and validation.

- Use `rg`, `git diff`, or language tools for ordinary source-code navigation.
- Use `anneal help` for exact CLI details.

## First Moves

Pick the loop that matches the request shape.

### Arriving at an unfamiliar corpus (general orientation)

1. `anneal areas` — what exists, and how healthy is each area?
2. `anneal find --kind=file --status=active --context` — what is each active thing actually about?
3. `anneal explain convergence` — what do the status values mean here?
4. `anneal orient --budget=50k` — if you need a reading list before making changes

### Scoped to a specific area or file (narrowing)

1. `anneal orient --area=<dir> --budget=30k` or `anneal orient --file=<path> --budget=30k`
2. `anneal map --around=<handle> --upstream` — what does this build on?
3. `anneal impact <file>` — what breaks if I change this?
4. `anneal check --area=<dir>` — any issues in the scope?

### Maintenance (gardening)

1. `anneal garden` — ranked maintenance tasks with fix/context/verify hints
2. Follow each task's `context:` hint (an `anneal orient` invocation) to load the files you need
3. Apply the `fix:` action
4. Run the task's `verify:` hint (usually an `anneal check` invocation) to confirm
5. `anneal diff --days=7` to see what moved since the last session

For a single concrete question, run the matching command directly.

## Command Map By Intent

### Orient

```bash
anneal areas
anneal areas --sort=grade
anneal orient --budget=50k
anneal orient --area=compiler --budget=30k
anneal orient --file=impl-plan.md --budget=30k
anneal status --json --compact
anneal check --scope=active
```

Use `areas` for per-area health profiles — each directory gets a grade (A–D) based on errors, connectivity, and metadata coverage. Use `orient` to generate a tiered, token-budgeted reading list (pinned → area entry points → upstream context → downstream consumers). Use `orient --file=X` as the upstream complement to `impact`: what does this file build on? Use `status --json --compact` when you need a machine-readable dashboard. Use plain-text `check --active-only` for default health checks.

### Garden (maintenance)

```bash
anneal garden
anneal garden --area=compiler
anneal garden --category=fix
anneal garden --json --limit=25
```

Use `garden` to surface ranked maintenance tasks across six categories: `fix` (E001 broken refs, E002 undischarged obligations), `tidy` (S001 orphans), `link` (island areas), `stale` (old files), `meta` (missing frontmatter), `drift` (namespaces leaking across areas). Each task includes `fix:`, `context:`, and `verify:` hints so the agent can close the loop without guidance.

### Inspect A Specific Thing

```bash
anneal get anneal-spec.md --context
anneal get arch.md impl.md spec.md --status-only
anneal get arch.md impl.md spec.md --context
anneal find <text> --limit 25
anneal find --status=active --kind=file --context
anneal find --recent --kind=file --sort=date
anneal map --around=anneal-spec.md
anneal map --around=anneal-spec.md --upstream
anneal map --around=anneal-spec.md --downstream
anneal map --by-area
anneal map --by-area --min-edges=10
```

Use `get` for one known handle, `find` for discovery (query is optional when any filter is present), and `map --around` when relationship shape matters more than raw text. Pass multiple handles to `get` for a compact batch view (`--status-only` trims to identity+status; `--context` adds the purpose/note summary). Add `--upstream` or `--downstream` to turn `map --around` into a directed tree — the same traversal `orient --file` and `impact` use. Add `--by-area` for the 30-second shape-of-the-corpus view: cross-area edge counts plus island detection.

### Ask A Structural Question

```bash
anneal query handles --kind label --namespace OQ
anneal query edges --kind DependsOn --confidence-gap
anneal query diagnostics --severity warning
anneal query obligations --undischarged
anneal query suggestions --code S001
```

Use `query` for graph-shaped questions that are too specific for `status`, too broad for `get`, and outside `find`'s identity-search role.

### Justify A Derived Result

```bash
anneal explain diagnostic --id diag_deadbeef
anneal explain impact anneal-spec.md
anneal explain convergence
anneal explain obligation REQ-12
anneal explain suggestion --id sugg_deadbeef
```

Use `explain` when the question is “why did anneal say this?” rather than “what exists?”.

### Understand Change Or Blast Radius

```bash
anneal diff
anneal diff --days=7
anneal diff --by-area
anneal diff --by-area --days=7
anneal impact anneal-spec.md
anneal impact <file-or-handle>
anneal orient --file=anneal-spec.md
```

Impact traverses edge kinds listed in `[impact] traverse` in `anneal.toml` (defaults to DependsOn, Supersedes, Verifies). Corpora with custom edge kinds like Synthesizes or Implements should configure this for accurate blast radius.

`orient --file=X` and `impact X` compose into a before/after pair for edits: `orient --file` is the upstream reading list ("what do I need to read before editing?") and `impact` is the downstream review set ("what do I need to verify after?").

Use `anneal diff` when the question is about structural corpus changes rather than line edits. Add `--days=7` or `--days=30` for a coarser session-resume view; add `--json` for a structured delta. `--by-area` pivots to a per-area trend table so you can see which areas are improving, holding, or degrading.

### Initialize Or Adjust Config

```bash
anneal init --dry-run
anneal init
```

Use this when the corpus lacks `anneal.toml` or when the user is formalizing status pipelines and handle namespaces.

The top-level `exclude` list in `anneal.toml` accepts directory names (e.g. `"vendor"`) and glob patterns (e.g. `"**/README.md"`). Glob patterns prevent matched files from entering the graph — useful for structural index files that should not trigger W003 or S003.

## Minimal Mental Model

- `handle`: a file, section, label, version, or external URL
- `status`: frontmatter lifecycle state; typically split into active vs terminal, representing how settled the knowledge is
- `snapshot`: `status` and `check` append to local anneal history, which powers convergence and diff
- `convergence`: structural evidence that the corpus is settling rather than fragmenting

You do not need the full model in your head. Reach for `anneal help` when exact semantics matter.

## Agent Rules

- Arriving at a new corpus: prefer `anneal areas` + `anneal orient` over `anneal status`. `areas` gives the shape (directories, grades, cross-links); `orient` gives a token-budgeted reading list. `status --json --compact` is the machine-readable dashboard — use it when capturing corpus state for another tool, not for human-facing orientation.
- Before editing a knowledge file: `anneal orient --file=<path>` for upstream context, then `anneal impact <file>` for downstream blast radius. The pair composes into a before/after workflow.
- When asked "what needs fixing?": `anneal garden`. Follow each task's `fix:`, `context:`, and `verify:` hints directly — they encode the full maintenance loop without further guidance.
- Use plain-text output for routine `check`, `get`, `find`, `query`, `explain`, `map`, `diff`, `impact`, `areas`, `orient`, `garden`, and `init` unless you are immediately filtering machine-readable output.
- Use plain-text `anneal check --scope=active` for default health checks. Scope with `--area=<dir>` or `--recent` / `--since=14d` to narrow quickly.
- Prefer bounded defaults like `anneal get <handle> --context`, `anneal find <text> --limit 25`, `anneal query ...`, and `anneal map --around=<handle>`.
- `anneal find` accepts an optional query — `anneal find --status=active --kind=file --context` works without a positional argument.
- Reach for `anneal query ...` when the user is asking an ad hoc structural question across many handles or edges.
- Reach for `anneal explain ...` when the user wants provenance for a diagnostic, suggestion, obligation state, impact set, or convergence signal. Outstanding obligations include the exact `discharges:` frontmatter syntax needed to remediate.
- Root detection is automatic: `--root` overrides, otherwise `anneal` prefers `.design/`, then `docs/`, then the current directory.
- After editing knowledge files, run `anneal check --scope=active` (or the `verify:` hint from the originating garden task).
- If error counts look surprisingly high, confirm whether terminal files are included before reporting the corpus as unhealthy.

When you need structured diagnostics, filter them to a narrow summary before returning them to the model, for example:

```bash
anneal check --scope=active --json | jq '.summary'
anneal check --scope=active --json --diagnostics --limit 25 | jq '.diagnostics[:5]'
```

## High-Value Diagnostics

- `E001`: broken reference
- `E002`: undischarged obligation
- `W001`: stale dependency — active handle has DependsOn edge to terminal (Cites and custom edges don't trigger W001)
- `W002`: confidence gap where higher-level work depends on lower-level work

For the full diagnostic set, use `anneal help check`.
