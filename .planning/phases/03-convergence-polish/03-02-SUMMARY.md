---
phase: 03-convergence-polish
plan: 02
subsystem: checks
tags: [suggestions, diagnostics, graph-queries, cli-filters, S001-S005]

# Dependency graph
requires:
  - phase: 03-01
    provides: Severity::Suggestion variant in checks.rs
provides:
  - Five suggestion rules S001-S005 as graph queries in checks.rs
  - run_suggestions() entry point collecting all five suggestion types
  - CheckFilters struct with --suggest, --stale, --obligations filter flags
  - cmd_check enhanced with diagnostic filtering per D-19
affects: [03-03, 03-04, 03-05]

# Tech tracking
tech-stack:
  added: []
  patterns: [BTreeMap for deterministic namespace iteration, CheckFilters struct for clean bool grouping]

key-files:
  created: []
  modified:
    - src/checks.rs
    - src/cli.rs
    - src/main.rs

key-decisions:
  - "CheckFilters struct encapsulates four boolean filter flags to satisfy clippy pedantic (fn_params_excessive_bools, too_many_arguments)"
  - "Suggestions placed in checks.rs alongside existing rules per D-18 (not separate suggest.rs -- not large enough to warrant it)"
  - "BTreeMap for namespace grouping in S004 ensures deterministic output order"
  - "S004 checks both terminal status AND freshness via compute_freshness (per KB-E8 frozen >N days criterion)"
  - "S005 limits to top 5 co-occurring pairs to avoid noise"

patterns-established:
  - "CheckFilters struct pattern: group related bools into struct to avoid clippy pedantic warnings"
  - "Suggestion diagnostic codes: S001-S005 for graph-structural suggestions"

requirements-completed: [SUGGEST-01, SUGGEST-02, SUGGEST-03, SUGGEST-04, SUGGEST-05]

# Metrics
duration: 6min
completed: 2026-03-29
---

# Phase 3 Plan 02: Suggestion Engine & Check Filters Summary

**Five graph-structural suggestion rules (S001-S005) detecting orphaned handles, candidate namespaces, pipeline stalls, abandoned namespaces, and concern group candidates, plus --suggest/--stale/--obligations filter flags on check command**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-29T08:32:19Z
- **Completed:** 2026-03-29T08:38:03Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Five suggestion functions as pure graph queries (no content heuristics, per KB-P5)
- S004 abandoned namespace detection uses both terminal status AND freshness threshold (compute_freshness with FreshnessLevel::Stale)
- Check command enhanced with filter flags per spec section 12.1 / D-19
- 12 new suggestion tests (TDD), all 49 tests passing, clippy clean
- Verified against Murail corpus: suggestions, staleness, and obligation filters all produce correct output

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement five suggestion rules in checks.rs**
   - `a46edb8` (test) - TDD RED: 12 failing tests for S001-S005
   - `e6857c1` (feat) - TDD GREEN: implement all five suggestion functions + run_suggestions
2. **Task 2: Add filter flags to check command** - `59c7e2e` (feat)

## Files Created/Modified
- `src/checks.rs` - Five suggestion functions (suggest_orphaned, suggest_candidate_namespaces, suggest_pipeline_stalls, suggest_abandoned_namespaces, suggest_concern_groups), run_suggestions entry point, 12 new tests
- `src/cli.rs` - CheckFilters struct, cmd_check updated with diagnostic filtering per D-19
- `src/main.rs` - Command::Check variant gains suggest/stale/obligations flags

## Decisions Made
- Used CheckFilters struct instead of four boolean parameters to satisfy clippy::fn_params_excessive_bools and clippy::too_many_arguments pedantic lints
- Kept suggestions in checks.rs (not separate suggest.rs) since they're five small functions following the same Diagnostic pattern
- S004 uses BTreeMap for deterministic namespace iteration order
- S005 limits output to top 5 co-occurring pairs by count to avoid noise in large corpora

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Refactored CheckFilters to struct to satisfy clippy pedantic**
- **Found during:** Task 2
- **Issue:** Adding 4 bools to cmd_check triggered clippy::fn_params_excessive_bools and clippy::too_many_arguments
- **Fix:** Created CheckFilters struct encapsulating the four filter booleans with any_active() helper
- **Files modified:** src/cli.rs, src/main.rs
- **Verification:** clippy --all-targets passes clean
- **Committed in:** 59c7e2e

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Better API design -- struct is cleaner than four separate bool params. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Suggestion diagnostics available for status dashboard (Plan 03) to reference
- CheckFilters pattern available for any future check command enhancements
- run_suggestions() can be called independently for status command suggestion count

---
*Phase: 03-convergence-polish*
*Completed: 2026-03-29*
