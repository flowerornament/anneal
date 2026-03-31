# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown files, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It helps disconnected intelligences — agents across sessions with no shared memory — orient in a body of knowledge and push it toward settledness.

## The problem

A knowledge corpus grows across many sessions. No single agent sees the full history. Documents reference each other, supersede each other, track obligations. Without tooling, every arriving agent has to read everything to understand what's settled, what's drifting, and what's connected to what.

`anneal` makes that orientation instant:

```
$ anneal status
 corpus  84 files, 1205 handles, 892 edges
         1140 active, 65 frozen
    pipeline  8 raw -> 5 draft -> 12 review -> 3 approved -> 6 published

 health  23 errors, 11 warnings

 convergence  advancing (resolution +4, creation +2, obligations 0)

 suggestions  7
       4  S001 orphaned handles
       2  S003 pipeline stalls
       1  S005 concern group candidates
```

## Install

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash
```

Installs to `~/.local/bin`. Override with `INSTALL_DIR=/usr/local/bin`.

Binaries available for: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`.

### From source

```bash
cargo install --path . --locked
```

### Nix

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
anneal get REQ-12

# Search handles
anneal find ADR

# What depends on this file?
anneal impact spec/api-v3.md

# Visualize a handle's neighborhood
anneal map --around=REQ-12 --depth=1

# See obligation status for linear namespaces
anneal obligations

# What changed since last session?
anneal diff

# Generate config from inferred structure
anneal init
```

## Concepts

**Handles** are the unit of knowledge. Five kinds:

| Kind | Example | Description |
|------|---------|-------------|
| file | `spec/api-v3.md` | A markdown document |
| section | `api-v3.md#authentication` | A heading within a file |
| label | `REQ-12` | A cross-reference tag (namespace + number) |
| version | `v3` | A versioned artifact |
| external | `https://example.com/spec` | An external URL referenced from corpus metadata |

**Edges** are typed relationships between handles:

| Edge | Meaning |
|------|---------|
| Cites | References without dependency |
| DependsOn | Structural dependency |
| Supersedes | Version chain (v3 supersedes v2) |
| Verifies | Formal verification link |
| Discharges | Obligation fulfillment |

**Status** is a frontmatter `status:` field, partitioned into **active** (in-progress) and **terminal** (settled). When pipeline ordering is configured, `anneal` shows a histogram of handles flowing through status levels.

**Convergence** tracks whether the corpus is advancing (more handles reaching terminal state), holding (stable), or drifting (creation outpacing resolution). Measured by comparing snapshots over time.

## Commands

### `anneal status`

Single-screen dashboard for arriving agents. Shows corpus size, active/frozen partition, pipeline histogram, health (errors + warnings), convergence direction, and suggestion breakdown.

```
$ anneal status -v
 corpus  84 files, 1205 handles, 892 edges
         1140 active, 65 frozen
    pipeline  8 raw -> 5 draft -> 12 review -> 3 approved -> 6 published
              raw (8):
                notes/2025-01-15-auth-redesign.md
                notes/2025-01-20-rate-limiting.md
                ...
```

Appends a snapshot to `.anneal/history.jsonl` for convergence tracking.

### `anneal check`

Five check rules and five suggestion rules with compiler-style diagnostics:

```
$ anneal check --active-only
error[E001]: broken reference: auth-flow.md not found
  -> spec/api-v3.md
error[E001]: broken reference: REQ-99 not found
  -> decisions/ADR-005.md

23 errors (23 in active files), 11 warnings, 1 info, 7 suggestions
```

| Flag | Effect |
|------|--------|
| `--active-only` | Skip diagnostics from terminal (settled) files |
| `--errors-only` | Errors only (for CI/pre-commit) |
| `--suggest` | Structural suggestions only (S001–S005) |
| `--stale` | Staleness warnings only (W001) |
| `--obligations` | Obligation diagnostics only (E002/I002) |
| `--file=path.md` | Scope diagnostics to a single file |

### `anneal get`

Resolve a handle and show its edges:

```
$ anneal get REQ-12
REQ-12 (label)
  File: requirements/REQUIREMENTS.md
  Snippet: Requirements (REQ-): | REQ-12 | Authentication requests must be signed | draft |
  Incoming:
    Cites <- spec/api-v3.md
    Cites <- decisions/ADR-003.md
    Cites <- notes/2025-01-15-auth-redesign.md
```

`get` now includes a snippet when anneal can extract one from the source file.

### `anneal impact`

Reverse dependency traversal — what's affected if a handle changes:

```
$ anneal impact spec/api-v3.md
Directly affected (depend on this):
  spec/api-v2.md
Indirectly affected (depend on the above):
  spec/api-v1.md
  archive/api-draft.md
```

Traverses DependsOn, Supersedes, and Verifies edges in reverse. Does not traverse Cites (citations are not dependencies).

### `anneal map`

Render the knowledge graph. Text for terminals, DOT for graphviz:

```bash
anneal map --around=REQ-12 --depth=1          # Text neighborhood
anneal map --format=dot | dot -Tpng -o g.png  # Graphviz PNG
anneal map --concern=api                      # Concern group subgraph
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

### `anneal obligations`

Summarize outstanding, discharged, and mooted obligations for configured linear namespaces:

```bash
$ anneal obligations
Obligations: 3 outstanding, 12 discharged, 1 mooted

REQ
  Outstanding:
    REQ-14
    REQ-19
    REQ-22
```

Use `--json` for machine-readable totals and per-namespace buckets.

### `anneal find`

Search handle identities:

```bash
anneal find ADR                    # All active ADR-* labels
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
active = ["draft", "review", "approved"]
terminal = ["published", "archived", "superseded"]
ordering = ["raw", "draft", "review", "approved", "published"]

[handles]
confirmed = ["REQ", "ADR", "RFC"]
rejected = ["SHA", "GPT"]
linear = ["REQ"]  # obligations: must be discharged exactly once

[suppress]
codes = ["I001"]  # optional global suppressions

[[suppress.rules]]
code = "E001"
target = "synthesis/v17.md"

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
api = ["REQ", "ADR"]
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
anneal --json get REQ-12 | jq '.edges'
anneal --json obligations | jq '.total_outstanding'
```

## Design

On every invocation, `anneal` walks a directory of markdown files and builds a typed knowledge graph in memory:

1. **Parse** — split YAML frontmatter from body text, extract typed references, and scan markdown structure with pulldown-cmark
2. **Resolve** — infer label namespaces from sequential cardinality (REQ-1 through REQ-50 is a namespace; SHA-256 is not), resolve cross-references to graph nodes with deterministic fallback candidates
3. **Lattice** — partition observed `status:` values into active and terminal sets, optionally infer from directory conventions (files only in `archive/` → terminal)
4. **Check** — run five consistency rules and five suggestion rules against the graph, then apply optional suppressions
5. **Snapshot** — capture counts to `.anneal/history.jsonl` for convergence tracking over time

No persistent database. The graph is ephemeral — rebuilt from files each run. The only state is the append-only snapshot history, which is derived and deletable.

The underlying model borrows from graded type systems: a document's convergence state has the same algebraic structure as a resource grade — both are values in bounded lattices that compose through meet/join operations. The active/terminal partition is a two-point lattice. Pipeline ordering extends it to a chain. Convergence tracking measures how the population moves through the lattice over time. Checks are local consistency rules — they examine a handle and its immediate neighbors, not global properties — which keeps them fast and compositional.

### Architecture

```
src/
  handle.rs       Handle, HandleKind, NodeId          (~100 lines)
  graph.rs        DiGraph with dual adjacency lists   (~130 lines)
  parse.rs        Frontmatter + markdown scanning     (~900 lines)
  extraction.rs   Typed reference extraction          (~150 lines)
  resolve.rs      Handle resolution across namespaces (~350 lines)
  lattice.rs      Convergence lattice, freshness      (~200 lines)
  config.rs       anneal.toml parsing                 (~190 lines)
  checks.rs       5 check rules + 5 suggestion rules  (~700 lines)
  impact.rs       Reverse graph traversal             (~50 lines)
  snapshot.rs     JSONL history, convergence summary   (~500 lines)
  cli.rs          9 commands + output formatting      (~2800 lines)
  style.rs        Terminal styling via console crate   (~30 lines)
  main.rs         Entry point + CLI dispatch          (~700 lines)
```

~8000 lines of Rust. 152 tests (150 passing, 2 external-corpus smoke tests ignored without external fixtures). 11 dependencies. <50ms on a 262-file corpus.

## License

MIT
