---
status: locked
date: 2026-06-04
epic: anneal-kftp
authors: [claude]
reviewers: [codex]
relates:
  - .design/2026-06-02-pass-contracts.md       # the locked appendix (§§6-8 = the design)
  - .design/2026-06-04-runtime-architecture.md # as-built map
  - .design/2026-06-04-post-arc-profile.md     # why (ranked #1 eval surface)
  - .design/2026-06-05-datalog-compiler-reference-map.md # Datalog compiler reference map + standing review gate
---

# Plan/IR middle-end — reconciliation (anneal-kftp Phase 0) — 2026-06-04

The pass-contract appendix (`§§6-8`) already specs the Plan/IR middle-end and
true slot frames, locked after codex review. But it was written assuming a
fuller greenfield than what the arc actually built. This doc reconciles the
locked design against as-built reality and fixes the **grafting decision** before
any Plan code.

**This amends the appendix migration path (`§§6-7`)** (codex): the appendix said
*resolve before analyze* and *re-key analysis to typed ids*. This milestone
deliberately does the opposite — `ResolvedProgram` collapses into a `PlanCatalog`
+ `plan()`; typed-id analysis is deferred unless later measured as needed. The
appendix's `§8` Plan/Frame contracts are still built as specced; only the
*ordering and the resolve artifact* change.

## As-built reality (what the Plan must attach to)

- `analyze(Program) -> AnalyzedProgram` is **string/`PredicateRef`-keyed over the
  surface AST**; `AnalyzedProgram` stores the raw `Program`, `strata` are
  `Vec<PredicateRef>`. It is proven and unchanged by the arc.
- **There is no resolve pass and no `ResolvedProgram`.** The appendix's
  `resolve → analyze` ordering does not exist; analyze consumes strings directly.
- `Evaluator { program: AnalyzedProgram, database }` where `database` is now the
  **tuple store** (`TupleDb`, Phase 3). `run_fixpoint_for_query` iterates
  `program.strata()`, collects rules per stratum, and `run_rule_group`
  **interprets them inline** — join order, primitive dispatch, aggregation,
  negation all computed at eval time. No `Plan` artifact.
- `Binding = SmallVec<[(Ident, Value); 2]>` (sorted, `Ident`-keyed, clones
  logical `Value`). `VarId`/`SlotId` exist as reserved `#[allow(dead_code)]`.

So: the **backend substrate exists** (tuple store), the **frontend+analysis
exist** (string-based), and the **missing layer is exactly the middle-end** — the
appendix's central claim, confirmed.

## Central decision: fold `resolve` INTO a single `plan()` pass

The appendix models four artifacts (`Program → ResolvedProgram → AnalyzedProgram
→ Plan`). Rebuilding `analyze` onto typed ids + adding a separate `resolve`
artifact is the purist path but high-disruption: it rewrites proven, string-based
stratification/safety for no measured win (analysis isn't an allocation bucket).

**Recommendation: keep `analyze` as-is (string-based, pre-resolve) and add ONE
new `plan()` pass that folds resolution into planning.**

```
 parse → analyze (AS-IS, string/PredicateRef)
              │  plan()  ── NEW: the middle-end
              ▼           resolve PredicateRef→RelationId (via SchemaRegistry),
        Plan { rule groups, slot layouts, atom plans }   Ident var→VarId→SlotId,
              │  execute(&Plan, &TupleDb)                 atoms→AtomPlan (rel/field/slot ids)
              ▼  ── NEW: plan-driven run_rule_group over the tuple store + slot Frame
        QueryOutput  (project ids→names at the boundary, unchanged)
```

- `plan()` consumes `AnalyzedProgram` and emits the appendix's `Plan` (`§8`).
  Resolution (`PredicateRef→RelationId`, `Ident→VarId/SlotId`, exprs→slot
  reads/literals) happens HERE — "resolve" is a step inside plan(), not a
  separate artifact. The appendix's `ResolvedProgram` collapses into plan state.
- `execute` replaces the inline `run_rule_group`, binds into a `PhysicalValue`
  slot `Frame` (`VarId→SlotId`), runs primitives/aggregates/negation/time-scope
  from the plan. The `SmallVec<(Ident,Value)>` binding retires here.

Four things the reconciliation MUST get right (codex review — the doc was naive
about each; these are now part of the contract):

1. **A `PlanCatalog`, because `SchemaRegistry` only covers stored builtins.**
   The current registry (`ir/schema.rs`) registers the ~9 stored relations from
   `STORED_RELATION_DESCRIPTORS` with `&'static str` fields. `plan()` needs
   schemas for **global derived predicates, query-local predicates, primitive
   signatures, and projections** too. Build a `PlanCatalog` step inside `plan()`
   that registers all of these — reusing the derived signatures analysis already
   computes (`PredicateSignature`/`ParameterNames`, private in `analysis.rs`,
   which must be exposed) and accepting **owned** `Ident`/`SymbolId` field names,
   not just `&'static str`. Do not pretend the stored registry resolves every
   `PredicateRef`.

2. **Resolve predicate KIND from analysis, not from the name (soft-primitive
   trap).** `active`/`terminal`/etc. are *soft primitives* a corpus can redefine
   as derived; eval already prefers the derived relation when one exists
   (`eval.rs` soft-primitive check). A planner that calls
   `PrimitivePredicate::from_predicate(name)` alone will **miscompile an
   overridden soft primitive**. `plan()` must read the analyzed predicate kind
   from the catalog/signature; `from_predicate` is valid only after the kind says
   "primitive."

3. **The execution target is an `ExecutionContext`/`DbView` over `Database`, not
   raw `TupleDb`.** `Database` wraps `TupleDb` *plus* graph/content/search
   providers, introspection, trail/dynamic rows, the tuple overlay, derived
   relations, hidden spans, and `EvalOptions`/policy. Primitives need provider
   state + `EvalOptions` authorization. `TupleDb` is the stored-relation
   substrate only; planned execution runs against the fuller context.

4. **Slot frames need an interner + list-arena for *eval-produced* values.**
   Stored tuples are pre-interned, but `search`/`read`/`match`/introspection
   produce **new** strings at eval time and aggregates materialize lists
   (`PhysicalValue::List` is currently reserved/test-only). The executor must own
   or borrow a mutable interner + eval-scoped `ListArena` facade compatible with
   `TupleDb`'s interner, with the escape rule: project `PhysicalValue → Value`
   before any row/provenance escapes the eval scope (per `runtime-architecture`
   doc + appendix §3).

`plan()` is **compilation, not a second analyzer** (codex): safety/stratification
are proven by `AnalyzedProgram`'s construction; `plan()` does slot layout, var
collection, and ready-set ordering, and may *assert* internal invariants, but
must not re-run user-facing validation.

This grafts onto the as-built with a catalog + one new pass + one new executor,
keeps the proven analysis, and reuses the tuple backend. It is the appendix's
architecture, attached at the real seam.

## What stays vs changes vs is deferred

| Appendix | Disposition |
|---|---|
| `ResolvedProgram` (separate resolve artifact) | **folded into `plan()`** — not a standalone pass |
| `AnalyzedProgram` re-keyed to typed ids | **NOT done** — analyze stays string-based; plan() does the id lowering |
| `Plan` (`§8`, all node types) | **built as specced** |
| `Frame` (VarId→SlotId, PhysicalValue) | **built as specced** — the true slot frame |
| `execute(&Plan, &DbView)` | **built** as plan-driven execution over an `ExecutionContext`/`DbView` over `Database` (providers + options + overlay), NOT raw `TupleDb` |
| `PlanCatalog` (schema synthesis for derived/local/primitive) | **NEW** — not in the appendix; required because the stored registry is incomplete |
| `〔MEASURED〕` AtomPlan/SlotLayout reps | **settled by the Phase-1 spike** |
| kill `ast.rs`/`parser.rs` shims | optional cleanup, not required for the win |

### Node-lowering specifics the spike must not under-power (codex)

- **Aggregates are the hardest node, especially `Rank`.** `AggregatePlan` must
  explicitly model **outer slots, inner slots, a synthetic rank-var slot, and
  result unification** — `Rank` sorts inner rows, injects the rank var into each
  inner binding, then evaluates the result expr; `TopK`/`TakeUntil` evaluate
  key/sum/budget exprs with distinct outer-vs-inner binding. Name these cases in
  `AggregateArgsPlan` so a representation can't be chosen that can't express them.
- **`TimeScope` is subtree execution over a scoped `ExecutionContext` view** —
  validate support, scope the context (tuple overlay **and** graph-primitive
  scope), evaluate only the subtree, rejoin outer bindings. Bind the plan node to
  the as-built overlay, not just relation scans.
- **`Negation`** lowers to an inner planned body + bound-input slots.
- **`PrimitiveCall`** carries provider + capability + demand and runs against the
  `ExecutionContext` (point 3 above), not the bare store.

## Phase plan (anneal-kftp)

0. **Reconciliation** (this doc) → codex review → **locked**.
1. **Planning-only artifact FIRST, no execution change** (codex's lower-risk
   sequencing). Build `PlanCatalog` + schema synthesis + `plan()` that lowers
   rules/queries to a `Plan`, with tests asserting the lowering — but **still
   execute the old eval**. This catches the schema-gap and soft-primitive-override
   bugs *before* any frame-execution churn. Validate: every prelude + murail query
   *plans* without error; planned predicate kinds match analysis (esp. a
   corpus-overridden soft primitive resolves to derived, not primitive).
2. **Spike one planned executor path** behind a flag: `plan()` + slot-frame
   `execute` for ONE hot rule group, differentialed (SHA-worktree vs current) +
   measured. **Spike fixture must exercise the traps**: a stored scan, a graph
   primitive (`active`/`pipeline_position`), an aggregate, AND a corpus-defined
   **soft-primitive override** — not just one clean scan. Settles the
   `〔MEASURED〕` reps. Decision gate.
3. **Migrate** incrementally with an explicit **bridge**: planned and interpreted
   rule groups must coexist mid-migration, so either (a) store derived relations
   in one shared physical format and project for legacy groups, or (b) run planned
   groups in shadow/differential mode until a stratum is wholly migrated. Each
   step differential-gated (derived counts + `at()` + `--explain` byte-identical
   on murail) + property tests. **Query-local rules are a SEPARATE sub-slice**
   (they clone `Database` + install query introspection in `eval_query` — not
   auto-covered by global group migration). Retire the `SmallVec<(Ident,Value)>`
   binding as slot frames take over.
4. **Settle**: join-order in the plan (the optimization the middle-end unlocks),
   then re-profile.

## Success bar — coherence, not just speed (Morgan, locked)

**This epic is judged by architecture, simplicity, and coherence — performance is
a side effect, not the goal.** The north star: **boring executor, rich plan.**

- `parse`/`analyze` = validity. `PlanCatalog`/`plan()` = one-time resolution of
  names, schemas, slots, providers, and scope. `execute` = a dull slot-frame VM
  over the scoped runtime context.
- `plan()` must be a **real simplifying boundary**, NOT a second evaluator beside
  the old one. The system should move from "eval rediscovers what an atom means
  every time" to "the compiler says what this program means once." That is the
  coherence win, and the **`PlanCatalog`/schema layer is the make-or-break part**.
- Migration may briefly grow the code, but **every slice must bias toward
  retiring rediscovery and making the executor duller**. A slice that makes eval
  faster while leaving runtime decisions scattered across `eval_body`-style
  branches has missed the point and should be reworked, not accepted.
- Concretely: scattered `if soft && derived…`, name-based kind dispatch, per-atom
  field-name lookups, and re-derived constraints should *disappear* from the
  executor as the plan absorbs them. Reviews check "did the executor get dumber?"
  not just "did dhat drop?"

### Standing review gate

Every planned-executor slice is reviewed against the standing gate defined in
`.design/2026-06-05-datalog-compiler-reference-map.md` ("Standing kftp review
gate"). Its seven questions:

1. Did the plan capture predicate meaning once? (relation kind/id, field ids, arity, primitive provider/capability/demand, soft-primitive override status — resolved before execution)
2. Did execute get dumber? (no name-based predicate dispatch, no runtime greedy readiness scheduling, no field-name lookup, no "is this primitive?" rediscovery in the migrated path)
3. Did variable binding become a planned slot contract? (each atom knows input + output/binding slots; unsupported shapes fail at plan time)
4. Did provenance survive as data? (authoritative planned paths emit byte-identical derivations; shadow gates compare tuple→derivation maps)
5. Did recursive control stay separate from rule logic? (strata/fixpoint/delta own recursive control; rule plans own the relational body)
6. Did we avoid dual representations? (no internal NamedRow rebuilds, no parallel row models, no projection before the output boundary)
7. Did we defer incremental/caching until the one-shot executor is clean?

## Success criteria (performance — the side effect)

The post-arc profile's remaining eval buckets — stored candidate/result vectors,
constraint building, derived-relation eval, SmallVec binding clones — collapse
together (plan compiles atoms to ids/slots ONCE instead of rebuilding per atom).
Status churn drops below 1.33 GB; results byte-identical throughout. Correctness
is the floor, exactly as the arc.

## Review log

- **2026-06-04 — codex adversarial review (revise-then-lock → LOCKED).** Central
  graft confirmed (fold resolve into `plan()`, keep `analyze` string-based).
  Amendments folded: (1) `PlanCatalog` + schema synthesis for derived/local/
  primitive predicates — the stored registry is incomplete; (2) resolve predicate
  kind from analysis, not name — the soft-primitive-override trap; (3) execution
  target is `ExecutionContext`/`DbView` over `Database`, not raw `TupleDb`;
  (4) executor needs a mutable interner + eval-scoped list arena for primitive/
  aggregate outputs + the project-before-escape rule; (5) incremental-migration
  bridge (shared physical format or shadow mode) + query-local rules as a
  separate sub-slice; (6) aggregate node must model outer/inner/rank slots +
  unification; (7) `TimeScope` = subtree over scoped context; (8) `plan()` is
  compilation not a second analyzer; (9) stated plainly as amending appendix
  §§6-7; (10) Phase-1 split into planning-only-artifact FIRST then executor spike,
  with a soft-override spike fixture. Locked.
