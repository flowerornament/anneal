---
status: draft
date: 2026-04-15
depends-on: anneal-spec.md
---

# Areas, Orient, and Garden

## Motivation

Anneal sees the corpus at two scales: the whole thing (`status`, `check`, `diff`) or individual handles (`get`, `query`, `explain`). There's no middle layer. But the natural unit of work — for both agents and humans — is an *area*: the compiler docs, the formal model, the research log.

This gap matters for two reasons:

1. **Agent orientation.** An agent starting work on the compiler needs to know what to read, in what order, within a token budget. Today it gets either a 12,000-handle dump or a single-file view. Neither helps it build a working mental model of the area it's about to modify.

2. **Corpus gardening.** A human doing maintenance needs to know what's degrading and where to focus effort. Today `check` gives a flat diagnostic list with no prioritization. "9 orphaned labels in compiler/" is more actionable than "S001: OD-1 is orphaned."

Both use cases need the same primitive: areas as a first-class grouping dimension.

### Background

This design is informed by OpenAI's "harness engineering" report on building software with agent teams. Key observations:

- Agents need a *map*, not a manual. Progressive disclosure beats large context dumps.
- Repository knowledge must be the system of record. What agents can't see doesn't exist.
- Entropy accumulates. Quality grades per domain, tracked over time, catch drift before it spreads.
- Human taste is encoded once, then enforced continuously.

Anneal already embodies several of these principles (the convergence model, diagnostics, the skill file). This spec extends them with area-scoped health tracking, agent-oriented context budgeting, and prioritized maintenance surfacing.

## Design

### Areas

An area is a directory-level grouping of files in the corpus. Areas are auto-detected from the top-level directory structure of the corpus root. No configuration is required.

For a corpus rooted at `.design/` with this structure:

```
.design/
├── compiler/
├── formal-model/
├── implementation/
├── language/
├── synthesis/
├── research-log/
├── OPEN-QUESTIONS.md
└── anneal.toml
```

The areas are: `compiler`, `formal-model`, `implementation`, `language`, `synthesis`, `research-log`, and `(root)` for top-level files.

**Concern group override.** When `[concerns]` is configured in `anneal.toml`, concern groups can act as areas. A concern group may span multiple directories. When concern groups are defined, `anneal areas` shows both directory-based and concern-based views. The `--area` flag accepts either a directory name or a concern group name.

**Area health.** Each area has a computed health profile:

| Signal | Source | Description |
|--------|--------|-------------|
| Grade | Composite | A/B/C/D letter grade from weighted signals |
| Files | Handle count | Number of files in the area |
| Handles | Handle count | Total handles (files + sections + labels + versions) |
| Connectivity | Edge count / handle count | Average edges per handle |
| Cross-links | Cross-area edge count | Edges reaching other areas (0 = island) |
| Diagnostics | Check output | Error/warning/suggestion counts scoped to area |
| Orphans | S001 count | Labels defined but never referenced |
| Trend | Snapshot history | Grade direction since last snapshot (↑→↓) |

**Grading heuristic.** The grade is a composite of error count, connectivity, and metadata coverage:

- **A**: No errors, connectivity ≥ 0.3, has active-status files
- **B**: No errors, but low connectivity, no active metadata, or elevated orphan count
- **C**: Has errors (E001/E002)
- **D**: Has errors and low connectivity (structural decay)

With snapshot history, grades gain trend arrows. A grade that was B last week and is C now shows `[C↓]`.

### `anneal areas`

List areas with health profiles.

```
$ anneal areas

Area               Files  Conn  Cross  Grade  Signal
synthesis/            34   1.1    316    [A]   healthy
language/             31   1.2     19    [A]   4 orphans
implementation/       73   0.6    482    [C]   2 broken refs
compiler/             28   0.4     31    [B]   9 orphans
formal-model/         53   0.1     45    [B]   sparse, 7 orphans
archive/              17   0.2      0    [B]   island
```

Flags:

| Flag | Effect |
|------|--------|
| `--json` | Structured output with full health profiles |
| `--include-terminal` | Include areas that contain only terminal files |
| `--sort=grade\|files\|conn\|name` | Sort order (default: files descending) |

### `--area` flag on existing commands

The `--area` flag scopes existing commands to a single area:

```
anneal status --area=compiler        # health summary for one area
anneal check --area=compiler         # diagnostics scoped to area
anneal map --area=compiler           # graph scoped to area + cross-edges
anneal query handles --area=compiler # handles in area
anneal query edges --area=compiler   # edges with source or target in area
anneal diff --area=compiler          # convergence trend for area
anneal impact --area=compiler <file> # impact scoped to area
```

Scoping semantics: a handle belongs to an area if its `file` field has that area as its first path component. For `--area=(root)`, handles belong if their file has no path separator. For concern group areas, handles belong if they match any pattern in the concern group.

### `anneal orient`

Generate a context-budgeted reading list for agents. This is the "give the agent a map" command.

```
$ anneal orient --area=compiler --budget=30k

compiler/ [B] — 28 files, 749 handles, conn=0.4

Read first (pinned):
  OPEN-QUESTIONS.md                                              [26k]

Read next (area entry points, ranked by centrality × recency):
  compiler/elaboration-study/2026-03-29-architecture-spike-findings.md  [5k]
  compiler/2026-04-09-graph-structural-coalescing.md                    [4k]

Budget: 35k / 30k — dropping tier 2 (upstream context)

Active issues:
  S001: 9 orphaned labels (OD-1..OD-7, SR-18, SR-19)
```

**Ranking formula.** Files are scored by:

```
score = (incoming + outgoing) * edge_weight
      + label_count * label_weight
      + recency_bonus * recency_weight
      + (2.0 if active_status else 0.3 if terminal else 0.5)
```

Where `recency_bonus` is days-since-epoch of the file's date (see Temporal Awareness below), normalized so the most recent file in the area scores 1.0 and the oldest scores 0.0.

**Tiering.** Files are presented in tiers:

1. **Pinned** — files listed in `[orient] pin`. Always included, always first.
2. **Area entry points** — files in the target area, ranked by score.
3. **Upstream context** — files outside the area that the area references (outgoing edges).
4. **Downstream consumers** — files outside the area that reference the area (incoming edges).

Tiers are filled in order until the token budget is exhausted. A tier that would exceed the budget is dropped with a note.

**Token estimation.** File size in bytes divided by 4. This is a rough estimate. Precision is not important — the budget is a soft cap, not a hard limit.

**Flags:**

| Flag | Effect |
|------|--------|
| `--area=X` | Scope to area (default: whole corpus) |
| `--budget=Nk` | Token budget, e.g. `50k`, `100k` (default from config or 50k) |
| `--paths-only` | Emit bare file paths, one per line (for agent tool piping) |
| `--json` | Structured output with scores, tiers, and budget math |
| `--file=X` | Scope to neighborhood of a specific file instead of area |

**`--file` variant.** Instead of scoping to an area, scope to a single file's upstream dependency tree. "I'm about to edit this file — what context do I need?" This follows outgoing edges from the file (DependsOn, Supersedes, frontmatter references) up to `depth` hops, collecting the files that feed into the target. The result is a reading list ranked by the same scoring formula, constrained by the token budget.

This is the upstream complement to `impact` (which traverses downstream: "what breaks if I change this?"). Both use the same directed traversal infrastructure. `map --around=X --upstream` renders the same walk as a tree visualization rather than a reading list.

**Configuration:**

```toml
[orient]
# Ranking weights (defaults shown)
edge_weight = 1.0
label_weight = 1.0
recency_weight = 0.5

# Default token budget when --budget is omitted
budget = "50k"

# Traversal depth for upstream/downstream tiers
depth = 3

# Files always included first (pinned context)
pin = [
  "OPEN-QUESTIONS.md",
  "LABELS.md",
]

# Files never included (noise for agents)
exclude = [
  "CHANGELOG.md",
]
```

The `pin` list is the most important configuration. It encodes human judgment about which files are essential context. Everything else is computed from graph structure.

The `exclude` list uses the same glob pattern syntax as the top-level `exclude` (plain names and glob patterns).

### `anneal garden`

Surface maintenance tasks ranked by blast radius.

```
$ anneal garden

 1. [fix]   2 broken refs in implementation/             blast=high
             specimens/cpu-fast-path/...family.toml not found
 2. [tidy]  9 orphaned labels in compiler/                blast=med
             OD-1..OD-7 defined but never referenced
 3. [tidy]  7 orphaned labels in formal-model/            blast=med
             labels defined but never referenced
 4. [link]  archive/ is an island (17 files, 0 cross)     blast=low
             nothing references this area
 5. [stale] formal-model/papers/2026-02-15-*.md           blast=low
             7 oldest files in corpus, no edges to recent work
 6. [meta]  papers/ has no active-status files (24 files)  blast=low
             add status: frontmatter to key files
```

**Task categories:**

| Category | Source | Description |
|----------|--------|-------------|
| `fix` | E001, E002 | Broken references and undischarged obligations |
| `tidy` | S001 | Orphaned labels, grouped by area |
| `link` | Cross-edge analysis | Areas with zero cross-links (islands) |
| `stale` | Temporal analysis | Old files with no edges to recent work |
| `meta` | W003 / status analysis | Areas missing frontmatter or status metadata |
| `drift` | Namespace dispersion | Namespaces leaking across area boundaries |

**Ranking.** Tasks are ranked by blast radius:

1. Errors always rank highest (they block correctness).
2. Orphans are ranked by count (more orphans = more concept drift).
3. Islands are ranked by file count (larger islands = more hidden knowledge).
4. Staleness is ranked by age × handle count (old large files = most forgotten context).
5. Metadata gaps are ranked by file count.

**Flags:**

| Flag | Effect |
|------|--------|
| `--area=X` | Scope to area |
| `--json` | Structured output with task details and blast radius scores |
| `--limit=N` | Show top N tasks (default: 10) |
| `--category=fix\|tidy\|link\|stale\|meta\|drift` | Filter to one category |

**Agent consumption.** `garden --json` output is designed for agent consumption. Each task includes enough context for an agent to act on it directly: the specific files involved, the diagnostic codes, and a one-line remediation hint.

For obligation tasks (`fix` category, E002), the remediation hint includes the frontmatter syntax needed to discharge: `add discharges: [COMP-OQ-1] to resolving document frontmatter`. This addresses a common confusion where agents resolve an obligation substantively but don't know how to wire the discharge edge.

### `--since` flag on existing commands

Temporal scoping, parallel to `--area` for spatial scoping. `--recent` is the shorthand for the common case. `--since=Nd` is the precise variant.

`--recent` filters to files within a default window (7 days, configurable via `[temporal] recent_days` in `anneal.toml`). `--since=Nd` overrides with an explicit window.

```
anneal check --recent                    # issues from the last 7 days
anneal check --area=compiler --recent    # issues in compiler this week
anneal areas --recent                    # only areas with recent activity
anneal garden --recent                   # new gardening tasks, not pre-existing
anneal find "OQ" --recent               # search only recent files
anneal query handles --since=14d         # explicit: last 14 days
anneal query edges --since=30d           # explicit: last 30 days
```

**Configuration:**

```toml
[temporal]
recent_days = 7  # default window for --recent (days)
```

Semantics: `--recent` and `--since=Nd` filter to file handles whose computed date (see Temporal Awareness) is within the window. Handles without a date are excluded. Section, label, and version handles inherit their parent file's date.

The `--sort=date` flag on `query` and `find` sorts results by file date (most recent first) without filtering. This replaces the need for a standalone `recent` command:

```
anneal find --sort=date --area=compiler --limit=10   # recent files in compiler
anneal query handles --sort=date --limit=20          # recent files corpus-wide
```

**`find` query becomes optional.** When any filter is present (`--status`, `--kind`, `--namespace`, `--area`, `--recent`, `--since`), the positional query argument defaults to empty. This makes `anneal find --status=active --kind=file` work without the awkward `""` placeholder — a common agent query that was previously hard to discover.

### `map --by-area`

Area-level topology graph. Nodes are areas, edges are cross-area connection counts. Gives the 30-second "what's the shape of this corpus?" view.

```
$ anneal map --by-area

(root) ──601──> runtime
(root) ──381──> synthesis
(root) ──309──> language
implementation ──123──> synthesis
implementation ──45──> research-log
synthesis ──54──> runtime

Islands: archive, reviews (0 cross-links)
```

This complements `map` (handle-level, too granular for orientation) and `areas` (tabular, no topology). The `--by-area` flag renders the same graph data that `areas` computes, but as a directed edge list or DOT graph.

Flags:

| Flag | Effect |
|------|--------|
| `--by-area` | Area-level topology instead of handle-level graph |
| `--dot` | DOT format output (composes with `--by-area`) |
| `--min-edges=N` | Only show cross-area connections with at least N edges (default: 1) |

### `map --around` directed traversal

`map --around=X` currently does undirected BFS — it shows the neighborhood. Two direction flags add directed tree traversal:

```
anneal map --around=impl-plan.md --upstream --depth=3    # what feeds into this file
anneal map --around=impl-plan.md --downstream --depth=3  # what depends on this file
anneal map --around=OQ-64 --upstream                     # upstream of a label
```

`--upstream` follows outgoing edges from the root: DependsOn targets, frontmatter references, Supersedes targets. This answers "what does this document build on?" — the dependency ancestry tree.

`--downstream` follows incoming edges to the root: handles that DependsOn, reference, or Supersede the root. This is the same traversal as `impact` but rendered as a tree rather than a flat direct/indirect list.

Without either flag, `--around` retains its current undirected BFS behavior. `--upstream` and `--downstream` are mutually exclusive. Both compose with `--depth`, `--render=dot`, and `--area` (which limits the tree to area-local handles plus boundary nodes).

The directed traversal is shared infrastructure: `orient --file=X` uses the same upstream walk to build a reading list, `impact` uses the same downstream walk for blast radius. `map` renders the walk as a tree; `orient` renders it as a budget-constrained file list.

### `diff --by-area`

Per-area convergence deltas. Shows which areas are improving, holding, or degrading since the last snapshot. This is the trend signal — not "what's wrong now" but "what's getting worse."

```
$ anneal diff --by-area

Area               Grade  Δ Errors  Δ Orphans  Δ Conn  Trend
compiler/          [B]         0        +3      -0.1   degrading
synthesis/         [A]         0         0      +0.2   improving
formal-model/      [B→C]      +2        0       0.0   new errors
implementation/    [C]         0         0       0.0   holding
```

Requires snapshot history. When no history is available, falls back to current-state-only view (equivalent to `areas`).

### Temporal Awareness

Anneal gains the ability to associate a date with each file handle. This date is used by `orient` (recency ranking), `garden` (staleness detection), `--since` (temporal filtering), and `--sort=date` (chronological ordering).

**Date extraction order** (first match wins):

1. `updated:` frontmatter field — explicit, authoritative when present.
2. `date:` frontmatter field — common in dated documents.
3. Filename date — extract `YYYY-MM-DD` prefix from the filename. E.g., `2026-03-29-architecture-spike-findings.md` → `2026-03-29`.
4. No date — file is undated. Excluded by `--since`, sorts last with `--sort=date`, gets zero recency bonus in `orient`.

Git dates are not used by default. They reflect last commit time, which may not correspond to content relevance (e.g., a bulk formatting commit touches every file).

**Storage.** File dates are computed during `build_graph` and stored on the `Handle` struct. No new persistent state is needed — dates are derived fresh on each run from frontmatter and filenames.

### Context enrichment

`--context` exists on `get` today: it produces a compact agent briefing with a body snippet. Two extensions:

**Frontmatter summary preference.** `get --context` currently shows the first body line as the snippet. Many corpora use `purpose:`, `note:`, or `summary:` frontmatter fields that are more useful for orientation. `--context` should prefer frontmatter summary fields over body text:

1. `purpose:` frontmatter field
2. `note:` frontmatter field
3. First non-empty body line (current behavior)

This requires storing additional frontmatter scalar fields on `HandleMetadata`. The parser already extracts all frontmatter — it currently discards fields that don't produce edges. Retaining `purpose` and `note` is a small extension.

**`--context` on `find` and `query handles`.** When present, adds a summary column to the output table using the same preference chain. This makes the common agent query "what's active and why?" a single command:

```
anneal find --status=active --kind=file --context
```

```
Handle                    Status   Purpose
────────────────────────────────────────────────────────────────
arch-synthesis.md         stable   Governing architecture for Herald
impl-plan.md              active   Phase-ordered implementation roadmap
compression-spec.md       active   Compression pipeline specification
```

`--context` on list commands is an enrichment dimension that composes with all other flags: `--area`, `--recent`, `--sort=date`.

### Obligation lifecycle guidance

`explain obligation` shows disposition and facts but doesn't tell agents *how to fix* outstanding obligations. Two additions:

**Remediation hint.** When disposition is `outstanding`, include actionable guidance:

```
$ anneal explain obligation COMP-OQ-1

obligation   COMP-OQ-1
namespace    COMP-OQ
status       outstanding

facts
  disposition   state      outstanding
  location      file       compression-spec.md

remediation
  To discharge, add to the resolving document's frontmatter:
    discharges: [COMP-OQ-1]
```

**Candidate dischargers.** Suggest files that are likely candidates for discharging the obligation, based on graph proximity:

- Files that cite handles in the same namespace
- Files with edges to the obligation's defining file
- Files in the same area that were recently modified

```
candidates (by graph proximity)
  compiler/connectors-refactor.md     cites COMP-OQ-2, COMP-OQ-3
  implementation/phase-b-plan.md      depends on compression-spec.md
```

Candidates are a suggestion, not a guarantee. When no candidates are found, say so — the discharge may need a new document.

### Pipeline semantics

`explain convergence` shows the convergence signal but doesn't expose the pipeline configuration. Agents encountering unfamiliar status values (e.g., `stable`, `incorporated`, `decision`) can't tell whether a document is still governing work or has settled.

**Pipeline display.** `explain convergence` shows the configured active/terminal partition and ordering:

```
pipeline
  active:    draft, active, stable
  terminal:  archived, superseded, incorporated
  ordering:  draft → active → stable
```

**Optional status descriptions.** A `[convergence.descriptions]` config table lets corpus authors document what each status means operationally:

```toml
[convergence.descriptions]
stable = "Content settled, still governs active work"
incorporated = "Absorbed into another document, no longer standalone"
decision = "Records a binding decision, reference-only"
```

When present, `explain convergence` includes descriptions alongside the pipeline display. When absent, only the structural classification is shown. Zero configuration cost for corpora that don't need it.

### Batch handle lookup

`get` accepts a single handle. When an agent needs status for 5 files, it calls `get` five times. Accept multiple positional arguments:

```
anneal get arch-synthesis.md impl-plan.md compression-spec.md --status-only
```

```
arch-synthesis.md      stable
impl-plan.md           active
compression-spec.md    active
```

When multiple handles are given, output defaults to a compact one-line-per-handle format. `--status-only` further reduces to just identity and status. `--context` shows the summary field. `--json` emits an array.

Single-handle `get` retains its current detailed output.

## Composability

The design introduces two scoping dimensions, an enrichment dimension, and two new commands. Everything composes:

| Dimension | Flag | Effect |
|-----------|------|--------|
| Spatial | `--area=X` | Scope to one area (directory or concern group) |
| Temporal | `--recent` | Scope to files within default window (7 days) |
| Temporal | `--since=Nd` | Scope to files dated within last N days (explicit) |

All three flags are additive on existing commands. They compose with each other and with all existing flags. `anneal check --area=compiler --recent --json` works as expected. `--since` overrides `--recent` when both are present.

| Sort | Flag | Effect |
|------|------|--------|
| Chronological | `--sort=date` | Sort results by file date, most recent first |
| Enrichment | `--context` | Add frontmatter summary to list output (find, query handles) |

This replaces the need for a standalone `recent` command. Chronological views are a sort mode, not a separate surface. Context enrichment is an output mode, not a filter — `anneal find --status=active --kind=file --area=compiler --context` composes all four dimensions.

### Interaction with existing commands

`anneal areas` fills the gap between `status` (corpus-wide) and `get` (single handle). It replaces no existing command.

`anneal orient` replaces no existing command. The anneal skill currently recommends `status --json --compact` for orientation — orient gives agents a better starting point. The skill should be updated to recommend `orient --area=X --budget=Nk` when an agent is scoped to a specific area of work.

`anneal garden` complements `check`. Where `check` is a correctness gate (pass/fail diagnostics), `garden` is a maintenance advisor (ranked actionable tasks). They share the same diagnostic data but present it differently.

`map --by-area` and `diff --by-area` are new modes on existing commands, not new commands.

### The agent fix cycle

Garden, orient, and check compose into a complete fix cycle. Garden identifies problems. Orient provides context to fix them. Check verifies the fix. Each garden task emits the commands for the next steps:

```
$ anneal garden

 2. [fix]   1 undischarged obligation in compiler/        blast=high
             COMP-OQ-1 has no Discharges edge
             fix:     add `discharges: [COMP-OQ-1]` to resolving document frontmatter
             context: anneal orient --area=compiler --budget=20k
             verify:  anneal check --area=compiler
 3. [tidy]  9 orphaned labels in compiler/                blast=med
             OD-1..OD-7 defined but never referenced
             context: anneal orient --area=compiler --budget=20k
             verify:  anneal check --area=compiler
```

An agent can execute garden → orient → fix → check without human guidance. The garden output closes the loop: `fix:` tells the agent what to do, `context:` points to the orient command for background, and `verify:` points to the check command for confirmation. For obligation tasks, the `fix:` hint includes the exact frontmatter syntax needed — the agent doesn't need to know how discharge edges work.

### The agent edit cycle

Orient and impact compose into a before/after pair for edits. Orient tells the agent what to read before touching a file. Impact tells it what will need review after.

```
anneal orient --file=compiler/cell-graph-optimization.md   # before: what to read
anneal impact compiler/cell-graph-optimization.md          # after: what's affected
```

The skill should teach this as a paired workflow.

## Discoverability

An agent that used anneal extensively on a real corpus still missed `find "" --status=active` and `diff --days=7` — features that already existed. The problem wasn't missing capability but invisible workflows. Agents are smart; they don't need a manual. But the workflows need to be *reconstructable* from the surfaces agents actually encounter: the skill file, `--help` text, and command output.

### Two workflows to teach

**Orientation** — arriving at a corpus, understanding what exists. Two variants:

General (new corpus):
```
anneal areas                                    # what's here?
anneal find --kind=file --context               # what does each thing do?
anneal explain convergence                      # what do the statuses mean?
anneal orient --budget=50k                      # what should I read?
```

Narrowing (specific task):
```
anneal orient --file=target.md --budget=30k     # what context do I need?
anneal map --around=target.md --upstream         # what feeds into this?
anneal impact target.md                         # what breaks if I change this?
anneal check --area=compiler                    # any issues nearby?
```

**Gardening** — maintaining corpus health:
```
anneal garden                                   # what needs fixing, ranked?
# → follow fix:/context:/verify: hints
anneal diff --days=7                            # what moved since last session?
anneal areas --sort=grade                       # which areas are degrading?
```

### Where these workflows must be visible

**Skill file.** The First Moves section teaches the orientation loop. It should distinguish general orientation (areas → find → orient) from narrowing (orient --file → map --upstream → impact). The gardening workflow belongs in a separate section — it's a different intent than orientation.

**Top-level `--help`.** The START HERE block already lists key commands. It should reflect the orientation flow: `areas` for shape, `orient` for reading list, `garden` for maintenance. The current block emphasizes `status` and `check` — those remain, but areas and orient are the stronger entry points for arriving agents.

**Command `--help` EXAMPLES.** Each command's examples should include the non-obvious composable patterns. `find --help` should show `find --status=active --kind=file --context` (the "active inventory" query). `diff --help` should show `diff --days=7` prominently. These are the patterns agents miss.

**Command output cross-references.** Commands that surface problems should hint at commands that provide context or remediation. Garden already does this with `fix:`/`context:`/`verify:` per task. Other commands should follow the pattern:

- `status` with errors > 0: hint at `check`
- `areas --sort=grade` with C/D areas: hint at `garden`
- `check` with E002 obligations: hint at `explain obligation`
- `explain obligation` with outstanding: include remediation syntax

The principle: **an agent following output hints should be able to reach the right next command without consulting documentation.** Garden → orient → fix → check is a closed loop. The skill teaches the entry points; command output teaches the transitions.

## Non-goals

- **Area configuration.** Areas are auto-detected from directories. We don't add an `[areas]` config section. Concern groups already serve the "custom grouping" role.
- **Multi-area scoping.** `--area` accepts one area. Use concern groups for cross-cutting views.
- **Trend persistence.** Area grades use existing snapshot history. No new storage mechanism.
- **Git integration.** Git dates are out of scope for the initial design. Filename and frontmatter dates are sufficient.
- **Dashboard rendering.** These commands produce text and JSON. Rendering dashboards is a downstream concern (agent skills, scripts, CI).
- **Standalone `recent` command.** Chronological views are served by `--sort=date` and `--since=Nd` on existing commands.
- **Content summarization.** Anneal exposes frontmatter fields (`purpose:`, `note:`) but does not interpret or summarize body text. Summarization is the agent's job; anneal provides the structural data the agent needs to decide what to read.
- **Delivery assessment.** Anneal tracks declared status in frontmatter, not whether a spec's work has been implemented. Comparing specs against code requires code understanding, which is outside the corpus-structural scope.
- **Work recommendation.** Anneal can surface active documents sorted by structural signals (dependency depth, recency, area health). It does not reason about project priorities, deadlines, or team capacity. Garden ranks maintenance tasks by blast radius, not business importance.
- **Diagram generation.** Anneal provides raw graph data and tree traversals. Annotated diagrams that combine structural knowledge with semantic understanding (purposes, delivery status, relationships) are the agent's job — anneal provides the data, the agent provides the interpretation.
- **Code-corpus alignment.** Anneal tracks convergence within a knowledge corpus. Cross-referencing specs against source code, issue trackers, or CI status is a separate tool's responsibility.
