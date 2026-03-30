---
phase: 06-resolution-cascade
plan: 03
subsystem: diagnostics
tags: [evidence, diagnostic-enrichment, cascade-candidates, json-additive]

requires:
  - phase: 06-resolution-cascade
    plan: 01
    provides: PendingEdge with line numbers, CheckConfig
  - phase: 06-resolution-cascade
    plan: 02
    provides: cascade_unresolved with CascadeResult, cascade_candidates HashMap
provides:
  - Evidence enum with four variants on Diagnostic struct
  - E001 diagnostics enriched with cascade candidates in message and evidence
  - JSON output additive-only with nullable evidence field
affects: []

tech-stack:
  added: []
  patterns: [internally-tagged serde enum for Evidence JSON serialization]

key-files:
  created: []
  modified: [src/checks.rs, src/main.rs, src/cli.rs, src/snapshot.rs]

key-decisions:
  - "Evidence uses serde(tag = 'type') for internally-tagged JSON: {type: 'BrokenRef', target: ..., candidates: [...]}"
  - "E001 always gets Evidence::BrokenRef even with empty candidates -- consistent for JSON consumers"
  - "W001, W002, W004 also enriched with Evidence variants (StaleRef, ConfidenceGap, Implausible)"
  - "E002, I002, W003, I001, S001-S005 get evidence: None (no structured data beyond message)"

patterns-established:
  - "Evidence enum as extensible structured data carrier on Diagnostic"
  - "JSON additive-only schema evolution: new nullable fields, no type changes"

requirements-completed: [DIAG-02, DIAG-03, DIAG-04]

duration: 5min
completed: 2026-03-30
---

# Phase 06 Plan 03: Diagnostic Evidence Enrichment Summary

**Evidence enum on Diagnostic carries structured cascade candidates in JSON output; E001 shows "did you mean?" in both human and JSON; schema additive-only**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-30T05:05:15Z
- **Completed:** 2026-03-30T05:10:27Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Evidence enum with four variants (BrokenRef, StaleRef, ConfidenceGap, Implausible) added to Diagnostic struct
- E001 diagnostics enriched with cascade candidates: "broken reference: TQ-001 not found; similar handle exists: TQ-1"
- JSON output includes `evidence` field with internally-tagged enum serialization (`{"type": "BrokenRef", ...}`)
- JSON schema verified additive-only: `severity`, `code`, `message`, `file`, `line` unchanged; `evidence` added as nullable
- W001, W002, W004 also enriched with appropriate Evidence variants
- 47 diagnostics on Murail corpus now show "did you mean?" candidates
- 139 tests pass (4 new evidence tests), clippy clean, fmt clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Evidence enum and evidence field to Diagnostic** - `82b15e7` (feat)
2. **Task 2: Thread cascade_candidates and verify JSON additive-only** - `6240137` (feat)

## Files Created/Modified
- `src/checks.rs` - Evidence enum with 4 variants; evidence field on Diagnostic; check_existence enriched with cascade_candidates; run_checks signature updated; 4 new tests
- `src/main.rs` - All 3 run_checks call sites pass &cascade_candidates; removed unused_variables allow
- `src/cli.rs` - cli.rs test run_checks call updated with empty cascade_candidates
- `src/snapshot.rs` - snapshot.rs test Diagnostic constructions updated with evidence: None

## Decisions Made
- Evidence uses `#[serde(tag = "type")]` for internally-tagged JSON serialization, producing `{"type": "BrokenRef", "target": "...", "candidates": [...]}` rather than externally-tagged `{"BrokenRef": {...}}`
- E001 always receives `Evidence::BrokenRef` even when candidates are empty -- consistent API for JSON consumers
- W001/W002/W004 also got Evidence variants since the data was already available at each construction site
- E002, I002, W003, suggestion codes (S001-S005) use `evidence: None` -- no additional structured data beyond the message string

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated snapshot.rs and cli.rs Diagnostic constructions**
- **Found during:** Task 1
- **Issue:** snapshot.rs test code and cli.rs test helper construct Diagnostic manually, missing new `evidence` field caused compile errors
- **Fix:** Added `evidence: None` to both snapshot.rs and cli.rs Diagnostic constructions
- **Files modified:** src/snapshot.rs, src/cli.rs
- **Committed in:** 82b15e7 (Task 1 commit)

**2. [Rule 2 - Missing functionality] Enriched W001/W002/W004 with Evidence variants**
- **Found during:** Task 1
- **Issue:** Plan suggested optionally adding Evidence to other diagnostic types
- **Fix:** Added StaleRef, ConfidenceGap, and Implausible Evidence variants to W001, W002, W004 diagnostics
- **Files modified:** src/checks.rs
- **Committed in:** 82b15e7 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 enhancement)
**Impact on plan:** No scope creep. Both deviations were anticipated by the plan.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Known Stubs
None - all data flows are wired end-to-end.

## Next Phase Readiness
- Evidence enum is extensible for future diagnostic codes
- JSON schema is additive-only, safe for downstream consumers
- Phase 06 resolution cascade is complete: line numbers, cascade strategies, and diagnostic enrichment all wired

---
*Phase: 06-resolution-cascade*
*Completed: 2026-03-30*
