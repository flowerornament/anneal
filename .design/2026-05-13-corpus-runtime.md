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

- **A substrate**: a corpus runtime with a stratified Datalog rule layer,
  stored facts, function primitives, a convergence standard library,
  self-description, and provenance.
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
*Other* extension seams (source orchestration, retrieval,
ranking/scoring, authorization policy, trail privacy, MCP tool
registration) are separate plugin surfaces declared in Part VII.

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
    pub search: Option<SearchInfo>,        // search scoring contract if the source/provider contributes hits
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
    pub low_confidence_threshold: Option<f32>, // omitted => runtime default (CR-OQ9)
}
```

**Definition CR-D57 (Source driver boundary).** `Source` is a
bounded extraction interface, not a scheduler. File watching,
remote polling, retry policy, cache invalidation, debounce, and
long-running MCP/host refresh loops belong to a runtime-owned
`SourceDriver` or to the embedding host. A driver decides *when* to
call `Source::extract`; the `Source` decides *what facts* the current
context yields.

Rationale: extraction should be testable as a pure-ish snapshot
operation. Mixing orchestration into `Source` would make every
adapter invent its own refresh semantics and would couple markdown
directory walking, host runtime polling, and remote issue trackers
to one trait.

**Definition CR-D68 (Source refresh transaction).** `anneal-core`
owns the source refresh transaction boundary around a `SourceDriver`
invocation. The core refresh operation builds `SourceContext` from
the current store generation, calls the driver, validates the returned
`FactBatch` scope, and commits it through the runtime `FactStore`
merge API. `Source` and `SourceDriver` never write through a sink, and
`FactStore` remains the single owner of generation retraction and
atomic merge semantics.

Rationale: long-running surfaces need one stable operation to call
when a source changes, while adapters still need extraction to remain
snapshot-scoped and independently testable.

A `Source` is one of: a directory walker (markdown, MDX, AsciiDoc),
a source-code analyzer (anneal-code), an external-system reader
(GitHub issues, CI events), or a host application's runtime
introspector (anneal-host: host-corpus's Ash resources, Phoenix routes,
Oban jobs as handles).

The runtime is identical across sources. Only the extraction differs.

### §6 Other extension seams [CR-D5]

**Definition CR-D5 (Plugin surfaces).** v2.0 names extension seams
beyond `Source` so adapter authors don't contort everything into
fact-emission:

| Surface | Trait | Purpose |
|---|---|---|
| Data ingestion | `Source` | Emit handle/edge/meta/content facts for a snapshot |
| Source orchestration | `SourceDriver` | Watch, poll, debounce, retry, and schedule extraction |
| Content retrieval | `ContentProvider` | Resolve bounded `read`/`read_full` chunks by handle/span |
| Search candidates | `SearchProvider` | Produce raw `SearchHit` rows from facts or adapter indexes |
| Ranking and scoring | `Ranker` | Per-adapter `search` score calibration; tie-break policy |
| Authorization policy | `Policy` | Actor → allow/deny on read/search/eval; scoped to MCP and host-embed |
| Trail capture/privacy | `TrailRecorder`, `TrailRedactor`, `TrailSummarizer`, `TrailStore` | Capture, redact, summarize, retain, and replay trail entries |

Default implementations ship in `anneal-core`. Adapters and hosts
override the narrow surface they own; the runtime composes the
most-specific implementations.

**Definition CR-D52 (Retrieval provider boundary).**
`ContentProvider` and `SearchProvider` are distinct from `Source`.
`Source` emits durable facts. `ContentProvider` retrieves bounded
content for `read` and `read_full`. `SearchProvider` emits raw
candidate hits for `search`. `Ranker` calibrates and orders those
hits; it does not fetch content.

The default providers are fact-store backed: `ContentProvider` reads
`*content` and `*span`; `SearchProvider` scans handle ids,
summaries, meta fields, and content spans. Large or host-backed
adapters may provide lazy content retrieval or indexed search without
changing the public `search(...)` and `read(...)` relation shapes.
Providers still emit enough provenance for `source_of`, trails, and
`--explain` to name the underlying handle/span/source.

`match(pattern, handle, line, snippet)` remains substrate-owned in
v0.11.0. It performs a bounded regex scan over the already-authorized
stored content for a bound handle and is policy-gated as `Action::Match`.
It is not a third provider surface yet, because adapter-native regex
search would need a separate budget, provenance, and capability story.
Future indexed or host-native regex matching is tracked as an open
provider question, not silently hidden inside `SearchProvider`.

Rationale: markdown can eagerly load content, but code indexes, issue
trackers, and host runtimes often cannot. Retrieval is an access-path
and performance decision; relation shape is the logical contract.

### §7 Ingestion lifecycle [CR-D6]

**Definition CR-D6 (Ingestion lifecycle).** Each invocation of the
runtime executes these phases in order:

```
1. Adapter registration: link Source impls into the binary.
2. Phase A — project parse:
   a. Parse anneal.dl, splitting static declarations from rule-layer
      statements:
      - `config` blocks for runtime config rows
      - `source` blocks for adapter-qualified discovery facts
      - rule clauses (Horn rules with `:=`)
      - verb declarations (@verb annotations)
   b. Existing anneal.toml files are upgrade input for conversion, not
      a second runtime config authority.
3. Phase B — Source extraction:
   a. For each enabled Source, build SourceContext from config and discovery facts.
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
│   ├── anneal-lang/             # private v2.0 language syntax library
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

### §8.1 Embeddable language boundary [CR-D51, CR-R9]

**Definition CR-D51 (Embeddable language boundary).**
`anneal-lang` owns the user-facing Datalog dialect as a lower-level
library boundary inside the substrate: lexer/parser, AST, source
locations, parse/load diagnostics, syntax-level `@verb` and `@doc`
metadata, and host-neutral include/import resolution. It is designed
so a future consumer can parse or inspect `anneal.dl` without linking
the full runtime.

`anneal-lang` must not depend on `anneal-core`, `Source`, `FactStore`,
adapters, search/read/rank primitives, trail capture, generation
tracking, or any concrete evaluation engine. Runtime-aware analysis
belongs in `anneal-core`: primitive signatures, stored-relation
schemas, capability checks, rule planning, fixpoint evaluation, and
adapter/runtime facts. If syntax-level analysis needs relation
signatures, it receives them through a narrow provider trait rather
than by depending on the runtime store.

In v2.0, `anneal-lang` is an internal crate boundary, not a stabilized
public package. `anneal-core` consumes it; surfaces and adapters
should reach language behavior through `anneal-core` unless they have
a parser-only need.

**Rule CR-R9 (Language API stabilization gate).** `anneal-lang` stays
`0.x` and `publish = false` until:

1. `@verb`, include/import, aggregation, and diagnostic-span semantics
   are pinned by this spec and parity fixtures.
2. At least one true parser-only consumer exists: either an external
   workspace crate uses `anneal-lang` without `anneal-core`, or a
   non-CLI/non-MCP crate inside this workspace uses `anneal-lang`
   independently of runtime introspection.
3. The public API hides representation choices with non-exhaustive
   enums, constructors/accessors, or equivalent compatibility guards.

Rationale: the language must be embeddable without bundling a
mandatory runtime, but observable parser and AST quirks become
permanent API commitments once external users depend on them. The
private crate boundary gives the architecture a clean lower layer now
while deferring public stability until real consumers force the right
interface.

**Definition CR-D74 (Rule layer/runtime substrate boundary).**
Engine choice is internal to `anneal-core`. v2.0 uses
[`ascent`](https://github.com/s-arash/ascent) for fixed
engine-derived primitive support and a dynamic IR for the rule layer
(prelude + project + inline). The rule layer is a stratified Datalog
fragment with aggregation; the substrate is a corpus runtime with
I/O-bearing primitives, actor-scoped fact visibility, capability and
policy gates, trail capture, source/provider traits, and runtime
self-description. The rule syntax is portable. The runtime semantics
are not a generic Datalog-engine contract.

The grammar in Part IV (Steele's criterion for project verbs, `@verb`
declarations, adapter-qualified discovery facts, `context` as a
composition primitive) is designed for agents reasoning about corpora,
not for Rust developers embedding a fact engine; it deliberately does
*not* mirror ascent's surface syntax. This is a load-bearing
invariant: swapping engines for performance, non-Rust embedding, or
incremental evaluation is allowed only behind `anneal-core` because
the user-facing language and stored-relation schema are independent of
the engine choice. The IR's internal AST may stay close to ascent's
shape so a primitives-lowering pass is thin, but that is an
implementation detail of `anneal-core`, not a public contract.

---

## Part III: Substrate primitives [CR-P]

### §9 Identity model [CR-D7]

**Definition CR-D7 (Identity).** Every fact carries enough origin to
distinguish it across corpora, sources, and adapter combinations.
Internal identity is `(corpus, source, kind, native_id)`; handle id is
the stable, user-facing query identity the adapter chose.

This applies from v2.0, not v2.2, because adding fields later forces
a query-breaking schema migration. Federation UI can defer; the
schema cannot.

### §10 Stored relations [CR-D8]

**Definition CR-D8 (Stored primitives).** The relations every adapter
populates and every rule may join on.

```
*handle{
  id,           // stable query id, unique within corpus
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

*config{key, value, ordinal, corpus}      // from anneal.dl config blocks; runtime-populated
*snapshot{snapshot, at, id, key, value, corpus}
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
| `*config` | Runtime configuration as queryable facts (lattice, namespaces); `ordinal` is null except for ordered list entries |
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

**Rule CR-R11 (Stored field validation).** A stored atom whose relation
is one of the CR-D8 runtime relations must use only that relation's
declared field names. Unknown fields are static-analysis errors with a
source location and the expected field set. Relations not declared by
CR-D8 remain outside this validation rule so local test fixtures and
future adapter-internal rule experiments can still use private stored
relations until they are promoted to runtime primitives.

Rationale: a typo such as `*handle{namspace: ns}` is a malformed query,
not a legitimate zero-row result. Cold agents need that distinction to
recover without guessing whether the corpus was empty.

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

**Rule CR-R7 (Bounded graph primitive anchors).** Graph traversal
primitives must have at least one endpoint argument bound by a
literal or by a positive atom outside the primitive call. `upstream`
and `downstream` require `h` or `anc`/`desc`; `impact` requires `h`
or `x`; `neighborhood` requires `h` or `member`. `depth` narrows a
bound traversal but is not itself an endpoint anchor. Fully unbound
all-pairs traversal is rejected at analysis time. Rationale:
unanchored traversal is a physical execution strategy disguised as a
relation; agents need explicit bounded entry points rather than
silent corpus-wide graph expansion.

**Rule CR-R8 (Bounded content primitive inputs).** Content access
primitives require their control arguments to be ground by a literal
or by a positive atom outside the primitive call. `search` requires
`query`; `read` requires `handle` and `budget`; `read_full` requires
`handle` and the `read_full` capability; `match` requires `pattern`
and `handle`.
`read` treats
`span_id` as an optional narrowing constraint and emits spans in
`start_line`, `span_id` order while the cumulative `tokens` remain
within `budget`. `match` scans only the bound handle's stored content
spans; corpus-wide regex search belongs in `search`/ranking or behind
an explicit future budgeted primitive. Rationale: content access is
the substrate's context-loading valve; agents need predictable bounded
reads instead of accidental full-corpus dumps.

**Definition CR-D35 (Sealed engine primitives).** Substrate-only
engine primitive predicate names in CR-D9 are sealed unless CR-D36
marks them soft lifecycle primitives. Prelude, project, import,
inline, and fact clauses may call sealed primitives but must not
define, shadow, or union with them. Projects that need
domain-specific variants wrap sealed primitives in separately named
derived predicates. Rationale: sealed primitive semantics are part of
the engine-replaceability contract; letting corpus rules redefine them
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

**Definition CR-D75 (Primitive lifecycle classes).** CR-D9 primitives
are classified by lifecycle, not by call syntax:

| Class | Predicates | Shadowing |
|---|---|---|
| Sealed substrate primitives | graph traversal, counts/metrics, content retrieval, `search`, `read`, `read_full`, `match`, self-description | cannot be defined, shadowed, or unioned by loaded rules |
| Soft lifecycle defaults | `terminal`, `active`, `settled`, `pipeline_position`, `pipeline_position_for`, `obligation`, `discharged`, `undischarged` | replaced by an unqualified rule definition per CR-D36 |
| Future structural primitives | none in v0.11.0 | CR-Fw5 may add read-only, no-I/O adapter structural primitives |

Every `schema(...)` row that describes an engine primitive must expose
its lifecycle class through the machine-readable `kind` field
specified in CR-D81. All CR-D9 primitives not listed as soft are
sealed unless a later CR-D* explicitly changes their lifecycle class.

**Definition CR-D76 (Adapter primitive extension boundary).**
Adapters cannot add new engine primitive predicates in v0.11.0. They
extend the runtime through stored facts, `ContentProvider`,
`SearchProvider`, `Ranker`, `Policy`, `Trail*` components, and derived
rules. Rationale: arbitrary adapter primitives would turn provider
registration into a runtime ABI before capability, provenance, and
engine-replacement rules exist. Adapter-native structural primitives
are a forward-looking escape hatch, not current behavior.

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

The aggregation form
`TopK{k: N, key: score : (h, score) : body}` (Part IV §17)
provides bounded selection. There is no parallel `top_k` function
primitive and no surface syntax shortcut — one mechanism, one
place.

### §12 Search scoring contract [CR-D10]

**Definition CR-D10 (Search contract).** Every linked
`SearchProvider` that contributes search results emits raw
`SearchHit` candidates. The runtime passes candidates through the
active `Ranker` before exposing the public `search(...)` relation, so
the `score` column agents see is calibrated for the loaded adapter
set.

- Public `score` is a calibrated float in `[0.0, 1.0]`. 1.0 means
  "strongest match after the active Ranker"; 0.0 means "no signal."
- Provider raw scores are **not** directly comparable. Provenance may
  expose both `raw_score` and calibrated `score`; ordinary queries
  see only calibrated `score`. The default `Ranker` ships with
  documented calibration; projects override by supplying project rules
  or by using surface flags until ranking config declarations are
  explicitly designed.
- `reason` is a short string explaining the match
  (`"title-substring"`, `"semantic-cluster"`,
  `"frontmatter-key-match"`). Adapters document their reason
  vocabulary in `SourceInfo`.
- `field` names which logical field of the handle matched (`"title"`,
  `"body"`, `"frontmatter:status"`, `"identifier"`). Used by agents
  to decide whether a hit is structural or content-based.
- Ordering by `score` is deterministic given a fixed corpus state and
  query; tie-breakers documented per `Ranker`.
- **Confidence threshold.** A search provider may declare a
  `low_confidence_threshold` through the source or provider
  registration; omission uses the runtime default tracked by CR-OQ9.
  Hits with calibrated
  `score < threshold` carry
  `low_confidence: true` in the relation, signalling agents that
  the hit is plausible but uncertain. The relation shape:

  ```
  search(query, handle, span_id, score, reason, field, low_confidence)
  ```

  Agents reading raw rows always see all hits with their confidence
  flag; agents consuming surfaced `TopK` search templates get only
  high-confidence hits by default, eliminating the "0.62 hit looks
  comparable to 0.93 hit" failure mode the live workflow simulation
  surfaced.

**Definition CR-D42 (Default lexical Ranker).** The v0.11.0 default
`Ranker` is deterministic and lexical. It emits internal `SearchHit`
candidates from handle identifiers, handle summaries as `title`,
frontmatter-style handle/meta fields, and content spans as `body`.
Raw adapter or field scores are internal. Public `score` is the
active ranker's calibrated score clamped to `[0.0, 1.0]`; the default
ranker multiplies lexical match quality by field weights, clamps the
result, and then orders by descending calibrated score, `source`,
reason priority, `handle`, `span_id`, `field`, and `reason`. Scores
below the active low-confidence threshold set `low_confidence: true`.
The lexical match quality includes light deterministic stemming and a
small built-in abbreviation table for corpus-planning idioms such as
`OQ` ↔ `open question`, `ADR` ↔ `architecture decision record`, and
`RFC` ↔ `request for comments`; this is still lexical expansion, not
semantic retrieval. The default calibration may apply small
deterministic authority adjustments from corpus structure, including
inbound-reference boosts for highly cited handles and penalties for
historical/prior-path handles, so tied lexical matches prefer current
canonical sources. Additional domain synonyms belong in a custom
`SearchProvider` or `Ranker`.
The exact built-in weight values and default threshold are shipped
policy defaults, not semantic contracts; CR-OQ8 and CR-OQ9 track their
future tuning.

**Definition CR-D73 (Clustered child-hit parent promotion).** When the
default lexical index finds matches on two or more child handles whose
stored `file` field names the same canonical parent handle, and that
parent handle exists in the same corpus/source, it emits an additional
handle-level `SearchHit` for the parent with reason `parent-cluster`
and field `identifier`. The cluster hit uses the best child raw score
plus a small deterministic boost, clamped to `[0.0, 1.0]`; at equal
calibrated scores, `parent-cluster` sorts before ordinary lexical
reasons. If the parent already has an equal-or-stronger direct
identifier/title hit by raw lexical match quality, or an
equal-or-stronger direct calibrated score, no extra cluster hit is
emitted. Single child hits do not synthesize a parent.
Rationale: agents asking orientation questions need canonical source
documents when many local labels or sections point to the same source,
not a ranked list of fragments that hides the document to read.

**Definition CR-D43 (Search selection policy).** The raw
`search(...)` relation emits every calibrated hit with its
`low_confidence` flag. Surfaced search result sets that use `TopK`
(`anneal search`, `anneal context`, and prelude search templates)
filter with `low_confidence = false` before `TopK` by default.
`--include-low-confidence` removes that predicate; a project-level
config default remains CR-OQ9. This is an
ordinary query/surface policy, not special aggregator behavior, so
custom Datalog can inspect low-confidence rows directly and opt into
them explicitly.

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
  prelude_hash,       // hash of loaded prelude; supports reproducibility
  visibility,         // "public" | "team" | "private" — policy-derived
  retention,          // ISO duration; runtime garbage-collects past expiry
}

*trail_ref{
  session_id, step, kind, ordinal, corpus, source, handle, span_id, score
}

*trail_generation{
  session_id, step, corpus, source, generation
}
```

**Surfaced vs consumed.** `surfaced_refs` is everything the verb's
output stream contained. `consumed_refs` is the subset the agent
*acted on* — handles passed to a subsequent `read`, handles
referenced in a later query, handles selected via `run_verb`
follow-up. The runtime infers `consumed_refs` from observed
verb-to-verb dataflow within the session; explicit selection is
also possible via the `TrailRecorder.note_consumed(handle)`
callback from a host application.

This distinction matters for trail replay (v2.1): a replay agent
re-executes consumed paths, not every surfaced candidate. The
output-explosion failure mode the live workflow simulation
surfaced — context verb surfaces 6 hits + 4 spans + 2 edges, agent
uses 1 — is resolved by recording both sets and treating consumed
as the load-bearing path.

**Definition CR-D77 (Consumed reference heuristic).** In the default
runtime, a surfaced handle becomes consumed when it appears in a
later verb's input within the same trail session, including `read`,
`context`, `source_of`, `run_verb`, or an ad-hoc query literal. Host
applications may call `TrailRecorder.note_consumed` for explicit
selection gestures. Display alone is not consumption. Rationale:
trail replay should follow the path an agent actually acted on, not
every candidate a ranking surface showed.

**Definition CR-D78 (Default trail redaction patterns).** The default
redactor redacts string literal values and meta/config values whose
keys contain `secret`, `password`, `token`, `api_key`, or
`private_key`, case-insensitively. Projects may extend or replace this
set through trail configuration. Redaction patterns are policy
inputs, not relation fields. Rationale: mandatory trail capture needs
a conservative local default while leaving project-specific secret
vocabularies configurable.

**Definition CR-D65 (Trail relation normalization).** JSONL trail
entries retain structured `surfaced_refs`, `consumed_refs`, and
`source_generations` arrays exactly as captured. The Datalog stored
surface normalizes those arrays into `*trail_ref` and
`*trail_generation` rows instead of requiring object-valued `Value`
terms or encoding JSON strings inside a relation field.
`*trail_ref.kind` is a typed `TrailRefKind` enum (`surfaced` or
`consumed`) and `ordinal` preserves the stored order. `*trail`
carries scalar audit fields only.

Rationale: the rule-layer value model intentionally has scalars and
lists, not arbitrary object values. Normalized rows keep refs
queryable, preserve ordering, and avoid a stringly JSON escape hatch
in the language.

**Definition CR-D66 (Trail storage hardening).** `TrailSessionId` is a
validated filename-safe newtype before it reaches `TrailStore`; hosts
may supply session ids, but path separators, reserved `.`/`..`
segments, empty ids, and oversized ids are rejected. `TrailContext`,
not caller-supplied row payloads, stamps actor identity and default
visibility onto redacted entries. `TrailQuery` is bounded by default
and supports session, step-window, visibility, and limit constraints;
private trail queries authorize `trail_private` before opening or
deserializing candidate trail files. `TrailRecorder.note_consumed`
returns `Result` and must not silently drop explicit consumption
signals.

Rationale: mandatory trail capture is a security boundary as well as a
provenance feature. The storage API must not let host-provided session
names escape `.anneal/trails`, forge audit fields, or turn normal trail
queries into unbounded private-history scans.

**Definition CR-D67 (Trail projection safety).** Loading persisted
trail entries into Datalog relations is a second resource and privacy
gate after `TrailStore::query`. The runtime must create empty
`*trail`, `*trail_ref`, and `*trail_generation` relations for valid
empty trail loads; must filter private trail rows against the
evaluation actor as well as the store query actor; must bound
normalized `*trail_ref` and `*trail_generation` fanout per entry; must
avoid indexing high-cardinality payload fields such as
`redacted_expr`, hashes, timestamps, retention, and scores; and must
project non-finite or out-of-range scores as `null`.

Rationale: `TrailQuery.limit` bounds JSONL entries, not normalized row
fanout, and a materialized `Database` may outlive the actor context
that loaded it. The relational projection must therefore remain safe
when empty, when queried by a different actor, and when reading
host-supplied or stale trail files.

A `TrailRedactor` (§38 extension seam) produces the
`redacted_expr` and may strip surfaced/consumed refs for handles
whose `visibility` is `private`. The default redactor removes values
inside string literals and meta-key values matching configured
patterns (`secret`, `password`, customer ids).

Trails persist to `.anneal/trails/<session-id>.jsonl` on session end.
Replay/diff workflows are forward-looking (v2.1; §47).

**Definition CR-D54 (Trail privacy boundary).** Trail capture is four
separate responsibilities:

- `TrailRecorder` observes evaluated queries, surfaced rows, consumed
  refs, prelude hash, actor, and generation set.
- `TrailRedactor` removes or hashes sensitive expression values,
  refs, and payloads before persistence or display.
- `TrailSummarizer` turns a recorded path into a human/agent digest.
- `TrailStore` owns persistence, retention, replay input, and diff
  input.

`TrailSummarizer` must not be the only privacy boundary. Redaction
happens before persistence unless policy explicitly permits raw trail
storage for the actor and corpus. Replay/diff consume recorded
entries through `TrailStore`, not by reading private raw buffers.

Rationale: summarization, redaction, retention, and replay are
different decisions. Combining them into one trait would make privacy
bugs look like formatting or digest bugs.

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
metadata as it computes, but full trees are only materialized on
`--explain`. `--explain` explains the first 3 output rows by default;
`--explain-first N` changes that row cap, and `--explain-all`
explicitly restores derivations on every row. Rows beyond the cap
remain ordinary output rows without `_derivation`. Per-record
derivation is bounded to a configurable depth (default 5 levels);
deeper chains report `... + <n> more levels (use --explain-depth)`
rather than expanding.

For recursive rules, derivation is path-summarized: chains of the
same rule with bound arguments collapse to "via <rule> × N hops"
unless `--explain-depth` is explicit.

**Definition CR-D84 (Explain output row cap).** `--explain` is a
row-count bounded output provenance mode. The default cap is the first 3 rows,
`--explain-first N` explains only the first `N` rows, and
`--explain-all` is required for every-row derivations. The row cap and
depth cap are independent: `--explain-depth` changes tree depth, not
how many rows receive `_derivation`. This cap limits emitted
derivation fields; it is not a fixpoint-work or memory budget.
Rationale: multi-row agent queries must remain scannable while
preserving an explicit full-trace escape hatch.

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

**Definition CR-D39 (Snapshot identity and fallback history
semantics).** `*snapshot.snapshot` is the point-in-time snapshot id;
`*snapshot.id` is the corpus-unique handle id whose historical
key/value pair was recorded. `.anneal/history.jsonl` stores one JSON
object per snapshot:
`{snapshot, at, corpus, facts:[{id,key,value}]}`. Empty snapshot ids,
empty fact ids/keys, and unparseable `at` timestamps are recoverable
history read warnings; invalid entries are skipped. Writers MUST
validate the same entry contract before appending history. `snapshot:last`
selects the parseable entry with the latest `at` timestamp;
`snapshot:<id>` selects by `snapshot`; ISO and relative date refs
select the nearest parseable `at` timestamp. Ties choose the later
snapshot so replay is monotonic as history grows. The runtime reads
history through the core history reader and hydrates it into
`*snapshot` rows before evaluation; the evaluator consumes relations
and does not open project files directly.

Snapshot history is the v2.0 fallback for handle-state time travel, not
an implicit full-corpus replay. When a `Source` cannot re-extract full
facts for a historical reference, snapshot-backed `at()` applies the
selected `*snapshot` handle key/value rows to current handles and
reports a structured partial-history warning for relations not backed
by that source. Rows for handles absent from the current extraction are
not synthesized from key/value snapshots alone. Snapshot fallback
supports `*handle`, selected `*snapshot` rows, and only handle-state
engine primitives recomputed from the historical handle overlay:
`terminal`, `active`, `settled`, `pipeline_position`,
`pipeline_position_for`, `obligation`, `freshness`, and `flux`.
Snapshot fallback rejects stored relations without snapshot backing,
edge/content-derived primitives, graph traversal primitives, and
derived predicates inside fallback `at()` blocks. Those require full
historical rule evaluation and remain v2.0 errors. Rationale: SP-Q6
needs stable status comparisons now, while full historical fact replay
belongs behind explicit source capabilities rather than accidental
current-state mixing.

**Definition CR-D41 (Corpus-unique handle ids).** `*handle.id` is
unique within a corpus across all loaded sources. `source` and
`native_id` preserve adapter-local identity for generation retraction,
origin tracking, and host integration, but graph endpoints
(`*edge.from`, `*edge.to`), content handles, snapshot handle state, and
public query predicates all use the corpus-unique `id`. If an
adapter's native ids can collide with another source, the adapter must
qualify the public id (for example with a source or URI prefix) while
leaving `native_id` unchanged. Rationale: graph traversal and snapshot
fallback cannot be deterministic if endpoint identity is only locally
unique to `(corpus, source)`.

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

Runtime code treats capabilities as typed `ActorCapability` values
and serializes them as strings only at host/surface boundaries. The
built-in constructors are explicit: trusted local CLI actors carry
all runtime capabilities plus private fact visibility; conservative
MCP actors carry none until the MCP surface or host grants them.

**Definition CR-D63 (Policy action gates).** Capability checks are the
built-in runtime floor; `Policy` is the host/project authorization
layer above that floor. `PolicyDecision` is a closed allow/deny result.
The default runtime policy allows all actions so v1-compatible CLI
execution is unchanged unless a surface or host installs a narrower
policy. A denied policy check returns a policy-denial error naming the
actor and action. `Action` is a policy-boundary type, not a source
type; variants carry the target data available at the gate, such as
`read`/`read_full` handle, `search` query and optional handle scope,
`match` pattern and optional handle scope, `eval`, and extraction
source. Policy is consulted before `read`, `search`, `match`, and
`read_full` perform provider work, regex compilation, or content
budgeting, and evaluator entrypoints consult `eval` policy before
rule/query execution. Capability-required errors remain distinct from
policy denials: missing `read_full`, `eval`, or `trail_private`
capability reports the missing capability before project policy is
considered.

**Definition CR-D64 (Trail-private authorization hook).** v2.0 exposes
a typed authorization hook for reading `private`-visibility trail
entries before the concrete `TrailStore` lands. The hook checks
`RuntimeCapability::TrailPrivate` and then `Policy` with the
`TrailPrivateRead` action. `anneal-t10.1` owns the concrete row-level
enforcement: every `TrailStore::query` result with private visibility
must pass through this hook before returning to CLI, MCP, or host API
callers.

**Definition CR-D53 (Fact visibility boundary).** Authorization is
not only a surface action gate. The fact store carries an evaluation
visibility envelope for source-derived rows. Relation schemas do not
expose that envelope as ordinary user data; it is the runtime's
filter for actor-scoped evaluation, search, read, provenance, and
trail capture. Sources may attach visibility at extraction time;
missing visibility defaults to `public`.

Visibility values are named policy labels. The built-in labels are
`public`, `team`, and `private`; hosts may define narrower labels as
policy inputs. Derived rows inherit the most restrictive visibility of
the facts and primitive rows used to derive them according to the
active policy ordering.

**Definition CR-D61 (Fact visibility capabilities).** The runtime maps
fact visibility labels to actor capabilities before derivation.
`public`, `team`, and `private` are the built-in labels and ship with
the default ordering `public < team < private`; custom label sets and
non-ordinal policy models remain CR-OQ10. `public` rows are visible to
all actors. `team` rows require `visibility:team` or
`visibility:private`. `private` rows require `visibility:private`.
Local CLI construction may use an all-visible database view for
backwards-compatible operator workflows; embedded hosts and MCP
surfaces must pass an explicit actor context. `trail_private` governs
private trail reads only; it does not by itself reveal private source
facts.

**Definition CR-D62 (Visibility closure over handle references).**
The runtime enforces a conservative closure over hidden handles before
derivation. Source-derived rows that reference a hidden handle inherit
at least that handle's hiddenness when constructing an actor-scoped
logical database: content, spans, metadata, concern membership, and
edges with a hidden endpoint are absent even if their own visibility
envelope is missing or broader. Runtime snapshot rows for hidden
handles are absent from actor-scoped evaluation as well. This keeps a
single missed child-row annotation from leaking private handle
existence through graph, search, read, diagnostic, or time-scope
queries.

**Rule CR-R10 (Visibility before derivation).** For actor-scoped
evaluation, the runtime filters hidden facts before any rule,
primitive, aggregate, search, read, diagnostic, trail, or provenance
step can observe them. Hidden rows are absent from the actor's
logical database; they are not filtered only after query output.

This prevents leaks through counts, low-confidence scores, diagnostic
presence, `schema`/`source_of` examples, trail refs, and aggregate
zero/nonzero differences. `schema`, `describe`, and `sources` may
describe relation shapes and linked adapters, but they must not reveal
private values, private row counts, or private handle identifiers
without policy approval.

---

## Part IV: The language [CR-L]

### §17 Grammar

Stratified Datalog rule layer over the corpus runtime. Named fields
on stored relations, lowercase identifiers, `:=` for "is true when,"
`?` for queries, `*relation{...}` for stored data. The rule syntax is
Datalog-shaped; primitives, directives, visibility, trails, policy,
and provider-backed content/search belong to the runtime substrate.

```
program     := statement*
statement   := fact | rule | query | directive

fact        := head '.'
rule        := head ':=' body '.'
query       := '?' [local_rules] body '.'
directive   := 'include' string '.'
             | 'at' '(' string ')' '{' statement* '}'
             | '@verb' '(' verb_args ')'
             | '@doc' '(' doc_args ')'
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
doc_args    := 'name' ':' string ',' 'doc' ':' string
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

**Definition CR-D79 (Directive reification).** `@verb`, `@doc`,
`include`, `import`, and `at` are syntax directives, not facts that
participate in fixpoint evaluation. `@verb` and `@doc` are also
reified into runtime introspection rows (`verbs`, `describe`,
`source_of`, and `examples` when present) after load-order and
shadowing rules resolve the program. Rationale: authors write
declarative annotations, while agents query a relational
self-description surface.

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

**Definition CR-D80 (Primitive evaluation within strata).** Function
primitive calls are evaluated as relational body atoms inside the
same stratum as the rule that contains them. They observe the
actor-filtered stored facts and all same-stratum positive derived
rows that have reached the current fixpoint iteration; if those
inputs grow, the primitive call is eligible for re-evaluation before
the stratum is complete. Primitive calls do not create hidden
side-channel state inside fixpoint evaluation. Rationale:
aggregation, graph traversal, search, and read must compose with
derived predicates without accidentally freezing an earlier partial
view of the stratum.

### §20 Aggregation [CR-D17]

**Definition CR-D17 (Aggregation).** Form:
`agg_var = AggFn{ [agg_args :] contributing_var : sub_body }`.

`TopK`, `Rank`, and `TakeUntil` take agg_args; the standard
aggregators don't.

```
area(area) := area_of(h, area).
total_potential(area, total) :=
  area(area),
  total = Sum{ e : potential(h, e), area_of(h, area) }.

namespace(ns) := *handle{namespace: ns}.
namespace_open_count(ns, n) :=
  namespace(ns),
  n = Count{ h : *handle{id: h, namespace: ns},
                 obligation(h),
                 not discharged(h) }.

top_blockers(h, score) :=
  (h, score) = TopK{ k: 10, key: score :
    (h, score) :
    *handle{id: h, namespace: "OQ"},
    not discharged(h),
    potential(h, score)
  }.

read_until_budget(h, span_id, text, start_line, end_line, tokens) :=
  (span_id, text, start_line, end_line, tokens) =
  TakeUntil{ budget: 4000, sum: tokens, key: start_line :
    (span_id, text, start_line, end_line, tokens) :
    read(h, 4000, span_id, text, start_line, end_line, tokens)
  }.
```

Standard Datalog aggregation semantics: compute the contribution rows
such that the sub-body holds, then collapse them with the aggregation
function. Free variables outside the aggregation form the grouping
key. `TopK` and `TakeUntil` are first-class — set semantics alone are
insufficient for agent retrieval workflows.

**Definition CR-D38 (Non-count aggregation semantics).** `Sum`,
`Avg`, `Min`, `Max`, `List`, and `Set` are scalar aggregators:
they produce one value per positively originated group that has at
least one contribution. `Count` is the only aggregate that emits for
an originated empty group. `Sum` and `Avg` require numeric
contribution values; `Avg` returns a float. `Min` and `Max` use the
runtime's total value ordering. `Sum` and `Avg` evaluate every
contribution row, so equal numeric values from distinct bindings still
contribute. `Count`, `List`, and `Set` operate on distinct
contribution values; `List` and `Set` return deterministic sorted
lists because the runtime has no bag value type.

`TopK`, `Rank`, and `TakeUntil` are row-producing aggregators:
they may emit zero or more rows per group by unifying the aggregate
result expression with selected contribution values. `TopK` requires
ground integer `k` and a ground `key` expression for each candidate;
it sorts by descending key with contribution-value tie-breaks and
emits exactly the first `k` candidates. Ties at the boundary are
resolved by that deterministic tie-break, not by "include all ties".

`Rank` requires `key` and `rank` args. `rank` must name a variable
available to the contribution expression; the runtime emits all
candidates sorted by descending key and binds 1-based dense ranks,
with equal keys sharing a rank. `TakeUntil` requires ground integer
`budget`, non-negative integer `sum` per candidate, and ground `key`
per candidate. It sorts by ascending key with contribution-value
tie-breaks, then emits candidates while the cumulative `sum` remains
`<= budget`; the first candidate that would exceed budget stops the
group. Contribution values may be non-numeric for row-producing
aggregators; only `key`, `k`, `budget`, and `sum` carry numeric or
ordering requirements. Rationale: budgeted context assembly must be
deterministic and explicit about ordering, while preserving the
relational shape agents can compose.

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

**Definition CR-D88 (Aggregate body stratification).** Any derived
predicate referenced inside an aggregate body is fully derived in an
earlier stratum than the rule containing the aggregate. This applies
to scalar and row-producing aggregators. Mutual recursion through an
aggregate body is rejected at load time with the cycle named. The
runtime does not re-evaluate aggregate outputs against same-stratum
deltas because `Count`, `TopK`, `Rank`, and `TakeUntil` can invalidate
previously emitted rows when new contribution rows appear; source-order
independence comes from stratifying aggregate inputs, not from
same-stratum aggregate retractions.

### §21 Negation, recursion, stratification [CR-D18]

**Definition CR-D18 (Stratified negation).** The runtime partitions
rules into strata such that any predicate referenced under `not` is
fully derived in an earlier stratum. Mutual recursion through
negation is rejected at load time with the cycle named.
Aggregate body dependencies are stratifying per CR-D88.

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

**Definition CR-D20 (Machine output contract).**

- **machine stdout: pure NDJSON.** One record per match, `\n`
  terminated, streamed as derived. No verb-banner text, no
  "underlying query" print on stdout — see §25 for where the query
  lives. CLI human rendering is a surface projection governed by
  CR-D87.
- **stderr: human text.** Progress, warnings, parse errors with
  line/column. Never NDJSON.

Field names come from the head's variables (or for headless queries,
from the body's bindings, last-mention-wins).

Cardinality: set semantics by default (duplicates deduplicated). For
multiset, use explicit aggregation or include the full key in the
head.

Provenance: `--explain` (CLI) or `derivation: true` (MCP) may add a
`_derivation` field to records. CLI `--explain` is capped by CR-D84;
without explain, records are bare.

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
  checks.dl         # standard diagnostics; E001 anchors convergence entropy
  ranking.dl        # search/ranking helper predicates around CR-D42
  views.dl          # the starter verbs as saved queries
```

`anneal -e '? source_of("convergence", file, lines).'` prints the
file:lines where the convergence vocabulary lives.
`ANNEAL_PRELUDE_PATH` overrides the
embedded prelude; doing so changes the `prelude_hash` and is
recorded in trails — agents replaying a trail later see whether the
prelude differs.

**Definition CR-D55 (Versioned prelude package).** The standard
library is an internal `PreludeSet`, not loose embedded text. A
`PreludeSet` has a version, ordered file list, content hash, and
source map. `anneal-core` loads the checked-in v2.0 set by default;
`ANNEAL_PRELUDE_PATH` creates a custom set with its own hash and no
compatibility promise beyond the public language/runtime contracts.

`prelude_hash` remains the reproducibility key recorded in trails,
snapshots, and query echo. The version exists for human compatibility
and migration messages; the hash is what replay uses to detect exact
drift.

Rationale: the prelude is now a standard library compatibility
surface. Treating it as a package hides file layout and embedding
details while preserving `source_of` line-level introspection.

**Definition CR-D59 (Custom prelude package order).**
`ANNEAL_PRELUDE_PATH` points to either one `.dl` file or a directory
containing `.dl` files. A file path loads that file as the whole custom
prelude package. A directory path loads all descendant `.dl` files in
bytewise UTF-8 relative-path order; non-UTF-8 descendant paths are
rejected. A single-file package uses the package-local hash key
`prelude.dl` rather than the caller's absolute path. The resulting
`PreludeSet` has no checked-in version, and its `prelude_hash` is
computed over the ordered package-local file keys plus each file's
bytes. Custom prelude package files are package members; `include` and
`import` directives are rejected inside `PreludeSet` packages. Use
directory membership for multi-file custom preludes.

Rationale: custom preludes must be replayable. Reproducible replay
requires the package boundary and load order to be deterministic before
the hash is meaningful.

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
         compare: anneal -e '? source_of("blocked", file, lines).'
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

entropy(h, "undischarged") :=
  obligation(h), not discharged(h), not terminal(h).

entropy(h, "broken_ref") :=
  diagnostic("E001", severity, h, file, line, evidence).

entropy(h, "stale_dep") :=
  *edge{from: h, to: t, kind: "DependsOn"},
  active(h), terminal(t).

entropy(h, "confidence_gap") :=
  *edge{from: h, to: t, kind: "DependsOn"},
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

potential_subject(h) := entropy(h, source).
potential(h, energy) :=
  potential_subject(h),
  energy = Sum{ w : entropy(h, source), potential_weight(source, w) }.

blocked(h) :=
  active(h),
  potential(h, energy), energy >= 3,
  flux(h, days: 30, delta: 0).

advancing(h) :=
  active(h),
  recently_advanced(h).

snapshot_history_present(count) :=
  count = Count{ snapshot : *snapshot{snapshot: snapshot} },
  count > 0.

recently_advanced(h) :=
  snapshot_history_present(count),
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
lifecycle by declaring `config convergence { ordering([...]). }` and
the predicates `terminal/1`, `active/1`, `pipeline_position_for/2`
in `anneal.dl` if the defaults don't fit.

Sample lifecycle profiles ship in `views.dl` for inspiration
(`profile_doc_corpus`, `profile_code_corpus`, `profile_issue_corpus`);
projects copy the one closest to their shape.

**Definition CR-D58 (Lifecycle profile examples).** Sample lifecycle
profiles are executable prelude data, not comments. `views.dl`
declares each profile as a unary predicate whose single string value
is a copyable `anneal.dl` snippet, and documents it with `@doc`.
Agents discover them with `describe("profile_doc_corpus", doc)` and
copy them with `? profile_doc_corpus(snippet).`.

Rationale: starter profiles are onboarding affordances. Representing
them as queryable data keeps them visible through the same
self-description channel as verbs and predicates, without making them
engine behavior.

### §27.1 Structural graph vocabulary [CR-D47]

**Definition CR-D47 (Structural graph vocabulary).** Predicates
defined in `graph.dl` expose source-neutral graph shape without
giving any adapter a privileged traversal direction.

```
area_of(h, area) :=
  *handle{id: h, area: area},
  area != "".

namespace_of(h, namespace) :=
  *handle{id: h, namespace: namespace},
  namespace != "".

status_of(h, status) :=
  *handle{id: h, status: status},
  status != null.

incoming_edge(h, from, kind) :=
  *edge{to: h, from: from, kind: kind}.

outgoing_edge(h, to, kind) :=
  *edge{from: h, to: to, kind: kind}.

incident(h, other) := incoming_edge(h, other, kind).
incident(h, other) := outgoing_edge(h, other, kind).

orphan(h) :=
  *handle{id: h, kind: kind},
  in_degree(h, 0),
  kind != "file",
  kind != "section".

stub(h) :=
  *handle{id: h, kind: "file"},
  token_estimate(h, 0).

hub(h, degree) :=
  *handle{id: h},
  degree = Count{ other : incident(h, other) },
  degree >= 10.
```

`hub/2` counts distinct neighboring handles, not raw edge count. The
threshold is intentionally small and stable for agent triage; richer
orientation scoring belongs in `ranking.dl` or project rules.

### §27.2 Work ranking vocabulary [CR-D48]

**Definition CR-D48 (Work ranking vocabulary).** Predicates defined
in `ranking.dl` provide default convergence-oriented selection over
the standard library's `potential/2` relation.

```
work_candidate(h, energy) :=
  potential(h, energy).

top_work(h, energy) :=
  (h, energy) = TopK{ k: 25, key: energy :
    (h, energy) :
    work_candidate(h, energy)
  }.

ranked_work(h, energy, rank) :=
  (h, energy, rank) = Rank{ key: energy, rank: rank :
    (h, energy, rank) :
    work_candidate(h, energy)
  }.
```

These are starter predicates, not a surface mandate. Surfaces may add
budgeting, capability checks, or output shaping, but the default
meaning of "work" remains "highest potential first."

### §28 Check rules [CR-D23]

**Definition CR-D23 (Check rule).** A rule whose head is
`diagnostic(...)` deriving a fact representing a consistency
violation.

The v2.0 check catalog mirrors anneal v1.x — E001 (broken refs), E002
(undischarged), W001-W004 (warnings), I001-I002 (info), S001-S005
(suggestions) — as Horn clauses in `checks.dl`. The substrate has no
hard-coded check logic. E001 is the minimal executable anchor required
by the convergence vocabulary; the remaining catalog must land before
Phase 6 check-rule parity closes.

```
# checks.dl excerpt

diagnostic("E001", "error", src, file, line, target) :=
  *edge{from: src, to: target, file: file, line: line},
  not *handle{id: target}.
```

### §28.1 Diagnostic relation boundary [CR-D49]

**Definition CR-D49 (Relational diagnostic contract).**
`checks.dl` owns the relational diagnostic contract:

```
diagnostic(code, severity, subject, file, line, evidence)
```

`code` and `severity` are stable strings. `subject` is the handle,
namespace, status, or corpus scope that caused the diagnostic. `file`
and `line` are nullable source-location fields. `evidence` is a
runtime value: `null`, a scalar, or a list/tuple whose first element is
an evidence kind such as `"broken_ref"` or
`"candidate_namespace"`.

Surfaces lower diagnostic rows into human messages, JSON records,
diagnostic IDs, and suggestion IDs. That lowering may use adapter
evidence rows from CR-D31 and source-specific compatibility code while
v1.x parity is being retired. `checks.dl` MUST still derive the same
diagnostic code/severity membership as the v1.x check pipeline on the
frozen parity fixtures; exact rendered JSON identity is a surface
compatibility gate, not a Datalog expressiveness requirement.
Status-level and corpus-level suggestions such as S003 and S005 use
`file = null`; v1.x representative files for those rows were display
choices, not semantic locations.

Rationale: v1.x diagnostic records include rendered prose, hashed IDs,
JSON evidence decoding, and resolution-cascade candidate formatting.
Those are output concerns; the standard library should remain
queryable Horn clauses over stored relations.

### §28.2 Corpus-scoped diagnostic locations [CR-D69]

**Definition CR-D69 (Corpus-scoped diagnostic locations).** I001,
S003, and S005 are corpus- or status-scoped diagnostics. Their
relational `diagnostic(...)` rows MUST use `file = null` and
`line = null`; concrete files involved in the evidence MAY appear in
the `evidence` value when they are semantically meaningful. During the
dual-CLI parity window, surfaces MAY attach v1.x representative files
when lowering these rows into legacy JSON records and diagnostic IDs.

Rationale: representative files for corpus-wide summaries were
historical display anchors. Agents querying the diagnostic relation need
the scope to be explicit instead of inferring corpus-level meaning from
an arbitrary source path.

### §28.3 Namespace inference and concern-candidate scope [CR-D50, CR-D90]

**Definition CR-D50 (S005 observed namespace scope).** The standard
S005 concern-group suggestion considers pairs of resolved label
namespaces. Because S005 describes a corpus-level co-occurrence pattern
rather than a single source location, its `diagnostic(...)` row uses
`file = null` and carries the concrete prefix pair plus file count in
evidence; surfaces may choose a representative file for compatibility
displays.

**Definition CR-D90 (Namespace config is policy, not inventory).**
Label namespace discovery is corpus-derived. A markdown Source infers
namespaces from recurring sequential labels and emits resolved label
handles as observed vocabulary. Project config must not snapshot every
observed namespace as a manual allow-list. `config handles { force([...]). }`
is reserved for sparse namespaces that cannot yet satisfy inference;
`rejected([...])` blocks false positives; `linear([...])` assigns
obligation semantics.

Rationale: forcing agents to append every new prefix to project config
turns ordinary corpus growth into schema maintenance. The stable
contract is observed namespace facts plus explicit policy overrides, not
a hand-curated inventory. The representative file in v1.x was an
iteration-order artifact, not part of the semantic diagnostic.

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
# Adapter-qualified: declarations are scoped to the source they target.
source md {
  file_extension(".md").
  scan_root(".").
  scan_exclude("node_modules").
  label_pattern("OQ",    "OQ-(\d+)",    "any").
  linear_namespace("OQ").
}

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

release_blocker(h, "broken_ref") :=
  diagnostic("E001", severity, h, file, line, evidence).
release_blocker(h, "undischarged") :=
  diagnostic("E002", severity, h, file, line, evidence).
release_blocker(h, "blocking_oq")  :=
  blocking_oq(h),
  *meta{handle: h, key: "milestone", value: "v0.3"}.

# === verbs ===
@verb(
  name: "release-blockers",
  query: "? release_blocker(h, why).",
  doc: "Open OQs and broken references gating the next release.",
  output_schema: "{\"h\":\"HandleId\",\"why\":\"String\"}",
  capabilities: ["read"]
)
```

**Discovery declarations are adapter-qualified** through their
`source <adapter-prefix>` block, so two adapters can't silently fight
over the same key. Raw adapter-qualified facts remain accepted for
optional discovery declarations. The runtime errors at load if a
discovery fact prefix names an adapter that isn't linked, unless the
prefix is declared optional:

```
source md { file_extension(".md"). }       # error if anneal-md not linked
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

**Definition CR-D60 (Executable verb schema encoding).** In v2.0, the
parser-accepted `@verb.output_schema` encoding is a canonical JSON
string whose top-level object maps output field names to schema terms.
The runtime parses that string at load, rejects malformed or unsupported
schema shapes, and validates the query's output fields against the
top-level schema keys. Object-literal schema examples in this document
are specification notation until `anneal-lang` grows object literal
syntax; surfaces and registries consume the parsed schema, not the raw
string. Rationale: this preserves typed load-time validation without
making Phase 7 also carry a broader expression-grammar expansion.

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

**Single-adapter sugar.** The canonical form is an explicit source
block, even when only one adapter is linked:

```dl
# In a markdown-only project:
source md {
  file_extension(".md").
  label_pattern("OQ", "OQ-(\d+)", "any").
}
```

For transitional snippets, when exactly one adapter is linked, the
runtime also auto-qualifies unqualified discovery facts to that adapter:

```dl
file_extension(".md").              # auto-qualified to md.file_extension

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

*meta{
  handle: <file handle>,
  key: "md.parent_dir",
  value: <parent directory relative to corpus root, or "">,
  ...
}

*meta{
  handle: <any handle>,
  key: "md.resolved_file",
  value: <owning file path relative to corpus root>,
  ...
}
```

Rationale: W004 and similar parse-filter diagnostics must be
reconstructible from stored facts without re-running a format-specific
parser inside the substrate. W003 needs the markdown parent directory,
not the source-neutral top-level area. Diagnostics for labels and
versions need an owning source file without changing the source-neutral
`*handle.file` field that legacy surface parity depends on.

---

## Part VII: Surfaces [CR-Su]

### §33 The starter verbs

The prelude's `views.dl` ships these as saved verb declarations.
Projects override or extend any.

| Verb | Question | Underlying query (sketch) |
|---|---|---|
| `anneal status` | where am I | composed of summary, work, advancing, blocked |
| `anneal H` | what is this handle | `*handle{id: H, ...}` + immediate edges |
| `anneal find TEXT` | identity-search by id substring | `*handle{id, ...}, id contains "TEXT"` |
| `anneal search TEXT` | content match by query | `TopK{... search("TEXT", h, span_id, score, reason, field, low_confidence), low_confidence = false}` |
| `anneal context GOAL` | composition for cold-agent localization | see §33.1 |
| `anneal read H` | give me H's content, bounded | `read(H, budget, span_id, text, start, end, tokens)` |
| `anneal work` | where should I work | `top_work(h, e)` |
| `anneal blocked H` | what's blocking H | `entropy("H", source), entropy_detail(...)` |
| `anneal trend` | corpus over time | `at(--at) { ... }` vs `at("now") { ... }` |
| `anneal broken` | are there errors | `diagnostic(code, "error", ...)` |
| `anneal vocab` | what words does this corpus use | observed statuses, edge kinds, namespaces, metadata keys |

Plus self-description surfaces from §11. v0.11.0 ships CLI verbs for
`schema`, `verbs`, `describe`, `sources`, and `vocab`; `predicates`,
`source_of`, and `examples` are query primitives available through
`anneal -e` until promoted to CLI verbs.

**Definition CR-D86 (Corpus vocabulary verb).** `anneal vocab` is a
standard-library verb that lists observed corpus-local vocabulary
needed before filtering: status values, edge kinds, namespaces, and
frontmatter/metadata keys. It is descriptive, not normative; the
runtime must not infer lattice semantics from the verb's rows.

Rationale: cold agents need to discover the corpus's actual words
before writing Datalog filters, and this should take one compact
command rather than a sequence of schema guesses.

Plus meta forms:

| Form | Purpose |
|---|---|
| `anneal -e '<q>'` | custom query; `-e -` reads from stdin |
| `anneal init` | scaffold repo-local `anneal.dl` project declarations |
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
| `--budget=N` | `4000` tokens | context budget hint; v2.0 derives a per-hit read cap from it |
| `--neighborhood-depth=D` | `1` | edges to traverse outward from the top hit |
| `--hits=K` | `3` | candidates to return (after `TopK` filtering) |

Underlying composition contract (from `views.dl`):

```dl
@verb(
  name: "context",
  query: "
    context_readable(h) :=
      *content{handle: h, tokens}, tokens <= per_hit_budget.

    context_hit(h, hit_span_id, score, reason, field) :=
      (h, hit_span_id, score, reason, field) = TopK{ k: hits, key: score :
        (h, hit_span_id, score, reason, field) :
        search(goal, h, hit_span_id, score, reason, field, low_confidence),
        low_confidence = false,
        context_readable(h)
      }.

    context_neighbor(h, h) := context_hit(h, hit_span_id, score, reason, field).
    context_neighbor(h, neighbor) :=
      context_hit(h, hit_span_id, score, reason, field),
      neighborhood(h, neighborhood_depth, neighbor).

    ?
      context_hit(h, hit_span_id, score, reason, field),
      (span_id, text, start_line, end_line, tokens) = TakeUntil{
        budget: per_hit_budget, sum: tokens, key: start_line :
        (span_id, text, start_line, end_line, tokens) :
          read(h, per_hit_budget, span_id, text, start_line, end_line, tokens)
      },
      context_neighbor(h, neighbor).
  ",
  output_schema: "{\"goal\":\"String\",\"hits\":\"List<{handle,span_id,score,reason,field}>\",\"spans\":\"List<{handle,span_id,start_line,end_line,tokens,text}>\",\"neighborhood\":\"List<{handle,neighbor}>\"}",
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

Budget allocation: v0.11.0 derives a per-hit `context_read_budget`
from the requested `--budget` and applies that cap independently to
each top hit's `read`. It does not divide the read cap by `K`; doing so makes
increasing `--hits` silently exclude the most relevant document before
`read` can return a useful first span. Exact whole-response token
accounting across grouped `hits`, `spans`, and `neighborhood` is not a
v0.11.0 invariant. The exact allocation ratio is a tuning default
tracked by CR-OQ11, not a semantic contract.

Cold-agent test (§49 large-corpus fixture) targets a single `context`
call after an optional `anneal status` surface — total tool calls ≤2,
counted including any required follow-ups.

**Definition CR-D45 (Executable context lowering fixture).** The
executable `context` contract in v0.11.0 is the lowered Datalog
program used by `views.dl` and `anneal-cli`: the surface introduces
parameter facts (`context_goal`,
`context_hits`, `context_read_budget`, `context_neighborhood_depth`).
`context_read_budget` is the already-allocated per-hit span budget,
not the total invocation budget. The query then runs over `TopK`,
`TakeUntil`, `read`, and `context_neighbor`. The `TopK` result is
first materialized as `context_hit` before joining reads or neighbors;
otherwise later positive atoms can bind `h` early and accidentally
turn the query into top-K-per-handle. `context_hit` also requires
`context_readable(h)`, meaning `*content{handle: h, tokens}` exists
before the handle can win TopK; oversized spans remain eligible and
are clipped by bounded `read` rather than being suppressed before
ranking. This check must use content metadata, not `read`, because
`read` constructs text-bearing rows and would do full-corpus read work
before ranking. `context_neighbor(h, h)` is always emitted from
`context_hit` so isolated top hits keep their read spans;
additional neighbors come from `neighborhood(h, depth, neighbor)`
anchored on `context_hit`, never from the full `*handle` universe. The
search hit's raw `hit_span_id` is preserved as hit metadata but `read`
binds a fresh content span variable, because summary/meta search hits
legitimately have `span_id = null` and must still yield readable
context. The declared `output_schema` is encoded as a canonical JSON
string in the parser-accepted fixture and grouped by the surface into
`hits`, `spans`, and `neighborhood`; raw row fields such as `h` and
`hit_span_id` are mapped to public grouped fields `handle` and
`span_id`, while invocation fields such as `goal` come from the verb
arguments rather than being duplicated into every relation row.
Rationale: this pins the shipped agent-visible behavior while leaving
future typed object-literal ergonomics to CR-OQ12.

**Definition CR-D71 (Context per-hit read cap).** The CLI derives
`context_read_budget` from `--budget` once and applies it as the cap
for each selected hit; it must not divide that cap by `context_hits`.
`context_readable(h)` may use content metadata to avoid choosing
handles that cannot produce any span under that cap, but it must not
make the top hit unreadable merely because the caller requested more
candidates. Rationale: the cold-agent gate is won by retrieving the
best document and a useful first span; exact response-wide budget
packing is a later grouped-output concern.

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
| `--explain` | include `_derivation` on first 3 records | `-e` / `eval` |
| `--explain-first=N` | include `_derivation` on first N records | `-e` / `eval` |
| `--explain-all` | include `_derivation` on every record | `-e` / `eval` |
| `--explain-depth=N` | derivation expansion depth (default 5) | `-e` / `eval` |
| `--meta` | emit `_meta` envelope record on stdout | global |
| `--no-snapshot` | don't append history on this run | global |
| `--quiet` | suppress stderr chatter | global |
| `--budget=N` | token budget for `work` / `read` / `context` | verb-specific |
| `--gate` | exit 1 if any results | `broken` |
| `--source=NAME` | restrict ingestion to one Source | global |
| `--mcp` | planned MCP launcher; v2.0 ships `anneal-mcp` as crate/library surface | global |
| `--color=auto` | TTY detect; pipes get plain text | global |
| `--pretty` | human-readable formatted JSON (breaks NDJSON contract) | global |
| `--include-low-confidence` | omit the default `low_confidence = false` predicate from search/context `TopK` templates | global, search-relevant |

**Definition CR-D82 (Help and reference projection).** CLI help is a
projection of the same runtime self-description that backs
`schema`, `predicates`, `verbs`, `describe`, `source_of`, `examples`,
and `sources`. v0.11.0 ships top-level `anneal --help`, static
`anneal <command> --help`, and the `schema`/`verbs`/`describe`/`sources`
CLI projections. Topic help, generated language-reference docs,
predicate/source/example CLI projections, and machine-readable help are
follow-up surfaces that must use this same source of truth rather than
hand-maintained parallel docs. `@doc` text may use restrained inline
Markdown (emphasis, code spans, and links) but not headings or block
structure. Error messages name available help topics or introspection
commands first; configured external URLs may be appended through
`[help].doc_url_base`. Rationale: help is not a parallel documentation
system; it is a projection layer over the runtime vocabulary.

### §36 I/O contract [CR-D25]

**Definition CR-D25 (I/O contract).**

- **machine stdout: pure NDJSON.** Bare record stream; `--meta` adds
  one envelope record at the top. Machine mode is selected when stdout
  is piped or when `--json` / `--format=json` is passed. Human mode is
  governed by CR-D87. The `--pretty` flag is also available for
  in-process formatting; it emits multi-line JSON and breaks the NDJSON
  contract, so it is human-only and never used in agent pipelines or
  `--mcp` mode.
- **stderr: human text.** Verb-banner echo, progress, warnings,
  parse errors. Never NDJSON.
- **stdin: `-` means stdin.** `anneal blocked -` reads handles, one
  per line. `anneal -e -` reads a query (heredoc-friendly).
- **Exit codes:** 0 success (including empty results), 1 query
  error, 2 invocation error, 3 gate failure.

**Definition CR-D85 (Empty row diagnostic).** Runtime commands whose
successful output is an NDJSON row stream emit `(0 rows)` to stderr
when the row stream is empty. stdout remains empty so pipes and MCP
callers can continue to treat stdout as pure NDJSON. Structurally
invalid inputs, such as an empty search query, remain invocation or
query errors rather than empty-result successes.

**Definition CR-D87 (CLI output mode selection).** Runtime CLI commands
render readable text when stdout is a terminal, preserve NDJSON when
stdout is piped, and accept `--format=text` to force readable text
through pipe-only harnesses. `--json` and `--format=json` force machine
mode. Human rendering is a CLI projection only: library, MCP, and
machine-mode CLI callers continue to consume the CR-D20/CR-D25 NDJSON
contract. Rationale: cold agents often run through pipe-only harnesses
but still need the same low-friction orientation humans get at a TTY.

Rationale: cold agents need to distinguish "the command ran and
matched nothing" from "the command produced no bytes because routing,
input, or parsing failed." Keeping the signal on stderr preserves the
stdout contract.

### §37 MCP surface [CR-D26]

**Definition CR-D26 (MCP transport).** The `anneal-mcp` crate
projects the runtime as a small stdio MCP surface. The tool surface is
**not 1:1 with verbs.** Tool inflation is a real failure mode; v2.0
ships a small stable surface that scales by introspection, not by
verb count.

**Definition CR-D72 (MCP launcher status).** v2.0 ships the MCP
surface as a crate/library boundary over `anneal-core` and adapters;
the root CLI does not yet expose a stable `anneal --mcp` or
`anneal mcp` launcher. That launcher is a surface projection follow-up
once process lifecycle, config, and policy defaults are wired through
the same path as CLI verbs. Rationale: the transport contract should
not pretend to be discoverable from the binary until it is actually
operator-ready.

Default MCP tool surface:

| Tool | Wraps | Capabilities |
|---|---|---|
| `eval` | `-e '<query>'` | gated by `eval` capability; default-denied for MCP unless project config or host policy grants it |
| `search` | `search` primitive | always allowed |
| `read` | `read` primitive (budgeted) | always allowed |
| `verbs` | `verbs` primitive | always allowed; agents see all available verbs |
| `describe` | `describe` primitive | always allowed |
| `schema` | `schema` primitive | always allowed |
| `source_of` | `source_of` primitive | always allowed |
| `status` | the `anneal` status verb | always allowed |
| `run_verb` | invoke any verb by name | gated by per-verb declared capabilities |

`read_full` is **not** a default MCP tool. Projects may expose it
via explicit configuration in `anneal.dl` if they accept
the data-exfiltration risk.

`run_verb` is the agent's entry to project-defined verbs without
flooding the top-level tool list. `tools/list` returns the ~10 tools
above; the agent discovers project verbs via `verbs` then calls them
via `run_verb`.

**Definition CR-D56 (Verb projection boundary).** `anneal-core` owns
the resolved `VerbRegistry`: verb name, source, query AST, output
schema, docs, capabilities, examples, and shadowing result. Surfaces
own `VerbProjection`: how a resolved verb becomes a CLI shorthand,
an MCP `run_verb` target, help text, or host-library call.

Surfaces must not parse raw `@verb` strings or reconstruct load-order
semantics. They ask the registry for the resolved verb set. Project
verbs shadow prelude verbs according to CR-D21 and CR-R4 before any
surface projection occurs. MCP exposes only the resolved name through
`verbs`/`run_verb`; it does not list both the shadowed and shadowing
definitions as tools.

Rationale: a verb is a runtime language object. A command or MCP tool
is a policy- and transport-specific projection of that object.

Server instructions include the standard-library prelude content
under a *trusted prelude* heading, separated from any *untrusted
corpus content* an agent might subsequently see via `search` or
`read`. Project `@verb` docs are quoted as data, not promoted into
instruction frames.

### §38 Plugin surfaces [CR-D27]

**Definition CR-D27 (Pluggable extension seams).** Beyond `Source`,
the runtime exposes narrow extension surfaces. Each surface hides one
decision that will vary across adapters or hosts:

```rust
pub trait SourceDriver {
    fn refresh(&self, cx: &SourceContext) -> Result<FactBatch, SourceError>;
}

pub fn refresh_source(
    driver: &dyn SourceDriver,
    request: &SourceRefreshRequest,
    store: &mut FactStore,
) -> Result<SourceRefreshReport, SourceDriverError>;

pub trait ContentProvider {
    fn read(&self, request: &ReadRequest, ctx: &ReadContext) -> Result<Vec<ReadChunk>, ReadError>;
}

pub trait SearchProvider {
    fn search(&self, request: &SearchRequest, ctx: &SearchContext) -> Result<Vec<SearchHit>, SearchError>;
}

pub trait Ranker {
    fn calibrate(&self, hit: &SearchHit, ctx: &RankingContext) -> f32;
    fn tie_break(&self, a: &SearchHit, b: &SearchHit) -> Ordering;
}

pub trait Policy {
    fn check(&self, actor: &ActorContext, action: &Action) -> PolicyDecision;
}

pub enum PolicyDecision { Allow, Deny }

pub enum Action {
    Read { handle: String },
    ReadFull { handle: String },
    Search { query: String, handle: Option<String> },
    Match { pattern: String, handle: Option<String> },
    Eval,
    TrailPrivateRead,
    Extract { source: String },
}

pub trait TrailRecorder {
    fn record(&self, entry: TrailEntryInProgress, ctx: &TrailContext) -> Result<(), TrailError>;
    fn note_consumed(
        &self,
        session_id: &TrailSessionId,
        step: u64,
        reference: TrailReference,
        ctx: &TrailContext,
    ) -> Result<(), TrailError>;
}

pub trait TrailRedactor {
    fn redact(&self, entry: TrailEntryInProgress, ctx: &TrailContext) -> TrailEntryRedacted;
}

pub trait TrailSummarizer {
    fn summarize(
        &self,
        session_id: &TrailSessionId,
        entries: &[TrailEntryRedacted],
        ctx: &TrailContext,
    ) -> TrailSummary;
}

pub trait TrailStore {
    fn append(&self, entry: TrailEntryRedacted, ctx: &TrailContext) -> Result<(), TrailError>;
    fn query(&self, request: TrailQuery, ctx: &TrailContext) -> Result<Vec<TrailEntryRedacted>, TrailError>;
}
```

Default impls ship in `anneal-core`. Adapters override
`SearchProvider` or `ContentProvider` for index-backed retrieval;
hosts override `Policy` for actor-based authorization; projects
override trail components to control what gets captured, redacted,
retained, and summarized.

---

## Part VIII: Onboarding [CR-O]

### §39 Configuration ladder and lattice-on default [CR-D28, CR-D89]

**Definition CR-D89 (Configuration ladder).** A corpus has three
configuration layers, in increasing project specificity:

1. Embedded prelude: the standard library shipped inside the binary
   (`graph.dl`, `convergence.dl`, `checks.dl`, `ranking.dl`,
   `views.dl`). It is loaded automatically and is not copied into a
   project by default.
2. `anneal.dl`: repo-local project declarations. It owns source
   settings, convergence statuses, terminal/active sets, ordering,
   namespaces, frontmatter mappings, source excludes, history mode, adapter
   discovery facts, project rules, overrides, and `@verb`
   declarations. Static `config` and `source` blocks are lowered
   before Source extraction; rules and verbs load after extraction.
3. User config: machine-local preferences under XDG config. It owns
   workstation paths and local preferences that must not be committed
   into repo config.

New installs work with no files at all: the embedded prelude and
markdown defaults are enough to run status, search, read, context, and
raw queries. Existing `anneal.toml` files are upgrade input for
`anneal init --force`, but runtime commands use `anneal.dl`. If
`anneal.toml` is still present, runtime commands fail with a migration
diagnostic rather than merging two config authorities. `anneal init` is a
scaffold and conversion operation: it previews with `--dry-run`,
refuses to overwrite existing repo config, and with `--force` writes
unified `anneal.dl` and moves an older TOML file to
`anneal.toml.legacy`.

**Definition CR-D28 (Init defaults).** `anneal init` scaffolds a
minimal lattice in `anneal.dl`:

```
$ anneal init

scanning corpus...
  found 47 markdown files
  inferred Source: anneal-md (linked)
  status frontmatter: present in 41/47 (87%)
  inferred lattice: raw → draft → current → stable

wrote anneal.dl
  source md {
    file_extension(".md").
    scan_root(".").
  }

  config convergence {
    ordering(["raw", "draft", "current", "stable"]).
    active(["draft", "current", "stable"]).
    terminal(["superseded", "archived"]).
  }

next steps:
  anneal status                see the landscape
  anneal describe convergence  read what convergence means here
  anneal work                  pick where to work
```

The agent's first session lands in convergence mode, not graph mode.
Graph mode is the zero-config fallback when no convergence config is
declared. Project rules and verbs are opt-in extensions inside the
same `anneal.dl` file, not a prerequisite for ordinary configuration.

### §40 The agent loop [CR-D29]

**Definition CR-D29 (Agent loop).**

```
1. anneal status           see the landscape
2. anneal work             pick where to work
3. anneal blocked H        understand why H isn't moving
4. (do the work)
5. anneal trend            confirm potential dissipated
```

For arrival on an unfamiliar corpus, prepend:

```
0a. anneal sources         what adapters are loaded
0b. anneal describe convergence  what convergence means here
0c. anneal vocab           what statuses, edge kinds, and namespaces exist
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

The Source decides the mapping. Handle id is unique within the corpus
per CR-D41; internal identity is `(corpus, source, kind, native_id)`.

### §42 Discovery configuration

Adapter-qualified discovery facts per §32. The markdown adapter's
shape:

```dl
source md {
  file_extension(".md").
  scan_root(".").
  scan_exclude("node_modules").
  label_pattern("OQ",    "OQ-(\d+)",    "any").
  label_pattern("KB-D",  "KB-D(\d+)",   ".design/**").
  linear_namespace("OQ").
  version_pattern("formal-model", "formal-model-v(\d+)\.md").
  section_min_depth(1).
  section_max_depth(3).
}
```

Other adapters declare their own (`code.module_pattern`,
`issues.repo`, etc.). The runtime errors if a required fact's prefix
isn't recognized by a linked Source; `optional` facts are skipped
when the adapter is absent.

**Definition CR-D70 (Markdown scan-root identity).** `md.scan_root`
selects one or more corpus-relative subtrees for markdown extraction;
it does not rewrite handle ids, `native_id`, or `origin_uri`. A file
at `included/b.md` remains `included/b.md` even when the only scan
root is `included`. Rationale: discovery configuration changes the
extraction window, not the identity of already-known corpus objects.

### §43 Introspection

**Definition CR-D44 (Introspection tuple encoding).**
Self-description primitives return scalar strings or literal lists, not
surface-specific records. `source_of(name, file, lines)` and
`predicates(..., source_lines)` encode `lines` as comma-separated
1-based line numbers, or `unknown` when the runtime only knows the
source identity. Engine-defined names point at their implementation
module or this spec; source-derived predicates point at the rule file.
`sources(..., recognizes, capabilities, doc)` emits `recognizes` as a
list of glob strings and `capabilities` as the list of true capability
names (`supports_git_ref`, `supports_time_snapshot`,
`supports_incremental`, `live_only`, plus `search` when the Source
advertises `SearchInfo`). `@verb.doc` is authoritative for verbs;
rule-defined predicates without attached docs get a generated fallback
doc and rely on `source_of` for precise context. Rationale: agents need
one stable relational shape across CLI, MCP, and library surfaces
without a second decoding convention for introspection rows.

**Definition CR-D81 (Schema signature encoding).** `schema(name,
kind, signature, determinism, source_provenance)` returns a canonical
signature string of the form `name(arg1: Type, arg2: Type) -> row` for
predicates and `name(args...) -> grouped-output` for verbs. `kind` is
a machine-readable lifecycle/source category such as `stored`,
`derived`, `sealed-primitive`, `soft-primitive`, `verb`, or `source`.
The number of top-level arguments before `->` is the arity surfaces
report in help and diagnostics. Unknown or variadic positions must be
spelled explicitly (`Any`, `List<T>`, or `...`); docstrings never
occupy arity or signature fields. Rationale: agents must be able to
recover a call shape and shadowing behavior from `schema` without
reverse-engineering prose or column order.

**Definition CR-D46 (Documentation declarations).** `@doc(name:
"...", doc: "...").` is a non-evaluating prelude or project
annotation with required string `name` and `doc` arguments. Malformed
or missing arguments are load errors. It contributes `describe(name, doc)` and
`source_of(name, file, lines)` rows using the annotation's source
location. When the same name is also a rule-defined predicate, the
`@doc` text is the predicate documentation and the predicate's rule
locations remain visible through `predicates(...)` and `source_of(...)`.
Later `@doc` declarations for the same name replace earlier
declarations by load order. Rationale: source-backed topic
documentation lets agents jump from runtime vocabulary such as
`convergence` to the canonical prelude source without creating dummy
relations.

```
# 1. Counts by kind
anneal -e '? *handle{kind: k}, c = Count{ h : *handle{id: h, kind: k} }.'

# 2. Label namespaces and counts
anneal -e '? *handle{kind: "label", namespace: ns}, c = Count{ h : *handle{id: h, kind: "label", namespace: ns} }.'

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
  anneal.dl             # config blocks + source declarations + project predicates + verbs
  .anneal/
    history.jsonl       # snapshot append log
    trails/             # session paths
      <session-id>.jsonl
    generations/        # generation tracking for retraction
      <source>.json     # current generation per source
```

### §45 Substrate files (embedded)

```
anneal-lang/src/
  ast.rs
  parser.rs
  loader.rs          # host-neutral include/import resolution
  diagnostics.rs

anneal-core/src/prelude/
  graph.dl
  convergence.dl
  checks.dl
  ranking.dl
  views.dl
```

The language files are a private v2.0 crate boundary per CR-D51.
Prelude files are compile-time embedded; `ANNEAL_PRELUDE_PATH`
overrides them (and changes the recorded `prelude_hash`).

---

## Part XI: Migration from v1.x [CR-M]

### §46 Command mapping

Every v1.x command is reachable in v2.0:

| v1.x | v2.0 |
|---|---|
| `anneal status` | `anneal status` |
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

1. **`anneal-lang` (private).** Parser, AST, source spans, and
   host-neutral loader boundary for `anneal.dl`; not published in
   v2.0 per CR-R9.
2. **`anneal-core`.** Datalog runtime, primitives, IR, embedded
   prelude; depends on `anneal-lang`, not the reverse.
3. **`anneal-md`.** Refactor v1.x parse pipeline behind `Source`;
   while parity is being proven, shared v1 parser/config behavior may
   live in `anneal-legacy` as a transition-only library boundary
   instead of the root CLI package.
4. **`anneal-cli` + `anneal-mcp`.** Surfaces over the shared core.
5. **Dual ship.** v1.x and v2.0 binaries in parallel for one minor
   release; v1.x prints deprecation warnings.
6. **Documentation.** SKILL.md, README.md rewritten.

**Definition CR-D83 (Legacy boundary deletion gate).**
`anneal-legacy` is a transition-only crate and must be removed before
v1.0.0 unless a later explicit CR-D renews it with a narrower purpose.
The migration is complete only when the root CLI depends on
`anneal-core`/`anneal-md` directly for shipped behavior and no
production path reaches back through the legacy parser/config bridge.
Rationale: compatibility scaffolding is useful during dual-CLI
migration, but an unbounded transition crate becomes architecture by
accident.

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
    `{"identifier-substring", "title-substring", "frontmatter-key-match",
    "frontmatter-value-match"}`
  - Following `read` on that handle returns the file's `## Method`
    or `## Summary` section in first span
  - `--explain` shows the rank derivation citing both score and
    field
- *Tool-call target*: 2 calls (search + read) with `MRR ≥ 0.5`
  across cold-agent runs
- *Context target*: `anneal context "v17 conformance audit"` after
  an optional `anneal status` call returns the same audit handle and
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

### §55.1 Structural primitives (v2.1+) [CR-Fw5]

CR-D35 keeps engine primitive names sealed for v0.11.0. v2.1+ may add
a `StructuralPrimitive` registration trait for adapter-native,
read-only, no-I/O graph or topology relations such as Oban DAGs,
Phoenix routes, Ash relationships, or code-call graphs. Authorization,
capability checks, content/search I/O, and policy remain
substrate-owned; structural primitives expose deterministic relation
rows over already-visible adapter state only. This is the planned
escape hatch for host-scale traversal without turning every adapter
into a general primitive engine.

### §55.2 Typed evidence rows (v2.1+) [CR-Fw6]

`*meta` remains the v0.11.0 compatibility and adapter-evidence escape
hatch. A future `*evidence` relation should replace JSON-shaped
diagnostic values and adapter-specific meta blobs with typed slots
for evidence kind, subject, locator, related handle, observed value,
and confidence. The goal is to keep diagnostic and provenance evidence
queryable without smuggling structure through strings.

### §55.3 Unified project declaration polish (v0.12+) [CR-Fw7]

CR-D89 makes `anneal.dl` the unified project declaration file in
v0.11.0. The remaining forward-looking work is polish around richer
literal syntax and generated reference documentation, not a second
project-file migration.

The target is not "TOML embedded as strings" and not "rules compute
configuration." Configuration needed by Sources must be known before
extraction, so it cannot depend on fixpoint evaluation. The target is
a small declaration layer inside the Datalog language that lowers to
the same relational facts and SourceContext inputs the runtime already
uses.

Current unified file shape:

```dl
config convergence {
  ordering(["raw", "draft", "current", "stable"]).
  active(["draft", "current", "stable"]).
  terminal(["archived", "superseded"]).
}

config handles {
  linear(["OQ"]).
  rejected(["SHA", "GPT"]).
}

source md {
  file_extension(".md").
  scan_root(".").
  scan_exclude(["archive", "node_modules"]).
}

# ordinary project rules and verbs continue in the same file
release_blocker(h, "broken_ref") :=
  diagnostic("E001", severity, h, file, line, evidence).

@verb(
  name: "release-blockers",
  query: "? release_blocker(h, why).",
  doc: "Open blockers gating the next release.",
  output_schema: "{\"h\":\"HandleId\",\"why\":\"String\"}",
  capabilities: ["read"]
).
```

Lowering rules:

- The chosen surface is **block-scoped predicate declarations**. It
  deliberately rejects `key = value.` assignment syntax so `.dl` does
  not become a second TOML-shaped language. Inside a declaration block,
  every line is still a predicate call terminated by `.`.
- `config <section> { key(value). }` lowers to one or more `*config`
  rows with `key = "<section>.<key>"`. Scalar values lower to one row
  with `ordinal = null`.
- `config <section> { key([a, b]). }` lowers according to the core
  schema for that key. Ordered-list keys emit ordinals `0`, `1`, ...
  per CR-D40. Unordered-set keys emit one row per element with
  `ordinal = null`.
- `source <adapter-prefix> { declaration(...) }` lowers to
  adapter-qualified discovery facts (`md.label_pattern(...)`) before
  Source extraction. The block name is the adapter prefix, not a
  human-readable adapter label, so the lowering target is unambiguous.
- Declaration blocks are static. They may contain literals and lists,
  but not variables, rule bodies, primitive calls, negation, or
  aggregation.
- Core owns schemas for built-in config sections. Adapters declare
  their source-block keys through SourceInfo. Unknown sections, unknown
  keys, wrong value types, and duplicate ordered ordinals are load
  errors with source spans.
- `schema`, `describe`, and future generated reference docs expose
  these declaration schemas so an agent can discover config vocabulary
  without reading Rust source.
- After lowering, the rest of the file follows the existing
  `anneal.dl` semantics: project rules, imports, docs, and `@verb`
  declarations load after the embedded prelude per CR-D21.

Rejected alternatives:

- Raw fact rows such as `config.convergence.ordering("raw", 0).`
  expose the correct relational shape but are too verbose for
  onboarding and make ordinary config feel like storage plumbing.
- TOML-like assignments such as `ordering = ["raw", ...].` are
  familiar, but they create a second expression form and make `=` mean
  "declaration assignment" in one place and "logical equality" in
  another.
- `@config(...)` directives reuse the existing annotation shape, but
  deeply nested directive arguments recreate JSON-in-a-string pressure
  and make adapter schemas harder to read.

The next language-polish slice should resolve CR-OQ12 by adding typed
object literals for directive fields such as `@verb.output_schema`.
The unified project file works without that syntax, but
JSON-in-a-string remains the most visible wart in the authoring
surface.

Phase order in the unified-file mode:

1. Parse the project file.
2. Lower static `config` declarations and adapter discovery
   declarations.
3. Build SourceContext and extract facts.
4. Load embedded prelude.
5. Load the project's rule, import, doc, and verb declarations.
6. Evaluate queries.

Migration ladder:

- v0.11.x: `anneal.dl` is generated and documented as the canonical
  repo config. Existing `anneal.toml` files are upgrade input for
  `anneal init --force`, not a second runtime config authority.
- v0.11.x: `anneal init --dry-run` previews the unified file;
  `anneal init --force` writes `anneal.dl` and moves `anneal.toml` to
  `anneal.toml.legacy`.
- v1.0 candidate: remove the transitional TOML reader unless renewed
  by explicit decision.

Conflict rule: a corpus has one repo config authority. Existing
`anneal.toml` is accepted as input to the conversion path, but runtime
commands use `anneal.dl`. If TOML remains at runtime, the command must
fail with a migration diagnostic rather than merging two config authorities.

Rationale: this gives agents one project vocabulary and makes config
queryable as facts, while preserving the adoption lesson that language
migrations must interoperate with existing files instead of requiring
all-at-once conversion.

---

## Part XIV: Open questions [CR-OQ]

### §56 take_until aggregation behavior

Resolved by CR-D38: `TakeUntil` contribution values may be
non-numeric, `sum` must be non-negative integer, `key` gives
deterministic ordering, and threshold ties are resolved by stable
value tie-breaks rather than bucket inclusion.

### §57 Default Ranker weights [CR-OQ8]

CR-D42 locks the deterministic default ranker mechanism: per-field
weighting, clamping, low-confidence marking, and stable tie-break
order. The numeric field weights remain policy defaults. v0.11.0 ships
`identifier = 1.0`, `title = 0.95`, `frontmatter:* = 0.88`,
`body = 0.82`, and `other = 0.75`; later releases should tune these
against real cold-agent fixtures and cross-corpus smoke instead of
treating them as semantics.

### §58 Low-confidence threshold [CR-OQ9]

CR-D43 locks the selection mechanism: raw `search(...)` exposes all
rows with `low_confidence`, while surfaced `TopK` templates filter
low-confidence rows by default unless configuration or flags opt in.
The default threshold value remains a policy default. v0.11.0 ships
`0.5`; later releases may tune this without changing relation shape.

### §59 Visibility label model [CR-OQ10]

CR-D61 locks fact visibility before derivation and the built-in
`public < team < private` labels. Real host deployments may need
attribute-based or non-ordinal visibility models. v0.11.0 keeps the
three built-ins for CLI/MCP and defers richer policy label semantics
until a host adapter forces them.

### §60 Context budget allocation [CR-OQ11]

CR-D71 locks the important invariant: `context_read_budget` is a
per-hit cap and is not divided by `--hits`. The exact allocation from
the user-facing `--budget` is a tuning default. v0.11.0 uses the
current CLI heuristic; later releases should tune response-wide
packing, span density, and neighborhood reserves from fixtures.

### §61 Executable context lowering syntax [CR-OQ12]

CR-D45 locks the shipped v0.11.0 lowered `views.dl` fixture for
`context`. The open question is syntax, not behavior: future
`anneal-lang` object literals may replace canonical JSON strings in
`@verb.output_schema` while preserving CR-R4 and the same grouped
surface output.

### §62 `match` provider surface [CR-OQ13]

CR-D52 leaves `match` substrate-owned in v0.11.0. Code and host
adapters may eventually need native regex or structural match indexes.
That should land as an explicit provider or structural primitive
decision with budget, provenance, visibility, and policy semantics,
not as an implicit change to `SearchProvider`.

### §63 Trail redaction patterns

Resolved by CR-D78: the default pattern set is `secret`, `password`,
`token`, `api_key`, and `private_key`, case-insensitive and
project-overridable through trail config declarations.

### §64 Distinguishing consumed-by-read from consumed-by-display

Resolved by CR-D77: display alone is not consumption; a surfaced
handle becomes consumed when it appears in a later verb input within
the same trail session or a host records explicit selection.

### §65 MCP run_verb routing

Resolved by CR-D56: `run_verb` dispatches through the core
`VerbRegistry`. Project verbs win over prelude verbs per CR-D21 and
CR-R4 before MCP projection, and MCP exposes only the resolved name.

### §66 Performance ceiling [CR-OQ6]

For corpora with hundreds of thousands of handles and rich rule sets,
evaluation time grows. The substrate is designed for
hundreds-of-thousands. Tens of millions of handles likely need indexed
or incremental evaluation and are out of scope for v0.11.0.

### §67 Ordered config fact representation [CR-D40]

**Definition CR-D40 (Ordered config facts).** `*config` rows carry an
explicit `ordinal` field. Scalar settings and unordered sets emit
`ordinal = null`. Ordered list settings, including
`convergence.ordering`, emit zero-based ordinals that are stable
across in-memory extraction, persistence, replay, and federation.

The runtime MUST interpret ordered config from the explicit ordinal,
not from fact insertion order. If compatibility rows omit `ordinal`,
the runtime MAY preserve legacy insertion-order behavior only for
transient local evaluation; persisted or federated config facts MUST
not omit `ordinal` for ordered lists. This keeps list order visible
as data instead of smuggling it through row sequence. Serde,
persistence, and replay MUST round-trip `ordinal` exactly, including
`null`; accessors that materialize ordered config lists MUST sort by
the numeric ordinal and reject duplicate ordinals for the same ordered
config key.

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
- CR-D38: Non-count aggregation semantics (§20)
- CR-D39: Snapshot identity and fallback history semantics (§15)
- CR-D40: Ordered config facts (§67)
- CR-D41: Corpus-unique handle ids (§15)
- CR-D42: Default lexical Ranker (§12)
- CR-D43: Search selection policy (§12)
- CR-D44: Introspection tuple encoding (§43)
- CR-D45: Executable context lowering (§33.1)
- CR-D46: Documentation declarations (§43)
- CR-D47: Structural graph vocabulary (§27.1)
- CR-D48: Work ranking vocabulary (§27.2)
- CR-D49: Relational diagnostic contract (§28.1)
- CR-D50: S005 observed namespace scope (§28.3)
- CR-D51: Embeddable language boundary (§8.1)
- CR-D52: Retrieval provider boundary (§6)
- CR-D53: Fact visibility boundary (§16)
- CR-D54: Trail privacy boundary (§13)
- CR-D55: Versioned prelude package (§25)
- CR-D56: Verb projection boundary (§37)
- CR-D57: Source driver boundary (§5)
- CR-D58: Lifecycle profile examples (§27)
- CR-D59: Custom prelude package order (§25)
- CR-D60: Executable verb schema encoding (§31)
- CR-D61: Fact visibility capabilities (§16)
- CR-D62: Visibility closure over handle references (§16)
- CR-D63: Policy action gates (§16)
- CR-D64: Trail-private authorization hook (§16)
- CR-D65: Trail relation normalization (§13)
- CR-D66: Trail storage hardening (§13)
- CR-D67: Trail projection safety (§13)
- CR-D68: Source refresh transaction (§5)
- CR-D69: Corpus-scoped diagnostic locations (§28.2)
- CR-D70: Markdown scan-root identity (§42)
- CR-D71: Context per-hit read cap (§33.1)
- CR-D72: MCP launcher status (§37)
- CR-D73: Clustered child-hit parent promotion (§12)
- CR-D74: Rule layer/runtime substrate boundary (§8.1)
- CR-D75: Primitive lifecycle classes (§11)
- CR-D76: Adapter primitive extension boundary (§11)
- CR-D77: Consumed reference heuristic (§13)
- CR-D78: Default trail redaction patterns (§13)
- CR-D79: Directive reification (§17)
- CR-D80: Primitive evaluation within strata (§19)
- CR-D81: Schema signature encoding (§43)
- CR-D82: Help and reference projection (§35)
- CR-D83: Legacy boundary deletion gate (§47)
- CR-D84: Explain output row cap (§14)
- CR-D85: Empty row diagnostic (§36)
- CR-D86: Corpus vocabulary verb (§33)
- CR-D87: CLI output mode selection (§36)
- CR-D88: Aggregate body stratification (§20)
- CR-D89: Configuration ladder (§39)
- CR-D90: Namespace config is policy, not inventory (§28.3)

### CR-R (Rules)
- CR-R1: Diagnostic ID literal (§29)
- CR-R2: Unique within ruleset (§29)
- CR-R3: Reserved prefixes (§29)
- CR-R4: Verb extensibility / Steele's criterion (§31)
- CR-R5: Workflow gates with pinned fixtures (§49)
- CR-R6: Edge closure (§10)
- CR-R7: Bounded graph primitive anchors (§11)
- CR-R8: Bounded content primitive inputs (§11)
- CR-R9: Language API stabilization gate (§8.1)
- CR-R10: Visibility before derivation (§16)
- CR-R11: Stored field validation (§10)

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
- CR-Fw5: Structural primitives (§55.1)
- CR-Fw6: Typed evidence rows (§55.2)
- CR-Fw7: Unified project declaration file (§55.3)

### CR-OQ (Open questions)
- CR-OQ6: Performance ceiling (§66)
- CR-OQ8: Default Ranker weights (§57)
- CR-OQ9: Low-confidence threshold (§58)
- CR-OQ10: Visibility label model (§59)
- CR-OQ11: Context budget allocation (§60)
- CR-OQ12: Executable context lowering syntax (§61)
- CR-OQ13: `match` provider surface (§62)

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
