---
status: current
date: 2026-05-07
depends-on:
  - 2026-05-03-language-redesign.md
description: >
  Implementation spec for v2.0 Phase 0 — engine spike and parity protocol.
  Defines the empirical gates that must pass before committing to Phase 1
  implementation, with concrete pass/fail criteria, a reproducible test
  harness against real corpora (anneal, large-corpus, host-corpus), and a regression
  budget for the dual-CLI deprecation cycle.
---

# v2.0 Phase 0: Engine Spike and Parity Protocol

A concrete, executable plan to validate the language-first redesign before
committing to Phase 1 implementation. The output is real data: a numeric
report against three real corpora that either greenlights Phase 1 or
identifies specific blockers.

This spec exists because §33 of the language-redesign reads as a vision
document. The engine choice and parity-with-v1.1 question are load-bearing
decisions that should be answered with measurements, not assumptions.

---

## Part I: Engine Spike [SP-S]

### §1 Goal

Pick a Rust Datalog engine — `ascent`, `crepe`, or "we write our own" — by
implementing a **minimum viable subset of the language redesign** against
each candidate and measuring against fixed criteria. The output is a
go/no-go verdict per engine plus an annotated decision matrix.

Two-week box. If the spike runs over, that itself is data: the language
redesign is more expensive than estimated.

### §2 Minimum viable subset [SP-D1]

**Definition SP-D1 (MVS).** The smallest set of language features that
exercises every load-bearing engine capability:

| # | Capability | Why it matters |
|---|---|---|
| MVS-1 | Stored relations from in-memory facts | All queries depend on `*handle`, `*edge`, `*meta` |
| MVS-2 | Multi-clause derived predicates (rule union) | Discovery of corpus-wide vocabulary |
| MVS-3 | Recursion to fixed point (e.g., `upstream/2`) | Required by `impact`, supersedes chains |
| MVS-4 | Stratified negation with cycle detection | `entropy` rules use `not discharged(h)` |
| MVS-5 | Aggregation with grouping (`Count`, `Sum`) | `total_potential` per area |
| MVS-6 | Time-travel block evaluation against snapshots | `recently_advanced` per §16 |
| MVS-7 | NDJSON streaming output | §13 cardinality and §21 I/O contract |
| MVS-8 | Provenance / `--explain` derivation chains | LR-D9 contract; `anneal explain` survival |
| MVS-9 | `anneal.dl` external file loading + shadowing warning | LR-D2 load order |

A spike implementation passes when **all nine capabilities work end to
end** against a fixture corpus drawn from large-corpus. Workarounds are
acceptable but must be documented with an estimate of the work to remove
them.

### §3 Pass / fail criteria [SP-R1]

**Rule SP-R1 (Engine selection).** An engine is selected for Phase 1
implementation when it satisfies all of:

1. **MVS-1 through MVS-9** all working with no fundamental blockers.
2. **Stratification** is enforced at load with a usable error message
   (the spike attempts the §10 cyclic-negation example and verifies
   the engine names the cycle).
3. **Performance**: full evaluation of the large-corpus fixture
   (424 files, 13k handles, 8.5k edges) under 2 seconds cold,
   under 200ms warm.
4. **Time-travel performance**: snapshot-based `at()` block under 500ms,
   git-ref `at()` block within 5x the snapshot cost (one full reparse
   acceptable).
5. **Memory ceiling**: full large-corpus load under 200MB resident.
6. **No unsafe**: candidate engine compiles with workspace
   `unsafe_code = "deny"` policy, or its unsafe is contained behind a
   clearly-bounded FFI / arena interface.

Failing 1–2 is a hard stop. Failing 3–5 is a soft stop: document the gap,
estimate cost to fix, decide whether to absorb or switch engines.

### §4 Spike deliverables [SP-D2]

**Definition SP-D2 (Spike output).** When the spike completes, the
following artifacts exist:

- `tools/spike-runner/` — Cargo workspace member containing one binary
  per candidate engine (`ascent-spike`, `crepe-spike`, `custom-spike`)
  that loads the large-corpus fixture and runs the MVS test suite.
- `.design/2026-05-XX-engine-spike-results.md` — written report with the
  decision matrix, measured numbers, blocker list, and recommendation.
- `.fixtures/sample-corpus/` — frozen subset of large-corpus used for
  reproducible benchmarks (~50 files covering the trickiest cases:
  recursive supersedes chains, cross-area edges, time-travel-relevant
  history).
- A working prototype of the *seven verbs* (sketch quality — they need
  not be production-ready, only demonstrate that the chosen engine can
  back them).

### §5 Specific test queries [SP-Q]

The spike must successfully run all of the following queries against the
large-corpus fixture. Each is written in the language redesign syntax (§6) and
exercises one or more MVS capabilities.

**SP-Q1 — Stored relation projection (MVS-1):**
```
? *handle{id, kind, status}, kind = "label", status = "open".
```
Expected: every open OQ/RQ/etc label in large-corpus.

**SP-Q2 — Multi-clause rule union (MVS-2):**
```
release_blocker(h, "broken_ref")    := diagnostic("E001", _, h, _, _, _).
release_blocker(h, "undischarged")  := diagnostic("E002", _, h, _, _, _).
? release_blocker(h, why).
```
Expected: union of E001 and E002 sources from large-corpus.

**SP-Q3 — Transitive recursion (MVS-3):**
```
upstream(h, anc) := *edge{from: h, to: anc, kind: "depends_on"}.
upstream(h, anc) :=
  *edge{from: h, to: mid, kind: "depends_on"},
  upstream(mid, anc).
? upstream("formal-model/v17.md", anc).
```
Expected: transitive closure of v17's depends_on edges; matches v1.1
`anneal map --around=formal-model/v17.md --upstream` modulo format.

**SP-Q4 — Stratified negation (MVS-4):**
```
unfinished(h) := *handle{id: h, kind: "label", namespace: "OQ"},
                 not terminal(h).
? unfinished(h).
```
Expected: every OQ that hasn't terminated.

**SP-Q5 — Aggregation with grouping (MVS-5):**
```
area_active_count(area, n) :=
  n = Count{ h : *handle{id: h, kind: "file", area},
                 active(h) }.
? area_active_count(area, n).
```
Expected: per-area active file counts; matches v1.1 `anneal areas`
totals.

**SP-Q6 — Time travel (MVS-6):**
```
status_changed(h, prev, curr) :=
  *handle{id: h, status: curr},
  at("snapshot:last") {
    *handle{id: h, status: prev}
  },
  prev != curr.
? status_changed(h, prev, curr).
```
Expected: handles whose status changed since the last snapshot.

**SP-Q7 — Streaming NDJSON output (MVS-7):**
Run any of the above with `--limit=1000` and verify stdout is one
complete JSON object per line, deduplicated, no buffering before first
emission.

**SP-Q8 — Provenance (MVS-8):**
```
? upstream("formal-model/v17.md", anc) --explain.
```
Each output record carries `_derivation` listing the rule chain and
supporting `*edge` facts.

**SP-Q9 — Loading `anneal.dl` (MVS-9):**
Add a file `fixture-anneal.dl` defining `release_blocker/2`. Verify the
engine loads it after the prelude, that `?  release_blocker(h, why).`
returns results, and that defining a `terminal/1` clause in the file
produces the shadow warning to stderr.

### §6 Cyclic-negation negative test [SP-NT1]

**Negative test SP-NT1.** Load the following ruleset:
```
blocked(h)    := active(h), not advancing(h).
advancing(h)  := active(h), not blocked(h).
```
The engine must reject load and emit an error naming both `blocked/1`
and `advancing/1` and the negation cycle. A silent success is a failure
of the spike.

---

## Part II: Parity Protocol [SP-P]

### §7 Goal

Verify that v2.0 produces functionally equivalent output to v1.1 against
real corpora before the dual-CLI deprecation window opens. The output is
a numeric regression report on three corpora; ship is blocked until the
regression budget is satisfied.

### §8 Reference corpora [SP-D3]

**Definition SP-D3 (Reference corpora).** The three corpora used for
parity evaluation, in order of weight:

| Corpus | Path | Rough size | Why |
|---|---|---|---|
| anneal self | `/path/to/anneal/.design/` | ~12 files, ~500 handles | self-check; should always pass |
| large-corpus | `/path/to/large-corpus/.design/` | ~424 files, ~13k handles, 15 areas, custom pipeline | breadth and graph density |
| host-corpus | `/path/to/host-corpus/.design/` | ~90 files | secondary; gates regressions caught only on host-corpus |

Each corpus is checked into a frozen snapshot directory (`.fixtures/`)
during Phase 0 so the parity numbers are reproducible across the
migration window even as the live corpora drift.

### §9 Parity dimensions [SP-D4]

**Definition SP-D4 (Parity dimension).** A pair (input → expected
output) where v1.1 and v2.0 are compared. Twelve dimensions covering
every shipped command:

| # | v1.1 input | v2.0 input | Compared output |
|---|---|---|---|
| PD-1 | `anneal status --json` | `anneal` | summary counts: files, handles, edges, active, terminal, diagnostics, pipeline counts |
| PD-2 | `anneal check --scope=active --json` | `anneal broken` (`-e diagnostic(...)` for non-error) | diagnostic IDs, severities, source locations as multisets |
| PD-3 | `anneal get H --refs --json` (for 50 sample handles) | `anneal H --refs` | identity, status, file, refs |
| PD-4 | `anneal find TEXT --limit=50 --json` (10 queries) | `anneal find TEXT --limit=50` | matched-handle id sets |
| PD-5 | `anneal map --around=H --upstream --json` (10 hubs) | `-e upstream("H", anc).` | anc set |
| PD-6 | `anneal impact H --json` (10 hubs) | `-e impact("H", x, depth).` | x set, depth distribution |
| PD-7 | `anneal obligations --json` | `-e obligation(h), discharged(h).` and `not discharged` | partitioned counts |
| PD-8 | `anneal query handles --kind=label --namespace=OQ --json` | `-e *handle{kind: "label", namespace: "OQ", ...}.` | id sets |
| PD-9 | `anneal areas --json` | `-e area_health(area, grade, ...).` (prelude rule) | per-area grade and counts |
| PD-10 | `anneal orient --budget=50000 --json` | `anneal work --budget=50k` | tier-classified file lists; allow 5% reorder |
| PD-11 | `anneal garden --json` | `-e maintenance_task(t, category, blast).` | category + handle multisets |
| PD-12 | `anneal diff --days=7 --json` | `anneal trend --at=--7days` | counts per area, direction signal |

### §10 Regression budget [SP-R2]

**Rule SP-R2 (Regression budget).** v2.0 ships dual-CLI when:

1. PD-1, PD-2, PD-3, PD-7, PD-8: **exact match** as multisets per corpus.
   These are deterministic, structural questions; any divergence is a
   regression.
2. PD-4, PD-5, PD-6, PD-12: **exact match** on identity sets, **±5%
   tolerance** on counts. Snapshot-based `diff`/`trend` may drift if the
   underlying snapshot store is reimplemented.
3. PD-9, PD-10, PD-11: **±5% tolerance** on rankings and counts. These
   involve heuristics (grade, budget greedy fill, garden ranking) that
   may rationally re-order but should not produce structurally different
   outputs.
4. **No new diagnostics** in v2.0 that v1.1 did not produce, unless
   explicitly added in the migration. New diagnostic IDs require a
   matching language-redesign §16-§18 reference.
5. **No silent regressions** in *human* output: every output the v2.0
   verb prints must be reachable from the corresponding v1.1 command.

Any breach > tolerance gates the dual-CLI release. Breaches under
tolerance are logged to the parity report but do not block.

### §11 Parity harness [SP-D5]

**Definition SP-D5 (Parity harness).** A command-line tool
(`tools/parity-runner`) that executes both v1.1 and v2.0 against each
fixture, captures stdout (NDJSON or `--json`) plus stderr, normalizes
the output to a canonical form, and emits a report.

```
$ tools/parity-runner --corpus=large-corpus
running PD-1  status        ... ok    (exact match)
running PD-2  check         ... ok    (exact match: 0/0/4/33)
running PD-3  get x50       ... ok    (exact match)
running PD-4  find x10      ... ok    (49/50, 1 ordering diff)
running PD-5  map x10       ... ok    (exact match)
running PD-6  impact x10    ... ok    (exact match)
running PD-7  obligations   ... ok    (exact match)
running PD-8  query labels  ... ok    (exact match)
running PD-9  areas         ... ok    (within ±2%)
running PD-10 work          ... fail  (12% reorder in pinned tier)
running PD-11 garden        ... ok    (within ±3%)
running PD-12 trend         ... ok    (within ±1%)

result: 1 fail (PD-10), 11 pass
report: .fixtures/parity-2026-05-07-large-corpus.json
```

Each PD entry produces a structured JSON record with: input command,
v1.1 output hash, v2.0 output hash, set-difference summary, count-delta
summary, and verdict against the regression budget.

### §12 Continuous parity [SP-R3]

**Rule SP-R3 (Continuous parity).** During the dual-CLI deprecation
release, the parity harness runs in CI on every commit. A regression
that crosses the §10 budget fails the build. New diagnostics or new
verbs that didn't exist in v1.1 are exempt; *changes to existing v1.1
behavior reachability* are not.

When the old CLI is removed in the next minor release, the parity
harness is retired but the fixture corpora are kept as integration
tests against v2.0 alone.

---

## Part III: Decision rubric [SP-DR]

### §13 Phase 1 entry conditions [SP-DR1]

**Decision rubric SP-DR1 (Proceed to Phase 1).** All of:

- One engine satisfies SP-R1 (or "custom" is selected with a separate
  cost estimate).
- All MVS queries (SP-Q1 through SP-Q9) pass on the large-corpus fixture.
- The cyclic-negation negative test (SP-NT1) is rejected at load.
- The spike report identifies any open blockers and either resolves
  them or scopes them as Phase 1 follow-ups.

### §14 Course-correct triggers [SP-DR2]

**Decision rubric SP-DR2 (Course-correct).** If the spike reveals any
of the following, the language-redesign spec returns for revision:

- No engine satisfies SP-R1 within the two-week box (escalate engine
  selection question).
- Time-travel against git refs is more than 10x slower than snapshots
  (consider scoping `at()` to snapshot-only for v2.0).
- Aggregation with grouping is awkward enough to require ~50 lines of
  Rust per verb (evaluate moving more verbs to non-Datalog
  implementations and shrinking the LR-P1 "one language" claim).
- Provenance / `--explain` requires substantial bespoke instrumentation
  per engine (consider deferring `--explain` to a later release).

### §15 Abandon trigger [SP-DR3]

**Decision rubric SP-DR3 (Abandon).** Stop the v2.0 redesign and
return to incremental v1.x improvements if:

- Spike box doubles to four weeks without an engine candidate.
- Parity harness reveals more than 25% regression budget breach across
  PD-1 through PD-8 *that cannot be rationally explained or fixed*.

Abandonment is a real option; the v1.x CLI is shippable today. The
language redesign is only worth its cost if the spike confirms the
engine-and-prelude story is achievable in the spec's estimated scope.

---

## Part IV: Schedule

| Week | Track | Deliverable |
|---|---|---|
| 1 | Engine spike | `ascent-spike` and `crepe-spike` running MVS-1..5; initial perf numbers |
| 2 | Engine spike | MVS-6..9; cyclic-negation test; engine-spike-results report |
| 2 | Parity harness | `tools/parity-runner` skeleton; PD-1..3 against v1.1 |
| 3 | Phase 1 | Begin engine work using selected candidate (or revise spec) |
| 3 | Parity harness | PD-4..12; baseline parity numbers from v1.1 alone (sanity check) |

The parity harness can begin in parallel with the spike — its first
job is to baseline v1.1 against itself, which exposes any
nondeterminism in the existing CLI before v2.0 has to match it.

---

## Part V: What this spec is *not*

This spec does not redesign the language. It assumes the
language-redesign spec is correct and asks: *can we afford to build
it, and how do we know we built it right?*

Out of scope:
- Rewriting `anneal.dl` semantics
- Changing the seven verbs
- Reopening the convergence vocabulary
- Multi-corpus federation (LR-OQ2)
- Agent ergonomics (search, MCP, context annotations — separate track)

The spike or parity work *may surface* findings that require revisiting
the language redesign — that's what SP-DR2 is for. But this spec's
deliverable is a report, not a redesign.

---

## Labels

### SP-S (Spike)
- SP-S1: Engine candidate evaluation per §1
- SP-S2: MVS coverage per §2

### SP-D (Definitions)
- SP-D1: Minimum viable subset (§2)
- SP-D2: Spike output (§4)
- SP-D3: Reference corpora (§8)
- SP-D4: Parity dimension (§9)
- SP-D5: Parity harness (§11)

### SP-R (Rules)
- SP-R1: Engine selection (§3)
- SP-R2: Regression budget (§10)
- SP-R3: Continuous parity (§12)

### SP-Q (Queries)
- SP-Q1..9 per §5

### SP-NT (Negative tests)
- SP-NT1: Cyclic negation rejection (§6)

### SP-DR (Decision rubric)
- SP-DR1: Proceed to Phase 1 (§13)
- SP-DR2: Course-correct (§14)
- SP-DR3: Abandon (§15)

---

## References

### Internal

- `2026-05-03-language-redesign.md` — the design this spec validates
- `anneal-spec.md` — Parts I-III preserved per LR-§34, used as the
  parity reference

### External

- `ascent` https://github.com/s-arash/ascent — primary engine candidate
- `crepe` https://github.com/ekzhang/crepe — secondary candidate
- Cozo aggregation grammar — reference for `take_until`-style
  aggregation if LR-OQ1 surfaces during the spike
