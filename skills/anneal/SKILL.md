---
name: anneal
description: "Orient in knowledge corpora, check health, trace handles, and assess edit impact. Use when a repo has `.design/`, `docs/`, or `anneal.toml`, or the user asks about convergence, broken refs, graph structure, what changed, or what depends on X."
metadata:
  short-description: Orient in knowledge corpora with anneal
---

# Anneal

Use `anneal` to understand a markdown knowledge corpus before you start making claims or edits.

If the tool is unfamiliar, run `anneal help` once. Use `anneal help <command>` for flags and edge cases instead of trying to memorize the CLI from this skill.

## Scope

This skill is for knowledge-corpus work, not ordinary source-code navigation.

- Ordinary code navigation where `rg`, `git diff`, or language tools are sufficient
- Deep CLI documentation; prefer `anneal help` for that

## First Moves

Use this default loop unless the request is narrower:

1. `anneal status`
2. `anneal check --active-only`
3. `anneal get <handle>` or `anneal find <text>` for the item the user cares about
4. `anneal impact <file-or-handle>` before editing corpus files

If the user only wants one thing, jump straight to the matching command.

## Command Map By Intent

### Orient

```bash
anneal status
anneal status -v
anneal check --active-only
```

Use `status` first when you need the high-level shape of the corpus. Use `check --active-only` for actionable problems without terminal-file noise.

### Inspect A Specific Thing

```bash
anneal get OQ-64
anneal find FM
anneal find "" --status=draft
anneal map --around=OQ-64
```

Use `get` for one known handle, `find` for discovery, and `map --around` when relationship shape matters more than raw text.

### Understand Change Or Blast Radius

```bash
anneal diff
anneal diff --days=7
anneal impact formal-model/v17.md
anneal impact OQ-64
```

Prefer `anneal diff` over plain `git diff` when the user wants structural corpus changes rather than line edits.

### Initialize Or Adjust Config

```bash
anneal init --dry-run
anneal init
```

Use this when the corpus lacks `anneal.toml` or when the user is formalizing status pipelines and handle namespaces.

## Minimal Mental Model

- `handle`: a file, section, label, version, or external URL
- `status`: frontmatter lifecycle state; typically split into active vs terminal
- `snapshot`: `status` and `check` append to `.anneal/history.jsonl`, which powers convergence and diff

You do not need the full model in your head to use the tool well. Reach for `anneal help` when the user needs exact semantics.

## Agent Rules

- Use `--json` whenever output will be parsed or summarized programmatically.
- Root detection is automatic: `--root` overrides, otherwise `anneal` prefers `.design/`, then `docs/`, then the current directory.
- Before editing knowledge files, run `anneal impact <file-or-handle>`.
- After editing knowledge files, run `anneal check --active-only`.
- If error counts look surprisingly high, confirm whether terminal files are included before reporting the corpus as unhealthy.

## High-Value Diagnostics

- `E001`: broken reference
- `E002`: undischarged obligation
- `W001`: stale reference from active work to terminal material
- `W002`: confidence gap where higher-level work depends on lower-level work

For the full diagnostic set, use `anneal help check`.
