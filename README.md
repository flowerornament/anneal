# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown, turns it into a typed knowledge graph,
evaluates a Datalog-shaped rule layer against those facts, and exposes a small
set of agent-friendly commands for orientation, retrieval, health, and
extension.

It is built for disconnected intelligences: agents across sessions with no
shared memory that need to recover what matters, read enough context, and leave
the corpus more settled than they found it.

The larger idea is convergence. In a large corpus, the useful signal is not just
what can be retrieved. It is what is becoming more precise, which obligations
are getting discharged, which decisions have replaced open questions, and which
paths are falling out of the active frontier. `anneal` keeps that signal
visible.

## Why

Large agent-readable corpora behave like living systems. Files cite each other,
supersede each other, encode obligations, accumulate local vocabulary, and move
through stages of settledness. Without tooling, every new agent burns context
reconstructing the same map.

`anneal` gives the agent a programmable retrieval surface:

```bash
anneal context "what should I read before changing the release path?"
anneal search "conformance audit" --limit 5
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --budget 4000
anneal schema
anneal vocab
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
```

The important move is that corpus structure becomes queryable. Markdown files,
frontmatter, labels, body text, spans, references, snapshots, and project
configuration are all exposed as typed relations. The built-in prelude derives
standard convergence facts and verbs from those relations, and project
`anneal.dl` files can add local rules or `@verb` declarations without changing
the binary.

The command names are still intentionally mnemonic. `context` gathers the first
orientation bundle. `search` and `read` make retrieval explicit. `schema`,
`vocab`, `verbs`, and `describe` let agents inspect the runtime before guessing.
`status`, `garden`, and `trend` keep the convergence frontier visible once the
agent is ready to act.

## Install

### Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash
```

The installer is the primary path for prebuilt binaries. It installs to
`~/.local/bin` by default, prints the target, URL, and destination before it
writes anything, fails fast on unsupported targets, and stays aligned with the
published release matrix.

Binaries are published for:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

By default the installer writes only the binary. To also install the bundled
agent skill, pass one or more explicit skill targets:

```bash
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- \
  --skill-target "$HOME/.agents/skills/anneal" \
  --skill-target "$HOME/.codex/skills/anneal"
```

Other useful installer forms:

```bash
# Install somewhere else
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | INSTALL_DIR="$HOME/bin" bash

# Same override, passed as a flag
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --install-dir "$HOME/bin"

# Preview without writing anything
curl -fsSL https://raw.githubusercontent.com/flowerornament/anneal/master/install.sh | bash -s -- --dry-run
```

### From Source

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
binary, writes machine-local user config under XDG paths, and can optionally
install the bundled skill into agent-managed skill directories.

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

Repo-owned corpus behavior lives in `anneal.dl`. User preferences and derived
history live under XDG paths.

## First Moves

Use the narrowest command that answers the next question.

```bash
# Arrive cold
anneal context "find the most urgent thing blocking v17 conformance" --format=text
anneal status --format=text

# Read enough to act
anneal search "v17 conformance audit" --limit 3 --format=text
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --budget 4000 --format=text
anneal handle reviews/2026-04-28-formal-model-v17-conformance-audit.md --format=text

# Work the convergence frontier
anneal garden
anneal work --format=text
anneal broken --format=text
anneal trend --format=text

# Inspect the runtime before writing a query
anneal describe convergence --format=text
anneal sources --format=text
anneal verbs --format=text
anneal schema --format=text
anneal vocab --format=text

# Drop to raw Datalog when the built-in verbs are too coarse
anneal -e '? *handle{id: h, kind: "file", status: s}.'
```

Runtime commands render readable text at a terminal and JSON/NDJSON when piped.
Use `--format=text` in pipe-only harnesses when an agent needs prose-like
output. Use `--json` or `--format=json` when you want stable machine output.

## Core Concepts

**Handle**  
The unit of knowledge. A handle may be a file, section, label, version, or
external URL.

**Edge**  
A typed relationship between handles. Common edges include `Cites`,
`DependsOn`, `Supersedes`, `Verifies`, and `Discharges`.

**Status**  
The lifecycle state from frontmatter. Project config partitions statuses into
active and terminal sets.

**Convergence Lattice**  
The model that tracks movement from active uncertainty toward terminal
settledness. If an ordering is configured, `anneal status` can show pipeline
shape: raw ideas becoming drafts, drafts becoming reviewed artifacts, reviewed
artifacts becoming authoritative.

**Snapshot**  
A point-in-time capture of graph state, stored in local anneal history. Snapshot
history powers `trend`, `diff`, and movement predicates such as advancing,
holding, and drifting.

**Prelude**  
The built-in standard library of rules, diagnostics, ranking, and verbs. It is
loaded automatically and should not be copied into a project.

## Command Map

### Orientation

```bash
anneal context "goal"
anneal status
anneal prime
```

`context` composes ranked search, bounded reads, and graph neighborhood into one
cold-start response. `status` shows the compact convergence frontier. `prime`
prints the bundled agent skill briefing from the installed binary.

Useful `context` flags:

- `--hits N`: number of search winners
- `--limit N`: alias for `--hits`, for parity with `search`
- `--budget N`: per-hit read cap; it is not divided by `--hits`
- `--depth N`: graph distance around winners
- `--include-low-confidence`: include lower-confidence search hits

### Retrieval And Inspection

```bash
anneal search "conformance audit" --limit 5
anneal read formal-model/v17.md --budget 4000
anneal handle formal-model/v17.md
anneal find ADR --limit 25
anneal get REQ-12 --context
```

`search` ranks content and metadata hits. `read` retrieves bounded content spans
for one handle. `handle` shows incoming and outgoing edges; `anneal H HANDLE` is
available as a short alias. `find` and `get` remain useful for identity-oriented
handle lookup.

### Convergence Work

```bash
anneal garden
anneal work
anneal broken
anneal blocked HANDLE
anneal trend
anneal diff --days=7
```

`garden` is the maintenance view: ranked tasks with hints for how to inspect
and verify them. `work` ranks active candidates. `broken` shows diagnostic
blockers. `blocked` explains one handle. `trend` and `diff` show movement
between snapshots.

### Edit Loop

```bash
anneal orient --file=design.md
anneal impact design.md
anneal check --scope=active
anneal obligations
```

Use `orient` before editing to find upstream context. Use `impact` after editing
to see downstream review targets. Use `check` and `obligations` to keep
references, lifecycle constraints, and linear namespaces honest.

### Introspection

```bash
anneal describe convergence
anneal sources
anneal verbs
anneal schema
anneal vocab
```

Use introspection before inventing a query. Agents should be able to discover
which sources are linked, which verbs exist, what a predicate means, which
capabilities a command needs, and which vocabulary the corpus already uses.

### Raw Queries

```bash
anneal -e '? *handle{id: h, kind: "file"}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
```

The query language is Datalog-shaped. Stored relations use `*` prefixes, for
example `*handle`, `*edge`, `*meta`, `*content`, `*span`, `*config`,
`*snapshot`, and `*generation`. Derived predicates come from the built-in
prelude, project `anneal.dl`, and inline query-local rules.

## Configuration Ladder

New corpora work without configuration. `anneal` loads the built-in prelude,
scans markdown with default adapter settings, infers handle namespaces from
corpus evidence, and runs the standard commands immediately.

Configuration has three layers:

- **Built-in prelude:** standard rules, diagnostics, ranking, and verbs. It is
  automatic.
- **`anneal.dl`:** repo-local corpus semantics: adapter discovery, convergence
  statuses, terminal/active sets, pipeline ordering, linear namespaces,
  frontmatter edge mappings, excludes, project rules, and `@verb` declarations.
- **User config:** machine-local preferences at
  `$XDG_CONFIG_HOME/anneal/config.toml` or `~/.config/anneal/config.toml`.

Repo config should describe the corpus. User config should describe the
machine.

`anneal init --dry-run` previews a scaffolded `anneal.dl`. `anneal init` writes
only when no repo config exists.

### Example `anneal.dl`

```dl
source md {
  file_extension(".md").
  scan_root(".").
  scan_exclude(["vendor", "**/README.md"]).
}

config convergence {
  ordering(["raw", "draft", "review", "approved", "published"]).
  active(["draft", "review", "approved"]).
  terminal(["published", "archived", "superseded"]).
  description("draft", "Under construction; may change substantially").
  description("approved", "Settled primary artifact; changes require review").
  description("archived", "Superseded or retired; no further changes expected").
}

config handles {
  rejected(["SHA", "GPT"]).
  linear(["REQ"]).
}

config frontmatter {
  field("depends-on", "DependsOn", "forward").
  field("superseded-by", "Supersedes", "forward").
}

config impact {
  traverse(["DependsOn", "Supersedes", "Verifies"]).
}

config state {
  history_mode("xdg").  # xdg | repo | off
}
```

Label namespaces are inferred automatically. Use `force(["REQ"])` only for a
sparse namespace that should be recognized before it has enough examples. Use
`rejected([...])` for false positives and `linear([...])` for obligation
namespaces whose labels must be discharged exactly once.

### Project Rules And Verbs

`anneal.dl` can also declare rules and project verbs.

```dl
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
second, prelude and project rules third, evaluation last. Project predicates
shadow prelude predicates by name and arity.

### Upgrading From Pre-0.11.0

`anneal.toml` has been replaced by `anneal.dl`.

If a corpus still has `anneal.toml`, run:

```bash
anneal init --dry-run
anneal init --force
```

The force path writes unified `anneal.dl` from the currently loaded config and
moves `anneal.toml` to `anneal.toml.legacy`.

Pre-0.11.0 `[handles].confirmed` inventories are not carried forward.
Namespaces are inferred automatically; add `force([...])` manually only for
sparse prefixes that need an explicit policy override. If an existing
`anneal.dl` still contains `confirmed(...)`, `anneal init --dry-run` previews a
cleaned file and `anneal init --force` rewrites it without the obsolete
inventory.

Snapshot history is the automatic migration path. When XDG history is enabled,
pre-0.11.0 repo-local `.anneal/history.jsonl` is copied into XDG state on the
first write so trend and diff history continue.

## Stored Relations

Adapters emit stored relations. Rules and queries consume them.

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

Every stored row carries enough identity to distinguish corpus, source, origin,
revision, and generation.

## Diagnostics

The diagnostic catalog tracks local consistency and convergence health.

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
| S002 | Suggestion | Reserved; namespaces are inferred automatically |
| S003 | Suggestion | Pipeline stall |
| S004 | Suggestion | Abandoned namespace |
| S005 | Suggestion | Concern group candidate |

## Design

The workspace is split by runtime boundary:

```text
crates/
  anneal-lang/     private parser, AST, source spans, loader (publish = false)
  anneal-core/     runtime, facts, store, evaluator, prelude, verbs, trails
  anneal-md/       markdown Source adapter
  anneal-legacy/   transition bridge for older parser/config behavior
  anneal-cli/      CLI surface over core + adapters
  anneal-mcp/      MCP library surface over core + adapters
```

`anneal-core` has no markdown-specific code. Adapters implement `Source` and
emit `FactBatch` values. Retrieval providers, trail recording, policy checks,
verb projection, and source orchestration are separate runtime seams so future
adapters can target code, host runtimes, issue trackers, and federated corpora
without changing the query language.

`anneal-lang` is a private crate (`publish = false`) until a second consumer
proves the syntax boundary. `anneal-mcp` ships as a crate/library surface, not
an installed root-CLI launcher. From a source checkout,
`cargo run -p anneal-mcp -- --tools` prints the crate-level MCP tool catalog.

## License

MIT
