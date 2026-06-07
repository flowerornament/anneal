---
status: superseded
superseded-by: 2026-06-07-core-decomposition-plan.md
date: 2026-06-04
epic: anneal-g0l4
authors: [claude]
relates:
  - .design/2026-06-02-pass-contracts.md   # the PLAN (appendix)
  - .design/2026-06-02-allocation-study.md # the WHY
---

# anneal runtime architecture (post-compiler-arc, as built) — 2026-06-04

> **Superseded by `2026-06-07-core-decomposition-plan.md`.** This was the as-built
> after the `g0l4` perf arc but **before kftp**. Its "plan vs actual" gap table
> below (Plan/IR middle-end `❌ not built`, `SmallVec` `Binding`, no `plan.rs`) was
> reversed by the kftp arc: `ir/plan.rs` exists, the executor is planned, the
> interpreted evaluator and `SmallVec` `Binding` are deleted. Retained as the
> point-in-time record of the perf-arc stopping point. For current shape and the
> decomposition plan, read the successor.

This documents the runtime **as it actually is** after the compiler arc
(`anneal-g0l4`), not as the pass-contract appendix planned it. Where the two
differ, this doc is authoritative for current code; the appendix remains the
design north star for the unbuilt remainder.

One-line status: **the physical data model was rebuilt (the performance goal —
done); the full compiler pass-pipeline was not (the structural goal — partial).**
`status` on murail went 3.1s→1.45s (~2.1×) with −54% allocation churn, byte-
identical results, on this architecture.

---

## Module map (`crates/anneal-core/src/`)

```
 ir/                  logical identity + schema (the typed-id vocabulary)
   ids.rs             index_id! → SymbolId, RelationId, FieldId, RowId, ListId   [used by store/schema]
                                  VarId, SlotId   [targeted #[allow(dead_code)] + comment: reserved for the Plan/IR middle-end]
   interner.rs        Interner: per-session str↔SymbolId (Box<str> storage)
   schema.rs          RelationSchema / FieldSchema / ValueType  (named field → FieldId column)
 vm/                  the physical substrate (Parnas-hidden behind the logical surface)
   value.rs           PhysicalValue {Sym|Number|Bool|Null|List} (Copy, ≤16B) + ListArena
                      from_logical / to_logical — the only Value↔PhysicalValue seams
   store.rs           Tuple(Box<[PhysicalValue]>), RelationStore (RowId rows + per-FieldId
                      indexes), TupleDb, TupleRow, time-overlay + snapshot patch logic
 runtime/             the evaluator + frontend glue (NOT decomposed — see gaps)
   eval.rs   (~11k)   the engine: scans TupleDb, runs the fixpoint, primitives,
                      aggregation, negation, time overlay, order-by, provenance,
                      output projection. Holds Binding (the join frame).
   analysis.rs        stratification / safety (string/PredicateRef-keyed, pre-resolve)
   prelude.rs         the .dl prelude + verb query templates
   primitives.rs      search/read/match/graph primitive evaluation
   introspection.rs   schema/describe/source-of
   ast.rs parser.rs loader.rs   1-line re-export shims → anneal_lang (NOT killed)
```

`ir::*` and `vm::*` are `pub(crate)`. Everything outside `anneal-core`
(`anneal-cli`, `anneal-mcp`) sees only the logical surface (`Value`, named `Row`)
and the eval entry points — the physical model is hidden, as intended.

---

## The data model (the win)

```
 LOGICAL (surface, output, --explain)        PHYSICAL (evaluator)
 ─────────────────────────────────────       ──────────────────────────────
 Value::String("stable")          ──intern──► PhysicalValue::Sym(SymbolId)   (Copy)
 Value::{Number,Bool,Null,List}   ─────────►  PhysicalValue::{…, List(ListId)}
 (a fact's named fields)          ──schema──► Tuple(Box<[PhysicalValue]>) columns by FieldId
 BTreeMap<Ident,StoredRelation>   ─────────►  TupleDb { RelationStore by RelationId }
   StoredRelation rows (NamedRow)             RelationStore { rows: Vec<Tuple> by RowId,
                                                indexes: BTreeMap<FieldId, RelationIndex> }
```

`PhysicalValue` is `Copy` and ≤16 bytes (size-asserted); the only `String`
heap-ownership is interned once in the `Interner`. Repeated `"stable"`/handle-ids/
statuses/kinds/edge-kinds collapse to one symbol. Stored rows are compact tuples
addressed by relation-local `RowId`, with per-column (`FieldId`) value indexes —
this is what removed the 361 MB / 3.6M-alloc `NamedRow` BTreeMap materialization
that ran on *every* query.

---

## Data flow (one query)

```
 markdown ──Source::extract──► FactBatch ──► FactStore
                                                │  TupleDb::from_store_with_visibility
                                                ▼  (lower facts → interned tuples; visibility filter applied HERE)
                                            TupleDb  (Arc, shared)
                                                │  eval.rs: scan relations via FieldId indexes
                                                │           join into Binding frames
                                                │           time scope → tuple OVERLAY (base + snapshot patch, no clone)
                                                │           primitives / aggregation / negation / order-by
                                                ▼
                                            result rows ──project (PhysicalValue→Value)──► Row{BTreeMap<String,Value>}
                                                                                          → text / json / --explain
```

Key properties that hold today:
- **Visibility/auth is construction-time** (`from_store_with_visibility` +
  `stored_row_visible`), NOT a view dimension — unchanged by the arc.
- **Time scope is an overlay, not a clone**: base tuples + a relation-patch for
  `snapshot`/`handle` synthetic rows; repeated `at("snapshot:last")` reuses it.
- **Output projection** is the only place named `BTreeMap`/`Row` is rebuilt.
- **Determinism**: canonical fact order (anneal-9sdn) is preserved through lowering.

---

## Plan vs actual (honest gap table)

| Appendix plan | Built? | Reality |
|---|---|---|
| Interned tuple store (`TupleDb`, `RowId`, column indexes) | ✅ | the −54% win lives here |
| `PhysicalValue` (Copy, interned Sym) + `ListArena` | ✅ | eval-scoped lists, round-trip tested |
| Schema registry (`RelationSchema`, `FieldId` columns) | ✅ | built from `STORED_RELATION_DESCRIPTORS` |
| Tuple-native time overlay (no clone, no NamedRow rebuild) | ✅ | Phase 2 + 3.5 |
| Typed ids `SymbolId/RelationId/FieldId/RowId/ListId` | ✅ | used by store/schema |
| Slot-frame `Binding` (`VarId`→`SlotId` array of `PhysicalValue`) | ⚠️ partial | `Binding` is `SmallVec<[(Ident,Value);2]>` sorted — flatter than the old BTreeMap, but **still `Ident`-keyed and clones logical `Value`**. Hence Phase 5's small −4.7%. `VarId`/`SlotId` exist as targeted `#[allow(dead_code)]` reserves (commented) awaiting the Plan. |
| `Plan`/IR middle-end (`resolve→analyze→plan→execute` typed passes) | ❌ | **not built.** No `plan.rs`. `eval.rs` (~11k lines) still fuses analysis-adjacent logic + execution; join order is computed inline at eval time. |
| Kill `runtime/ast.rs`/`parser.rs` re-export shims | ❌ | still 1-line shims |
| Provenance as a separate row-id→derivations multimap | ⚠️ | `--explain`/trail preserved and byte-identical, but derivation lives in the eval path, not a standalone `ProvenanceStore` |

**Why this is a legitimate stopping point.** The arc's *measured* goal was the
allocation/perf ceiling, and that was the physical data model — which is done and
validated. The *structural* goal ("a compiler with a real middle-end") is only
half-realized: the substrate is clean and modular, but the IR pass-pipeline and
true slot frames were correctly deferred because the re-measure showed their
remaining bucket was small and they're the highest-risk, net-new work. The
unused `VarId`/`SlotId` are scaffolding reserved for that next milestone.

---

## What this opens (next optimization surface — feeds the re-profile)

The clean substrate makes these tractable that weren't before; the post-arc
re-profile (`2026-06-04-post-arc-profile.md`, codex) sizes them:
- **The Plan/IR middle-end + true slot frames** (coupled): assign `VarId→SlotId`
  in a plan pass, make `Binding` a `PhysicalValue` slot array. Removes the
  remaining `Ident`-keyed + logical-`Value`-cloning eval churn. Also the home for
  join-order optimization. This is the big unbuilt piece.
- **Columnar tuple storage**: the appendix's deferred "measured-if-scans-dominate"
  choice; the tuple store is the prerequisite.
- **Interner-enabled opts**: symbol-id comparisons/dedup in hot predicates.
- **Extraction** (~387 MB on `status`): the largest *non-eval* bucket, but it's
  the `anneal-md` adapter axis — a parallel track, not this engine.
