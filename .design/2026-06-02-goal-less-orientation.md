---
status: draft
---

# Goal-less orientation: the cold-start gap `status` left behind

A cold agent arriving at a corpus with **no specific goal** has no good
entrypoint. `status` shows convergence *problems*; `context` *requires* a goal.
The "what is this place, what's authoritative, where do I start reading"
question — orientation — has no home. This is a CR-D2 (cold-agent test)
regression, found by dogfooding `anneal status` on the murail corpus (2026-06-02).

## Evidence

`anneal status` on murail (`.design`, 1576 handles) renders:

```
Convergence  broken=2  blocked=6  open=0  advancing=0  holding=146  drifting=0
Broken   (2)   E001 ...
Blocked  (6)   spec_code_drift / stale_dep ...
Holding  (146) ... 134 more
```

Three facts make this all-problem, no-orientation:

1. **`status` only ever showed the convergence frontier** (work / blocked /
   advancing / holding / drifting / broken). It never had an orientation layer.
   That was acceptable while `orient` existed; it was retired in the v0.13
   reduction without `status` absorbing its role.
2. **The frontier collapses to problems on a real corpus.** `open`/`work` = 0
   (everything statusful is holding or blocked); `advancing`/`drifting` = 0 (no
   snapshot history). What remains to show is broken + blocked + a wall of
   holding. W006 (shipped v0.15.0) correctly *added* blocked rows — making the
   surface even more problem-shaped.
3. **Most of the corpus is invisible to it.** murail status distribution:
   **1203 handles `status: null`**, ~370 with a status. `status` reports the
   frontier over the status-bearing ~25%; the statusless 75% — much of the
   actual knowledge — never appears.

And `context` cannot fill the gap: `context` with no goal errors
(`context requires a goal`), `context ""` errors (empty search). There is
literally no goal-less command.

## What orientation owes a cold agent

Not "what's wrong" (that's `status`) and not "answer my question" (that's
`context GOAL`). Orientation answers, with no input but the corpus:

- **What is this?** Scale (files, handles), shape (status histogram, areas),
  the corpus's own vocabulary.
- **What's load-bearing?** The authoritative / foundation documents — high
  incoming-citation hubs, `authoritative`/`stable`/`reference` status, curated
  entry points (README/OVERVIEW/INDEX/DESIGN-GOALS by convention). "Read these
  first."
- **Where is it moving?** The active frontier — but as one section among
  orientation, not the whole report.

The old `orient` command had exactly this instinct (Frontier = where work is now,
Foundation = stable hubs the frontier still cites, with curated-hub bonuses). Its
logic survives in `crates/anneal-legacy/src/cli/orient.rs` as reference — not to
resurrect wholesale, but the two-tier "frontier + foundation" framing is the
right one and predates the regression.

## The statusless-majority problem (design must handle this)

A surface that keys on lifecycle status shows 25% of murail. Orientation must
work for corpora that barely use status frontmatter — it should lean on
**graph-structural** signals (incoming-edge hubs, neighborhood density, curated
filenames, recency) that exist regardless of status. The runtime already exposes
`hub(h, degree)`, `incoming_edge(h, from, kind)`, `neighborhood(h, depth, member)`,
and `changed_within` — orientation is largely a matter of composing these, not
new primitives.

## Surface options (the decision)

Three shapes; not mutually exclusive, but pick the primary:

1. **Restore a goal-less `orient` verb.** A dedicated arrival surface: corpus
   shape + foundation hubs + frontier, no goal required. Pro: clean separation
   of concerns (orient = where am I, status = what's unsettled, context = answer
   my goal). Con: re-adds a command to a surface we deliberately narrowed —
   must earn its place per CR-D102 (the Surface Evolution Framework).
2. **`status` grows an Orientation header.** Prepend a corpus-shape + foundation
   section before the convergence/problem sections, so arrival leads with "what
   is this place." Pro: no new command; `status` becomes a true arrival surface
   as its help already claims. Con: conflates two jobs in one verb; risks a
   long, mode-heavy output.
3. **`context` gains a no-goal mode.** `context` with no argument returns the
   orientation bundle instead of erroring. Pro: `context` is already "the
   cold-start command"; one entrypoint. Con: overloads one verb with two
   behaviors (orient vs retrieve-for-goal); the no-goal output is a different
   shape than the goal-driven one.

Lean (per first analysis, open to codex/Morgan): **option 2 as the primary** —
`status` should deliver on the arrival promise its own help text makes
("use this as the arrival command"), led by orientation, with the convergence
frontier as a section within it. It keeps the surface narrow (no new verb) and
fixes the exact "status is all-diagnostic" complaint at its source. A goal-less
`context` (option 3) is a reasonable secondary alias. Restoring `orient`
(option 1) is the cleanest conceptually but adds surface; only if 2 proves too
crowded.

## Open questions for review

- Primary surface: status-header vs goal-less-context vs restored-orient?
- Foundation ranking: reuse legacy `orient`'s recency-weighted incoming-degree +
  curated-hub bonus, or a simpler `hub`-degree + authoritative-status pass?
- Does orientation need budget/pagination (murail is 1576 handles)? Almost
  certainly — a bounded "top N foundation, top N frontier," like `context --hits`.
- Statusless handles: surface them via graph signals, or flag the corpus as
  "low status adoption, orientation is graph-only here" (an honest CR-R12-style
  signal about the corpus's own shape)?
- Interaction with the perf-architecture arc: this is prelude/verb-layer work
  (composing existing predicates), mostly independent of the runtime rewrite —
  can proceed in parallel.
