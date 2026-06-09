---
status: current
date: 2026-06-09
authors: [claude]
bd: anneal-jkt4
relates:
  - 2026-06-09-the-convergent-corpus-runtime.md   # the synthesis this operationalizes (Part VI moves 1-3)
  - 2026-06-08-trust-invariant.md                  # xy45 — the disposition gate, folded in
  - 2026-06-08-currency.md                         # the proof that clarifying an axis makes features simple
---

# anneal — establishing the dimensional foundation — 2026-06-09

The working spec for the next arc. The synthesis (`the-convergent-corpus-runtime`)
named the target; this establishes the **foundation** that gets us there:
find and fix the **axes**, simplify the codebase, remove what no longer makes
sense, and leave a clean, **evidence-backed** base for the core goal. Not
backward-compatibility constrained. Framed as **one coherent transition**, not a
pile of micro-slices — the axes *are* the map.

## Why this is the frontier

The whole arc taught one lesson, twice: **clarifying an axis is what makes the
features on it correct and simple.** Currency was tangled with lifecycle until we
separated them — and the separation *caught two soundness bugs* and shrank the
design. The open bd queue is the same disease unaddressed: `dqfq` (field-name
inconsistency), `bmq` (file-local vs corpus-level), the recency family, the
139-predicate long tail of uneven evidence. These are **symptoms of unclean
axes**, not independent chores. Treating the cause (the axes) dissolves the
symptoms *and* clears the ground for the clustering keystone (a clean 9th axis,
not a tangled add-on). This is **anneal annealing its own vocabulary** — pointing
its convergence discipline at itself.

## The core goal this foundation serves

Re-findability + trust for amnesiac agents over a churning corpus — i.e.
**provenance + navigation**, presented oracle-honestly. anneal is also the
**prototype/proving-ground for Herald's substrate**; a clean axis foundation here
is what lets that substrate be trusted. Every axis and predicate must earn its
place against this goal — *with evidence*.

## The method (one transition, four movements)

### 1. Establish the axes as first-class
Name the orthogonal dimensions, define each precisely, and **assign every
predicate to exactly one**. An axis is defined by: the **question** it answers,
its **oracle** (what makes its answer earned), its **disposition**
(GATE/REPORT/TREND/PRE-FLIGHT), and its **monotonicity**. A predicate that can't
be placed on one axis is a tangle to resolve or a cut.

| axis | question | oracle | disposition | state |
|---|---|---|---|---|
| **relevance** | matches my query? | text × query | REPORT | clean |
| **currency** | displaced? | `Supersedes` edges | REPORT (marked GATE-able) | **just cleaned** |
| **lifecycle** | draft / operative / retired? | `status` field | REPORT / PRE-FLIGHT | clean (just split from currency) |
| **recency** | authored / changed / observed *when*? | dates · mtime · snapshots | REPORT / TREND | **TANGLED — next untangle** |
| **importance** | central? | degree / cites | REPORT | clean |
| **convergence** | settling? | snapshot deltas | TREND | clean-ish |
| **structure** | organized / connected? | `edge` + kinds | REPORT | broad; `area` is proto-cluster |
| **obligations** | owed? | obligation/discharge facts | GATE? (verify) | under-exercised |
| **topic** *(coming)* | same subject? | labels + community detection | REPORT (never asserted edge) | the clustering keystone |

The deliverable is this table made *true and enforced*: each axis precise, each
predicate placed, tangles named.

### 2. Evidence — exercise or cut
A predicate earns its place only if **a verb, a real query, or a consumer need
exercises it**; otherwise it is a cut candidate. The reduction is an *evidence
pass*: the 139 derived predicates have uneven evidence, and we don't currently
know which are load-bearing. Default verdict, per the surface-evolution ethos:
**CUT**. Removal is the primary act; additions must justify against the goal.

### 3. Simplify and remove
Concrete targets (not exhaustive — the transition is unbounded by design):
- **Untangle recency** — the next currency/lifecycle-style win: separate
  *authored age* (`freshness`) from *change recency* (`changed_within`, retire
  git-mtime as a currency/age proxy) from *history movement* (`flux`, snapshots);
  collapse the overlaps; one clear predicate per sub-notion.
- **Dissolve the symptom-debt** — `dqfq` (field-name consistency), `bmq`
  (corpus-level vs file-local), and kin become axis-cleanups, resolved by getting
  the axis right, not patched in place.
- **Cut dead vocabulary** — unexercised prelude predicates, abandoned families,
  redundant variants.
- **Continue codebase simplification** — the `pcwd` decomposition tail and
  `orpd` "deeper reduction" fold in: the cleaner the substrate, the clearer the
  axes (and vice versa).

### 4. The disposition gate (xy45), applied uniformly
`xy45` becomes the standing rule and a CR-D in the master spec: **every surviving
predicate/surface carries a disposition on a named axis, and presents only the
authority its oracle earns.** This is the gate every survivor must pass and every
new predicate must answer.

## The clean foundation (acceptance — by shape, not task count)

We are done with this arc when:
- the axes are **named, precise, and orthogonal**, with every predicate placed and
  no known tangle (recency resolved);
- the vocabulary is **evidence-backed** — every predicate is exercised or cut;
- every surface is **disposition-typed** (xy45 uniform);
- the symptom-debt beads are **dissolved**, not patched;
- the result is **smaller** (fewer predicates, fewer verbs, the language as the
  power surface) and **ready for clustering** as a clean ninth axis.

Then — and only then — the **clustering keystone** lands cleanly: `topic` as the
ninth axis (reconciling `area`), unlocking topical-navigate + unmarked-currency.

## Non-goals (what we are NOT relitigating)
The substrate is sound and stays: the planned executor, the `ir/`/`vm/` split,
the machine gates, the Source-trait substrate/adapter/surface architecture. This
arc is about the **vocabulary and the axes over** that substrate, plus the
ongoing code simplification — not a re-architecture.

## Evidence discipline (how we stay honest)
- **Exercise-or-cut** is verified against real queries / verbs / consumer use
  (murail is the dogfood corpus), not asserted.
- Axis changes are **differential-gated byte-identical** where they touch
  behavior, **and perf-gated** (the byte-identical-misses-perf lesson).
- Each removal must **delete a manual practice or a real complexity** — subtractive,
  per the synthesis. If a slice only adds metadata ceremony, it doesn't ship.
- The corpus stays its own witness: `anneal check` clean, and the prelude/specs
  themselves trend toward settled (anneal annealing itself).
