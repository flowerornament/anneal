---
name: anneal
description: "Orient in a knowledge corpus using the anneal CLI. Use when arriving at a project with a .design/ or docs/ directory, when asked about corpus health, convergence, broken references, handle relationships, or knowledge graph structure. Triggers on: anneal, corpus health, convergence, knowledge graph, broken references, stale references, what changed, what depends on."
---

# Anneal: Knowledge Corpus Orientation

Use the `anneal` CLI to understand, navigate, and track convergence of markdown knowledge corpora.

**IMPORTANT: If you haven't used `anneal` before, run `anneal help` first.** It explains core concepts (handles, edges, status, convergence, snapshots) and shows every command with examples. Run `anneal help <command>` for detailed usage of any subcommand. The help text is designed for agents — read it.

## When to Activate

- Arriving at a project with `.design/`, `docs/`, or `anneal.toml`
- User asks about corpus health, broken references, convergence
- User asks "what changed", "what depends on X", "show me the graph"
- User asks about handle relationships, label namespaces, pipeline status
- Before editing files in a knowledge corpus (check impact first)

## First Contact

If this is your first time at this corpus:

```bash
anneal help                      # Read this — concepts, commands, output format
anneal status                    # Orient: what exists, health, convergence
anneal check --active-only       # Actionable errors (skip terminal file noise)
```

## Quick Orientation

Always start with status to understand the corpus:

```bash
anneal status                    # Dashboard: counts, pipeline, health, convergence
anneal status --json             # Machine-readable for programmatic analysis
anneal status -v                 # Expand pipeline to list files per level
```

## Core Commands

### Navigate

```bash
anneal get OQ-64                 # Look up a handle: kind, status, edges
anneal find FM                   # Search handles by text (active only)
anneal find FM --all             # Include terminal handles
anneal find "" --status=draft    # Find all handles with status "draft"
anneal find "" --kind=label      # Find all label handles
anneal map --around=OQ-64        # BFS neighborhood (text)
anneal map --format=dot          # Graphviz DOT output
```

### Check Health

```bash
anneal check                     # All diagnostics (errors, warnings, suggestions)
anneal check --active-only       # Skip errors from terminal (settled) files
anneal check --errors-only       # Errors only (for CI/pre-commit)
anneal check --suggest           # Structural suggestions only (S001-S005)
anneal check --stale             # Staleness warnings only (W001)
```

### Track Changes

```bash
anneal diff                      # Changes since last snapshot
anneal diff --days=7             # Changes since ~7 days ago
anneal diff HEAD~3               # Structural diff against git ref
```

### Understand Impact

```bash
anneal impact formal-model/v17.md   # What depends on this file?
anneal impact OQ-64                 # What depends on this label?
```

### Setup

```bash
anneal init                      # Generate anneal.toml from inferred structure
anneal init --dry-run            # Preview without writing
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Handle** | Unit of knowledge: file, section, label (OQ-64), or version (v17) |
| **Edge** | Relationship: Cites, DependsOn, Supersedes, Verifies, Discharges |
| **Status** | Frontmatter `status:` field, partitioned into active/terminal |
| **Pipeline** | Ordered status progression (raw -> draft -> ... -> authoritative) |
| **Convergence** | Direction signal: advancing (resolving), holding (stable), drifting (growing) |
| **Snapshot** | Point-in-time state in `.anneal/history.jsonl`, enables diff and convergence |

## Diagnostic Codes

| Code | Severity | Meaning |
|------|----------|---------|
| E001 | Error | Broken reference (handle not found) |
| E002 | Error | Undischarged obligation (linear handle) |
| W001 | Warning | Stale reference (active -> terminal) |
| W002 | Warning | Confidence gap (higher level depends on lower) |
| W003 | Warning | Missing frontmatter (no status: field) |
| I001 | Info | Unresolved section references (summary) |
| I002 | Info | Multiple discharges on one obligation |
| S001 | Suggestion | Orphaned label/version (no incoming edges) |
| S002 | Suggestion | Candidate namespace (recurring prefix, not confirmed) |
| S003 | Suggestion | Pipeline stall (no outflow to next level) |
| S004 | Suggestion | Abandoned namespace (all members terminal/stale) |
| S005 | Suggestion | Concern group candidate (co-occurring prefixes) |

## Workflow: Arriving at a Corpus

1. `anneal status` -- orient: what exists, what's broken, convergence direction
2. `anneal check --active-only` -- actionable errors (skip terminal file noise)
3. `anneal find "" --status=raw` -- what needs processing?
4. `anneal get <label>` -- look up specific handles mentioned in tasks
5. `anneal impact <file>` -- check blast radius before editing

## Workflow: Before Editing

1. `anneal impact <file>` -- what depends on this?
2. `anneal map --around=<handle> --depth=1` -- see neighborhood
3. Edit the file
4. `anneal check --active-only` -- verify no new broken references

## Configuration (anneal.toml)

Located at the corpus root. Optional -- anneal works without it (existence lattice mode).

Key sections:
- `[convergence]` -- active/terminal status lists + pipeline ordering
- `[handles]` -- confirmed/rejected namespaces, linear (obligation) namespaces
- `[freshness]` -- staleness thresholds (warn/error days)
- `[frontmatter.fields]` -- map frontmatter fields to edge types
- `[concerns]` -- named handle groups for `anneal map --concern`

Run `anneal init` to generate from inferred structure, then tune manually.

## Rules

- Always use `--json` when parsing output programmatically
- `anneal status` appends a snapshot -- run it to build convergence history
- `anneal check` also appends snapshots (convergence tracking is automatic)
- Error count includes terminal file errors -- use `--active-only` for actionable count
- Use `anneal help <command>` for detailed documentation on any command
