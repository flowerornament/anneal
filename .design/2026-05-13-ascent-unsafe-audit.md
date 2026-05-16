---
status: current
date: 2026-05-13
depends-on:
  - 2026-05-07-engine-spike-and-parity-protocol.md
  - 2026-05-13-engine-spike-results.md
description: >
  Unsafe-code audit for Ascent 0.8 as used by the v2.0 Phase 0 engine
  spike. Documents the Ascent-owned unsafe blocks, transitive dependency
  exposure, and the resulting risk posture for using Ascent behind fixed
  engine-derived primitives.
---

# Ascent Unsafe Audit

This closes the audit artifact requested by SP-R1 §3.6 and the Phase 0
closure plan. It does **not** prove Ascent is unsafe-free. It proves the
opposite: Ascent 0.8 contains non-FFI unsafe code in its relation-index
implementation, plus ordinary transitive unsafe from concurrency/hashmap
crates.

The only acceptable Phase 1 reading is therefore narrow:

> Ascent may be used behind the fixed engine-derived primitive boundary,
> pinned at 0.8, with dependency-unsafe risk explicitly accepted. This
> audit does not support making Ascent the general runtime `.dl` engine.

The research graph's Rust safety notes frame the standard: unsafe Rust is
acceptable only when the library-specific verification condition is
encapsulated behind a safe API. A grep count is insufficient; the relevant
question is whether the unsafe code's invariants are hidden from anneal and
whether anneal can keep the blast radius behind a small module boundary.

## Method

Commands run from `tools/spike-runner`:

```bash
cargo metadata --manifest-path tools/spike-runner/Cargo.toml --format-version 1
cargo tree --manifest-path tools/spike-runner/Cargo.toml
rg -n "unsafe\\s*\\{" ~/.local/share/cargo/registry/src/.../ascent-0.8.0/src
rg -n "\\bunsafe\\b|#\\s*\\[\\s*allow\\s*\\(\\s*unsafe_code" <transitive package roots>
```

Direct Ascent crates:

| Crate | Unsafe findings |
|---|---|
| `ascent 0.8.0` | 12 live unsafe expressions in `src/` |
| `ascent_base 0.8.0` | no unsafe expressions found |
| `ascent_macro 0.8.0` | no unsafe expressions found |

The broader transitive tree includes unsafe-heavy foundational crates:
`dashmap`, `hashbrown`, `rayon`, `crossbeam-*`, `parking_lot_core`,
`zerocopy`, `libc`, and platform crates. Those are not audited per block
here; they are treated as ordinary third-party Rust ecosystem risk. The
Phase 0 question was whether Ascent itself adds unsafe that defeats the
workspace's `unsafe_code = "deny"` policy. It does.

## Ascent-Owned Unsafe Blocks

| Location | Scope | Invariant Ascent Relies On | Risk | Audit Read |
|---|---|---|---|---|
| `internal.rs:137` | Mutates `MOVE_REL_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Single-process benchmark telemetry can tolerate unsynchronized mutation, or generated programs do not read it concurrently in a racy way | data race if read/write concurrently | Not memory-structural to relations, but not contained FFI |
| `internal.rs:193` | Mutates `MOVE_FULL_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Same telemetry invariant as above | data race if read/write concurrently | Same as above |
| `c_rel_no_index.rs:53` | Converts `RwLock<Vec<V>>::data_ptr()` to shared `&Vec<V>` for serial read | Relation is frozen; no writers mutate the lock while readers bypass lock guards | shared reference to data without lock guard | Sound only if freeze discipline is global and correct |
| `c_rel_no_index.rs:75` | Same `data_ptr()` bypass for Rayon parallel read | Frozen relation and no concurrent mutation across Rayon readers | data race or invalid reference if unfrozen writes occur | Higher blast radius because parallel iteration amplifies invariant failures |
| `c_rel_no_index.rs:111` | Mutates `MOVE_NO_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Same telemetry invariant as above | data race if read/write concurrently | Telemetry-only but non-FFI unsafe |
| `c_rel_index.rs:94` | Uses DashMap `_yield_write_shard` inside an alternate insert helper | Caller has selected the correct shard from the key hash and holds exclusive write access for that shard | corrupts DashMap internals if shard/hash invariant is wrong | Helper is marked dead code, but still compiled unsafe in the crate |
| `c_rel_index.rs:209` | Mutates `MOVE_REL_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Same telemetry invariant as above | data race if read/write concurrently | Telemetry-only but non-FFI unsafe |
| `c_rel_index.rs:255` | Reads DashMap shard through `data_ptr()` for parallel all-iteration | Relation has been converted to read-only/frozen view; no mutation during iteration | lock-bypass aliasing/data race | Core relation-read invariant; relevant if anneal uses concurrent relations |
| `c_rel_index.rs:282` | Same shard `data_ptr()` bypass for indexed all-iteration | Same frozen/read-only invariant | lock-bypass aliasing/data race | Core relation-read invariant; relevant if anneal uses concurrent relations |
| `c_rel_full_index.rs:94` | Uses DashMap `_yield_write_shard` in `insert_if_not_present2` | Correct shard selected from key hash and exclusive write access held | corrupts DashMap internals if invariant is wrong | Active helper for concurrent full index writes |
| `c_rel_full_index.rs:269` | Mutates `MOVE_FULL_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Same telemetry invariant as above | data race if read/write concurrently | Telemetry-only but non-FFI unsafe |
| `c_lat_index.rs:180` | Mutates `MOVE_REL_INDEX_CONTENTS_TOTAL_TIME` static timing counter | Same telemetry invariant as above | data race if read/write concurrently | Telemetry-only but non-FFI unsafe |

## Risk Assessment

**Strict SP-R1 result:** fail as written. Ascent does not satisfy "no
unsafe", and the unsafe is not merely contained behind FFI.

**Practical Phase 1 result:** acceptable only as dependency risk behind a
narrow boundary. The unsafe blocks sit inside Ascent's relation-index and
telemetry internals. Anneal should not expose Ascent relation types,
generated modules, or execution semantics as public API. The dynamic IR
must own prelude, project, and inline rules; Ascent should remain an
implementation detail for fixed primitive derivations.

**Containment requirements if Ascent remains selected for primitives:**

1. Keep `unsafe_code = "deny"` in anneal-owned crates and `spike-runner`.
2. Pin `ascent = 0.8` until a deliberate upgrade audit is performed.
3. Put all Ascent use behind an `anneal-core` primitive-engine module.
4. Do not pass user-authored `anneal.dl` directly to Ascent-generated code.
5. Add stress tests for concurrent/frozen relation reads before enabling
   Ascent's concurrent relation paths in production.
6. Treat any Ascent panic or unexplained nondeterminism as a blocker, not a
   recoverable runtime diagnostic.

## Recommendation

Accept Ascent only for fixed engine-derived primitives in Phase 1, and
record the unsafe sub-criterion as "audited and explicitly accepted as
bounded dependency risk", not "cleared".

If Phase 1 expands Ascent beyond that boundary, this audit must be
reopened. A general runtime engine must either avoid these unsafe paths,
wrap them behind a locally tested containment layer, or replace Ascent
with the dynamic IR/custom evaluator path.
