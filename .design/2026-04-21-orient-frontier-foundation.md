---
status: draft
date: 2026-04-21
depends-on:
  - anneal-spec.md
  - 2026-04-15-areas-orient-garden.md
  - 2026-04-17-cli-ux-audit-v2.md
---

# Orient: Frontier + Foundation

## Motivation

`anneal orient` is the "help me start reading" command. It currently
ranks files by `edge_weight × log(in-degree) + recency_bonus +
status_bonus`, clamped by a content-size factor in 0.9.3-unreleased.
Cross-corpus testing on `~/code/murail/.design` and
`~/code/herald/.design` exposed a structural mismatch between that
formula and what orientation actually needs.

- **Murail (12.6k handles):** the top-7 surfaced `DESIGN-GOALS.md`,
  then `synthesis/2026-03-15-architectural-reconciliation.md` (a pre-
  v17 reconciliation that the corpus has since moved past),
  `synthesis/2026-03-15-spike-results.md`, and two tiny directory
  READMEs. It missed the corpus's own `README.md`, its `CHANGELOG.md`,
  the `v17-convergence-synthesis.md` that anchors all April 2026 work,
  and every April 2026 implementation landing. The newcomer would
  build an outdated mental model.

- **Herald (smaller, denser):** the tool surfaced `system-theory.md`
  (March 24 axioms) and `herald-architecture.md` (April 8 canonical
  architecture) correctly, but missed `synthesis/2026-04-15-corpus-
  map.md` (purpose-built as "read this first"), `architecture/2026-04-
  10-herald-architecture-synthesis.md` (frontmatter says "Entry point.
  Read this first, follow references"), the April 11 implementation
  plan, and the April 21 session-handoff. Two tiny files (208 bytes
  and 578 bytes) leaked through the size penalty.

The two misses tell the same story: the algorithm finds **old stable
hubs** but misses **the current frontier** and the **curated entry
points** maintainers wrote on purpose. Raw centrality treats "heavily
cited by anyone, ever" as equivalent to "heavily cited by the current
frontier" — but the corpus has moved, and citations from March 2026
shouldn't carry the same weight as citations from April 2026 when
orienting a reader today.

## Goal

> Orient surfaces **the current frontier** and the **stable foundation
> the frontier still depends on**. Everything else — resolved work,
> superseded hubs, redirect stubs, archived history — stays out of the
> reading list.

Two partitions, not one score. "Frontier" is where work is now;
"Foundation" is what it rests on. The newcomer reads both and converges
on the current shape of the project.

## Current state

### Ranking

```
score(file) = 
    edge_weight  * (2·ln(in + 1) + ln(out + 1))
  + label_weight * ln(labels_by_this_file + 1)
  + recency_weight * 0.5^(age_days / half_life)
  + status_bonus(status)
  * content_factor(size_bytes)   # 0 if size ≤ 500, linear to 1 at 2500
```

`content_factor` was added in 0.9.3-unreleased to fix the 200-byte
alias-stub leakage; it partially works but leaves stubs at 500–1000
bytes alive.

### Tier pipeline

```
Pinned  → EntryPoint  → Upstream  → Downstream
```

Each tier fills until its candidates run out or budget is exhausted.
`EntryPoint` is the single mixed tier we're splitting into Frontier +
Foundation.

### What goes wrong

1. **Old hubs outrank current hubs.** In-degree doesn't care when the
   citer was written. A March 2026 synthesis that's superseded by a
   later synthesis still accumulates centrality from its pre-April
   citers. Recency applies only to the cited file's own date, not to
   the citer.

2. **Curated hubs are invisible to the algorithm.** `README.md`,
   `CHANGELOG.md`, `synthesis/README.md`, `corpus-map.md`, and any
   file whose frontmatter literally says `purpose: Entry point. Read
   this first.` score as ordinary files. The maintainer's explicit
   orientation signal is being ignored.

3. **Status penalties are too soft.** `status: superseded` gets 0.3 of
   status bonus vs 2.0 for active — a 1.7-point difference that
   centrality (~5-10 points) dwarfs. A superseded file with many
   incoming edges still ranks high.

4. **Frontmatter `superseded-by` is ignored.** When a file explicitly
   declares its replacement, the tool treats both as independent
   candidates and may surface the replaced one.

5. **Size penalty is too soft for the middle band.** 500–1000 byte
   files (directory READMEs, fixed-bug notes) still survive with
   meaningful attenuated scores.

## Design

### Output: two tiers, explicit

Tier order (top-to-bottom in output):

```
Pinned → Frontier → Foundation → Upstream → Downstream
```

Frontier first: agents are usually resuming or working, not
onboarding. Current work goes at the top of the reading list.
Foundation is context the reader returns to as they work through
Frontier.

```
Frontier (3)
where work is now, ranked per area

  implementation/2026-04-20-jz1v-arc-landing.md        14k
      Landed the jz1v arc on master; compound-cell lowering…
  synthesis/2026-03-25-v17-convergence-synthesis.md    20k
      …

Foundation (4)
stable hubs the frontier still cites

  README.md                                            3k
      …
  CHANGELOG.md                                         6k
      …
  synthesis/README.md                                  2k
      …
  DESIGN-GOALS.md                                      20k
      …
```

Pinned, Upstream, Downstream tiers stay.

### Tier assignment

A file is assigned to **exactly one** tier by category, not by score:

- **Frontier** = file passes hard filters AND
  - status is in the active-like set
    (`active|draft|current|in-progress|plan|complete|open|proposed`), AND
  - date is within the per-area top-K newest (K=1 in global mode,
    K=budget-bounded in area mode)
- **Foundation** = file passes hard filters AND is not Frontier.
  Includes curated hubs (always-surface) + cited-by-recent-work
  stable docs.

Categorical assignment beats score-vs-score because the research
showed frontier and foundation measure different things; a single
scalar conflating them is what produced the current bug.

### Ranking within each tier

**Frontier:** per-area, newest active file by date. Ties broken by
recency-weighted in-degree (so a draft cited by the frontier beats an
orphaned draft of the same date). The tier is the union of per-area
picks, ordered by date desc.

**Foundation:**

```
foundation_score(file) =
    curated_hub_bonus(file)                  # flat bonus, see below
  + recency_weighted_in_degree(file)         # Σ over in-edges:
                                             #   0.5^(citer_age / half_life)
  + label_weight * log(labels + 1)
  + status_bonus(status)                     # stable: 1.0, living: 1.0,
                                             #   active: 0.5 (pulled by
                                             #   Frontier anyway)
```

The critical change: **`recency_weighted_in_degree`** weights each
incoming citation by the recency of the **citer**, not the cited file.
A March-2026 file cited by 20 April-2026 docs scores highly; a March-
2026 file cited by 50 February-2026 docs (pre-frontier) scores low.

### Curated hub detection

Curated hub detection leans on **basename** because agents and humans
alike forget to add frontmatter cues. The corpus must work with zero
annotation; frontmatter is a nice-to-have tiebreaker.

A file is a curated hub if **any** of:

1. Basename (case-insensitive) matches:
   `README`, `CHANGELOG`, `DESIGN-GOALS`, `OPEN-QUESTIONS`, `LABELS`,
   `INDEX`, `VERSIONS`, `ROADMAP`, `OVERVIEW`, `TOC`, `MANIFEST`,
   `GLOSSARY`. At any depth. This is the primary signal.
2. `status: living` (murail's convention for "authoritative,
   maintained" — cheap extra signal).
3. Frontmatter `purpose:` contains any of: "entry point",
   "read first", "read this first", "orientation", "overview",
   "map", "starting point", "guide". Case-insensitive substring.

No body-text parsing, no H1 inspection, no prose heuristics. Three
cheap predicates. If a maintainer wants to promote a non-obviously-
named doc to curated-hub status, they add a one-line `purpose:`
entry. Zero-annotation corpora still get the basename set.

`curated_hub_bonus` is a flat additive term sized to compete with
heavy centrality. Default weight `10.0` (configurable via
`[orient].curated_hub_weight`).

### Hard filters

A file is **excluded from orient output entirely** if any of:

1. Status is in the terminal set:
   `{superseded, archived, historical, prior, incorporated, digested,
   resolved, retired, deprecated, obsolete}`
2. Frontmatter has `superseded-by: X` (redirect target; the
   replacement is what we want)
3. Size < `stub_bytes` (raised from 500 to 1000 bytes) AND the file
   is not a curated hub
4. Body starts with a detected redirect pattern: first non-blank line
   after frontmatter is a "See" link, "Moved to ..." sentence, or
   "Historical alias" header — optional; implement only if needed
   after corpus validation
5. Matches `[orient].exclude` glob (existing)

These replace the current soft-penalty approach. The current
`content_factor` function is retired; size becomes a filter, not a
multiplier.

### Frontier detection

- **`--area=X` is set:** frontier = top-K files in X by date (newest
  first) that pass the hard filters and have an active-like status.
  K bounded by what fits in the budget.
- **`--area` not set and corpus has subdirectories:** treat each
  top-level directory as an area (or use `config.areas` if set). For
  each area, take the single newest active file. The Frontier tier
  is the union of per-area champions, ordered by date desc.
- **`--area` not set and corpus is flat** (e.g., `anneal/.design/`):
  Frontier = top-K globally by date among active files. No per-area
  partition to make.

### Foundation detection

Foundation candidates: every filtered file not in Frontier. Rank by
`foundation_score`, greedy-fill by budget. Curated hubs rise to the
top naturally because of the `curated_hub_bonus`.

In area-scoped mode (`--area=X`), Foundation draws from two sources:

- **Global hubs** (always): curated-hub files at the corpus root, even
  if outside X. A newcomer touching `compiler/` still needs the
  project `README.md` and `DESIGN-GOALS.md`.
- **Area-local**: files in X ranked by area-local recency-weighted
  in-degree.

These merge into one ranked list; greedy fill takes the top.

### Budget handling

Keep greedy fill in tier order (`Pinned → Frontier → Foundation →
Upstream → Downstream`). No budget-share allocation between tiers;
agents decide what to fetch based on what's in the output.

The output already displays token counts per entry — that's the raw
information the agent needs. No smart allocation; the agent can re-
run with a wider budget or a narrower area scope if the current
result under-serves.

### Overflow

When a candidate's token cost exceeds the remaining budget, emit it
as a path-and-tokens line **without a snippet**, in an inline
"Overflow" sub-list attached to its tier:

```
Foundation (4)
stable hubs the frontier still cites

  README.md                     3k
      …snippet…
  CHANGELOG.md                  6k
      …snippet…

  Overflow (2)
  too large for current budget; re-run with larger --budget
    DESIGN-GOALS.md             20k
    v17-convergence-synthesis.md 20k
```

The agent sees paths + token counts for the rest and can decide to
expand budget, fetch specific files via `anneal get`, or skip.
No snippet expansion for overflow entries — that's the cost-saving
move.

The existing `dropped_tiers` mechanism stays for the case where a
whole tier's candidates are empty.

### Config surface

Minimal additions. Most of the policy lives in code defaults —
tweakable via TOML only when a corpus needs it, not by default.

```toml
[orient]
# Existing fields stay. edge_weight retained for label-weight
# computation; recency_weight keeps its current role.
edge_weight = 1.0
label_weight = 1.0
recency_weight = 5.0
recency_half_life_days = 90
budget = "50k"
depth = 3
pin = []
exclude = []

# New, defaulted — most users never set these.
stub_bytes = 1000                # hard filter: files below this are
                                 # excluded unless they're a curated
                                 # hub (basename match, purpose:, or
                                 # status: living).
curated_hub_weight = 10.0        # additive bonus for curated hubs.
```

**What's NOT configurable:**

- The active-status set and terminal-status set are hardcoded from the
  `HandleStatus` vocabulary. A per-corpus override is conceivable but
  not needed for the release — both murail and herald use standard
  status names. Revisit if a corpus breaks the convention.
- The curated-hub basename list is hardcoded. If someone names their
  entry point `GUIDE.md`, they can add `status: living` or a
  `purpose:` line — cheap to annotate, removes the need for config.

Zero config still works. Every new default is safe for corpora that
don't annotate anything — the basename signal covers the common case.

### Frontmatter extension

No new parsing. `purpose:` already lives in Handle metadata via
`HandleMetadata::purpose` (or equivalent — confirm at implementation
time). `superseded-by:` needs to be surfaced — check whether
`HandleMetadata` already carries it; if not, add it as an optional
field that the parser fills from frontmatter and the ranker checks.

### Output tier labels

```
Frontier (N)     where work is now
Foundation (N)   stable hubs the frontier still cites
Upstream (N)     dependencies outside this area
Downstream (N)   consumers outside this area
Pinned (N)       always-included context
```

Captions are the subtitle under each heading. Matches Round 2/3 UX
rule R2 (heading + count).

## Scenarios

Sanity-checked against four realistic agent calls. Each expected
output is paper-simulated; actual behavior to be verified at
implementation time.

### S1. Fresh agent onboarding to murail (no flags, budget=50k)

```
anneal --root ~/code/murail/.design orient
```

Expected Frontier: 4-6 files, one per active area.
- `implementation/2026-04-20-jz1v-arc-landing.md` (complete, 14k)
- `implementation/2026-04-20-session-service-architecture.md` (active)
  — but only one per area; if jz1v is pulled as implementation top-1,
  session-service falls to Foundation or Overflow.
- `compiler/<newest-active>.md`
- `language/<newest-active>.md` if any
- possibly `synthesis/<newest-active>.md`

Expected Foundation, in `foundation_score` order:
- `README.md` (curated hub, ~3k)
- `CHANGELOG.md` (curated hub, ~6k)
- `DESIGN-GOALS.md` (status:living + curated-like hub, ~20k — may be
  Overflow if budget tight)
- `synthesis/README.md`, `formal-model/README.md`,
  `implementation/README.md` (all curated)
- `synthesis/2026-03-25-v17-convergence-synthesis.md` (heavy recency-
  weighted in-degree from April files)

Overflow: DESIGN-GOALS and v17-convergence when their token cost
exceeds remaining budget — path + size line, no snippet.

### S2. Area-scoped work on murail compiler

```
anneal --root ~/code/murail/.design --area=compiler orient --budget=30k
```

Expected Frontier: newest active files in `compiler/` (top-K by date).
Expected Foundation:
- Global curated hubs: `README.md`, `DESIGN-GOALS.md`, etc.
- Area-local: compiler-internal files with high recency-weighted
  in-degree within compiler/.

Expected Upstream/Downstream: boundary files from other areas linked
to compiler. Unchanged from current behavior.

### S3. Agent resuming on herald

```
anneal --root ~/code/herald/.design orient
```

Expected Frontier: `proposals/2026-04-21-session-handoff.md`
(proposals-area newest active), plus one newest per other active
area. Frontier leads the output so the resuming agent sees the handoff
doc first.

Expected Foundation: `synthesis/2026-04-15-corpus-map.md` (curated via
`purpose: entry point`), `architecture/2026-04-10-herald-architecture-
synthesis.md` (curated via frontmatter), `theory/2026-03-24-system-
theory.md` (heavy recency-weighted in-degree from April docs).

### S4. Tight budget on anneal's own corpus

```
anneal --root ~/code/anneal/.design orient --budget=10k
```

Flat corpus (no subdirs), so per-area frontier collapses to top-K
globally. Expected Frontier: `2026-04-21-orient-frontier-foundation.md`
(this doc), `2026-04-17-cli-ux-audit-v2.md`.

Foundation: `anneal-spec.md` has heavy recency-weighted in-degree but
is 14k — likely Overflow at 10k budget. The agent sees it in Overflow
with its token count and can re-run with `--budget=30k` to include it.

## Non-goals

- **Not an area-inference system.** If `config.areas` is empty we fall
  back to top-level directory. A smarter area detector is a separate
  project.
- **Not a semantic-similarity engine.** No embedding, no LLM calls.
  Graph + frontmatter signals only.
- **Not a history-aware ranker.** We don't look at git log or snapshot
  history for orient. Recency comes from the file's own `date:` or
  filename prefix.
- **Not a personal-history ranker.** No "files you recently touched"
  notion — orient is stateless across invocations.

## Test plan

### Unit tests (new)

- `frontier_score_respects_recency`: a file dated today outranks one
  dated 180 days ago given same status.
- `frontier_excludes_terminal_status`: a file with `status: archived`
  is absent even if recent and cited.
- `foundation_score_weights_recent_citations`: A cited by 10 March
  files vs B cited by 10 April files → B scores higher.
- `curated_hub_bonus_applies_by_basename`: `foo/README.md` gets the
  bonus; `foo/readme-spec.md` does not.
- `curated_hub_bonus_applies_by_purpose`: frontmatter `purpose: Entry
  point. Read this first.` triggers bonus.
- `superseded_by_filter_excludes_source`: file with `superseded-by:
  X` is absent from output; X remains eligible.
- `stub_bytes_is_hard_filter`: 800-byte non-curated file is absent;
  800-byte `README.md` is present.
- `per_area_frontier_picks_newest_per_area`: two areas each with 5
  active files yields one frontier per area.

### Integration tests (corpus-level)

- `orient_anneal_corpus_surfaces_spec_plus_recent_audits`: confirm
  `anneal-spec.md` and the 2026-04 audits lead.
- `orient_murail_corpus_hits_readme_and_changelog`: confirm `README.md`
  and `CHANGELOG.md` appear in Foundation and that `2026-04-20`
  implementation files lead Frontier.
- `orient_herald_corpus_surfaces_corpus_map`: confirm `synthesis/2026-
  04-15-corpus-map.md` and `architecture/2026-04-10-herald-
  architecture-synthesis.md` appear.
- `orient_murail_no_alias_stubs`: confirm none of
  `spec/SPEC.md`, `compiler/prior/bytecode-format-v1.md`,
  `spec/decisions-spec-v08.md`, `references/transcripts/2026-03-29-
  MANIFEST.md` appear.

Herald and murail tests use `#[ignore]` (skipped if corpus unavailable)
like the existing `test_murail_corpus` integration.

### Manual validation

Run on all three corpora before release, eyeball top-7. Acceptance:

- anneal/.design: `anneal-spec.md` leads Foundation; recent CLI-UX
  audits lead Frontier.
- murail/.design: `README.md` + `CHANGELOG.md` + `DESIGN-GOALS.md`
  + `synthesis/README.md` in Foundation; `2026-04-20` implementation
  files in Frontier; no superseded-v17 docs; no alias stubs; no sub-
  1k READMEs.
- herald/.design: `synthesis/2026-04-15-corpus-map.md` +
  `architecture/2026-04-10-herald-architecture-synthesis.md` lead
  Foundation; April 2026 proposals/specs lead Frontier; 208B review
  note and 578B use-case stub absent.

## Self-documenting surfaces

Every agent interaction with anneal is a chance to teach the
philosophy. The Frontier/Foundation change touches five self-
documenting surfaces, each with a specific teaching role. These are
**part of the design**, not an afterthought — an agent that only ever
reads CLI help should still converge on the right mental model.

### What we're teaching (the philosophy, in four beats)

1. **Curation beats centrality.** Humans name `README.md`,
   `CHANGELOG.md`, `DESIGN-GOALS.md` on purpose. The tool respects
   that intent as a strong signal; graph centrality is secondary.
2. **Status is a contract.** `status: active|draft|current|in-
   progress|...` means "include me"; `status: superseded|archived|
   historical` means "exclude me"; `status: living` means
   "authoritative and maintained." Agents who annotate get better
   output.
3. **Frontier + Foundation, not one big pile.** Current work leads,
   stable context follows. Reading order matches working order.
4. **Redirects are not content.** Tiny alias stubs (`superseded-by: X`,
   200-byte "Historical alias" files) are filtered out entirely.
   They're graph plumbing, not reading material.

Each surface below reinforces one or more of these beats.

### `anneal prime` / `skills/anneal/SKILL.md`

Primary teaching surface. Agents run `anneal prime` at session start
and get the canonical briefing. Update the following blocks:

- **"First moves"** — update the onboarding recipe:
  - `anneal status --json --compact` (unchanged)
  - `anneal orient` (now emits Frontier + Foundation; explain both)
  - `anneal orient --area=X` (for scoped work — mention that Foundation
    still surfaces global curated hubs)
- **"Command map"** — change orient's one-line description from
  "context-budgeted reading list" to "**Frontier** (where work is now)
  + **Foundation** (stable hubs). Per-area in global mode; area-scoped
  when `--area=X`. Curated hubs (`README`, `CHANGELOG`, `DESIGN-GOALS`,
  etc.) always surface."
- **New section: "Annotation conventions"** — short callout:
  - `status: active|draft|current|complete|...` → Frontier-eligible
  - `status: living` → always in Foundation (authoritative hub)
  - `status: superseded|archived|historical` → excluded
  - `superseded-by: <path>` → excluded; `<path>` is the replacement
  - `purpose: "read this first"` → Foundation curated-hub bonus
- **Agent rules** — add one line: "When creating a doc that obsoletes
  another, set `status: superseded` and `superseded-by: <new-path>` on
  the old file. The corpus stops surfacing the redirect the moment you
  save."

This is where the *minimum viable annotation* contract lives — agents
who read SKILL.md learn the 5-key vocabulary and can maintain it.

### `anneal orient --help`

Short `about` stays surgical: ~10 words summarizing the command.

Long description (the `long_about`) is where agents discover signals
without needing to read SKILL.md:

```
Context-budgeted reading list for onboarding or resuming.

Output is split into tiers:

  Frontier    Where work is now. Per-area newest file with an
              active-like status. In area mode (--area=X), all
              area files by date.
  Foundation  Stable hubs the frontier still cites. Curated hubs
              (README, CHANGELOG, DESIGN-GOALS, INDEX, ...) always
              surface. Cited-by-recent-work files surface by
              recency-weighted in-degree.
  Pinned      User-configured always-include files (see
              [orient].pin).
  Upstream    In area mode, boundary files that the area cites.
  Downstream  In area mode, boundary files that cite the area.

Each row shows token count. Overflow entries (too large for budget)
show path + size only.

Curated-hub detection: basename match (README/CHANGELOG/
DESIGN-GOALS/OPEN-QUESTIONS/LABELS/INDEX/...) OR status: living OR
frontmatter `purpose:` containing "entry point"/"read first"/
"overview".

Filtered out entirely: status in {superseded, archived, historical,
prior, incorporated, digested, resolved, retired, deprecated,
obsolete}, files with `superseded-by:` frontmatter, and files
below stub_bytes (default 1000) that aren't curated hubs.
```

An agent that types `anneal orient --help` once should have enough
context to stop asking "why is X missing" or "why does Y appear."

### `README.md`

The orient section needs:

- Updated sample output showing both Frontier and Foundation tiers.
- A one-paragraph "How it picks" explaining curation-beats-centrality.
- The annotation vocabulary table (status values and their effect,
  `purpose:`, `superseded-by:`). Same table as SKILL.md — single
  source of truth; link SKILL to README's table.

Agents reading the README first (the common case when exploring
anneal as a new tool) learn the philosophy before they ever run the
command.

### `CHANGELOG.md`

Unreleased entry must read as a *why this changed* narrative, not a
changelog of internal refactors:

```
### Changed

- `anneal orient` output redesigned around two tiers: Frontier
  (where work is now, per-area newest active file) and Foundation
  (stable hubs the frontier still cites). Rankings now weight each
  incoming citation by the recency of the *citer*, so old hubs
  cited only by pre-frontier material stop dominating the top.
  Curated hubs (README, CHANGELOG, DESIGN-GOALS, INDEX, OPEN-
  QUESTIONS, and files with `status: living` or a `purpose:` line
  mentioning "entry point") receive an explicit bonus — human-
  curated orientation cues outrank graph-centrality guesses.

  Hard filters now exclude files with `status: superseded|archived|
  historical|prior|incorporated|digested|resolved|retired|
  deprecated|obsolete`, files with `superseded-by:` frontmatter
  (the replacement wins), and files below `stub_bytes` (default
  1000) that aren't curated hubs. The previous soft content-size
  penalty is retired.

  The `EntryPoint` tier in `--json` is renamed to `Foundation` and
  a new `Frontier` tier is added; downstream JSON consumers must
  update.
```

### `config.toml` defaults + inline config docs

Config comments already serve as a tutorial when someone opens
`anneal.toml` for the first time. For every new field, the comment
must say (a) what it does, (b) what happens at the default, (c) when
to tweak. Example:

```toml
[orient]
# Files smaller than this are treated as stubs and excluded from
# orient unless they're curated hubs (README, CHANGELOG, etc.).
# Raise if your corpus has legitimately short standalone specs;
# lower if redirect stubs leak into Foundation.
stub_bytes = 1000

# Additive bonus for curated hubs in Foundation. Sized to compete
# with heavy centrality — a README should beat a 50-citation hub
# by default. Raise if curated hubs still get outranked.
curated_hub_weight = 10.0
```

### Cross-reference

- SKILL.md → points to `anneal orient --help` for full tier docs
- `--help` → points to README for annotation vocabulary table
- README → points to this design doc for rationale
- CHANGELOG → points to README for "how it picks"

No surface repeats the full explanation; each shows the shape needed
at that level of zoom. An agent can drill from `prime` to `--help`
to README to design without losing the thread.

## Migration

### Breaking changes to watch

- JSON shape of `anneal orient --json`: the `EntryPoint` tier gets
  renamed to `Foundation` and a new `Frontier` tier is added.
  Agents parsing the JSON need updates. Document in CHANGELOG; bump
  minor version.
- SKILL.md, README.md, orient `--help`, CHANGELOG — see
  "Self-documenting surfaces" above. All update in the same commit
  as the tier split (step 7) so the docs never drift from the
  behavior.

### Non-breaking

- Existing `[orient]` config fields keep their meaning.
- `--area`, `--file`, `--budget`, `--paths-only` flags unchanged.
- Pinned tier unchanged.
- Humans relying on the tool's visual output get a clearer surface
  (tiers are explicit) without losing any information they had before.

## Execution sequencing

1. **Design sign-off** (this doc).
2. **Frontmatter plumbing.** Confirm `purpose:` and `superseded-by:`
   are in `HandleMetadata`; add what's missing. Single commit.
3. **Hard filters.** Replace `content_factor` with a boolean filter;
   add status-set filter; add `superseded-by` filter. Tests for each.
   Single commit.
4. **Curated hub detection.** Basename and frontmatter-purpose
   signals. Tests. Single commit.
5. **Recency-weighted in-degree.** New scoring helper + tests.
   Separate commit from the tier change.
6. **Per-area frontier detection.** New helper that partitions by
   area and picks top-K per area. Tests.
7. **Tier split.** Rename `EntryPoint` → `Foundation`, add `Frontier`,
   update `add_tier` flow. Update JSON schema. Update output labels.
   Tests including a cross-corpus snapshot. **Same commit** updates
   `anneal orient --help` long description so the CLI stops describing
   the old behavior the moment the new behavior ships.
8. **Self-documenting surfaces.** Update SKILL.md (first moves +
   command map + annotation conventions), README.md (orient section +
   sample output + annotation vocabulary table), CHANGELOG.md
   (Unreleased entry written as *why this changed*), and inline config
   comments in the default `anneal.toml` scaffold. Doc commit.
9. **Release bump.** `just release-bump 0.10.0` + `just release-
   verify` + tag.

**Co-commit policy:** `--help` text lives in-source (`clap` macros),
so step 7 always lands with updated help. Step 8 bundles the
external-surface docs. No PR ships with old sample output or stale
"orient surfaces entry points by centrality" copy.

Target: one focused session per step, atomic commits.

## References

- Research agent reports (Herald and Murail), 2026-04-21.
- Current impl: `src/cli/orient.rs`.
- Related spec: `2026-04-15-areas-orient-garden.md` (area model).
- Round 3 UX: `2026-04-17-cli-ux-audit-v2.md` (R2 heading pattern).
