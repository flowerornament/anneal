---
status: draft
date: 2026-06-09
authors: [claude, morgan]
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

> **Code at HEAD is current by construction — it is what runs. A spec is
> current only by maintenance, which is the exception. The default state of a
> design assertion about code is "true as of its authoring," not "true."**

Consequences (design principles, pre-locked by CR-D103):

1. **Cross-edges are dated assertions, not standing facts.** A spec→code edge
   means "this spec referred to this code *as of the spec's date / the
   revision then current*." Presenting it as a live relationship is the lie.
   The schema already carries what's needed: `*handle.date` on the spec side,
   `revision` (git sha) on the code side, and git history between them.
2. **Cross-currency has a real, computable oracle** — and it is *referent
   drift*, not absolute age: *has the cited code changed since the assertion
   was authored?* `git log <path> --since=<spec date>` is a clean oracle:
   zero commits → the assertion's ground is intact (whatever its age); N
   commits / moved / deleted → the assertion is suspect *in proportion to
   evidence*. This is content-age **relative to the referent** — the honest
   version of the recency/currency composition, and it sidesteps the
   degraded-git-mtime trap (we ask about the *referent's* history, not the
   citing file's mtime).
3. **The joint graph amplifies trust failures if currency is flat.** A stale
   spec linked to live code *gains* false authority from the link. So the
   cross-relation surface ships currency-qualified or it does not ship:
   every cross-edge presentation carries its drift disposition
   (`intact` / `drifted(n)` / `referent-moved` / `referent-gone`), and
   navigation across the boundary routes through current heads, exactly as
   `--lineage` does within a corpus.
4. **Mass-staleness is the normal input, not the degenerate case.** On murail
   today the honest report is likely "most design→code assertions have
   drifted." The surfaces must be designed for that answer — aggregate first
   (drift profile per area/spec), drill-down second — so the truth is usable
   rather than a wall of red. A tool that makes a real corpus look 90% broken
   on day one is honest but dead; disposition + grouping + head-routing is
   how honesty stays consumable.

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
   second source. Per-axis honesty about the hypotheses — lifecycle and
   currency near-certain (the compiler enforces them: unstable/stable/
   deprecated, `#[deprecated(note)]` is a declared supersession edge); topic-
   as-coupling strong; **recency and convergence are open hypotheses**
   (`since="1.45"` is a stabilization date, not authoring; "settling" may be
   release-cadence, not snapshot drift). The simulation answers these, not
   the spec.
2. **Cross-relation** (the goal): the joint graph with drift-qualified
   cross-edges, proven on a corpus that *has both sides*.

Which fixes the flagship choice: rust-stdlib proves (1) at scale but has no
design corpus, so it cannot prove (2). The corpora that can are the ones in
hand — **anneal itself** (`.design/` + `crates/`, where CR-D labels are
already cited from code and commits) and **murail** (`.design` + the crates
its specs cite, with the mass-staleness reality built in). Likely shape:
simulate extraction on a real external crate for (1); aim the product
milestone at the self-corpus for (2) — *anneal annealing itself, one layer
deeper* — with murail as the adversarial second corpus precisely because its
design layer has drifted.

## Method (the loop, unchanged)

Simulate → design → adversarial review → lock → implement → gate; coordinator
verifies the landed feature against the *simulation's* expectations. First
moves, before any adapter code:

1. **Extraction sim**: rustdoc-JSON over a real crate — what handles/edges/
   lattice actually come out; where the axis hypotheses bend.
2. **Cross-currency sim**: on murail/anneal *today* — for every spec→code
   citation, compute referent drift (commits since spec date; moved; gone).
   Measures the mass-staleness reality (principle 4) and validates the drift
   oracle (principle 2) on real data **before** designing surfaces over it.
3. Then the adapter design (against `Source` §5) and the cross-edge identity
   question (how a spec's path-string resolves to a code handle across
   corpora — CR-D41 federation identity does the heavy lifting).

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
