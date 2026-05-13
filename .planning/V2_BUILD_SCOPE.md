# v2.0 Build Scope

This document translates `.design/2026-05-13-corpus-runtime.md` into an
implementation path for agents. The master spec owns product semantics. `bd`
owns live state. This file owns build order, stop gates, and safe parallelism.

The goal is not to build a nicer v1 CLI first. The goal is to build the
programmable corpus runtime in slices that each produce a working, measurable
agent affordance.

## Source Of Truth

- Master spec: `.design/2026-05-13-corpus-runtime.md`
- Engine decision: `.design/2026-05-13-engine-spike-results.md`
- Current state: `.planning/STATE.md`
- Work graph: `bd show anneal-rsx`

If these disagree, the master spec wins for semantics, `bd` wins for live
status, and this file should be patched.

## Build Rules

1. Close Phase 0 before user-facing v2 surfaces. Do not treat "Ascent is fast"
   as "Phase 0 is complete"; parity, frozen fixtures, dynamic-IR measurement,
   and unsafe audit are still gating work.
2. Keep one rule universe. The dynamic IR owns prelude rules, project
   `anneal.dl`, and inline `-e` rules together. Ascent is allowed for
   engine-derived primitives and hot fixed derivations only until a later
   measurement proves a stronger split preserves shadowing, provenance, and
   explanation semantics.
3. Keep the adapter-substrate boundary narrow. A source returns `FactBatch`
   from `Source::extract`; source-specific behavior should not leak through
   side channels into the runtime.
4. Treat `search`, `read`, `*content`, `*span`, schema/describe, and trails as
   core agent affordances. They are not polish tracks.
5. Surface late. CLI and MCP should expose stable runtime contracts after
   policy, trails, verbs, and self-description exist. Avoid creating interim
   tool shapes that agents will learn accidentally.
6. Every phase must have a fixture-backed definition of done. If a phase cannot
   be demonstrated against a pinned fixture, it is not closed.
7. Prefer compact, high-signal outputs over broad dumps. Agent context is a
   budgeted runtime resource.

## Phase 0: Closure And Falsification

Issue: `anneal-apa`

Purpose: finish the evidence needed before implementation momentum makes the
wrong architecture expensive.

Parallel tracks:

| Issue | Track | Output | Stop Gate |
|---|---|---|---|
| `anneal-apa.1` | Parity runner | `tools/parity-runner` runs PD-1..3 v1.1 against v1.1 | deterministic baseline output or documented current nondeterminism |
| `anneal-apa.2` | Frozen fixture | `.fixtures/sample-corpus/` plus runner defaults | no live `/path/to/large-corpus` dependency for gates |
| `anneal-apa.3` | Dynamic IR bench | parser/evaluator skeleton with warm prelude timing | <50ms warm on fixture or documented compiled-prelude fallback |
| `anneal-apa.4` | Unsafe audit | `.design/2026-05-XX-ascent-unsafe-audit.md` | every Ascent unsafe block and relevant transitive unsafe is classified |

Do not start Phase 1 until all four tracks are closed or explicitly deferred in
the master spec and `bd`.

Supporting work that may run during Phase 0:

- `anneal-10c`: SP-Q literal-query conformance layer, especially where parity
  output needs a stable query contract.
- `anneal-1i2`: executable `context` contract fixture, because the standard
  library must not become prose-only.
- `anneal-bmq`: I001 rule correction, if it can land as an isolated v1.x fix.

## Phase 1: Foundation

Issue: `anneal-xu2`

Purpose: create the crate boundaries and relation vocabulary without changing
user-facing behavior.

Recommended implementation slices:

1. Workspace skeleton: `anneal-core`, `anneal-md`, `anneal-cli`, `anneal-mcp`.
2. Stored relation structs with identity fields: `corpus`, `source`,
   `native_id`, `origin_uri`, `revision`, and `generation` where the spec
   requires them.
3. `Source`, `SourceContext`, `SourceInfo`, `FactBatch`, `FactBatchMode`, and
   atomic generation merge/retraction semantics.
4. `anneal-md` extraction that reproduces v1.x facts through the new adapter.
5. Discovery facts from `anneal.toml` and `anneal.dl` Phase A, limited to the
   markdown adapter.

Exit gate: parity runner says the markdown adapter can feed PD-1..3 without
semantic drift from v1.x.

Do not add host/code adapter abstractions yet. Preserve room for them by
keeping identity/origin fields real, but do not implement federation or async
source scheduling without a v2.1 adapter forcing the shape.

## Phase 2: Runtime

Issue: `anneal-jqh`

Purpose: make rules executable in one language universe.

Recommended implementation slices:

1. Parser and typed IR for the master-spec grammar.
2. Stratification and negation-cycle diagnostics.
3. Naive evaluator over stored relation tables.
4. Provenance hooks for derived facts.
5. NDJSON query output with explicit stability markers.

Exit gates:

- SP-NT1 names the negation cycle, not only one predicate.
- MVS-1..5b run through the real parser/IR rather than spike-only Rust code.
- Dynamic IR warm prelude timing remains inside the Phase 0 budget or the
  fallback decision is activated.

Do not optimize before correctness fixtures exist. Ascent integration belongs
behind fixed primitive derivations, not in the project-rule path.

## Phase 3: Engine Primitives

Issue: `anneal-f2b`

Purpose: move v1 graph, lifecycle, obligation, aggregation, and time-reference
behavior behind stored relations and primitive functions.

Exit gates:

- All §11 primitives have fixture coverage.
- Snapshot and git-ref `at()` paths are measured.
- Aggregation semantics are covered at realistic corpus scale.

The main risk here is identity drift. Every primitive should preserve
corpus/source/generation enough for later trails and multi-corpus federation to
explain where a result came from.

## Phase 4: Content And Retrieval

Issue: `anneal-9yl`

Purpose: give agents the shortest reliable path from "what should I inspect?"
to the right span of corpus text.

Recommended implementation slices:

1. `*content` and `*span` storage from `anneal-md`.
2. `read(handle, span_id, content)` with token budgets and stable errors.
3. `search(query, handle, span_id, score, reason, field, low_confidence)`.
4. Ranker calibration contract and deterministic fixture assertions.
5. `match(pattern, handle, span_id, captured)` only after read/search are
   stable.

Exit gate: the large-corpus/v17 conformance workflow reaches the audit document and
the blocking concern group through search/read on the pinned fixture.

Do not expose `read_full` broadly. It remains capability-gated and budgeted
from its first implementation.

## Phase 5: Self-Description

Issue: `anneal-1gy`

Purpose: let agents ask the runtime what relations, verbs, sources, and
capabilities exist instead of guessing from documentation.

This phase can run in parallel with Phase 4 after Phase 3 lands. Keep it
read-only: schema/describe/source primitives should describe the runtime, not
mutate it.

Exit gate: an agent can discover the relation shape needed for the v17 workflow
without reading the spec.

## Phase 6: Standard Library

Issue: `anneal-1xb`

Purpose: express convergence behavior as ordinary prelude rules and verbs.

Inputs that must be closed first:

- Phase 4 content/search/read.
- Phase 5 self-description.
- `anneal-1i2` context contract fixture.
- `anneal-bmq` I001 rule decision.

Exit gates:

- Starter verbs are saved templates, not engine commands.
- `context` is executable and fixture-backed, not just described.
- Prelude hash is recorded for outputs and trails.

## Phase 7: Project Extension

Issue: `anneal-7it`

Purpose: make project `anneal.dl` indistinguishable from prelude code under
Steele's criterion.

Exit gates:

- Phase A discovery facts influence source extraction before Phase C rule
  loading.
- A project `@verb` can shadow or extend a prelude verb exactly as specified.
- Adapter-qualified discovery facts reject ambiguous multi-adapter forms unless
  the single-adapter sugar rule applies.

This is where subtle ordering bugs will hide. Add fixtures with conflicting
adapter discovery facts before implementing convenience syntax.

## Phase 8: Trails And Provenance

Issue: `anneal-t10`

Purpose: capture enough session path to recover, audit, and later replay what
an agent did without leaking private content by default.

Exit gates:

- Every trail entry records actor, capability context, prelude hash,
  source_generations, surfaced_refs, consumed_refs, visibility, and retention.
- Redaction happens before persistence.
- `--explain` can expand a result without dumping unbounded provenance.

This phase depends on Phase 4 because consumed references must distinguish
shown spans from actually consumed text.

## Phase 9: Capability And Policy

Issue: `anneal-m08`

Purpose: make dangerous affordances explicit before MCP or host embedding
exposes them.

Exit gates:

- CLI starts permissive for local use; MCP starts conservative.
- `eval`, `read_full`, private trail read, and host-sensitive source operations
  are gated by `ActorContext` and `Policy`.
- Prompt-injection sensitive fields are marked or filtered at the surface.

This phase can run after Phase 3 and in parallel with Phases 4, 5, and 8 if it
does not edit their output contracts.

## Phase 10: Surfaces

Issue: `anneal-toe`

Purpose: expose the runtime through CLI, MCP, and library APIs after the
contracts are stable enough for agents to learn them.

Exit gates:

- MCP surface stays small: eval, run_verb, read, search, context, schema,
  describe, source, trail_query.
- MCP `eval` is default-denied unless config or host policy grants it.
- CLI output and MCP output share contract tests.
- `anneal init` scaffolds lattice-on defaults and a minimal standard library
  posture.

Do not add one MCP tool per verb. Verbs should route through `run_verb` with
self-description support.

## Phase 11: Migration And Acceptance

Issue: `anneal-px9`

Purpose: replace v1 behavior only after pinned workflows and parity prove the
new runtime is usable.

Exit gates:

- PD-1..12 parity passes or differences are accepted in the spec.
- Workflow-completion gates pass on pinned fixtures.
- v1 command compatibility is either preserved or deliberately retired with
  migration docs.
- `just check`, release verification, and `.design` self-check pass.

## Safe Parallelism

| Can Run Together | Conditions |
|---|---|
| Phase 0 parity, fixture, unsafe audit | Disjoint files; dynamic IR bench must not bake in unreviewed grammar choices |
| Phase 4 content and Phase 5 self-description | Phase 3 relation/primitives contracts stable |
| Phase 8 trails and Phase 9 policy | Output contract changes coordinated through Phase 10 |
| `anneal-10c`, `anneal-1i2`, selected rule fixes | Kept fixture-backed and not used to bypass Phase 0 |

Avoid parallel edits to:

- Stored relation identity fields.
- Grammar and IR structs.
- Search/read output contracts.
- Trail persistence format.
- MCP tool schemas.

## First Build Move

Claim one of the four Phase 0 closure tasks that block `anneal-apa`:

1. Parity runner PD-1..3 baseline.
2. Frozen large-corpus snapshot fixture.
3. Dynamic IR warm-prelude benchmark.
4. Ascent unsafe audit.

Only after those are closed should an agent claim `anneal-xu2`.

## Research Pressure

The research graph points in the same direction as the live workflow review:

- Agents perform better with compact, purpose-built actions and concise
  environment feedback.
- Search and summarized retrieval should be first-class when localization is
  the task, not an afterthought after graph verbs.
- Observable output shapes become compatibility contracts, so unstable
  internals should not leak through early CLI/MCP surfaces.
- Extensible systems need narrow stable interfaces more than broad early
  feature surfaces.

Build v2.0 so each slice gives a cold agent less to guess, fewer commands to
try, and better evidence about why an answer is valid.
