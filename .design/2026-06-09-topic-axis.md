---
status: draft
date: 2026-06-09
authors: [claude]
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
   files sharing ≥2 citation targets (files *and* labels, hub-excluded):
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

## The decision

**Topic is a pairwise axis.** No partition, no community ids, no new Rust
primitive. One derived relation built from existing stored facts:

```
# the oracle: shared citation targets, hub- and mega-target-excluded
topic_sibling(a, b, shared) :=
  *handle{id: a, kind: "file"},
  *handle{id: b, kind: "file"},
  a != b,
  shared = Count{ t :
    *edge{from: a, to: t, kind: "Cites"},
    *edge{from: b, to: t, kind: "Cites"},
    not topic_nondiscriminative(t)
  },
  shared >= 2.
```

- **`topic_nondiscriminative(t)`** = the navigate-arc index-class
  (`orientation_curated_hub`) **plus mega-targets**: a target cited by more
  than K files (sim used K=40) discriminates nothing (citing the formal model
  means little; sharing a niche label means a lot). This also **bounds the
  pairwise join**: cliques per target ≤ K², keeping the aggregation cheap.
- Disposition: **REPORT, never an asserted edge** (locked by z4x3's design
  constraint and CR-D104's reserved row). Topic *flags*; it never *asserts*.
- Score: `shared` count only (integer, monotone). The sim's cosine added
  ordering nuance but needs sqrt and float thresholds; start with the count +
  the composition filters, add normalization only if ranking quality demands
  it (open question 2).

## The consumers (this slice ships the first; the axis enables both)

**(a) `currency_suspect` — the z4x3 unmarked-supersession hint:**

```
currency_suspect(stale, newer) :=
  topic_sibling(stale, newer, shared),
  authored_age(stale, a), authored_age(newer, b), b < a,
  operative(newer),
  not currency_superseded(stale).     # marked case already handled
```

Surface: a REPORT annotation on context/search hits — "N newer topical
siblings exist (top: X)" — grouped/collapsed, never a down-rank by itself in
the first cut (ranking integration is a separate, perf-gated follow-up).
Degenerate input: no topic signal → no row → no annotation (silence here is
honest: the hint is additive).

**(b) topical neighbor scoring (`xhpl`)** — `topic_sibling` becomes a
candidate source / scoring term for `context` neighbors later; full
root→intent topical paths stay deferred (the navigate decision stands:
curated verbs only where the walk is structurally sound).

## area reconciliation (the CR-D104 update)

`area` **stays on structure** — it is the *declared* directory grouping, a
clean oracle, exercised by `status`/`area_health`/`area_frontier`. Topic is
the *derived* subject axis; they answer different questions (the sim proved
it: 667/828 strong topic pairs cross areas). The reconciliation is
definitional, not a merge: CR-D104's topic row updates from "community
structure over Cites (future clustering substrate)" to "shared citation
targets over hub-excluded `Cites` (pairwise)"; the topic axis card +
`axis`/`axis_of` rows land with the slice (the ut1j placement test enforces
this mechanically).

## Acceptance (deterministic, gated)

- `topic_sibling` on murail: the acid pair (05-30 × 05-31) couples with
  shared ≥ 2; the index-class and mega-target exclusions hold (LABELS.md and
  >K-cited targets contribute nothing).
- `currency_suspect(05-30, x)`: a small set (order ~6 on today's murail) that
  includes 05-31; zero rows for docs that are already marked-superseded.
- All existing surfaces **byte-identical** on murail + `.design` (this slice
  adds relations + an annotation surface; ranking unchanged). Schema diff =
  exactly the new predicates, enumerated. The context/search annotation diff
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

## Open questions for review
1. **Datalog vs primitive**: the pairwise Count aggregation over `*edge` —
   does the planned executor handle the self-join + aggregation at murail
   scale within the perf gate? If not, fallback is a Rust-native
   `topic_sibling` primitive (CR-D9 amendment) with identical relational
   shape — decide on evidence, not taste.
2. **Score shape**: shared-count only vs normalized (cosine needs sqrt —
   not in the expression builtins). Is count + composition filters enough
   for the suspect surface's ordering?
3. **K (mega-target cap)**: sim used 40 on murail (468 files). Fixed
   constant vs corpus-relative? Where does the constant live (prelude
   literal per CR-D42's "shipped policy defaults" precedent)?
4. **Annotation surface shape**: per-hit "N newer siblings (top: X)" — on
   context only, or search too? Collapse policy?
5. **`authored_age` vs date join**: `currency_suspect` needs date ordering;
   `authored_age` gives days-since — confirm the comparison shape
   (b < a in age-days = newer) handles same-day authorship sanely.
