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

Use this orientation loop when the request is broad:

1. `anneal status --json --compact`
2. `anneal check --active-only`
3. `anneal get <handle> --context` or `anneal find <text> --limit 25` for the item the user cares about
4. `anneal query ...` when the question is structural rather than identity-based
5. `anneal explain ...` when you need to justify a warning, suggestion, impact set, convergence signal, or obligation state
6. `anneal impact <file-or-handle>` before editing corpus files

For a single concrete question, run the matching command directly.

## Command Map By Intent

### Orient

```bash
anneal status --json --compact
anneal status -v
anneal check --active-only
```

Use `status --json --compact` to capture corpus shape and convergence context. Use plain-text `check --active-only` for default health checks and session orientation.

### Inspect A Specific Thing

```bash
anneal get anneal-spec.md --context
anneal find <text> --limit 25
anneal find "" --status=draft
anneal map --around=anneal-spec.md
```

Use `get` for one known handle, `find` for discovery, and `map --around` when relationship shape matters more than raw text.

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
anneal impact anneal-spec.md
anneal impact <file-or-handle>
```

Impact traverses edge kinds listed in `[impact] traverse` in `anneal.toml` (defaults to DependsOn, Supersedes, Verifies). Corpora with custom edge kinds like Synthesizes or Implements should configure this for accurate blast radius.

Use `anneal diff` when the question is about structural corpus changes rather than line edits.

### Initialize Or Adjust Config

```bash
anneal init --dry-run
anneal init
```

Use this when the corpus lacks `anneal.toml` or when the user is formalizing status pipelines and handle namespaces.

## Minimal Mental Model

- `handle`: a file, section, label, version, or external URL
- `status`: frontmatter lifecycle state; typically split into active vs terminal, representing how settled the knowledge is
- `snapshot`: `status` and `check` append to local anneal history, which powers convergence and diff
- `convergence`: structural evidence that the corpus is settling rather than fragmenting

You do not need the full model in your head. Reach for `anneal help` when exact semantics matter.

## Agent Rules

- Use `anneal status --json --compact` for orientation. Use plain-text output for routine `check`, `get`, `find`, `query`, `explain`, `map`, `diff`, `impact`, and `init` unless you are immediately filtering machine-readable output.
- Use plain-text `anneal check --active-only` for default orientation and health checks.
- Prefer bounded defaults like `anneal get <handle> --context`, `anneal find <text> --limit 25`, `anneal query ...`, and `anneal map --around=<handle>`.
- Reach for `anneal query ...` when the user is asking an ad hoc structural question across many handles or edges.
- Reach for `anneal explain ...` when the user wants provenance for a diagnostic, suggestion, obligation state, impact set, or convergence signal.
- Root detection is automatic: `--root` overrides, otherwise `anneal` prefers `.design/`, then `docs/`, then the current directory.
- Before editing knowledge files, run `anneal impact <file-or-handle>`.
- After editing knowledge files, run `anneal check --active-only`.
- If error counts look surprisingly high, confirm whether terminal files are included before reporting the corpus as unhealthy.

When you need structured diagnostics, filter them to a narrow summary before returning them to the model, for example:

```bash
anneal check --active-only --json | jq '.summary'
anneal check --active-only --json --diagnostics --limit 25 | jq '.diagnostics[:5]'
```

## High-Value Diagnostics

- `E001`: broken reference
- `E002`: undischarged obligation
- `W001`: stale dependency — active handle has DependsOn edge to terminal (Cites and custom edges don't trigger W001)
- `W002`: confidence gap where higher-level work depends on lower-level work

For the full diagnostic set, use `anneal help check`.
