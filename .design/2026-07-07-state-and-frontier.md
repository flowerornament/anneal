---
status: current
authority: orienting
date: 2026-07-07
authors: [claude, morgan]
purpose: >
  The state-of-anneal and the frontier, for the durable-tool era. anneal is no
  longer framed as a throwaway prototype for Herald — it is a tool worth using
  on its own terms and worth investing in. This reconciles the specs and bd
  tracker with the shipped reality (v0.21.3), states what "good" means, and lays
  out the prioritized frontier. Successor to the orienting role of
  2026-06-09-the-convergent-corpus-runtime.md (which remains the deep synthesis;
  this is the current-state + roadmap on top of it).
relates:
  - 2026-06-09-the-convergent-corpus-runtime.md   # the deep synthesis (history, architecture, the dimensional language)
  - 2026-05-13-corpus-runtime.md                  # the master spec (CR-D* law)
  - 2026-06-09-code-and-corpus.md                 # the shipped arc's charter
---

# anneal — state and frontier — 2026-07-07

## 1. The reframe

anneal is a **durable tool**, invested in for its own sake — not a prototype to
freeze once Herald matures. The governing mandate is simple: **make anneal good
on its own terms.** (Herald's currency arc is Herald agents' work; the better
anneal is, the better the example they learn from — but tracking Herald is not
anneal's job.)

## 2. What shipped (the two arcs are done and released)

```
 DIMENSIONAL FOUNDATION (v0.20.0)        CODE & CORPUS (v0.21.0 → v0.21.3)
 ───────────────────────────────        ────────────────────────────────
 • 9 declared axes (CR-D104)             • the joint design↔code graph
 • trust invariant (CR-D103)             • assertion provenance on edges
 • verb-surface rung (CR-D105)           • the referent-drift oracle (7 dispositions)
 • currency + recency untangled          • anneal-code adapter (rustdoc/EEP-48/file)
 • topic axis, vocab cut to evidence     • anneal-on-anneal joint graph, proven
                                            on herald (3.8% intact — mass-staleness)
```

Both arcs' design specs (`2026-06-09-code-and-corpus`, `-anneal-code-adapter`,
`-drift-refresh-design`, the `903i`/`bqqc` evidence) are `status: current` —
correctly, as the *living design* of shipped features, not superseded. The
`instrument-don't-conclude` stance (rankers expose their signal frame; the
over-conclusion lives at rendering) is the latest principle, shipped v0.21.3.

**Release cadence proof-of-life:** four clean releases in three days, each a
real consumer signal turned same-day fix (`0tfi` frontmatter code-ref from a
murail agent; the two herald perf wins from dogfooding; the ranker frame). The
tool is alive and its users find real bugs — the reason to keep it good.

## 3. Corpus-health readings (anneal on anneal, 2026-07-07)

Dogfooding the self-corpus surfaced its own signals, some of which are frontier
work:

- **`check` clean** — 0 broken refs, 0 errors. The corpus is consistent.
- **"blocked" = 6 completed-arc specs flagged `spec_code_drift`** + 1
  confidence_gap. These specs describe now-built code whose paths drifted —
  *correct drift reporting*, but they route to the **blocked** convergence
  section when they are not blocked, they are *drifted*. **`spec_code_drift`
  arguably belongs in `drifting`, not `blocked`** — a convergence-vocabulary
  refinement worth making (see frontier §4, corpus-honesty).
- **76 of 134 files statusless (57%)** — mostly legitimate dated point-in-time
  records (research/reviews/evidence); not a defect, but the coverage number is
  a reminder that orientation is graph+recency-led here, not status-led.
- The shipped ranker-frame works on anneal's own spine: `corpus-runtime.md`
  ranks #1 showing all four contributing signals, not one label.

## 4. The frontier (prioritized — "make anneal good")

```
 LANE                 ITEMS (bd)                         WHY / LEVERAGE
 ────                 ──────────                         ──────────────
 A. PERF (daily UX)   vs33  cold drift refresh 5:21      the sharpest wart on a
   the felt cost            → profile-first, git-spawn    real corpus; predictably
                            bound like the 6-9× win      a big win, measure first
                      eygi  eval env-sharing (~2.5→1.5s) status/context latency
                      78qc  murail-scale load ~8s        cold-load floor
                      kra   cache parsed @verb programs  load-path
 B. QUALITY           wno8  decompose anneal-code/lib.rs the eval.rs disease, one
   (the codebase)           (3,922 lines) — seam map      slice old; seam map
                            banked in the bead            banked, clean codex slot
                      orpd  deeper reduction pass
                      txkp  post-kftp polish (Tarjan,
                            plan caching, sort unify)
 C. CORRECTNESS       3nw5  drift recall gap: cross-crate a real oracle gap found
   & HONESTY               file MOVES read as no-drift    in the drift machinery
                      qxzm  3 nits (missing-handle hint,  small honest UX wins
                            --limit 0, ndjson alias)
                      NEW   spec_code_drift → drifting     corpus-honesty: drift is
                            not blocked (this dogfood)     not blocked (§3)
 D. FEATURES          a2bw  benchmark the search ranker   evidence for ranker tuning
   (planned)          hhv7  clippy::nursery (use_self)    cheap hygiene, murail method
                      (topical navigate / clustering      deferred; needs a consumer
                       remain deferred — no pull yet)      pull before building
```

**The recommended first move next session: `vs33`** — the cold drift refresh
(5:21 on herald) is the biggest felt cost, it is almost certainly the same
git-subprocess class as the cache-load win (34s→6s), and the profile-first
discipline makes it a predictable, satisfying win. Profile the refresh's
`compute_drift_evidence` path, confirm the hotspot, then dedup shared history
walks + parallelize the per-ref probes (pure function, embarrassingly parallel).

**Do NOT open** the TMS horizon or topical-navigate — both are construction
arcs that need a consumer pull anneal does not have. The method says wait.

## 5. Method (unchanged — the real asset)

simulate → adversarial review → lock → implement → gate → verify-against-the-sim.
Coordinator (claude) designs/reviews/releases; codex implements via smux.
Measure before designing (two confident guesses were wrong this session; a
profile named the truth both times). Byte-identical gates correctness, not
perf — gate perf explicitly.

## 6. New-session entry point

1. `bd prime` → `bd ready` (the frontier §4 items are the ready set).
2. This doc for the frontier; `2026-06-09-the-convergent-corpus-runtime.md` for
   the deep architecture; the master spec for CR-D* law.
3. Start on `vs33` (§4), profile-first.
