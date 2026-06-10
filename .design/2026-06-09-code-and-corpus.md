---
status: current
date: 2026-06-09
authors: [claude, morgan]
reviewed-by: codex (adversarial, 2026-06-09 — REVISE-THEN-LOCK; all 8 findings folded. Locked: goal=joint graph; dated assertions; currency-qualified-or-no-presentation; simulate-first; the authority ladder. Revised: oracle, identity, asymmetry, sim scopes, axis grades, flagship sequencing.)
relates:
  - 2026-06-09-the-convergent-corpus-runtime.md   # Part VI/VII — this is the next arc after the foundation
  - 2026-06-09-dimensional-foundation.md           # the axes this arc stress-tests
  - 2026-06-09-topic-axis.md                       # pairwise coupling — reused across the corpus boundary
  - 2026-05-13-corpus-runtime.md                   # §5 Source trait · §53 federation · CR-D41 identity · CR-D103/104
  - 2026-05-30 code-as-corpus spike (bd anneal-8yxl, closed)  # rustdoc JSON validated; stability attrs = lattice
---

# Code and corpus: the joint graph — 2026-06-09

Scoping seed for the next arc. Not locked; the simulation comes first.

## The goal, reframed

The obvious framing — "anneal-code: a second adapter, code as a corpus" — is
the **stepping stone**, not the goal. A doc corpus alone is documentation
hygiene; a code corpus alone competes with rust-analyzer. The goal is the
**joint graph**: design intent and implementation as first-class handles in
federated corpora, with **cross-edges that carry the trust story**:

```
which spec governs this module?          which code realizes this decision?
the spec says X — does the code?         the decision was superseded — does
the code changed — did the spec?         the code still implement it?
```

These cross-layer questions are where agents get burned worst, and nothing
else answers them. anneal is already half-reaching across this boundary —
specs cite code paths as edges *today*, `W006 spec_code_drift` exists,
`asserts_code` sits on the lifecycle axis, every fact carries `corpus` +
`revision` (CR-D41/CR-D8), and federation is spec'd (§53). What's missing is
the far side having handles at all.

## The keystone constraint: cross-corpus currency (Morgan, 2026-06-09)

**Getting currency right here is essential — and the joint corpus makes it
harder, not easier.** The empirical reality this arc must design for: in a
living project, *almost the entirety of the design corpus is out of date
relative to the code*. Murail's and Herald's `.design` are the named cases —
dozens of specs that were true when written and have not tracked the code
since. This is not corpus neglect; it is the **structural asymmetry**:

> **Code at HEAD is the current *artifact* — it is what runs. A spec is
> current only by maintenance, which is the exception. The default state of a
> design assertion about code is "true as of its authoring," not "true."**

Reviewed precision (codex P1): "HEAD is current" is a **PRE-FLIGHT artifact
premise, not a lifecycle/currency claim.** Counterclasses are real:
dead/unreachable code, feature-gated, generated/vendored, tests/fixtures,
unreleased work on master, deprecated-but-present APIs, and code
*intentionally lagging* a newly updated spec (the asymmetry can locally
invert). Code lifecycle still requires source-derived disposition
(exported / reachable / deprecated / test / generated / private); the
asymmetry governs the *default*, not every handle.

Consequences (design principles, gated by CR-D103):

1. **Cross-edges are dated assertions, not standing facts.** A spec→code edge
   means "this spec referred to this code *as of an assertion time*."
   Presenting it as a live relationship is the lie. **The assertion time is
   itself modeled, not assumed** (codex P0): CR-D8 gives `*edge` no
   date/revision field, and frontmatter dates can be editorial, copied, or
   absent — so assertion as-of = the citing handle's `date` (fallback), plus
   the citing handle's source `revision` where available, plus an explicit
   **`assertion_date_unknown` disposition** when neither exists. "Date exists
   on the handle" must never silently become "all cross-edges are dated."
2. **Cross-currency has a real but NOT clean oracle** — referent drift:
   *has the cited code changed since the assertion was authored?* The naive
   form (`git log <path> --since=<date>`) is **move-blind** — exactly the
   `3nw5`/W006 class, where moved paths read as gone — and the current probe
   (`target_probe.rs`) only does HEAD-history set membership, far weaker than
   the drift question. So the oracle is **move-aware and degraded-input-typed
   from day one**, with seven output buckets:
   `exact-path-intact` · `exact-path-drifted(n)` · `deleted` ·
   `moved-confident` · `moved-ambiguous` · `history-unavailable` ·
   `no-assertion-date`. **Only the first two are clean**; moved is strong
   only when the move chain is confident; every degraded bucket presents as
   UNKNOWN/REPORT — never as `intact`, never GATE. (Still the right shape:
   it asks about the *referent's* history, not the citing file's mtime —
   the degraded-git-mtime lesson holds.)
3. **The joint graph amplifies trust failures if currency is flat.** A stale
   spec linked to live code *gains* false authority from the link. So the
   cross-relation surface ships currency-qualified or it does not ship:
   every cross-edge presentation carries its drift disposition (the bucket
   vocabulary above), and navigation across the boundary routes through
   current heads, exactly as `--lineage` does within a corpus. The
   move-detection machinery stays *behind* the disposition contract —
   mechanics are accidental complexity, the disposition is the product.
4. **Mass-staleness is the normal input, not the degenerate case.** On murail
   today the honest report is likely "most design→code assertions have
   drifted." The surfaces must be designed for that answer — aggregate first
   (drift profile per area/spec), drill-down second — so the truth is usable
   rather than a wall of red. A tool that makes a real corpus look 90% broken
   on day one is honest but dead; disposition + grouping + head-routing is
   how honesty stays consumable.
5. **Cross-corpus identity is the central implementation problem, not a
   solved one** (codex P0): CR-D41 gives *within-corpus* id uniqueness and
   §53 defers the federation surface; neither resolves a markdown
   path-string citation to a code *item* handle. The bridge needs a designed
   resolver — keyed by `(corpus, origin_uri / native_id / path+range,
   revision)` or a dedicated cross-edge relation. This is a first-class
   design item of the arc, not plumbing.

## The disposition ladder, stretched across the boundary

"Principled but loose" = the existing authority ladder applied to
cross-relations. No total-linkage ceremony (the traceability-matrix failure
mode); partial coverage, honestly signalled:

| rung | mechanism | authority |
|---|---|---|
| **declared** | spec cites a code path; code doc-comment cites a spec/CR-D label | strong — author-declared, machine-checked, **drift-qualified** (principle 2) |
| **derived** | pairwise topic coupling *across* corpora (a spec and a module sharing citation targets/identifiers) | REPORT — suspect hint, never an asserted edge (the topic-axis design, unchanged) |
| **diagnostic** | drift checks: referent changed/moved/gone since assertion; superseded spec with live code-edges; code with no governing design | GATE/REPORT per check — surfaces disagreement, never auto-fixes |

## What the arc proves (in order)

1. **Source-agnosticism** (the stepping stone): the nine axes survive a
   second source. Per-axis hypotheses, as re-graded at review:
   - **lifecycle — plausible, not near-certain**: compiler attrs cover some
     *public API*, not private modules/tests/generated code; reachability/
     export class must supply the rest.
   - **currency — useful but weaker than `Supersedes`**: deprecation is not
     always displacement; `#[deprecated(note)]` parsing is REPORT unless the
     note names a *resolvable* successor.
   - **topic — strong**, but the nondiscriminative-target policy is needed
     from day one (`Vec`/`Option` are the LABELS.md of code).
   - **importance / structure — stronger than first graded**: call/use/
     containment are natural code axes.
   - **recency / convergence — open** (`since="1.45"` is stabilization, not
     authoring; "settling" may be release-cadence, not snapshot drift).
   - **obligations — weak** unless TODO/FIXME/tracking-issues are in scope.
   The simulation answers these, not the spec.
2. **Cross-relation** (the goal): the joint graph with drift-qualified
   cross-edges, proven on a corpus that *has both sides*.

Flagship, with the reviewed sequencing: **first lock on the self-corpus
(anneal `.design/` + `crates/`, where CR-D labels already cross the boundary)
plus one external Rust crate** (source-agnostic extraction). **murail/herald
join only after the move-aware oracle is honest** — otherwise the first
review drowns in false moved/gone classifications; they are the *hostile
validation* corpora, not the first target.

## Method (the loop, unchanged)

Simulate → design → adversarial review → lock → implement → gate; coordinator
verifies the landed feature against the *simulation's* expectations. First
moves, before any adapter code:

1. **Cross-currency sim FIRST, as an oracle audit** (not a product sim): for
   every existing spec→code citation (W006 already emits the refs and probe
   metadata), output citing handle/date/status, target path, current
   existence, commits-since-date on the exact path, rename/move evidence,
   history status, and the final bucket disposition. **Stop-rule: if this
   report cannot classify moves honestly, stop and fix the oracle design
   before anneal-code grows the graph.** Scope note: this runs on existing
   metadata + git only — it answers path-citation drift; "which code realizes
   this CR-D" needs the code side and waits. A cheap *reverse-edge scan*
   (CR-D labels in code comments/attrs) rides along to measure the reverse
   shape before surfaces are designed.
2. **Extraction sim**: rustdoc-JSON over a real crate — what handles/edges/
   lattice actually come out; where the axis hypotheses bend; **and scale as
   a hard output, not an open question**: handle/edge/content-byte counts,
   load time, fixpoint time, measured *before* any adapter design is blessed
   (the correctness-gates-miss-perf lesson, applied prospectively).
3. Then the adapter design (against `Source` §5) and the **cross-corpus
   resolver design** (principle 5 — the central problem, not plumbing).

## Open questions (seed — the sims will grow this list)
- Recursion (`kh6p` #1): transitive deps may be the first real demand for
  recursive *rules* vs query-local recursion. Answer with the consumer, not
  in advance.
- Scale: std-sized corpora make perf priorities evidence-driven (`eygi` rides
  alongside; `78qc` load cost becomes pressing).
- Cross-corpus `topic_nondiscriminative_target` policy: `Vec`/`Option` are
  the LABELS.md of code — the mega-target cap likely needs the corpus-relative
  form sooner here.
- Where does the drift disposition LIVE — on the edge presentation (per-hit
  annotation, like currency) or as derived predicates an agent composes? (Both,
  probably; the annotation teaches the query, per the v0.20 pattern.)
