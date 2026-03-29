---
phase: 03-convergence-polish
plan: 05
subsystem: cli
tags: [diff, convergence-tracking, git-aware, snapshot-comparison, graph-delta]

# Dependency graph
requires:
  - phase: 03-01
    provides: "Snapshot type, read_history, build_snapshot, JSONL I/O"
provides:
  - "anneal diff command with three reference modes (last snapshot, --days, git ref)"
  - "DiffOutput struct with structured graph-level change deltas"
  - "Git-aware structural diff via git archive + temp pipeline rebuild"
affects: [status-command, suggestion-engine]

# Tech tracking
tech-stack:
  added: []
  patterns: ["git archive piped to tar for ref extraction", "shell_escape helper for safe command interpolation"]

key-files:
  created: []
  modified:
    - src/cli.rs
    - src/main.rs

key-decisions:
  - "ObligationDelta fields keep _delta suffix with clippy allow for JSON schema clarity"
  - "Git ref extraction via git archive | tar instead of git show per-file (single subprocess, handles renames)"
  - "find_snapshot_by_days uses min_by_key on timestamp distance for closest match"
  - "Temp dir naming uses PID + epoch seconds instead of random (no rand dependency)"

patterns-established:
  - "Diff structs: separate delta types per domain (handles, states, obligations, edges, namespaces)"
  - "Graph reconstruction: load_config + build_graph + resolve_all + infer_lattice + run_checks + build_snapshot pipeline"

requirements-completed: [CONVERGE-04, CLI-08]

# Metrics
duration: 5min
completed: 2026-03-29
---

# Phase 3 Plan 05: Diff Command Summary

**Graph-level diff command with three modes: last snapshot, time-based (--days), and git-aware structural comparison via git archive**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-29T08:32:18Z
- **Completed:** 2026-03-29T08:37:25Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Implemented `anneal diff` with DiffOutput covering handle deltas, state changes, obligation deltas, edge deltas, and namespace deltas
- Three reference modes: default (last snapshot), `--days=N` (time-based), and positional git ref (structural diff)
- Git-aware mode extracts files at a ref via `git archive | tar`, runs full anneal pipeline, and diffs the resulting snapshots
- 7 new unit tests covering all diff detection scenarios and output formatting

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement DiffOutput and cmd_diff with snapshot-based and git-aware modes** - `2a194ca` (feat)
2. **Task 2: Wire Diff command into CLI dispatch in main.rs** - `fa2adde` (feat)

## Files Created/Modified
- `src/cli.rs` - DiffOutput, diff_snapshots, find_snapshot_by_days, build_graph_at_git_ref, cmd_diff, 7 tests
- `src/main.rs` - Diff variant in Command enum, match arm dispatching to cmd_diff

## Decisions Made
- Used `#[allow(clippy::struct_field_names)]` on ObligationDelta because the `_delta` suffix is meaningful in JSON output schema
- Git ref extraction uses `git archive | tar` piped through `sh -c` for single-command extraction
- find_snapshot_by_days returns closest snapshot by absolute timestamp distance, not the first one before the target
- Temp directory uses PID + epoch seconds for uniqueness without needing a rand dependency
- shell_escape helper wraps values in single quotes with proper escaping for safe shell interpolation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy struct_field_names pedantic lint**
- **Found during:** Task 1
- **Issue:** ObligationDelta fields all ended with `_delta` suffix, triggering clippy::struct_field_names
- **Fix:** Added `#[allow(clippy::struct_field_names)]` attribute since the suffix is intentional for JSON schema clarity
- **Files modified:** src/cli.rs
- **Verification:** `cargo clippy --all-targets` passes clean
- **Committed in:** 2a194ca (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Minimal -- clippy lint suppression for intentional naming.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Diff command complete, ready for status and map commands (Plans 03, 04)
- Snapshot infrastructure (Plan 01) fully consumed by diff
- All 44 tests passing, clippy clean

## Self-Check: PASSED

All files exist, all commits found, all acceptance criteria met.

---
*Phase: 03-convergence-polish*
*Completed: 2026-03-29*
