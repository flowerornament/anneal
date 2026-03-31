---
phase: 07-ux-enrichment
plan: 03
subsystem: ux
tags: [rust, clap, diagnostics, snapshots, cli]
requires:
  - phase: 07-01
    provides: suppression-aware check pipeline and external handle groundwork
provides:
  - file-scoped `anneal check --file=...` filtering after suppressions
  - heuristic terminal-state inference for zero-config init generation
  - temporal S003 pipeline stall detection with static fallback when no history exists
affects: [07-04, check, init, status, diff]
tech-stack:
  added: []
  patterns:
    - explicit snapshot context threaded into diagnostics instead of hidden global reads
    - heuristic terminal classification layered behind config and directory conventions
key-files:
  created:
    - .planning/phases/07-ux-enrichment/07-03-SUMMARY.md
  modified:
    - src/main.rs
    - src/parse.rs
    - src/lattice.rs
    - src/checks.rs
    - src/cli.rs
key-decisions:
  - "File scoping is applied after suppressions so terminal output and snapshots stay aligned with the same filtered diagnostic set."
  - "Temporal S003 compares the current level population with the most recent snapshot and only flags unchanged-or-growing levels, falling back to static edge analysis when no history is available."
patterns-established:
  - "Command handlers that need historical heuristics should read snapshot history once and pass `history.last()` into lower-level checks."
  - "Heuristic inference remains a fallback behind explicit config and stronger structural signals."
requirements-completed: [UX-05, UX-03, QUALITY-03]
duration: 4 min
completed: 2026-03-31
---

# Phase 07 Plan 03: File Scope and Temporal Stall Summary

**File-scoped diagnostics, smarter terminal status inference, and snapshot-aware pipeline stall detection for anneal check/init UX**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-31T02:39:47Z
- **Completed:** 2026-03-31T02:43:48Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Added `anneal check --file=...` so diagnostics can be scoped to one file after suppression rules have already been applied.
- Added terminal-name heuristics such as `superseded`, `archived`, and `completed` so zero-config lattice inference classifies obvious settled statuses automatically.
- Replaced purely static S003 stall reporting with snapshot-aware population comparisons while preserving the previous edge-based fallback for first-run corpora.

## Task Commits

Each task was committed atomically:

1. **Task 1: File-scoped check and terminal status heuristics** - `ba30fda` (feat)
2. **Task 2: Temporal S003 pipeline stall detection (RED)** - `a15ac20` (test)
3. **Task 2: Temporal S003 pipeline stall detection (GREEN)** - `6e97025` (feat)

## Files Created/Modified

- `src/main.rs` - Added the `--file` check flag, normalized file-path filtering, and threaded previous snapshot context into check/status/diff diagnostics.
- `src/parse.rs` - Added reusable terminal-status heuristic constants and helper.
- `src/lattice.rs` - Applied heuristic terminal inference after config and directory-based classification.
- `src/checks.rs` - Added temporal S003 logic, snapshot-aware suggestion wiring, and temporal/static fallback tests.
- `src/cli.rs` - Updated the snapshot-building test helper to match the new `run_checks` signature.

## Decisions Made

- Kept `--file` filtering in `main.rs` after suppression application instead of pushing it into individual check rules, so human output and stored snapshots reflect the same filtered set.
- Used the latest persisted snapshot as the temporal S003 reference point rather than aggregating multiple snapshots; this keeps the heuristic understandable and cheap for every command invocation.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated `cli.rs` test helper for the new snapshot-aware check signature**
- **Found during:** Task 2 (Temporal S003 pipeline stall detection)
- **Issue:** `just check` failed because an internal `cli.rs` test helper still called `run_checks` with the old 8-argument signature.
- **Fix:** Added the required `None` previous-snapshot argument to the helper so workspace tests compile against the new API.
- **Files modified:** `src/cli.rs`
- **Verification:** `just check` passed after the helper update.
- **Committed in:** `6e97025` (part of Task 2 GREEN commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** No scope creep. The extra change was required to keep the existing test harness compatible with the plan’s API change.

## Issues Encountered

- `rustfmt` and Clippy both required small follow-up adjustments during Task 2: formatting the new assertions and collapsing the lattice heuristic branch into a single condition. Both were resolved before the final quality gate run.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ready for `07-04-PLAN.md`; the remaining self-check plan can now rely on file-scoped diagnostics and temporal S003 behavior.
- `anneal check --file=anneal-spec.md` now scopes output to `anneal-spec.md`, and the current self-check blocker remains the real broken reference in `.design/anneal-spec.md` rather than tool UX gaps.

## Self-Check: PASSED

- Verified `.planning/phases/07-ux-enrichment/07-03-SUMMARY.md` exists on disk.
- Verified task commits `ba30fda`, `a15ac20`, and `6e97025` exist in git history.
