---
status: current
locked: 2026-06-09
date: 2026-06-09
authors: [claude]
reviewed-by: codex (adversarial, 2026-06-09 — REVISE-THEN-LOCK; revisions folded)
bd: anneal-tdik
relates:
  - 2026-06-09-dimensional-foundation.md   # the arc; topic is the declared 9th axis
  - 2026-05-13-corpus-runtime.md           # CR-D103 (authority), CR-D104 (axis map; topic reserved)
  - 2026-06-08-navigate.md                 # the hub policy this reuses; topical navigate deferred here
  - 2026-06-08-currency.md                 # unmarked supersession = the suspect consumer
---

# The topic axis: pairwise siblings, not partitions — 2026-06-09

**For codex adversarial review before implementation.** The clustering keystone
(`tdik`), simulated on murail first — and the simulation *rejected the assumed
shape*. CR-D104 reserved topic with oracle "community structure over Cites";
the evidence says partition-based community detection is the wrong primitive and
a **pairwise sibling relation** is the right one. Both downstream consumers
(`z4x3` unmarked-supersession suspect; `xhpl` topical neighbor scoring) ask a
pairwise question.

## What the simulation found (murail, 2026-06-09)

1. **The declared oracle alone (label co-membership) misses the case that
   matters.** Only 47% of files cite a label; the z4x3 stale spec (05-30) cites
   **zero** — old docs predate labeling passes, and stale docs are by
   construction old. High precision where present (667/828 strong-shared-label
   pairs are CROSS-area — real information directories can't give), wrong
   coverage shape.
2. **Partition-based community detection degenerates.** Deterministic label
   propagation over the hub-excluded file-file Cites graph: 9 communities, but
   one of **183/251 nodes (73%)**. A "topic" containing most of the corpus
   answers nothing. (The acid pair did co-cluster — inside the useless giant.)
3. **Pairwise bibliographic coupling works and subsumes both oracles.** Two
   files sharing ≥2 citation targets (files *and* labels, hub-excluded;
   section targets are intentionally excluded):
   - the acid pair couples (3 shared targets, cosine 0.37);
   - coupling coverage (41%) strictly contains label-only coverage (27%) —
     label citations are just citation targets, so **one mechanism carries the
     declared and structural signals with declared naturally weighted in**;
   - the topic *neighborhood* it returns is genuinely topical (the stale spec's
     top siblings are the parametric-perf spec family).
4. **The axis composition does the rest** (CR-D104: features compose axes).
   Raw coupling ranks the true successor 9/60 — correct, because topic only
   answers *same subject*. Compose topic-sibling ∧ newer (`authored_age`) ∧
   non-terminal (lifecycle) ∧ not-already-marked (currency):
   **suspect set = 6 docs, true successor rank 3, every member a plausible
   successor.** That hint would have caught the real z4x3 incident.
5. **The remaining 59% of files have no topic signal** — under CR-D103 that is
   *signalled* ("no topic signal"), never faked.

## The decision (locked, codex-reviewed)

**Topic is a pairwise axis.** No partition, no community ids. Pure Datalog
first — with three structural requirements from review that are
lock-blocking, not style:

1. **Canonical pairs.** The oracle relation is `topic_pair(left, right,
   shared)` with `left < right` (string ordering is supported), so no
   symmetric duplicates leak into surfaces or counts. A symmetric
   `topic_sibling(a, b, shared)` view (two clauses over `topic_pair`) is the
   consumer-facing form.
2. **Target-first derivation.** Pairs originate from shared targets, never
   from an all-file-pairs outer binding — that is what makes the mega-target
   cap actually bound the work (cliques ≤ K² per target):

```
topic_citation_target(t) :=
  *edge{from: f, to: t, kind: "Cites"},
  *handle{id: f, kind: "file"},
  *handle{id: t, kind: "file"}.

topic_citation_target(t) :=
  *edge{from: f, to: t, kind: "Cites"},
  *handle{id: f, kind: "file"},
  *handle{id: t, kind: "label"}.

topic_target_citation_count(t, n) :=
  topic_citation_target(t),
  n = Count{ f : *edge{from: f, to: t, kind: "Cites"},
                 *handle{id: f, kind: "file"} }.

topic_mega_target_cap(40).                      # named shipped policy default

topic_nondiscriminative_target(t) := orientation_curated_hub(t).
topic_nondiscriminative_target(t) :=
  topic_target_citation_count(t, n), topic_mega_target_cap(k), n > k.

topic_shared_target(a, b, t) :=
  *edge{from: a, to: t, kind: "Cites"},
  *edge{from: b, to: t, kind: "Cites"},
  *handle{id: a, kind: "file"}, *handle{id: b, kind: "file"},
  topic_citation_target(t),
  a < b,
  not topic_nondiscriminative_target(t).

topic_pair(a, b, shared) :=
  topic_shared_target(a, b, anchor),            # originate target-first
  shared = Count{ t : topic_shared_target(a, b, t) },
  shared >= 2.

topic_sibling(a, b, shared) := topic_pair(a, b, shared).
topic_sibling(a, b, shared) := topic_pair(b, a, shared).
```

3. **Global-prelude cost is the perf decision point.** As a global prelude
   relation, every command may pay for topic during fixpoint — not just
   context/search. Pure-prelude is acceptable **only if** the target-first
   derivation is cheap enough as always-on global vocabulary (measure
   status/search/context on murail). If it is not: keep it verb/query-local
   for the first consumer, or promote to a Rust-native primitive with the
   identical relational shape. **Decide on measured cost, not taste.**

- **`topic_nondiscriminative_target`** is its own explicit policy — curated
  inventory file handles (`orientation_curated_hub`) **plus** any target
  cited by more than K files. Do not assume the navigate hub predicate alone
  covers label/inventory targets; the cap is named
  (`topic_mega_target_cap(40)`) and the count relation
  (`topic_target_citation_count`) is queryable so `describe topic` can
  explain *why* a target was excluded. Fixed default first; corpus-relative
  can wait.
- Disposition: **REPORT, never an asserted edge** (locked by z4x3's design
  constraint and CR-D104's reserved row). Topic *flags*; it never *asserts*.
- Score: `shared` count only (integer, monotone) — sufficient for the
  suspect surface composed with the newer/operative/unmarked filters.
  Normalization (cosine needs sqrt, not a builtin) reopens only for the
  later topical-neighbor-ranking slice; do not add math to anticipate it.

## The consumers (this slice ships the first; the axis enables both)

**(a) `currency_suspect` — the z4x3 unmarked-supersession hint:**

```
currency_suspect(stale, newer) :=
  topic_sibling(stale, newer, shared),
  authored_age(stale, a), authored_age(newer, b), b < a,   # lower age-days = newer; same-day → no row (conservative)
  operative(newer),
  not currency_superseded(newer),    # an already-displaced middle node is not the suspect head
  not currency_superseded(stale).    # marked case already handled by currency proper
```

Surface: a REPORT annotation on context/search hits — **"N unmarked newer
topical siblings (top: X)"** — grouped/collapsed, never a down-rank by itself
in the first cut (ranking integration is a separate, perf-gated follow-up).
**Wording is lock-blocking: never "successor"** — succession is asserted only
by a real `Supersedes` edge; the hint names a *possible sibling*. The surface
distinguishes "no topic signal" from "topic signal, no suspected sibling";
absence of the annotation when there is no signal is honest silence (the hint
is additive).

**(b) topical neighbor scoring (`xhpl`)** — `topic_sibling` becomes a
candidate source / scoring term for `context` neighbors later; full
root→intent topical paths stay deferred (the navigate decision stands:
curated verbs only where the walk is structurally sound).

## area reconciliation (the CR-D104 update)

`area` **stays on structure** — it is the *declared* directory grouping, a
clean oracle, exercised by `status`/`area_health`/`area_frontier`. Topic is
the *derived* subject axis; they answer different questions (the sim proved
it: 667/828 strong topic pairs cross areas). The reconciliation is
definitional, not a merge: **CR-D104's topic row is amended explicitly** (not
silently redefined) from "community structure over Cites (future clustering
substrate)" to "shared citation targets over hub-excluded `Cites` (pairwise)";
the topic axis card + `axis`/`axis_of` rows land with the slice (the ut1j
placement test enforces this mechanically). Placements: `topic_pair`,
`topic_sibling`, `topic_target_citation_count`, `topic_nondiscriminative_target`,
`topic_mega_target_cap` → **topic** (or topic-infrastructure for the policy
facts); **`currency_suspect` → composition** — it composes topic × recency ×
lifecycle × currency, and per CR-D104 rule 2 that is a composition predicate,
not pure topic.

## Acceptance (deterministic, gated)

- `topic_sibling` on murail: the acid pair (05-30 × 05-31) couples with
  shared ≥ 2; the index-class and mega-target exclusions hold (LABELS.md and
  >K-cited targets contribute nothing).
- `currency_suspect(stale, 05-31)`: a small set (order ~6 on today's murail)
  for unmarked stale topical siblings; the already-marked 05-30 row is zero by
  design (`not currency_superseded(stale)`).
- All existing surfaces **byte-identical** on murail + `.design` (this slice
  adds relations + an annotation surface; ranking unchanged). Schema diff =
  exactly the new predicates, enumerated. The context annotation diff is
  enumerated.
- **Perf-gated explicitly**: the pairwise aggregation is the risk (the eygi
  lesson); measure context/search/status before/after on murail; the
  mega-target cap is the lever if it regresses.
- `describe topic` works (axis card); `axis_of` rows placed; the ut1j
  placement test passes; `anneal check` clean on `.design`.

## Explicitly NOT in this slice
- No partition / named communities / community ids (degenerate per the sim;
  no consumer needs them).
- No machine-asserted `Supersedes` edges from the suspect hint, ever.
- No ranking changes (suspect is an annotation; ranking integration is a
  separate perf-gated slice).
- No new Rust primitive (pure prelude unless review proves the aggregation
  can't hold the perf gate — open question 1).

## Open questions — resolved at review
1. **Datalog vs primitive** → pure Datalog first, but only in the canonical
   target-first shape, with **global-prelude cost as the explicit decision
   point** (measure status/search/context; fall back to verb-local scoping or
   a Rust primitive on measured failure, not taste). Codex probe confirmed
   string `<` and the pairwise Count self-join parse and execute.
2. **Score shape** → shared-count only for this slice; normalization reopens
   with the topical-neighbor-ranking slice, not before.
3. **K** → fixed named default `topic_mega_target_cap(40)`, queryable count
   relation alongside; corpus-relative deferred.
4. **Annotation surface** → context first (the orientation surface); "N
   unmarked newer topical siblings (top: X)", collapsed; never "successor".
   Whether search gets it too is an implementation-time call on rendering
   surface, not semantics.
5. **Age comparison** → `b < a` in age-days = newer is correct; same-day → no
   row, conservative, accepted. Added `not currency_superseded(newer)` so a
   marked middle node never becomes the suspect head.
