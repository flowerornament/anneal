---
phase: 04-types-plausibility
plan: 02
subsystem: parse
tags: [rust, plausibility, classification, false-positives, diagnostics]

requires:
  - phase: 04-types-plausibility
    plan: 01
    provides: classify_frontmatter_value function and RefHint enum in extraction.rs
provides:
  - Plausibility filter wired into build_graph frontmatter edge loop
  - W004 diagnostic code for implausible frontmatter values
  - ImplausibleRef and ExternalRef tracking types in parse.rs
affects: [05-body-scanner, 06-resolution-cascade, HandleKind-External]

tech-stack:
  added: []
  patterns: [classify-before-resolve, plausibility-gate-in-build-graph]

key-files:
  created: []
  modified: [src/parse.rs, src/checks.rs, src/main.rs, src/cli.rs]

key-decisions:
  - "Store ImplausibleRef/ExternalRef as parse.rs structs to avoid circular dependency with checks.rs Diagnostic"
  - "ExternalRef.file field marked dead_code until HandleKind::External is added in future plan"

patterns-established:
  - "Classify-before-resolve: frontmatter values classified via RefHint before becoming PendingEdge entries"
  - "Plausibility diagnostics as W004 warnings, not errors, since they are informational"

requirements-completed: [EXTRACT-05, EXTRACT-06]

duration: 6min
completed: 2026-03-29
---

# Phase 4 Plan 02: Plausibility Filter Wiring Summary

**Wire classify_frontmatter_value into build_graph to eliminate false positive E001 errors from URLs, prose, absolute paths, and wildcards in frontmatter fields**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-29T20:24:10Z
- **Completed:** 2026-03-29T20:30:15Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 4

## Accomplishments
- Wired classify_frontmatter_value into build_graph's frontmatter edge loop so every target is classified before becoming a PendingEdge
- URLs in frontmatter now tracked as ExternalRef (never reach resolution or E001)
- Implausible values (prose, absolute paths, wildcards) produce W004 diagnostics instead of false E001 errors
- Valid labels and .md paths continue to create PendingEdge unchanged
- Added check_plausibility to run_checks producing W004 warnings with file location and reason
- 5 new integration tests validating plausibility filter behavior in build_graph

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): Failing plausibility filter integration tests** - `405eaad` (test)
2. **Task 1 (GREEN): Wire plausibility filter + W004 diagnostic** - `8656f5f` (feat)

## Files Created/Modified
- `src/parse.rs` - Added ImplausibleRef, ExternalRef structs; modified build_graph loop to classify before creating PendingEdge; added 5 integration tests
- `src/checks.rs` - Added check_plausibility function (W004 diagnostic); added implausible_refs parameter to run_checks
- `src/main.rs` - Updated 3 run_checks call sites to pass implausible_refs
- `src/cli.rs` - Updated 1 run_checks call site to pass implausible_refs

## Decisions Made
- Used separate ImplausibleRef/ExternalRef structs in parse.rs rather than importing Diagnostic from checks.rs, to avoid circular dependency (checks.rs already imports from parse.rs)
- Added `#[allow(dead_code)]` on ExternalRef and external_refs field since external URL handling is tracked but not yet consumed (future HandleKind::External plan)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy single_char_pattern in test**
- **Found during:** Task 1 (GREEN phase, clippy check)
- **Issue:** Test used `contains("*")` which clippy pedantic flags as single_char_pattern
- **Fix:** Changed to `contains('*')`
- **Files modified:** src/parse.rs
- **Verification:** cargo clippy passes clean
- **Committed in:** 8656f5f

---

**Total deviations:** 1 auto-fixed (1 clippy lint)
**Impact on plan:** Trivial fix for clippy compliance. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Known Stubs
None - all wiring is complete. ExternalRef tracking is intentionally additive-only (data collected, consumed in future HandleKind::External plan).

## Next Phase Readiness
- Plausibility filter fully operational in build_graph
- W004 diagnostics visible in `anneal check` output
- Ready for Phase 5 (pulldown-cmark body scanner) or Phase 6 (resolution cascade)

## Self-Check: PASSED

All files exist, all commits found, all acceptance criteria content verified.

---
*Phase: 04-types-plausibility*
*Completed: 2026-03-29*
