---
status: converged
updated: 2026-05-28
author: claude (post-v0.13.1 cold-agent simulation + v0.14 proposal synthesis)
reviewer: codex (independent review converged 2026-05-28)
supersedes: 2026-05-28-v014-shape-proposal.md
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
is no canonical describe surface that gives an agent mid-task the
5-step pattern. The agent has to reach for `help agent` and read a
long briefing to recover one pattern. Solution: expand
`describe convergence` to multi-section (meta-process + the act +
vocabulary + tuning) rather than adding a new describe target.

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

**A. Flow triad.** Ship the five predicates project owner and codex
   converged on (post-review normalization):
   ```dl
   advancing(h)         already exists
   holding(h)           active + potential + status unchanged @ snapshot:last
   regressed(h)         status moved backwards in pipeline @ snapshot:last
   re_opened(h)         active now, terminal @ snapshot:last
   drifting(h)          regressed OR re_opened
   flow(h, direction)   direction ∈ {"advancing", "holding", "drifting"}
                        union over the coarse triad — re_opened and
                        regressed are LEAF explanations under drifting,
                        NOT extra flow directions (avoids double-count)
                        settled handles outside flow by design
   ```
   ~50 lines of prelude rules + 5 describe cards + 1 test.

**B. History concepts in describe runtime.** Three-row subsection
   disambiguating snapshot (graph state over time), generation
   (source data epoch), trail (per-query provenance). Pure docs.

### THEME 2: Signal calibration

**C. Calibration: lower freshness_decay weight 2 → 1.**
   Weight-only change (status-gating and adaptive threshold deferred
   pending more corpus evidence). Codex measured on large-corpus:
   handles with energy ≥ 3 drop **37 → 3** with weight-only. `.design`
   stays at 5 → 5 (no flood there). Weight-only sharpens the work
   pool 12× on large-corpus without a second semantic change.

   Lands as:
   - Updated `potential_weight("freshness_decay", 1)` in `convergence.dl`
   - Real config override path (see C2 below)
   - CHANGELOG Behavior Change section explicit about the default
   - describe potential_weight teaches the override syntax inline

**C2. Real config override path for potential_weight.** Currently
   `config potential_weight { ... }` is not a supported declaration.
   Naively adding project-level `potential_weight(...)` rows would
   double-count in potential's Sum aggregate. v0.14 must either:
   - Implement a real override (config schema + `effective_potential_weight(source, weight)`
     predicate that potential's Sum aggregates over), OR
   - Strip the override teaching from describe potential_weight.

   **Implementing the schema** is the locked path (per project owner).
   Implementation cost ~0.5 day. Without this, the calibration story
   is half-shipped (defaults change, tuning unmentioned) and the
   framework's "teaching messages must not lie" rule fires.

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
   `describe convergence` becomes a multi-section card: the
   meta-process (existing line), then The Act (5-step canonical
   loop with concrete queries), then Vocabulary (entropy → potential
   → frontier → flow), then Tuning (potential_weight override).
   No new describe target; the existing convergence topic absorbs
   the agent-loop teaching.

### THEME 4: CLI seam polish

**H. handle --impact: align two surfaces (real semantic mismatch).**
   Codex verified on large-corpus: `handle references/README.md --impact`
   renders "Incoming (1) Cites" but "Impact Direct (0)". The eval
   primitive `? impact("references/README.md", x, d).` returns the
   same citing handle at depth=1. Root cause: CLI uses
   `DEFAULT_IMPACT_TRAVERSE = {DependsOn, Supersedes, Verifies}` +
   config; eval `impact()` traverses ALL graph edges in core. The
   two canonical surfaces disagree on what "impact" means.

   Fix shape: either make eval `impact()` honor the same configured
   traverse set, OR make `handle --impact` use the core `impact`
   primitive. Then label CLI's section appropriately (e.g.,
   "Impact (configured reverse traversal)" if the set stays
   configurable). Acceptance: `handle --impact` Direct rows = the
   `impact("H", _, 1)` row set, not just a label change.

**I. Unknown-predicate error: arity-aware suggestion.** Keep the
   existing stored-relation-prefix special case as highest priority.
   Then up to 3 schema candidates by edit distance, preferably
   arity-aware (`potental/1` → `potential/2` only if arity matches
   or is close). For semantic misses where edit distance fails
   (`unsettled(h)`), route the recovery toward `describe convergence`
   / `schema` rather than fake confidence in a wrong synonym.

### THEME 5: Project-level documentation sync

**J. Sync project-level docs against v0.14 surface.** Codex's
   review expanded the doc-sync scope beyond CLAUDE.md and SKILL.md:
   - `CLAUDE.md` — `--compact`, `map`, `find`, `get`, `impact`
     (retired commands).
   - `AGENTS.md` — same stale guidance; injected into codex context
     (high impact on cold-codex sessions).
   - `README.md` — project-verb examples must be copy-runnable against
     `.design`.
   - `skills/anneal/SKILL.md` — verify against v0.14 surface.
   - Top-level `--help`, `help eval`, `describe runtime` — already
     synced through hmpr.4 + 38a609f but re-verify.
   - Historical `.design/` docs: leave alone (they're historical
     record, not authoritative teaching).

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
Convergence  broken=1  blocked=3  open=22  advancing=0  holding=~  drifting=0

Broken
 1. references/README.md  score=100  E001

Blocked
 1. references/README.md                              score=5  broken_ref
 2. language/elaboration-convergence-v2.md            score=3  stale_dep
 3. synthesis/2026-05-18-monoidal-computer-reframing  score=3  stale_dep

Other work
 1. (freshness_decay tier, weight=1 → many handles, dropped from
    foreground listing into a tier-by-tier summary)
 ... showing primary-entropy-only for clarity

(holding handles count populated once snapshot history accumulates.)
```

Codex's measured weight-only on large-corpus confirms blocked drops from
37 → 3 in the energy ≥ 3 tier. Real signals dominate again.

Change vs v0.13.1:
- Convergence header gains `holding=` and `drifting=` (Theme 1.A).
- Blocked drops sharply (codex measured 37 → 3 handles with
  energy ≥ 3 on large-corpus) because freshness_decay weight goes from
  2 → 1, falling into the noise tier alongside missing_meta
  (Theme 2.C). Real signals (stale_dep, broken_ref, undischarged)
  dominate again.
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
  freshness_decay (1) active file > N days old
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
$ anneal describe convergence
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
  freshness_decay 1    (lowest — active-file age signal)
  missing_meta    1    (lowest — frontmatter hygiene)
  orphan_label    1    (lowest — referenceless label)

	Tuning: override in project anneal.dl with `config potential_weight`.
	  Example:
	    config potential_weight {
	      undischarged(8).      # weight obligations higher in this corpus
	      freshness_decay(0).   # disable freshness signal entirely
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

$ anneal -e '? flow(h, dir).' --limit 5
{"dir":"holding", "h":"compiler/2026-03-16-monoidal-core-design.md"}
{"dir":"holding", "h":"formal-model/proofs/WHAT-IS-PROVEN.md"}
{"dir":"holding", "h":"implementation/2026-03-23-architecture-plan.md"}
{"dir":"holding", "h":"language/2026-03-19-cross-cutting-analysis.md"}
{"dir":"advancing", "h":"references/README.md"}
```

The triad ships honestly. Status header shows the breakdown.
flow gives the queryable shape over the coarse triad.

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

## Locked decisions (post-project owner + post-codex convergence)

1. **Calibration:** weight-only `freshness_decay 2 → 1`. Status-
   gating and adaptive threshold deferred. Real config override path
   (`config potential_weight { ... }` schema +
   `effective_potential_weight` predicate) lands in Theme 2.C2 so
   tuning teaching is honest.
2. **flow exhaustiveness:** describe card explicitly states "settled
   handles are outside flow by design." No `direction = "settled"`
   row.
3. **describe target:** existing `describe convergence` expands to
   multi-section card (meta + The Act + Vocabulary + Tuning). No new
   describe target.
4. **Magic-word audit:** ship the 14-card slice in v0.14, single
   template (~25-40 lines per card, metaphor-first opener, every
   Example/Common join includes Output columns). Full CR-D102 annual
   inventory still owed but doesn't block v0.14.
5. **handle --impact:** align the two surfaces, not just relabel.
   Acceptance = `handle --impact` Direct rows match
   `impact(H, _, 1)` rows.
6. **Unknown-predicate suggestion:** stored-relation-prefix priority,
   then up to 3 arity-aware schema candidates, semantic miss routes
   to `describe convergence` / `schema`.
7. **Phase 0 timing:** parallel design doc, implementation deferred
   pending substrate evidence. Phase 0 does not block v0.14
   unless it proves no-break and tiny.
8. **work_candidate:** deprecate for v0.14, retire in v0.15. Do not
   collapse immediately; v0.13 taught it in README/SKILL.
9. **Doc sync scope expanded:** CLAUDE.md, AGENTS.md, README.md
   (copy-runnable project verb examples), skills/anneal/SKILL.md, plus
   top-level help / help eval / describe runtime re-verification.

## After this doc lands

Directive to codex via tmux-bridge with this locked scope.
v0.14.0 ships in ~4 days (now ~4.5 including config override
implementation). Phase 0 ships as
`.design/2026-05-28-phase0-handle-kind-consolidation.md` whose
outcome determines the v0.14.1 vs v0.15 fork.

## Codex convergence (2026-05-28)

Independent review of the calibration design after project owner's lock-in
on vocabulary + scope. I converge on v0.14 as the calibration
release, with three implementation blockers fixed above before
green-lighting codex:

**1. Vocabulary normalization.** The committed doc carried stale
`convergence_flow` and `describe convergence_loop` references after
project owner locked `flow(h, direction)` and expanded
`describe convergence`. Patched throughout. Also confirmed: `flow`
direction values are the coarse triad only —
`{"advancing", "holding", "drifting"}` — with `regressed(h)` and
`re_opened(h)` as leaf explanations under `drifting(h)`, NOT extra
flow directions (avoids double-count when agents query `flow`).

**2. Calibration measured: weight-only is enough.** I measured
freshness_decay 2 → 1 on large-corpus directly: handles with energy ≥ 3
drop 37 → 3. `.design` stays 5 → 5 (no flood there). Weight-only
already sharpens the work pool 12×. The plan text/simulation said
37 → 8 with status-gating; that was a different semantic change
that wasn't project owner-approved. Recommendation honored: weight-only
ships, status-gating deferred.

**3. Override teaching honesty.** `config potential_weight { ... }`
is not a supported config declaration. Naive project-level
`potential_weight(...)` rows would double-count in potential's Sum
aggregate. If `describe potential_weight` teaches override syntax
without implementation, that's a "teaching messages must not lie"
violation. project owner locked: implement the real override path (config
schema + `effective_potential_weight` predicate that potential's
Sum aggregates over). Adds ~0.5 day to v0.14 cost. Tuning becomes
a first-class story.

**handle --impact** is a real semantic mismatch, not a label bug.
CLI uses `DEFAULT_IMPACT_TRAVERSE = {DependsOn, Supersedes,
Verifies}` + config; eval `impact()` traverses all graph edges in
core. Two canonical surfaces disagree on what "impact" means. Fix
is align the two, then re-label the CLI section. Acceptance =
`handle --impact` Direct rows = `impact("H", _, 1)` row set.

**Unknown-predicate suggestion** keeps the existing stored-relation-
prefix special case as highest priority. Up to 3 arity-aware schema
candidates by edit distance. Semantic misses (`unsettled(h)` →
where would you route?) reroute to `describe convergence` / `schema`
rather than fake confidence in a wrong synonym.

**Phase 0 sequencing.** Parallel design doc is the right shape.
Substrate identity/migration can easily dominate the release if
implementation gets bundled prematurely. Keep v0.14 vocabulary/
calibration independent of substrate work unless Phase 0 explicitly
proves a safe, no-migration fold.

**More stale teaching found** during the review:
- `AGENTS.md` carries the same stale `--compact / get / find / map /
  impact` guidance as `CLAUDE.md`. Injected into codex context.
- `README.md` project-verb examples must be copy-runnable against
  `.design`.
- Historical `.design/` docs contain many retired-command examples
  — leave alone; they're historical record. Current authoritative
  docs (CLAUDE.md, AGENTS.md, README.md, SKILL.md, top-level help)
  need to be clean.

Doc sync scope expanded in Theme 5.J accordingly.

**Net:** v0.14 is the calibration release. With the three blockers
fixed above, the implementation directive is green to land. ~4.5
days codex cost. Phase 0 design doc lives in parallel at
`.design/2026-05-28-phase0-handle-kind-consolidation.md`.
