# anneal

Convergence assistant for knowledge corpora.

> [!NOTE]
> **Experimental, pre-1.0, and moving fast.** anneal is in active development:
> the CLI surface, the `anneal.dl` rule layer, diagnostics, and the on-disk
> formats all still change between releases, sometimes without a deprecation
> path. It is built and dogfooded in the open — useful today, but not yet
> stable. Pin a version if you depend on it, expect rough edges, and read the
> [CHANGELOG](CHANGELOG.md) before upgrading.

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
`status` is the arrival dashboard: aggregate corpus vital signs plus
copy-runnable queries for goal-less orientation and work. `context` gathers a
retrieval bundle once you can name a goal. `schema` maps the language, and
`describe` teaches one relation, primitive, predicate, or verb at a time.
`search`, `read`, and `handle` retrieve evidence. When saved forms are too
broad, `anneal -e` is the normal way to ask the corpus a precise question. When
a precise question becomes reusable, edit `anneal.dl` and add an `@verb`
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

For flake inputs that should follow the latest published release instead of
`master`, pin the moving `release` branch:

```nix
anneal.url = "github:flowerornament/anneal?ref=refs/heads/release";
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
anneal status --format=text
anneal -e '? recent_frontier(h, rank, recency), rank <= 12, *handle{id: h, file: file}.' --limit 12 --format=text
anneal -e '? ranked_anchor(h, rank, score, why), rank <= 12, *handle{id: h, file: file}.' --limit 12 --format=text
anneal context "find the most urgent thing blocking v17 conformance" --format=text

# Discover the language before guessing
anneal describe convergence --format=text
anneal schema --format=text

# Read enough to act
anneal search "v17 conformance audit" --limit 3 --format=text
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --budget 4000 --format=text
anneal read reviews/2026-04-28-formal-model-v17-conformance-audit.md --span-id 'reviews/2026-04-28-formal-model-v17-conformance-audit.md#h/summary' --format=text
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
The graph unit of knowledge. A handle may be a file, label, version, or
external reference. Headings are content spans (`*span`) attached to file
handles. In-repo code references such as `lib/app.ex:10-20` use external
handles with `external_class = "code"` metadata.

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
history powers `at("snapshot:last")` queries, `flow(h, direction)`,
`advancing(h)`, `holding(h)`, `drifting(h)`, and the status convergence summary.
`anneal status` records bounded automatic snapshots, coalescing unchanged
consecutive status reads.

**Prelude**  
The built-in standard library of rules, diagnostics, ranking, and verbs. It is
loaded automatically and should not be copied into a project.

## Command Map

### Arrive

```bash
anneal status
anneal -e '? recent_frontier(h, rank, recency), rank <= 12, *handle{id: h, file: file}.' --limit 12
anneal -e '? ranked_anchor(h, rank, score, why), rank <= 12, *handle{id: h, file: file}.' --limit 12
anneal context "goal"
anneal help agent
```

`status` renders aggregate corpus vital signs and prints copy-runnable
orientation/work queries. `recent_frontier` returns recently changed file
handles for goal-less reading; `ranked_anchor` returns the durable spine from
the broader `anchor` relation. `context`
composes ranked heading-span search, compact span metadata, and graph
neighborhood once you have a goal. Add `--read-spans` when you want matched
span bodies inline.
`help agent` prints the bundled agent skill briefing from the installed binary.
The hidden `prime` alias remains for installed skill loaders and muscle memory.

Useful `context` flags:

- `--hits N`: number of search winners
- `--budget N`: per-hit span selection cap; also caps bodies with `--read-spans`
- `--depth N`: graph distance around winners
- `--include-low-confidence`: include lower-confidence search hits
- `--read-spans`: include matched span bodies inline

### Program The Corpus

```bash
anneal schema
anneal describe search
anneal describe runtime
anneal -e '? search("conformance", h, span, score, reason, field, low).'
anneal -e '? changed_within(h, 7), *handle{id: h, kind: "file", summary: summary}.'
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
anneal read formal-model/v17.md --span-id 'formal-model/v17.md#h/protocol'
anneal handle formal-model/v17.md
anneal handle formal-model/v17.md --impact
```

`search` ranks content and metadata hits and includes `heading_path` for
heading-span matches. Scores combine lexical strength with status and hub
boosts, so authoritative/highly-cited sections win ties against draft mentions.
`read` retrieves bounded content spans for one handle; use `--span-id` when a
search hit already identified the section you need.
`handle` shows incoming and outgoing edges grouped by kind and separates
in-repo code references; `--impact` adds direct and indirect reverse
dependencies before an edit.

Use `anneal context "X"` when the task is "find the section that defines X";
it returns ranked section hits, span metadata, `heading_path`, and graph
neighborhood. Add `--read-spans` only when inline matched bodies are worth the
extra output. Use `grep -rn "X"` when you need every literal occurrence with
line numbers. Use `anneal -e '? ...'` when the question is structural, such as
"which handles match this graph predicate?"

### Work The Convergence Frontier

```bash
anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? area_health(area, grade, files, errors, cross_edges).'
anneal -e '? diagnostic{code: code, severity: "error", subject: h, file: file, line: line}.'
anneal -e '? blocker(h, energy, source), h = "HANDLE".'
anneal -e '? *handle{id: h, file: f}, git_mtime(f, t).'
```

`potential` exposes raw unsettled-work energy; `frontier` projects the
highest-energy candidates. `area_health` grades per-area convergence.
`diagnostic{severity: "error", ...}` filters to blockers. `blocker` explains why one
handle is stalled. `flow` classifies active movement as advancing, holding, or
drifting; settled handles are outside flow by design. `changed_within` and
`git_mtime` let agents ask what changed without a separate `--since` surface.
The convergence vocabulary lives in the prelude — use `describe convergence`,
`describe potential`, `describe entropy`, `describe blocker`,
`describe changed_within`, or `describe git_mtime` to learn the
joins, then compose with `-e`. The `check` command remains as a hidden CI gate
alias for `diagnostic{code: code, severity: "error", subject: h, file: file, line: line}`.

### Raw Queries

```bash
anneal -e '? *handle{id: h, kind: "file"}.'
anneal -e '? *edge{from: src, to: dst, kind: "DependsOn"}.'
anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'
anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'
anneal -e '? source_of("frontier", file, lines).'
anneal -e '? search("conformance", h, span, score, reason, field, low).' --limit 20
anneal -e '? changed_within(h, 7), *handle{id: h, kind: "file"}, search{query: "conformance", handle: h}.'
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
  asserts_code(["draft", "review", "approved"]).
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

config code_path_root {
  root(["web", "bin"]).
}

config search_boost {
  status("authoritative", 0.08).
  status("draft", 0).
  hub(0.01).
}

config potential_weight {
  freshness_decay(0).
  undischarged(8).
}

config state {
  history_mode("xdg").  # xdg | repo | off
}
```

Label namespaces are inferred automatically. Use `force(["REQ"])` only for a
sparse namespace that should be recognized before it has enough examples. Use
`rejected([...])` for false positives and `linear([...])` for obligation
namespaces whose labels must be discharged exactly once.

`search_boost` adjusts ranking calibration without changing what search can
match. Use exact status names for lifecycle boosts and `hub(...)` for the
bounded per-incoming-edge boost; `anneal describe search_boost` shows the
queryable config rows.

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
  name: "area-diagnostics",
  query: "area_diagnostic(h, code, file) :=\n  verb_arg(\"area\", area),\n  diagnostic{subject: h, code: code, file: file},\n  area_of{h: h, area: area}.\n\n? area_diagnostic(h, code, file).",
  doc: "Diagnostics in one area.",
  output_schema: "{\"h\":\"HandleId\",\"code\":\"String\",\"file\":\"String|null\"}",
  args: ["area:String"],
  capabilities: ["read"]
).
```

After that declaration is present in `anneal.dl`, call it like any other verb:

```bash
anneal --root docs area-diagnostics '(root)' --format=text
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
running the retired commands. Use the language-first ladder above:
`status` plus its `recent_frontier`/`ranked_anchor` queries to arrive,
`context GOAL` when the goal is known, `schema`/`describe` to discover,
`search`/`read`/`handle` to retrieve, `handle --impact` for reverse
dependencies, and `anneal -e` for precise composite questions.

Common replacements:

- `find TEXT`: `anneal -e '? *handle{id: h, kind: kind, status: status}, h contains "TEXT".'`
- `get H`: `anneal handle H` or `anneal read H`
- `map`: `anneal -e '? *edge{from: src, to: dst, kind: kind}.'`
- `health`: `anneal status` plus `anneal -e '? diagnostic{code: code, severity: severity, subject: h, file: file, line: line}.'`
- `diff`: `anneal -e '? at("snapshot:last") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.'`
- `obligations`: `anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'`
- `garden`: `anneal status` plus `anneal -e '? frontier(h, energy), entropy(h, source).'`
- `orient`: `anneal status`, then the printed `recent_frontier`/`ranked_anchor` queries; use `anneal context "GOAL"` once you have a goal
- `impact H`: `anneal handle H --impact`
- `work`: `anneal status` or `anneal -e '? frontier(h, energy), *handle{id: h, file: file, summary: summary}.'`
- `blocked H`: `anneal handle H` or `anneal -e '? blocker(h, energy, source), h = "H".'`
- `diagnostics`: `anneal -e '? diagnostic(code, severity, subject, file, line, evidence).'`
- `broken`: `anneal -e '? diagnostic{code: code, severity: "error", subject: h, file: file, line: line}.'` or `anneal check`
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
- `*meta`: frontmatter and extracted metadata, including `external_class`,
  `target_path`, `target_start_line`, `target_end_line`, `target_exists`, and
  `target_history_status` for code external handles
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
| W005 | Warning | Lifecycle config gap |
| W006 | Warning | Code-authoritative spec cites missing code |
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
