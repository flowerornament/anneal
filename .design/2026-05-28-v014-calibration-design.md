---
status: proposed
updated: 2026-05-28
author: claude (post-v0.13.1 cold-agent simulation + v0.14 proposal synthesis)
supersedes: .design/2026-05-28-v014-shape-proposal.md (folds it forward, does not replace)
description: >
  v0.14 design as the "calibration release." Folds the v0.14 vocabulary
  slice with everything cold-agent simulation surfaced on v0.13.1 plus
  the substrate Phase 0 design conversation. Includes simulated future
  shape so we can confirm the experience before implementation begins.
---

# v0.14 — Calibration Release Design

## Narrative arc

```
v0.13.0: "anneal becomes the language it has always claimed to be."
         (simplification — 23 visible → 9 visible + 2 hidden)

v0.13.1: "anneal predicates match their describe-card promises."
         (bug fix — area_health, pipeline_stall, missing_frontmatter_file, context --hits)

v0.14.0: "anneal calibrates the convergence signal."
         (the vocabulary completes, the magic words deepen,
          the signal-to-noise ratio improves, and the substrate
          gets its first round of accidental-complexity removal)
```

The arc keeps building: shape → correctness → signal quality. Each
release is one coherent thing the previous release made possible.

## What cold-agent simulation on v0.13.1 surfaced

I exercised v0.13.1 against `/path/to/large-corpus/.design` as an agent
arriving cold. Worked through arrival, frontier-pick, drill-into,
verify, and error-recovery flows. The friction below is real and
witnessed, not speculative.

### F1. Signal-to-noise: freshness_decay dominates the work pool

```
large-corpus frontier top 6 (energy=3, all):
  language/elaboration-convergence-v2.md           stale_dep        (real)
  grc-elaboration-study/.../cross-cutting-analysis  freshness_decay (noise)
  grc-elaboration-study/.../expression-tree-...     freshness_decay (noise)
  grc-elaboration-study/.../grc-program-01-...      freshness_decay (noise)
  grc-elaboration-study/.../grc-program-02-...      freshness_decay (noise)
  grc-elaboration-study/.../grc-program-03-...      freshness_decay (noise)
```

`freshness_decay` weight is 2. There are many old files. The actual
work signals (`undischarged`=5, `broken_ref`=4, `stale_dep`=3,
`confidence_gap`=3) get drowned in the volume.

The fix is calibration:
- Raise the bar: freshness_decay should fire only on early-lifecycle
  files (raw/draft), not research/exploratory/plan/current.
- OR drop weight to 1 (matches `missing_meta` noise tier).
- OR scale weight by status (freshness_decay matters more for draft,
  less for research).

Or all three. Calibration is the v0.14 product question.

### F2. Section handles flood predicates that join *handle

```
large-corpus: ? changed_within(h, 7).             -> 165 rows
large-corpus: ? changed_within(h, 7), kind=file.  -> ~10 rows

957 section handles for 28 files in .design.
13,076 section handles for 429 files in large-corpus.

Every file appears once; every section in that file appears too.
```

`changed_within` (and other temporal predicates) join `*handle.file`
to git mtime. Sections have a file field, so they inherit. The result
is 30:1 noise.

Two fixes:
- Phase 0 result: if sections fold to spans, the noise vanishes
  structurally.
- Interim teaching: describe `changed_within` should recommend the
  `kind: "file"` filter for "what changed?" questions.

### F3. Describe-card depth is uneven on magic-word predicates

```
RICH cards (Common joins + Example + See also + Rule source):
  frontier, blocker, broken_reference, pipeline_stall, etc.

THIN cards (Signature + Requires + See also + Example only):
  entropy, potential, work_candidate, advancing, settled, active,
  terminal, hub, orphan, freshness, obligation, ...
```

The most important magic-word predicates have the THINNEST cards.
This is backwards. The framework (CR-D102 magic-word inventory) would
flag this on its first annual pass.

### F4. No describe target for the agent loop

The canonical agent loop (CR-D29) lives in `anneal help agent`. There
is no `describe convergence_loop` or `describe agent_loop` for an
agent that's mid-task and wants to remind itself of the pattern. The
agent has to reach for `help agent` and read a long briefing to
recover one pattern.

### F5. handle --impact display labeling is confusing or wrong

```
large-corpus: handle "references/README.md" --impact:

Outgoing (13 edges)         ← correct
Incoming (1 edge)           ← correct
Impact
  Direct (0) (none)         ← confusing: the 1 incoming IS a direct impact
  Indirect (0)              ← correct (no transitive)
```

Either the section labels are misleading (Direct shows depth=2+ not
depth=1) or there's a real off-by-one. Agent reading this can't tell
if `synthesis/2026-03-16-v14-protocol-synthesis.md` is a "Direct
impact" or not.

### F6. Project-level CLAUDE.md teaches retired commands

```
CLAUDE.md:
  `anneal status --json --compact`    -> --compact does not exist
  `anneal map --around=<handle>`      -> map was retired in hmpr.4
```

CLAUDE.md is the binding project context. It's teaching commands that
fail. Cold-agent sessions starting with CLAUDE.md context will fumble.
Same pattern as the --help triad lie in v0.13.0, scoped one layer out.

### F7. Unknown predicate error has no suggestion

```
$ anneal -e '? unsettled(h).'
error: query failed static analysis: cli-query:1:3: unknown predicate 'unsettled/1'
```

No "did you mean potential_subject(h)? entropy(h, source)?" Agents
that typo or guess a synonym get a flat error.

### F8. blocker(h, energy, source) over-fires (carried from v0.14 proposal)

```
large-corpus: ? blocker(h, e, s).  -> 62 rows for 32 blocked handles.
```

By design — handle × entropy_source. Status output dedups via
`primary_entropy`. Raw eval gives multi-row-per-handle. Teaching gap.

### F9-F11. Carried v0.14 proposal items
- holding/drifting predicates missing (taught in --help)
- snapshot/generation/trail conflation in describe runtime
- *concern stub describe card

### F12. Handle-kind accidental complexity (codex's Phase 0 evidence)

957:28 sections:files in `.design`. Every section handle has zero
in/out edges, line=0, empty summary. This is substrate noise that
shows up in every join.

## v0.14.0 scope (the calibration release)

Ten items, organized by theme. Each justified against CR-D102.

### THEME 1: Convergence vocabulary completion

**A. Convergence flow triad.** Ship the four predicates project owner and
   codex converged on:
   ```dl
   advancing(h)         already exists
   holding(h)           active + potential + status unchanged @ snapshot:last
   regressed(h)         status moved backwards in pipeline @ snapshot:last
   re_opened(h)         active now, terminal @ snapshot:last
   drifting(h)          regressed OR re_opened
   convergence_flow(h, direction)  union for one-shape querying
   ```
   ~50 lines of prelude rules + 5 describe cards + 1 test.

**B. History concepts in describe runtime.** Three-row subsection
   disambiguating snapshot (graph state over time), generation
   (source data epoch), trail (per-query provenance). Pure docs.

### THEME 2: Signal calibration

**C. Calibration round on potential_weight + freshness threshold.**
   Two sub-decisions, each needs evidence:
   - freshness_decay: lower weight (2 → 1), or filter by status, or
     both?
   - Default freshness threshold: 60 days is short for slow-moving
     reference corpora; long for active development. Should it scale
     with corpus median age?
   - This is design + measurement: simulate on `.design` and large-corpus
     with different weights, pick what produces a usable signal.
   - Lands as updated default weights in `convergence.dl` + describe
     teaching for project owners on how to tune in `anneal.dl`.

**D. describe blocker teaches primary_entropy.** One paragraph + one
   join example in the existing describe card. Solves F8 without
   adding surface.

**E. describe changed_within teaches kind=file filter.** Same
   pattern as D. One paragraph + join example. Solves F2 interim
   pending Phase 0.

### THEME 3: Magic-word depth

**F. Deepen describe cards for the core magic-word predicates.**
   Take the rich-card pattern from `frontier`/`blocker` and apply to:
   - `entropy(h, source)` — show the 7 sources, weights, and how
     they're combined into potential. Relationship to diagnostics.
   - `potential(h, energy)` — show energy = sum of weights. Pair with
     `primary_entropy` for one-row-per-handle.
   - `work_candidate(h, energy)` — relationship to frontier (capped
     projection).
   - `advancing(h)` — snapshot:last semantics, lattice-relative.
   - `settled(h)`, `terminal(h)`, `active(h)` — lifecycle primitives.
   - `freshness(h, days)`, `flux(h, days, delta)` — temporal
     primitives.
   - `hub(h, degree)`, `orphan(h)` — graph-shape predicates.
   - `obligation(h)`, `discharged(h)`, `undischarged(h)` — obligation
     lifecycle.

   ~12 thin cards become ~12 rich cards. Pure docs. Big agent-
   ergonomics win.

**G. Add describe target for the convergence loop.** New runtime
   topic: `describe convergence_loop` (or `describe agent_loop`).
   Names the 5-step pattern with concrete queries. Eliminates the
   "needs to re-read help agent mid-task" friction.

### THEME 4: CLI seam polish

**H. handle --impact labeling.** Either rename "Direct (N)" to
   "Direct (depth=1, N transitive)" or fix the off-by-one. Match
   what `impact(h, x, depth)` returns. Acceptance: agent reading
   the output can predict what query reproduces it.

**I. Unknown-predicate error suggestion.** Edit distance match
   against schema predicates; suggest up to 3. Solves F7.

### THEME 5: Project-level documentation sync

**J. Sync CLAUDE.md + skills/anneal/SKILL.md against v0.14 surface.**
   Audit every command/flag mention against the real CLI. Remove
   --compact, map, anything else retired. Replace with current
   teaching. Solves F6.

### Substrate arc (Phase 0 lives parallel to v0.14.0 implementation)

**E (Phase 0). Handle-kind consolidation design.**
- Lean: 5 → 4 (fold section, keep version).
- Codex's 6-section design doc in `bd anneal-gbuz`.
- Independent reviewers: claude + codex + large-corpus reviewer.
- Outcome: if 5 → 4 with no identity break, v0.14.0 absorbs.
  If 5 → 3 with snapshot migration, splits to v0.15 or v1.0.

## What v0.14.0 will FEEL like (simulation before implementation)

### Cold-agent arrival on large-corpus under v0.14

```
$ anneal --root /path/to/large-corpus/.design status
Status
Convergence  broken=1  blocked=8  work=22  advancing=0  holding=14  drifting=0

Broken
 1. references/README.md  score=100  E001

Blocked
 1. references/README.md                              score=6  broken_ref
 2. language/elaboration-convergence-v2.md            score=3  stale_dep
 3. synthesis/2026-05-18-monoidal-computer-reframing  score=3  stale_dep
 4. ... 5 more

Other work
 1. compiler/2026-03-16-monoidal-core-design.md       score=2  potential
 2. formal-model/proofs/WHAT-IS-PROVEN.md             score=2  potential
 ... 20 more

(holding 14 handles unchanged + carrying potential since last snapshot.)
```

Change vs v0.13.1:
- Convergence header gains `holding=` and `drifting=` (Theme 1.A).
- Blocked dropped from 37 → 8 because freshness_decay no longer
  fires on research/exploratory files (Theme 2.C).
- New trailing note hints at `holding` for the agent's next-deeper
  question.

### Drilling into a magic word

```
$ anneal describe entropy
Return one reason a handle looks unsettled — the seven entropy
sources that feed potential.

Kind: derived predicate.
Signature: entropy(h, source).
Relationship: Per-handle, per-source enumeration of unsettled-work
  signals. Sum into potential via potential_weight(source, weight);
  pair with primary_entropy(h, source) for one row per handle.

Sources (weight, when it fires):
  undischarged    (5) obligation handle, not discharged, not terminal
  broken_ref      (4) E001 diagnostic — *edge.to has no *handle
  stale_dep       (3) active handle DependsOn a terminal handle
  confidence_gap  (3) handle at pipeline stage > target + 1
  freshness_decay (1) early-lifecycle file (raw|draft) > N days old
  missing_meta    (1) file lacks status frontmatter
  orphan_label    (1) label with no incoming citations

Common joins:
- entropy(h, source), *handle{id: h, summary: summary}
    -> Output: h, source, summary
- entropy(h, source), source = "broken_ref",
    broken_reference(h, target, file, line)
    -> Output: h, target, file, line
- potential(h, energy), entropy(h, source), source = "stale_dep"
    -> filter potential by a specific signal

See also: potential, primary_entropy, work_candidate, diagnostic,
  potential_weight, entropy_priority.
Tuning: see `describe potential_weight` and project anneal.dl to
  retune for your corpus.

Rule source: crates/anneal-core/src/prelude/convergence.dl:43.
```

Change vs v0.13.1:
- Rich card with Sources table, Common joins, Tuning hint.
- Magic-word density: `unsettled`, `potential`, `obligation`, `signal`
  all reinforced in one card.

### Asking the meta question

```
$ anneal describe convergence_loop
The canonical agent loop for making a corpus settle.

Kind: runtime topic.

Five steps:
  1. anneal status
     See the convergence landscape: broken, blocked, work,
     advancing, holding, drifting.

  2. anneal -e '? frontier(h, energy), primary_entropy(h, source).'
     Pick where to work, with one reason each.

  3. anneal handle <H> --impact
     Inspect local context and reverse-dependency blast radius.

  4. (do the work — edit corpus files)

  5. anneal status
     Confirm potential dissipated; the snapshot autocaptures so
     `at("snapshot:last")` becomes the reference for advancing(h).

For arrival on an unfamiliar corpus, prepend `anneal describe runtime`
and `anneal -e '? sources(name, recognizes, capabilities, doc).'`.
For multi-session handoff, query `*trail` after step 5.

See also: status, context, handle, frontier, primary_entropy,
  advancing, holding, drifting, settled.
```

Change vs v0.13.1:
- New describe target for the agent loop.
- The 5 steps are exact eval forms the agent can run.

### History concepts disambiguated

```
$ anneal describe runtime
... (existing content) ...

History concepts:
  Snapshot    Graph state at a point in time.
              Written by anneal status (autosnap).
              Queryable via at("snapshot:last") { ... }.
              Use to answer: "what did the corpus look like before?"

  Generation  Source-data epoch.
              Stored in *generation{corpus, source, current}.
              Incremented when adapter re-extracts.
              Use to answer: "has the source data changed since
              I last looked?"

  Trail       Per-query provenance.
              Stored in *trail{session_id, step, redacted_expr, ...}.
              Written when --explain or session capture is on.
              Use to answer: "why did this query return this row?"
```

Change vs v0.13.1:
- The three "history" concepts are no longer conflated. Agents can
  distinguish "what did the corpus look like a week ago" from
  "did the source data change."

### Tuning hint surfaced

```
$ anneal describe potential_weight
Return the default score weight for each kind of unsettled-work
signal.

Kind: derived predicate.
Signature: potential_weight(source, weight).
Relationship: Per-source weights feeding potential(h, energy).
Default weights:
  undischarged    5    (highest — obligations matter most)
  broken_ref      4    (next — broken references are correctness bugs)
  stale_dep       3    (real work — pre-terminal pointing at terminal)
  confidence_gap  3    (real work — premature claim)
  freshness_decay 1    (lowest — early-lifecycle staleness only)
  missing_meta    1    (lowest — frontmatter hygiene)
  orphan_label    1    (lowest — referenceless label)

Tuning: override in project anneal.dl with `config potential_weight`.
  Example:
    config potential_weight {
      undischarged 8        # weight obligations higher in this corpus
      freshness_decay 0     # disable freshness signal entirely
    }
  Higher weights = stronger pull on the work pool.

See also: entropy, potential, primary_entropy, entropy_priority.
```

Change vs v0.13.1:
- Defaults shown inline. Tuning syntax shown inline. No need to
  read prelude source.

### The agent loops, the corpus settles

```
$ anneal -e '? holding(h), *handle{id: h, summary: summary}.' --limit 3
{"h":"compiler/2026-03-16-monoidal-core-design.md", "summary":"..."}
{"h":"formal-model/proofs/WHAT-IS-PROVEN.md", "summary":"..."}
{"h":"implementation/2026-03-23-architecture-plan.md", "summary":"..."}

$ anneal -e '? drifting(h).'
(0 rows)

$ anneal -e '? convergence_flow(h, dir).' --limit 5
{"dir":"holding", "h":"compiler/2026-03-16-monoidal-core-design.md"}
{"dir":"holding", "h":"formal-model/proofs/WHAT-IS-PROVEN.md"}
{"dir":"holding", "h":"implementation/2026-03-23-architecture-plan.md"}
{"dir":"holding", "h":"language/2026-03-19-cross-cutting-analysis.md"}
{"dir":"advancing", "h":"references/README.md"}
```

The triad ships honestly. Status header shows the breakdown.
convergence_flow gives the queryable shape.

## What v0.14.0 will NOT include

```
DEFERRED to Phase 0 design (parallel work):
  Handle-kind consolidation implementation
  (lean 5 → 4; bd anneal-gbuz; outcome decides v0.14 vs v0.15)

DEFERRED to v0.15+ (out of scope here):
  - Multi-corpus federation
  - Adapters beyond markdown (anneal-mdx, anneal-code, anneal-host)
  - MCP server polish beyond status quo
  - learn verb (schema + describe collapse)
  - convergence(...) system aggregate
  - status output redesign (severity-aware sections)
  - "+N more like this" dedup in status
```

## Estimated cost

| Theme | Items | Cost |
|---|---|---|
| 1. Vocabulary completion | A, B | 1 day |
| 2. Signal calibration | C, D, E | 1 day (with measurement) |
| 3. Magic-word depth | F, G | 1 day |
| 4. CLI seam polish | H, I | 0.5 day |
| 5. Project doc sync | J | 0.5 day |
| **v0.14.0 total** | | **~4 days** |

Phase 0 substrate design runs in parallel; outcome decides whether
the substrate work ships as v0.14.1 (no breaking) or v0.15 (breaking).

## What needs project owner's confirmation before implementation

1. **Calibration call (Theme 2.C):** are we OK lowering freshness_decay
   weight to 1, OR filtering it to early-lifecycle only, OR both?
   This is the only place in v0.14 where we're changing existing
   user-facing default behavior. (A,B,F,G,J are additive; D,E,H,I
   are polish.)

2. **convergence_flow exhaustiveness language:** per codex, the
   describe card should explicitly say "settled handles are outside
   the flow by design." Confirm we like that framing or want a
   `direction = "settled"` row in convergence_flow.

3. **describe target name:** `describe convergence_loop` vs
   `describe agent_loop` vs fold the loop teaching into
   `describe convergence`. Which name?

4. **Magic-word audit scope (Theme 3.F):** the proposed 12 cards is
   a starting set. Should we audit the full magic-word inventory
   first (per CR-D102 annual cadence) and deepen all of them at
   once, or take the 12 most-friction-laden first?

5. **CLI seam fix shape (Theme 4.H):** rename labels or fix
   off-by-one? Confirm which.

6. **Phase 0 timing:** does Phase 0 design get a few days NOW
   parallel to v0.14.0 implementation, OR does v0.14.0 ship first
   and Phase 0 follows? Codex leaned parallel; I agree but want
   explicit confirmation.

## After project owner confirms

If confirmed: directive to codex with locked-in scope. v0.14.0
ships in ~4 days. Phase 0 ships as a `.design/` doc whose outcome
determines the v0.14.1 vs v0.15 fork.
