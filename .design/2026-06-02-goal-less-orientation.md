---
status: draft
---

# Status is aggregate; orientation is a query: fixing the cold-start surface

Two coupled defects, one elegant fix, **no new command**:

1. `status` became `garden` — a per-handle list of problems to fix — instead of
   a *status*: an aggregate readout of how the corpus is doing.
2. Goal-less orientation ("what is this, what do I read first") has no home:
   `context` requires a goal; the old `orient` verb was retired in v0.13 and
   nothing absorbed it. A CR-D2 (cold-agent test) regression.

Both trace to one root — **`status` is wired to a per-handle predicate when its
job is aggregate** — and resolve together: make `status` the aggregate
dashboard, and make orientation a pair of *taught prelude predicates + queries*
rather than a command. This keeps anneal's thesis intact (a minimal verb surface
over a rich queryable language; agents learn and compose the Datalog) and adds
zero CLI surface. Found dogfooding `anneal status` on murail (2026-06-02).

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
- **Where is the frontier?** Recency-first: the most recently moved specs.
  See "Specs are a moving frontier" below — this is the *dominant* signal.
- **What are the durable anchors?** The *small* set of living-authoritative
  documents the frontier still cites (formal-model v17 in murail), and curated
  entry points (README/OVERVIEW/INDEX/DESIGN-GOALS by convention). NOT "every
  high-citation hub."

### Specs are a moving frontier (the load-bearing design input)

Morgan, 2026-06-02: in practice these corpora work from the **most recent**
specs. Keeping old docs up to date is computationally infeasible, so old docs
**rot rather than getting maintained** — staleness is the norm, not a defect to
surface. The exception is a small set of *living-authoritative* anchors that are
deliberately kept current (murail's formal-model, currently v17).

This inverts the naive "foundation = most-cited hubs" model. An old, heavily
cited document is usually **archaeology, not foundation** — citation mass
accumulates with age, so ranking by it surfaces exactly the stale docs an agent
should *not* start from. The orientation ranking must therefore be:

- **Recency-dominant for the frontier tier.** Newest-moved specs first; this is
  where real work and current intent live. (`changed_within` / git-mtime, which
  the runtime already has.)
- **Authority as a small curated/living tier, not a citation-mass tier.**
  Surface the deliberately-maintained anchors (status `authoritative`/`living`,
  or a `purpose:`-style curated marker, or a configured short list like
  formal-model) — a handful, not a ranked dump of old hubs.
- **Log-scale and recency-weight any citation signal** so inbound mass cannot
  drown recency. This is precisely the bug the legacy `orient` already fixed
  (see 0.9.1/0.9.2 CHANGELOG: exponential recency decay + `ln(count+1)` inbound,
  added *because* old label-anchors were beating recent active specs). Do not
  re-introduce it.

The balancing act: recency-first surfaces the working frontier but would bury
the stable v17-style anchors that *are* worth reading first; the curated/living
tier exists to hold those up explicitly. Two tiers, with recency driving the
frontier and explicit authority (not citation age) driving the anchors.

The old `orient` command had exactly this instinct (Frontier = where work is now
by recency; Foundation = the living anchors the frontier still cites, with
recency-weighted inbound and curated-hub bonuses — *not* raw citation mass). Its
logic survives in `crates/anneal-legacy/src/cli/orient.rs` as reference — not to
resurrect wholesale, but the two-tier "frontier + foundation" framing, and
specifically its recency-weighting fix, is the right one and predates the
regression.

## The statusless-majority problem (design must handle this)

A surface that keys on lifecycle status shows 25% of murail. Orientation must
work for corpora that barely use status frontmatter — it should lean on
**graph-structural** signals (incoming-edge hubs, neighborhood density, curated
filenames, recency) that exist regardless of status. The runtime already exposes
`hub(h, degree)`, `incoming_edge(h, from, kind)`, `neighborhood(h, depth, member)`,
and `changed_within` — orientation is largely a matter of composing these, not
new primitives.

## Surface options (considered)

Three shapes were weighed (decision in the next section):

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

## Decision: no new command — `status` becomes aggregate, orientation becomes a taught query

Converged claude + codex + Morgan, 2026-06-02. Codex argued to restore an
`orient` verb; Morgan overrode, and the override is right *on anneal's own
thesis*: the product is **a minimal verb surface over a rich queryable
language** — the whole point is agents learning the Datalog and being creative
with it. A bespoke `orient` command is the un-anneal move; it re-grows the noun
catalog the v0.13 reduction deliberately cut. The elegant fix makes orientation
a **language capability**, not a command.

The real defect, stated precisely: **`status` was miscategorized.** It is wired
to `status_item(section, h, score, why)` — a **per-handle** predicate — so it can
only render as *a list of handles to fix*. That is `garden`, not status. A true
status is **aggregate**: vital signs about the whole corpus. The fix is a
data-shape fix, and it splits cleanly along a single principle:

> **`status` is the only aggregate verb. Everything else is the language.**

Resulting surface — **no new command**:

- **`anneal status`** — becomes a true **state readout / dashboard**: all
  aggregates (scale, lifecycle coverage, pipeline histogram, health counts,
  movement), zero per-handle dumps. It answers "how is this corpus doing?" and
  ends with **pointers that teach the queries** for the deeper views (orientation
  reading-list, problem work-list). It stops being `garden` because it stops
  emitting per-handle rows at all.
- **Orientation = taught prelude predicates + a worked query**, surfaced through
  the *existing* language surface (`describe`, `help`, the status pointer):
  ```
  anneal -e '? recent_frontier(h, rank, recency), *handle{id: h, file: f}.'
  anneal -e '? anchor(h, score, why), *handle{id: h, file: f}.'
  ```
  New prelude predicates (`recent_frontier`, `anchor`) carry the ranking; the
  "reading list" is a query an agent runs and can then *modify* — filter by area,
  widen the window, join to `read`. That's strictly more powerful than a fixed
  `orient` command, and it teaches the language instead of hiding it.
- **`context GOAL`** — goalful retrieval, unchanged. No-goal `context` recovers
  with a hint pointing at the orientation query, not a silent mode-flip.

Why this satisfies both of Morgan's nudges at once: "status should be *status*"
(it becomes pure aggregate vital-signs) and "do both a bit / avoid a new
command" (status carries orientation *pointers* + a coverage line, but the
orientation workflow itself lives in the language, not a verb). The
"status-does-both" worry dissolves: status does aggregates + points; the
ranked-handle work is `-e` queries the agent owns.

## Ranking model (recency-frontier + durable anchors)

Two top-level tiers, NOT one "foundation = most-cited" ranking:

1. **Recent frontier / live specs.** Recency-weighted, status-aware,
   graph-aware. In a moving-frontier corpus, recency is *truth-proximity*, not a
   tie-breaker — this is the first thing a cold agent should see.
2. **Anchors.** Durable load-bearing docs that stay true despite age:
   `stable`/`reference`/`authoritative`/`living` status, curated hub names,
   high incoming degree, explicit `purpose:` cues. A handful, deliberately held
   up — not a ranked dump of old hubs.

Recency applies **strongly** to the frontier tier and **weakly** to anchors —
else a fresh-but-peripheral doc beats formal-model-v17 (over-recency), or pure
hub-degree over-ranks stale archaeology (the legacy 0.9.1/0.9.2 bug:
exponential recency decay + `ln(count+1)` inbound were added precisely because
old label-anchors beat recent active specs — do not regress it). Mine the legacy
`orient` scoring instincts; do not resurrect it wholesale.

## Statusless-majority handling

Orientation is **graph-first, status-optional**. murail's 1203/1576 `status:null`
means lifecycle status is not the corpus's primary navigation substrate; rank on
recency + graph signals (`hub`, `incoming_edge`, `neighborhood`, `changed_within`
— all already in the runtime), using status only as a boost/filter where the
corpus provides it. Do NOT globally punish `status:null` (many statusless docs
are central references). Emit a CR-R12-style honesty line:
`status coverage: 24% — orientation is graph+recency-led`.

The ranked-handle output is a *query the agent runs*, so budgeting is the
agent's `--limit` / a `TopK` in the query, not a fixed command's pagination — one
more reason orientation belongs in the language, not a verb.

## The new `status` (aggregate dashboard)

All lines are aggregates (`Count{}` / histogram predicates); **no per-handle
rows**. Target shape:

```
Status — <corpus>
  Scale        1576 handles · 24% lifecycle coverage (1203 statusless)
  Pipeline     raw 0 → draft 1 → research 2 → exploratory 3 → plan 4 → current 5 → active 6
  Convergence  146 holding · 0 advancing · 0 drifting   (no snapshot history — re-run to accrue)
  Health       2 broken · 6 blocked · 11 spec-code-drift
  Read first   12 recent frontier · 5 anchors
               ? recent_frontier(h, rank, recency), *handle{id: h, file: f}.
  Work         8 problems
               ? diagnostic{severity: "error", subject: h}.
```

The bottom two lines are the "do both a bit": status *names the counts* and
*hands the agent the query* to expand each — orientation and work-list become
one-line teachable queries, not inline dumps. `status coverage: 24%` is the
CR-R12 honesty line (this corpus is graph+recency-led, not status-led).

## How this maps to the Datalog (the heart of the plan)

The surface confusion is a predicate-category confusion. The prelude already has
both kinds; the verbs were mismatched:

| category | predicates (exist today) | belongs to |
|---|---|---|
| **aggregate** | `configured_pipeline_status`, `area_file_count`, `frontmatter_adoption_high`, `Count{}` rollups | **`status`** |
| **per-handle** | `status_item`, `frontier`, `blocker`, `diagnostic` | the language (`-e`) |

Work, in three Datalog pieces:

1. **Aggregate rollups for `status`.** Add prelude predicates the dashboard
   reads: a corpus-scale/coverage rollup, a convergence-count rollup
   (`Count{ h : holding(h) }` etc.), reuse `configured_pipeline_status` for the
   histogram. `status` becomes a thin projection over these — *not* `status_item`.
2. **The missing orientation predicates** (the real new language surface):
   - `recent_frontier(h, rank, recency)` — recency-dominant, status-aware,
     graph-aware; the "what's live" tier. Recency strong here.
   - `anchor(h, score, why)` — durable load-bearing docs; recency-weak,
     authority/curation/graph-degree driven; the "read-first-despite-age" tier.
   Both per-handle and `-e`-queryable, so agents compose/modify them (filter by
   area, widen window, join `read`). This is the strictly-more-powerful-than-a-
   command win, and the actual new Datalog work.
3. **`status_item` demotion.** It stops backing `status`. It can remain a
   queryable predicate (or be folded into `frontier`/`blocker`) — but the
   `status` *verb* no longer renders it.

Provenance/inspectability: because ranking lives in `recent_frontier`/`anchor`
predicates (not CLI Rust), an agent can `describe recent_frontier` to learn the
scoring and `--explain` a row to see why it ranked. Ranking that teaches itself.

Connection to the perf-architecture arc (epic anneal-g0l4): the aggregate
`Count{}` rollups `status` will lean on are exactly what the planned evaluator +
tuple indexes make cheap; today they re-run the fixpoint. The two efforts are
independent (this is prelude/verb-layer, that is runtime-core) but aligned —
status-as-aggregate gets faster for free when the arc lands.

## Ranking the two tiers (details)

`recent_frontier`: recency-dominant (exponential decay, `changed_within`/
git-mtime), status as a boost where present, light graph signal. `anchor`:
authority/curation-driven (`stable`/`reference`/`authoritative`/`living` status,
curated hub filenames, `ln(count+1)` recency-weighted incoming degree), recency
**weak** so formal-model-v17 stays up despite age. Do not regress the legacy
0.9.1/0.9.2 fix (recency decay + log-scaled inbound, added because old
label-anchors beat recent specs). Mine `anneal-legacy/src/cli/orient.rs` for the
scoring instincts; reimplement as v2 prelude predicates — do not port the command.

## Phased plan

1. **`status` → aggregate.** Define the rollup predicates; rewire the `status`
   verb to project them; add the coverage honesty line + the two pointer lines.
   This alone fixes "status is garden." Differential check: aggregate counts
   match what the old per-handle lists summed to.
2. **Orientation predicates.** Add `recent_frontier` + `anchor` to the prelude
   with `describe` cards teaching the scoring; the status pointers reference
   them. Validate on murail (recent specs surface; formal-model-v17 stays an
   anchor despite age) and on a statusless-heavy slice (graph+recency still
   orients).
3. **Teaching.** `help`/SKILL/README: the cold-start ladder becomes
   `status` (how is it doing) → `recent_frontier`/`anchor` queries (what do I
   read) → `context GOAL` (answer my question). No new verb to document.

## Tightenings before build (codex review, 2026-06-02 — converged)

Codex reversed its restore-`orient` recommendation under this reframe ("no new
verb is correct"). Nine tighten-before-build points, all folded:

1. **Discoverability is the whole risk — pointers must always be visible and
   copy-runnable.** If orientation is query-only, `status` must teach it *every
   run*, not only on low-coverage corpora. Always render a compact "Read first"
   line carrying both queries (count-aware but never absent). The ladder must be
   physically on screen: `status → recent_frontier/anchor → context GOAL`.
2. **Aggregate-only, but not sterile.** The pointer lines are part of the
   contract, not decoration: status names the counts AND gives the exact
   copy-runnable queries (read-first: `recent_frontier`+`anchor`; work:
   `diagnostic`/`blocker`). A dashboard with no expansion path is a dead end.
3. **`recent_frontier`/`anchor` live in a new `orientation.dl`**, not
   `convergence.dl` — they *use* convergence/status signals but their job is
   arrival. Keeps `convergence.dl` from becoming a grab bag; lets `describe
   runtime` group them as "Orientation predicates."
4. **`recent_frontier` MUST be recency-dominant, status as boost/filter — never
   gated on `active()`.** Verified on murail: the 10 most-recently-moved docs are
   a mix of `draft`/`active`/`superseded`/statusless. Gating on `active()` would
   reproduce the 25%-coverage bug under a new name. BUT (refinement from the same
   data) exclude terminal/`superseded`: a doc is often touched *at the moment it
   is superseded*, and a freshly-superseded doc is not the frontier. So:
   recency-dominant, statusless-inclusive, terminal-excluded, active a boost.
   (Also seen: `formal-model/v18-updates.md` is recent+draft while v17 is the
   `authoritative` anchor — the spine itself moves; v18 = frontier, v17 = anchor.
   The design handles this naturally.)
5. **Ranking inspectable, not clever.** Expose the components via `describe` +
   `--explain`: a row's `why` distinguishes `recent` / `configured_anchor` /
   `authoritative_status` / `curated_name` / `inbound_degree`. Agents must see
   *why* a doc ranked.
6. **Status aggregate counts must preserve old meaning (correctness gate).**
   Before removing per-handle rows: broken/blocked/holding/open counts must match
   the old section counts; flow baseline behavior stays honest; W006/diagnostic
   counts must not double-count duplicate diagnostic rows; and the **coverage
   denominator must be defined** — lifecycle coverage is over *file handles*
   (not labels/externals), reported distinctly from total handle count. Nail
   these in tests.
7. **`status_item` stays a queryable predicate for existing `-e` users** — only
   the `status` *verb* stops rendering it. Demotion, not deletion (don't break
   agents who query `status_item` directly).
8. **Pointer queries carry a bound** (`--limit 12` or a `rank <= 12` guard) so a
   cold agent copying the query doesn't dump a 1576-handle corpus. `--limit` is
   the clearer form.
9. **Anchor config is a boost/seed, not a replacement.** Signal-derived first.
   Design `config orientation { anchor([...]). }` now (a corpus owner knows its
   living spine — formal-model — better than the graph), but implement only if
   signal-derived ranking fails the formal-model-v17 acceptance test.
   Configured anchors become *guaranteed candidates* with `why=configured_anchor`,
   then still scored/explained — never an opaque hardcoded CLI list.

## Open (smaller) questions

- `anchor` naming: keep it (short, nice) and rely on `describe runtime` grouping
  it under Orientation, or go explicit `orientation_anchor` (uglier)? Lean: keep
  `anchor`, fix meaning via the describe card.
- Exact recency decay + inbound-weight constants — port the legacy 0.9.1/0.9.2
  shape, tune against the murail acceptance test (recent specs surface; v17 holds
  as anchor; freshly-superseded docs do not appear as frontier).
