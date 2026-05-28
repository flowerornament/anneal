# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown, turns it into typed facts, evaluates a
Datalog-shaped rule layer against those facts, and exposes a small
agent-friendly surface for orientation, retrieval, convergence work, and
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

`anneal` gives the agent a programmable corpus surface:

```bash
anneal context "what should I read before changing the release path?"
anneal schema
anneal describe search
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
```

The important move is that corpus structure becomes queryable. Markdown files,
frontmatter, labels, body text, spans, references, snapshots, and project
configuration are all exposed as typed relations. The built-in prelude derives
standard convergence facts and verbs from those relations, and project
`anneal.dl` files can add local rules and callable `@verb` declarations without
changing the binary.

The command names are intentionally mnemonic, but they are not the whole tool.
`context` gathers the first orientation bundle. `schema` maps the language,
and `describe` teaches one relation, primitive, predicate, or verb at a time.
`search`, `read`, and `handle` retrieve evidence. `status` keeps the
convergence frontier visible. When those saved forms are too broad,
`anneal -e` is the normal way to ask the corpus a precise question. When a
precise question becomes reusable, edit `anneal.dl` and add an `@verb`
declaration in the same language.

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

Repo-owned corpus behavior lives in `anneal.dl` and should be committed with
the corpus. Home Manager does not write project rules, source discovery, handle
policy, or convergence config; it only installs the binary/skill and writes
machine-local preferences such as history storage under XDG paths.

## First Moves

Use the narrowest surface that answers the next question. The ladder is:
arrive, discover the language, retrieve evidence, then write a query when the
built-in verbs are too broad.

```bash
# Arrive cold
anneal context "find the most urgent thing blocking v17 conformance" --format=text
anneal status --format=text

# Discover the language before guessing
anneal describe convergence --format=text
anneal schema --format=text

# Read enough to act
anneal search "v17 conformance audit" --limit 3 --format=text
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --budget 4000 --format=text
anneal handle reviews/2026-04-28-formal-model-v17-conformance-audit.md --format=text
anneal handle reviews/2026-04-28-formal-model-v17-conformance-audit.md --impact --format=text

# Ask a precise corpus question
anneal -e '? *handle{id: h, kind: "file", status: s}.'
anneal -e '? diagnostic{subject: h}, area_of{h: h, area: "language"}.'

# Work the convergence frontier
anneal status --format=text
anneal -e '? diagnostic{severity: "error", subject: h}.'
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
history powers `at("snapshot:last")` queries and movement predicates such as
advancing, holding, and drifting. `anneal status` records bounded automatic
snapshots, coalescing unchanged consecutive status reads.

**Prelude**  
The built-in standard library of rules, diagnostics, ranking, and verbs. It is
loaded automatically and should not be copied into a project.

## Command Map

### Arrive

```bash
anneal context "goal"
anneal status
anneal help agent
```

`context` composes ranked search, bounded reads, and graph neighborhood into one
cold-start response. `status` shows the compact convergence frontier.
`help agent` prints the bundled agent skill briefing from the installed binary.
The hidden `prime` alias remains for installed skill loaders and muscle memory.

Useful `context` flags:

- `--hits N`: number of search winners
- `--limit N`: alias for `--hits`, for parity with `search`
- `--budget N`: per-hit read cap; it is not divided by `--hits`
- `--depth N`: graph distance around winners
- `--include-low-confidence`: include lower-confidence search hits

### Program The Corpus

```bash
anneal schema
anneal describe search
anneal describe runtime
anneal -e '? search("conformance", h, span, score, reason, field, low).'
anneal -e '? recent(h, 7), *handle{id: h, summary: summary}.'
anneal -e '? sources(name, recognizes, capabilities, doc).'
```

Use `schema` to see queryable relations and signatures. Use `describe` for one
primitive, predicate, or verb. Use `describe runtime` for the compact map and
copyable examples. Corpus-local vocabulary is queryable directly through
relations such as `*handle{status: status}`, `*edge{kind: kind}`,
`*handle{namespace: ns}`, and `*meta{key: key}`. Adapter information is
queryable through `sources(name, recognizes, capabilities, doc)`. Use `-e`
when you need to compose a question directly. When a working query should
become a named project move, edit `anneal.dl` and add an `@verb` declaration.

### Retrieve Evidence

```bash
anneal search "conformance audit" --limit 5
anneal read formal-model/v17.md --budget 4000
anneal handle formal-model/v17.md
anneal handle formal-model/v17.md --impact
```

`search` ranks content and metadata hits. `read` retrieves bounded content
spans for one handle. `handle` shows incoming and outgoing edges; `--impact`
adds direct and indirect reverse dependencies before an edit.

### Work The Convergence Frontier

```bash
anneal -e '? top_work(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? area_health(area, grade, files, errors, cross_edges).'
anneal -e '? diagnostic{severity: "error"}.'
anneal -e '? blocked_row(h, energy, source), h = "HANDLE".'
anneal -e '? *handle{id: h, file: f}, git_mtime(f, t).'
```

`top_work` ranks active candidates by entropy. `area_health` grades per-area
convergence. `diagnostic{severity: "error"}` filters to blockers. `blocked_row`
explains why one handle is stalled. `recent` and `git_mtime` let agents ask
what changed without a separate `--since` surface. The convergence vocabulary
lives in the prelude — use `describe potential`, `describe entropy`,
`describe blocked`, `describe recent`, or `describe git_mtime` to learn the
joins, then compose with `-e`. The `check` command remains as a hidden CI gate
alias for `diagnostic{severity: "error"}`.

### Raw Queries

```bash
anneal -e '? *handle{id: h, kind: "file"}.'
anneal -e '? *edge{from: src, to: dst, kind: "DependsOn"}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? top_work(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? source_of("top_work", file, lines).'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
anneal -e '? recent(h, 7), search{query: "conformance", handle: h}.'
```

The query language is Datalog-shaped. Stored relations use `*` prefixes, for
example `*handle`, `*edge`, `*meta`, `*content`, `*span`, `*config`,
`*snapshot`, and `*generation`. Derived predicates come from the built-in
prelude, project `anneal.dl`, and inline query-local rules. `anneal -e -`
reads a query from stdin, so agents can keep larger questions in scratch files.
Use `--limit N` while exploring broad predicates.

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
  query: "release_row(h, why, milestone) :=
    verb_arg(\"milestone\", milestone),
    release_blocker(h, why),
    *meta{handle: h, key: \"milestone\", value: milestone}.

    ? release_row(h, why, milestone).",
  doc: "Open blockers for the next release.",
  output_schema: "{\"h\":\"HandleId\",\"why\":\"String\",\"milestone\":\"String\"}",
  args: ["milestone:String"],
  capabilities: ["read"]
).
```

Load order is fixed: config and discovery facts first, source extraction
second, prelude and project rules third, evaluation last. Project predicates
shadow prelude predicates by name and arity. Project verbs are callable by name:

```bash
anneal release-blockers v0.11.0 --format=text
anneal release-blockers --milestone v0.11.0 --explain
```

Reusable project moves are plain `@verb` declarations in `anneal.dl`:

```datalog
@verb(
  name: "broken-area",
  query: "broken_area(h, code, file) :=\n  verb_arg(\"area\", area),\n  diagnostic{subject: h, code: code, severity: \"error\", file: file},\n  area_of{h: h, area: area}.\n\n? broken_area(h, code, file).",
  doc: "Error diagnostics in one area.",
  output_schema: "{\"h\":\"HandleId\",\"code\":\"String\",\"file\":\"String|null\"}",
  args: ["area:String"],
  capabilities: ["read"]
).
```

Then call it like any other verb:

```bash
anneal broken-area language --format=text
```

If a project verb is wrong, edit or remove the `@verb(...)` block in
`anneal.dl`.

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
first write so snapshot-based history queries continue.

Older compatibility commands now return teaching recovery messages instead of
running parallel workflows. Use the language-first ladder above:
`status`/`context` to arrive, `schema`/`describe` to discover,
`search`/`read`/`handle` to retrieve, `handle --impact` for reverse
dependencies, and `anneal -e` for precise composite questions.

Common replacements:

- `find TEXT`: `anneal -e '? *handle{id: h, kind: kind, status: status}, h contains "TEXT".'`
- `get H`: `anneal handle H` or `anneal read H`
- `map`: `anneal -e '? *edge{from: src, to: dst, kind: kind}.'`
- `health`: `anneal status` plus `anneal -e '? diagnostic{severity: severity, subject: h}.'`
- `diff`: `anneal -e '? at("snapshot:last") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`
- `obligations`: `anneal -e '? undischarged(h), obligation(h).'`
- `garden`: `anneal status` plus `anneal -e '? top_work(h, energy), entropy(h, source).'`
- `orient`: `anneal context "GOAL"` or `anneal handle H --impact`
- `impact H`: `anneal handle H --impact`
- `work`: `anneal status` or `anneal -e '? top_work(h, energy), *handle{id: h, file: file, summary: summary}.'`
- `blocked H`: `anneal handle H` or `anneal -e '? blocked_row(h, energy, source), h = "H".'`
- `diagnostics`: `anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'`
- `broken`: `anneal -e '? diagnostic{severity: "error"}.'` or `anneal check`
- `areas`: `anneal -e '? area_health(area, grade, files, errors, cross_edges).'`
- `trend`: `anneal -e '? at("snapshot:last") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`
- `sources`: `anneal -e '? sources(name, recognizes, capabilities, doc).'`
- `query`: `anneal -e`
- `explain`: `anneal -e '...' --explain`

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
