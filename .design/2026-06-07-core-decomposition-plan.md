---
status: current
date: 2026-06-07
epic: anneal-pcwd
authors: [claude]
relates:
  - 2026-06-02-pass-contracts.md          # the LOCKED north star (§12 module layout, §13 old→new map)
  - 2026-06-04-runtime-architecture.md    # superseded by this doc (its gap table is pre-kftp)
  - 2026-06-04-plan-ir-reconciliation.md  # the kftp plan that built the planned executor
  - 2026-06-06-kftp-slices-2-5-code-review.md  # decomposition findings (kbgj.1/.2), plan-authority (kbgj.5)
---

# anneal-core decomposition: realizing pass-contracts §12 — 2026-06-07

This is the **scoping + de-risk plan** for `anneal-pcwd`: split `runtime/eval.rs`
along the seams the locked pass-contract appendix (`2026-06-02-pass-contracts.md`
§12/§13) already named. It is **not a new design** — §12 is locked. This doc
records (a) the corrected post-kftp as-built, (b) what §12 is now *done* vs
*remaining*, (c) the de-risk findings that fix the sequencing, and (d) the gated
order of moves. It supersedes `2026-06-04-runtime-architecture.md`, whose gap
table predates the kftp arc and now reads as a stale-`status:active` doc — the
exact failure the retrieval-currency work (`anneal-z4x3`) exists to catch.

One-line status: **kftp built the planned executor and the IR/VM substrate; it
did not yet pull the executor *out* of `eval.rs`.** `eval.rs` is 12,183 lines —
34% of `anneal-core` (35,823). The mass is one file, not uniform bloat.

---

## What kftp changed (correcting the 2026-06-04 gap table)

The prior as-built doc said the Plan/IR middle-end was `❌ not built`, `Binding`
was still `SmallVec`/`Ident`-keyed, and there was no `plan.rs`. The kftp arc
(slices 1a→5c, `298432a`; panic fix `f7fef31`) reversed all three:

| 2026-06-04 said | Now (post-kftp) |
|---|---|
| `Plan`/IR middle-end ❌ not built | ✅ `ir/plan.rs` (2,581 lines): `plan()`, `StageMigration` certificate, `StageExecution::{SinglePass,Recursive}`, `PlanCatalog` |
| `Binding` = `SmallVec<[(Ident,Value);2]>`, clones logical `Value` | ✅ `PlannedFrame` (`VarId→SlotId` `PhysicalValue` slots); `SmallVec` `Binding` **deleted** |
| join order computed inline at eval time | ✅ stage plan owns ordering; executor consumes `StratumPlan`/`RuleStagePlan` |
| interpreted evaluator is the runtime path | ✅ **one** planned executor; interpreted `eval_rule`/`eval_body_traced` **deleted** (slice 5c) |
| recursion: interpreted only | ✅ planned semi-naive `DeltaPlan`; recursion goldens green; murail byte-identical |

So §12/§13 are **partially realized**. The substrate (`ir/`, `vm/`) exists and is
`pub(crate)`-clean; the executor was *built planned* but **still lives inside
`eval.rs`** rather than in `vm/execute.rs`.

---

## §12 target vs current module reality

```
pass-contracts §12 target           current state (2026-06-07)
─────────────────────────           ──────────────────────────────────────────
ir/  ids.rs                          ✅ ir/ids.rs (66)
     interner.rs                     ✅ ir/interner.rs
     schema.rs                       ✅ ir/schema.rs (194)
     resolved.rs   (resolve)         ⚠️ resolve in anneal-lang; no ir/resolved.rs
     analyzed.rs   (analyze)         ❌ analyze still in runtime/analysis.rs (2,676), string/PredicateRef-keyed
     plan.rs       (the IR owner)    ✅ ir/plan.rs (2,581)
     source_map.rs                   ❌ diagnostics still inline
vm/  value.rs                        ✅ vm/value.rs (157)
     store.rs      (Tuple/TupleDb)   ✅ vm/store.rs (868)
     frame.rs      (Frame+mask)      ❌ PlannedFrame lives in eval.rs
     view.rs       (DbView overlay)  ⚠️ overlay logic in vm/store.rs, not a named view.rs
     provenance.rs (RowId→derivs)    ❌ DerivationNode + derived store in eval.rs
     execute.rs    (execute(Plan))   ❌ the planned executor IS eval.rs — the 12k fusion
runtime/  thin logical façade        ❌ runtime/ = 20.7k; eval.rs holds execute+fixpoint+
     (eval.rs decomposes away)          primitives+aggregation+negation+time-overlay+
                                        order-by+provenance+projection; NO public façade
```

The decomposition is: **move the contents of `eval.rs` into the `vm/` modules §12
already names**, leave `runtime/` as the thin façade, and re-key analysis into
`ir/` last (highest-risk, not required to tame `eval.rs`).

---

## De-risk findings (these set the order)

**A — `kbgj.5` (plan authority) is the *enabling refactor*, not cleanup.**
§12 states an invariant: *"the executor depends on `Plan`, **never** on the
parser or surface AST."* Today it doesn't hold cleanly: the executor positionally
indexes the **analysis** strata (`run_fixpoint_matching` eval.rs:3982–4005;
`eval_query` matches queries by AST identity + indexes plan positionally
eval.rs:4019–4048) and **re-derives executability at runtime** that the plan
already computed (`planned_aggregate_executable` eval.rs:5059;
`planned_comparison_executable` eval.rs:6063; `time_scope_unsupported_atom`
eval.rs:4831). You cannot draw a clean `vm/execute.rs | runtime/analysis`
boundary while the executor reaches back into analysis. **`kbgj.5` realizes the
§12 invariant — `pcwd` depends on it.** (bd dep recorded.)

**B — façade before lock; do not big-bang the boundary** (§12 verbatim).
`anneal-cli` (`app.rs`) and MCP reach around the boundary today — importing
`AnalyzedProgram`, `Atom`, `Body`, `Expr`, `Evaluator`, `Database`,
`parse_program`, `analyze`. Locking `pub(crate)` on `ir/vm` before a **public
`runtime` façade** exists would break those call sites. The façade
(`parse`/`analyze`/`eval`/schema-lookup/verb-registry/hint helpers, all speaking
the logical surface) is its own gated step; CLI/MCP migrate onto it *before* the
internals seal.

**C — analysis re-key is the highest-risk move; defer it.**
`runtime/analysis.rs` is string/`PredicateRef`-keyed, pre-resolve. Porting it to
`ir/analyzed.rs` typed-id (per §13) touches the analyze↔plan boundary and is
net-new risk. It is **not required to get `eval.rs` under control** — pulling
`execute`/`frame`/`provenance`/`view` out of `eval.rs` is the high-value,
lower-risk win and lands first. Re-key analysis as a later, separately-gated step.

**D — the surface layer is non-colliding and parallelizable.**
`ranking.rs` (1,837) + `project.rs` (1,540) + `verbs.rs` (1,358) +
`retrieval.rs` (353) ≈ 5,088 lines are already top-level modules with **no edit
overlap with `eval.rs`**. They can be tidied/regrouped in parallel with the
executor extraction. This is the `anneal-query` crate candidate — but **start as
a module grouping; promote to a crate only if the boundary needs
compiler-enforcing** (§14). Substrate stays one crate (AGENTS.md: engine internal
to `anneal-core`).

---

## Gated sequence

Each step: behind the locked §12/§13 contracts; differential **byte-identical on
murail** (`~/code/murail/.design`); recursion goldens green; `just check` clean.
Pre-change binary built in a **/tmp clone** (never a worktree — `anneal-re9h`).
Tests **follow the code they cover** out of `eval.rs` (addresses `kbgj.1`: the
recursion goldens are the executor's oracle and must travel with `vm/execute.rs`).

0. **(in flight, codex)** `kbgj.5` — plan fully authoritative; executor consumes
   `Plan`, stops re-deriving capability and positionally-indexing analysis.
   *Unblocks a clean execute boundary.* Also `kbgj.1`/`kbgj.2` cleanup (stale
   wording: `ir/mod.rs:5`, `plan.rs:1`, `plan.rs:4-5`, `plan.rs:1405` still say
   "planning-only"/"old eval"/"Phase 1" — false post-5c).

1. **`vm/execute.rs`** — lift the planned stage runner + delta loop out of
   `eval.rs` (the bulk). Depends on step 0. Recursion goldens move with it.

2. **`vm/frame.rs`** — `PlannedFrame` (slot array + bound mask) → its own module.

3. **`vm/provenance.rs`** — `DerivationNode` + derived store + `--explain`/trail
   wiring → `ProvenanceStore` (§13: RowId→derivations multimap). Keep `--explain`
   byte-identical.

4. **`vm/view.rs`** — name the `DbView` overlay (time/visibility/derived) as a
   module; today it's folded into `vm/store.rs`.

5. **public `runtime` façade** (de-risk B) — introduce the façade services;
   migrate `anneal-cli`/MCP call sites onto them; *then* tighten `ir/vm` to
   `pub(crate)`.

6. **`ir/analyzed.rs`** (de-risk C, separately gated, lower priority) — port
   `runtime/analysis.rs` into `ir/` and re-key to typed ids.

**Parallel track (de-risk D, any time):** regroup the surface-composition layer
(`ranking`/`project`/`verbs`/`retrieval`). Promote to an `anneal-query` crate
only once the boundary is stable and earned — not before.

Fold-ins: `anneal-txkp` (PlannedEvalCtx param-sprawl; `eval_query` plan caching;
QueryOutputPlan shape; Tarjan/SCC refactor; sort unification) are decomposition
concerns — absorb them into the steps they touch rather than as a separate pass.

---

## Progress (updated 2026-06-07)

Underway, each step byte-identical on murail (status/check + the relevant
`--explain` surface) with 344/344 `anneal-core` tests and `just check` green per
commit. `eval.rs` 12,183 → 10,259 so far (−16%).

| step | module | commit | gate beyond status/check |
|---|---|---|---|
| leaf | `vm/frame.rs` | `0e3dd9a` | — |
| leaf | `vm/provenance.rs` | `c8a20ef` | `--explain` identical |
| leaf | `vm/view.rs` | `bae4a32` | `at(snapshot:last) --explain` identical |
| bulk | `vm/execute.rs` (executor core) | `2c92a70` | real-recursion `dep_path` `--explain` identical; §12 grep-evidenced |

The executor core is out of `eval.rs` and Plan-driven — no `analysis`/raw-AST
imports (machine-checked by grep in the commit report). Residual `vm/* →
runtime::eval::{Database, Value, ExplainOptions, …}` edges are explicit and named
at single import blocks, to resolve when those types relocate to boundary
modules.

Note on staging: step 1 above was split in execution — the planned **executor
core** moved to `vm/execute.rs` while the **coordinator/fixpoint** (`Evaluator`,
`run_fixpoint*`, the stage runner) deliberately stayed in `runtime/eval.rs` for a
separate move, to keep each commit's byte-identical blast radius understandable.

Remaining: coordinator/fixpoint extraction (planning in progress), then the
residual relocations (`Value`/`ExplainOptions`/`DbView`), `PlannedExecCtx`
param-sprawl, the public runtime façade (step 5), and the deferred
`ir/analyzed.rs` re-key (step 6).

## Done-when

`eval.rs` is no longer the center of gravity: `vm/execute.rs` owns execution,
`runtime/` is a thin logical façade, `ir/vm` internals are `pub(crate)` with
`anneal-cli`/MCP on the public façade, and the §12 module map is realized (modulo
the deliberately-deferred `ir/analyzed.rs` re-key and any unearned crate splits).
No behavior change at any step — the logical `Value`/`Row`/AST surface is
preserved throughout (§13). This is the clean substrate the feature work
(`anneal-z4x3` currency, `anneal-xhpl` navigate) then builds on — features land
*after* the substrate is legible, not into the 12k monolith.
