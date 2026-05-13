---
status: current
date: 2026-05-13
description: >
  anneal v2.0 — the master spec. A programmable knowledge-corpus runtime
  for agents and humans. Substrate (Datalog primitives + convergence
  standard library) is decoupled from sources (markdown, MDX, code,
  issue trackers, host applications). The same agent skills work across
  every corpus the substrate can ingest.
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
  facts the substrate can reason about.
- **Two surfaces**: a CLI for humans and shell scripts; an MCP server
  for agents. Both project the same runtime contracts.

What makes a system *anneal* is the convergence model — settledness as
a first-class dimension. The substrate ships that model as readable,
overridable `.dl` files. Every project can use it as-is, replace it,
or extend it for the work that matters to that team.

---

## Part I: Why this shape [CR-F]

### §1 The thing agents need

A cold agent arriving in a knowledge corpus has four problems:

1. **Localization.** "Where is the thing relevant to my task?" Today
   they enumerate filenames, scroll find output, or read whole files.
2. **Composition.** "What's the connection between these things I just
   found?" Today they string together calls, parsing output between them.
3. **Memory.** "What did the previous agent decide?" Today they read
   chat transcripts or commit messages and hope.
4. **Extension.** "I've learned my way around; how do I leave that
   knowledge behind?" Today they write a docs file no future agent
   reads.

anneal v2.0 answers each as a runtime primitive: `search` for
localization, the Datalog dialect for composition, `*trail` and
snapshots for memory, `@verb` and `anneal.dl` for extension.

### §2 Why substrate, not a markdown tool [CR-D1]

**Definition CR-D1 (Substrate).** A program that exposes a typed
knowledge-graph runtime, format-agnostic, with sources as plugins.
The substrate's value compounds across every source it can ingest;
markdown is one source among many.

Treating anneal as "the markdown corpus tool" loses three futures the
substrate makes natural:

- **MDX, AsciiDoc, org-mode, JSON-schema, YAML, code, issue trackers**
  — every format is an adapter; the convergence model and query
  language are the same.
- **Anneal embedded in another application** — host-corpus, an Ash app,
  any host that wants its own runtime state to be agent-queryable.
  The host implements a `Source`; the runtime serves the same agents.
- **Multi-corpus federation** — an agent works across several corpora
  simultaneously (a project's design docs, its source code, its
  related upstream projects). Handle namespaces compose.

The substrate also future-proofs against agent evolution: today's
agents have specific shapes (token budgets, no shared memory). The
primitives, self-description, trails, and programmable extension
survive any agent generation.

### §3 The cold-agent test [CR-D2]

**Definition CR-D2 (Cold-agent test).** Given a real corpus and a
goal, a cold agent (no prior session memory of the corpus) reaches
the answer in ≤2 tool calls plus optional `--explain`.

This is the product's primary acceptance criterion. Engine viability
(MVS coverage, perf gates) is necessary but not sufficient. If a
v2.0 runtime ships passing MVS and the cold-agent test fails on
real workflows, we shipped a smaller-but-harder shell.

---

## Part II: Architecture [CR-A]

### §4 Three layers [CR-D3]

**Definition CR-D3 (Layering).** anneal v2.0 decomposes into three
layers with sharp responsibilities:

| Layer | Form | Responsibility |
|---|---|---|
| **Substrate** | `anneal-core` Rust crate | Datalog runtime, primitives, convergence standard library, provenance, IR. No source-specific code. |
| **Adapters** | `anneal-md`, `anneal-mdx`, `anneal-code`, `anneal-host`, … | Format-specific extraction: parse source → emit handle/edge/content/meta facts via the `Source` trait. |
| **Surfaces** | `anneal-cli` binary, `anneal-mcp` server, library API | Project the substrate to humans, agents, and host applications. |

The substrate is the product. Adapters and surfaces are how it
reaches users.

### §5 Crate topology

```
anneal/
├── crates/
│   ├── anneal-core/             # the substrate
│   │   ├── runtime/             # Datalog IR + evaluator
│   │   ├── primitives/          # stored relations + function primitives
│   │   ├── prelude/             # convergence stdlib as embedded .dl
│   │   └── source/              # the Source trait + FactSink
│   ├── anneal-md/               # markdown adapter
│   ├── anneal-cli/              # the binary; links core + md
│   └── anneal-mcp/              # the MCP server; links core + md
├── adapters/                    # external adapter crates
│   ├── anneal-mdx/              # (v2.1)
│   ├── anneal-code/             # (v2.1)
│   └── anneal-host/             # host-embed helpers (v2.1)
└── .design/
```

`anneal-core` is the only crate other anneal crates depend on. Adapters
are siblings, not children. A consumer can link any combination of
adapters into their own binary; the CLI ships markdown by default.

### §6 The Source trait [CR-D4]

**Definition CR-D4 (Source trait).** The contract every adapter
implements. Source is the *only* extensibility point at the
adapter-substrate boundary.

```rust
pub trait Source {
    /// Extract handle/edge/content/meta facts for whatever this Source
    /// recognizes (a directory, a file, a database, an API, etc.).
    ///
    /// Adapters push facts into the FactSink; the runtime stores them
    /// in the appropriate relations and triggers fixpoint evaluation.
    fn extract(&self, sink: &mut FactSink) -> Result<(), SourceError>;

    /// Self-describe what this source recognizes. Returns enough for
    /// `anneal sources` to list available adapters and for the runtime
    /// to choose between them by config or auto-detection.
    fn describe(&self) -> SourceInfo;
}

pub struct FactSink<'a> {
    // …implementation detail; exposes push_handle / push_edge / push_content / push_meta
}

pub struct SourceInfo {
    pub name: &'static str,       // "markdown", "mdx", "rust-code", "github-issues"
    pub recognizes: Vec<Pattern>, // ["**/*.md"], ["**/*.mdx"], ["src/**/*.rs"]
    pub doc: &'static str,
    pub config_keys: Vec<&'static str>,
}
```

A `Source` is one of:

- a directory walker that emits per-file handles + cross-file edges
  (markdown, MDX, AsciiDoc)
- a source-code analyzer that emits per-function/module handles +
  call edges (anneal-code)
- an external-system reader (issue tracker API, CI events, deployment
  log)
- a host application's runtime introspector (anneal-host: host-corpus's
  Ash resources, Phoenix routes, Oban jobs as handles)

The runtime is identical across sources. Only the extraction differs.

### §7 The substrate's ingestion lifecycle

```
1. Surface (CLI/MCP/library) picks Sources based on config or args
2. For each Source: Source.extract(sink) emits facts
3. Runtime stores facts in stored relations (*handle, *edge, etc.)
4. Runtime loads prelude + project anneal.dl into the IR
5. IR runs fixpoint over loaded rules
6. Surface accepts queries; runtime evaluates against the populated relations
7. Optional: trail capture writes the session's path to *trail
```

Steps 1-5 are "ingestion." Steps 6-7 are "query." A long-running
runtime (MCP server, embedded host) keeps the populated relations in
memory and re-ingests on source changes.

---

## Part III: Substrate primitives [CR-P]

### §8 Stored relations [CR-D5]

**Definition CR-D5 (Stored primitives).** The relations every adapter
populates and every rule may join on.

```
*handle{id, kind, status, namespace, file, line, date, area, summary}
*edge{from, to, kind, file, line}
*meta{handle, key, value}
*content{handle, span_id, lines, text, tokens}
*span{id, handle, start_line, end_line, summary}
*concern{name, member, source}
*config{key, value}
*snapshot{at, id, key, value}
*trail{session_id, step, expr, summary}
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
| `*trail` | Session paths: search → candidates → reads → conclusions |

A `Source` populates `*handle`, `*edge`, `*meta`, `*content`, `*span`,
`*concern`. `*config` is populated from `anneal.toml`. `*snapshot` is
populated from `.anneal/history.jsonl`. `*trail` is populated by the
runtime as queries execute.

Every stored relation is **format-agnostic.** Whether the source is a
markdown file or a Rust function, a handle's shape is the same.

### §9 Engine-derived predicates [CR-D6]

**Definition CR-D6 (Function primitives).** Predicates implemented in
the substrate (not as Datalog rules) because they need Rust-native
traversal, IO, ranking, or content access.

```
// Graph
upstream(h, anc)               // transitive depends_on
downstream(h, desc)
impact(h, x, depth)            // reverse closure, configured edge set
neighborhood(h, depth, member) // bounded subgraph

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
flux(h, days: N, delta)
token_estimate(h, n)

// Content retrieval
search(query, hit)              // hit row: handle, span_id, score, reason
read(h, budget, span)           // budget-bounded slice; emits span rows
read_full(h, content)           // entire file (use sparingly)
match(pattern, h, line)         // regex over content; line-level

// Self-description
schema(name, kind, signature)   // every relation + predicate with arity
predicates(name, doc, source)   // rule-defined predicates
verbs(name, query, doc)         // every verb (engine + prelude + project)
describe(name, doc)             // doc string for any of the above
source(name, file, lines)       // where a predicate is defined
examples(name, example)         // worked examples per predicate
sources(name, recognizes, doc)  // adapters available at runtime

// Composition helpers
top_k(k, key, body)             // bounded selection: top k by key
rank(handle, key)               // deterministic rank within body
```

These are not optional. `search` and `read` are as load-bearing as
`*handle` and `*edge` — together they answer "where is X" and "what
does X say," which are 80% of cold-agent moves.

### §10 Provenance is universal [CR-D7]

**Definition CR-D7 (Provenance contract).** Every output record can be
expanded via `--explain` (CLI) or `derivation: true` (MCP) into a
derivation tree:

- which `search_hit` rows brought a handle into consideration, with
  their scores, reasons, and matched fields
- which `*content` spans the engine consulted
- which `*edge` rows joined to produce each derived fact
- which `*meta` / status values participated
- which rule chain (prelude, project, inline `where`) fired

Provenance is produced by the IR during evaluation, not by per-rule
companion relations (the spike found those don't scale). The IR
tracks fact derivation as it computes the fixpoint and surfaces the
derivation tree on demand.

### §11 Snapshot and time travel [CR-D8]

**Definition CR-D8 (`at(<ref>)` block).** A body fragment that scopes
its sub-body to evaluate against historical corpus state. Inside the
block, stored relations read from the snapshot; engine-derived
predicates re-evaluate against that state.

References:

| Form | Mechanism | Cost |
|---|---|---|
| `at("snapshot:last")` | read `.anneal/history.jsonl` | <100ms |
| `at("snapshot:<id>")` | indexed history.jsonl lookup | <100ms |
| `at("--7days")` | resolve to nearest snapshot | <100ms |
| `at("2026-04-01")` | ISO date → nearest snapshot | <100ms |
| `at("HEAD~3")` / `at("v0.2.1")` / `at("<sha>")` | git ref: re-run all Sources against that commit | O(corpus) |

The `--at=<ref>` global flag is sugar for wrapping the entire query in
an `at(<ref>) { ... }` block.

### §12 Trails [CR-D9]

**Definition CR-D9 (Trail).** A session's path through the substrate
— search hits, reads, derived conclusions, verification queries —
written to `*trail` by the runtime. Trails are the unit of handoff
between sessions and between agents.

A trail entry carries:

- `session_id`: opaque identifier (uuid; auto-generated unless host
  supplies one)
- `step`: monotonic ordinal within the session
- `expr`: the query expression or verb invocation
- `summary`: short text describing what was learned (engine-generated
  from the first N rows + score distribution)

Trails persist to `.anneal/trails/<session-id>.jsonl` on session end
and are queryable as a normal relation. An agent picking up another
agent's work can ask `? *trail{session_id, step, expr, summary}.` to
recover the prior path.

Trail capture is mandatory in v2.0; trail-driven workflows (replay,
diff, merge) are forward-looking (v2.1+).

---

## Part IV: The language [CR-L]

### §13 Grammar

Modern Datalog. Named fields on stored relations, lowercase identifiers,
`:=` for "is true when," `?` for queries, `*relation{...}` for stored
data.

```
program     := statement*
statement   := fact | rule | query | directive

fact        := head '.'
rule        := head ':=' body '.'
query       := '?' [local_rules] body '.'
directive   := 'include' string '.'
             | 'at' '(' string ')' '{' statement* '}'
             | '@verb' '(' verb_args ')'

head        := ident '(' arg_list ')'
local_rules := ('where' rule)+
body        := atom (',' atom)*
atom        := stored | derived | comparison | aggregation | negation | time_block
stored      := '*' ident '{' field_list '}'
derived     := ident '(' arg_list ')'
comparison  := value op value
negation    := 'not' (stored | derived)
aggregation := value '=' agg_fn '{' var ':' body '}'
time_block  := 'at' '(' string ')' '{' body '}'

field_list  := field (',' field)*
field       := ident                        # bind: same name as variable
             | ident ':' value_or_var       # bind: explicit
arg_list    := value_or_var (',' value_or_var)*
value_or_var := var | literal | '_'
var         := /[a-z_][a-z0-9_]*/
literal     := string | number | bool | list

agg_fn      := 'Count' | 'Sum' | 'Min' | 'Max' | 'Avg' | 'List' | 'Set'
             | 'TopK' | 'Rank'   # composition helpers
op          := '=' | '!=' | '<' | '>' | '<=' | '>='
             | 'in' | 'matches' | 'contains'
             | 'starts_with' | 'ends_with'
ident       := /[a-z_][a-z0-9_]*/
```

Comments: `#` to end of line. Whitespace insignificant. Statements
terminated by `.`. Strings double-quoted with standard escapes.

### §14 Types and operators

Dynamic, four primitive types plus one composite:

| Type | Literal |
|---|---|
| String | `"OQ-37"` |
| Number | `42`, `3.14` (unified i64/f64) |
| Bool | `true`, `false` |
| Null | `null` |
| List | `[1, 2, 3]`, `["raw", "decided"]` |

No first-class records. Records exist only as patterns inside
`*relation{...}`. This keeps the language small and avoids the
equality-of-records question.

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

### §15 Stored vs derived predicates [CR-D10]

**Definition CR-D10 (Stored).** A relation prefixed `*` reads from
facts produced by Sources during ingestion or from configuration
(`*config`, `*snapshot`, `*trail`). Pattern-matching a stored relation
binds field values to variables.

**Definition CR-D11 (Derived).** A relation without `*` is defined by
one or more rules. Rules may live in the prelude (substrate stdlib),
in `anneal.dl` (project vocabulary), or inline via `where` clauses.

The `*` prefix is a visible marker: *this is real data, not derived.*
It tells an agent reading a query whether they're looking at corpus
state or computed inferences.

### §16 Aggregation

Form: `agg_var = AggFn{ contributing_var : sub_body }`.

```
total_potential(area, total) :=
  total = Sum{ e : potential(h, e), area_of(h, area) }.

namespace_open_count(ns, n) :=
  n = Count{ h : *handle{id: h, namespace: ns},
                 obligation(h),
                 not discharged(h) }.

newest_in_area(area, latest) :=
  latest = Max{ d : *handle{kind: "file", area, date: d}, active(_) }.

top_blockers(h, score) :=
  (h, score) = TopK{ 10 :
    *handle{id: h, namespace: "OQ"},
    not discharged(h),
    potential(h, score)
  }.
```

Standard Datalog aggregation semantics: compute the set of values for
the contributing variable such that the sub-body holds, then collapse
with the aggregation function. Free variables outside the aggregation
form the grouping key. `TopK` and `Rank` are first-class — set
semantics alone aren't enough for agent retrieval workflows.

### §17 Negation, recursion, stratification [CR-D12]

**Definition CR-D12 (Stratified negation).** The runtime partitions
rules into strata such that any predicate referenced under `not` is
fully derived in an earlier stratum. Mutual recursion through
negation is rejected at load time with the cycle named.

Recursion without negation is allowed and uses fixed-point evaluation:
the engine applies rules until no new facts are derived, then stops.

Safety rules (enforced at load):

1. Every variable in a rule head must appear positively in the body
2. Every variable inside `not P(...)` must be bound positively
   elsewhere in the same rule
3. No mutual recursion through negation (engine names the cycle)

Load error example:

```
error: cyclic negation between 'blocked' and 'advancing'
  blocked/1 (anneal.dl:48) → not advancing(h)
  advancing/1 (anneal.dl:55) → not blocked(h)
fix: derive both from a non-mutually-negated common predicate.
```

### §18 Inline rules and includes

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
include "/abs/path/to/team-vocab.dl".
```

Resolved relative to the file containing the `include`. No
namespacing — rules merge into the global predicate space. Conflicts
form multi-clause definitions, with shadowing warnings consistent with
prelude/project boundary behavior (§22).

### §19 Output shape [CR-D13]

**Definition CR-D13 (Output contract).** Queries emit NDJSON to
stdout, one record per match, streamed as derived. No whole-result
buffering. Field names come from the head's variables (or for
headless queries, from the body's bindings, last-mention-wins).

```
hot_handle(h, energy) := potential(h, energy), energy > 5.
? hot_handle(h, e).
```

Output:

```
{"h":"OQ-37","e":12}
{"h":"formal-model/v17.md","e":8}
```

Cardinality: set semantics by default (duplicates deduplicated). For
multiset, use explicit aggregation or include the full key in the
head.

Provenance: `--explain` (CLI) or `derivation: true` (MCP) adds a
`_derivation` field to each record. Without it, records are bare.

### §20 Errors

Three categories, all to stderr, all with file:line context:

**Parse errors** (load):
```
anneal.dl:42:15: expected ',' or '.', got '{'
  potential(h, energy {
                      ^
```

**Static errors** (load): safety violations, stratification cycles,
unknown predicates with did-you-mean suggestions, reserved diagnostic
IDs.

**Runtime errors** (evaluation): regex compile failures, time-travel
ref not found, division by zero.

All three exit with code 1. Stdout stays clean — no partial NDJSON
if a query failed mid-evaluation.

---

## Part V: Standard library [CR-S]

### §21 Layout

The substrate embeds the standard library at compile time and exposes
it for inspection:

```
anneal-core/src/prelude/
  graph.dl          # structural shapes (orphan, stub, hub)
  convergence.dl    # potential, entropy, blocked, advancing, weights
  checks.dl         # E001, E002, W001-W004, I001, S001-S005
  ranking.dl        # tier, weighted_in_degree, curated_hub
  views.dl          # the starter verbs as saved queries
```

Path discovery: `anneal source convergence` prints the file:lines
where the convergence vocabulary lives. The user can `cat` it directly
to learn the model.

`ANNEAL_PRELUDE_PATH` overrides the embedded prelude at runtime, for
projects that want to fork the standard library entirely.

### §22 Load order and shadowing [CR-D14]

**Definition CR-D14 (Load order).** On every evaluation the runtime
loads, in order:

1. The embedded prelude (`graph.dl`, `convergence.dl`, `checks.dl`,
   `ranking.dl`, `views.dl`)
2. `anneal.dl` in the corpus root, if present
3. Inline rules from `where` clauses in the current query, if any
4. The query itself

Later layers shadow earlier layers by predicate name. **Shadowing is
total replacement**, not merge. To extend a prelude predicate, the
user provides multiple clauses for the same head in `anneal.dl`;
multi-clause definitions union as Datalog naturally does. When
`anneal.dl` defines a name the prelude also defines, the engine warns
at load on stderr:

```
warning: anneal.dl:42: 'blocked/1' overrides prelude (2 clauses)
         compare: anneal source blocked
```

### §23 Convergence vocabulary [CR-D15]

**Definition CR-D15 (Convergence vocabulary).** The predicates defined
in `convergence.dl` that name the convergence-physics concepts. The
contract between the convergence frame and project predicates.

```
# convergence.dl

# Weights — projects can retune in anneal.dl.
potential_weight("undischarged",     5).
potential_weight("broken_ref",       4).
potential_weight("stale_dep",        3).
potential_weight("confidence_gap",   3).
potential_weight("freshness_decay",  2).
potential_weight("missing_meta",     1).
potential_weight("orphan_label",     1).

# Total potential = weighted sum of entropy sources.
potential(h, energy) :=
  energy = Sum{ w : entropy(h, source), potential_weight(source, w) }.

# Entropy sources — each rule names a kind of unsettled work.
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

# Blocked: high potential, no recent change.
blocked(h) :=
  active(h),
  potential(h, energy), energy >= 3,
  flux(h, days: 30, delta: 0).

# Advancing: actively moving forward in the lattice.
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

| Predicate | Meaning |
|---|---|
| `potential(h, e)` | numeric measure of unsettled work on `h` |
| `entropy(h, source)` | a specific source of disorder on `h` |
| `blocked(h)` | high potential, no recent flux |
| `advancing(h)` | recently moved forward in the lattice |
| `terminal(h)` | settled (substrate primitive — no convergence-layer alias) |

The convergence model works on **any handle graph**, not just
markdown corpora. A code corpus where handles are functions/types
gets the same `potential` and `blocked` semantics — provided the
project's `anneal.dl` defines an appropriate lattice and obligation
conventions for that source.

### §24 Check rules [CR-D16]

**Definition CR-D16 (Check rule).** A rule whose head is
`diagnostic(...)` deriving a fact representing a consistency
violation. The runtime collects diagnostics across prelude and
`anneal.dl`, exposes them through `diagnostic/6` for queries, and
provides them to verbs like `broken` and to the dashboard.

The seven check rules from anneal v1.x — E001 (broken refs), E002
(undischarged), W001-W004 (warnings), I001 (info), S001-S005
(suggestions) — live in `checks.dl` as Horn clauses, not in
substrate Rust code. The substrate has no hard-coded check logic.

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

# … E002, W002, W003, W004, I001, S001-S005 same shape.
```

Every diagnostic-emitting rule must have a string literal in arg 1.
The runtime enforces this at load (§25).

### §25 Diagnostic ID rules [CR-R1, CR-R2, CR-R3]

**Rule CR-R1 (Diagnostic ID literal).** Every rule whose head is
`diagnostic(...)` must have a string literal as the first argument.
A variable in arg 1 is a load error.

**Rule CR-R2 (Unique within ruleset).** Two rules with the same
diagnostic ID literal in the same loaded ruleset error at load with
both file:line locations.

**Rule CR-R3 (Reserved prefixes).** The prefixes `E*`, `W*`, `I*`,
`S*` are prelude-owned. User rules in `anneal.dl` using these
prefixes error at load. Projects pick their own prefix (`PROJ-001`,
`RELEASE-002`, `TEAM-007`).

Load error example:

```
error: anneal.dl:84: diagnostic ID 'E099' is in reserved range E*
       use a project-specific prefix (e.g. 'PROJECT-099')
```

This makes diagnostic ID stability mechanical and validated.

---

## Part VI: Project extension [CR-E]

### §26 `anneal.dl` conventions

Project predicates live in `anneal.dl` at the corpus root. Section
headers organize the file:

```dl
# === handles ===
file_extension(".md").
label_pattern("OQ",    "OQ-(\d+)",    "any").
label_pattern("KB-D",  "KB-D(\d+)",   ".design/**").
linear_namespace("OQ").
version_pattern("formal-model", "formal-model-v(\d+)\.md").

# === overrides ===
# Override the prelude's freshness threshold for this corpus.
entropy(h, "freshness_decay") :=
  *handle{id: h, kind: "file"},
  active(h), freshness(h, days), days > 30.   # 30 instead of 60

# === project predicates ===
# OQs become "blocking" when a formal-status doc transitively depends on them.
blocking_oq(q) :=
  *handle{id: q, kind: "label", namespace: "OQ", status: "open"},
  upstream(spec, q),
  *handle{id: spec, status: "formal"}.

# Release blockers — what must clear before v0.3.
release_blocker(h, "broken_ref")   := diagnostic("E001", _, h, _, _, _).
release_blocker(h, "undischarged") := diagnostic("E002", _, h, _, _, _).
release_blocker(h, "blocking_oq")  :=
  blocking_oq(h),
  *meta{handle: h, key: "milestone", value: "v0.3"}.

# === verbs ===
@verb(
  name: "release-blockers",
  query: ? release_blocker(h, why).,
  doc: "Open OQs and broken references gating the next release."
)
```

Discovery facts (`file_extension`, `label_pattern`, etc.) are
consumed by the appropriate `Source` adapter at parse time. Rules
load into the IR. Verbs register with the surface layer.

### §27 Steele's criterion for verbs [CR-R4]

**Rule CR-R4 (Verb extensibility).** A verb defined in `anneal.dl`
via `@verb(...)` is syntactically indistinguishable from a verb
shipped in the prelude. Identical:

- Discovery: `anneal verbs` lists both
- Help: `anneal describe <verb>` works for both
- Output envelope: same NDJSON shape, same `--explain` support
- Callable shape: a rule body can reference either verb's underlying
  predicate
- Documentation surface: worked `examples` work for both

There is no privileged distinction between built-in verbs and
project verbs at runtime. Project-defined `release-blockers` carries
the same affordances as prelude-defined `blocked`.

### §28 Discovery facts and the Source

Discovery facts in `anneal.dl` are read by Sources during ingestion,
not by the IR. The markdown `Source` (`anneal-md`) reads
`file_extension`, `label_pattern`, `linear_namespace`,
`version_pattern`, `scan_exclude`, `section_min_depth`,
`section_max_depth` to control its parse.

Other adapters read their own discovery facts. The `anneal-code`
adapter might read `code_language`, `module_pattern`,
`coverage_source`. The contract: an adapter declares which
`config_keys` it consumes (via `SourceInfo`); the runtime warns at
load if a config key is unrecognized by any registered adapter.

---

## Part VII: Surfaces [CR-Su]

### §29 The starter verbs

The prelude's `views.dl` ships these saved expressions. Projects
override or extend any of them.

| Verb | Question | Underlying expression (sketch) |
|---|---|---|
| `anneal` | where am I | `? dashboard().` (composed of summary, work, advancing, blocked) |
| `anneal H` | what is this handle | `? *handle{id: H, ...}, *edge{from: H, ...}, *edge{to: H, ...}.` |
| `anneal find TEXT` | where's the thing called TEXT | `? *handle{id, ...}, id contains "TEXT".` |
| `anneal search TEXT` | what content matches TEXT | `? search("TEXT", hit), hit.handle, hit.span_id, hit.score.` |
| `anneal read H` | give me H's content, bounded | `? read("H", budget: 4000, span).` |
| `anneal work` | where should I work | `? potential(h, e), entropy(h, src). top_k 25 by e.` |
| `anneal blocked H` | what's blocking H | `? entropy("H", source), entropy_detail("H", source, d).` |
| `anneal trend` | which way is the corpus moving | comparison of `at(--at) { ... }` vs `at("now") { ... }` |
| `anneal broken` | are there errors | `? diagnostic(_, "error", _, _, _, _).` |

Plus the self-description verbs from §9: `schema`, `predicates`,
`verbs`, `describe`, `source`, `examples`, `sources`.

Plus the meta forms:

| Form | Purpose |
|---|---|
| `anneal -e '<q>'` | custom query; `-e -` reads from stdin |
| `anneal init` | scaffold a corpus with starter `anneal.toml` + `anneal.dl` |
| `anneal --prelude-path` | print the embedded-prelude inspection path |
| `anneal --inspect S` | parse-test a string against handle conventions |

Every verb prints its underlying expression above its result.

### §30 CLI flags

Operational only — never query-shaping. Filters belong in queries.

| Flag | Effect | Scope |
|---|---|---|
| `--root=PATH` | operate on a different corpus | global |
| `--at=<ref>` | evaluate at a historical reference | global |
| `--limit=N` | cap output records | global |
| `--explain` | include `_derivation` per record | global |
| `--no-snapshot` | don't append history on this run | global |
| `--quiet` | suppress stderr chatter | global |
| `--budget=N` | token budget for `work` / `read` | `work`, `read` |
| `--gate` | exit 1 if any results | `broken` |
| `--source=NAME` | restrict ingestion to one Source | global |
| `--mcp` | start as an MCP server on stdin/stdout | global |

### §31 I/O contract [CR-D17]

**Definition CR-D17 (I/O contract).**

- **stdout: NDJSON.** One record per line, `\n` terminated, streamed
  as derived. Verbs that produce a single nested record (the
  dashboard) emit one line; every other verb is a stream of
  homogeneous records. No envelope unless `--meta`.
- **stderr: human text.** Progress, warnings, parse errors with
  line/column. Never NDJSON.
- **stdin: `-` means stdin.** `anneal blocked -` reads handles, one
  per line. `anneal -e -` reads a query (heredoc-friendly).
- **Exit codes:** 0 success (including empty results), 1 query
  error, 2 invocation error, 3 gate failure (`--gate` with non-empty
  results). Empty results are not errors.
- **`--color=auto`** detects TTY. Pipes always get plain text.

### §32 MCP surface [CR-D18]

**Definition CR-D18 (MCP transport).** `anneal --mcp` (or the
separate `anneal-mcp` binary) starts a stdio MCP server exposing
every primitive and verb as an MCP tool. Tool schemas are generated
from the substrate's `schema()`, `describe()`, and `verbs()`
primitives — no manually-maintained tool list.

MCP tools project the substrate identically to the CLI:

- `tools/list` returns every verb + the `eval` tool (for custom
  queries) + the introspection tools (`schema`, `describe`, `source`,
  `examples`).
- Each tool's input schema is generated from the verb's arg list.
- Tool output is the same NDJSON shape as the CLI, wrapped per MCP
  conventions.
- Server instructions include the standard-library prelude — agents
  read it as the system prompt.

The MCP server is a v2.0 deliverable, not a follow-up. If anneal is
for agents, MCP is the primary surface.

---

## Part VIII: Onboarding [CR-O]

### §33 Lattice-on default [CR-D19]

**Definition CR-D19 (Init defaults).** `anneal init` always scaffolds
a minimal lattice and a starter `anneal.dl` referencing the prelude's
convergence vocabulary. Output:

```
$ anneal init

scanning corpus...
  found 47 markdown files
  inferred Source: anneal-md
  status frontmatter: present in 41/47 (87%)
  inferred lattice: raw → draft → current → stable

wrote anneal.toml
  [convergence]
  ordering = ["raw", "draft", "current", "stable"]
  active = ["draft", "current", "stable"]
  terminal = ["superseded", "archived"]

wrote anneal.dl (15 lines)
  # === handles ===
  # === verbs ===
  # …

next steps:
  anneal                   see the landscape
  anneal source convergence  read what convergence means here
  anneal work                pick where to work
```

The agent's first session lands in convergence mode, not graph mode.
The standard library is loaded; the convergence vocabulary works
out of the box. Projects with sparse frontmatter get a one-line
suggestion to populate it, not a configuration gate.

Graph mode (lattice-off) is reachable via `[convergence] disabled =
true` in `anneal.toml` — an explicit opt-out for projects that
genuinely want a graph DB without the convergence physics.

### §34 The agent loop [CR-D20]

**Definition CR-D20 (Agent loop).** The five-step cycle an agent
executes against a v2.0 corpus:

```
1. anneal              see the landscape
2. anneal work         pick where to work
3. anneal blocked H    understand why H isn't moving
4. (do the work)
5. anneal trend        confirm potential dissipated
```

For arrival on an unfamiliar corpus, prepend:

```
0a. anneal sources     what adapters are loaded
0b. anneal source convergence  what does convergence mean here
```

For multi-session handoff, prepend:

```
0c. anneal -e '? *trail{session_id: last, step, expr, summary}.'  what did the prior agent do
```

---

## Part IX: Handle model [CR-H]

### §35 Kinds

Five handle kinds are substrate-shaped:

| Kind | Examples by Source |
|---|---|
| `file` | markdown file (anneal-md), MDX file (anneal-mdx), Rust module (anneal-code), Ash resource (anneal-host) |
| `section` | markdown heading (anneal-md), MDX heading, Rust impl block, Phoenix scope |
| `label` | OQ-22 (anneal-md frontmatter), RFC-101 (anneal-code attribute), GitHub issue #42 (anneal-issues) |
| `version` | versioned spec like `formal-model-v17.md`, semver-tagged release |
| `external` | URL, external API reference, dependency |

The Source decides how to map its native concepts onto these five
kinds. `anneal-host` for host-corpus might map Ash resources to `file`,
actions to `section`, decision-log entries to `label`. The Datalog
query layer doesn't care about the mapping — it sees handles.

### §36 Discovery configuration

Each Source declares the discovery facts it consumes. For
`anneal-md`:

```
# === handles ===
file_extension(".md").
file_extension(".mdx").              # if anneal-mdx is also loaded

scan_root(".").
scan_exclude("node_modules").
scan_exclude(".git").

label_pattern("OQ",    "OQ-(\d+)",    "any").
label_pattern("KB-D",  "KB-D(\d+)",   ".design/**").

linear_namespace("OQ").
linear_namespace("P").

version_pattern("formal-model", "formal-model-v(\d+)\.md").
external_in_frontmatter("references").

section_min_depth(1).
section_max_depth(3).
```

`anneal-code` would declare its own (`code_language`, `module_path`,
`coverage_source`, etc.). The runtime warns at load if a corpus's
`anneal.dl` declares facts no loaded Source recognizes.

### §37 Introspection

Five ways an agent learns the handle landscape:

```
# 1. Counts by kind
anneal -e '? c = Count{*handle{kind: k, ...}}, k.'

# 2. Label namespaces and counts
anneal -e '? c = Count{*handle{kind:"label", namespace: ns, ...}}, ns.'

# 3. The corpus's discovery conventions
anneal -e '? label_pattern(ns, regex, scope).'

# 4. Inspect a specific string
anneal --inspect "OQ-99"

# 5. Read the file directly
cat anneal.dl
anneal describe handles
```

---

## Part X: Files and layout [CR-FL]

### §38 Project files

```
<corpus>/
  anneal.toml           # engine config: lattice, scan rules, snapshot path
  anneal.dl             # discovery facts + project predicates + verbs + overrides
  .anneal/
    history.jsonl       # snapshot append log
    trails/             # session paths
      <session-id>.jsonl
```

### §39 Substrate files (embedded)

The `anneal-core` crate embeds the standard library at compile time.
The runtime exposes it through `source(name)` (file:line within the
embedded resource map). Users can dump it with `anneal source <name>`
or override entirely via `ANNEAL_PRELUDE_PATH`.

```
anneal-core/src/prelude/
  graph.dl
  convergence.dl
  checks.dl
  ranking.dl
  views.dl              # the starter verbs as saved queries
```

### §40 Snapshots

Snapshot semantics from anneal v1.x are preserved. Snapshots are
appended to `.anneal/history.jsonl` by every `anneal`, `anneal trend`,
and `anneal broken` invocation (suppressed by `--no-snapshot`). The
`*snapshot{at, id, key, value}` stored relation exposes them to
queries; `at(<ref>) { ... }` blocks read from them transparently.

---

## Part XI: Migration from v1.x [CR-M]

### §41 Command mapping

Every v1.x command is reachable in v2.0:

| v1.x | v2.0 |
|---|---|
| `anneal status` | `anneal` |
| `anneal status --json --compact` | `anneal --limit=summary` (or just `anneal`) |
| `anneal get H` | `anneal H` |
| `anneal get H --refs` | `anneal H` (refs included by default) |
| `anneal get H --trace` | `anneal H --explain` |
| `anneal find TEXT` | `anneal find TEXT` (unchanged: identity search) |
| `anneal find --namespace=OQ --status=open` | `anneal -e '? *handle{kind:"label", namespace:"OQ", status:"open"}.'` |
| (no v1.x equivalent) | `anneal search TEXT` (content retrieval, new) |
| `anneal check` | `anneal broken` (errors) or `anneal -e '? diagnostic(c, s, ...).'` |
| `anneal check --errors-only` | `anneal broken --gate` |
| `anneal check --file=X` | `anneal -e '? diagnostic(c, s, h, "X", l, e).'` |
| `anneal map` | `anneal -e '? *edge{from, to, kind}.'` |
| `anneal map --around=H --depth=2` | `anneal -e '? neighborhood("H", 2, x).'` |
| `anneal impact H` | `anneal -e '? impact("H", x, depth).'` |
| `anneal obligations` | `anneal -e '? obligation(h), disposition(h).'` |
| `anneal diff` | `anneal trend` |
| `anneal diff HEAD~3` | `anneal trend --at=HEAD~3` |
| `anneal query handles --kind label` | `anneal -e '? *handle{kind:"label", ...}.'` |
| `anneal areas` | `anneal -e '? area_health(area, grade, ...).'` |
| `anneal orient` | `anneal work` |
| `anneal orient --budget=50k` | `anneal work --budget=50k` |
| `anneal orient --file=F` | `anneal -e '? candidate_for_file("F", c, tier, score). top_k 50 by score.'` |
| `anneal garden` | `anneal -e '? maintenance_task(t, category, blast).'` |
| `anneal init` | `anneal init` (now lattice-on by default; see §33) |
| `anneal prime` | `anneal help` or `anneal describe runtime` |

### §42 Migration path

1. **Implement `anneal-core`.** The Datalog runtime, primitives, IR,
   and embedded prelude. Estimate: ~3-4kloc.
2. **Implement `anneal-md`.** The markdown Source adapter. Mostly
   refactoring the v1.x parse pipeline behind the `Source` trait.
3. **Implement `anneal-cli` and `anneal-mcp`.** Surfaces over the
   shared core.
4. **Dual-CLI deprecation.** Ship v1.x and v2.0 binaries in parallel
   for one minor release; v1.x prints deprecation warnings on every
   invocation; the v2.0 binary is opt-in via flag. Next minor release
   removes v1.x.
5. **Documentation.** `skills/anneal/SKILL.md` rewritten around the
   primitives + language + verbs surface. `README.md` rewritten with
   the agent loop and the substrate framing.

### §43 What stays unchanged

The core model from `anneal-spec.md` Parts I-III is preserved:

- Handle definition (KB-D1) and the five kinds (KB-D2)
- Graph construction (KB-D5, KB-D6)
- Convergence lattice (KB-D7-KB-D12)
- Local check semantics (KB-D13)
- Linearity (KB-D15)
- Impact analysis (KB-D16)
- Convergence tracking (KB-D17, KB-D18, KB-D19)
- Design principles (KB-P1 through KB-P9)
- Theoretical lineage (KB-F1 through KB-F5)

The model is sound. What changes is the runtime that exposes it,
who can ingest it, and how agents extend it.

---

## Part XII: Acceptance [CR-Acc]

### §44 Workflow-completion gates [CR-R5]

**Rule CR-R5 (Workflow gates).** v2.0 ships when these cold-agent
workflows pass on a real corpus (large-corpus's `.design/`):

| Workflow | Target |
|---|---|
| "What's the corpus state?" | 1 tool call (`anneal`) |
| "Find the thing relevant to my task" | 2 tool calls (`anneal search "..."` + `anneal read H`) |
| "What does X depend on?" | 2 tool calls (`anneal H` showing refs, or `-e upstream(...)`) |
| "What changed in the last week?" | 1 tool call (`anneal trend --at=--7days`) |
| "Why is this fact in the output?" | 1 tool call (`--explain` on the prior call) |
| "Extend the vocabulary for my corpus" | Write 5 lines in `anneal.dl`; new verb available next invocation |
| "Recover what a prior agent did" | 1 tool call (`anneal -e '? *trail{...}.'`) |

These replace MVS coverage as the SP-DR1 capstone.

### §45 Substrate validation

MVS-1..9 from the engine-spike spec validate the substrate's ability
to execute the rule layer. Workflow gates above validate that the
substrate makes agent work efficient. Both must hold.

### §46 Performance gates

Per SP-R1 in `2026-05-07-engine-spike-and-parity-protocol.md`:

| Sub-criterion | Target |
|---|---|
| Cold full evaluation on large-corpus (13.9k handles) | <2s |
| Warm full evaluation | <200ms |
| Snapshot `at()` | <500ms |
| Git-ref `at()` | <5× snapshot cost |
| Resident memory | <200MB |
| Dependency unsafe | audited, contained, or `unsafe_code = deny` |

These are necessary but not sufficient. A v2.0 that meets perf gates
and fails workflow gates ships a fast but unusable tool.

---

## Part XIII: Forward-looking [CR-Fw]

These directions are stated as v2.x+ scope so the v2.0 IR doesn't
preclude them.

### §47 Trail-driven workflows (v2.1)

Trails are captured in v2.0. v2.1 builds on them:

- `anneal trail replay <session-id>` re-runs the path against a
  newer corpus state, surfacing what's changed
- `anneal trail diff <a> <b>` compares two sessions
- `anneal trail summarize <session-id>` produces a markdown digest
  for inclusion in commit messages or PRs

### §48 Multi-corpus federation (v2.2)

A single `anneal` invocation operates across multiple corpora:

```
anneal --root .design --root /path/to/host-corpus/.design --root /path/to/large-corpus/.design \
       -e '? *handle{id: h, corpus: c}, c != "self".'
```

Handle ids are corpus-prefixed; queries can join across corpora with
provenance.

### §49 Adapters beyond markdown (v2.1+)

- **anneal-mdx**: MDX with JSX-island parsing. Components become
  edges to component definitions.
- **anneal-code**: Rust / Elixir / TypeScript / Python source.
  Handles are functions, types, modules. Edges are calls, imports,
  implements. Status is coverage + lint + ownership.
- **anneal-issues**: GitHub / Linear issue trackers. Handles are
  issues; edges are blocks/relates/duplicates.
- **anneal-host**: library API for embedding the runtime in a host
  application. Host produces facts; runtime serves the same agents.

### §50 Host Corpus embedding (v2.1+)

Concrete use case for `anneal-host`: host-corpus embeds `anneal-core` as
a Rust dep alongside its Elixir runtime (via Rustler or stdio
bridge). Host Corpus-side adapter exposes Ash resources, Phoenix routes,
Oban jobs, decision-log entries, and customer-state transitions as
handles. The same agent skill that runs in large-corpus's `.design/` runs
inside host-corpus — same vocabulary, different corpus.

This is the load-bearing reason for the substrate split: it makes
"agent-queryable application" a v2.1 deliverable, not a v3 rewrite.

---

## Part XIV: Open questions [CR-OQ]

### §51 take_until aggregation

Same as language-redesign LR-OQ1. The `work --budget` greedy fill is
iterative selection with a running sum. The `TopK` and `Rank`
helpers in §16 give 80% coverage; a `TakeUntil{sum: var, threshold:
T : body}` aggregator would handle the rest natively. Worth
investigating during Phase 1.

### §52 Adapter discovery

When a corpus's `anneal.dl` references discovery facts no loaded
Source recognizes (e.g. `code_language("rust")` with no
`anneal-code` linked), should the runtime warn or error? Lean
toward warn — the user might be running a CLI without all adapters
linked. Pin down during Phase 1.

### §53 Concurrent ingestion

Multiple Sources running in parallel during ingestion. The
`FactSink` needs to be thread-safe; the runtime's fact storage needs
to handle concurrent writes. Probably fine but worth a perf check on
multi-adapter corpora.

### §54 Trail privacy

`*trail` records every query expression. For corpora with sensitive
content, this is a privacy concern (someone reads the trail
afterwards and sees what was searched). Default behavior should
probably be opt-in or scrubbed.

### §55 MCP tool naming

The MCP surface generates tool names from verb names. Project
verbs collide with prelude verbs if both are defined. The Steele's-
criterion rule (CR-R4) says project verbs replace prelude verbs by
name; MCP needs the same.

### §56 Performance ceiling

For corpora with hundreds of thousands of handles and rich rule sets,
evaluation time grows. The substrate is designed for hundreds-of-
thousands; tens-of-millions probably needs profiling and possibly
indexed evaluation. Out of scope for v2.0.

---

## Part XV: Labels [CR-Labels]

### CR-F (Framing)
- CR-F1: §1 The thing agents need
- CR-F2: §2 Why substrate, not a markdown tool

### CR-D (Definitions)
- CR-D1: Substrate (§2)
- CR-D2: Cold-agent test (§3)
- CR-D3: Layering (§4)
- CR-D4: Source trait (§6)
- CR-D5: Stored primitives (§8)
- CR-D6: Function primitives (§9)
- CR-D7: Provenance contract (§10)
- CR-D8: `at(<ref>)` block (§11)
- CR-D9: Trail (§12)
- CR-D10: Stored (§15)
- CR-D11: Derived (§15)
- CR-D12: Stratified negation (§17)
- CR-D13: Output contract (§19)
- CR-D14: Load order (§22)
- CR-D15: Convergence vocabulary (§23)
- CR-D16: Check rule (§24)
- CR-D17: I/O contract (§31)
- CR-D18: MCP transport (§32)
- CR-D19: Init defaults (§33)
- CR-D20: Agent loop (§34)

### CR-R (Rules)
- CR-R1: Diagnostic ID literal (§25)
- CR-R2: Unique within ruleset (§25)
- CR-R3: Reserved prefixes (§25)
- CR-R4: Verb extensibility / Steele's criterion (§27)
- CR-R5: Workflow gates (§44)

### CR-Su (Surfaces)
- CR-Su1: Starter verbs (§29)
- CR-Su2: CLI flags (§30)
- CR-Su3: MCP surface (§32)

### CR-O (Onboarding)
- CR-O1: Lattice-on default (§33)
- CR-O2: Agent loop (§34)

### CR-A (Acceptance)
- CR-A1: Workflow-completion gates (§44)
- CR-A2: Performance gates (§46)

### CR-Fw (Forward-looking)
- CR-Fw1: Trail-driven workflows (§47)
- CR-Fw2: Multi-corpus federation (§48)
- CR-Fw3: Adapters beyond markdown (§49)
- CR-Fw4: Host Corpus embedding (§50)

### CR-OQ (Open questions)
- CR-OQ1: take_until aggregation (§51)
- CR-OQ2: Adapter discovery (§52)
- CR-OQ3: Concurrent ingestion (§53)
- CR-OQ4: Trail privacy (§54)
- CR-OQ5: MCP tool naming (§55)
- CR-OQ6: Performance ceiling (§56)

---

## References

### Internal
- `anneal-spec.md` — the convergence model the standard library encodes
- `2026-05-07-engine-spike-and-parity-protocol.md` — engine validation protocol
- `2026-05-13-engine-spike-results.md` — engine-viability report; architectural revision (ascent for primitives, dynamic IR for rules) carries forward

### External
- Cloudflare Code Mode — `https://blog.cloudflare.com/code-mode/` —
  programmability as the agent surface, not menu APIs
- qmd — `https://github.com/jamesrisberg/qmd` — content as
  addressable spans
- Host Corpus eval design (internal) — runtime self-description so agents
  can teach themselves the model
- ascent — `https://github.com/s-arash/ascent` — the candidate engine
  for fixed primitives layer
- Cozo — `https://github.com/cozodb/cozo` — modern Datalog with
  named fields, reference for `take_until` aggregation
- Bush, "As We May Think" — the trail-as-memex insight underpinning
  `*trail`
- Naur, "Programming as Theory Building" — agents handing off to
  agents need paths, not just facts
