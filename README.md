# anneal

Programmable corpus runtime for agents.

`anneal` reads a knowledge corpus, turns it into typed facts, evaluates a
Datalog-style standard library, and exposes a small set of agent-friendly
commands for orientation, retrieval, health, and extension. It is built for
disconnected intelligences: agents across sessions with no shared memory that
need to recover what matters, read enough context, and leave the corpus more
settled than they found it.

## Why

A corpus grows across many sessions. Files reference each other, supersede each
other, encode obligations, drift out of date, and accumulate local vocabulary.
Without tooling, every arriving agent has to page through the same material and
rebuild the same map.

`anneal` gives the arriving agent a runtime instead:

```bash
anneal context "v17 conformance audit"
anneal search "release blocker" --limit 5
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
```

The same substrate also keeps the older corpus-health loop reachable during the
v2 migration window:

```bash
anneal status --json --compact
anneal check --scope=active
anneal get design.md --context
```

## Install

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash
```

The installer is the primary path for prebuilt binaries. It installs to
`~/.local/bin` by default, prints the target, URL, and destination before it
writes anything, fails fast on unsupported targets, and stays aligned with the
published release matrix.

By default it installs only the binary. To also install the bundled anneal
skill, pass one or more explicit skill targets. Each target is a directory path
that anneal will populate with the bundled skill files.

```bash
# Install somewhere else
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | INSTALL_DIR="$HOME/bin" bash

# Same override, passed as an installer flag
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --install-dir "$HOME/bin"

# Preview without writing anything
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --dry-run

# Install the bundled skill into agent-managed locations
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- \
  --skill-target "$HOME/.agents/skills/anneal" \
  --skill-target "$HOME/.codex/skills/anneal"
```

Binaries are published for:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

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

### Nix + Home Manager

Use the exported Home Manager module for a declarative setup. It installs the
binary, writes the same XDG user config that non-Nix setups use, and can
optionally install the bundled skill into agent-managed skill directories.

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

Repo-owned corpus behavior still lives in `anneal.toml`. User preferences and
derived history live under XDG paths.

## Agent Loop

Use the narrowest command that answers the next question.

```bash
# 1. One-call orientation
anneal context "find the most urgent thing blocking v17 conformance"

# 2. Retrieval when you need to inspect a specific candidate
anneal search "v17 conformance audit" --limit 3
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --budget 4000

# 3. Graph and health checks
anneal H 2026-05-13-corpus-runtime.md
anneal broken
anneal work
anneal trend  # emits rows when snapshot history exists

# 4. Introspection before writing custom rules
anneal describe convergence
anneal sources
anneal verbs
anneal schema

# 5. Raw Datalog when the built-in verbs are too coarse
anneal -e '? *handle{id: h, kind: "file", status: s}.'
```

The v2 surface is NDJSON-first. Each row is a standalone JSON object. Use
`--pretty` when a human needs to read the stream directly.

## Commands

### `anneal`

Compact corpus dashboard from the standard library's `anneal` verb. Use this
when the question is "what state is this corpus in right now?"

### `anneal context GOAL`

Cold-agent orientation in one call. `context` composes `search`, graph
neighborhood, and bounded `read` spans into one grouped response.

Useful flags:

- `--budget N`: context budget hint; v2 derives a per-hit read cap from it
- `--hits N`: number of search winners
- `--neighborhood-depth N`: graph distance around winners
- `--include-low-confidence`: include low-confidence search hits

### `anneal search TEXT`

Content retrieval over stored `*content`, `*span`, and metadata. This is not
the legacy identity search. It returns ranked hits with handle, span, score,
reason, field, and low-confidence marker.

```bash
anneal search "conformance audit" --limit 5
```

### `anneal read HANDLE`

Budgeted content retrieval for a handle.

```bash
anneal read formal-model/v17.md --budget 4000
```

`read` is bounded by default so agents can safely compose it with search
without dumping the whole corpus.

### `anneal H HANDLE`

Handle neighborhood summary. Use it after search or when a handle is already
known and relationship shape matters more than text.

### Starter Verbs

The standard library ships starter verbs as ordinary `@verb` definitions:

- `anneal`: compact dashboard
- `H`: handle neighborhood
- `find`: identity-oriented handle lookup
- `work`: ranked work candidates
- `blocked`: blockers for a handle or corpus
- `broken`: diagnostic gate
- `trend`: convergence movement rows when snapshot history exists
- `context`: cold-agent retrieval bundle

Project verbs declared in `anneal.dl` are surfaced the same way as standard
library verbs.

### Introspection

```bash
anneal describe convergence
anneal sources
anneal verbs
anneal schema
```

Use introspection before inventing a query. Agents should be able to discover
which sources are linked, which verbs exist, what a predicate means, and which
capabilities a command needs.

### Raw Queries

```bash
anneal -e '? *handle{id: h, kind: "file"}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
```

The query language is Datalog-shaped. Stored source facts use `*` prefixes, for
example `*handle`, `*edge`, `*meta`, `*content`, `*span`, `*config`,
`*snapshot`, and `*generation`. Derived predicates come from the standard
library, project `anneal.dl`, and inline query-local rules.

### Compatibility Commands

During the v2 migration window the v1 corpus-health commands remain available:

```bash
anneal status
anneal check --scope=active
anneal get HANDLE --context
anneal find TEXT --limit 25
anneal map --around=HANDLE
anneal impact HANDLE
anneal diff --days=7
anneal obligations
anneal init
anneal prime
```

Use them when you need exact v1 behavior or release compatibility. New agent
workflows should prefer `context`, `search`, `read`, verbs, and raw Datalog.

## Project Extension

`anneal.dl` lives at the corpus root. It can declare discovery facts, project
rules, and verbs.

```dl
# discovery facts are consumed by adapters before extraction
md.file_extension(".md").
md.scan_root(".").
md.scan_exclude("node_modules").
md.linear_namespace("OQ").

# rules run after source facts exist
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

Load order is fixed: config and discovery facts first, source extraction
second, standard library and project rules third, evaluation last. Project
predicates shadow prelude predicates by name and arity.

## Configuration

`anneal.toml` at the corpus root controls corpus semantics. It is optional;
without it, anneal falls back to an existence lattice and reference checking.

```toml
exclude = ["vendor", "**/README.md"]

[convergence]
active = ["draft", "review", "approved"]
terminal = ["published", "archived", "superseded"]
ordering = ["raw", "draft", "review", "approved", "published"]

[convergence.descriptions]
draft = "Under construction; may change substantially"
approved = "Settled primary artifact; changes require review"
archived = "Superseded or retired; no further changes expected"

[handles]
confirmed = ["REQ", "ADR", "RFC"]
rejected = ["SHA", "GPT"]
linear = ["REQ"]

[frontmatter.fields.depends-on]
edge_kind = "DependsOn"
direction = "forward"

[frontmatter.fields.superseded-by]
edge_kind = "Supersedes"
direction = "forward"

[impact]
traverse = ["DependsOn", "Supersedes", "Verifies"]

[state]
history_mode = "xdg"  # xdg | repo | off
```

User config lives at `$XDG_CONFIG_HOME/anneal/config.toml` with fallback
`~/.config/anneal/config.toml`. Derived snapshot history lives under
`$XDG_STATE_HOME/anneal/...` with fallback `~/.local/state/anneal/...`.

Repo config can choose whether history is machine-local, repo-local, or
disabled. Only user config can choose an arbitrary machine-local history path.

## Stored Facts

Adapters emit stored facts. Rules and queries consume them.

- `*handle`: handle id, kind, status, namespace, file, line, area, summary,
  identity, revision, generation
- `*edge`: typed handle relationship
- `*meta`: frontmatter and extracted metadata
- `*content`: bounded text chunks
- `*span`: addressable content spans
- `*concern`: concern-group membership
- `*config`: ordered runtime config facts
- `*snapshot`: history rows for `at(...)`
- `*generation`: source refresh bookkeeping

Every stored row carries enough identity to distinguish corpus, source,
origin, revision, and generation. Engine choice is internal to `anneal-core`;
dynamic IR owns prelude, project, and inline rules.

## Diagnostics

The v2 `checks.dl` catalog mirrors the shipped v1 diagnostic membership.

| Code | Severity | Description |
| --- | --- | --- |
| E001 | Error | Broken reference |
| E002 | Error | Undischarged obligation |
| W001 | Warning | Active handle depends on terminal handle |
| W002 | Warning | Higher lifecycle state depends on lower state |
| W003 | Warning | Missing frontmatter |
| W004 | Warning | Malformed or suspicious frontmatter value |
| I001 | Info | Section reference summary |
| I002 | Info | Multiple discharges on one obligation |
| S001 | Suggestion | Orphaned handle |
| S002 | Suggestion | Candidate namespace |
| S003 | Suggestion | Pipeline stall |
| S004 | Suggestion | Abandoned namespace |
| S005 | Suggestion | Concern group candidate |

## Design

The v2 workspace is split by runtime boundary:

```text
crates/
  anneal-lang/     private parser, AST, source spans, loader
  anneal-core/     runtime, facts, store, evaluator, prelude, verbs, trails
  anneal-md/       markdown Source adapter
  anneal-legacy/   transition-only v1 parser/config bridge
  anneal-cli/      CLI surface over core + adapters
  anneal-mcp/      MCP surface over core + adapters
```

`anneal-core` has no markdown-specific code. Adapters implement `Source` and
emit `FactBatch` values. Retrieval providers, trail recording, policy checks,
verb projection, and source orchestration are separate runtime seams so future
adapters can target code, host runtimes, issue trackers, and federated corpora
without changing the Datalog language.

In v2.0, `anneal-mcp` is a crate/library surface. The root CLI does not yet
ship a stable `anneal --mcp` or `anneal mcp` launcher.

## License

MIT
