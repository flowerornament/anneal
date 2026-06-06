---
status: draft
date: 2026-06-06
authors: [claude]
reviewers: []
relates:
  - .design/2026-06-05-datalog-compiler-reference-map.md   # standing review gate, reference frame
  - .design/2026-05-13-corpus-runtime.md                    # master spec — CR-D home for the trust invariant
  - /Users/morgan/code/murail-dev/GATES.md                  # external: the architecture-witness shape (source)
---

# Disposition-typed witnesses — anneal as architecture-witness consumer and instance — 2026-06-06

A cross-agent transfer from murail-dev (`GATES.md`) shared a tooling shape:
**make a codebase's structural invariants a measured, enforced surface**, with
**disposition** as the governing design axis. anneal relates to that idea twice,
and the second relation is the load-bearing one:

1. **As a consumer** — anneal's own build wants architecture-fitness gates.
2. **As an instance** — anneal's `check`/diagnostics *are* disposition-typed
   witnesses over a corpus. murail already runs `anneal check` as the
   `check-design` **gate** in its `check-all`. anneal's whole thesis — "make a
   corpus's structural invariants a measured, enforced surface" — *is* the
   architecture-witness organizing idea, in the knowledge-corpus domain instead
   of the code domain. anneal is a witness; it should know it, and apply the
   discipline to itself.

## The disposition framework

Every check has exactly one **disposition**, and assigning it wrong is the one
mistake to avoid (a "gate" without a clean oracle becomes false-failure noise
that trains people to ignore it):

| Disposition | Role | Oracle | Cadence |
|---|---|---|---|
| **GATE** | blocks | clean pass/fail | every commit/CI |
| **REPORT** | informs | none (human-judged) | on-demand |
| **TREND** | tracks | slope over time | periodic / ledger |
| **PRE-FLIGHT** | grounds | current-artifact premise witness | before building on a premise |

Maxims that emerged (each maps onto anneal):

1. **Disposition is the design.** A gate needs a clean oracle; else it's a report
   or a trend. (murail rejected `cargo-modules --acyclic` as a gate — false
   cycles — and made it a report.)
2. **A gate is only as honest as its visibility.** `cargo-public-api` renders
   `pub use foo::*` as one opaque line, so a glob leak is invisible to the gate →
   sealed by an ast-grep glob-ban sentinel.
3. **A gate is only as useful as its signal-to-noise.** A 7074-line public-API
   snapshot (mostly compiler noise) → `-sss` deliberate-surface → 2438
   reviewable lines.
4. **Tools default to noise; the useful artifact is the deliberate-altitude one.**
5. **The witness can find its own blindspot** — a gate *finding* exposed the
   gate's own gap; fixing it made the witness more honest.
6. **Triangulation validates** — independent instruments converging on one
   hotspot rediscover what was already suspected.

The spine: **make the durable rule a machine fact.** Prose conventions rot —
teams distant from the theory silently re-introduce bad decompositions; a
machine-checked predicate cannot. This is *also anneal's product pitch*: a
machine-checked corpus invariant beats a prose convention.

## Level A — anneal's build tooling (consumer)

**Have (GATES, clean oracle, in `just check`):** `cargo deny check bans licenses
sources` + `cargo machete`. These enforce on every commit (pre-commit hook).
Working as designed.

**Gap — the crate-DAG is prose, not a machine fact.** CLAUDE.md declares
`anneal-lang → anneal-core → {anneal-md, anneal-cli, anneal-mcp}` and the
substrate-only rule for `anneal-core` — but nothing enforces it. As the DAG
evolves (planned `anneal-query` extraction, eventual `eval.rs` split), prose
rots. **Adopt a `check-arch` crate-DAG GATE** (laws-as-predicates over the
dependency graph; a small xtask or equivalent). This is the highest-value
adoption — see bd task below.

**Disposition assignments for the structural tools (per `anneal-orpd`):**
- crate-DAG → **GATE** (clean oracle).
- `cargo-modules` orphans/structure → **REPORT** (no clean oracle; false cycles).
- `cargo-public-api` → **GATE only with `-sss` + an ast-grep glob-ban** (maxims
  2+3); anneal's crate surfaces are small, so this is lower priority than the
  crate-DAG gate.
- `cargo deny check advisories` (full `just audit`) → **TREND/periodic**, not a
  per-commit gate (needs network; appropriate at release/periodic cadence).

## Level B — anneal's output surfaces are disposition-typed witnesses (instance)

anneal presents `check` results, convergence status, scores, **and retrieval
hits (`context`/`search`)** with **uniform confidence**. That is the disposition
gap, and it is exactly what the **trust invariant** (`anneal-xy45`) is about:
*never return a confident answer over degenerate input.* Restated in this frame:
**a surface may only present as a GATE where it has a clean oracle on
non-degenerate input; otherwise it must signal its disposition honestly, not
silently succeed.** Two surfaces, two missing oracles: diagnostics lack a
*clean-oracle / degenerate-input* guard; retrieval lacks a *currency* one.

### B1 — diagnostics: the clean-oracle gap

Proposed dispositions for anneal's own diagnostics:

| Diagnostic / surface | Disposition | Oracle |
|---|---|---|
| `broken_reference` (E001) | **GATE** | a ref resolves or it doesn't |
| `spec_code_drift`, `confidence_gap` | **REPORT / TREND** | graded, not clean pass/fail |
| convergence status (advancing/holding/drifting) | **TREND** | slope over snapshots |
| `status` aggregate over a degenerate corpus | **PRE-FLIGHT honesty** | must declare the premise (empty/unclassified/no-history) rather than answer confidently |

**Stakes (concrete):** a false-confident anneal diagnostic *false-fails a
consumer's gate* — murail's `check-design` runs `anneal check`. The open
`anneal-veiw` bug (`[[wikilink]]`/`qmd://` external refs misclassified as corpus
handles → false E001) is **maxim 2** exactly: the broken-ref gate is blind to a
ref class it can't classify, so it emits false failures into a downstream gate.
Disposition-honesty makes this a first-class correctness concern, not a polish nit.

### B2 — retrieval: currency is the missing oracle (murail-claude, 2026-06-06)

Independent feedback from a consumer agent surfaced the *same* gap on a different
surface. `context`/`search` rank by **relevance**, and a relevance match cannot
distinguish *the current authoritative spec* from *a superseded historical doc
that happens to match*. In a corpus rewritten weekly, pure-relevance retrieval
surfaces the whole history with flat authority — and the failure is **silent: a
confident, relevant, wrong (stale) hit.** The agent reasoned from a 4-month-old
`status:active` spec as if current and burned real time before catching it.

**Currency is the missing oracle** for retrieval: relevance ≠ currency. anneal's
recency machinery (`recent_frontier`, `ranked_anchor`, status `search_boost`) is
good — *better than qmd's none* — but it lives in **opt-in** verbs; the obvious
move (`context`) does not steer there. The currency signal must move into the
**default** retrieval path:

- **Per-hit legibility** — every `context`/`search` hit shows status + recency,
  and flags "newer docs exist on this topic" when a topic-cluster has a clearly
  newer member. A stale `status:active` doc must not look identical to the current one.
- **Default steer** — fold a recency/status boost into `context` default ranking
  (make the existing `search_boost` default-on / stronger); when hits span a wide
  date range / mixed statuses, hint to also run `recent_frontier`/`ranked_anchor`.
- **Supersession detection** — flag/down-rank an older doc when a newer
  authoritative sibling exists on the same topic, even if the old one was never
  formally marked `superseded` (the trap: the stale doc was `status:active`).

This is the trust invariant on the retrieval surface: *don't present a stale hit
with a current hit's confidence.*

## Consequences

- **Fold into the master spec:** the trust invariant (`xy45`) becomes a `CR-D`
  decision stated in disposition terms — surfaces are GATE/REPORT/TREND, a gate
  requires a clean oracle on non-degenerate input, degenerate input is signalled
  not answered. This is the design gate for every new predicate/verb.
- **`check-arch` crate-DAG gate** — make the module-boundary prose a machine fact.
- **`anneal-veiw`** is re-weighted: it's a gate-visibility blindspot affecting a
  consumer's CI, not cosmetic.
- **Retrieval currency legibility** — move the currency signal into the *default*
  `context`/`search` path (per-hit status/recency, recency-boost default-on,
  newer-sibling/supersession detection). The gap is silent and consumer-facing.
- The disposition taxonomy is the standing lens across `anneal-orpd` (build
  tooling), `anneal-xy45` (product diagnostics), and retrieval currency — three
  surfaces, one principle: present no result with more authority than its oracle
  earns.
