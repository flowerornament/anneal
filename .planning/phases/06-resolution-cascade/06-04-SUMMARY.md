---
phase: 06-resolution-cascade
plan: 04
subsystem: diagnostics
tags: [rust, diagnostics, source-locations, DIAG-01, json-output]

requires:
  - phase: 06-resolution-cascade/03
    provides: "Evidence enum on diagnostics, cascade_candidates for E001 enrichment"
provides:
  - "Every diagnostic in --json output has non-null file and line"
  - "artifact_file helper for Version handle -> parent file resolution"
  - "section_ref_file threading from collect_unresolved_owned through run_checks"
affects: [verification, future-diagnostic-work]

tech-stack:
  added: []
  patterns: ["artifact_file helper for Version handle file_path resolution", "representative file pattern for aggregate diagnostics"]

key-files:
  created: []
  modified: ["src/checks.rs", "src/parse.rs", "src/main.rs", "src/cli.rs"]

key-decisions:
  - "Line 1 for all frontmatter-sourced diagnostics (no per-field YAML line numbers available)"
  - "Version handles resolve file_path through artifact field pointing to parent file node"
  - "Orphaned handles fall back through artifact -> outgoing edge -> incoming edge chain for file discovery"
  - "section_ref_file threaded from collect_unresolved_owned rather than re-scanning inside check_existence"
  - "run_checks gets #[allow(clippy::too_many_arguments)] for new section_ref_file parameter"

patterns-established:
  - "artifact_file(handle, graph) helper for resolving version handle file paths"
  - "Representative file pattern: aggregate diagnostics pick first relevant handle's file"

requirements-completed: [DIAG-01]

duration: 10min
completed: 2026-03-30
---

# Phase 06 Plan 04: DIAG-01 Source Locations Summary

**All 334 Murail diagnostics now carry non-null file and line -- zero null fields in --json output**

## Performance

- **Duration:** 10 min
- **Started:** 2026-03-30T07:04:47Z
- **Completed:** 2026-03-30T07:14:33Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Every diagnostic code (E001, E002, W001-W004, I001, I002, S001-S005) produces non-null file and line
- ImplausibleRef struct now carries a line field for future per-field line number refinement
- Version handles without file_path are resolved through their artifact field to the parent file node
- Aggregate diagnostics (I001, S002-S005) use representative file from first relevant handle

## Task Commits

Each task was committed atomically:

1. **Task 1: Add line numbers to W/S diagnostics, add line to ImplausibleRef** - `630dd11` (feat)
2. **Task 2: Thread representative file+line into aggregate diagnostics** - `9dc4332` (feat)

## Files Created/Modified
- `src/checks.rs` - All Diagnostic constructions now produce non-null file and line; artifact_file helper; representative file lookups for S002-S005
- `src/parse.rs` - ImplausibleRef struct gains `line: u32` field
- `src/main.rs` - collect_unresolved_owned returns section_ref_file; run_checks call sites updated with new parameter
- `src/cli.rs` - Updated run_checks call in diff snapshot builder

## Decisions Made
- Used line: 1 universally for frontmatter-sourced diagnostics since YAML parser does not expose per-field line numbers
- Version handles resolve file_path through artifact field (parent file node) rather than searching by name pattern
- section_ref_file extracted in collect_unresolved_owned and passed through as Option<&str> rather than re-scanning inside check_existence
- Added #[allow(clippy::too_many_arguments)] on run_checks rather than introducing a config struct (minimizes churn)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Version handles with no file_path produced null file in diagnostics**
- **Found during:** Task 2 (verification on Murail corpus)
- **Issue:** 25 S001 and 1 W001 diagnostics had null file because Version handles are created with file_path: None
- **Fix:** Added artifact_file helper that resolves Version handle -> artifact NodeId -> parent file's file_path. Applied to S001 (orphaned), W001 (staleness), and W002 (confidence gap)
- **Files modified:** src/checks.rs
- **Verification:** `jq '[.diagnostics[] | select(.file == null)] | length'` outputs 0
- **Committed in:** 9dc4332 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential for achieving zero null fields. No scope creep.

## Issues Encountered
None

## Known Stubs
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- DIAG-01 gap fully closed: every diagnostic carries file + line
- Ready for verification phase

---
*Phase: 06-resolution-cascade*
*Completed: 2026-03-30*
