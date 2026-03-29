# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown files, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It helps disconnected intelligences — agents across sessions with no shared memory — orient in a body of knowledge and push it toward settledness.

## The problem

A knowledge corpus grows across many sessions. No single agent sees the full history. Documents reference each other, supersede each other, track obligations. Without tooling, every arriving agent has to read everything to understand what's settled, what's drifting, and what's connected to what.

`anneal` makes that orientation instant:

```
$ anneal status
 corpus  262 files, 9882 handles, 6992 edges
         9785 active, 97 frozen
    pipeline  10 raw -> 3 draft -> 6 exploratory -> 4 research -> 8 active
              -> 7 reference -> 10 stable -> 9 decision -> 11 authoritative

 health  1009 errors, 147 warnings

 convergence  holding (resolution +0, creation +0, obligations 0)

 suggestions  38
      25  S001 orphaned handles
       8  S003 pipeline stalls
       5  S005 concern group candidates
```

## Install

### From source

```bash
cargo install --path .
```

### Nix

```bash
nix run github:flowerornament/anneal -- status
```

Or add to your flake inputs:

```nix
anneal = {
  url = "github:flowerornament/anneal";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

## Quick start

```bash
# Orient: what exists, what's broken, what direction
anneal status

# Find broken references and structural issues
anneal check

# Look up a specific handle
anneal get OQ-64

# Search handles
anneal find FM

# What depends on this file?
anneal impact formal-model/v17.md

# Visualize a handle's neighborhood
anneal map --around=OQ-64 --depth=2

# What changed since last session?
anneal diff

# Generate config from inferred structure
anneal init
```

## Concepts

**Handles** are the unit of knowledge. Four kinds:

| Kind | Example | Description |
|------|---------|-------------|
| file | `formal-model/v17.md` | A markdown document |
| section | `v17.md#definitions` | A heading within a file |
| label | `OQ-64` | A cross-reference tag (namespace + number) |
| version | `v17` | A versioned artifact |

**Edges** are typed relationships between handles:

| Edge | Meaning |
|------|---------|
| Cites | References without dependency |
| DependsOn | Structural dependency |
| Supersedes | Version chain (v17 supersedes v16) |
| Verifies | Formal verification link |
| Discharges | Obligation fulfillment |

**Status** is a frontmatter `status:` field, partitioned into **active** (in-progress) and **terminal** (settled). When pipeline ordering is configured, `anneal` shows a histogram of handles flowing through status levels.

**Convergence** tracks whether the corpus is advancing (more handles reaching terminal state), holding (stable), or drifting (creation outpacing resolution). Measured by comparing snapshots over time.

## Commands

### `anneal status`

Single-screen dashboard for arriving agents. Shows corpus size, active/frozen partition, pipeline histogram, health (errors + warnings), convergence direction, and suggestion breakdown.

```
$ anneal status -v
 corpus  262 files, 9882 handles, 6992 edges
         9785 active, 97 frozen
    pipeline  10 raw -> 3 draft -> ...
              raw (10):
                formal-model/prior/RESTRUCTURE-PROPOSAL.md
                research-log/2026-03-15-research-spikes-execution.md
                ...
```

Appends a snapshot to `.anneal/history.jsonl` for convergence tracking.

### `anneal check`

Five check rules and five suggestion rules with compiler-style diagnostics:

```
$ anneal check --active-only
error[E001]: broken reference: summary.md not found
  -> archive/research/README.md

799 errors (799 in active files), 147 warnings, 1 info, 38 suggestions
```

| Flag | Effect |
|------|--------|
| `--active-only` | Skip diagnostics from terminal (settled) files |
| `--errors-only` | Errors only (for CI/pre-commit) |
| `--suggest` | Structural suggestions only (S001–S005) |
| `--stale` | Staleness warnings only (W001) |
| `--obligations` | Obligation diagnostics only (E002/I002) |

### `anneal get`

Resolve a handle and show its edges:

```
$ anneal get FM-17
FM-17 (label)
  File: synthesis/2026-03-15-architectural-reconciliation.md
  Incoming:
    Cites <- DESIGN-GOALS.md
    Cites <- formal-model/prior/notes-toward-v12.md
    Cites <- formal-model/history/murail-formal-model-v11.md
    Cites <- LABELS.md
    Cites <- OPEN-QUESTIONS.md
```

### `anneal impact`

Reverse dependency traversal — what's affected if a handle changes:

```
$ anneal impact formal-model/murail-formal-model-v17.md
Directly affected (depend on this):
  formal-model/history/murail-formal-model-v16.md
Indirectly affected (depend on the above):
  formal-model/history/murail-formal-model-v15.md
  formal-model/history/murail-formal-model-v14.md
  ...
```

Traverses DependsOn, Supersedes, and Verifies edges in reverse. Does not traverse Cites (citations are not dependencies).

### `anneal map`

Render the knowledge graph. Text for terminals, DOT for graphviz:

```bash
anneal map --around=FM-17 --depth=1                  # Text neighborhood
anneal map --format=dot | dot -Tpng -o graph.png     # Graphviz PNG
anneal map --concern=formal-model                    # Concern group subgraph
```

### `anneal diff`

What changed since last session:

```
$ anneal diff
Since last snapshot:
  Handles: +12 (+8 active, +4 frozen)
  State: draft: 3 -> 5 (+2)
  Obligations: +0 outstanding, +2 discharged, +0 mooted
  Edges: +47
```

Three reference modes: last snapshot (default), `--days=N` (time-based), or a git ref (`HEAD~3`, `main`) for structural diff.

### `anneal find`

Search handle identities:

```bash
anneal find FM                     # All active FM-* labels
anneal find "" --status=draft      # All handles with status "draft"
anneal find "" --kind=file --all   # All files including terminal
```

### `anneal init`

Generate `anneal.toml` from inferred corpus structure:

```bash
anneal init              # Write anneal.toml
anneal init --dry-run    # Preview without writing
```

Infers active/terminal partition, label namespaces, and frontmatter field mappings. Pipeline ordering and linear namespaces require manual tuning.

## Configuration

`anneal.toml` at the corpus root. Optional — `anneal` works without it (existence lattice mode: reference checking only).

```toml
[convergence]
active = ["draft", "active", "stable"]
terminal = ["archived", "superseded", "decision"]
ordering = ["raw", "draft", "active", "stable", "decision"]

[handles]
confirmed = ["OQ", "FM", "SR", "DG"]
rejected = ["SHA", "GPT"]
linear = ["OQ"]  # obligations: must be discharged exactly once

[freshness]
warn = 30   # days before staleness warning
error = 90  # days before staleness error

[frontmatter.fields.depends-on]
edge_kind = "DependsOn"
direction = "forward"

[frontmatter.fields.superseded-by]
edge_kind = "Supersedes"
direction = "forward"

[concerns]
formal = ["FM", "A", "D"]
```

## Diagnostic reference

### Check rules

| Code | Severity | Description |
|------|----------|-------------|
| E001 | Error | Broken reference — handle not found |
| E002 | Error | Undischarged obligation — linear handle without Discharges edge |
| W001 | Warning | Stale reference — active handle references terminal one |
| W002 | Warning | Confidence gap — higher pipeline level depends on lower |
| W003 | Warning | Missing frontmatter — file without `status:` field |
| I001 | Info | Section reference summary |
| I002 | Info | Multiple discharges on single obligation |

### Suggestion rules

| Code | Description |
|------|-------------|
| S001 | Orphaned handles — labels/versions with no incoming edges |
| S002 | Candidate namespaces — recurring prefix not in confirmed list |
| S003 | Pipeline stalls — status level with no outflow to next level |
| S004 | Abandoned namespaces — all members terminal or stale |
| S005 | Concern group candidates — label prefixes co-occurring across files |

## JSON output

All commands support `--json` for machine consumption:

```bash
anneal --json status | jq '.convergence'
anneal --json check --active-only | jq '.errors'
anneal --json get FM-17 | jq '.edges'
```

## Design

`anneal` is built on the formal analogy between graded type systems and knowledge management: a document's convergence state has the same algebraic structure as a program's resource grade — both are values in bounded lattices that compose through meet/join operations.

The graph is computed from files on every invocation. No persistent database. The only state is `.anneal/history.jsonl` — append-only, derived, deletable.

### Architecture

```
src/
  handle.rs       Handle, HandleKind, NodeId          (~100 lines)
  graph.rs        DiGraph with dual adjacency lists   (~130 lines)
  parse.rs        Frontmatter + RegexSet scanning     (~700 lines)
  resolve.rs      Handle resolution across namespaces (~350 lines)
  lattice.rs      Convergence lattice, freshness      (~200 lines)
  config.rs       anneal.toml parsing                 (~190 lines)
  checks.rs       5 check rules + 5 suggestion rules  (~700 lines)
  impact.rs       Reverse graph traversal             (~50 lines)
  snapshot.rs     JSONL history, convergence summary   (~500 lines)
  cli.rs          8 commands + output formatting      (~2500 lines)
  style.rs        Terminal styling via console crate   (~30 lines)
  main.rs         Entry point + CLI dispatch          (~700 lines)
```

~7200 lines of Rust. 75 tests. 11 dependencies. <50ms on a 262-file corpus.

## License

MIT
