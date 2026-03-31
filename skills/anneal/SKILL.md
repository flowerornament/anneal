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

1. `anneal status --json`
2. `anneal check --active-only`
3. `anneal get <handle>` or `anneal find <text>` for the item the user cares about
4. `anneal impact <file-or-handle>` before editing corpus files

For a single concrete question, run the matching command directly.

## Command Map By Intent

### Orient

```bash
anneal status --json
anneal status -v
anneal check --active-only
```

Use `status --json` to capture corpus shape and convergence context. Use plain-text `check --active-only` for default health checks and session orientation.

### Inspect A Specific Thing

```bash
anneal get anneal-spec.md
anneal find <text>
anneal find "" --status=draft
anneal map --around=anneal-spec.md
```

Use `get` for one known handle, `find` for discovery, and `map --around` when relationship shape matters more than raw text.

### Understand Change Or Blast Radius

```bash
anneal diff
anneal diff --days=7
anneal impact anneal-spec.md
anneal impact <file-or-handle>
```

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

- Use `anneal status --json` for orientation. Use plain-text output for routine `check`, `get`, `find`, `map`, `diff`, `impact`, and `init` unless you are immediately filtering machine-readable output.
- Use plain-text `anneal check --active-only` for default orientation and health checks.
- Root detection is automatic: `--root` overrides, otherwise `anneal` prefers `.design/`, then `docs/`, then the current directory.
- Before editing knowledge files, run `anneal impact <file-or-handle>`.
- After editing knowledge files, run `anneal check --active-only`.
- If error counts look surprisingly high, confirm whether terminal files are included before reporting the corpus as unhealthy.

When you need structured diagnostics, filter them to a narrow summary before returning them to the model, for example:

```bash
anneal check --active-only --json | jq '{summary, diagnostic_count: (.diagnostics | length)}'
```

## High-Value Diagnostics

- `E001`: broken reference
- `E002`: undischarged obligation
- `W001`: stale reference from active work to terminal material
- `W002`: confidence gap where higher-level work depends on lower-level work

For the full diagnostic set, use `anneal help check`.
