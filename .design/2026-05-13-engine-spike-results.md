---
status: current
date: 2026-05-13
depends-on:
  - 2026-05-07-engine-spike-and-parity-protocol.md
  - 2026-05-13-corpus-runtime.md
description: >
  Engine-spike findings for the v2.0 language redesign. Honest accounting
  of what was validated versus what the SP-D2 deliverable list requires,
  with the architectural revision that emerged: ascent is the right
  substrate for fixed engine-derived primitives, not for the entire
  rule layer. A dynamic IR (parser + evaluator) should own prelude +
  project + inline rules together.
---

# Engine-Spike Results

Phase 0 ran across six sessions (2026-05-07 through 2026-05-13). The
spike was driven by `.design/2026-05-07-engine-spike-and-parity-protocol.md`
(SP-spec). This document is its SP-D2 deliverable.

The headline is more measured than earlier session notes suggested:
**ascent is a viable substrate for static/fixed-rule execution at the
spec's target corpus scale. SP-D2 artifact closure is incomplete, and
SP-R1 was only partially measured. The architectural correction from
peer review is to put ascent under the *primitives*, not under the
entire prelude.**

---

## Part I: Engine-Viability Verdict [SR-V]

### §1 What was validated

A typed ascent program at `tools/spike-runner/` exercises eight of the
nine MVS capabilities and the SP-NT1 negative test. Coverage:

| MVS | Result | Notes |
|---|---|---|
| MVS-1 stored relations | ✓ | typed tuples; swapped fields fail at compile time |
| MVS-2 multi-clause union | ✓ | three `release_blocker` clauses fire |
| MVS-3 transitive recursion | ✓ | three-deep supersedes chain on fixture; transitive `upstream` works at large-corpus scale (13 facts) |
| MVS-4 stratified negation | ✓ | `!terminal(q)` excludes the terminal label |
| MVS-5 aggregation grouping | ✓ | `count() in body(_)` works; grouping by free vars per spec §9 |
| MVS-6 time travel | ✓ on fixture | snapshot-as-relation; not exercised against real corpus |
| MVS-7 streaming NDJSON | partial | `capability::emit` is per-row to BufWriter; not stressed with `--limit=1000` on a real query stream |
| MVS-8 provenance | ✓ for hand-instrumented rules | companion-relation pattern works; general `_derivation` is unproven |
| MVS-9 runtime `.dl` loading | not validated | ascent is macro-only; documented as out-of-engine work |
| SP-NT1 cyclic-negation rejection | ✓ with caveat | rejects at compile time, but names only one side of the cycle |

### §2 Performance — one sub-criterion of SP-R1 measured

`tools/spike-runner/src/bin/corpus_spike.rs` loads the live large-corpus
corpus via shell-out to anneal v1.1 and times each phase. Representative
run (release build, 13,904 handles, 6,416 edges):

| Phase | ms |
|---|---|
| Load (anneal subprocess + JSON parse) | 500 |
| Fill program (push tuples) | 0.15 |
| Fixpoint (ascent eval) | 4.5 |
| All MVS row construction | <1.0 |
| Total wall clock | 505 |

**What SP-R1 §3 requires, scored honestly:**

| Sub-criterion | Result |
|---|---|
| 1. MVS-1..9 all working | partial — MVS-7 untested at scale, MVS-9 not validated |
| 2. Stratification with usable error | partial — rejected but cycle not fully named |
| 3. <2s cold, <200ms warm full evaluation at large-corpus | cold live-runner path cleared (<510ms total, <5ms fixpoint); warm unmeasured; not run against frozen fixture |
| 4. <500ms snapshot `at()`, <5× git-ref | **unmeasured** |
| 5. <200MB memory | **unmeasured** |
| 6. `unsafe_code = deny` or contained FFI | **not audited** — ascent 0.8 has unsafe blocks at `ascent/src/c_rel_index.rs:94,209,255,282` and siblings |

So only the cold/fixpoint slice of one sub-criterion is decisively
measured. No complete SP-R1 criterion is closed as written because
warm evaluation, frozen fixtures, runtime loading, snapshot/git-ref
travel, memory, and dependency-unsafe audit remain open. The earlier
"SP-R1 cleared by 530×" framing in session notes was too strong.

---

## Part II: SP-D2 Deliverable Gap [SR-G]

SP-D2 enumerates four artifacts when the spike completes. Honest status:

| Required | Status |
|---|---|
| `tools/spike-runner/` workspace member with `ascent-spike`, `crepe-spike`, `custom-spike` binaries | partial — `ascent_spike` and `corpus_spike` exist; **`crepe_spike` and `custom_spike` were never attempted** |
| `.design/2026-05-XX-engine-spike-results.md` | **this document** (created 2026-05-13) |
| `.fixtures/sample-corpus/` frozen reproducible benchmark | **missing** — current corpus_spike shells out to the live `/path/to/large-corpus/.design/` directory; counts drift over time (saw 13,890 vs 13,904 across runs) |
| Working prototype of the seven verbs | **missing** — only MVS capability tests exist; no `anneal`, `H`, `find`, `work`, `blocked`, `trend`, `broken` surfaces |

Also missing from SP-D5 (parity protocol): the `tools/parity-runner`
harness and any baseline of v1.1 against itself across PD-1..12.

**Honest assessment:** the engine-viability question was answered.
The artifact closure for SP-D2 and the parity protocol for SP-D5 are
not done.

---

## Part III: SP-Q Drift [SR-Q]

The spike's MVS-shaped tests diverge from the literal SP-Q queries
specified in `2026-05-07-engine-spike-and-parity-protocol.md` §5.
Identified by peer review (codex, 2026-05-13). Concrete divergences:

| SP-Q | Spec query | What the spike actually runs |
|---|---|---|
| SP-Q1 | `? *handle{id, kind, status}, kind = "label", status = "open".` (open labels only) | emits *all* handles projected to a 5-column row |
| SP-Q2 | `release_blocker(h, why)` over E001/E002 | matches spec ✓ |
| SP-Q3 | `upstream("formal-model/v17.md", anc)` | runs `supersedes_chain` instead; an `upstream` relation exists but is verified via MVS-8's chain reconstruction, not via this direct query |
| SP-Q4 | `unfinished(h) := *handle{kind: "label", namespace: "OQ"}, not terminal(h)` | matches spec ✓ |
| SP-Q5 | `area_active_count(area, n)` — per-area **active file** counts | runs `oq_pressure` and `oq_per_area` instead |
| SP-Q6 | `status_changed(h, prev, curr)` across snapshot | matches spec on fixture; **not run on real corpus** |
| SP-Q7 | any of the above with `--limit=1000`, verify streaming NDJSON | `capability::emit` is architecturally streaming but the `corpus_spike` binary emits a pretty-JSON ScaleReport followed by 3-row samples — not a real streaming test |
| SP-Q8 | `upstream(...) --explain` returning `_derivation` chains | bespoke companion-relation provenance for `release_blocker` and `upstream`; no general `--explain` flag |
| SP-Q9 | load external `anneal.dl`, observe override/shadow warning | not implemented |

**Reconciliation needed.** Either (a) add the literal SP-Q queries to
the spike as a conformance layer keeping the existing MVS-shaped tests
as project-specific coverage, or (b) revise the engine-spike spec's
SP-Q list to reflect what we actually want to validate. The current
state is "the spike passes" while "the spec's tests" are not all run.
Recommendation: (a), executed as Phase 0 closure work before any
user-visible Phase 1 surface depends on the new engine.

**2026-05-16 update.** `anneal-10c` implements option (a). The
`tools/spike-runner` Ascent harness now emits SP-Q1..9 pass/fail reports
alongside the older MVS reports. The SP-Q layer is literal where the MVS
layer had drifted: SP-Q1 projects open labels only, SP-Q3 queries
`upstream("formal-model/v17.md", anc)`, SP-Q5 counts active files by
area, SP-Q6 reports all status changes across `snapshot:last`, SP-Q7
validates the NDJSON row/report stream, SP-Q8 filters explained upstream
to the literal v17 query, and SP-Q9 validates a dynamic-IR
`fixture-anneal.dl` load plus `terminal/1` shadow warning. MVS probes
remain as broader capability coverage, not as substitutes for SP-Q.

---

## Part IV: Type-Design Debts [SR-T]

The spike's type model uses `&'static str` newtypes throughout
(`HandleId`, `Area`, `Namespace`, `FilePath`, etc.) backed by a
`Box::leak`-based interner in `loader.rs`. This is acceptable for a
short-lived benchmark binary and not for production. Each item below
is spike-acceptable; each must be revisited in Phase 1.

| Site | Debt | Phase 1 move |
|---|---|---|
| `loader.rs::Interner` | leaks every unique string for the process lifetime | arena-backed interner with explicit ownership (`Arc<str>` table or `lasso::Spur` ids) |
| `types.rs::Status::Other("")` | conflates absent status with corpus-specific status; both are `is_active()` | `Option<Status>` at the boundary; `Missing` variant or absent-handle-kind alternative |
| `types.rs::PIPELINE_ORDERING`, `Status::is_settled()` | hard-codes corpus policy into the binary | lattice ordering loaded from `anneal.toml` / `convergence.dl`; `is_settled` is project predicate, not engine constant |
| `types.rs::DiagnosticCode` | closed enum over `E001..S005` | built-in codes + opaque project IDs (`PROJ-001` from `anneal.dl` per LR-R3) |
| `types.rs::Namespace::NONE = ""` | sentinel | `Option<Namespace>` field-level |
| `types.rs::EdgeKind::Other(&'static str)` | escape hatch with `'static` requirement | string-table-indexed kind |

None of these are blockers for the engine-viability decision. All are
load-bearing for shipping v2.0.

---

## Part V: Architectural Revision [SR-A]

Earlier session notes claimed the v2.0 architecture should be **"ascent
for the prelude, parser+interpreter for project `anneal.dl`."** Peer
review (codex, 2026-05-13) identified a real problem with that split.

### §3 Problem with the original split

`2026-05-03-language-redesign.md` §5 (LR-D2) specifies that:

- Prelude `.dl` files load first, all of them in lexical order.
- `anneal.dl` loads second, with **total replacement** when a project
  predicate's name matches a prelude predicate.
- Inline `where` rules load third, scoped to the current query.

If prelude rules are compiled into ascent and project rules live in
a separate runtime interpreter, the shadowing/replacement boundary
crosses two engines. Multi-clause union, total replacement,
`--explain` provenance through a project rule that calls a prelude
predicate, and the warning behavior at the shadow point all become
hairy. The spec's vision — prelude is readable, project extends it
freely — does not survive a hard engine boundary.

### §4 Revised architecture [SR-D1]

**Definition SR-D1 (Engine layering).** v2.0 should layer as:

| Layer | Form | Responsibility |
|---|---|---|
| **Ascent (compiled)** | Rust binary | only the engine-derived *primitives* — `upstream`, `downstream`, `impact`, `freshness`, `flux`, `pipeline_position`, `cite_count`, `in/out_degree`, `discharge_count`, `terminal`, `active`, `obligation`, `discharged`, `token_estimate`. Anything that needs Rust-native traversal, date math, or aggregation across the corpus universe. |
| **Dynamic IR (interpreted)** | parser + evaluator | the entire Horn-clause rule layer — prelude `.dl`, project `anneal.dl`, inline `where` rules. Compiles to a shared IR; evaluates over ascent's primitive relations via a thin FFI surface. |
| **Project** | `anneal.toml` + `anneal.dl` | handle conventions, lattice config, project predicates, prelude overrides — all loaded into the dynamic IR. |

This preserves LR-D2 verbatim: prelude and project live in one
evaluator with one set of shadowing rules. Ascent stops being "the
engine" and becomes "the fast primitives the engine uses." This is
also more honest about what we proved: ascent is fast and type-safe,
not magic.

The research-graph check points the same way:

- `static languages prevent runtime introspection` warns that a
  compiled artifact severs the source/runtime link that v2.0's
  query language is meant to preserve.
- `observable semantics lock in implementation details and block
  optimization` and `hirams law makes all observable interpreter
  behavior a permanent api commitment` warn against letting spike
  output shapes or evaluation quirks escape before the compatibility
  contract is explicit.
- `language runtime bootstrap requires broad infrastructure before
  any program can run` warns that a dynamic rule layer will have a
  long invisible-progress phase; parity fixtures and layer tests are
  the mitigation.
- `language quality validation requires production use not internal
  development` argues for treating Phase 1 semantics as provisional
  until real corpora and agent workflows exercise them.

### §5 Trade-offs

| Concern | Old (ascent for prelude) | New (ascent for primitives) |
|---|---|---|
| Performance — prelude evaluation | likely faster | likely slower (interpreted) |
| Shadowing semantics | hard across engine boundary | natural in one IR |
| Provenance / `--explain` | partial proof (companion relations) | designed-in (IR records derivation steps as it evaluates) |
| Runtime `.dl` loading | impossible without separate engine | inherent |
| Hot-path cost | low | needs profile; primitives stay in ascent so the worst rules don't pay interpretation cost |

The performance question is real. Phase 1 should measure prelude
evaluation in the dynamic IR on large-corpus-scale data before committing.
If interpreted is too slow, a **compiled prelude with explicit
shadow-disabling at known override points** is the fallback — but
that should be a measured fallback, not the default.

---

## Part VI: Open Risks [SR-R]

Risks the spike did not retire and that Phase 1 must engage with:

**SR-R1: Provenance contract.** The companion-relation pattern proves
one hand-instrumented path; general `_derivation` over arbitrary
rules is much bigger. Per-rule explicit instrumentation does not
scale — Phase 1 needs a derivation-tracking design that the IR can
implement uniformly. Memory cost of stored provenance is also
unbounded today.

**SR-R2: Parity discipline missing.** No `tools/parity-runner`
exists. The dual-CLI deprecation cycle (language-redesign §33) cannot
honestly start until v2.0 outputs match v1.1 within the regression
budget (SP-R2). Without this discipline, v2.0 will silently diverge
during implementation.

**SR-R3: Hyrum's Law on spike scaffolding.** First-emitted query
semantics become API. NDJSON record shapes (`{capability, row: {...}}`
+ `CapabilityReport`), the `_derivation` shape in MVS-8 output, and
the field names in `BlockerRow` are all visible from the spike's
binary output. Treat any of these that survive into Phase 1 as
breaking-change surfaces, not implementation details.

**SR-R4: Real-corpus stress is light.** large-corpus has 13 transitive
`upstream` facts (DependsOn edges are rare in that corpus; Cites
dominates at 6,376 of 6,412 edges). Transitive closure and recursive
provenance are not actually stressed by the current measurements.
Phase 1 should bench against host-corpus's design corpus or synthetic
graphs with denser DependsOn fan-out.

**SR-R5: Snapshot subsystem unbuilt.** MVS-6 worked on a hand-coded
`SNAPSHOTS` fixture. Real implementation requires loading from
`.anneal/history.jsonl`, resolving ISO dates and `--7days` to nearest
snapshots, and reparsing entire corpora against git refs. These are
substantial subsystems; SP-R1 §4 perf targets are unmeasured because
none of them exist.

**SR-R6: Unsafe in transitive dependencies.** spike-runner's
`unsafe_code = "deny"` policy applies only to its own crate. ascent
0.8 contains unsafe blocks (`ascent/src/c_rel_index.rs:94,209,255,282`
plus siblings). Phase 1 needs an explicit audit or a documented
scoped-risk acceptance.

---

## Part VII: SP-NT1 Detail

Cyclic negation reject test passes at compile time. The exact ascent
error captured at `tools/spike-runner/tests/compile_fail/sp_nt1_cyclic_negation.stderr`:

```
error: use of aggregated relation `advancing` cannot be stratified
  --> tests/compile_fail/sp_nt1_cyclic_negation.rs:25:31
   |
25 |     blocked(h) <-- active(h), !advancing(h);
   |                               ^
```

For static compiled rules this is earlier than load-time. For v2.0's
runtime `.dl` loader contract, it is not enough: the dynamic loader
still needs to reject the same program before evaluation. It is also
weaker than the spec's "naming both rules in the cycle" expectation —
ascent names only `advancing/1`, not both `blocked/1` and
`advancing/1`. The cycle itself is not named.

Phase 1 implication: if the spec's diagnostic-quality bar matters
(it probably does for agent ergonomics — agents need to act on
errors), Phase 1's loader should pre-validate rule sets with its own
cycle detector before handing off to ascent, surfacing a richer
diagnostic that names every rule in the cycle.

---

## Part VIII: Decision [SR-DR1-revisited]

The protocol's SP-DR1 conditions:

- One engine satisfies SP-R1 — no; ascent satisfies the narrower
  "static primitive substrate is viable" claim, not SP-R1 as written
- All SP-Q queries pass — no; 4 of 9 match spec literal and the rest
  drift from spec, see §SR-Q
- Cyclic-negation rejected at load — no for dynamic `.dl` loading;
  yes only for static Ascent compile-time validation, with diagnostic
  caveat
- Spike report identifies open blockers — this document

**Honest verdict: SP-DR1 is not met as written.** The spike supports
a narrower decision: use ascent for fixed engine-derived primitives
unless Phase 1 measurements falsify that choice. The closure work
(artifacts, parity, unsafe audit, real SP-Q queries, snapshot subsystem)
is real Phase 0 debt that should either be retired before Phase 1
implementation work starts, or be explicitly scoped into Phase 1 as
its first deliverable.

### §6 Recommended path forward

1. **Accept the architectural revision** (§SR-A): ascent for
   primitives, dynamic IR for rules. Update the v2.0 language-redesign
   spec to reflect this.

2. **Phase 1 starts with closure work, not with engine implementation.**
   Specifically:
   - Add `tools/parity-runner` with PD-1..3 baselined (anneal v1.1
     against itself on large-corpus) — gets the harness ready before any
     v2.0 output exists to diff against.
   - Build the dynamic-IR skeleton (parser + evaluator) and bench
     prelude evaluation on large-corpus; gate the architectural revision
     on this measurement.
   - Freeze `.fixtures/sample-corpus/` so future runs are
     reproducible.
   - Run an unsafe audit on ascent's transitive code and document
     scope.

3. **Update SP-D2.** This report supersedes the "spike complete"
   framing. The honest record is that we know ascent is fast and
   typed-correct; we have not yet built the dynamic IR or any v2.0
   user surface.

4. **Defer the seven-verb prototype** to after the dynamic IR exists.
   Verbs are saved queries; building them on top of an architecture
   we may revise is wasted work.

If the dynamic-IR bench in step 2 fails to meet a tightened SP-R1
warm-evaluation target (say <50ms on large-corpus), revisit the
compiled-prelude option with explicit shadow-disable annotations.

---

## Labels

### SR-V (Verdict)
- SR-V1: MVS-1..8 + SP-NT1 validation results (§1-§2)
- SR-V2: SP-R1 sub-criteria scorecard (§2)

### SR-G (Gap)
- SR-G1: SP-D2 artifact deliverable status (§II)
- SR-G2: SP-D5 parity-runner status (§II)

### SR-Q (Query drift)
- SR-Q1..9: per-SP-Q reconciliation (§III)

### SR-T (Type debts)
- SR-T1..6: type-design debts for Phase 1 retirement (§IV)

### SR-A (Architecture)
- SR-D1: Engine layering (§4)
- SR-D2: Trade-off matrix (§5)

### SR-R (Open risks)
- SR-R1: Provenance contract (§VI)
- SR-R2: Parity discipline missing (§VI)
- SR-R3: Hyrum's Law on spike scaffolding (§VI)
- SR-R4: Real-corpus stress is light (§VI)
- SR-R5: Snapshot subsystem unbuilt (§VI)
- SR-R6: Unsafe in transitive deps (§VI)

---

## References

### Internal
- `2026-05-03-language-redesign.md` — the spec the spike validates
- `2026-05-07-engine-spike-and-parity-protocol.md` — protocol this
  report fulfills (partial)
- `anneal-spec.md` Parts I-III — preserved domain model

### External
- ascent 0.8 — `https://github.com/s-arash/ascent` — the candidate
  engine; chosen substrate for fixed primitives in the revised
  architecture
- crepe — `https://github.com/ekzhang/crepe` — alternative
  considered but **not actually evaluated**; flagged as Phase 1
  comparison work if the dynamic-IR direction needs validation
- Cozo — `https://github.com/cozodb/cozo` — reference for
  `take_until` aggregation if LR-OQ1 reappears
