---
status: current
authority: synthesis
date: 2026-06-09
authors: [claude]
purpose: >
  The orienting synthesis for anneal: what it is, how its design arrived here
  (the arc-of-arcs over ~40 prior specs), the architecture and conceptual
  language as they actually stand today, and the shape of the beautiful, elegant,
  powerful tool we are iterating toward. Written to be read cold and to be the
  one document that contextualizes all the others. Not backward-compatibility
  constrained — it states the target, and treats the past specs as the record of
  how we learned it.
relates:
  - 2026-05-13-corpus-runtime.md                  # the v2.0 master reference (detail)
  - 2026-06-06-disposition-typed-witnesses.md     # the disposition framework
  - 2026-06-08-trust-invariant.md                 # the governing design gate
  - 2026-06-08-currency.md
  - 2026-06-08-navigate.md
  - 2026-06-07-core-decomposition-plan.md
# frame (cross-corpus, prose): herald .design/synthesis/2026-06-07-the-convergent-substrate.md
---

# anneal — the convergent corpus runtime — 2026-06-09

## Abstract

anneal turns a body of knowledge into a **queryable, typed, self-auditing system**
and helps disconnected intelligences — agents across sessions with no shared
memory — orient in it and push it toward settledness. This document is the
synthesis: one idea, the history that arrived at it, the architecture and
conceptual language as built, and the shape we are heading toward.

**The one idea.** Model a corpus as a **log of typed, provenance-linked claims**,
and *everything you read* — relevance, currency, importance, convergence, a
navigation trail — is a **projection** over it, computed by a Datalog runtime and
presented with **only as much authority as its oracle earns.** (As-built today
the store is *generationed current-state with historical snapshot/trail
projections* — an approximation of the fully append-only ideal, which is the TMS
horizon in Part VI; the projection model holds either way.) Settledness is not
metadata bolted onto files; it is a *derived property of a queryable system.*

---

# Part I — What anneal is

## 1. The need

Bush's memex thesis is the frame: *the bottleneck of knowledge work is selection,
not storage — the record cannot be consulted as fast as it grows.* Specialize it
to anneal's user — **amnesiac agents, cold every session, churning a shared
corpus** — and the need is **reliable re-findability + trust under churn**: with
no memory, get to the *right, current, important* knowledge and *trust* it, fast.

Two failure modes, and they are the two jobs a stronger model will never subsume
for you (the bitter-lesson split):

- **orientation failure** — can't find it, or finds the wrong thing → *navigation.*
- **trust failure** — finds stale/peripheral material and is misled → *provenance.*

A confident, plausible, *wrong* answer is the worst outcome: it is silent and
upstream of the agent's reasoning, so no downstream capability recovers from it.
Everything anneal builds serves provenance + navigation, and nothing it presents
may claim more authority than its oracle earns.

## 2. The shape of the answer

anneal is a **substrate** (a Datalog runtime + a convergence standard library)
decoupled from **sources** (markdown today; code, hosts later) via a `Source`
trait, exposed through **surfaces** (CLI, MCP) that project the same contracts.
The corpus becomes a **fact store**; the convergence vocabulary becomes **derived
relations** over it; the agent reads those.

## 3. A note on vocabulary (and the Herald kinship)

Precision, since this doc consciously shares a lens with Herald's *convergent
substrate* — anneal is, in effect, a **prototype / proving-ground** for that
substrate at the corpus altitude. What is genuinely anneal's, and what is borrowed:

- **`Fact` / fact store — genuinely anneal's.** `facts.rs` defines `HandleFact`,
  `EdgeFact`, `SpanFact`, `ContentFact`, `MetaFact`, `SnapshotFact`, `FactBatch`,
  `FactStore`. Not bleed-over.
- **"projection" — the borrowed lens.** In *this doc* "projection" means a
  *derived relation over the fact store* — anneal's `kind: derived` prelude
  predicates. Note anneal's code already uses "projection" for a *different*
  thing (query-output columns), and Herald uses `Projection` as a materialized
  read-view primitive; the conceptual sense (a derived view over facts) is shared
  and apt, but anneal's native word for these is **derived predicate / prelude rule**.
- **"append-only log" — Herald's, not yet anneal's.** anneal's `FactStore` is a
  *generationed current-state* store (`FullSnapshot` replaces, `Delta` upserts +
  retracts), with `snapshot`/`trail` relations giving historical projections. The
  fully append-only event log (and the TMS over it) is the *target* (Part VI),
  which anneal approximates today.

The blur is deliberate — proving anneal's substrate is how we find out where
Herald's is still soft — but the differences above are real and load-bearing.

---

# Part II — How we got here (the arc-of-arcs)

Six arcs over ~40 specs, each moving *settledness one layer deeper* — from a
vocabulary, to a substrate, to a compiled plan, to a trustworthy live view.

| arc | dates | key specs (status) | what it contributed | retired |
|---|---|---|---|---|
| **1 · CLI / output discipline** | 04-02→04-21 | `anneal-spec` (draft); cli-output / progressive-disclosure / query-explain; areas-orient (superseded) | a 14-command CLI over a typed handle graph; agent-budget discipline baked in | → arc 2 |
| **2 · Language-first → master** | 04-30→05-13 | `2026-05-03-language-redesign` (superseded); engine-spike + results (current); **`2026-05-13-corpus-runtime` (MASTER, current)** | collapse 14 commands into one Datalog dialect + prelude + verbs; substrate/adapter/surface split | the whole CLI era |
| **3 · Surface / calibration / retrieval** | 05-16→05-30 | `surface-evolution-framework` (current); v0.14 calibration + v0.15 retrieval (converged); `code-as-corpus-spike` (current) | evidence-based "what earns a place"; sections→spans; *API stability IS a convergence lattice* | the cut audit trio |
| **4 · Compiler / performance** | 06-01→06-05 | `perf-architecture-arc` (draft); `allocation-study` (complete); `pass-contracts` + `plan-ir-reconciliation` (locked); `datalog-compiler-reference-map` | **anneal is a compiler; build it like one** — a real Plan/IR middle-end; ~2.1× faster, byte-identical | interpreted eval |
| **5 · Decomposition** | 06-04→06-07 | `runtime-architecture` (superseded) → **`core-decomposition-plan` (current)** | split the 12k `eval.rs` into a `vm/` backend along locked seams; §12 boundary → a machine gate | the pre-kftp as-built |
| **6 · Trust / currency / navigate** | 06-06→06-09 | `disposition-typed-witnesses` (draft); **`trust-invariant` + `currency` (current)**; `navigate` (draft, first cut shipped) | convergence becomes *retrieval-facing*; oracle-honest authority; currency + lineage + ranked neighbors | — |

**The throughline.** A corpus began as *a typed handle graph whose convergence
states form a graded-type lattice* → became *a programmable Datalog runtime with
substrate decoupled from sources* → was recognized as *a compiler whose plan must
capture meaning once so the executor stays dumb* → and is now *an oracle-honest
projection over facts.* The compounding bet: **settledness is a derived, typed,
machine-checked property** — and each arc pushed that property one layer deeper
(vocabulary → substrate → compiled plan → trustworthy live view).

**Live authoritative set today:** the master (`corpus-runtime`); the engine-gate
evidence (`engine-spike*`); the locked compiler contracts (`pass-contracts`,
`plan-ir-reconciliation`, `ordered-query-output`); the decomposition plan; and
`trust-invariant` + `currency` (current; `navigate` still draft though its first
cut shipped). 13 specs are already `superseded` — the corpus
self-prunes, which is the thesis dogfooding itself.

---

# Part III — The architecture, as built

Everything is a projection over the fact store, in four layers (counts from the
live `schema`):

```
 STORED FACTS  (12)        the generationed fact store (+ snapshot/trail history)
   handle · edge · span · content · meta · concern · config ·
   snapshot · generation · trail · trail_generation · trail_ref
        │  Source::extract → FactBatch → FactStore (current-state, generation swaps)
        │                              → TupleDb (interned, Copy ≤16B)
        ▼
 ENGINE PRIMITIVES (32)    fixed backend-computed relations (Rust-native indexes:
   search · match · read · neighborhood · impact · in/out_degree · cite_count ·   GraphIndex /
   upstream/downstream · freshness · changed_within · flux · active · settled ·    SearchIndex /
   terminal · obligation · discharged · token_estimate · source_of · schema …     Introspection)
        ▼
 PRELUDE DERIVATIONS (148) the convergence vocabulary, as Datalog rules
   currency · anchor · recent · orientation · status · abandoned · potential ·
   entropy · area · namespace · trail · lifecycle · pipeline · parent · …
        ▼  plan() → planned executor (vm/execute,fixpoint) → project
 SURFACE                   the deliberate, agent-facing verbs (see Part V)
   status · context · search · read · handle · schema · describe · eval · init
```

- **The substrate is a real compiler.** One planned, certificate-driven Datalog
  executor (interpreted engine retired); `ir/` (ids, schema, plan) + `vm/`
  (value, store, frame, provenance, view, execute, fixpoint); the analysis-aware
  coordinator lives in `runtime/evaluator.rs`, with the public/runtime types in
  `runtime/eval.rs`. eval.rs went 12.2k → ~9.5k this arc.
- **Primitives are Rust-native, not Ascent.** The 32 fixed primitives dispatch to
  `GraphIndex` / `SearchIndex` (ranking.rs) / introspection — *not* an Ascent
  runtime. (Ascent was engine-spike evidence + an accepted bounded-dependency
  risk, never the shipped primitive backend; see the engine-spike specs.) Engine
  choice stays internal to the backend.
- **Boundaries are machine facts.** `check-arch` (in `just check`) enforces the
  crate-DAG and "vm imports no analysis/AST"; `anneal check` keeps the corpus
  honest.
- **One substrate, many sources.** markdown today via the `Source` trait;
  `code-as-corpus` proved rustdoc JSON works (stability attrs = a lattice).

---

# Part IV — The conceptual language (the heart)

The 148 derived predicates in the live schema are not a flat bag — they are **projections along a
small set of orthogonal dimensions.** Naming these axes *is* the design language,
and the discipline is: **keep them orthogonal, name each precisely, present each
with an honest disposition.** (Conflating two axes is the canonical bug — see §
currency below.)

## The dimensions

| dimension | the question | families | monotone? |
|---|---|---|---|
| **relevance** | does it match my query? | search · match · hit · anchor · ranked | per-query |
| **currency** | has it been displaced? | currency · supersession | **no** (non-monotone) |
| **lifecycle** | draft / operative / retired? | status · active · abandoned · terminal · settled | ~yes |
| **recency** | when authored / changed / observed? | authored_age(freshness) · changed_recently/changed_within · flux · snapshot · git_mtime (raw timestamp; rejected as age/currency oracle) | yes |
| **importance** | how central? | in/out_degree · cite_count · impact · neighborhood | yes |
| **convergence** | is it settling? | potential · entropy · advancing/holding/drifting | trend |
| **structure** | how organized/connected? | edge · parent · **area** · namespace · section · pipeline · trail | yes |
| **obligations** | what is owed? | obligation · discharged/undischarged | yes |

A feature is a **composition of axes**, never a new silo:
- **currency** = the *displacement* axis alone (`current :- not superseded`).
- **lineage** = currency × structure (the `Supersedes` DAG, head-routed).
- **ranked neighbors** = relevance × currency × lifecycle × importance.
- **orientation** = relevance × currency × lifecycle, hub-penalized.

## The disposition law (the trust invariant)

Every surface carries exactly one **disposition**, and presents *only the
authority its oracle earns:*

| disposition | role | oracle | on degenerate input |
|---|---|---|---|
| **GATE** | blocks | clean pass/fail | signal the degeneracy, don't answer |
| **REPORT** | informs | graded / human-judged | — |
| **TREND** | tracks | slope over snapshots | declare "no baseline" |
| **PRE-FLIGHT** | grounds | current-artifact premise | declare the missing premise |

This is `anneal-xy45`. **`check` is the true GATE exemplar** (a broken reference
blocks). **Currency is the first *retrieval-facing* disposition instance:** marked
`Supersedes` is a strong displacement oracle, but its surface behaviour is
*down-rank / annotate / lineage*, **not block** — superseded material stays
reachable by design; unmarked supersession is a REPORT hint, never an asserted
edge; no history is `unknown`, signalled. It is *also anneal's product pitch*: a
machine-checked corpus invariant beats a prose convention.

---

# Part V — The current feature-set, situated

The **orientation / retrieval / trust surface** (the deliberate verbs; `schema` ·
`describe` · `eval` · `init` are infrastructure, `check` · `prime` are support):

| surface | dimensions it composes | what it answers |
|---|---|---|
| `status` | convergence × lifecycle | is the corpus advancing / holding / drifting? |
| `context` | relevance × currency × lifecycle × structure | orient me to a goal — the live hits + their current neighborhood |
| `search` | relevance × currency | find evidence for X, current-aware |
| `read` | — | the bytes, budget-bounded |
| `handle` | structure (`--impact` = DependsOn; `--lineage` = Supersedes × currency) | what is this, what does it touch, how did it evolve |
| `check` *(support)* | the diagnostic dispositions | is it consistent (broken refs = GATE; drift = REPORT/TREND) |

(`lineage` is the `handle --lineage` mode — the `Supersedes` × currency walk to
the current head, sibling of `--impact`, not a standalone verb.)

The recent arc (currency, `--lineage`, currency-ranked neighbors) **landed the
first cuts of the provenance + navigation surfaces** the thesis says matter most
(topical navigation is still deferred — Part VI) — and a 20× retrieval regression
from the compiler arc was caught and fixed, so they are usable at scale. The general "walk edges of kind K" remains a *query-local
recursive rule in the language* (not a verb) — curated verbs exist only where a
walk is both structurally sound and semantically distinct (`--impact`,
`--lineage`).

---

# Part VI — The shape we want

An elegant tool here has a precise meaning, and the system is *close but not
there.* The target:

> **A few orthogonal dimensions, a small verb surface, the language as the
> convenience underneath, every surface oracle-honest, and a corpus that
> converges — including anneal's own.**

Five moves carry us there:

### 1. Make the dimensional map first-class
The ~8 axes are real but *implicit* — discovered, not declared. Name them in the
substrate; assign every predicate-family to one; and **surface the tangles**.
Currency/lifecycle was one untangle (and it caught two bugs); **recency** was the
second (`2026-06-09-recency-axis.md`: authored-age vs change-recency vs
history-movement, with `git_mtime` rejected as an age/currency oracle after the
murail simulation showed it degraded — 87% of files sharing one bulk-commit
timestamp). *Clarifying an axis is what makes the features on it correct and
simple* — the single most reliable lesson of this arc, now proven twice.

### 2. Disposition-type the whole surface
The trust invariant is enforced on currency and `check`, but most of the 148
predicates predate it. Every projection should carry its disposition, so no
surface ever presents more than its oracle earns. This is `xy45` applied
uniformly — folded into the master spec as **CR-D103**, the gate for every new
predicate; the remaining work is typing the existing 148.

### 3. Reduce the vocabulary (find the language)
192 callable relations over a handful of deliberate verbs is more than the value
requires. Point
anneal's *own* discipline at its prelude: which predicates are terminal/abandoned
(unexercised), which axes overlap, which verbs are the deliberate surface vs
speculative accretion. *Default verdict: CUT.* The prelude is itself a
cross-linked corpus that must converge — **anneal annealing its own language.**

### 4. The clustering substrate — the keystone
The corpus edge graph is ~95% `Cites` + index hubs, so *topical* navigation can't
be walked. The missing primitive is **pairwise topic structure** over the graph
— and it unlocks **two** deferred features at once: *unmarked supersession*
(currency's `suspect`: "a newer sibling on the same topic") and *topical
navigate* (`root → intent` trails). Crucially, the `area` family (8 predicates:
`area_of`, `area_frontier`, `area_health`, `area_cross_edges`, …) is **explicit
proto-clustering already** — so this lands as a *reconciliation of `area`*, a
deliberate ninth dimension done right, not another accretion.

### 5. Toward a truth-maintenance corpus
The far end: proofs/gates as first-class vertices and **retraction propagation** —
"a CI failure retracts a `holds` fact." Currency is the first non-monotone
projection; the TMS generalizes it. Build subtractively: each step must *delete a
manual practice*, or it doesn't ship.

### What "elegant" means here
- **Orthogonal** — each axis answers one question; no surface conflates two.
- **Honest** — every result carries its disposition; degenerate input is
  signalled, never papered.
- **Small at the top, rich underneath** — few verbs; the Datalog language is the
  power surface; primitives compose.
- **Self-converging** — the corpus (including the prelude and the specs) is
  itself kept settled by the same machinery it offers others.
- **Source-agnostic** — markdown, code, hosts: one substrate, the same lattice.

---

# Part VII — Roadmap

```
NOW  ── substrate clean & gated · currency + navigate shipped (v0.19) · perf recovered
  │
  ├─ (cheap, framing) xy45 → CR-D in master spec; the dimensional map written down
  ├─ (reduction)      vocabulary/disposition audit — anneal on its own prelude; CUT
  ├─ (keystone)       clustering substrate (reconcile `area`) ──┐
  │                                                              ├─ unmarked currency (suspect)
  │                                                              └─ topical navigate (root→intent)
  ├─ (perf lever)     eygi env redesign (status/context 2.5s→~1.5s)
  ├─ (decomposition)  plan.rs split · Value/DbView relocation · public façade · ir/analyzed re-key
  └─ (horizon)        TMS — proofs/gates as vertices, retraction propagation
```

Order by leverage, not by size: **name the axes and reduce first** (cheap, makes
everything after it simpler), **then the clustering keystone** (unlocks the two
biggest deferred features), with perf/decomposition as parallel hygiene and TMS
as the horizon.

---

# Part VIII — Where the theory bends (honest)

- **"Everything is a projection" is a stance, not a law** — it earns its keep
  only where treating a thing as a derived view buys currency/provenance/
  composability; transient state is the counter-case.
- **The substrate/model split narrows if models get trustworthy enough** that
  self-reported provenance is "good enough" — then anneal's durable edge shrinks
  toward sovereignty + audit. Still real, smaller.
- **Topical navigation may live in the *reasoning*, not the store** — a uniform
  graph may buy less than it looks; the clustering + intent-scoring is where the
  difficulty actually sits (proven once already when naive navigate broke).
- **Evidence is uneven** — strong for the worked surfaces (currency, navigate,
  check, status, validated on murail); thin for the long-tail prelude. The
  reduction pass (§VI.3) is partly an *evidence* pass: exercise or cut.
- **Correctness gates miss perf** — byte-identical differential is necessary, not
  sufficient; the compiler arc passed every diff and still regressed 20×. Gate
  perf explicitly.

---

*This document is the orienting reference; the master spec (`corpus-runtime`)
holds the substrate detail, and the dated arc specs hold the record of how each
layer was learned. Read this to understand the whole; read those to build.*
