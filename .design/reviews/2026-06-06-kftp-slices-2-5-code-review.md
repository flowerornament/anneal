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

## Research Lens

The local `qmd` research-graph query failed with `SQLITE_CANTOPEN` during review startup, so this pass used the checked-in topic map `~/code/systems-research-graph/notes/compiler-and-adoption.md` as a fallback lens. Non-binding reminders applied here:

- Observable interpreter behavior becomes API. The byte-identical differential and recursion goldens are the right retirement gate.
- Compilation artifacts should be inspectable. `StageMigration { mode, reasons }` is a good direction; review should keep asking whether it explains the plan's decision.
- Runtime performance comes from explicit representation choices, not a "sufficiently smart" compiler. Review should prefer plan-owned decisions over executor rediscovery.

## Initial Findings

### High: User-Written Function Calls Can Panic The Planned-Only CLI

The language parser accepts expression function calls, and analysis only rejects named function arguments ([analysis.rs:1078](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/analysis.rs:1078), [parser.rs:1542](/Users/morgan/code/anneal/crates/anneal-lang/src/parser.rs:1542)). The planner rejects every `Expr::FunctionCall` as `PlanError::UnsupportedExpression` ([plan.rs:1565](/Users/morgan/code/anneal/crates/anneal-core/src/ir/plan.rs:1565)). `ensure_planned` then panics because planning failure is treated as impossible after analysis ([eval.rs:4012](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4012)).

Reproduced:

```bash
./target/debug/anneal -e '? lower("A") = "a".' --format=json
```

The command panics at `eval.rs:4018` with `analyzed program should plan before planned execution: unsupported expression in planning-only artifact`.

This is the most important miss from the first review. It is a planned-only surface regression class: once the interpreter is retired, `PlanError` can no longer be treated as an internal impossibility for any syntax that parse/analyze still accepts.

Suggested follow-up: either implement the documented expression functions in the planner/evaluator, or reject them during analysis with a normal `StaticError`. Independently, replace the panic in `ensure_planned`/query planning with a user-facing error so unsupported planner gaps cannot crash the CLI.

### Medium: `Evaluator` Still Treats The Plan As Optional

`Evaluator` stores `planned: Option<ProgramPlan>` and lazily fills it in `ensure_planned` ([eval.rs:3922](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3922), [eval.rs:4012](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4012)). `eval_query` has a second local planning path when `self.planned` is absent ([eval.rs:4022](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4022)).

This is no longer a correctness issue now that interpreted fallbacks are gone, but it preserves the old "maybe planned" shape after the architecture has become planned-only. It also means query evaluation can construct a separate `ProgramPlan` instead of using a single evaluator-owned artifact.

Suggested follow-up: make `ProgramPlan` mandatory at evaluator construction, or introduce one plan accessor that caches and returns the same plan for fixpoint and query output. This should stay small and be covered by tests for `eval_query` before/after `run_fixpoint`, including query-local rules.

### Medium: The Staged Runner Still Bridges Through AST Rules

`run_fixpoint_matching` filters/clones `Rule` values out of the analyzed program, then passes both AST rules and `StratumPlan` into `run_rule_group` ([eval.rs:3966](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:3966), [eval.rs:4000](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4000)). `run_staged_rule_group` rebuilds `rules_by_predicate` from those rules before consulting the stage plan ([eval.rs:4193](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4193)).

The planned executor is authoritative, but this keeps one foot in the old representation: the runner still needs AST-rule selection to decide which planned groups are active. That weakens the "plan owns the schedule" boundary and makes the executor harder to audit.

Suggested follow-up: move active-rule selection fully into `Plan`/`StratumPlan` and let the runner dispatch over `RuleStagePlan`/`RuleGroupPlan` directly. The executor should not need a parallel `rules: &[Rule]` input once planning is authoritative.

### Low: Planned Eval Needs A Context Object

The planned path now threads `catalog`, `database`, `warnings`, `options`, `env`, and sometimes `delta` through most executor functions ([eval.rs:4459](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4459), [eval.rs:4573](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4573), [eval.rs:4615](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4615), [eval.rs:4786](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:4786)).

The sprawl is understandable during migration, especially for warning/provenance parity, but it now obscures the executor boundary. A `PlannedEvalCtx` would make scope changes, warning threading, and list/interner lifetime easier to reason about.

Suggested follow-up: introduce `PlannedEvalCtx` after this review, not during it. This overlaps the existing polish bead `anneal-txkp`.

### Low: `Rank` Recomputes Sort Keys

`eval_planned_rank` evaluates the key expression inside the sort comparator and then evaluates it again during output/rank assignment ([eval.rs:5233](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:5233)).

This is not a correctness problem, but it is an easy post-migration efficiency cut. Decorate rows with `(key, frame)` once, sort the decorated rows, then consume the cached keys in the rank loop.

### Low: Recursion Goldens Are Stable But Opaque

The recursion suite now checks planned output against byte count plus FNV digest of the tuple/provenance JSON ([eval.rs:7737](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7737), [eval.rs:7780](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:7780), [eval.rs:11697](/Users/morgan/code/anneal/crates/anneal-core/src/runtime/eval.rs:11697)).

That is compact and avoids bulky fixture churn, but a failure will say "digest drifted" without showing the semantic drift. The suite is now the only recursion oracle after interpreted deletion, so its failure mode matters.

Suggested follow-up: keep the digest gate, but improve assertion diagnostics by printing the first differing row against a stored expected payload or by writing temporary failure artifacts under `target/`.

### Low: Stale `v2` And `legacy` Language Still Needs A Naming Pass

The one-engine runtime still contains public-facing or near-public comments/docs with old framing, for example `anneal-cli` crate docs say the crate owns runtime commands "while the legacy crate" existed ([lib.rs:1](/Users/morgan/code/anneal/crates/anneal-cli/src/lib.rs:1)), `anneal-core` crate docs still call the substrate "anneal v2" ([lib.rs:1](/Users/morgan/code/anneal/crates/anneal-core/src/lib.rs:1)), and history comments refer to "v2 snapshot" entries ([history.rs:12](/Users/morgan/code/anneal/crates/anneal-core/src/history.rs:12)).

Some `legacy` terms are still correct, especially `anneal.toml` migration handling, so this should not be a blind rename. But after the interpreted/runtime migration, stale `v2` and "legacy crate" language weakens the coherence story and can mislead future reviewers about what still exists.

Suggested follow-up: do a targeted naming pass after the architecture review. Keep compatibility labels where they describe old data formats; remove language that describes already-retired architecture.

## Review Passes

1. Certificate and executor coherence (`anneal-kbgj.3`)
   - Verify `StageMigration`/`StageExecution` is the only migration decision source.
   - Check that executor dispatch never re-derives predicate kind, atom order, aggregate capability, or time-scope support.
   - Identify remaining AST-rule bridges and lazy-plan seams.

2. Provenance, recursion, and goldens (`anneal-kbgj.1`)
   - Inspect stored, aggregate, negation, time-block, and recursive provenance shapes.
   - Confirm recursion goldens cover the intended cases and remain maintainable.
   - Look for hidden explain/warning parity traps.

3. Cleanup cuts (`anneal-kbgj.2`)
   - Rank post-kftp simplifications by coherence value.
   - Link existing beads when they already cover the issue; create new beads only for uncovered findings.
   - Watch for stale "legacy", "v2", or "interpreted" language that no longer matches the one-engine architecture.

## Current Verdict

The migration appears directionally sound: the plan/certificate architecture exists, recursive stages are represented explicitly, and the interpreted executor has been removed rather than left as a parallel engine.

The main review pressure is now split. Most findings are cleanup of migration-shaped seams, but the function-call panic is a real user-facing correctness bug and should be handled before treating the planned-only surface as fully hardened. The two highest-value coherence follow-ups remain making the evaluator own a mandatory plan and removing the AST-rule bridge from the staged runner.
