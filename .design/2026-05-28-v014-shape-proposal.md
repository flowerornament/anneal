---
status: converged
updated: 2026-05-28
author: claude (post-v0.13 cold-agent simulation + master spec deep read)
reviewer: codex (independent review converged 2026-05-28)
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-26-surface-evolution-framework.md
description: >
  Proposed v0.14 shape after grasping the full v0.13 mental model and
  surface UI through cold-agent simulation on .design + large-corpus. Names
  the architecture as it actually is, identifies one v0.13 tag-blocker,
  and proposes a four-item v0.14 slice plus deferrals.
---

# v0.14 Shape Proposal

## The Mental Model (v0.13.0 as-shipped, six layers)

```
                            KNOWLEDGE CORPUS
                          (markdown on disk)
                                  │
                ╔═════════════════ │ ═════════════════╗
                ║   ADAPTER (anneal-md, anneal-host)   ║
                ║   extracts typed facts               ║
                ╚═════════════════ │ ═════════════════╝
                                  │
                                  ▼
┌────────── SUBSTRATE (stored relations, §10) ─────────────────┐
│  *handle  *edge  *meta  *content  *span                       │
│  *concern  *config  *snapshot  *trail  *generation            │
│                                                                │
│  *handle.kind ∈ {file, section, label, version, external}     │
└──────────────────────────┬────────────────────────────────────┘
                           ▼
┌────────── ENGINE PRIMITIVES (§11, Rust-native) ───────────────┐
│  GRAPH:     upstream  downstream  impact  neighborhood        │
│             in_degree  out_degree                              │
│  LIFECYCLE: terminal  active  settled  discharged              │
│             undischarged  obligation  pipeline_position[_for]  │
│             flux                                               │
│  TIME:      freshness  git_mtime  changed_within               │
│  CONTENT:   search  match  read  read_full  token_estimate     │
│  COUNT:     cite_count  discharge_count                        │
│  INTROSPECT: schema  describe  examples  predicates            │
│             sources  source_of  verbs                          │
└──────────────────────────┬────────────────────────────────────┘
                           ▼
┌────────── PRELUDE VOCABULARY (Datalog, prelude/*.dl) ─────────┐
│                                                                │
│  POTENTIAL                                                     │
│     entropy(h, source)  →  potential(h, energy)               │
│                          ↘  primary_entropy(h, source)        │
│     7 sources:                                                 │
│       undischarged, broken_ref, stale_dep, confidence_gap,    │
│       freshness_decay, missing_meta, orphan_label              │
│                                                                │
│  WORK                                                          │
│     work_candidate(h, energy)  ←─ raw pool                    │
│           ▼                                                    │
│     frontier(h, energy)        ←─ capped TopK                 │
│     area_frontier(area, h, score, why)                        │
│                                                                │
│  BLOCK                                                         │
│     blocked(h)  →  blocker(h, energy, source)                 │
│                                                                │
│  FLOW   (snapshot-relative)                                    │
│     advancing(h)  ←─ ✓ ships                                  │
│     holding(h)    ←─ ⚠ taught in --help, predicate missing    │
│     drifting(h)   ←─ ⚠ taught in --help, predicate missing    │
│                                                                │
│  GROUPING                                                      │
│     area_of(h, area)  ←─ derived from filesystem              │
│     area, area_health, area_frontier                          │
│     *concern{name, member}  ←─ configured in anneal.dl        │
│                                                                │
│  DIAGNOSTICS                                                   │
│     diagnostic(code, severity, subject, file, line, evidence) │
│     12 codes (E001 W001-W004 I001-I002 S001 S003-S005)        │
│     each backed by a rule predicate                            │
│                                                                │
│  GRAPH HELPERS                                                 │
│     namespace_of  hub  orphan  incoming_edge  outgoing_edge   │
│     file_prefix  file_parent_dir  incident                    │
└──────────────────────────┬────────────────────────────────────┘
                           ▼
┌──────── CLI SURFACE (9 visible + 2 hidden) ───────────────────┐
│                                                                │
│  ARRIVAL                                                       │
│    status     ▶ Convergence header + Broken + Blocked + Work  │
│    context    ▶ search + read + neighborhood for a goal       │
│                                                                │
│  PROGRAM                                                       │
│    schema     ▶ predicate catalog                              │
│    describe   ▶ teach NAME (rule, topic, diagnostic code)     │
│                                                                │
│  RETRIEVE                                                      │
│    search     ▶ engine primitive                               │
│    read       ▶ engine primitive                               │
│    handle     ▶ stored projection + edges + --impact          │
│                                                                │
│  COMPOSE                                                       │
│    eval (-e)  ▶ THE LANGUAGE itself                           │
│                                                                │
│  BOOTSTRAP                                                     │
│    init       ▶ scaffold anneal.dl from sources                │
│  HELP                                                          │
│    help       ▶ topics + agent briefing                        │
│                                                                │
│  HIDDEN                                                        │
│    check      ▶ CI gate alias                                  │
│    prime      ▶ skill loader alias for `help agent`           │
└──────────────────────────┬────────────────────────────────────┘
                           ▼
                AGENT LOOP (CR-D29)
        1. status  ▶ 2. eval frontier  ▶ 3. handle --impact
                ▶ 4. (do the work)  ▶ 5. status (confirm)
```

The substrate is small and clean. The prelude vocabulary is where the
philosophy lives. The CLI surface is the cold-agent landing pad. Each
layer earns its keep against the framework (CR-D102).

## Friction Surfaced By Cold-Agent Simulation

I exercised the v0.13 polish build against `.design` (anneal's own corpus)
and `/path/to/large-corpus/.design` (the test corpus). Real friction:

### 1. TRIAD HONESTY — `--help` lies (v0.13 tag-blocker)

`crates/anneal-legacy/src/app.rs:63`:
> Enables convergence tracking (advancing/holding/drifting) via
> `at("snapshot:last")` and snapshot history queries.

But `holding(h)` and `drifting(h)` return "unknown predicate" errors.
Framework principle: teaching messages must not lie. Same pattern the
pre-tag review caught for trend/temporal in `ac9dece`. This is a tag-
blocker. Two paths:

  (A) Trim --help in v0.13.0 to teach what exists.
  (B) Ship the triad in v0.13.0 (requires semantic design).

I propose (A) for v0.13.0 + (B) properly in v0.14.0.

### 2. BLOCKED-LIST OVER-FIRING

On large-corpus, `blocker(h, energy, source)` emits one row per (handle ×
entropy source). A handle hit by 3 signals appears 3 times. The Blocked
list looks like:

```
 4. ...cross-cutting-analysis.md   freshness_decay
 5. ...cross-cutting-analysis.md   missing_meta
 6. ...expression-tree-functor-analysis.md   freshness_decay
 7. ...expression-tree-functor-analysis.md   missing_meta
```

That's two handles in four rows. The status CLI doesn't deduplicate;
the eval `blocker` doesn't deduplicate. `primary_entropy(h, source)`
exists for the dedup case but isn't taught as the recommended path.

### 3. AREA_HEALTH IS BROKEN

`area_health` returns rows only for areas with errors > 0. Areas with
zero errors are silently absent. large-corpus has 16 areas; only `references`
shows up. Looking at `convergence.dl:136`:

```dl
area_error_count(area, errors) :=
  area(area),
  errors = Sum{ count : area_error_location_count(area, code, ...) }.
```

When `area_error_location_count` yields zero rows for an area, the Sum
aggregate doesn't fire and `area_error_count` produces no row, so
`area_health` joins fail. Real Datalog bug. Not user-facing today
(status doesn't render area grades), but the predicate doesn't deliver
what its name promises.

### 4. *CONCERN HAS NO TEACHING STORY

```
$ anneal --root .design describe '*concern'
Stored concern membership facts.
Kind: stored relation.
Signature: *concern{corpus, source, native_id, origin_uri, revision,
                    generation, name, member}.
Example: ? *concern{name: concern, member: h}.
Contract: .design/2026-05-13-corpus-runtime.md.
```

No relationship explanation. No worked example. No mention of S005
(concern_group_candidate) which is the one place *concern earns its
keep today. Compare to the rich `area_of` describe card which already
ships.

### 5. HISTORY-CONCEPT CONFLATION

Three "history" concepts coexist with no disambiguation in describe runtime:

```
*snapshot{snapshot, at, id, key, value}      — graph state over time
*generation{corpus, source, current}          — source data epoch
*trail{session_id, step, redacted_expr, ...}  — per-query provenance
```

An agent reading describe runtime can't tell when each applies. They
answer different questions:
- "What did the corpus look like a week ago?" → *snapshot
- "Did source data change since I last ran?" → *generation
- "Why did this query return this row?" → *trail (via --explain)

## v0.14 Conviction

```
┌─────────────────────── V0.14 SLICE ───────────────────────────┐
│                                                                │
│  A. CONVERGENCE FLOW TRIAD                                     │
│     ┌────────────────────────────────────────────────┐         │
│     │  advancing(h)  ✓ exists, snapshot-relative     │         │
│     │  holding(h)    ← NEW: status unchanged + has   │         │
│     │                  potential + active             │         │
│     │  drifting(h)   ← NEW: regressed in pipeline,   │         │
│     │                  OR re-opened from terminal     │         │
│     │  convergence_flow(h, direction) ← NEW: union   │         │
│     └────────────────────────────────────────────────┘         │
│     All four derived; trivial rules; no engine work.           │
│     --help truthful again.                                     │
│                                                                │
│  B. HISTORY CONCEPTS in describe runtime                       │
│     Three-row subsection under describe runtime:               │
│       Snapshot   — graph state at a point in time              │
│       Generation — source data epoch                           │
│       Trail      — per-query provenance                        │
│     Pure docs. Anchors the magic words.                        │
│                                                                │
│  C. *CONCERN teaching story                                    │
│     Rich describe card mirroring area_of's depth:              │
│       Relationship: configured cross-cutting grouping;         │
│         dual to area_of (which is filesystem-derived).         │
│       Common joins:                                            │
│         *concern{name: "X", member: h}, frontier(h, energy)    │
│           — concern-scoped work                                │
│         *concern{name: c, member: h}, diagnostic{subject: h}   │
│           — concern-scoped diagnostics                         │
│         diagnostic{code:"S005", ...}, top_pair(...)            │
│           — when S005 suggests a new concern                   │
│       Example: declared in project's anneal.dl                 │
│       See also: area_of, S005, top_pair, *handle               │
│                                                                │
│  D. AREA_HEALTH FIX                                            │
│     Make area_health emit rows for ALL areas, not just those   │
│     with errors. Either:                                       │
│       (i)  Change area_error_count to default to 0 when no    │
│            rule rows exist                                     │
│       (ii) Add area_health body that joins area+files only,   │
│            then add error/cross-edge facets                    │
│     This is a real Datalog bug; it's also the substrate for   │
│     possible future per-area status rendering.                 │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

### Item A semantics (the design decision)

```dl
# advancing already exists (recently_advanced):
#   status at snapshot:last → moved forward in pipeline → status now

@doc(
  name: "holding",
  doc: "Return active handles whose status is unchanged from the latest
        snapshot and which still carry unsettled-work signals."
).
holding(h) :=
  active(h),
  potential(h, energy),
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  prior = current.

@doc(
  name: "drifting",
  doc: "Return handles that moved backwards in the lifecycle or
        re-opened from terminal since the latest snapshot."
).
drifting(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  pipeline_position_for(prior, p_prior),
  pipeline_position_for(current, p_current),
  p_current < p_prior.

# Re-opening from terminal is also drifting:
drifting(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  terminal_status_in_config(prior),
  active(h).

@doc(
  name: "convergence_flow",
  doc: "Categorize each handle's recent motion: advancing, holding,
        drifting, or unknown if there is no prior snapshot."
).
convergence_flow(h, "advancing") := advancing(h).
convergence_flow(h, "holding")   := holding(h).
convergence_flow(h, "drifting")  := drifting(h).
```

This is the natural extension of `recently_advanced`. The `convergence_flow`
union is queryable as one shape.

### What I'm NOT proposing (deferred from v0.14)

```
┌─────────────── DEFERRED ──────────────────────────────────────┐
│                                                                │
│  E. HANDLE-KIND CONSOLIDATION (5 → 3)                          │
│     file, section, label, version, external                    │
│       → file, label, external                                  │
│     Large-corpus reviewer's proposal. Substrate-affecting; breaks      │
│     identity contract; needs migration story. Real value but   │
│     own design arc. Defer to its own .design/ doc.            │
│                                                                │
│  F. convergence(broken, blocked, work, advancing) AGGREGATE    │
│     Status header line exposes the four counts already. Eval  │
│     composition expresses each component. Framework KEEP-c    │
│     says only if no eval expresses; they do. Add only if      │
│     cold-agent evidence shows agents fumbling the question.   │
│                                                                │
│  G. learn VERB (collapse schema + describe)                    │
│     Premature. v0.13 just finished the reduction arc. Let     │
│     the surface settle first.                                  │
│                                                                │
│  H. BLOCKED DEDUP via primary_entropy                          │
│     The over-firing is real but the fix is teaching, not a    │
│     new predicate. Solved by Item C's pattern: describe       │
│     blocker should recommend the `primary_entropy` join for   │
│     unique-per-handle listings. Add to describe blocker as    │
│     part of Item C (no separate item needed).                  │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

## v0.13 Tag-Blocker Resolution

```
crates/anneal-legacy/src/app.rs:63

  - Enables convergence tracking (advancing/holding/drifting) via
  - `at("snapshot:last")` and snapshot history queries.
  + Enables convergence tracking via advancing(h) and snapshot
  + history queries. The full flow vocabulary (holding/drifting)
  + ships in a later release.
```

One-line trim. Honest now. Framework principle preserved. v0.13 tags
without lying.

## Estimated Cost

Item A: ~30 lines of new prelude rules + 4 describe cards + 1 test.
Item B: pure docs, ~40 lines in describe_runtime output.
Item C: ~25 lines in describe content for *concern + ~10 lines for
        describe blocker (primary_entropy teaching).
Item D: bug investigation + fix in convergence.dl, plus test.

Estimate: 1-2 days. Comparable to a single v0.13.0 polish slice.

## CR-D Proposals

- **CR-D103 (Convergence flow triad).** advancing/holding/drifting are
  the three snapshot-relative motion predicates; convergence_flow(h, dir)
  is their union. All four are derived prelude predicates with no engine
  changes. Settled handles are outside the flow (settled(h) is durable).
  Handles without snapshot history are invisible to flow predicates by
  construction.

- **CR-D104 (Concern as configured grouping).** *concern is the dual to
  area_of: area_of groups handles by filesystem, *concern groups handles
  by configured cross-cutting concern (in anneal.dl). Both are valid
  grouping lenses; neither subsumes the other. S005 surfaces concern-
  group candidates from frequent namespace co-occurrence.

## Open Questions For Confirmation

1. **Triad semantics**: is the snapshot-relative definition right? "Holding"
   in particular has two reasonable readings: (a) status unchanged + still
   has potential (my proposal), or (b) status unchanged regardless of
   potential. Option (a) means a clean terminal-but-just-reached handle
   shows up as settled not holding; option (b) means anything in stasis
   shows up. Lean (a) because it matches the physics frame: holding =
   "stuck with work remaining".

2. **--help v0.13 wording**: trim to "advancing(h) and snapshot history
   queries" — or refuse to mention any predicate name in CORE CONCEPTS
   ("convergence tracking via snapshot history queries") to keep CORE
   CONCEPTS conceptual? I lean the named version because agents will
   reach for the predicate.

3. **area_health fix shape**: (i) make Sum default to 0, or (ii) drop
   the error-required constraint by restructuring rule heads? (i) is
   cleaner if Ascent supports it; (ii) more verbose but explicit. Need
   to verify (i) is the Ascent behavior.

4. **Should Item D be in v0.14 or its own bug-fix v0.13.1?** It's a real
   regression in eval correctness. If we're strict about "teaching
   messages must not lie", we should also be strict about "predicates
   must do what their names promise". area_health says it grades every
   area; it doesn't. Lean v0.14 because it's a real fix with semantics
   that need thought.

5. **CR-D102 process check**: does this proposal itself pass the Feature
   Justification Template? Items A and C add new vocabulary. The
   template would ask: eval equivalent? — for the new predicates, the
   eval form is the rule body itself (composable from existing facts).
   Real-world signal? — Item A is driven by a stale-teaching bug in
   shipped --help; Item C is driven by S005's documented dependency.
   Both have evidence. Item D fixes a regression, doesn't add surface.
   Item B is pure docs. Net surface delta: +3 predicates (holding,
   drifting, convergence_flow), 0 commands, 0 verbs, 1 fix. The slice
   passes the framework.

## project owner's decisions (2026-05-28, on first review)

1. **holding(h) semantics:** WITH-POTENTIAL.
   `holding(h) := active(h), potential(h, energy), status unchanged
   since snapshot:last`. Matches physics frame: "stuck with work
   remaining". Settled-but-just-reached handles read as settled, not
   holding.

2. **--help v0.13 wording:** RESOLVED — codex landed 38a609f. v0.13
   tag boundary clean, awaiting project owner's explicit go.

3. **area_health fix in v0.14:** YES. Bundle with the conceptual
   work. Real fix with semantics that need thought.

4. **Handle-kind consolidation (5→3) IN v0.14 SCOPE.** This expands
   v0.14 from a vocabulary-polish slice to a substrate-affecting arc.
   Implications:
   - Substrate change to *handle.kind enum
   - Identity contract evolution; needs migration story
   - Source trait may shift: how does `kind` flow from adapter →
     substrate when fewer kinds exist?
   - Large-corpus reviewer's input belongs in the design conversation
   - Backward-compatibility decision: do v0.13 corpora's handle
     identities migrate cleanly, or is this a major version bump?

5. **Codex review:** message now for independent take.

## v0.14 scope (locked-in after project owner's review)

```
v0.14 = CONCEPTUAL ARC (not a polish slice)

A. Convergence flow triad (advancing/holding/drifting/convergence_flow)
B. History concepts disambiguation in describe runtime
C. *concern teaching story
D. area_health bug fix (rows for all areas)
E. HANDLE-KIND CONSOLIDATION (5 → 3 candidate)
   file, section, label, version, external
     → ???  (file, label, external? file, label, external+version?)
   Substrate change. Needs its own design conversation inside this arc.
```

## What needs more design before v0.14 implementation

Item E surfaces three sub-decisions:

E1. **Target kinds.** Two candidates from large-corpus reviewer:
   - (3 kinds) file, label, external — section becomes a marker on
     *handle, version becomes a *meta.kind="version"
   - (4 kinds) file, label, external, version — preserve version
     as substrate-shaped because versioned artifacts are a first-class
     concept for spec-heavy corpora
   - (3 kinds, different) file, label, ref — combine version and
     external as "ref" (anything referenced from outside)

E2. **Section → ??** Currently `*handle.kind = "section"` carries
   `(file, heading)` semantics. If sections fold away:
   - Option a: become *span rows (already exists; sections are spans)
   - Option b: become *meta rows on the parent file with key="section"
   - Option c: stay as *handle but with kind="file" + span_id

E3. **Version → ??** Versioned artifacts (e.g. `formal-model-v17.md`):
   - Option a: become labels (just a labeled file)
   - Option b: become *meta on the file with key="version", value="17"
   - Option c: stay because Supersedes edges need a typed target

This is non-trivial. v0.14 probably needs a Phase 0 design doc
specifically for E before A-D implementation begins.

## Suggested v0.14 sequencing

```
Phase 0 (design):
  - .design/2026-XX-XX-handle-kind-consolidation.md
  - Independent: claude + codex + large-corpus reviewer
  - Resolves E1/E2/E3 with substrate impact analysis
  - 1-2 weeks of design discussion

Phase 1 (slice 1):
  - Items A + B + C (vocabulary + docs only, no substrate impact)
  - Ships as 0.14-alpha-1 or held until full release
  - 1-2 days

Phase 2 (slice 2):
  - Item D (area_health bug fix)
  - 0.5 day

Phase 3 (slice 3):
  - Item E execution per Phase 0 decisions
  - Substrate change, migration story, possible breaking change
  - Could be its own minor (v0.15) if breaking; otherwise v0.14

Tag: v0.14.0 after all phases land, or split v0.14 (A-D) and v0.15 (E)
based on what Phase 0 reveals.
```

## Codex convergence (2026-05-28)

Independent review of the proposal, with schema/code checks and a
research-graph pass. Converged on the overall shape with five
refinements that strengthen the design.

### Model diagram — two refinements

The six-layer model is honest enough to converge on. Two adjustments:

1. **Split the substrate box into "stored/runtime relations."**
   - Source-owned: `*handle`, `*edge`, `*meta`, `*content`, `*span`,
     `*concern`
   - Runtime-owned: `*config`, `*snapshot`, `*generation`, `*trail`,
     `*trail_ref`, `*trail_generation`
   - The distinction matters for migration: snapshot history is a
     runtime concern with its own compatibility surface.

2. **Prelude vocabulary is the product ontology, not just philosophy.**
   That is why naming fixes matter more than normal API polish.
   Research-graph aligns: essential complexity is what users must
   reason about; agent interfaces should expose compact conceptual
   operations rather than implementation leakage.

### Item A refinements — flow predicates

**holding(h) guard.** project owner's with-potential semantics is right. Add
`energy > 0` if `potential(h, energy)` can ever emit zero rows. If
potential only emits rows for nonzero energy (verify), the current
rule is fine without the guard.

**Split drifting into regressed + re_opened.** Keeping them as separate
predicates preserves evidence; `drifting(h)` unions them.

```dl
@doc(name: "regressed",
     doc: "Active handles whose status moved backwards in the configured
           pipeline since the latest snapshot.").
regressed(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  pipeline_position_for(prior, p_prior),
  pipeline_position_for(current, p_current),
  p_current < p_prior.

@doc(name: "re_opened",
     doc: "Handles that returned from a terminal status to active
           since the latest snapshot.").
re_opened(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  terminal_status_in_config(prior),
  active(h).

drifting(h) := regressed(h).
drifting(h) := re_opened(h).
```

Reason: re-opened is operationally different and useful in
explanations/status. Hiding it inside drifting throws away evidence.

**convergence_flow exhaustiveness.** Current `advancing(h)` requires
`active(h)`, so a handle that just reached terminal vanishes from the
triad. If settled handles are outside flow by design, name that
explicitly. If the product story says convergence means handles
reaching terminal, consider `newly_settled(h)` or
`direction = "settled"` later. Do not block v0.14 A-D on this, but the
design doc should not imply the triad is exhaustive when it is not.

### Item C, D — agree as proposed

Bundle area_health fix in v0.14. Real predicate bug, not a v0.13 tag
blocker because default status does not render area grades. Fix with
tests proving all areas emit, including zero-error areas.

### Item E — handle-kind consolidation: lean 5 → 4, not 5 → 3

I do not yet buy 5 → 3 as the default. Current bias: fold section,
keep version until Phase 0 proves a cleaner representation.

**Why section folds (evidence):** current corpora show section handles
are mostly structural noise.

| Corpus | Sections | Files | Ratio |
|---|---|---|---|
| `.design` | 957 | 28 | 34:1 |
| `/path/to/large-corpus/.design` | 13,076 | 429 | 30:1 |

In both corpora, section handles had **0 incoming and 0 outgoing edges**.
They also currently have `line = 0` and empty summaries. That screams
accidental substrate complexity. Large-corpus reviewer's "section as
granularity" is real, but implementation needs work: `*span` today does
not emit heading spans, only full-file and label-definition spans. So
folding section means **adding heading spans with stable span ids, line
ranges, and summaries**; then search/read can narrow by `span_id`.
Section references can remain diagnostics/metadata, not handles.

**Why version probably stays for now:** large-corpus has 42 version handles
with `Supersedes` chains (version → version). Version is not just
display granularity — it is an artifact identity participating in graph
semantics and S001 orphan logic. Folding version into label would
pollute labels, because labels mean cross-reference/obligation
namespaces. Folding version into `*meta` can be right only if we also
introduce an explicit version relation and move `Supersedes` to
file/file or versioned_file/file semantics without losing
queryability. That may be the better final design, but it is not a
mechanical cleanup.

**Source trait impact:** the core `Source` trait likely survives
because adapters already emit `FactBatch` and `HandleFact.kind` is a
string. The break is semantic contract, not Rust trait shape.
`anneal-md` / legacy v2 adapter / any future adapter must stop emitting
section/version or emit the new relations. Compatibility risk is in
query/schema/history, not trait compilation.

**Migration story:** source facts rederive, so corpus files may need no
file migration. But eval queries/scripts using `kind = "section"` or
`kind = "version"` break, and snapshot history may contain old handle
ids/kinds. Phase 0 needs an explicit snapshot compatibility decision:
tolerate old kinds in `at()` history with partial-history warnings,
reset snapshot schema, or translate old rows. Also preserve recovery
docs for old query patterns: section → `*span`/read span_id, version →
`version_of` / `supersedes` relation or kept version kind.

**Release placement.** Include Phase 0 in v0.14 scope, but do not
pre-commit implementation of E to v0.14. If Phase 0 lands on 5 → 4
with no identity break, v0.14 absorbs it. If Phase 0 lands on 5 → 3
with version identity changes and snapshot migration, split A-D as
v0.14 and make the substrate arc v0.15 or v1.0. Pre-1.0 semver allows
breakage, but the product story is clearer when the substrate arc has
its own release narrative.

### Phase 0 doc shape — decision matrix plus evidence

Structure the Phase 0 doc with these required sections (not only a
reviewer list):

1. **Current uses inventory** — every `HandleKind` match arm, every
   `*handle.kind` query/doc/test, status/check/search/read/handle
   effects.
2. **Corpus evidence** — kind counts, edge participation,
   search/read examples on `.design` and `large-corpus`.
3. **Candidate models** — 5→4, 5→3 (version as relation/meta), 5→3
   (version as label). Explicit rejected options.
4. **Migration plan** — fact rederive, snapshot history, eval query
   compatibility, docs/changelog.
5. **Acceptance tests** — no section handles if folded; heading
   spans queryable/readable; `Supersedes` semantics preserved; S001
   semantics preserved; cold-agent smoke on large-corpus.
6. **Review section** — Claude, Codex, large-corpus reviewer independent
   notes, then project owner decision.

### Tag question — converged

**Tag v0.13.0 now.** Both reviewers agree. `38a609f` is clean, the
simplification arc plus vocabulary polish is a complete story, and
v0.13 does not constrain v0.14. Holding the tag until handle-kind
design resolves would blur two arcs and make the next substrate
decision heavier than it needs to be.

### Net convergence

| Item | Claude proposed | Codex review | Converged |
|---|---|---|---|
| Model diagram | Six layers, substrate as one box | Split substrate into stored vs runtime | ✓ split |
| holding(h) | active + potential + unchanged | Add `energy > 0` guard | ✓ guard added |
| drifting(h) | backward OR re-opened | Split regressed + re_opened, union as drifting | ✓ split |
| convergence_flow | union of three | Name non-exhaustiveness explicitly | ✓ documented |
| area_health fix | v0.14 bundle | Agree | ✓ |
| Handle kinds | 5 → 3 candidate | 5 → 4 (fold section, keep version) | ✓ working assumption |
| Phase 0 | reviewer list | Decision matrix + 6 required sections | ✓ structured |
| Tag v0.13.0 | Now | Now | ✓ now |
