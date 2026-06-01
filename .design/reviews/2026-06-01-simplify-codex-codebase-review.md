# simplify-codex review — 2026-06-01

Scope: whole-codebase cleanup pass after the v0.15 spec-code coherence work, with explicit Rust review context and three lanes: code reuse, code quality, and efficiency. The review focused on current hot paths and recently touched seams: `target_probe`, markdown fact emission, context/search ranking, NDJSON output, the CR-R12 cold-start harness, and runtime loading.

## Executive summary

The largest immediate performance issue was found and fixed during this pass: `RuntimeSession::load` was running `git log -1` once per file to populate `git_mtime`, which made even trivial commands pay hundreds of subprocesses on larger corpora. Commit `37c9814` batches git mtime discovery and drops murail status/schema/bare-handle paths from about 9.4s to about 3.8s, with system time falling from about 3.5s to about 0.22s.

The remaining cleanup opportunities cluster around four seams:

1. Keep ranking policy in one place. `context` currently has CLI-local reranking constants that can drift from `anneal_core::ranking`.
2. Make standard metadata and path semantics typed enough that W006/code-ref behavior cannot drift by raw string typo or subtly different relative-path checks.
3. Stop rereading markdown files through the legacy adapter bridge; parse, meta, spans, and revision hashing should share one per-file payload.
4. Continue performance work below the 3.8s floor. The remaining cost is CPU/fixpoint/database materialization, not git subprocess overhead.

## Findings

### P1 — Markdown extraction rereads files several times on every runtime load

Evidence:

- `crates/anneal-legacy/src/parse.rs` reads files during graph extraction.
- `crates/anneal-legacy/src/v2_adapter.rs` then rereads for frontmatter meta, content spans, and revision hashing.

Why it matters:

This is shipped hot-path work for every command, including commands that do not need bodies or frontmatter beyond facts already parsed. On murail-sized corpora, the git subprocess storm was the first ceiling; this repeated-read path is the next likely load-side cleanup before attacking fixpoint cost.

Recommended cleanup:

Carry normalized frontmatter, body text, and revision material through the parse result or a per-file payload cache. Emit `*meta`, `*content`, `*span`, and revision facts from that single representation.

### P2 — Context ranking policy is split between CLI and core

Evidence:

- `crates/anneal-cli/src/context.rs` owns `context_reason_bonus` and `context_rank_bonus`.
- `crates/anneal-core/src/ranking.rs` owns `Ranker`, field weights, specificity, and tie-break behavior.

Why it matters:

The v4cd retrieval fix made context much better, but it also created a second ranking vocabulary. Future tuning can improve `search` while accidentally regressing `context`, or vice versa.

Recommended cleanup:

Move context-specific ordering helpers next to `Ranker`, or expose a core context-ranking helper that takes the same hit records and returns the context order. Keep the CLI as a renderer/orchestrator, not the owner of rank semantics.

### P2 — Standard code-reference metadata is still raw-string distributed

Evidence:

- Emission uses standard keys such as `external_class`, `target_path`, `target_exists`, `target_in_history`, `target_probe_base`, and `target_resolved_path`.
- The same keys appear in adapter emission, `checks.dl`, handle display, tests, and docs.

Why it matters:

These keys are now part of anneal's public substrate vocabulary. A typo or partial rename can silently drop diagnostics or display sections.

Recommended cleanup:

Add exported constants or a small helper module for standard metadata keys, plus a `CodeTargetMeta` constructor/accessor shape. Use it anywhere Rust emits or reads the standard code-target metadata. The Datalog prelude will still contain string literals, but Rust-side drift should shrink.

### P2 — Relative path policy is hand-rolled in multiple trust-boundary paths

Evidence:

- `crates/anneal-core/src/target_probe.rs` normalizes relative targets.
- `crates/anneal-md/src/lib.rs` validates configured relative paths.
- `crates/anneal-legacy/src/resolve.rs` has separate inside-root normalization.

Why it matters:

W006's correctness depends on exact distinctions: absolute paths, `..`, unresolved paths, and paths inside the probed base must not blur into each other. Multiple validators make CR-R12 behavior harder to audit.

Recommended cleanup:

Introduce a core relative-target type or helper with explicit policy knobs, for example whether empty and `.` are allowed. Reuse it from target probing and markdown source config.

### P2 — Target history metadata collapses "unavailable" and "checked absent"

Evidence:

- `target_probe` internally distinguishes `Some(false)` from `None` for history lookup.
- Emitted metadata currently uses boolean-like `target_in_history=false` for both not-in-history and history-unavailable cases.

Why it matters:

The diagnostic behavior is conservative, which is good: absent + no history is `unknown`, not drift. But the audit metadata is less expressive than the evidence model, so a cold agent cannot tell whether the probe checked history and found no path or could not check history.

Recommended cleanup:

Add a tri-state audit key such as `target_history_status = "present" | "absent" | "unavailable"`, or replace `target_in_history` with that shape before the metadata contract hardens further.

### P2 — `potential_weight` is outside the config schema abstraction

Evidence:

- Runtime config schema lives in `crates/anneal-core/src/config_schema.rs`.
- `potential_weight` lowering is special-cased in `crates/anneal-core/src/project.rs` and consumed by a private Datalog key.

Why it matters:

This is exactly the kind of tuning surface that should stay self-described. If validation, lowering, docs, and prelude fixtures do not share one declaration, teaching can drift from runtime behavior.

Recommended cleanup:

Model map-like config declarations in the schema layer, or move the `potential_weight` key/mode declaration into `config_schema` so the schema is the source of truth.

### P2 — Ordered edge emission does quadratic removal work

Evidence:

- `crates/anneal-legacy/src/v2_adapter.rs` builds a vector of edges, then repeatedly searches/removes while preserving desired ordering.

Why it matters:

The cost grows with edge count, and code refs add more `Cites` edges. This was not the primary murail wall-clock issue, but it is the kind of avoidable O(n^2) path that will show up as corpora grow.

Recommended cleanup:

Index edges by `(source, target, kind)` with small buckets, or use a consumed bitset and emit leftovers in a final pass instead of removing from the middle of a vector.

### P3 — Row field access helpers are duplicated

Evidence:

- `crates/anneal-cli/src/context.rs` has local string/number helpers.
- `crates/anneal-cli/src/app.rs` has similar required field helpers.
- Runtime eval tests have another small row access shape.

Why it matters:

The duplicated helpers differ in missing/null/number handling and error text. It is low risk today but makes output-contract changes noisier.

Recommended cleanup:

Create a tiny row-reader helper for required/optional strings and numbers, with caller-provided labels for error context.

### P3 — Git-history test setup is duplicated

Evidence:

- `crates/anneal-core/src/target_probe.rs` and `crates/anneal-cli/tests/cold_start_honesty.rs` both initialize temporary git repositories and configure identity.

Why it matters:

CR-R12 and W006 are now heavily git-history-aware. More tests will likely need the same setup, and duplicate shell helpers make these tests harder to extend.

Recommended cleanup:

Add `crates/anneal-cli/tests/support` for integration helpers. If cross-crate usage grows, consider a dev-only test-support crate.

## Already addressed in this pass

### Batched git mtimes

Commit `37c9814` changed `git_mtimes_for_files` from one `git log -1` per file to one batched `git log --relative --format=%cI --name-only -- <files...>` parse. The targeted `eval_git_mtime_uses_git_history` test passes, which is the important correctness check because `changed_within` and freshness depend on the same per-file mtime semantics.

Measured on murail:

- status/schema/bare-handle query: about 9.4s before, about 3.8s after
- system time: about 3.5s before, about 0.22s after

Remaining timing instrumentation before removal showed the new floor is mostly runtime database/fixpoint work, not subprocess overhead.

## Suggested follow-up order

1. File or continue the perf issue for the remaining murail floor below 3.8s. Start with database materialization and fixpoint profiling, not W006 or git.
2. Refactor markdown extraction to avoid rereading each file through the legacy bridge.
3. Consolidate ranking policy in `anneal_core::ranking` so search and context cannot drift.
4. Type standard code-target metadata keys and relative path policy.
5. Add tri-state target history audit metadata.
6. Fold `potential_weight` into the config schema abstraction.
7. Clean the smaller helper duplication once the high-risk seams settle.

