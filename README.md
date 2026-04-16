# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown files, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It helps disconnected intelligences — agents across sessions with no shared memory — orient in a body of knowledge, recover relevant context quickly, and push it toward settledness.

## The problem

A knowledge corpus grows across many sessions. No single agent sees the full history. Documents reference each other, supersede each other, track obligations. Without tooling, every arriving agent has to read everything to understand what's settled, what's drifting, and what's connected to what.

`anneal` makes that orientation instant:

```
$ anneal status
 corpus  84 files, 1205 handles, 892 edges
         1140 active, 65 terminal
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

The installer is the primary path for prebuilt binaries. It installs to `~/.local/bin` by default, prints the exact target / URL / destination before it writes anything, fails fast on unsupported targets, and stays aligned with the published release matrix.

By default it installs only the binary. To also install the bundled anneal
skill, pass one or more explicit skill targets. Each target is a directory path
that anneal will populate with the bundled skill files.

Common variations:

```bash
# Install somewhere else
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | INSTALL_DIR="$HOME/bin" bash

# Same override, but passed as an installer flag
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --install-dir "$HOME/bin"

# Preview what would happen without writing anything
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --dry-run

# Install the bundled skill into agent-managed locations
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- \
  --skill-target "$HOME/.agents/skills/anneal" \
  --skill-target "$HOME/.codex/skills/anneal"
```

Binaries available for: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`.

### From source

```bash
git clone https://github.com/flowerornament/anneal.git
cd anneal
cargo install --path . --locked
```

### Nix

Run without installing:

```bash
nix run github:flowerornament/anneal
```

Install into your profile:

```bash
nix profile install github:flowerornament/anneal
```

Add as a flake input:

```nix
anneal = {
  url = "github:flowerornament/anneal";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

This installs the binary only unless you additionally wire skill installation
yourself. `anneal` keeps the same runtime config layout no matter how it was
installed:

- repo config: `anneal.toml` at the corpus root
- user config: `$XDG_CONFIG_HOME/anneal/config.toml`
- derived history: `$XDG_STATE_HOME/anneal/...`

### Nix + Home Manager

For a declarative Nix-native setup, use the exported Home Manager module. It
installs `anneal`, writes the same XDG user config that non-Nix setups use,
and can optionally install the bundled anneal skill into agent-managed skill
directories.

Add the flake input:

```nix
anneal = {
  url = "github:flowerornament/anneal";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then include the module in your Home Manager configuration:

```nix
{
  imports = [
    inputs.anneal.homeManagerModules.default
  ];

  programs.anneal = {
    enable = true;
    settings.state.historyDir = config.xdg.stateHome;
    skill.enable = true;
    skill.targets = [
      ".agents/skills/anneal"
      ".codex/skills/anneal"
    ];
  };
}
```

Available module options:

- `programs.anneal.enable`
- `programs.anneal.package`
- `programs.anneal.settings.state.historyMode`
- `programs.anneal.settings.state.historyDir`
- `programs.anneal.skill.enable`
- `programs.anneal.skill.targets`

Repo-owned corpus behavior still lives in `anneal.toml`. Skill targets are
home-relative symlink paths, so you can install the anneal skill anywhere your
agent tooling expects it.

If you already use Home Manager through nix-darwin, add the exported module to
`home-manager.sharedModules` (or otherwise make it available in your shared
Home Manager module set), then enable `programs.anneal` in the user config that
owns your shell environment.

## Quick start

```bash
# Corpus shape, health, and convergence direction
anneal status
anneal status --json --compact

# Per-area health profiles
anneal areas

# Actionable issues in active work
anneal check

# Resolve a specific handle
anneal get REQ-12
anneal get REQ-12 --context

# Search handle identities
anneal find ADR --limit 25

# Ask an ad hoc structural question
anneal query diagnostics --severity error
anneal query edges --kind DependsOn --confidence-gap

# Explain a derived result
anneal explain convergence
anneal explain diagnostic --id diag_deadbeef

# Reverse dependencies for a file or handle
anneal impact spec/api-v3.md

# Local graph neighborhood
anneal map --around=REQ-12 --depth=1
anneal map --render=text --full

# Linear namespace summary
anneal obligations

# Snapshot delta
anneal diff

# Infer configuration
anneal init
```

## Workflow

`anneal` supports a practical loop for corpus work:

1. Orient: run `anneal status` for the corpus dashboard, `anneal areas` for per-area health, then `anneal check` for actionable diagnostics.
2. Locate context: use `anneal get --context`, bounded `anneal find --limit ...`, `anneal query ...`, and `anneal map --around=...` to understand the specific files, labels, or structural patterns involved in the task.
3. Justify the current signal: use `anneal explain ...` when you need to know why a warning, suggestion, impact set, convergence signal, or obligation state exists.
4. Assess impact: run `anneal impact <file-or-handle>` before editing to see what depends on the thing you are about to change.
5. Verify: run `anneal check --file=...` for a local pass or `anneal check` for a broader pass after editing.
6. Review accumulated change: run `anneal diff` to see what changed since the last snapshot, even when no single agent saw those changes happen.

This is what the tool buys you in practice: quick context recovery, structural inspection, and safer edits in a corpus that outlives any one session.

## Concepts

**Handles** are the unit of knowledge. Five kinds:

| Kind     | Example                    | Description                                     |
| -------- | -------------------------- | ----------------------------------------------- |
| file     | `spec/api-v3.md`           | A markdown document                             |
| section  | `api-v3.md#authentication` | A heading within a file                         |
| label    | `REQ-12`                   | A cross-reference tag (namespace + number)      |
| version  | `v3`                       | A versioned artifact                            |
| external | `https://example.com/spec` | An external URL referenced from corpus metadata |

**Edges** are typed relationships between handles:

| Edge       | Meaning                          |
| ---------- | -------------------------------- |
| Cites      | References without dependency    |
| DependsOn  | Structural dependency            |
| Supersedes | Version chain (v3 supersedes v2) |
| Verifies   | Formal verification link         |
| Discharges | Obligation fulfillment           |

**Status** is a frontmatter `status:` field, partitioned into **active** (in-progress) and **terminal** (settled). When pipeline ordering is configured, `anneal` shows a histogram of handles flowing through status levels.

**Convergence** tracks whether the corpus is advancing (more handles reaching terminal state), holding (stable), or drifting (creation outpacing resolution). Measured by comparing snapshots over time.

## Commands

### `anneal status`

Single-screen dashboard for quick orientation. Shows corpus size, active/terminal partition, pipeline histogram, health (errors + warnings), convergence direction, and suggestion breakdown.

```
$ anneal status -v
 corpus  84 files, 1205 handles, 892 edges
         1140 active, 65 terminal
    pipeline  8 raw -> 5 draft -> 12 review -> 3 approved -> 6 published
              raw (8):
                notes/2025-01-15-auth-redesign.md
                notes/2025-01-20-rate-limiting.md
                ...
```

Appends a snapshot to local anneal history for convergence tracking. By default this is machine-local XDG state, not a repo file.

For agents and other tools, prefer:

```bash
anneal status --json --compact
```

### `anneal areas`

Per-area health profiles. Areas are auto-detected from the top-level directory structure — each subdirectory is an area, files at the corpus root are grouped under `(root)`. Each area gets a grade (A–D) based on error count, connectivity, and metadata coverage.

```
$ anneal areas
Area                 Files  Conn  Cross Grade  Signal
────────────────────────────────────────────────────────────────────────
synthesis/              34   1.5    793   [A]  healthy
implementation/         73   0.7    552   [C]  2 broken
compiler/               28   0.6    104   [B]  9 orphans
archive/                17   0.3      0   [B]  island, no active files
```

Grades:
- **A**: No errors, adequate connectivity, has active files
- **B**: No errors, but low connectivity, no active metadata, or elevated orphan count
- **C**: Has errors (E001/E002)
- **D**: Has errors and low connectivity

When `[concerns]` is configured in `anneal.toml`, concern groups can also act as areas.

| Flag                 | Effect                                          |
| -------------------- | ----------------------------------------------- |
| `--sort=files\|grade\|conn\|name` | Sort order (default: files descending) |
| `--include-terminal` | Include areas that contain only terminal files   |

### `anneal check`

Five check rules and five suggestion rules with compiler-style diagnostics:

```
$ anneal check
error[E001]: broken reference: auth-flow.md not found
  -> spec/api-v3.md
error[E001]: broken reference: REQ-99 not found
  -> decisions/ADR-005.md

23 errors (23 in active files), 11 warnings, 1 info, 7 suggestions
```

`anneal check` reports active-file diagnostics by default. Use `--include-terminal` when you want the full picture, including settled material.

For interactive health checks, prefer the plain-text report. JSON is summary-first by default, then expands deliberately with explicit flags.

| Flag                 | Effect                                            |
| -------------------- | ------------------------------------------------- |
| `--include-terminal` | Include diagnostics from terminal (settled) files |
| `--active-only`      | Skip diagnostics from terminal (settled) files    |
| `--errors-only`      | Errors only (for CI/pre-commit)                   |
| `--suggest`          | Structural suggestions only (S001–S005)           |
| `--stale`            | Staleness warnings only (W001)                    |
| `--obligations`      | Obligation diagnostics only (E002/I002)           |
| `--file=path.md`     | Scope diagnostics to a single file                |
| `--diagnostics`      | Include bounded diagnostics in JSON output        |
| `--limit=N`          | Cap JSON diagnostic samples                       |
| `--extractions-summary` | Include aggregate extraction facts in JSON output |
| `--full-extractions` | Include full extraction payloads in JSON output   |
| `--full`             | Explicitly request full diagnostics + extractions |

### `anneal get`

Resolve a handle and show bounded graph context:

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

Useful expansions:

```bash
anneal get REQ-12 --context            # compact agent briefing
anneal get REQ-12 --refs               # bounded incoming/outgoing references
anneal get REQ-12 --trace --full       # full adjacency
anneal get REQ-12 --json               # summary JSON with counts and samples
```

`get` includes a snippet when anneal can extract one from the source file. JSON reports edge counts and truncation by default instead of dumping the full adjacency list.

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

Traverses the edge kinds configured in `[impact] traverse` in `anneal.toml` (defaults to DependsOn, Supersedes, Verifies). Does not traverse Cites by default (citations are not dependencies).

### `anneal map`

Render or summarize the knowledge graph:

```bash
anneal map                                    # Graph summary
anneal map --around=REQ-12 --depth=1          # Focused text neighborhood
anneal map --render=text --full               # Full active graph (text)
anneal map --render=dot --full | dot -Tpng -o g.png
                                               # Graphviz PNG
anneal map --json                             # Summary JSON
anneal map --json --nodes --limit-nodes 50    # Structured node sample
```

### `anneal diff`

What changed since last session:

```
$ anneal diff
Since last snapshot:
  Handles: +12 (+8 active, +4 terminal)
  State: draft: 3 -> 5 (+2)
  Obligations: +0 outstanding, +2 discharged, +0 mooted
  Edges: +47
```

Three reference modes: last snapshot (default), `--days=N` (time-based), or a git ref (`HEAD~3`, `main`) for structural diff.

By default, `diff` reads the same machine-local snapshot history used by `status` and `check`. If you previously used repo-local `.anneal/history.jsonl`, anneal will continue reading that legacy history for compatibility.

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
anneal find ADR                    # Bounded active ADR-* sample
anneal find ADR --limit 50         # Larger sample
anneal find "" --status=draft      # Broad but narrowed query
anneal find "" --kind=file --all   # All files including terminal
anneal find "" --status=draft --json
                                   # JSON sample with counts, truncation, and facets
```

`find` is bounded by default. Use `--offset` to page through results or `--full` when you intentionally want the full match set.

### `anneal query`

Run bounded structural selectors over the current graph and freshly derived diagnostics:

```bash
anneal query handles --kind label --namespace REQ
anneal query edges --kind DependsOn --confidence-gap
anneal query diagnostics --severity warning
anneal query obligations --undischarged
anneal query suggestions --code S001
```

`query` is for graph-shaped questions that are too specific for `status`, too broad for `get`, and outside `find`'s identity-search role. All query domains inherit anneal's bounded-output defaults: `--limit`, `--offset`, `--scope`, and explicit `--full`.

### `anneal explain`

Explain why anneal produced a derived result:

```bash
anneal explain diagnostic --id diag_deadbeef
anneal explain impact spec/api-v3.md
anneal explain convergence
anneal explain obligation REQ-12
anneal explain suggestion --id sugg_deadbeef
```

`explain` is the provenance companion to anneal's structural outputs. It does not search semantically; it shows the handles, edges, states, rules, and snapshots behind a specific result.

### `anneal init`

Generate `anneal.toml` from inferred corpus structure:

```bash
anneal init              # Write anneal.toml
anneal init --dry-run    # Preview without writing
```

Infers active/terminal partition, label namespaces, and frontmatter field mappings. Pipeline ordering and linear namespaces require manual tuning.

## Configuration

`anneal.toml` at the corpus root. Optional — `anneal` works without it (existence lattice mode: reference checking only).

`anneal` separates repo-owned corpus behavior from machine-local runtime preferences:

- Repo config lives in `anneal.toml` at the corpus root.
- User config lives in XDG config:
  - `$XDG_CONFIG_HOME/anneal/config.toml`
  - fallback `~/.config/anneal/config.toml`
- Derived snapshot history lives in XDG state by default:
  - `$XDG_STATE_HOME/anneal/...`
  - fallback `~/.local/state/anneal/...`

This keeps automatic convergence tracking and `anneal diff` useful without dirtying the git worktree during normal use.

```toml
exclude = ["**/README.md"]  # glob patterns and directory names to skip

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

[frontmatter.fields.synthesizes]
edge_kind = "Synthesizes"  # custom edge kinds are accepted — any string works
direction = "inverse"

[concerns]
api = ["REQ", "ADR"]

[impact]
traverse = ["DependsOn", "Supersedes", "Verifies", "Synthesizes", "Implements"]

[state]
history_mode = "xdg"  # optional: xdg | repo | off
```

`anneal.toml` controls corpus semantics: statuses, namespaces, suppressions, frontmatter mappings, concern groups, impact traversal, file exclusions, and the history backend mode (`xdg`, `repo`, or `off`). The `exclude` list accepts both plain directory names (e.g. `"vendor"`) and glob patterns (e.g. `"**/README.md"`) — glob entries are matched against paths relative to root and prevent matched files from entering the graph entirely.

Five edge kinds have built-in diagnostic behavior: `Cites`, `DependsOn`, `Supersedes`, `Verifies`, `Discharges`. Any other `edge_kind` string (e.g. `Synthesizes`, `Flags`, `Implements`) is accepted as a custom kind — indexed in the graph and queryable via `anneal query edges --kind=<name>`, but with no built-in checks. W001 (stale dependency) fires only on `DependsOn` edges.

Area and temporal tuning:

```toml
[areas]
orphan_threshold = 5  # orphan count that downgrades an area to grade B

[temporal]
recent_days = 7  # default window for --recent (days)
```

The `[impact] traverse` list controls which edge kinds `anneal impact` follows when computing affected handles. When absent, falls back to the built-in default (`DependsOn`, `Supersedes`, `Verifies`). Corpora using custom edge kinds for structural relationships should configure this to get accurate impact analysis.

If you want repo-local snapshots, set:

```toml
[state]
history_mode = "repo"
```

User config controls machine-local preferences such as the base directory for derived history:

```toml
[state]
history_dir = "/Users/alice/.local/state"
```

Important boundary: repo config can choose whether history is machine-local, repo-local, or disabled, but it cannot choose an arbitrary machine-local path. Only user config can override `history_dir`.

For Nix users, the Home Manager module writes this same user config file
declaratively. It does not introduce a separate anneal-specific config path or
runtime mode.

## Diagnostic reference

### Check rules

| Code | Severity | Description                                                     |
| ---- | -------- | --------------------------------------------------------------- |
| E001 | Error    | Broken reference — handle not found                             |
| E002 | Error    | Undischarged obligation — linear handle without Discharges edge |
| W001 | Warning  | Stale dependency — active handle has DependsOn edge to terminal |
| W002 | Warning  | Confidence gap — higher pipeline level depends on lower         |
| W003 | Warning  | Missing frontmatter — file without `status:` field              |
| I001 | Info     | Section reference summary                                       |
| I002 | Info     | Multiple discharges on single obligation                        |

### Suggestion rules

| Code | Description                                                         |
| ---- | ------------------------------------------------------------------- |
| S001 | Orphaned handles — labels/versions with no incoming edges           |
| S002 | Candidate namespaces — recurring prefix not in confirmed list       |
| S003 | Pipeline stalls — status level with no outflow to next level        |
| S004 | Abandoned namespaces — all members terminal or stale                |
| S005 | Concern group candidates — label prefixes co-occurring across files |

## JSON output

All commands support `--json` for machine consumption. Risky commands now use progressive disclosure:

- default JSON is bounded and explicit about truncation
- expansion flags like `--diagnostics`, `--refs`, `--nodes`, and `--full` request more detail
- `--pretty` exists for humans; plain `--json` stays compact for tools

Use JSON for compact facts and jq-filtered summaries rather than dumping full structured output back into chat:

```bash
anneal status --json --compact | jq '.convergence'
anneal check --active-only --json | jq '.summary'
anneal check --active-only --json --diagnostics --limit 25 | jq '.diagnostics[:5]'
anneal find ADR --json | jq '._meta'
anneal map --json | jq '{nodes, edges, by_kind}'
```

## Design

On every invocation, `anneal` walks a directory of markdown files and builds a typed knowledge graph in memory:

1. **Parse** — split YAML frontmatter from body text, extract typed references, and scan markdown structure with pulldown-cmark
2. **Resolve** — infer label namespaces from sequential cardinality (REQ-1 through REQ-50 is a namespace; SHA-256 is not), resolve cross-references to graph nodes with deterministic fallback candidates
3. **Lattice** — partition observed `status:` values into active and terminal sets, optionally infer from directory conventions (files only in `archive/` → terminal)
4. **Check** — run five consistency rules and five suggestion rules against the graph, then apply optional suppressions
5. **Snapshot** — capture counts to local anneal history for convergence tracking over time

No persistent database. The graph is ephemeral — rebuilt from files each run. The only state is the append-only snapshot history, which is derived, deletable, and machine-local by default.

The underlying model borrows a simple idea from type theory and static analysis: statuses are ordered, so the tool can compare neighboring handles and ask questions like "is this dependency less settled than the thing that depends on it?" The active/terminal split is the simplest form of that ordering. A configured pipeline turns it into a richer progression. Convergence tracking measures how the population moves through that progression over time. Checks stay local — they compare a handle with its immediate neighbors rather than trying to solve a whole-program global proof — which keeps the tool fast and predictable.

### Architecture

```
src/
  handle.rs       Handle, HandleKind, NodeId
  graph.rs        DiGraph with dual adjacency lists
  parse.rs        Frontmatter + markdown scanning
  extraction.rs   Typed reference extraction
  resolve.rs      Handle resolution across namespaces
  lattice.rs      Convergence lattice, freshness
  config.rs       anneal.toml parsing
  checks.rs       Check rules + suggestion rules
  impact.rs       Reverse graph traversal
  area.rs         Per-area health computation + grading
  snapshot.rs     JSONL history, convergence summary
  cli.rs          Commands + output formatting
  style.rs        Terminal styling via console crate
  main.rs         Entry point + CLI dispatch
```

The codebase is intentionally straightforward: parse markdown, build an in-memory graph, run local checks, and emit either human-readable output or JSON. The direct dependency set is small, the test suite covers the core graph/check/CLI paths, and the tool is fast enough for interactive use on a few-hundred-file corpus.

## License

MIT
