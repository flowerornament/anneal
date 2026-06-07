# kftp Slices 2-5 Code Review

Date: 2026-06-06
Review arc: anneal-kbgj
Status: in progress

## Scope

This review covers the kftp planned-executor migration from stored-scan provenance through interpreted-evaluator retirement:

- slice 2: stored-scan provenance, positive-DAG stage scheduling, entropy authoritative, auto-migration certificate, scalar aggregates, `TakeUntil`
- slice 3: planned `TimeScope`
- slice 4: query-local planned execution
- slice 5: accidental fallback cleanup, planned recursion, recursion goldens, interpreted evaluator deletion

The review bar is architectural coherence first: did the plan capture meaning once, did the executor get duller, and did the migration leave fewer concepts behind?

## bd Arc

- `anneal-kbgj`: Review kftp slices 2-5 architecture coherence
- `anneal-kbgj.3`: Review plan certificate and staged executor coherence
- `anneal-kbgj.1`: Review provenance recursion and golden coverage
- `anneal-kbgj.2`: Review post-kftp simplification and cleanup cuts
- `anneal-kbgj.4`: Fix planned-only function-call panic
- `anneal-kbgj.5`: Make the plan fully authoritative in eval dispatch

## Research Lens

This pass used the checked-in compiler/adoption research topic map as a non-binding review lens. Reminders applied here:

- Observable interpreter behavior becomes API. The byte-identical differential and recursion goldens are the right retirement gate.
- Compilation artifacts should be inspectable. `StageMigration { mode, reasons }` is a good direction; review should keep asking whether it explains the plan's decision.
- Runtime performance comes from explicit representation choices, not a "sufficiently smart" compiler. Review should prefer plan-owned decisions over executor rediscovery.

## Initial Findings

### High: Planner Errors Can Panic The Planned-Only CLI

The language parser accepts expression function calls, and analysis only rejects named function arguments ([analysis.rs:1078](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/analysis.rs:1078), [parser.rs:1542](/Users/morgan/code/anneal/crates/anneal-lang/src/parser.rs:1542)). The planner rejects every `Expr::FunctionCall` as `PlanError::UnsupportedExpression` ([plan.rs:1565](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1565)). `ensure_planned` then panics because planning failure is treated as impossible after analysis ([eval.rs:4012](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4012)).

Reproduced:

```bash
./target/debug/anneal -e '? lower("A") = "a".' --format=json
```

The command panics at `eval.rs:4018` with `analyzed program should plan before planned execution: unsupported expression in planning-only artifact`.

This is release-blocking. The concrete repro is `FunctionCall`, but the bug class is broader: once the interpreter is retired, `PlanError` can no longer be treated as an internal impossibility for any syntax that parse/analyze still accepts.

Tracking: `anneal-kbgj.4`.

Suggested follow-up: first make `ensure_planned` and `eval_query` planning propagate `PlanError` as a user-facing `EvalError`, mirroring the stage-level `PlannedExecutorUnsupported` path ([eval.rs:4227](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4227)). Then decide the FunctionCall-specific surface separately: either implement the documented expression functions in the planner/evaluator, or reject them during analysis with a normal `StaticError`.

### Medium: The Plan Is Not Fully Authoritative Yet

Two findings point at the same coherence gap. `Evaluator` stores `planned: Option<ProgramPlan>` and lazily fills it in `ensure_planned` ([eval.rs:3922](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3922), [eval.rs:4012](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4012)). `eval_query` has a second local planning path when `self.planned` is absent ([eval.rs:4022](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4022)).

Separately, `run_fixpoint_matching` filters/clones `Rule` values out of the analyzed program, then passes both AST rules and `StratumPlan` into `run_rule_group` ([eval.rs:3966](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3966), [eval.rs:4000](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4000)). `run_staged_rule_group` rebuilds `rules_by_predicate` from those rules before consulting the stage plan ([eval.rs:4193](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4193)).

The planned executor is authoritative, but this keeps one foot in the old representation: the evaluator still has a "maybe planned" shape, and the runner still needs AST-rule selection to decide which planned groups are active.

Suggested follow-up: one merged coherence cut: make the evaluator own a mandatory `ProgramPlan`, make active-stage selection plan-owned, and drop the `rules: &[Rule]` bridge from the executor. The executor should dispatch over `StratumPlan`/`RuleStagePlan` directly.

### Low: Planned Eval Needs A Context Object

The planned path now threads `catalog`, `database`, `warnings`, `options`, `env`, and sometimes `delta` through most executor functions ([eval.rs:4459](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4459), [eval.rs:4573](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4573), [eval.rs:4615](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4615), [eval.rs:4786](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4786)).

The sprawl is understandable during migration, especially for warning/provenance parity, but it now obscures the executor boundary. A `PlannedEvalCtx` would make scope changes, warning threading, and list/interner lifetime easier to reason about.

Suggested follow-up: introduce `PlannedEvalCtx` after this review, not during it. This overlaps the existing polish bead `anneal-txkp`.

### Low: `Rank` Recomputes Sort Keys

`eval_planned_rank` evaluates the key expression inside the sort comparator and then evaluates it again during output/rank assignment ([eval.rs:5233](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:5233)).

This is not a correctness problem, but it is an easy post-migration efficiency cut. Decorate rows with `(key, frame)` once, sort the decorated rows, then consume the cached keys in the rank loop.

Tracking: `anneal-txkp` ("generic sort unification").

### Low: Recursion Goldens Are Stable But Opaque

The recursion suite now checks planned output against byte count plus FNV digest of the tuple/provenance JSON ([eval.rs:7737](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7737), [eval.rs:7780](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7780), [eval.rs:11697](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:11697)).

That is compact and avoids bulky fixture churn, but a failure will say "digest drifted" without showing the semantic drift. It also pretty-serializes every tuple/provenance row before hashing, including a 163 KB chain golden ([eval.rs:7749](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7749), [eval.rs:7773](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7773), [eval.rs:11707](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:11707)). The suite is now the only recursion oracle after interpreted deletion, so its failure mode matters.

Suggested follow-up: keep the digest gate, but improve assertion diagnostics by printing the first differing row and, when useful, writing temporary failure artifacts under `target/`. Avoid promoting bulky stored fixtures unless a specific golden needs that extra oracle. If the suite grows, consider compact serialization or a streaming digest plus first-drift output so recursion coverage stays cheap.

### Low: Stale `v2` And `legacy` Language Still Needs A Naming Pass

The one-engine runtime still contains public-facing or near-public comments/docs with old framing, for example `anneal-cli` crate docs say the crate owns runtime commands "while the legacy crate" existed ([lib.rs:1](/Users/morgan/code/anneal/crates/anneal-cli/src/lib.rs:1)), `anneal-core` crate docs still call the substrate "anneal v2" ([lib.rs:1](/Users/morgan/code/anneal/crates/anneal-core/src/lib.rs:1)), and history comments refer to "v2 snapshot" entries ([history.rs:12](/Users/morgan/code/anneal/crates/anneal-core/src/history.rs:12)).

Some `legacy` terms are still correct, especially `anneal.toml` migration handling, so this should not be a blind rename. But after the interpreted/runtime migration, stale `v2` and "legacy crate" language weakens the coherence story and can mislead future reviewers about what still exists.

Suggested follow-up: do a targeted naming pass after the architecture review. Keep compatibility labels where they describe old data formats; remove language that describes already-retired architecture.

## Refactor And File-Layout Findings

These are applications of the locked kftp success bar, not new architecture: "boring executor, rich plan" in the reconciliation doc and the standing reference-map gate that asks whether each slice made `execute` dumber.

### Medium: `eval.rs` Is Still The Runtime Architecture In One File

`eval.rs` is down from the pre-retirement peak, but it is still 12,177 lines. It contains the public `Database` representation ([eval.rs:789](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:789)), derived relation storage and indexes ([eval.rs:3744](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3744)), staged execution and recursion ([eval.rs:4193](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4193)), frame/value execution ([eval.rs:4381](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4381)), all planned atom evaluators ([eval.rs:4459](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4459)), plus the large test tail through the end of the file.

This is not a condemnation of the migration; keeping one file during the cutover made the differential safer. But after 5c, the one-engine architecture deserves module boundaries that match the new concepts and the pass-contract target: `ir/` owns the middle-end, `vm/` owns the hidden relational backend, and `runtime/` becomes the thin logical facade ([pass-contracts §12](/Users/morgan/code/anneal/.design/2026-06-02-pass-contracts.md)).

Possible end-state modules, aligned with that contract:

- `runtime/database.rs`: `Database` as the facade/composition root, until the facade can thin further over `vm`.
- `runtime/relation.rs` or `runtime/derived.rs`: `DerivedRelation`, indexes, base relation storage helpers.
- `runtime/content.rs` or `runtime/providers.rs`: content/search providers and indexes.
- `runtime/graph_index.rs`: graph primitive backing state.
- `runtime/time_scope.rs`: tuple overlay and scoped snapshot mechanics.
- `vm/execute.rs` or an `execute/` subtree under `vm`: staged runner, recursive loop, frame/value execution, atom dispatch, aggregate helpers, and planned provenance. The exact file shape should follow the pass-contract boundary rather than invent a second runtime-owned VM.

Tracking: `anneal-kbgj.2`.

This should be an incremental decomposition arc, not one heroic move. Start with the cheapest review-cost reducers, then reassess:

1. Move the architecture-critical tests/goldens into owning test modules without changing production code.
2. Extract one leaf runtime area, probably `DerivedRelation`/indexes or planned aggregate helpers.
3. Extract time-scope or recursion only after the leaf move proves the visibility seams are clean.
4. Finish each move with named gates: the planned-only panic repro, query-local planned execution, recursion goldens, a representative `at()`/`--explain` query, and `just check`.

Preservation checklist: mechanical moves only; no logic cleanup in the same commit; no visibility widening solely to satisfy moved tests; keep module-private seams where possible.

### Medium: `plan.rs` Mixes Catalog, Lowering, Scheduling, And Certification

`plan.rs` is 2,495 lines and currently owns several separate compiler phases: `PlanCatalog` ([plan.rs:367](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:367)), top-level `plan()` ([plan.rs:587](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:587)), global/query lowering ([plan.rs:602](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:602), [plan.rs:634](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:634)), positive stage scheduling/SCCs ([plan.rs:759](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:759)), migration certification ([plan.rs:946](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:946)), time-scope support checks ([plan.rs:1024](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1024)), and atom/expression lowering ([plan.rs:1201](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1201)).

The file is coherent in concept, and the Plan/IR reconciliation already locked the main boundary: `plan()` is the middle-end and the executor is supposed to be dull ([reconciliation](/Users/morgan/code/anneal/.design/2026-06-04-plan-ir-reconciliation.md)). The remaining risk is that new planned capabilities get added "wherever they fit" instead of through explicit internal pass boundaries.

Suggested split:

- `ir/plan/catalog.rs`: `PlanCatalog`, relation signatures, schema synthesis.
- `ir/plan/lower.rs`: rule/query/body/atom/expression lowering.
- `ir/plan/stages.rs`: positive dependency components, stage order, `StageExecution`, `DeltaPlan`.
- `ir/plan/certificate.rs`: `StageMigration`, `UnsupportedReason`, executable checks.
- `ir/plan/support.rs` or `ir/plan/capabilities.rs`: aggregate/comparison/time-scope support matrix, primitive provider/capability/demand lowering.
- `ir/plan/types.rs`: shared plan structs, ids, provenance payloads.

The key is not smaller files for their own sake; it is preserving the standing kftp gate from the compiler reference map: did the plan capture predicate meaning once, and did execute get dumber ([reference map](/Users/morgan/code/anneal/.design/2026-06-05-datalog-compiler-reference-map.md))? The split should make catalog, lowering, scheduling, and certification independently reviewable.

Tracking: `anneal-kbgj.3`.

### Medium: Runtime Certification Leftovers Keep Meaning In The Executor

`eval_planned_time_scope` re-checks support at runtime before every scoped execution ([eval.rs:4786](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4786), [eval.rs:4836](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4836)), even though the planner owns the same time-scope support predicate ([plan.rs:1024](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1024)). `eval_planned_aggregate` similarly validates aggregate shape at runtime ([eval.rs:4966](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4966)), and individual aggregate helpers still re-check required args ([eval.rs:5068](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:5068)).

This is mostly defensive, but it is also the same conceptual smell as the AST-rule bridge: the executor still re-certifies things the plan has already decided. In the short term, keep correctness guards. In the cleanup phase, move toward plan types that make invalid states unrepresentable, for example a pre-certified `TimeScopePlan` and aggregate arg structs where required fields are not `Option`.

Tracking: `anneal-kbgj.5` for the fully-authoritative plan cut; keep this as review evidence for the concrete cleanup shape.

### Low: Derived Indexes Clone Whole Tuples Per Indexed Column

`DerivedRelation::insert_with_derivation` stores the tuple in the set and then pushes a full `tuple.clone()` into every column index ([eval.rs:3759](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3759), [eval.rs:3769](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3769)). Candidate lookup then picks the shortest matching column index ([eval.rs:3782](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3782)).

This is a reasonable first planned-executor shape and it preserves deterministic tuple ordering, but it is a clear candidate for measurement on recursive or high-cardinality derived relations. Do not jump straight to tuple ids/arena indexes or plan-selected indexes; profile first, then pick the representation if this becomes a real bucket.

Tracking: `anneal-kbgj.2`.

### Medium: Tests Are Now Architecture-Critical But Buried In Giant Modules

The recursion goldens and planned-query tests live inside the `eval.rs` test tail ([eval.rs:7737](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7737), [eval.rs:11697](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:11697)). Planning-policy tests live at the bottom of `plan.rs` ([plan.rs:2293](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:2293)).

Those tests are no longer ordinary unit tests; they are the replacement oracle for the deleted interpreter. Their location makes them harder to scan, extend, and audit as a suite.

Suggested follow-up:

- Move recursion goldens into `#[cfg(test)] mod recursion_tests;` under the owning runtime module, or use integration tests through public surfaces.
- Move certificate/policy tests into an owning-module `#[cfg(test)]` module such as `ir/plan/certificate_tests.rs`.
- Do not widen APIs to `pub(crate)` solely because a test moved.
- For golden diagnostics and failure output, apply the earlier "Recursion Goldens Are Stable But Opaque" finding.

Tracking: `anneal-kbgj.1`.

### Low: Some Reserved-For-Later Scaffolding Is Still Honest, But Needs A Cleanup Decision

Some dead-code allowances are targeted and documented, for example reserved id types in `ir/ids.rs` ([ids.rs:24](/Users/morgan/code/anneal/crates/anneal-core/src/ir/ids.rs:24)) and `PhysicalValue::List` ([value.rs:53](/Users/morgan/code/anneal/crates/anneal-core/src/vm/value.rs:53)). Others are now stale after kftp, notably `ir/mod.rs` still says plan is a Phase 1 artifact before the executor consumes it ([mod.rs:5](/Users/morgan/code/anneal/crates/anneal-core/src/ir/mod.rs:5)), and `plan_aggregate` still mentions "old eval" executing a phase ([plan.rs:1405](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1405)).

This belongs to the same targeted cleanup pass as the stale `v2`/`legacy` wording above, not a second naming project:

- keep targeted `#[allow(dead_code)]` only when the comment names a real next use;
- delete or reword obsolete phase comments;
- remove any "planning-only" phrasing that is false after 5c.

Tracking: `anneal-kbgj.2`.

## Review Passes

1. Certificate and executor coherence (`anneal-kbgj.3`)
   - Verify `StageMigration`/`StageExecution` is the only migration decision source.
   - Check that executor dispatch never re-derives predicate kind, atom order, aggregate capability, or time-scope support.
   - Identify remaining AST-rule bridges and lazy-plan seams.
   - Track the merged authoritative-plan cleanup in `anneal-kbgj.5`.

2. Provenance, recursion, and goldens (`anneal-kbgj.1`)
   - Inspect stored, aggregate, negation, time-block, and recursive provenance shapes.
   - Confirm recursion goldens cover the intended cases and remain maintainable.
   - Look for hidden explain/warning parity traps.

3. Cleanup cuts (`anneal-kbgj.2`)
   - Rank post-kftp simplifications by coherence value.
   - Link existing beads when they already cover the issue; create new beads only for uncovered findings.
   - Watch for stale "legacy", "v2", or "interpreted" language that no longer matches the one-engine architecture.
   - Treat file decomposition as an architecture cleanup, not a speculative refactor.

## Current Verdict

The migration appears directionally sound: the plan/certificate architecture exists, recursive stages are represented explicitly, and the interpreted executor has been removed rather than left as a parallel engine.

The main review pressure is now split. The release-blocking issue is the planner-error panic path (`anneal-kbgj.4`). The highest-value coherence follow-up is making the plan fully authoritative in eval dispatch (`anneal-kbgj.5`). The remaining findings are cleanup, layout, and maintainability work that should make the successful one-engine architecture easier to understand and extend.
