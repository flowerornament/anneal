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
2. `anneal check --json`
3. `anneal get <handle> --json` or `anneal find <text> --json` for the item the user cares about
4. `anneal impact <file-or-handle> --json` before editing corpus files

For a single concrete question, run the matching command directly.

## Command Map By Intent

### Orient

```bash
anneal status --json
anneal status -v --json
anneal check --json
```

`status --json` gives corpus shape and convergence context. `check --json` gives actionable problems without terminal-file noise by default; use `--include-terminal` for the full picture.

### Inspect A Specific Thing

```bash
anneal get OQ-64 --json
anneal find FM --json
anneal find "" --status=draft --json
anneal map --around=OQ-64 --json
```

Use `get` for one known handle, `find` for discovery, and `map --around` when relationship shape matters more than raw text.

### Understand Change Or Blast Radius

```bash
anneal diff --json
anneal diff --days=7 --json
anneal impact formal-model/v17.md --json
anneal impact OQ-64 --json
```

Use `anneal diff` when the question is about structural corpus changes rather than line edits.

### Initialize Or Adjust Config

```bash
anneal init --dry-run --json
anneal init --json
```

Use this when the corpus lacks `anneal.toml` or when the user is formalizing status pipelines and handle namespaces.

## Minimal Mental Model

- `handle`: a file, section, label, version, or external URL
- `status`: frontmatter lifecycle state; typically split into active vs terminal, representing how settled the knowledge is
- `snapshot`: `status` and `check` append to local anneal history, which powers convergence and diff
- `convergence`: structural evidence that the corpus is settling rather than fragmenting

You do not need the full model in your head. Reach for `anneal help` when exact semantics matter.

## Agent Rules

- Default to `--json` for fact gathering and reasoning.
- Root detection is automatic: `--root` overrides, otherwise `anneal` prefers `.design/`, then `docs/`, then the current directory.
- Before editing knowledge files, run `anneal impact <file-or-handle> --json`.
- After editing knowledge files, run `anneal check --json`.
- If error counts look surprisingly high, confirm whether terminal files are included before reporting the corpus as unhealthy.

## High-Value Diagnostics

- `E001`: broken reference
- `E002`: undischarged obligation
- `W001`: stale reference from active work to terminal material
- `W002`: confidence gap where higher-level work depends on lower-level work

For the full diagnostic set, use `anneal help check`.
