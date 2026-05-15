---
status: current
date: 2026-05-13
description: >
  anneal v2.0 — the master spec. A programmable knowledge-corpus runtime
  for agents and humans. Substrate (Datalog primitives + convergence
  standard library) is decoupled from sources (markdown, MDX, code,
  issue trackers, host applications) by the Source trait. The same
  agent skills work across every corpus the substrate can ingest.
supersedes:
  - 2026-05-03-language-redesign.md
  - 2026-05-13-primitives-first-corpus-vm.md
depends-on:
  - anneal-spec.md
  - 2026-05-07-engine-spike-and-parity-protocol.md
  - 2026-05-13-engine-spike-results.md
---

# anneal v2.0 — Programmable Corpus Runtime

A knowledge corpus is a graph of typed handles with addressable content,
relationships, lifecycle state, and history. anneal v2.0 is the runtime
that makes that graph queryable, extensible, and durable across sessions
— for any source that can produce handles and edges, not just markdown.

The product is:

- **A substrate**: a Datalog dialect with stored and derived primitives,
  a convergence standard library, self-description, and provenance.
- **A family of sources**: format adapters that turn markdown, MDX, code,
  issue trackers, host applications, and future formats into handle/edge
  facts the substrate can reason about. Sources are the only
  data-ingestion boundary; ranking, authorization, trail policy, and
  search backends are separate extension seams.
- **Two surfaces**: a CLI for humans and shell scripts; an MCP server
  for agents. Both project the same runtime contracts.

What makes a system *anneal* is the convergence vocabulary —
settledness as a first-class dimension. The substrate ships that
vocabulary as readable, overridable `.dl` files. Every project can
use it as-is, replace it, or extend it for the work that matters to
that team.

---

## Part I: Why this shape [CR-F]

### §1 The thing agents need

A cold agent arriving in a knowledge corpus has four problems:

1. **Localization.** "Where is the thing relevant to my task?"
2. **Composition.** "What's the connection between these things?"
3. **Memory.** "What did the previous agent decide?"
4. **Extension.** "How do I leave my learning behind?"

anneal v2.0 answers each as a runtime primitive: `search` for
localization, the Datalog dialect for composition, `*trail` and
snapshots for memory, `@verb` and `anneal.dl` for extension.

### §2 Why substrate, not a markdown tool [CR-D1]

**Definition CR-D1 (Substrate).** A program that exposes a typed
knowledge-graph runtime, format-agnostic, with sources as plug-ins.
The substrate's value compounds across every source it can ingest;
markdown is one source among many.

Treating anneal as "the markdown corpus tool" loses three futures the
substrate makes natural:

- **MDX, AsciiDoc, org-mode, JSON-schema, YAML, code, issue trackers**
  — every format is an adapter; the convergence vocabulary and query
  language are the same.
- **Anneal embedded in another application** — host-corpus, an Ash app,
  any host that wants its own runtime state to be agent-queryable.
- **Multi-corpus federation** — an agent works across several
  corpora simultaneously (a project's design docs, its source code,
  its related upstream projects).

The substrate also future-proofs against agent evolution: today's
agents have specific shapes (token budgets, no shared memory). The
primitives, self-description, trails, and programmable extension
survive any agent generation.

### §3 The cold-agent test [CR-D2]

**Definition CR-D2 (Cold-agent test).** Given a real corpus and a
goal, a cold agent (no prior session memory of the corpus) reaches
the answer in ≤2 tool calls plus optional `--explain`.

This is the product's primary acceptance criterion. Engine viability
(MVS coverage, perf gates) is necessary but not sufficient. The
test is fixture-pinned in §44 — count alone is gameable; the spec
names specific corpora, queries, expected handles, and rank
tolerances.

---

## Part II: Architecture [CR-A]

### §4 Three layers [CR-D3]

**Definition CR-D3 (Layering).**

| Layer | Form | Responsibility |
|---|---|---|
| **Substrate** | `anneal-core` crate | Datalog runtime, primitives, convergence stdlib, IR, provenance, trail capture |
| **Adapters** | `anneal-md`, `anneal-mdx`, `anneal-code`, `anneal-host`, … | Format-specific extraction; implement [`Source`](#5-the-source-trait-cr-d4) |
| **Surfaces** | `anneal-cli`, `anneal-mcp`, library API | Project the substrate to humans, agents, and host applications |

### §5 The Source trait [CR-D4]

**Definition CR-D4 (Source trait).** The contract every adapter
implements. Sources are the only data-ingestion extensibility point.
*Other* extension seams (ranking/scoring, authorization policy, trail
summarization, MCP tool registration) are separate plugin surfaces
declared in Part VII.

```rust
pub trait Source {
    /// Self-describe what this source recognizes, snapshot capability
    /// declarations, and the discovery-fact keys it consumes.
    fn describe(&self) -> SourceInfo;

    /// Extract facts. Sources do not write to shared mutable state;
    /// they return a FactBatch which the runtime merges atomically
    /// with generation tracking.
    fn extract(&self, cx: &SourceContext) -> Result<FactBatch, SourceError>;
}

pub struct SourceContext<'a> {
    pub corpus: CorpusId,                  // logical corpus this Source contributes to
    pub roots: &'a [Utf8Path],             // where the Source's data lives
    pub config_facts: &'a ConfigFacts,     // pre-loaded discovery facts (see §7)
    pub time_ref: Option<TimeRef>,         // None = current; Some = historical
    pub previous_generation: Option<Generation>,  // for incremental delta extraction
    pub actor: ActorContext,               // identity + capabilities for policy enforcement
    pub cancellation: CancellationToken,
}

pub struct FactBatch {
    pub mode: FactBatchMode,              // FullSnapshot or Delta
    pub generation: Generation,            // monotonic; runtime treats new gen as supersede
    pub handles: Vec<HandleFact>,
    pub edges: Vec<EdgeFact>,
    pub content: Vec<ContentFact>,
    pub spans: Vec<SpanFact>,
    pub meta: Vec<MetaFact>,
    pub concerns: Vec<ConcernFact>,
    pub retractions: Vec<NativeId>,        // used only for Delta batches
}

pub enum FactBatchMode {
    FullSnapshot,                          // replaces all current facts for (corpus, source)
    Delta,                                 // upserts facts and retracts listed native ids
}

pub struct SourceInfo {
    pub name: &'static str,                // "markdown", "mdx", "rust-code", "github-issues"
    pub recognizes: Vec<Pattern>,          // ["**/*.md"]
    pub doc: &'static str,
    pub config_keys: Vec<ConfigKey>,       // adapter-qualified discovery facts consumed
    pub capabilities: SourceCapabilities,  // see §11
    pub search: Option<SearchInfo>,        // search scoring contract if the Source contributes hits
}

pub struct SourceCapabilities {
    pub supports_git_ref: bool,            // can re-extract against arbitrary git refs
    pub supports_time_snapshot: bool,      // honors SourceContext.time_ref
    pub supports_incremental: bool,        // honors previous_generation for deltas
    pub live_only: bool,                   // historical at() returns "unsupported" not silent
}

pub struct SearchInfo {
    pub reason_vocabulary: Vec<&'static str>,
    pub fields: Vec<&'static str>,
    pub low_confidence_threshold: f32,      // default 0.5 if omitted
}
```

A `Source` is one of: a directory walker (markdown, MDX, AsciiDoc),
a source-code analyzer (anneal-code), an external-system reader
(GitHub issues, CI events), or a host application's runtime
introspector (anneal-host: host-corpus's Ash resources, Phoenix routes,
Oban jobs as handles).

The runtime is identical across sources. Only the extraction differs.

### §6 Other extension seams [CR-D5]

**Definition CR-D5 (Plugin surfaces).** v2.0 names four extension
seams beyond `Source` so adapter authors don't contort everything
into fact-emission:

| Surface | Trait | Purpose |
|---|---|---|
| Data ingestion | `Source` | Emit handle/edge/content facts |
| Ranking and scoring | `Ranker` | Per-adapter `search` score calibration; tie-break policy |
| Authorization policy | `Policy` | Actor → allow/deny on read/search/eval; scoped to MCP and host-embed |
| Trail summarization | `TrailSummarizer` | Engine-generated trail-entry summaries with privacy-aware redaction |

`Ranker` and `Policy` ship default implementations in `anneal-core`.
Adapters and hosts override them; the runtime uses the most-specific
one. `TrailSummarizer` is mandatory per §13 because trail privacy
needs explicit per-corpus configuration.

### §7 Ingestion lifecycle [CR-D6]

**Definition CR-D6 (Ingestion lifecycle).** Each invocation of the
runtime executes these phases in order:

```
1. Adapter registration: link Source impls into the binary.
2. Phase A — config parse:
   a. Read anneal.toml (lattice, [convergence], policies, source bindings).
   b. Parse anneal.dl, splitting into:
      - discovery facts (file_extension, label_pattern, code_language, …)
      - rule clauses (Horn rules with `:=`)
      - verb declarations (@verb annotations)
3. Phase B — Source extraction:
   a. For each enabled Source, build SourceContext from anneal.toml + discovery facts.
   b. Source.extract(cx) → FactBatch.
   c. Runtime merges batches with generation tracking:
      - FullSnapshot replaces all current facts for (corpus, source).
      - Delta upserts returned facts and retracts listed native ids.
4. Phase C — rule load:
   a. Load embedded prelude (graph.dl, convergence.dl, checks.dl, ranking.dl, views.dl).
   b. Load anneal.dl rule clauses.
   c. Resolve verb registrations.
   d. Static analysis: stratification, safety, diagnostic ID uniqueness/prefixes.
5. Phase D — evaluation:
   a. IR fixpoint over current generation of facts.
   b. Surface accepts queries; runtime evaluates.
   c. Trail capture per §13.
```

Discovery facts are consumed by Sources in Phase B, not by the IR.
Rules and verbs are loaded in Phase C, after facts exist. This
resolves the contradiction Phase 1 would have hit had `anneal.dl`
needed to load all-at-once.

A long-running runtime (MCP server, embedded host) keeps populated
relations in memory and re-runs Phase B on source-change events;
generation tracking handles retractions. A deleted file or host
object disappears because a FullSnapshot no longer contains it, or
because a Delta batch lists its `native_id` in `retractions`.

### §8 Crate topology

```
anneal/
├── crates/
│   ├── anneal-core/             # the substrate
│   ├── anneal-legacy/           # transition-only v1 parser/config bridge
│   ├── anneal-md/               # markdown adapter
│   ├── anneal-cli/              # the binary; links core + md
│   └── anneal-mcp/              # MCP server; links core + md
├── adapters/                    # external adapter crates (v2.1+)
│   ├── anneal-mdx/
│   ├── anneal-code/
│   └── anneal-host/
└── .design/
```

**Definition CR-D32 (Transition-only legacy boundary).**
`anneal-core` is the only permanent crate other anneal crates depend
on. During the v1-to-v2 migration, `anneal-legacy` is allowed as a
transition-only parser/config boundary so adapters can preserve
v1 parity without depending on the root CLI package; it must not
become a substrate extension point.
Adapters are siblings. A consumer can link any combination of
adapters into their own binary; the CLI ships markdown by default.

**Engine choice is internal to `anneal-core`.** v2.0 uses
[`ascent`](https://github.com/s-arash/ascent) for engine-derived
primitives and a dynamic IR for the rule layer (prelude + project +
inline). The surface language is a stratified Datalog dialect with
aggregation — semantics every Datalog engine in the relevant class
(ascent, Crepe, hand-rolled, soufflé) supports. The grammar in
Part IV (Steele's criterion for project verbs, `@verb` declarations,
adapter-qualified discovery facts, `context` as composition primitive)
is designed for agents reasoning about corpora, not for Rust
developers embedding a fact engine; it deliberately does *not* mirror
ascent's surface syntax. This is a load-bearing invariant: swapping
engines (for performance, for non-Rust embedding, for an incremental
evaluator) is allowed because the user-facing language and the
stored-relation schema are independent of the engine choice. The IR's
internal AST stays close to ascent's shape so the primitives-lowering
pass is thin, but that's an implementation detail of `anneal-core`,
not a public contract.

---

## Part III: Substrate primitives [CR-P]

### §9 Identity model [CR-D7]

**Definition CR-D7 (Identity).** Every fact carries enough origin to
distinguish it across corpora, sources, and adapter combinations.
Internal identity is `(corpus, source, kind, native_id)`; display
identity is the user-friendly id the adapter chose.

This applies from v2.0, not v2.2, because adding fields later forces
a query-breaking schema migration. Federation UI can defer; the
schema cannot.

### §10 Stored relations [CR-D8]

**Definition CR-D8 (Stored primitives).** The relations every adapter
populates and every rule may join on.

```
*handle{
  id,           // display id, locally unique within (corpus, source)
  kind,         // "file" | "section" | "label" | "version" | "external"
  status,       // string in the project lattice; may be null
  namespace,    // string; "" if not labeled
  file,         // adapter-meaningful path or locator
  line,         // declaration line; 0 if not applicable
  date,         // ISO date; may be null
  area,         // top-level grouping (first path component or adapter-defined)
  summary,      // short text; adapter-generated
  corpus,       // CorpusId
  source,       // "markdown" | "mdx" | "rust-code" | host-defined
  native_id,    // adapter-private id used for delta tracking
  origin_uri,   // canonical URI (file://, https://, app://host-corpus/ash/User, …)
  revision,     // source revision (git sha, file mtime hash, app-version)
  generation    // monotonic; latest generation wins on conflict
}

*edge{from, to, kind, file, line, corpus, source, generation}

*meta{handle, key, value, corpus, source, generation}

*content{
  handle, span_id, lines, text, tokens, corpus, source, generation
}

*span{
  id, handle, start_line, end_line, summary, corpus, source, generation
}

*concern{name, member, source, corpus, generation}

*config{key, value, corpus}               // from anneal.toml; runtime-populated
*snapshot{at, id, key, value, corpus}
*trail{...}                                // see §13
*generation{corpus, source, current}       // current generation per (corpus, source)
```

| Relation | Purpose |
|---|---|
| `*handle` | Identity: every thing the corpus knows about |
| `*edge` | Typed binary relationships |
| `*meta` | Open key/value extension on handles |
| `*content` | Bounded text spans of a handle; the read-substrate |
| `*span` | Citable region with line range and engine-generated summary |
| `*concern` | Cross-cutting groupings: any handle can belong to any concern |
| `*config` | Runtime configuration as queryable facts (lattice, namespaces) |
| `*snapshot` | Historical handle state from snapshot history |
| `*trail` | Session paths (§13) |
| `*generation` | Per-source generation tracker (§7); supports retraction |

Every source-derived stored relation is **adapter-populated and
generation-tracked**. `*config` is runtime-populated but still
corpus-scoped so federation never merges two corpora's config facts.
The runtime atomically swaps a `FullSnapshot` generation for an
entire `(corpus, source)` or applies a `Delta` batch's upserts and
retractions. This makes long-running runtimes (MCP, host-embed)
correct under source edits and deletions.

**Rule CR-R6 (Edge closure).** `*edge.to` is a handle identity string,
not a foreign-key requirement. Unresolved reference attempts remain
stored as `*edge` rows even when no matching `*handle{id: to}` exists;
consumers that require a closed graph explicitly join `*edge.to` to
`*handle.id`.

Rationale: existence diagnostics, resolution-cascade suggestions, and
legacy parity need to reason about failed references, not only resolved
relationships.

### §11 Engine-derived predicates [CR-D9]

**Definition CR-D9 (Function primitives).** Predicates implemented in
the substrate (not as Datalog rules) because they need Rust-native
traversal, IO, ranking, or content access. **All return relational
shapes** — pattern matching binds variables; no record-style field
access.

```
// Graph
upstream(h, anc)
downstream(h, desc)
impact(h, x, depth)
neighborhood(h, depth, member)

// Status and lifecycle (work against the configured lattice)
terminal(h)
active(h)
settled(h)
pipeline_position(h, n)
pipeline_position_for(s, n)

// Obligation lifecycle
obligation(h)
discharged(h)
undischarged(h)

// Counts and metrics
cite_count(h, n)
in_degree(h, n)
out_degree(h, n)
discharge_count(h, n)
freshness(h, days)
flux(h, days, delta)             // ground `days`; binds `delta`
token_estimate(h, n)

// Content retrieval — all RELATIONAL
search(query, handle, span_id, score, reason, field, low_confidence)
read(handle, budget, span_id, text, start_line, end_line, tokens)
read_full(handle, content)       // capability-gated; see §16
match(pattern, handle, line, snippet)

// Self-description
schema(name, kind, signature, determinism, source_provenance)
predicates(name, doc, source_file, source_lines)
verbs(name, query, doc, output_schema)
describe(name, doc)
source_of(name, file, lines)     // renamed from `source` to avoid collision with *source field
examples(name, example)
sources(name, recognizes, capabilities, doc)
```

**Definition CR-D35 (Sealed engine primitives).** Substrate-only
engine primitive predicate names in CR-D9 are sealed. Prelude,
project, import, inline, and fact clauses may call them but must not
define, shadow, or union with them. Projects that need domain-specific
variants wrap sealed primitives in separately named derived
predicates. Rationale: sealed primitive semantics are part of the
engine-replaceability contract; letting corpus rules redefine them
would make runtime behavior depend on load order rather than the
substrate contract.

**Definition CR-D36 (Soft lifecycle primitives).** Lifecycle
predicates whose semantics are corpus-specific (`terminal/1`,
`active/1`, `settled/1`, `pipeline_position/2`,
`pipeline_position_for/2`, `obligation/1`, `discharged/1`,
`undischarged/1`) are runtime-provided defaults, not sealed
substrate contracts. If no loaded unqualified rule defines the
predicate, the default primitive relation is available. If the
prelude, project, include, or inline layer defines the same
unqualified predicate, CR-D21 shadowing applies and the rule
definition replaces the default. Module-qualified imports do not
shadow unqualified soft defaults unless a project explicitly re-exports
them under the unqualified name. Rationale: code, host, issue, and
markdown corpora need a common lifecycle vocabulary without forcing
markdown's status model into every adapter.

All other CR-D9 primitives are sealed unless a later CR-D* explicitly
marks them soft.

**Definition CR-D37 (Default scalar lifecycle metrics).**
`discharge_count(h, n)` counts incoming `Discharges` edges for known
handle `h`. `freshness(h, days)` returns whole days since
`*handle.date` when present and parseable as an ISO date, clamped at
`0` for future dates; missing or unparseable dates yield `0` so
fresh-but-undated handles remain queryable. `token_estimate(h, n)`
returns the sum of `*content.tokens` for `h`, or `0` when no content
spans exist. Unknown handles produce no rows for these relations.
`flux(h, days, delta)` requires `h` to be a known handle and `days` to
be ground and non-negative; otherwise it produces no rows. It counts
status transitions for `h` across `*snapshot{id: h, key: "status"}`
rows within the window plus the current `*handle.status`. With no
matching history, `delta` is `0`. Rationale: these metrics must be
total over known handles so agent queries can distinguish "no signal"
from "relation missing"; snapshot-backed precision can improve
without changing the relational shape.

The aggregation form `TopK{k: N, key: score : body}` (Part IV §17) provides
bounded selection. There is no parallel `top_k` function primitive
and no surface syntax shortcut — one mechanism, one place.

### §12 Search scoring contract [CR-D10]

**Definition CR-D10 (Search contract).** Every adapter that
contributes search results emits raw `SearchHit` candidates. The
runtime passes candidates through the active `Ranker` before exposing
the public `search(...)` relation, so the `score` column agents see
is calibrated for the loaded adapter set.

- Public `score` is a calibrated float in `[0.0, 1.0]`. 1.0 means
  "strongest match after the active Ranker"; 0.0 means "no signal."
- Adapter raw scores are **not** directly comparable. Provenance may
  expose both `raw_score` and calibrated `score`; ordinary queries
  see only calibrated `score`. The default `Ranker` ships with
  documented calibration; projects override via `[ranking]` in
  `anneal.toml`.
- `reason` is a short string explaining the match
  (`"title-substring"`, `"semantic-cluster"`,
  `"frontmatter-key-match"`). Adapters document their reason
  vocabulary in `SourceInfo`.
- `field` names which logical field of the handle matched (`"title"`,
  `"body"`, `"frontmatter:status"`, `"identifier"`). Used by agents
  to decide whether a hit is structural or content-based.
- Ordering by `score` is deterministic given a fixed corpus state and
  query; tie-breakers documented per `Ranker`.
- **Confidence threshold.** Each adapter declares a
  `low_confidence_threshold: f32` in `SourceInfo.search` (default
  `0.5`). Hits with calibrated `score < threshold` carry
  `low_confidence: true` in the relation, signalling agents that
  the hit is plausible but uncertain. The default `Ranker` filters
  low-confidence hits from `TopK` results unless the query
  explicitly opts in (`search_include_low_confidence` flag in
  `anneal.toml` `[ranking]`, or `--include-low-confidence` on the
  CLI). The relation shape:

  ```
  search(query, handle, span_id, score, reason, field, low_confidence)
  ```

  Agents reading raw rows always see all hits with their
  confidence flag; agents consuming `TopK` get only high-confidence
  hits by default, eliminating the "0.62 hit looks comparable to
  0.93 hit" failure mode the live workflow simulation surfaced.

### §13 Trails [CR-D11]

**Definition CR-D11 (Trail).** A session's path through the substrate
— queries, search hits, reads, derived conclusions — captured into
`*trail` as the runtime executes. Trail capture is mandatory in
v2.0; raw expression and content capture are policy-controlled.

```
*trail{
  session_id,         // opaque; uuid unless host supplies one
  step,               // monotonic ordinal within the session
  timestamp,          // ISO datetime
  actor,              // ActorContext.actor; "anonymous-cli" by default
  corpus,             // which corpus this entry refers to
  verb,               // verb invocation name; "-e" for ad-hoc queries
  redacted_expr,      // expr with values redacted per policy
  input_hash,         // hash of full expression (deterministic provenance)
  surfaced_refs,      // list of {corpus, source, handle, span_id, score} emitted
  consumed_refs,      // subset of surfaced_refs the agent actually used
                      // in the next step (read, follow-up query, etc.)
  prelude_hash,       // hash of loaded prelude; supports reproducibility
  source_generations, // {(corpus, source): generation} snapshot at query time
  visibility,         // "public" | "team" | "private" — policy-derived
  retention,          // ISO duration; runtime garbage-collects past expiry
}
```

**Surfaced vs consumed.** `surfaced_refs` is everything the verb's
output stream contained. `consumed_refs` is the subset the agent
*acted on* — handles passed to a subsequent `read`, handles
referenced in a later query, handles selected via `run_verb`
follow-up. The runtime infers `consumed_refs` from observed
verb-to-verb dataflow within the session; explicit selection is
also possible via the `TrailSummarizer.note_consumed(handle)`
callback from a host application.

This distinction matters for trail replay (v2.1): a replay agent
re-executes consumed paths, not every surfaced candidate. The
output-explosion failure mode the live workflow simulation
surfaced — context verb surfaces 6 hits + 4 spans + 2 edges, agent
uses 1 — is resolved by recording both sets and treating consumed
as the load-bearing path.

A `TrailSummarizer` (Part VI extension seam) produces the
`redacted_expr` and may strip surfaced/consumed refs for handles
whose `visibility` is `private`. Default summarizer redacts values
inside string literals and meta-key values matching configured
patterns (`secret`, `password`, customer ids).

Trails persist to `.anneal/trails/<session-id>.jsonl` on session end.
Replay/diff workflows are forward-looking (v2.1; §47).

### §14 Provenance contract [CR-D12]

**Definition CR-D12 (Provenance).** Every output record can be
expanded via `--explain` (CLI) or `derivation: true` (MCP) into a
derivation tree:

- `search(...)` rows that brought a handle into consideration, with
  calibrated scores, optional raw scores, reasons, matched fields
- `*content` spans the engine consulted
- `*edge` rows that joined to produce each derived fact
- `*meta` and status values that participated
- rule chain (prelude, project, inline `where`) that fired
- `prelude_hash` and `source_generations` at evaluation time

Provenance is **lazy and bounded**. The IR records derivation
metadata as it computes, but the full tree is only materialized on
`--explain`. Per-record derivation is bounded to a configurable
depth (default 5 levels); deeper chains report `... + <n> more
levels (use --explain-depth)` rather than expanding.

For recursive rules, derivation is path-summarized: chains of the
same rule with bound arguments collapse to "via <rule> × N hops"
unless `--explain-depth` is explicit.

### §15 Snapshots and time travel [CR-D13]

**Definition CR-D13 (`at(<ref>)` block).** A body fragment that scopes
its sub-body to evaluate against historical corpus state.

The runtime resolves `<ref>` differently per source. Each `Source`
declares its snapshot capabilities (§5). A query that crosses
sources at time T reports **partial unsupported history**, not
silently mixes now-state and then-state:

```
warning: at("HEAD~3") evaluated against partially-supported sources:
  - markdown (anneal-md): supports_git_ref ✓ — re-extracted at HEAD~3
  - rust-code (anneal-code): supports_git_ref ✓ — re-extracted at HEAD~3
  - github-issues (anneal-issues): live_only — using current state
  - host-corpus-runtime (anneal-host): supports_time_snapshot ✓ — using nearest snapshot
```

Reference forms:

| Form | Mechanism | Cost |
|---|---|---|
| `at("snapshot:last")` / `at("snapshot:<id>")` | read `.anneal/history.jsonl` | <100ms |
| `at("--7days")` / `at("2026-04-01")` | resolve to nearest snapshot | <100ms |
| `at("HEAD~3")` / `at("v0.2.1")` / `at("<sha>")` | git ref: re-run Sources with `supports_git_ref` | O(corpus) per supporting source |

### §16 `read_full` and capability gating [CR-D14]

**Definition CR-D14 (Capability gating).** Some primitives are
dangerous in unattended-agent contexts. The runtime distinguishes:

| Primitive | Default capability | Notes |
|---|---|---|
| `search`, `read`, `match`, `schema`, `verbs`, `describe` | Always allowed | Bounded by design |
| `read_full` | Requires `read_full` capability | Hard budget (default 8000 tokens); explicit error if larger; never exposed as a default MCP tool |
| `eval` / `-e` | Requires `eval` capability | Default-allowed for CLI; default-denied for MCP without per-tool override |
| Trail read of `private`-visibility entries | Requires `trail_private` capability | Default-denied |
| Source extraction with non-default actor | Per-`Source` policy | Adapters may scope by actor; host-embed sets this explicitly |

`ActorContext.capabilities` is set by the surface. CLI starts with
all capabilities; MCP starts with a conservative default; host-embed
sets explicitly. The `Policy` trait (§6) overrides any of the above
per project.

---

## Part IV: The language [CR-L]

### §17 Grammar

Modern Datalog. Named fields on stored relations, lowercase
identifiers, `:=` for "is true when," `?` for queries,
`*relation{...}` for stored data.

```
program     := statement*
statement   := fact | rule | query | directive

fact        := head '.'
rule        := head ':=' body '.'
query       := '?' [local_rules] body '.'
directive   := 'include' string '.'
             | 'at' '(' string ')' '{' statement* '}'
             | '@verb' '(' verb_args ')'
             | 'import' ident 'from' string '.'   // see §28

head        := ident '(' positional_arg_list ')'
local_rules := ('where' rule)+
body        := atom (',' atom)*
atom        := stored | derived | comparison | aggregation | negation | time_block
stored      := '*' ident '{' field_list '}'
derived     := ident '(' call_arg_list ')'
comparison  := expr op expr
negation    := 'not' (stored | derived)
aggregation := value_or_tuple '=' agg_fn '{' [agg_args ':'] value_or_tuple ':' body '}'
time_block  := 'at' '(' string ')' '{' body '}'

field_list  := field (',' field)*
field       := ident                        # bind: same name as variable
             | ident ':' value_or_var       # bind: explicit
positional_arg_list := value_or_var (',' value_or_var)*
call_arg_list := call_arg (',' call_arg)*
call_arg    := expr                         # positional
             | ident ':' expr               # named call-site sugar
agg_args    := named_arg (',' named_arg)*
named_arg   := ident ':' expr
value_or_tuple := expr | tuple
tuple       := '(' expr (',' expr)+ ')'
value_or_var := expr | '_'
expr        := var | literal | function_call | '(' expr ')'
             | expr arith_op expr
function_call := ident '(' call_arg_list ')'
var         := /[a-z_][a-z0-9_]*/
literal     := string | number | bool | list

agg_fn      := 'Count' | 'Sum' | 'Min' | 'Max' | 'Avg' | 'List' | 'Set'
             | 'TopK' | 'Rank' | 'TakeUntil'
op          := '=' | '!=' | '<' | '>' | '<=' | '>='
             | 'in' | 'matches' | 'contains'
             | 'starts_with' | 'ends_with'
arith_op    := '+' | '-' | '*' | '/' | '%'
ident       := /[a-z_][a-z0-9_]*/
```

Comments: `#` to end of line. Whitespace insignificant. Statements
terminated by `.`. Strings double-quoted with standard escapes.
Named predicate arguments are call-site sugar over the predicate's
declared signature. Rule heads are canonical positional definitions;
calls may use positional arguments followed by named arguments.
Named arguments are not records and do not introduce field access.

### §18 Types and operators

Dynamic, four primitive types plus one composite:

| Type | Literal |
|---|---|
| String | `"OQ-37"` |
| Number | `42`, `3.14` (unified i64/f64) |
| Bool | `true`, `false` |
| Null | `null` |
| List | `[1, 2, 3]`, `["raw", "decided"]` |

No first-class records. Records exist only as patterns inside
`*relation{...}`. Multi-column outputs from function primitives like
`search` and `read` are **relational** — pattern-match each column
into a variable:

```
? search("conformance", h, span_id, score, reason, field, low_confidence),
  low_confidence = false,
  score > 0.7,
  read(h, 4000, span_id, text, start, end, tokens).
```

This guarantees uniform handling across stored and derived
relations.

Operators:

| Operator | Meaning |
|---|---|
| `=` | unification or equality (context-dependent) |
| `!=` `<` `>` `<=` `>=` | comparison; numbers, strings (lexical), dates |
| `in` | `x in [a, b, c]` or `x in *list_relation` |
| `matches` | `s matches "regex"` |
| `contains` | `s contains "substring"`; list contains element |
| `starts_with` `ends_with` | string prefix / suffix |
| `+` `-` `*` `/` `%` | arithmetic on numbers |

Built-in functions (used in expressions, not as predicates):

```
basename(path)      length(s)        lower(s)       upper(s)
max(a, b)           min(a, b)        abs(n)         days(d1, d2)
```

### §19 Stored vs derived predicates [CR-D15]

**Definition CR-D15 (Stored).** A relation prefixed `*` reads from
facts produced by Sources during ingestion or from configuration
(`*config`, `*snapshot`, `*trail`). Pattern-matching binds field
values to variables.

**Definition CR-D16 (Derived).** A relation without `*` is defined by
one or more rules. Rules may live in the prelude, in `anneal.dl`,
or inline via `where` clauses.

The `*` prefix is a visible marker: *this is real data, not
derived.*

### §20 Aggregation [CR-D17]

**Definition CR-D17 (Aggregation).** Form:
`agg_var = AggFn{ [agg_args :] contributing_var : sub_body }`.

`TopK` and `TakeUntil` take agg_args; the standard aggregators don't.

```
total_potential(area, total) :=
  total = Sum{ e : potential(h, e), area_of(h, area) }.

namespace_open_count(ns, n) :=
  n = Count{ h : *handle{id: h, namespace: ns},
                 obligation(h),
                 not discharged(h) }.

top_blockers(h, score) :=
  (h, score) = TopK{ k: 10, key: score :
    *handle{id: h, namespace: "OQ"},
    not discharged(h),
    potential(h, score)
  }.

read_until_budget(h, span_id, text, used) :=
  used = TakeUntil{ budget: 4000, sum: tokens :
    span_id : read(h, 4000, span_id, text, _, _, tokens)
  }.
```

Standard Datalog aggregation semantics: compute the set of values for
the contributing variable such that the sub-body holds, then collapse
with the aggregation function. Free variables outside the
aggregation form the grouping key. `TopK` and `TakeUntil` are
first-class — set semantics alone are insufficient for agent
retrieval workflows.

**Definition CR-D33 (Aggregate result unification).** The left-hand
side of an aggregation form participates in normal equality
unification. If the result variable is already bound, the aggregate
row survives only when the computed aggregate value equals the bound
value. Aggregate evaluation never overwrites an existing binding.

**Definition CR-D34 (Empty-group origination).** A grouping key exists
only when it is positively bound elsewhere in the rule body outside
the aggregation form. For every such group, `Count` emits a value,
including `0` when the aggregate sub-body contributes no rows. Groups
that are not reachable from a non-aggregate body atom are not
synthesized from the value universe.

### §21 Negation, recursion, stratification [CR-D18]

**Definition CR-D18 (Stratified negation).** The runtime partitions
rules into strata such that any predicate referenced under `not` is
fully derived in an earlier stratum. Mutual recursion through
negation is rejected at load time with the cycle named.

Safety rules (enforced at load):

1. Every variable in a rule head must appear positively in the body
2. Every variable inside `not P(...)` must be bound positively
   elsewhere in the same rule
3. No mutual recursion through negation (engine names the cycle and
   all rules participating)

Load error example:

```
error: cyclic negation between 'blocked' and 'advancing'
  blocked/1 (anneal.dl:48) → not advancing(h)
  advancing/1 (anneal.dl:55) → not blocked(h)
fix: derive both from a non-mutually-negated common predicate.
```

### §22 Inline rules, includes, imports [CR-D19]

A query may declare rules visible only for that query:

```
?
  where ancient(q, d) :=
    *handle{id: q, namespace: "OQ", status: "open"},
    freshness(q, d), d > 60.
  ancient(q, d), area_of(q, "compiler").
```

External files via `include`:

```
include "checks/release.dl".
```

`include` merges rules into the global predicate space. Conflicts
form multi-clause definitions, with shadowing warnings per §27.

`import` provides namespaced loading for cases where total
shadowing is undesired:

```
import strict_checks from "checks/release.dl".

? strict_checks.broken_ref(h, why).
```

Imported names are accessed with the `module.predicate` syntax;
they do not collide with the global predicate space. `anneal.dl`
should use `import` for adapter-provided helpers and project
verb-library files; the global namespace is reserved for prelude
overrides and small project vocabularies.

### §23 Output shape [CR-D20]

**Definition CR-D20 (Output contract).**

- **stdout: pure NDJSON.** One record per match, `\n` terminated,
  streamed as derived. No verb-banner text, no "underlying query"
  print on stdout — see §25 for where the query lives.
- **stderr: human text.** Progress, warnings, parse errors with
  line/column. Never NDJSON.

Field names come from the head's variables (or for headless queries,
from the body's bindings, last-mention-wins).

Cardinality: set semantics by default (duplicates deduplicated). For
multiset, use explicit aggregation or include the full key in the
head.

Provenance: `--explain` (CLI) or `derivation: true` (MCP) adds a
`_derivation` field to each record. Without it, records are bare.

### §24 Errors

Three categories, all to stderr, all with file:line context:

**Parse errors** (load):
```
anneal.dl:42:15: expected ',' or '.', got '{'
  potential(h, energy {
                      ^
```

**Static errors** (load): safety violations, stratification cycles,
unknown predicates with did-you-mean suggestions, reserved
diagnostic IDs, adapter-qualified discovery-fact violations,
namespace import resolution failures.

**Runtime errors** (evaluation): regex compile failures, time-travel
ref not found, division by zero, capability denial.

All three exit with code 1. Stdout stays clean — no partial NDJSON
if a query failed mid-evaluation.

---

## Part V: Standard library [CR-S]

### §25 Layout

The substrate embeds the standard library at compile time. The
`prelude_hash` (a content hash of the loaded prelude files) is
recorded in every trail entry and snapshot for reproducibility.

```
anneal-core/src/prelude/
  graph.dl          # structural shapes (orphan, stub, hub)
  convergence.dl    # potential, entropy, blocked, advancing, weights
  checks.dl         # E001, E002, W001-W004, I001, S001-S005
  ranking.dl        # default Ranker calibration as Datalog rules
  views.dl          # the starter verbs as saved queries
```

`anneal source-of convergence` prints the file:lines where the
convergence vocabulary lives. `ANNEAL_PRELUDE_PATH` overrides the
embedded prelude; doing so changes the `prelude_hash` and is
recorded in trails — agents replaying a trail later see whether the
prelude differs.

### §26 Load order and shadowing [CR-D21]

**Definition CR-D21 (Load order).** Phase C of §7 loads:

1. The embedded prelude (`graph.dl`, `convergence.dl`, `checks.dl`,
   `ranking.dl`, `views.dl`)
2. `anneal.dl` rule clauses in the corpus root
3. Inline rules from `where` clauses in the current query
4. The query itself

Later layers shadow earlier layers by predicate name. **Shadowing is
total replacement.** To extend a prelude predicate rather than
replace it, provide multiple clauses for the same head in
`anneal.dl`; multi-clause definitions union as Datalog naturally
does. The runtime warns at load on stderr:

```
warning: anneal.dl:42: 'blocked/1' overrides prelude (2 clauses)
         compare: anneal source-of blocked
```

For predicates that should *never* be shadowed (engine identity
guarantees), the prelude declares `@sealed` — projects attempting to
shadow get a load error.

### §27 Convergence vocabulary [CR-D22]

**Definition CR-D22 (Convergence vocabulary).** Predicates defined in
`convergence.dl` that name the convergence-physics concepts. The
contract between the convergence frame and project predicates.

```
# convergence.dl

# Weights — projects retune in anneal.dl.
potential_weight("undischarged",     5).
potential_weight("broken_ref",       4).
potential_weight("stale_dep",        3).
potential_weight("confidence_gap",   3).
potential_weight("freshness_decay",  2).
potential_weight("missing_meta",     1).
potential_weight("orphan_label",     1).

potential(h, energy) :=
  energy = Sum{ w : entropy(h, source), potential_weight(source, w) }.

entropy(h, "undischarged") :=
  obligation(h), not discharged(h), not terminal(h).

entropy(h, "broken_ref") :=
  diagnostic("E001", _, h, _, _, _).

entropy(h, "stale_dep") :=
  *edge{from: h, to: t, kind: "depends_on"},
  active(h), terminal(t).

entropy(h, "confidence_gap") :=
  *edge{from: h, to: t, kind: "depends_on"},
  pipeline_position(h, n_h),
  pipeline_position(t, n_t),
  n_h > n_t + 1.

entropy(h, "freshness_decay") :=
  *handle{id: h, kind: "file"},
  active(h), freshness(h, days), days > 60.

entropy(h, "missing_meta") :=
  *handle{id: h, kind: "file"},
  active(h),
  not *meta{handle: h, key: "status"}.

entropy(h, "orphan_label") :=
  *handle{id: h, kind: "label"},
  cite_count(h, n: 0),
  not discharged(h).

blocked(h) :=
  active(h),
  potential(h, energy), energy >= 3,
  flux(h, days: 30, delta: 0).

advancing(h) :=
  active(h),
  recently_advanced(h).

recently_advanced(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  pipeline_position_for(prior, p_prior),
  pipeline_position_for(current, p_current),
  p_current > p_prior.
```

**The vocabulary is portable across handle graphs; the lifecycle is
not.** A code corpus where handles are functions/types doesn't have
"status: draft" frontmatter. It has coverage state, deprecation
markers, public/private visibility. Projects define their own
lifecycle by declaring `[convergence] ordering` in `anneal.toml` and
the predicates `terminal/1`, `active/1`, `pipeline_position_for/2`
in `anneal.dl` if the defaults don't fit.

Sample lifecycle profiles ship in `views.dl` for inspiration
(`profile_doc_corpus`, `profile_code_corpus`, `profile_issue_corpus`);
projects copy the one closest to their shape.

### §28 Check rules [CR-D23]

**Definition CR-D23 (Check rule).** A rule whose head is
`diagnostic(...)` deriving a fact representing a consistency
violation.

The check rules from anneal v1.x — E001 (broken refs), E002
(undischarged), W001-W004 (warnings), I001 (info), S001-S005
(suggestions) — live in `checks.dl` as Horn clauses. The substrate
has no hard-coded check logic.

```
# checks.dl excerpt

diagnostic("E001", "error", src, file, line, broken_ref(target)) :=
  *edge{from: src, to: target, kind: _, file, line},
  not *handle{id: target}.

diagnostic("W001", "warning", src, file, line,
           stale_ref(s_status, t_status)) :=
  *edge{from: src, to: target, kind: "depends_on", file, line},
  active(src), terminal(target),
  *handle{id: src, status: s_status},
  *handle{id: target, status: t_status}.
```

### §29 Diagnostic ID rules [CR-R1, CR-R2, CR-R3]

**Rule CR-R1 (Diagnostic ID literal).** Every rule whose head is
`diagnostic(...)` must have a string literal as the first argument.

**Rule CR-R2 (Unique within ruleset).** Two rules with the same
diagnostic ID literal in the same loaded ruleset error at load with
both file:line locations.

**Rule CR-R3 (Reserved prefixes).** The prefixes `E*`, `W*`, `I*`,
`S*` are prelude-owned. Projects use their own prefix
(`PROJ-001`, `RELEASE-002`).

**Projects can fully replace a built-in diagnostic** by forking the
prelude (set `ANNEAL_PRELUDE_PATH`) — the resulting `prelude_hash`
in trails reflects the divergence. Adding clauses to an existing
diagnostic ID via `anneal.dl` is rejected because diagnostic IDs
must be unique. This is intentional: a project replacing `E001`
semantics is a significant deviation and should be a deliberate
prelude fork, not a silent extension.

---

## Part VI: Project extension [CR-E]

### §30 `anneal.dl` conventions

Project predicates, verbs, and discovery facts live in `anneal.dl`
at the corpus root. Section headers organize the file:

```dl
# === discovery ===
# Consumed by Sources in Phase B of ingestion.
# Adapter-qualified: each fact names the source it targets.
md.file_extension(".md").
md.scan_root(".").
md.scan_exclude("node_modules").
md.label_pattern("OQ",    "OQ-(\d+)",    "any").
md.linear_namespace("OQ").

# === imports ===
# Use namespaced imports for adapter helpers or shared verb libraries.
import team_verbs from "verbs/team.dl".
import strict_checks from "checks/release.dl".

# === overrides ===
# Override the prelude's freshness threshold for this corpus.
entropy(h, "freshness_decay") :=
  *handle{id: h, kind: "file"},
  active(h), freshness(h, days), days > 30.

# === project predicates ===
blocking_oq(q) :=
  *handle{id: q, kind: "label", namespace: "OQ", status: "open"},
  upstream(spec, q),
  *handle{id: spec, status: "formal"}.

release_blocker(h, "broken_ref")   := diagnostic("E001", _, h, _, _, _).
release_blocker(h, "undischarged") := diagnostic("E002", _, h, _, _, _).
release_blocker(h, "blocking_oq")  :=
  blocking_oq(h),
  *meta{handle: h, key: "milestone", value: "v0.3"}.

# === verbs ===
@verb(
  name: "release-blockers",
  query: "? release_blocker(h, why).",
  doc: "Open OQs and broken references gating the next release.",
  output_schema: { h: HandleId, why: String },
  default_args: {},
  capabilities: ["read"]
)
```

**Discovery facts are adapter-qualified** (`md.file_extension`,
`code.module_pattern`, etc.) so two adapters can't silently fight
over the same key. The runtime errors at load if a discovery fact
prefix names an adapter that isn't linked, unless the prefix is
declared optional:

```
md.file_extension(".md").                 # error if anneal-md not linked
optional code.module_pattern("**/*.rs").  # silently skipped if anneal-code absent
```

### §31 Steele's criterion for verbs [CR-R4]

**Rule CR-R4 (Verb extensibility).** A verb declared in `anneal.dl`
via `@verb(...)` is syntactically indistinguishable from a verb
shipped in the prelude. Identical:

- Discovery: `anneal verbs` lists both
- Help: `anneal describe <verb>` works for both
- Output envelope: same NDJSON shape, same `--explain` support, same
  declared `output_schema`
- Callable shape: a rule body references the verb's *underlying
  predicate* (verbs are predicates with a query body and declared
  output schema; not opaque saved strings)
- Documentation surface: `examples` work for both
- Capabilities: both honor the declared `capabilities` list

`@verb` is structured: `name` (snake-or-hyphen-case), `query`
(string, parsed at load to AST), `doc` (string), `output_schema`
(field name → type), `default_args` (argument bindings), and
`capabilities` (list of required ActorContext capabilities). The
runtime validates `query` against `output_schema` at load.

### §32 Discovery fact contract

Adapters declare their consumed discovery facts in `SourceInfo.config_keys`:

```rust
SourceInfo {
    name: "markdown",
    config_keys: vec![
        ConfigKey::required("md.file_extension"),
        ConfigKey::required("md.scan_root"),
        ConfigKey::optional("md.scan_exclude"),
        ConfigKey::optional("md.label_pattern"),
        ConfigKey::optional("md.linear_namespace"),
        ConfigKey::optional("md.version_pattern"),
    ],
    ...
}
```

The runtime errors at load if a corpus's `anneal.dl` declares a
required discovery fact whose prefix is recognized by no linked
adapter. The user fixes by linking the adapter, qualifying the fact
for a linked adapter, or marking the fact `optional`.

**Single-adapter sugar.** When exactly one adapter is linked, the
runtime auto-qualifies unqualified discovery facts to that adapter:

```dl
# In a markdown-only project, this is allowed:
file_extension(".md").              # auto-qualified to md.file_extension
label_pattern("OQ", "OQ-(\d+)", "any").

# In a multi-adapter project, the same line errors at load:
error: anneal.dl:4: ambiguous discovery fact 'file_extension'
       multiple linked adapters declare config keys with this name:
         - md.file_extension (anneal-md)
         - mdx.file_extension (anneal-mdx)
       resolve by qualifying explicitly (md.file_extension or mdx.file_extension)
```

This removes the ergonomic tax on single-adapter corpora (the
common case) while keeping the disambiguation guarantee on
multi-adapter corpora. The user discovers which mode applies via
`anneal sources` — listing one adapter means unqualified facts
work; listing more requires qualification.

### §32.1 Adapter diagnostic evidence [CR-D31]

**Definition CR-D31 (Diagnostic evidence facts).** Adapter observations
that are not handles, edges, content, or spans but are required to
reproduce diagnostics are stored as adapter-qualified `*meta` rows on
the nearest owning handle, usually the file handle. Keys MUST be
adapter-qualified (`md.*`, `code.*`, `host.*`) and their value encoding
MUST be documented by the adapter.

The v2.0 markdown adapter defines:

```
*meta{
  handle: <file handle>,
  key: "md.implausible_ref",
  value: JSON.stringify({
    value,   // raw frontmatter value rejected as a reference
    reason,  // "absolute path" | "wildcard pattern" |
             // "comma-separated list" | "freeform prose"
    line     // source line, null if unavailable
  }),
  ...
}
```

Rationale: W004 and similar parse-filter diagnostics must be
reconstructible from stored facts without re-running a format-specific
parser inside the substrate.

---

## Part VII: Surfaces [CR-Su]

### §33 The starter verbs

The prelude's `views.dl` ships these as saved verb declarations.
Projects override or extend any.

| Verb | Question | Underlying query (sketch) |
|---|---|---|
| `anneal` | where am I | composed of summary, work, advancing, blocked |
| `anneal H` | what is this handle | `*handle{id: H, ...}` + immediate edges |
| `anneal find TEXT` | identity-search by id substring | `*handle{id, ...}, id contains "TEXT"` |
| `anneal search TEXT` | content match by query | `search("TEXT", h, span_id, score, reason, field, low_confidence)` |
| `anneal context GOAL` | composition for cold-agent localization | see §33.1 |
| `anneal read H` | give me H's content, bounded | `read(H, budget, span_id, text, start, end, tokens)` |
| `anneal work` | where should I work | `(h, e) = TopK{k: 25, key: e : potential(h, e), entropy(h, src)}` |
| `anneal blocked H` | what's blocking H | `entropy("H", source), entropy_detail(...)` |
| `anneal trend` | corpus over time | `at(--at) { ... }` vs `at("now") { ... }` |
| `anneal broken` | are there errors | `diagnostic(_, "error", ...)` |

Plus self-description verbs from §11: `schema`, `predicates`, `verbs`,
`describe`, `source-of`, `examples`, `sources`.

Plus meta forms:

| Form | Purpose |
|---|---|
| `anneal -e '<q>'` | custom query; `-e -` reads from stdin |
| `anneal init` | scaffold a corpus with starter `anneal.toml` + `anneal.dl` |
| `anneal --prelude-path` | print the embedded-prelude inspection path |
| `anneal --inspect S` | parse-test a string against handle conventions |

### §33.1 The `context` verb [CR-D30]

**Definition CR-D30 (Context verb).** The `context` verb is the
load-bearing primitive for cold-agent localization. It composes
`search`, `read`, and `neighborhood` into a single budgeted call
that returns enough to make progress without a second tool call.

```
anneal context GOAL [--budget=N] [--neighborhood-depth=D] [--hits=K]
```

| Flag | Default | Meaning |
|---|---|---|
| `--budget=N` | `4000` tokens | total token budget across hits + spans + neighborhood |
| `--neighborhood-depth=D` | `1` | edges to traverse outward from the top hit |
| `--hits=K` | `3` | candidates to return (after `TopK` filtering) |

Underlying composition contract (from `views.dl`):

```dl
@verb(
  name: "context",
  query: "
    ? (h, span_id, score, reason, field) = TopK{ k: hits, key: score :
        search(goal, h, span_id, score, reason, field, low_confidence),
        low_confidence = false
      },
      per_hit_budget = budget * 0.6 / hits,
      (span_id, text, start_line, end_line, tokens) = TakeUntil{
        budget: per_hit_budget, sum: tokens :
        (span_id, text, start_line, end_line, tokens) :
          read(h, per_hit_budget, span_id, text, start_line, end_line, tokens)
      },
      neighborhood_or_self(h, neighborhood_depth, neighbor).
  ",
  output_schema: {
    goal: String,
    hits: List<{h, span_id, score, reason, field}>,
    spans: List<{h, span_id, start_line, end_line, tokens, text}>,
    neighborhood: List<{h, neighbor}>
  },
  capabilities: ["read"]
)
```

The `context` output is grouped by the verb surface from relational
rows into `hits`, `spans`, and `neighborhood`; grouping is an
`output_schema` concern, not record-style field access in the query
language. `views.dl` defines `neighborhood_or_self/3` so an otherwise
isolated top hit still returns its read span. Phase 1 must pin this
as an executable `views.dl` fixture before `context` is treated as a
shipped verb.

Budget allocation: 60% to span reads on top hits, 20% to
neighborhood expansion, 20% reserved for `--explain` if requested.
The runtime adjusts allocations downward when the requested K hits
or D-depth neighborhood don't exist; it never overruns.

Cold-agent test (§49 large-corpus fixture) targets a single `context`
call after an optional `anneal` dashboard — total tool calls ≤2,
counted including any required follow-ups.

### §34 Query echo behavior [CR-D24]

**Definition CR-D24 (Query echo).** When a verb runs, the runtime
prints the underlying query above the result *on stderr*, not on
stdout. Stdout stays pure NDJSON. Optionally `--meta` adds a single
NDJSON envelope record on stdout containing the underlying query
and runtime info:

```
$ anneal blocked OQ-37 --meta
{"_meta": {"verb": "blocked", "query": "? entropy(\"OQ-37\", src), …", "prelude_hash": "…", …}}
{"src": "undischarged", "detail": "namespace OQ open 82 days"}
{"src": "stale_dep", "detail": "depends_on .design/synth/discharge.md (superseded)"}
```

Without `--meta`, stdout is bare rows; the query is echoed only on
stderr. Agents consuming stdout via pipe (`jq`, `xargs`) never see
the echo.

### §35 CLI flags

Most flags are operational. Any flag that changes result policy is
called out explicitly and mirrored by config or query predicates;
filters still belong in queries.

| Flag | Effect | Scope |
|---|---|---|
| `--root=PATH` | operate on a different corpus | global |
| `--at=<ref>` | evaluate at a historical reference | global |
| `--limit=N` | cap output records | global |
| `--explain` | include `_derivation` per record | global |
| `--explain-depth=N` | derivation expansion depth (default 5) | global |
| `--meta` | emit `_meta` envelope record on stdout | global |
| `--no-snapshot` | don't append history on this run | global |
| `--quiet` | suppress stderr chatter | global |
| `--budget=N` | token budget for `work` / `read` / `context` | verb-specific |
| `--gate` | exit 1 if any results | `broken` |
| `--source=NAME` | restrict ingestion to one Source | global |
| `--mcp` | start as MCP server on stdin/stdout | global |
| `--color=auto` | TTY detect; pipes get plain text | global |
| `--pretty` | human-readable formatted JSON (breaks NDJSON contract) | global |
| `--include-low-confidence` | include hits with `low_confidence: true` in `TopK` | global, search-relevant |

### §36 I/O contract [CR-D25]

**Definition CR-D25 (I/O contract).**

- **stdout: pure NDJSON.** Bare record stream; `--meta` adds one
  envelope record at the top. Pipe to `jq` for human-readable
  formatting: `anneal | jq` is the canonical pretty-print path. The
  `--pretty` flag is also available for in-process formatting; it
  emits multi-line JSON and breaks the NDJSON contract, so it is
  human-only and never used in agent pipelines or `--mcp` mode.
- **stderr: human text.** Verb-banner echo, progress, warnings,
  parse errors. Never NDJSON.
- **stdin: `-` means stdin.** `anneal blocked -` reads handles, one
  per line. `anneal -e -` reads a query (heredoc-friendly).
- **Exit codes:** 0 success (including empty results), 1 query
  error, 2 invocation error, 3 gate failure.

### §37 MCP surface [CR-D26]

**Definition CR-D26 (MCP transport).** `anneal --mcp` (or the
`anneal-mcp` binary) starts a stdio MCP server. The tool surface is
**not 1:1 with verbs.** Tool inflation is a real failure mode; v2.0
ships a small stable surface that scales by introspection, not by
verb count.

Default MCP tool surface:

| Tool | Wraps | Capabilities |
|---|---|---|
| `eval` | `-e '<query>'` | gated by `eval` capability; default-denied for MCP unless `[mcp] allow_eval = true` or host policy grants it |
| `search` | `search` primitive | always allowed |
| `read` | `read` primitive (budgeted) | always allowed |
| `verbs` | `verbs` primitive | always allowed; agents see all available verbs |
| `describe` | `describe` primitive | always allowed |
| `schema` | `schema` primitive | always allowed |
| `source_of` | `source_of` primitive | always allowed |
| `dashboard` | the `anneal` verb | always allowed |
| `run_verb` | invoke any verb by name | gated by per-verb declared capabilities |

`read_full` is **not** a default MCP tool. Projects may expose it
via explicit configuration in `anneal.toml` `[mcp]` if they accept
the data-exfiltration risk.

`run_verb` is the agent's entry to project-defined verbs without
flooding the top-level tool list. `tools/list` returns the ~10 tools
above; the agent discovers project verbs via `verbs` then calls them
via `run_verb`.

Server instructions include the standard-library prelude content
under a *trusted prelude* heading, separated from any *untrusted
corpus content* an agent might subsequently see via `search` or
`read`. Project `@verb` docs are quoted as data, not promoted into
instruction frames.

### §38 Plugin surfaces [CR-D27]

**Definition CR-D27 (Pluggable extension seams).** Beyond `Source`,
the runtime exposes three extension surfaces:

```rust
pub trait Ranker {
    fn calibrate(&self, hit: &SearchHit, ctx: &RankingContext) -> f32;
    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering;
}

pub trait Policy {
    fn check(&self, actor: &ActorContext, action: &Action) -> PolicyDecision;
}

pub trait TrailSummarizer {
    fn summarize(&self, entry: &TrailEntryInProgress, ctx: &TrailContext) -> TrailSummary;
    fn redact(&self, expr: &str, ctx: &TrailContext) -> String;
}
```

Default impls ship in `anneal-core`. Adapters override `Ranker` for
their hit shapes; hosts override `Policy` for actor-based
authorization; projects override `TrailSummarizer` to control what
gets recorded.

---

## Part VIII: Onboarding [CR-O]

### §39 Lattice-on default [CR-D28]

**Definition CR-D28 (Init defaults).** `anneal init` always scaffolds
a minimal lattice and a starter `anneal.dl` referencing the
prelude's convergence vocabulary:

```
$ anneal init

scanning corpus...
  found 47 markdown files
  inferred Source: anneal-md (linked)
  status frontmatter: present in 41/47 (87%)
  inferred lattice: raw → draft → current → stable

wrote anneal.toml
  [convergence]
  ordering = ["raw", "draft", "current", "stable"]
  active = ["draft", "current", "stable"]
  terminal = ["superseded", "archived"]

wrote anneal.dl (16 lines)
  # === discovery ===
  md.file_extension(".md").
  …

next steps:
  anneal                       see the landscape
  anneal source-of convergence read what convergence means here
  anneal work                  pick where to work
```

The agent's first session lands in convergence mode, not graph mode.
Graph mode (lattice-off) requires `[convergence] disabled = true`
in `anneal.toml`.

### §40 The agent loop [CR-D29]

**Definition CR-D29 (Agent loop).**

```
1. anneal                  see the landscape
2. anneal work             pick where to work
3. anneal blocked H        understand why H isn't moving
4. (do the work)
5. anneal trend            confirm potential dissipated
```

For arrival on an unfamiliar corpus, prepend:

```
0a. anneal sources         what adapters are loaded
0b. anneal source-of convergence  what convergence means here
```

For multi-session handoff, prepend:

```
0c. anneal -e '? *trail{session_id: last, step, redacted_expr, consumed_refs}.'
```

---

## Part IX: Handle model [CR-H]

### §41 Kinds

Five handle kinds are substrate-shaped:

| Kind | Examples by Source |
|---|---|
| `file` | markdown file (anneal-md), MDX file (anneal-mdx), Rust module (anneal-code), Ash resource (anneal-host) |
| `section` | markdown heading (anneal-md), Rust impl block, Phoenix scope |
| `label` | OQ-22 (anneal-md frontmatter), RFC-101 (anneal-code attribute), GitHub issue #42 (anneal-issues) |
| `version` | versioned spec (`formal-model-v17.md`), semver-tagged release |
| `external` | URL, external API reference, dependency |

The Source decides the mapping. Display id is locally unique within
`(corpus, source)`; internal identity is
`(corpus, source, kind, native_id)`.

### §42 Discovery configuration

Adapter-qualified discovery facts per §32. The markdown adapter's
shape:

```
md.file_extension(".md").
md.scan_root(".").
md.scan_exclude("node_modules").
md.label_pattern("OQ",    "OQ-(\d+)",    "any").
md.label_pattern("KB-D",  "KB-D(\d+)",   ".design/**").
md.linear_namespace("OQ").
md.version_pattern("formal-model", "formal-model-v(\d+)\.md").
md.section_min_depth(1).
md.section_max_depth(3).
```

Other adapters declare their own (`code.module_pattern`,
`issues.repo`, etc.). The runtime errors if a required fact's prefix
isn't recognized by a linked Source; `optional` facts are skipped
when the adapter is absent.

### §43 Introspection

```
# 1. Counts by kind
anneal -e '? c = Count{ h : *handle{id: h, kind: k} }, k.'

# 2. Label namespaces and counts
anneal -e '? c = Count{ h : *handle{id: h, kind: "label", namespace: ns} }, ns.'

# 3. The corpus's discovery conventions (adapter-qualified)
anneal -e '? md.label_pattern(ns, regex, scope).'

# 4. Inspect a specific string
anneal --inspect "OQ-99"

# 5. Read directly
cat anneal.dl
anneal describe handles
```

---

## Part X: Files and layout [CR-FL]

### §44 Project files

```
<corpus>/
  anneal.toml           # engine config: lattice, [convergence], [ranking], [mcp], [policy]
  anneal.dl             # discovery facts + project predicates + verbs + overrides
  .anneal/
    history.jsonl       # snapshot append log
    trails/             # session paths
      <session-id>.jsonl
    generations/        # generation tracking for retraction
      <source>.json     # current generation per source
```

### §45 Substrate files (embedded)

```
anneal-core/src/prelude/
  graph.dl
  convergence.dl
  checks.dl
  ranking.dl
  views.dl
```

Compile-time embedded; `ANNEAL_PRELUDE_PATH` overrides (and changes
the recorded `prelude_hash`).

---

## Part XI: Migration from v1.x [CR-M]

### §46 Command mapping

Every v1.x command is reachable in v2.0:

| v1.x | v2.0 |
|---|---|
| `anneal status` | `anneal` |
| `anneal get H` | `anneal H` |
| `anneal find TEXT` | `anneal find TEXT` (identity search; unchanged) |
| (new) | `anneal search TEXT` (content retrieval) |
| (new) | `anneal context GOAL` (search + read in one verb) |
| `anneal check` | `anneal broken` or `anneal -e '? diagnostic(c, s, ...).'` |
| `anneal check --errors-only` | `anneal broken --gate` |
| `anneal map --around=H` | `anneal -e '? neighborhood("H", 2, x).'` |
| `anneal impact H` | `anneal -e '? impact("H", x, depth).'` |
| `anneal obligations` | `anneal -e '? obligation(h), disposition(h).'` |
| `anneal diff` | `anneal trend` |
| `anneal areas` | `anneal -e '? area_health(area, grade, ...).'` |
| `anneal orient` | `anneal work` |
| `anneal garden` | `anneal -e '? maintenance_task(t, category, blast).'` |
| `anneal init` | `anneal init` (now lattice-on by default) |
| `anneal prime` | `anneal describe runtime` |

### §47 Migration path

1. **`anneal-core`.** Datalog runtime, primitives, IR, embedded
   prelude.
2. **`anneal-md`.** Refactor v1.x parse pipeline behind `Source`;
   while parity is being proven, shared v1 parser/config behavior may
   live in `anneal-legacy` as a transition-only library boundary
   instead of the root CLI package.
3. **`anneal-cli` + `anneal-mcp`.** Surfaces over the shared core.
4. **Dual ship.** v1.x and v2.0 binaries in parallel for one minor
   release; v1.x prints deprecation warnings.
5. **Documentation.** SKILL.md, README.md rewritten.

### §48 What stays unchanged

Core model from `anneal-spec.md` Parts I-III: handle definition,
graph construction, convergence lattice, local check semantics,
linearity, impact analysis, convergence tracking, design
principles, theoretical lineage.

---

## Part XII: Acceptance [CR-Acc]

### §49 Workflow-completion gates [CR-R5]

**Rule CR-R5 (Workflow gates).** v2.0 ships when these pinned
cold-agent fixtures pass:

**Fixture: large-corpus/v17-conformance-audit**

- *Corpus*: `/path/to/large-corpus/.design/` at git ref `v17-audit-fixture`
  (frozen for reproducibility)
- *Goal*: "Find the most urgent thing blocking v17 conformance and
  read enough context to make progress."
- *Pass criteria*:
  - Top-3 `search` result for query `"v17 conformance audit"` includes
    `reviews/2026-04-28-formal-model-v17-conformance-audit.md` with
    `score > 0.7` and `reason` in
    `{"title-match", "frontmatter-key-match"}`
  - Following `read` on that handle returns the file's `## Method`
    or `## Summary` section in first span
  - `--explain` shows the rank derivation citing both score and
    field
- *Tool-call target*: 2 calls (search + read) with `MRR ≥ 0.5`
  across cold-agent runs
- *Context target*: `anneal context "v17 conformance audit"` after
  an optional `anneal` dashboard returns the same audit handle and
  a useful first span in ≤2 total calls

**Fixture: anneal/release-blocker**

- *Corpus*: `/path/to/anneal/.design/` at the v2.0-release-fixture tag
- *Goal*: "What's blocking the v2.0 release?"
- *Pass criteria*: `anneal -e '? release_blocker(h, why).'` returns
  at least one row whose `why` is grounded in a derived diagnostic
  visible via `--explain`

**Fixture: host-corpus-host/runtime-snapshot** (v2.1+ when
`anneal-host` lands; v2.0 declares the fixture shape only)

- *Corpus*: a host-corpus instance with `anneal-host` adapter linked
- *Goal*: "Which Ash actions have no test coverage in `accounts`?"
- *Pass criteria*: same workflow gates apply

Additional workflow targets:

| Workflow | Target |
|---|---|
| "What's the corpus state?" | 1 call (`anneal`) |
| "Where is X?" | 2 calls (`search` + `read`) |
| "What does X depend on?" | 2 calls (`anneal H` or `-e upstream`) |
| "What changed in 7 days?" | 1 call (`anneal trend --at=--7days`) |
| "Why is this fact here?" | 1 call (`--explain`) |
| "Extend the vocabulary" | Write 5 lines in `anneal.dl`; verb available next invocation |
| "Recover what a prior agent did" | 1 call (`anneal -e '? *trail{...}.'`) |

### §50 Substrate validation

MVS-1..9 from the engine-spike protocol validate the substrate's
ability to execute the rule layer. Workflow gates above validate the
product. Both must hold.

### §51 Performance gates

Per SP-R1 of `2026-05-07-engine-spike-and-parity-protocol.md`:

| Sub-criterion | Target |
|---|---|
| Cold full evaluation on large-corpus | <2s |
| Warm full evaluation | <200ms |
| Snapshot `at()` | <500ms |
| Git-ref `at()` | <5× snapshot cost |
| Resident memory | <200MB |
| Dependency unsafe | audited, contained, or `unsafe_code = deny` |

---

## Part XIII: Forward-looking [CR-Fw]

### §52 Trail-driven workflows (v2.1)

v2.0 captures trails (§13). v2.1 adds:

- `anneal trail replay <session-id>` — re-runs the path against the
  current corpus state, surfacing what's changed
- `anneal trail diff <a> <b>` — compares two sessions
- `anneal trail summarize <session-id>` — markdown digest for
  inclusion in commit messages or PRs

### §53 Multi-corpus federation (v2.2)

UI for queries across corpora. The *schema* is in v2.0 (every fact
has `corpus`, `source`, `origin_uri`); the *surface* is v2.2.

```
anneal --root .design --root /path/to/host-corpus/.design --root /path/to/large-corpus/.design \
       -e '? *handle{id: h, corpus: c}, c != "self".'
```

### §54 Adapters beyond markdown (v2.1+)

- **anneal-mdx**: MDX with JSX-island parsing
- **anneal-code**: Rust/Elixir/TS source — handles as functions/types
- **anneal-issues**: GitHub/Linear — issues as handles
- **anneal-host**: library API for embedding (host-corpus)

### §55 Host Corpus embedding (v2.1+)

Concrete use case for `anneal-host`: host-corpus embeds `anneal-core` as
a Rust dep alongside its Elixir runtime. Host Corpus-side adapter exposes
Ash resources, Phoenix routes, Oban jobs, decision-log entries, and
customer-state transitions as handles. The same agent skill that
runs in large-corpus's `.design/` runs inside host-corpus.

---

## Part XIV: Open questions [CR-OQ]

### §56 take_until aggregation behavior

§20 declares `TakeUntil` but its exact semantics on non-numeric
contributing variables and on tied threshold-crossings need pinning
during Phase 1.

### §57 Cross-adapter score calibration in the default `Ranker`

§12 defines per-adapter score range, reason vocabulary, and
confidence threshold. The default `Ranker`'s calibration formula
across adapters is a Phase 1 design question. Probably:
linear-rescale by adapter quartiles + bonus for matched fields
named in the user query + low-confidence filter applied last.

### §58 Default trail summarizer redaction patterns

§13 says default summarizer redacts values in string literals and
meta-key values matching configured patterns. The default pattern
set (`secret`, `password`, etc.) needs review; probably project-
overridable via `[trail]` in `anneal.toml`.

### §59 Distinguishing consumed-by-read from consumed-by-display

§13 distinguishes `surfaced_refs` from `consumed_refs`. The runtime
infers `consumed_refs` from observed verb-to-verb dataflow. Two
edge cases: an agent that reads then never uses the content (the
read is consumption-of-attention but not consumption-of-output);
an agent that bulk-surfaces and silently drops most. Default heuristic
TBD: lean toward `consumed = handles that appeared in a subsequent
verb's input within the same session`.

### §60 MCP run_verb routing

§37's `run_verb` dispatches by verb name. Naming collisions across
shadowed verbs (prelude vs project) need a documented resolution:
project verbs win over prelude verbs (per CR-R4), but MCP must
expose only the resolved name, not both.

### §61 Performance ceiling

For corpora with hundreds of thousands of handles and rich rule
sets, evaluation time grows. The substrate is designed for
hundreds-of-thousands; tens-of-millions probably needs indexed
evaluation. Out of scope for v2.0.

### §62 Context verb executable contract

§33.1 defines the `context` verb as a grouped, budgeted composition
over `search`, `read`, and `neighborhood_or_self`. Phase 1 must pin
the exact executable `views.dl` form and the row-to-group
`output_schema` behavior in tests before `context` becomes a shipped
verb. This is a contract question, not a UX polish item, because the
cold-agent gate depends on it.

### §63 Ordered config fact representation

§10 models runtime configuration as `*config{key, value, corpus}`.
That is sufficient for scalar settings and unordered sets, but
`[convergence] ordering = [...]` is list-valued and the current
relational shape relies on fact insertion order to preserve ordinals.
Phase 3 must decide whether ordered config uses an explicit ordinal
field, a separate relation, or a value encoding with a stable
round-trip contract before persisted/federated config facts ship.

---

## Part XV: Labels [CR-Labels]

### CR-F (Framing)
- CR-F1: §1 What agents need
- CR-F2: §2 Why substrate

### CR-D (Definitions)
- CR-D1: Substrate (§2)
- CR-D2: Cold-agent test (§3)
- CR-D3: Layering (§4)
- CR-D4: Source trait (§5)
- CR-D5: Plugin surfaces (§6)
- CR-D6: Ingestion lifecycle (§7)
- CR-D7: Identity (§9)
- CR-D8: Stored primitives (§10)
- CR-D9: Function primitives (§11)
- CR-D10: Search scoring contract (§12)
- CR-D11: Trail (§13)
- CR-D12: Provenance (§14)
- CR-D13: `at(<ref>)` block (§15)
- CR-D14: Capability gating (§16)
- CR-D15: Stored vs derived (§19)
- CR-D16: Derived (§19)
- CR-D17: Aggregation (§20)
- CR-D18: Stratified negation (§21)
- CR-D19: Inline rules, includes, imports (§22)
- CR-D20: Output contract (§23)
- CR-D21: Load order (§26)
- CR-D22: Convergence vocabulary (§27)
- CR-D23: Check rule (§28)
- CR-D24: Query echo behavior (§34)
- CR-D25: I/O contract (§36)
- CR-D26: MCP transport (§37)
- CR-D27: Plugin surfaces (§38)
- CR-D28: Init defaults (§39)
- CR-D29: Agent loop (§40)
- CR-D30: Context verb (§33.1)
- CR-D31: Diagnostic evidence facts (§32.1)
- CR-D32: Transition-only legacy boundary (§8, §47)
- CR-D33: Aggregate result unification (§20)
- CR-D34: Empty-group origination (§20)
- CR-D35: Sealed engine primitives (§11)
- CR-D36: Soft lifecycle primitives (§11)
- CR-D37: Default scalar lifecycle metrics (§11)

### CR-R (Rules)
- CR-R1: Diagnostic ID literal (§29)
- CR-R2: Unique within ruleset (§29)
- CR-R3: Reserved prefixes (§29)
- CR-R4: Verb extensibility / Steele's criterion (§31)
- CR-R5: Workflow gates with pinned fixtures (§49)
- CR-R6: Edge closure (§10)

### CR-Su (Surfaces)
- CR-Su1: Starter verbs (§33)
- CR-Su2: Context verb (§33.1)
- CR-Su3: CLI flags (§35)
- CR-Su4: MCP surface (§37)

### CR-O (Onboarding)
- CR-O1: Lattice-on default (§39)
- CR-O2: Agent loop (§40)

### CR-A (Acceptance)
- CR-A1: Workflow-completion gates (§49)
- CR-A2: Performance gates (§51)

### CR-Fw (Forward-looking)
- CR-Fw1: Trail-driven workflows (§52)
- CR-Fw2: Multi-corpus federation surface (§53)
- CR-Fw3: Adapters beyond markdown (§54)
- CR-Fw4: Host Corpus embedding (§55)

### CR-OQ (Open questions)
- CR-OQ1: take_until semantics (§56)
- CR-OQ2: Cross-adapter Ranker calibration formula (§57)
- CR-OQ3: Default trail redaction patterns (§58)
- CR-OQ4: Consumed-by-read vs consumed-by-display heuristic (§59)
- CR-OQ5: MCP run_verb routing under shadowed names (§60)
- CR-OQ6: Performance ceiling (§61)
- CR-OQ7: Context verb executable contract (§62)
- CR-OQ8: Ordered config fact representation (§63)

---

## References

### Internal
- `anneal-spec.md` — the convergence model the standard library encodes
- `2026-05-07-engine-spike-and-parity-protocol.md` — engine validation protocol
- `2026-05-13-engine-spike-results.md` — engine-viability findings; architectural revision (ascent for primitives, dynamic IR for rules) carries forward

### External
- Cloudflare Code Mode — `https://blog.cloudflare.com/code-mode/` —
  programmability as the agent surface
- qmd — `https://github.com/jamesrisberg/qmd` — content as
  addressable spans
- Host Corpus eval design (internal) — runtime self-description; trail
  capture with privacy
- ascent — `https://github.com/s-arash/ascent` — engine candidate
  for fixed primitives layer
- Cozo — `https://github.com/cozodb/cozo` — modern Datalog;
  reference for `TakeUntil` aggregation
- Bush, "As We May Think" — trail-as-memex
- Naur, "Programming as Theory Building" — handoff via paths
- SWE-agent ACI — purpose-built affordances beat raw shell;
  summarized search beats iterative paging
