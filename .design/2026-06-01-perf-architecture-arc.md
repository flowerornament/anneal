---
status: draft
---

# Performance & Architecture Arc — anneal is a compiler; build it like one — 2026-06-01

The reframe that organizes this arc (Morgan, 2026-06-01): **anneal is a
programming language and its runtime is a compiler — we should build it
properly as one.** Every perf pain we hit is a symptom of having built a
compiler without admitting it: a parser treated as "file loading," an analyzer
treated as "validation," and an evaluator running directly on **surface syntax**
(string field names, `BTreeMap` bindings) — i.e. interpreting the AST, the
classic naive-first-compiler mistake. There is no IR / plan layer separating
what the user wrote from what the machine runs.

The fix is the standard compiler pipeline, which was always implicitly present:

```
   SURFACE          FRONTEND            MIDDLE-END         BACKEND
   .dl / -e   ──►   parse + analyze ──► PLAN / IR    ──►  relational VM
   names, prose     (exists, under-      (the MISSING      tuple store,
   map rows          named today)         layer)            slots, views
                                                               │
                                        names/maps ◄───────────┘
                                        only at the output boundary
```

The missing piece has a name: a **middle-end** — the plan/IR where names become
ids, fields become column indices, variables become slots, *once*; everything
downstream runs on the lowered form. This is the same boundary as the
logical/physical split below, arrived at from compiler-architecture first
principles. `anneal-lang` is already a separate crate — it has been a language
all along; this arc commits to the compiler architecture behind it.

Discipline this imports:
- The **IR is a real, versioned artifact**, not an implementation detail.
- **Passes are explicit and independently testable**: parse → resolve/intern →
  analyze → plan → execute. (No separate `optimize` pass initially — join /
  dependency ordering lives inside `plan`. A named optimizer earns its place
  only once there are multiple rewrites or cost-based choices to make.)
- **Provenance = symbol table + debug info**: `--explain` is the compiler
  emitting source mappings; it must survive lowering (hard constraint below).
- **Make invalid states unrepresentable** in the IR (typed ids,
  parse-don't-validate) as in any typed AST.
- Future frontends (the `anneal-code` / Rust-stdlib code-as-corpus north star,
  epic anneal-8yxl) feed the *same* IR — the architecture pays off twice.

## Perf context (what exposed the missing middle-end)

We built the runtime feature-first and never designed it for evaluation
throughput. Cold start on a 15.7MB corpus (murail) was 9.4s; point-fixes
(git batching, demand-driven probes, lazy search index, query-scoped fixpoint)
brought it to ~2.58s. Those were real, but they work *around* the core. This
arc targets the core itself.

Optimization-engineer framing: performance comes from deep understanding and
elegant system design — minimizing waste, then using Rust's zero-cost
abstractions fully. The waste here is structural, in the data model.

## Architecture map (as built)

```
  CLI verb / -e query
        │
        ▼
  RuntimeSession::load   ──►  parse (pulldown + 4 regex passes) ──► FactStore
        │                      ~15% of cold start; secondary
        ▼
  RuntimeSession::eval   ──►  semi-naive fixpoint over prelude + query rules
        │                      ~60–65% of cold start; THE hot path
        ▼
  Database (per-eval)    ──►  StoredRelation rows + DerivedRelation deltas
        │                      cloned/scoped repeatedly (time-blocks, actor)
        ▼
  QueryOutput { rows: Vec<Row> }
```

### The core types (the defect)

From `crates/anneal-core/src/runtime/eval.rs`:

```rust
pub enum Value { String(String), Number(NumberValue), Bool(bool), Null, List(Vec<Value>) }
pub type Binding = BTreeMap<Ident, Value>;          // one per partial match, during joins
pub struct Row { fields: BTreeMap<String, Value>, derivation: Option<DerivationNode> }
pub struct Tuple(pub Vec<Value>);                    // the *good* shape — already columnar
```

Three structural costs compound:

1. **`Value::String(String)` is heap-owned and cloned by value.** Identifiers,
   statuses, handle ids, file paths — all short, highly repeated strings — each
   allocate and each `clone()` re-allocates. A corpus has thousands of rows
   carrying the same ~dozens of distinct status/kind/path strings.

2. **`Binding` and `Row` are `BTreeMap<String/Ident, Value>`.** Every join step
   clones a binding and inserts a field → a fresh tree allocation + per-entry
   string clones, per partial match. In a semi-naive fixpoint, partial matches
   multiply; this is the ~18% clone/drop cluster and much of `eval_body`. A
   String-keyed map per row is the classic interpreter antipattern: field
   access is a string-keyed tree lookup, row construction is a tree build.

3. **`Database` is cloned to scope it** (time-refs, actor visibility). `status`
   alone calls `scoped_to_time_ref("snapshot:last")` 29× per run, each cloning a
   DB full of the above. (Tracked as Lever 4 / anneal-eygi — but it's a symptom
   of the model, not an independent bug.)

`eval.rs` has 147 `.clone()` sites in 10,207 lines. Not all are hot, but the
density reflects a clone-to-make-the-borrow-checker-happy model rather than a
borrow/intern/arena model.

## The redesign: separate the logical model from the physical model

The framing (sharpened with codex, 2026-06-01) is NOT "optimize `Binding`." It is
**Parnas information-hiding**: the hidden thing is the *physical representation*.
End users and output get **names and readable rows**; the evaluator runs on
**ids, slots, tuples, and views**. The target is a small **relational VM**:
schema registry + intern table + tuple store + planned evaluator + view
overlays. The current `BTreeMap<String, Value>` survives ONLY as the
output/projection boundary — never as the runtime substrate.

```
   LOGICAL (user-facing)                 PHYSICAL (evaluator)
   ──────────────────────                ────────────────────
   field names "h","status"   ──plan──►  FieldId / column index
   relation name "diagnostic" ──plan──►  RelationId
   variable h                 ──plan──►  VarId / slot
   Value::String("stable")    ──intern►  SymbolId  (Copy)
   Row {fields: BTreeMap}     ◄project──  Tuple rows in a relation store
                              (only at output/describe/JSON/--explain)
```

Six pieces:

1. **Symbol universe + typed ids.** Intern corpus strings once per session.
   Use distinct newtypes — `SymbolId`, `VarId`, `RelationId`, `FieldId` — NOT a
   single `u32` (a global `Symbol(u32)` just recreates stringly-typed bugs in
   integer form). Clone→`Copy`; eq/hash/ord→integer ops; thousands of duplicate
   `"stable"` collapse to one table entry. (rust-skills: type-newtype-ids,
   type-no-stringly, own-copy-small, mem-smaller-integers, mem-assert-type-size.)

2. **Relation schema registry.** Every relation has a `RelationSchema`
   (relation id, field ids, arity, field order, optional value types). Row
   storage is keyed by `RelationId`, not `Ident`. Named fields compile to column
   indexes once. Output determinism comes from schema/projection order, not from
   a `BTreeMap` in the hot path — if JSON wants sorted keys, sort at the boundary.

3. **Tuple store, not map rows.** Stored rows are compact tuples
   (`Box<[Value]>` / `SmallVec` / `Arc<[Value]>` per measured shape) with
   per-relation, per-column/value indexes over **relation-local row ids** (so
   views/overlays reference rows without copying). Do NOT premise full columnar
   SoA — compact tuples + indexes are the premise; columnar is a *later measured
   choice* only if scans dominate. (SmallVec is a container, not the architecture.)

4. **Planned eval, not named eval.** Analyze Datalog into a *plan*: atoms
   reference `RelationId`, fields by column index, variables by `VarId`/slot.
   `Binding` becomes an **execution frame** (slot array + bound bitset, or a
   planned frame where atom order guarantees prefix-bound slots), not a
   `BTreeMap`. Negation, aggregation, and time-blocks consume the same planned
   representation. (rust-skills: api-parse-dont-validate, mem-smallvec,
   mem-reuse-collections, own-borrow-over-clone.)

5. **Time scope as `RelationView`/overlay.** `scoped_to_time_ref` must NOT clone
   `Database`. It produces a `DatabaseView` / `RelationView` (base relation +
   overlay/replacement + optional filter); repeated `at("snapshot:last")` reuses
   one view per eval context. Clean once rows are immutable tuple stores with row
   ids. (This dissolves Lever 4 / anneal-eygi — it's a symptom of the missing
   view abstraction.)

6. **Boundary projection.** Only final query output, `describe`, JSON/text
   render, and `--explain` reconstruct named maps. Surface preserved; evaluator
   is machine-shaped underneath.

**Hard constraint — provenance survives planning.** `--explain`/trail must map
tuple row ids back to relation/field names and source facts. Provenance metadata
is a design input to the plan + tuple store, not an afterthought. A faster
runtime that loses traceability is a regression.

The existing `Tuple(pub Vec<Value>)` already shows the instinct — it is
**row-compact tuple storage** (NOT columnar; columnar is struct-of-arrays, a
separate later-and-measured choice). The row/binding layer regressed to maps;
the arc restores tuple discipline as a real boundary.

### Value layout (name it, don't just intern strings)

`Value` is `String | Number | Bool | Null | List`. Interning addresses only the
String arm. The physical value domain must be named whole:
`PhysicalValue::{ Sym(SymbolId), Number(NumberValue), Bool(bool), Null, List(...) }`.
Open (measured) sub-decisions: are `List` elements interned recursively, or do
lists stay boxed/`Arc<[PhysicalValue]>` because they're rare? Assert sizes
(`mem-assert-type-size`) so the hot `Value` stays small and `Copy`-cheap for the
scalar arms. The logical `Value` (with `String`) survives at the projection
boundary; `PhysicalValue` is backend-internal.

### Fixpoint execution contract (semi-naive ↔ tuple store)

The planned evaluator is more than a fast binding shape — it must make
semi-naive fixpoint cheap over the tuple store:

- Deltas are **row-id sets / delta relation *views***, not copied tuple `Vec`s.
  A recursive stratum iterates new rows by id, not by cloning relations.
- Indexes must support **base + delta lookup** within a recursive stratum (probe
  the accumulated base and the current delta together).
- The `Plan` carries, per stratum: ordered rule groups, identified recursive
  delta inputs, aggregate group-key slots, and bound negation input slots.

This is where recursion/negation/aggregation semantics actually live; the plan
layer is under-specified without it.

## Contracts, boundaries, modularity (a compiler has an ABI)

A pipeline is only as good as the **typed contracts between its passes** (Morgan,
2026-06-01). Each pass consumes and produces a *designed artifact* with stated
invariants — the next pass must not reach into the previous pass's internals.
This is the difference between a compiler and an interpreter that grew.

Current state (the smell): `runtime/eval.rs` is 10,207 lines fusing analysis,
planning, and execution; there is an `analysis.rs` and an `eval.rs` but **no
`plan.rs`** — analysis touches execution directly, with no IR contract between.
`runtime/ast.rs` and `runtime/parser.rs` are reexport shims (`pub use
anneal_lang::...::*`) that preserve old import paths — not duplicate code, but a
muddy seam: core/project/eval import the frontend *through* runtime shims rather
than a clean boundary. Collapse to one frontend boundary. Boundaries are
implicit; that's what we're fixing.

The pass contracts (each a type whose invariants are guaranteed by construction
— parse-don't-validate, so a downstream pass cannot receive an ill-formed input):

```
   Program (anneal-lang AST)        -- surface syntax, names as strings
        │  resolve + intern
        ▼
   ResolvedProgram {interner,        INVARIANT: every ident/relation/field
        │            schemas}        resolved to a typed id; no strings downstream
        │  analyze
        ▼
   AnalyzedProgram                   INVARIANT: range-restricted/safe vars,
        │                            stratified negation, aggregate-input deps
        │  plan                      satisfied, recursion strata known
        ▼
   Plan {strata, rule groups,        INVARIANT: per stratum — ordered rule
        │  slot layouts}             groups, recursive delta inputs identified,
        │  execute(Plan, &DbView)    aggregate group-key slots + negation input
        ▼                            slots bound; every var has a slot, every
   QueryOutput                       atom a RelationId+column indices
        ── project ids→names at this boundary only
```

The invariants are the point: each output type makes the *prior pass's error
class unrepresentable*. An unsafe or unstratified rule cannot reach `Plan`; a
string field name cannot reach `execute`. The fixpoint semantics (strata,
deltas, aggregate keys) live in `Plan`, named — not improvised in the executor.

Boundary rules:

- **One IR owner.** The `Plan`/IR types live in one place (a `plan` module, or a
  dedicated crate if the frontend/backend split earns it). The executor depends
  on `Plan`, never on the parser or surface AST. Kill the duplicate
  `runtime/ast.rs`/`parser.rs` — one frontend (`anneal-lang`), one IR.
- **Encapsulated representation.** `SymbolId`, tuple stores, slot frames are
  `pub(crate)`/private to the backend; the rest of `anneal-core` and all of
  `anneal-cli`/`anneal-mcp` see only the logical surface (`Value`, named rows)
  and the `eval(plan) -> QueryOutput` entry point. The physical model is the
  hidden thing (Parnas); changing it must not ripple past the boundary.
- **Typed ids as the ABI.** Newtypes (`SymbolId`, `VarId`, `RelationId`,
  `FieldId`, plus existing `NativeId`/`CorpusId`/`Generation`) are the
  inter-pass vocabulary — never bare `u32`/`String`. A pass signature states
  exactly which id-space it speaks. (rust-skills: type-newtype-ids,
  type-no-stringly, api-parse-dont-validate, m05-type-driven, m15-anti-pattern.)
- **`DatabaseView` is multi-dimensional, not just time.** The current DB is
  scoped for time AND actor visibility — both today via cloning. The view
  abstraction must cover both dimensions (+ optional demand/query filter):
  `DbView = base store + time-snapshot overlay + visibility/capability filter`.
  Fixing snapshot cloning while leaving actor-scope as a parallel clone smell
  would only do half the job.
- **Provenance is a row-id → derivations *multimap*.** A deduped derived row can
  have multiple derivations / source facts, so a single back-ref is
  insufficient. The contract: tuple row ids map into a provenance store that
  `--explain` and trails read; many-to-one is the normal case.
- **Diagnostics/source maps are a first-class pass artifact.** Beyond `--explain`
  provenance, every pass (parse/analyze/plan/execute) must carry a
  `SourceMap`/`DiagnosticMap` mapping `SourceLocation`/`PlanNodeId`/`RelationId`
  back to user-facing names, so an error at any stage reports in the user's
  vocabulary. (Inspectable compilation artifacts improve reasoning — the
  Compiler-Explorer lesson; resonant in the research graph.)
- **IR is an *internal* semantic contract, not a public ABI (yet).** "Versioned"
  means: an explicit shape with tests and a debug-dump, deliberately changed —
  NOT serialized/plugin-stable across releases. It becomes a real ABI only if a
  future persistent cache serializes it (a separate decision). Don't promise
  plugin-stable IR today. Engine choice stays internal to `anneal-core` (master
  spec §8) — Ascent/dynamic-IR must not leak, exactly as the IR/executor split
  must not leak.

Modularity payoff: passes become **independently testable** (feed a `Plan`,
assert execution; feed a `ResolvedProgram`, assert analysis) and the
differential harness can diff at *any* boundary, not just end-to-end.

Two distinct extension routes (do not conflate):
- **Query-language frontends** compile surface syntax to a `Program` →
  `ResolvedProgram` → `Plan`: they feed the *plan pipeline*.
- **Source adapters** (`anneal-md`, future `anneal-code`) implement
  `Source::extract -> FactBatch`: they populate the *tuple store / schema
  registry*, not the plan pipeline. The relational VM helps both, by different
  routes. `anneal-code` is a fact-ingestion frontend, not a Datalog frontend.

## Arc phases (design + measurement first, no blind rewrite)

0. **Pass-contract appendix (design, before any code).** Write the concrete
   artifact types and module boundaries — `ResolvedProgram`, `AnalyzedProgram`,
   `Plan` (with the stratum/delta/aggregate fields named), `PhysicalValue`,
   `DbView`, the provenance multimap, the `SourceMap` — and where each lives.
   The allocation study validates the *cost model*; this appendix validates the
   *shape*. Both gate the rewrite.
1. **Confirm the root with an allocation study.** Re-profile at 2.58s with an
   allocation profiler (not just CPU sampling) — e.g. `dhat` /
   `--features dhat-heap`, or `samply` alloc view — measuring the named
   suspects: `Binding` clone/extend bytes, `Row` construction, `Value::String`
   clones, `BTreeMap` node churn, scoped-`Database` clone, output projection.
   Produce written findings: bytes/query, allocations/row, the realistic ceiling
   a redesign buys. **Decision gate:** proceed only if the study confirms the
   model is the dominant ceiling.
2. **Design the representation + spike.** Interner (per-session, typed-id
   boundary — see open questions), slot-binding frame, Row schema/tuple split,
   `PhysicalValue` layout, `DbView` overlays. Spike one hot rule group end-to-end
   behind a feature flag to measure before committing.
3. **Migrate the eval core** incrementally, each step gated by **two** test
   layers: (a) the **differential harness** (build pre-change binary, diff
   derived-predicate result counts + `at()`/snapshot + `--explain` traces on
   murail — the protocol that cleared Lever 3); and (b) **property/metamorphic
   tests** at the compiler boundary (alpha-renaming preserves results; legal atom
   reorder preserves results; old named-eval == planned-eval on generated small
   safe programs; explain/trail rows stay source-mappable; invalid
   stratification/aggregate programs fail *before* planning). Differential is
   mandatory but not sufficient — property tests catch what fixed corpora miss.
   Correctness is the floor; a faster runtime that changes results is a failure.

## Sequencing

- **v0.15.0 tags first at ~2.58s** — the spec→code coherence feature + cleanup +
  the three verified perf levers ship now, not held hostage to a multi-week
  rewrite. Lever 4 (anneal-eygi) is folded into this arc, not pre-tag.
- The arc is its own milestone after the tag.

## Non-goals

- Persistent on-disk fact cache (Morgan: last resort). The arc makes the work
  cheap enough that caching may never be needed; revisit only if the redesigned
  runtime still can't hit the cold-start bar on large corpora.
- Parse/extract micro-opt (the ~15% regex/pulldown) — real but secondary;
  after the eval model, if it still shows.

## Resolved during review (claude + codex, 2026-06-01)

- **Interner lifecycle: per-session, never global/cross-run.** Matches
  load-once/eval-once, avoids leaks, and keeps symbol ids from becoming
  persistent ABI. Hand-rolled thin wrapper, or `lasso` only if it doesn't fight
  the typed-id boundary; either way expose typed newtypes (`SymbolId`), never
  crate-native raw ids. `Box<str>` storage; assert id/value sizes.
- **Output ordering: preserve current text/JSON ordering at the projection
  boundary.** Do NOT depend on `Row.fields = BTreeMap` internally; project into
  an ordered map at output. NDJSON/tool consumers may have learned the current
  (lexical) order, so preserve it until a separate, deliberate output-contract
  decision — don't change surface behavior as a side effect of the rewrite.
- **Testing: differential harness is mandatory but NOT sufficient — add
  property/metamorphic tests** (see phase 3). Random *full* Datalog generation
  is expensive; start with bounded generated programs over tiny relations.

## Open (genuinely deferred)

- Concrete `Plan` node enum shape and the slot-frame representation
  (`SmallVec<[PhysicalValue; N]>` + bound bitset vs prefix-bound-by-atom-order)
  — settle in the phase-0 pass-contract appendix with the spike's measurements.
- Whether the frontend/backend split eventually warrants a crate boundary
  (`anneal-ir` / `anneal-vm`) or stays module boundaries within `anneal-core`.
  Start with modules; promote to a crate only if the boundary needs enforcing.
