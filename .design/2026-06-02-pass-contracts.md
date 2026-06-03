---
status: locked
date: 2026-06-02
issue: anneal-dm3w
epic: anneal-g0l4
authors: [claude]
reviewers: [codex, subagent-factcheck]
relates: [.design/2026-06-01-perf-architecture-arc.md, .design/2026-06-02-allocation-study.md]
---

# Pass-contract appendix — the compiler's typed artifacts and boundaries

This is **Phase 0** of the compiler arc. The arc doc
(`2026-06-01-perf-architecture-arc.md`) argues the *why* and the shape; this
appendix fixes the *concrete types and module boundaries* the rewrite is built
against, **before any rewrite code**. It and the Phase 1 allocation study
co-gate the rewrite: the study validates the cost model, this appendix validates
the shape.

Concrete signatures below are the contract. Where a representation is a
**measured choice** (settled in Phase 2 with the spike), it is marked
`〔MEASURED〕` — the *type's existence and invariant* is fixed here; its internal
layout may change behind the boundary without rippling.

Guiding discipline: **parse-don't-validate** (each artifact's invariant is
guaranteed by construction, so a downstream pass cannot receive an ill-formed
input) and **Parnas information-hiding** (the physical model is `pub(crate)` to
the backend; everything else sees only the logical surface).

> **Allocation-study correction (Phase 1, `2026-06-02-allocation-study.md`).**
> dhat reordered the target. `Binding` clone/extend is **negligible** (~1.7–6.5
> MB) — the narrow "binding clone dominates" hypothesis is false. The measured
> per-query ceiling is **`NamedRow` BTreeMap store-materialization** (361 MB,
> 3.6 M of 5.8 M allocs, *every query*) + **`scoped_to_time_ref` deep clone**
> (+170 MB per `at`) + **`Value::String` duplication** + **markdown extraction**
> (383 MB, a parallel adapter axis). So the build order below is **measurement-
> driven, not §-order**:
>
> 1. **`DbView` scope overlay (§9)** — kills the time-clone; cheapest contained
>    win, biggest `status` impact. (Dissolves anneal-eygi.)
> 2. **Tuple store (§5) + schema registry (§4)** — kills `NamedRow`
>    materialization (the 361 MB / 3.6 M-alloc tax). The big structural win.
> 3. **Interner (§2) + `PhysicalValue` (§3)** — collapse duplicate strings while
>    lowering facts into tuples.
> 4. **Plan + slot `Frame` (§§7–8) LAST** — and only after *re-measuring*
>    whether `Binding` still matters once the substrate stops emitting BTreeMaps.
>    The planner is net-new and highest-risk; it does not block the allocation
>    wins above.
>
> The contracts in §§1–13 are unchanged — only the *order they're built and
> migrated in* is set by the data. Extraction-side allocation (383 MB) is a
> parallel anneal-md follow-up, not a reason to widen this arc.

---

## 0. The pipeline at a glance

```
 anneal-lang::Program                       surface AST, names as strings
      │  resolve + intern        (frontend → ir)
      ▼
 ResolvedProgram { interner, schemas, … }   every ident/relation/field → typed id
      │  analyze                 (ir)
      ▼
 AnalyzedProgram { … }                       safe/range-restricted, stratified,
      │  plan                                aggregate deps + strata known
      ▼
 Plan { strata, rule_groups, slots, … }      per-stratum ordered groups, delta
      │  execute(&Plan, &DbView) (vm)        inputs, agg keys, negation slots
      ▼
 QueryOutput { rows: Vec<Row>, … }           ids → names projected HERE ONLY
```

Two extension routes feed this pipeline at **different** points (do not
conflate):

- **Query frontends** (the `.dl`/`-e` surface, future alt-syntaxes) produce a
  `Program` → enter at `resolve`.
- **Source adapters** (`anneal-md`, future `anneal-code`) implement
  `Source::extract -> FactBatch` → populate the **tuple store + schema
  registry**, never the plan pipeline.

---

## 1. Typed id vocabulary (the inter-pass ABI)

Integer-backed `Copy` newtypes. A new `index_id!` macro mirrors the existing
`string_id!` in `ids.rs`, but for `u32` index ids (`Copy`, integer eq/hash/ord).
NEVER a shared `Symbol(u32)` — distinct id-spaces prevent stringly-typed bugs in
integer form.

```rust
// crates/anneal-core/src/ir/ids.rs  (new module; siblings of existing ids.rs)
macro_rules! index_id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(u32);
        impl $name {
            #[inline] pub(crate) const fn from_raw(v: u32) -> Self { Self(v) }
            #[inline] pub(crate) const fn index(self) -> usize { self.0 as usize }
        }
    };
}

index_id!(SymbolId);    // an interned corpus string ("stable", a handle id, …)
index_id!(VarId);       // a query/rule variable, resolved within one program
index_id!(RelationId);  // a relation ("handle", "diagnostic", an inline pred)
index_id!(FieldId);     // a field name ("id", "status") within a schema
index_id!(SlotId);      // a column/slot position in an execution frame
index_id!(RowId);       // a relation-LOCAL row id (see §5)
```

`from_raw`/`index` are `pub(crate)` — outside the backend these ids are opaque
handles, never indexable. `mem-assert-type-size`: assert `size_of::<SymbolId>()
== 4` and `size_of::<PhysicalValue>() <= 16` (see §3) in a test.

These join the existing logical ids (`CorpusId`, `SourceName`, `NativeId`,
`OriginUri`, `Revision`, `Generation`) — those stay string/u64 and remain the
*fact-identity* vocabulary; the new ids are the *evaluation* vocabulary.

---

## 2. Interner (per-session)

```rust
// crates/anneal-core/src/ir/interner.rs
pub(crate) struct Interner {
    by_text: HashMap<Box<str>, SymbolId>,
    texts:   Vec<Box<str>>,            // SymbolId.index() → text
}
impl Interner {
    pub(crate) fn intern(&mut self, s: &str) -> SymbolId { … }
    pub(crate) fn resolve(&self, id: SymbolId) -> &str { … }   // projection only
}
```

INVARIANT: a `SymbolId` is meaningful only against the `Interner` that minted it.
Lifecycle = **per session** (one load/eval context), never global/cross-run — so
ids never become a persistent ABI and there are no leaks. `Box<str>` storage.
`lasso` is acceptable iff it doesn't fight the typed-id boundary; either way the
public currency is `SymbolId`, never a crate-native raw id.

---

## 3. PhysicalValue (name the value domain whole)

**Naming note.** anneal is a **query compiler**: it compiles Datalog to a physical
plan and evaluates it on a relational VM — it is *not* a codegen compiler (no
emitted machine code, no "lowering to a backend"). In query-compiler vocabulary
"physical" is the term of art — Codd's logical/physical data independence; the
logical-plan → physical-plan split of every query optimizer (Postgres, Calcite,
Catalyst). So the value type deliberately mirrors the model split: `Value`
(logical/surface) ↔ `PhysicalValue` (physical/evaluator). The pairing is
self-documenting; it is kept over the standalone term `Datum` precisely because it
names its place in the logical/physical architecture. Avoid PL-compiler words
("lowering/backend/codegen") for this layer — they're the wrong dialect and make
"physical" read as foreign.

The logical `Value` (`String | Number | Bool | Null | List`) survives **only at
the projection boundary**. The evaluator runs on:

```rust
// crates/anneal-core/src/vm/value.rs
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PhysicalValue {
    Sym(SymbolId),          // every string arm is interned
    Number(NumberValue),    // reuse existing NumberValue (Int/…)
    Bool(bool),
    Null,
    List(ListId),           // 〔MEASURED〕 see below
}
```

`List` is the one non-`Copy`-friendly arm. `ListId` indexes a list arena of
`Arc<[PhysicalValue]>`, keeping the scalar arms `Copy` and the enum ≤ 16 bytes
(verified: `NumberValue` is `Copy` and 16 bytes — `eval.rs:735` — so the tag
packs into its alignment slack and `size_of::<PhysicalValue>() == 16`; keep the
size assertion because the margin is tight). `〔MEASURED〕` whether list elements
intern recursively or box whole.

**List lifecycle is a hot-path concern, not a rarity** (fact-check correction):
base facts rarely carry lists, but `list`/`set` aggregates *materialize* lists
during evaluation (`eval.rs:4802`), consumed by `in`/`contains`/`take_until`.
So the arena must handle **transient per-eval lists**, not just session-stable
ones. Contract: the list arena is **eval-context-scoped** (dropped with the eval,
like `Frame`s), distinct from the session-scoped string `Interner` — transient
aggregate lists must not leak into session memory. `〔MEASURED〕` whether hot
aggregate lists are worth interning at all vs. a cheap bump arena reclaimed per
query.

**Escape invariant (codex P3):** any `PhysicalValue::List(ListId)` that reaches
**output or provenance** — outputs and `--explain` outlive execution — must be
**projected to a logical `Value::List` before the eval arena drops**. Provenance
must not retain a raw `ListId` (it would dangle once the arena is reclaimed); a
`Derivation` (§10) holding a computed list materializes it to owned
`Value`/`Arc<[…]>` at capture. `list`/`set`/tuple-exprs/`in`/`contains` all route
lists through comparison/projection, so this is checked at the projection seam.

`Value ↔ PhysicalValue` conversion lives at exactly two seams: `FactBatch`
ingestion (logical → physical, interning) and output projection (physical →
logical, resolving). Nowhere else.

---

## 4. Schema registry

```rust
// crates/anneal-core/src/ir/schema.rs
pub(crate) struct RelationSchema {
    pub(crate) relation: RelationId,
    pub(crate) fields:   Vec<FieldId>,   // positional; index = column = SlotId space
    pub(crate) arity:    usize,
    pub(crate) value_types: Option<Vec<ValueType>>,  // optional typing, later
}
pub(crate) struct SchemaRegistry {
    schemas: Vec<RelationSchema>,                 // RelationId.index() → schema
    field_index: HashMap<(RelationId, FieldId), usize>, // named field → column
}
```

INVARIANT: a named field access (`*handle{status: s}`) is resolved to a **column
index once**, at resolve/plan time — never by string lookup in the hot path.
Output determinism is a property of `fields` order + projection, NOT of a
`BTreeMap` in eval (§9). The seven built-in relations (handle/edge/meta/content/
span/concern/config/snapshot/generation) get fixed schemas; inline/derived
predicates get schemas synthesized at resolve.

---

## 5. Tuple store (the relational substrate)

Replaces `StoredRelation { rows: Vec<NamedRow>, indexes: BTreeMap<Ident,
BTreeMap<Value, Vec<usize>>> }`.

```rust
// crates/anneal-core/src/vm/store.rs
pub(crate) struct Tuple(Box<[PhysicalValue]>);   // 〔MEASURED〕 Box<[]> | SmallVec | Arc<[]>

pub(crate) struct RelationStore {
    schema: RelationId,
    rows:   Vec<Tuple>,                           // RowId.index() → tuple
    indexes: HashMap<SlotId, HashMap<PhysicalValue, Vec<RowId>>>, // per-column
}
pub(crate) struct TupleDb {
    relations: Vec<RelationStore>,                // RelationId.index() → store
    interner:  Interner,
    schemas:   SchemaRegistry,
    provenance: ProvenanceStore,                  // §10
}
```

INVARIANTS:
- `RowId` is **relation-local** and stable for the life of a store, so views and
  overlays (§9) reference rows by id without copying tuples.
- Rows are immutable once inserted (within an eval context); recursion appends.
- `〔MEASURED〕` tuple backing + index map type (`HashMap` vs sorted `Vec` vs
  fx-hash) — Phase 2 spike. **Do NOT premise columnar SoA**; compact tuples +
  per-column indexes are the premise; columnar is a later, measured choice if
  scans dominate.

Determinism note (anneal-9sdn just shipped): canonical insertion order is now a
property we keep — the store preserves the source-canonical row order, and
projection (§8) does not reorder unless the query asks (`order by`).

---

## 6. `ResolvedProgram` (frontend → ir boundary)

```rust
// crates/anneal-core/src/ir/resolved.rs
pub(crate) struct ResolvedProgram {
    pub(crate) interner: Interner,
    pub(crate) schemas:  SchemaRegistry,
    pub(crate) rules:    Vec<ResolvedRule>,   // heads/bodies over typed ids
    pub(crate) queries:  Vec<ResolvedQuery>,  // ≥1; each carries ordering (§8)
    pub(crate) registry: ResolvedRegistry,    // config/verb/doc/source resolved out of statements
    pub(crate) source_map: SourceMap,         // §11
}
```

INVARIANT: **no surface strings reach downstream.** Every relation name →
`RelationId`, field name → `FieldId`/column, variable → `VarId`, string literal →
`SymbolId`. Produced by `resolve(&Program) -> Result<ResolvedProgram,
ResolveError>`. This is also where alt query frontends converge — they hand off a
`Program`; everything past here is shared.

**A `Program` is not just `{rules, query}`** (fact-check correction). `Statement`
(`ast.rs:171`) is a 12-arm enum — besides `Rule`/`Query` it includes `AtBlock`,
`ConfigBlock`, `SourceBlock`, `Include`, `Import`, `Verb`, `Doc`, `Predicate` —
and a program may contain **multiple `Query` statements**. `resolve` must account
for all arms, not silently assume one goal. The disposition:
- `AtBlock` (the time-scoping construct §9's `DbView` exists to serve) **lowers
  to a `TimeScope` region over a body *subtree*, not a whole-rule annotation**
  (codex P1). Time blocks appear two ways and the lowering must handle both: (a)
  top-level `Statement::AtBlock { ... }` expands the statements it contains so
  their bodies run under that time scope; (b) `Atom::TimeBlock` is a **nested body
  atom** that can be *mixed with current-time atoms in the same body*
  (`eval_atom_traced` scopes only the subtree, then rejoins its bindings with the
  outer binding). So it lowers to `AtomPlan::TimeScope { time, inner }` (§8) +
  `TimeOverlay` (§9) — a coarse "annotate the enclosing rule/query" lowering would
  lose mixed current+historical joins.
- `Include`/`Import`/`SourceBlock` are resolved/expanded before or during resolve
  (they shape which rules/facts exist), not carried into `Plan`.
- `ConfigBlock`/`Verb`/`Doc`/`Predicate` populate the schema registry / verb
  registry / docs — registry inputs, not plan inputs.
- Multiple `Query` statements → `ResolvedProgram.queries: Vec<ResolvedQuery>`
  (plural), each planned independently against the shared analyzed rule set.

`ResolvedProgram` therefore carries `queries: Vec<ResolvedQuery>` and whatever
registry state resolve produces, not a single `query`.

---

## 7. `AnalyzedProgram` (analysis boundary)

```rust
// crates/anneal-core/src/ir/analyzed.rs
pub(crate) struct AnalyzedProgram {
    pub(crate) resolved: ResolvedProgram,
    pub(crate) strata:   Vec<Stratum>,        // stratification result
    pub(crate) safety:   SafetyWitness,        // range-restriction proof tokens
}
```

INVARIANT: range-restricted/safe variables, **stratified negation**, aggregate
input dependencies satisfied, recursion strata identified. An unsafe or
unstratifiable program **cannot construct an `AnalyzedProgram`** —
`analyze(ResolvedProgram) -> Result<AnalyzedProgram, AnalysisError>` is the only
constructor, and it fails before planning. (This is where the order-by
unbound-key check becomes an analysis invariant.)

**This subsumes the existing string-based analysis — it is not greenfield**
(fact-check correction, HIGH). `runtime/analysis.rs` already has
`analyze(Program) -> AnalyzedProgram` with `Stratum`, `AnalyzedQuery`,
`compute_strata`, `check_cyclic_stratification`. But the existing types are
**string-keyed on the surface AST**: the existing `AnalyzedProgram` *stores the
`Program`* and `Stratum.predicates` is `Vec<PredicateRef>` (string-based). That
directly contradicts the §6 "no strings downstream" invariant. The arc's move:
- **resolve runs *before* analyze** (current order is the reverse — analysis
  consumes the raw `Program`). The existing stratification/safety *logic* is
  sound and ported, but it operates on `RelationId`/`VarId`, not `PredicateRef`/
  string. `PredicateRef → RelationId` lowering happens in resolve.
- The new `ir::AnalyzedProgram`/`ir::Stratum` **replace** the
  `runtime::analysis` types (same algorithm, typed-id inputs), not coexist with
  them — to avoid two `AnalyzedProgram`s. Migration ports `compute_strata` /
  `check_cyclic_stratification` onto the id-keyed representation.
- Net: analysis is *already a separate module* (good — the split is real, not
  invented), but it is currently on the wrong side of the resolve boundary. The
  arc moves it behind resolve and re-keys it.

---

## 8. `Plan` (ir → vm boundary) — where fixpoint semantics live, named

```rust
// crates/anneal-core/src/ir/plan.rs
pub(crate) enum PlanKind {           // (codex review) inputs differ — one facade, distinct kinds
    GlobalFixpoint,                  // materialize derived relations over the whole DB
    Query { local_rules: Vec<RuleGroupPlan> },  // a `?` goal + its `where` rules (overlay, not clone)
}
pub(crate) struct Plan {
    pub(crate) kind:       PlanKind,
    pub(crate) strata:     Vec<StratumPlan>,   // execution order
    pub(crate) output:     OutputPlan,         // projection + ordering
    pub(crate) schemas:    SchemaRegistry,     // carried for execution + projection
}
pub(crate) struct StratumPlan {
    pub(crate) rule_groups: Vec<RuleGroupPlan>, // ordered within stratum
    pub(crate) recursive:   bool,
    pub(crate) deltas:      Vec<DeltaPlan>,      // per-atom delta substitution (see below)
}
// (codex P2) semi-naive is per-rule/per-atom, not a relation-set: run_rule_group
// reruns eval_rule_with_delta(rule, delta, atom_index) for EACH recursive atom.
pub(crate) struct DeltaPlan {
    pub(crate) rule_group:   usize,             // index into stratum.rule_groups
    pub(crate) atom_index:   usize,             // which recursive atom gets the delta
    pub(crate) predicate:    RelationId,
    pub(crate) delta_input:  RelationId,        // delta relation probed alongside base
}
pub(crate) struct RuleGroupPlan {
    pub(crate) head:   RelationId,
    pub(crate) atoms:  Vec<AtomPlan>,           // body atoms IN ORDER (joins, prims, aggs, negs)
    pub(crate) slots:  SlotLayout,              // VarId → SlotId, frame width
}
// (codex P1) atoms occur among each other in body order — primitives, aggregates,
// and negation are FIRST-CLASS atom nodes, not sibling singletons.
pub(crate) enum AtomPlan {  // 〔MEASURED〕 exact representation firms up in the Phase-2 spike
    Scan      { relation: RelationId, binds: Vec<(SlotId, ColumnRef)>,
                constraints: Vec<ColumnConstraint> },
    Filter    { comparisons: Vec<ComparePlan> },       // over slots/exprs
    Negation  { inner: Box<RuleBodyPlan>, bound_inputs: Vec<SlotId> },
    // (codex P1) primitives: search/read/read_full/match/introspection/graph — each
    // has required-bound inputs, output slots, constraint positions, provider +
    // capability action, and cache/demand behavior. No honest home without this.
    PrimitiveCall {
        primitive:      PrimitiveId,
        input_slots:    Vec<SlotId>,            // required-bound inputs (policy enforced at analyze)
        output_slots:   Vec<SlotId>,
        constraints:    Vec<ColumnConstraint>,  // bound positions (e.g. fixed handle/span)
        provider:       ProviderRef,            // search index / source / graph / introspection
        capability:     CapabilityAction,       // capability check to run
        demand:         DemandPolicy,           // lazy/cache behavior (e.g. search-index, history)
    },
    // (codex P1) aggregates are tuple-producing ordered subqueries over an inner
    // body, evaluated per outer binding; MULTIPLE per rule body allowed.
    Aggregate {
        function:    AggregateFunction,         // Count/Sum/Min/Max/Avg/List/Set/TopK/Rank/TakeUntil
        inner:       Box<RuleBodyPlan>,         // the inner body evaluated per outer binding
        group_keys:  Vec<SlotId>,               // outer group-by slots
        args:        AggregateArgsPlan,         // k/budget/sum/key/rank-var/result exprs per fn
        result:      Vec<SlotId>,               // unified result slots back into the outer frame
    },
    // (codex P1) time scope is a SUBTREE region, mixable with current-time atoms
    // in the same body — not a whole-rule annotation (see §9).
    TimeScope { time: TimeRef, inner: Box<RuleBodyPlan> },
}
pub(crate) struct OutputPlan {
    // (codex P3) outputs may be exprs (order-by exprs, aggregate tuple results),
    // not only bound vars.
    pub(crate) projection: Vec<(FieldId, ProjectionSource)>,  // SlotId | Expr, schema-ordered
    pub(crate) ordering:   Vec<OrderKeyPlan>,                  // lowered `order by`
}
```

INVARIANT: every variable has a `SlotId`, every relational atom a `RelationId` +
column indices, every primitive a resolved provider + capability + required-bound
inputs, every aggregate its inner body + group keys + result slots, every
recursive stratum its per-atom `DeltaPlan`s. A string field name cannot reach
`execute`. **Fixpoint, primitives, aggregation, negation, and time scope live
here, named — not improvised in the executor.** `〔MEASURED〕` the concrete
`AtomPlan`/`SlotLayout`/`RuleBodyPlan` representation (slot array + bound bitset
vs prefix-bound-by-atom-order) is the Phase-2 spike's to settle; the artifact's
*fields and invariants* are fixed now.

**`Plan` is net-new construction, not a lift-and-shift.** There is **no planning
phase today**: `eval_query`/`eval_body`/`eval_derived_traced`/`run_rule_group`
compute join evaluation, primitive calls, aggregation, and semi-naive recursion
inline at runtime; join order lives nowhere extractable. So unlike resolve/analyze
(which port existing logic), the planner is built from scratch — the highest-risk
new pass. The Phase-2 spike must drive **one hot rule group that exercises a
relational scan, a primitive (`search` or `active`), and an aggregate** end-to-end
through a real `Plan` before the representation is committed.

**Backend entry is a facade over distinct executions** (codex factual fix). Global
fixpoint, query-local-rule eval, and query-body eval have different inputs today;
the contract is `execute(&Plan, &DbView) -> QueryOutput` dispatching on
`PlanKind` (equivalently `execute_program`/`execute_query`), not a single
uniform path pretending they're the same.

Execution frame (replaces `Binding = BTreeMap<Ident, Value>`):

```rust
// crates/anneal-core/src/vm/frame.rs
pub(crate) struct Frame {
    slots: SmallVec<[PhysicalValue; INLINE_SLOTS]>,  // 〔MEASURED〕 width/inline N
    bound: BoundMask,                                  // which slots are bound
}
```

---

## 9. `DbView` — multi-dimensional, overlay not clone

`scoped_to_time_ref` must NOT clone `Database`. The view covers the scoping
dimensions plus an optional demand filter — but the two dimensions are **not
symmetric** in the current code (fact-check correction, HIGH):

- **Time** *is* a clone today: `scoped_to_time_ref` (`eval.rs:1284`) →
  `clone_for_time_scope` deep-clones the whole store, invoked per `AtBlock`
  during body eval (the ~29× symptom). This is the clone the overlay removes.
- **Actor visibility is NOT a clone.** It is applied at DB *construction* via
  `from_store_with_visibility` (`eval.rs:857`, a build-time `FactStore` filter)
  plus a per-scan row predicate `stored_row_visible` (`eval.rs:4441`, trail-row
  privacy).
- **A second clone the view must also subsume:** `eval_query` does
  `self.database.clone()` whenever a query carries `local_rules` (`eval.rs:3593`)
  — inline `where` rules. The "no clone" design absorbs this too (local rules
  become an overlay of derived relations, not a cloned base).

**LOCKED DECISION (codex P2) — don't change auth and perf at once.** The first
`DbView` spike (Phase 2) models **only** the time overlay + the local-derived
overlay. **Visibility stays exactly as today** — `from_store_with_visibility`
build-time filter + `stored_row_visible` scan predicate — i.e. visibility is
`TupleDb` *construction policy*, not a view dimension, in the first cut. Lifting
visibility into a `RowId`-set/bitset filter on the view is a **later** step, taken
only once `TupleDb` can retain all rows + visibility bitsets, and gated on its own
auth-equivalence tests. The first spike must not alter auth semantics.

```rust
// crates/anneal-core/src/vm/view.rs
pub(crate) struct DbView<'db> {
    base:        &'db TupleDb,
    time:        Option<TimeOverlay>,        // see below — a relation PATCH, not a row filter
    local_rules: Option<DerivedOverlay>,     // query `where` rules: added derived relations
    demand:      Option<DemandFilter>,       // optional query-scope narrowing
    // visibility: NOT here in the first cut — it is TupleDb construction policy.
}

// (codex P2) snapshot semantics today clone the DB, REPLACE the `snapshot`
// relation rows, apply a handle snapshot (logical replacements / synthetic rows
// that need not map 1:1 to base row ids), and swap the graph index to
// graph.scoped_to_snapshot. So the overlay is relation-PATCH shaped, with its own
// overlay-local row-id namespace for synthetic rows — "replace/add rows by base
// RowId" is too weak.
pub(crate) struct TimeOverlay {
    snapshot_rows: RelationPatch,            // replacement rowset for `snapshot`
    handle_patch:  HandleSnapshotPatch,      // synthetic/replacement `handle` rows by LOGICAL id
    graph_overlay: GraphScopeOverlay,        // scoped graph-primitive state
    synthetic:     OverlayRowArena,          // overlay-local RowIds for synthetic rows
}
```

INVARIANT: a view never deep-copies the base store; it composes base relations
with relation patches + derived overlays. Synthetic/replacement rows live in an
overlay-local row-id namespace so scans see "base minus replaced, plus patch."
Repeated `at("snapshot:last")` in one eval reuses one overlay. This **dissolves
Lever 4 / anneal-eygi** — the 29×-clone is the missing-view symptom. Backend entry
is `execute(&Plan, &DbView) -> QueryOutput` dispatching on `PlanKind` (§8).

---

## 10. Provenance (row-id → derivations multimap)

```rust
// crates/anneal-core/src/vm/provenance.rs
pub(crate) struct ProvenanceStore {
    // (RelationId, RowId) → one-or-more derivations (deduped rows have several)
    derivations: HashMap<(RelationId, RowId), SmallVec<[Derivation; 1]>>,
}
// (codex P1) the current DerivationNode has 11 variants — Query/Rule/Fact/Stored/
// Primitive/Comparison/Aggregate/Negation/TimeBlock/RecursiveChain/Truncated.
// Several are NON-ROW events or carry transient/computed values not in any stored
// relation, so Derivation must preserve them, not collapse to Fact|Rule.
pub(crate) enum Derivation {
    Fact      { source: FactRef },                              // base fact (row)
    Stored    { relation: RelationId, row: RowId },             // stored-row ref
    Rule      { rule: RuleId, premises: Vec<DerivationRef> },   // premises may be non-row
    Primitive { primitive: PrimitiveId, inputs: Vec<PhysicalValue>,
                computed: Box<[PhysicalValue]> },               // computed tuple, no stored row
    Comparison{ location: SourceLocation, lhs: PhysicalValue, rhs: PhysicalValue },
    Negation  { location: SourceLocation, absent: DerivationRef },
    Aggregate { function: AggregateFunction, children: Vec<Derivation>,
                truncated: bool },                              // bounded child traces
    TimeScope { time: TimeRef, inner: Box<Derivation> },
    Projection{ query: QueryId, bindings: Box<[PhysicalValue]> }, // transient final-query row
    RecursiveChain { summary: RecursionSummary },               // summarized, not unbounded
    Truncated,                                                  // explicit elision marker
}
// A premise can be a stored row, a primitive call, a comparison, etc. — not only
// a (RelationId, RowId). Hence an indirection:
pub(crate) enum DerivationRef {
    Row(RelationId, RowId),
    Inline(Box<Derivation>),     // non-row events (comparison/primitive/negation/…)
}
```

HARD CONSTRAINT (from the arc doc): `--explain`/trail must map tuple row ids back
to relation/field names and source facts. Many-to-one is the **normal** case
(deduped derived rows), so a single back-ref is insufficient — hence the
multimap. Provenance is a design input to the tuple store + plan, not an
afterthought. A faster runtime that loses traceability is a regression.

**This generalizes the existing `Row.derivation`** (fact-check correction): the
current `Row` carries `derivation: Option<DerivationNode>` (`eval.rs:73`) — an
inline, per-row, single derivation. That is the mechanism `--explain` reads
today. The `ProvenanceStore` replaces it: derivation moves **off the row** (rows
become bare `Tuple`s, §5) into a `(RelationId, RowId)`-keyed multimap. Migration
must preserve every `DerivationNode` shape `--explain` currently renders — port
`DerivationNode` into `Derivation`, don't redesign the trace format. The
differential harness diffs `--explain` output old-vs-new as a correctness gate.

---

## 11. `SourceMap` / diagnostics (first-class pass artifact)

```rust
// crates/anneal-core/src/ir/source_map.rs
// (codex P3) define the node-id spaces explicitly — the existing `NodeId` in the
// graph vocabulary is a DIFFERENT concept; don't reuse it here.
index_id!(AstNodeId);     // a surface AST node (parse/resolve diagnostics)
index_id!(PlanNodeId);    // a plan node (plan/execute diagnostics)
pub(crate) struct SourceMap {
    relations: HashMap<RelationId, String>,   // → user name
    fields:    HashMap<FieldId, String>,
    vars:      HashMap<VarId, String>,
    ast_loc:   HashMap<AstNodeId, SourceLocation>,
    plan_loc:  HashMap<PlanNodeId, AstNodeId>,  // plan node → originating AST node → loc
}
```

Every pass (parse/analyze/plan/execute) carries enough of this to report errors
in the **user's vocabulary** — an error at any stage maps `RelationId`/`FieldId`/
`PlanNodeId` back to names + `SourceLocation`. Inspectable compilation artifacts
(debug-dump of `ResolvedProgram`/`Plan`) are part of the contract — the
Compiler-Explorer lesson. This is what lets the differential harness diff at *any*
boundary, not just end-to-end.

---

## 12. Module layout + encapsulation boundary

```
crates/anneal-core/src/
  ir/                    ← frontend→backend contracts (the "middle-end")
    ids.rs               SymbolId/VarId/RelationId/FieldId/SlotId/RowId + index_id!
    interner.rs          Interner
    schema.rs            RelationSchema, SchemaRegistry
    resolved.rs          ResolvedProgram + resolve()
    analyzed.rs          AnalyzedProgram + analyze()
    plan.rs              Plan + plan()              ← the IR owner
    source_map.rs        SourceMap
  vm/                    ← the relational backend (Parnas-hidden)
    value.rs             PhysicalValue, ListId arena
    store.rs             Tuple, RelationStore, TupleDb
    frame.rs             Frame, BoundMask
    view.rs              DbView + overlays/filters
    provenance.rs        ProvenanceStore
    execute.rs           execute(&Plan, &DbView) -> QueryOutput
  runtime/               ← shrinks: becomes the thin logical façade
    (eval.rs decomposes INTO ir/ + vm/; the 10k-line fusion goes away)
```

ENCAPSULATION (the whole point):
- `ir::*` and `vm::*` internals are `pub(crate)`. The rest of `anneal-core`, and
  all of `anneal-cli`/`anneal-mcp`, see only: the logical `Value`, named `Row`,
  and a **public `runtime` facade**. The physical model is the hidden thing;
  changing it must not ripple past this boundary.
- **This is a MIGRATION TARGET, not the current state** (codex P2). Today
  `app.rs` reaches around any boundary — it imports `AnalyzedProgram`, `Atom`,
  `Body`, `CallArg`, `CallStyle`, `Expr`, `Literal`, `NegatedAtom`, `StoredAtom`,
  `stored_relation_fields`, `parse_program`, `analyze`, `Evaluator`, `Database`
  (for empty-binding hints, retired-section warnings, dynamic verbs, test
  helpers); `context.rs` builds databases/evaluators in tests; MCP imports
  `parse_program`/`parse_prelude_program`. Locking `pub(crate) ir/vm` without
  replacing these forces the CLI to keep reaching in. So the facade must offer,
  as **public** services, what those call sites actually need:
  - `parse(&str) -> Result<Program, _>` and a `validate/analyze` service for
    query-shape introspection (empty-binding hints, retired-section warnings).
  - `eval(program, &db) -> QueryOutput` and a query-local-rules variant.
  - schema/field lookup (`stored_relation_fields` replacement) + verb registry
    access for dynamic verbs.
  - diagnostic/hint helpers that speak the logical surface.
  The CLI/MCP migrate onto these; only then do the raw `Atom`/`Database`/
  `Evaluator` imports go away. Name the facade in Phase 2, migrate call sites as
  passes land — do not big-bang the boundary.
- The executor depends on `Plan`, **never** on the parser or surface AST.
- Kill the `runtime/ast.rs`/`runtime/parser.rs` reexport shims — one frontend
  (`anneal-lang`), one IR (`ir::plan`). Core imports the frontend directly, not
  through a runtime shim.
- Engine choice (Ascent/dynamic-IR) stays internal to the backend exactly as the
  physical model does (master spec §8). "Versioned IR" = an explicit shape with
  tests + a debug-dump, deliberately changed — NOT a serialized/plugin-stable
  ABI. It becomes a real ABI only if a future persistent cache serializes it (a
  separate, deferred decision).

---

## 13. Old → new map (what each current type becomes)

| Current (`runtime/eval.rs`)                 | Becomes                                   |
|---------------------------------------------|-------------------------------------------|
| `Binding = BTreeMap<Ident, Value>` (`eval.rs:58`) | `vm::Frame` (slot array + bound mask) |
| `NamedRow = BTreeMap<Ident, Value>` (stored, `eval.rs:1551`) | `vm::Tuple` (the stored thing) |
| `Row { fields: BTreeMap<String,Value>, derivation }` (`eval.rs:73`, output) | reconstructed only at output projection; `derivation` → `ProvenanceStore` |
| `Value::String(String)`                     | `PhysicalValue::Sym(SymbolId)`            |
| `Value::{Number,Bool,Null,List}`            | `PhysicalValue::{Number,Bool,Null,List}`  |
| `Tuple(Vec<Value>)` (`eval.rs:63`)          | `vm::Tuple(Box<[PhysicalValue]>)` 〔MEASURED〕|
| `StoredRelation {rows: Vec<NamedRow>, indexes}` (`eval.rs:1554`) | `vm::RelationStore` (RowId-keyed + col indexes) |
| `Database { stored: BTreeMap<Ident,…> }` (`eval.rs:777`) | `vm::TupleDb` (RelationId-keyed) |
| `scoped_to_time_ref` clone (`eval.rs:1284`) | `vm::DbView` time overlay (no clone)      |
| `eval_query` `self.database.clone()` for local_rules (`eval.rs:3593`) | `vm::DbView` derived-relation overlay (no clone) |
| actor visibility (build-time `FactStore` filter + `stored_row_visible` scan predicate) | `vm::DbView` visibility filter (new uniform model — NOT an existing clone) |
| `runtime::analysis` (string/`PredicateRef`-keyed, stores `Program`) | `ir::AnalyzedProgram` (typed-id, post-resolve) — ported + re-keyed |
| inline join eval in `eval_query`/`eval_body` (no plan today) | `ir::Plan` → `vm::execute` (**net-new** planner) |
| `Row.derivation: Option<DerivationNode>` inline | `vm::ProvenanceStore` (RowId→derivations multimap) |

The logical `Value`, `Row`, `Ident`, and the `anneal-lang` AST are **unchanged** —
they are the preserved surface. Everything physical moves behind `ir`/`vm`.

---

## 14. Deliberately deferred to Phase 2 (the spike settles, with measurements)

- Concrete `AtomPlan` node enum + `SlotLayout` representation (slot array + bound
  bitset vs prefix-bound-by-atom-order). Marked `〔MEASURED〕` above.
- `Tuple` backing (`Box<[]>` vs `SmallVec` vs `Arc<[]>`) and index map type.
- Whether `List` elements intern recursively.
- Whether `ir`/`vm` eventually warrant crate boundaries (`anneal-ir`/`anneal-vm`)
  or stay modules in `anneal-core`. **Start as modules**; promote only if the
  boundary needs compiler-enforcing.

These are representation choices behind fixed contracts — they can change in
Phase 2 without altering any signature in §§1–12 that a sibling pass depends on.

---

## Acceptance for Phase 0

This appendix is "done" when: every pass artifact (§§6–8), the physical substrate
(§§1–5), the view/provenance/source-map contracts (§§9–11), and the module/
encapsulation boundary (§12) are concrete enough that an implementer can build a
pass against its input type and test it in isolation — and codex's adversarial
review finds no under-specified center. It then locks alongside the Phase 1
allocation-study verdict.

### Review log

- **2026-06-02 — grounded fact-check (subagent vs. codebase).** Confirmed the
  core type bet (Copy `PhysicalValue` ≤16B; reexport shims real; AST shape as
  assumed). Folded corrections: visibility is a build-time filter + scan
  predicate, **not** a clone (§9); the existing `runtime::analysis` is
  string/`PredicateRef`-keyed and must be ported behind resolve, not coexisted
  with (§7); `Plan` is net-new — no planning phase exists today (§8); the
  `eval_query` local-rules clone must also be subsumed (§9); `ResolvedProgram`
  must handle all 12 `Statement` arms incl. `AtBlock` + multiple queries (§6);
  `Row.derivation`/`DerivationNode` is the provenance mechanism to generalize, not
  invent (§10); list arena is eval-scoped because aggregates materialize lists on
  the hot path (§3); `Row` vs `NamedRow` corrected (§13).
- **2026-06-02 — codex adversarial design review (revise-then-lock).** codex
  pressure-tested the contracts against the live evaluator and pre-authorized the
  lock conditional on six additions, all now folded:
  1. `AtomPlan::PrimitiveCall` — primitives (`search`/`read`/`match`/graph/
     introspection) with input/output slots, constraints, provider, capability,
     demand (§8). ✓
  2. First-class `AtomPlan::Aggregate` — aggregates are body atoms (multiple per
     body), inner body per outer binding, tuple-producing ordered subqueries (§8). ✓
  3. Subtree time-scope (`AtomPlan::TimeScope`, mixable with current-time atoms) +
     `TimeOverlay` as a relation-PATCH with overlay-local synthetic row ids
     (§§6, 8, 9). ✓
  4. Full `Derivation` enum preserving all 11 `DerivationNode` variants incl.
     non-row events + transient values, via `DerivationRef` (§10). ✓
  5. Locked visibility decision for Phase 2: visibility stays `TupleDb`
     construction policy; only time + local-derived overlays in the first
     `DbView` (§9). ✓
  6. Public `runtime` facade list for CLI/MCP migration; §12 reframed as a
     migration target with named services. ✓
  Plus P2/P3: per-atom `DeltaPlan` semi-naive (§8), `PlanKind` facade dispatch
  (§8), list escape invariant (§3), `AstNodeId`/`PlanNodeId` spaces (§11),
  projection exprs (§8). **LOCKED** per codex's stated condition; a courtesy
  confirm-read is welcome but non-blocking. Phase 2 may begin.
