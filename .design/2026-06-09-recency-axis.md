---
status: current
locked: 2026-06-09
date: 2026-06-09
authors: [claude]
reviewed-by: codex (adversarial, 2026-06-09 — REVISE-THEN-LOCK → Option B)
bd: anneal-jkt4.2
relates:
  - 2026-06-09-dimensional-foundation.md          # the arc; recency is tangle #1
  - 2026-06-09-the-convergent-corpus-runtime.md    # the dimensional model
  - 2026-05-13-corpus-runtime.md                   # master spec — CR-D9/CR-D37 primitives, CR-D103 gate
  - 2026-06-08-currency.md                         # the exemplar: clarify an axis, features get simple
---

# Untangling the recency axis — 2026-06-09

**For codex adversarial review before any implementation.** This is the second
axis-clarification (currency/lifecycle was the first). It is *simulated-first* on
murail, and the simulation changed the design after the implementation option was
chosen — so the central question below is a genuine fork, not a rubber stamp.

## The tangle (evidence-backed)

"Recency" is **three sub-axes by source-of-truth**, currently laundered into one:

| sub-axis | question | primitive | source | CR-D37 semantics | prelude consumers |
|---|---|---|---|---|---|
| **authored-age** | how old is the writing? | `freshness(h,days)` | `*handle.date` | days since date; **0 for undated/unparseable** | search, context, orientation anchors, entropy, checks — load-bearing |
| **change-recency** | edited recently? | `changed_within(h,days)` | git_mtime | membership: git_mtime within N days | **only** the `recent_recency` undated branch |
| (base) | — | `git_mtime(file,instant)` | git commit time | latest commit ts for tracked file | **zero prelude consumers** |
| **history-movement** | converging? | `flux(h,days,delta)` | snapshots | status transitions in window | one rule (stalled detection) |

**The conflation** (`orientation.dl:132–155`): `recent_recency(h, days)` emits a
single integer that means **authored-age** (`freshness`) for dated handles but
**change-recency** for undated ones — mapped onto the same `days` scale via magic
buckets (45/90/180/365). Two clocks, one ordering key. That's the "retire
git_mtime as an age proxy" target, and it's a CR-D103 violation in spirit:
change-recency wearing authored-age's authority.

## The simulation finding that changes everything (murail, 2026-06-09)

- murail is **85% dated** (397/468 file handles); 71 undated.
- **0 of 71 undated handles surface in `recent_frontier`** today — the magic-bucket
  branch is *inert* on date-rich corpora. It only bites on date-poor corpora
  (the anneal-code direction).
- **git_mtime is a degraded oracle here: 406/468 files (87%) share ONE identical
  commit timestamp** (`2026-05-29T22:09:28`) — a bulk import/checkout commit.
  "343 undated handles changed within 7d" is this artifact, not real edits.

**Implication:** the magic buckets (45/90/180/365) were crudely *damping git-mtime
noise*. A naive "real change_age in days" primitive would feed that noise straight
into the frontier — promoting dozens of bulk-touched undated files to the top.

## Decision: Option B (locked, codex-reviewed)

The arc decision started as **"add a real `change_age(h,days)` primitive"** (chosen
before the degraded-oracle finding). The murail simulation undercut it and codex's
adversarial review confirmed the reversal: **Option B — subtractive axis
clarification, no new primitive.** A precise day-scale `change_age` over git
metadata that is 87% one bulk-commit timestamp would look as authoritative as
`freshness` while being mostly noise — a CR-D103 trust-invariant violation (false
precision over a degraded oracle). `changed_within` already exposes the lower-level
git membership oracle, so a second primitive is more surface than truth.

**Option A is deferred, not dead:** it becomes right only when anneal has a
*measured* corpus where git history is a trustworthy per-file edit oracle (real
commit times, not a bulk import) — and even then it ships as `change_age` **+
confidence/disposition**, never a bare day-scale peer of `freshness`.

## The sub-axes, declared (the deliverable)

| sub-axis | predicate | disposition | source / authority | degenerate input (CR-D103) |
|---|---|---|---|---|
| **authored-age** | `authored_age(h, days)` *(new derived wrapper)* | REPORT | `*handle.date`, clean oracle | undated → **no row** (the wrapper guards `date != null`) |
| **change-recency** | `changed_within` + a coarse `changed_recently(h, band)` | REPORT, **explicitly lower authority** | git_mtime, degraded | bulk-touched git → coarse band only, never authored-age scale |
| **history-movement** | `flux` | TREND | snapshots | no history → no baseline |

## Implementation plan (locked — byte-identical + perf gated)

1. **`authored_age` derived wrapper** — `authored_age(h, days) := *handle{id: h,
   date: date}, date != null, freshness(h, days).` This is the *named* guard
   (replaces "documented mandatory guard in prose"). **Do NOT change `freshness`'s
   sealed CR-D37 `0-for-undated` behavior** — `entropy("freshness_decay")` and
   `abandoned_stale_member` consume the row-emitting form directly; changing it is a
   separate, separately-simulated migration, not this slice. Migrate authored-age
   consumers onto the wrapper deliberately, starting with `recent_frontier`.
2. **Replace `recent_recency`'s overloaded integer with two predicates** — an
   authored-age term (via the wrapper, date-backed, dominant) and a coarse
   git-backed change-recency band (lower authority). `recent_frontier_candidate`
   composes them; **authored-age dominates**, change-recency only lets *undated*
   files enter a lower-authority lane as a small flat/coarse boost — never as fake
   authored days. The existing 45/90/180/365 buckets are acceptable as the
   transitional damped signal, but must no longer be *named or printed* as authored
   recency.
3. **Keep `recent_frontier(h, rank, recency)` public arity/output stable** this
   slice. Add `describe`/doc text *before* any surface schema change. Murail stays
   byte-identical because undated rows do not enter the frontier and the dated path
   is unchanged.
4. **`git_mtime` stays** — keep it as the raw exact-timestamp inspection primitive
   and the base for `changed_within`; remove only the *framing* that it is an
   age/currency oracle. (It is NOT zero-consumer in the meaningful sense:
   `changed_within` consumes the git_mtimes internally and `describe runtime` teaches
   it.) No primitive removal.
5. **Residual compat cleanup, where mechanical and safe** — `recent` is already
   retired from the user schema (analysis returns the retired-alias teaching error);
   this is internal/doc cleanup only, not a user-visible removal. Rename internal
   `recent_tuples` → `changed_within_tuples` (pure clarity, no semantic churn).
6. **Update `describe`** for `recent_frontier`, `git_mtime`, `changed_within` so git
   mtime reads as "raw exact commit timestamp / lower-authority change signal," not
   authored age.

## Acceptance (deterministic, gated)
- **Byte-identical on murail** for `recent_frontier`/`status`/`context`/`search`
  (dated path unchanged; undated rows do not enter the frontier) — **and perf-gated**
  (byte-identical-misses-perf lesson). No public arity/schema change to
  `recent_frontier` in this slice.
- The three sub-axes are named predicates/docs with explicit dispositions; no rule
  puts change-recency on the authored-age `days` scale.
- `describe` for the three updated predicates reflects the honest framing.
- `anneal check` on `.design` clean; the prelude stays its own witness.

## Explicitly NOT in this slice (codex-confirmed)
- No `change_age` primitive (deferred to a measured trustworthy-git corpus, with
  disposition).
- No sealed `freshness` behavior change to no-row.
- No `git_mtime` primitive removal.
- No broad rewiring of all `freshness` consumers until `authored_age` is in place and
  separately simulated.
- No public arity/schema change to `recent_frontier` under a byte-identical gate.
