---
phase: 05-pulldown-cmark-migration
plan: 03
subsystem: parsing
tags: [pulldown-cmark, scanner, parallel-run, regression-testing]

requires:
  - phase: 05-02
    provides: "scan_file_cmark function with pulldown-cmark event walker"
  - phase: 05-01
    provides: "SourceSpan, LineIndex types, pulldown-cmark dependency"
provides:
  - "Production body scanning via pulldown-cmark (scan_file_cmark wired into build_graph)"
  - "Parallel-run comparison tests documenting scanner quality improvement"
  - "Body DiscoveredRefs with SourceSpan in FileExtraction"
affects: [06-resolution-cascade, checks, diagnostics]

tech-stack:
  added: []
  patterns: ["pulldown-cmark event walker as production scanner", "parallel-run regression testing pattern"]

key-files:
  created: []
  modified:
    - src/parse.rs
    - src/extraction.rs

key-decisions:
  - "scan_file retained as pub(crate) for parallel-run comparison tests, not removed"
  - "FileExtraction construction moved after scan_file_cmark to include body refs"

patterns-established:
  - "Parallel-run comparison: run both old and new scanners on real corpora, assert new <= old false positives"

requirements-completed: [QUALITY-01]

duration: 4min
completed: 2026-03-30
---

# Phase 05 Plan 03: Production Pipeline Wiring Summary

**pulldown-cmark scanner replaces regex scanner in build_graph; parallel-run tests confirm 13-18% fewer false positives on Murail and Herald corpora with 100% SourceSpan coverage**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-30T04:08:32Z
- **Completed:** 2026-03-30T04:12:36Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Wired scan_file_cmark into build_graph, replacing regex-based scan_file for production body scanning
- FileExtraction now includes body DiscoveredRefs with SourceSpan (line numbers for all body references)
- Parallel-run comparison tests document quality improvement: Murail 9734->8441 refs (13% fewer), Herald 1039->852 refs (18% fewer)
- 100% SourceSpan coverage on body DiscoveredRefs across both corpora (8441/8441 Murail, 852/852 Herald)

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire scan_file_cmark into build_graph** - `c7560a9` (feat)
2. **Task 2: Parallel-run comparison tests** - `f6e0eb1` (test)

## Files Created/Modified
- `src/parse.rs` - Replaced scan_file with scan_file_cmark in build_graph; added parallel_run_murail and parallel_run_herald tests
- `src/extraction.rs` - Removed dead_code allows on LineIndex and RefSource::Body (now used in production)

## Decisions Made
- Kept scan_file as pub(crate) rather than removing it, needed for parallel-run comparison tests
- Moved FileExtraction construction after scan_file_cmark call to include body refs via discovered_refs.extend(body_refs)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed dead_code allows now that types are used**
- **Found during:** Task 1
- **Issue:** LineIndex and RefSource::Body had #[allow(dead_code)] annotations from when they were unused
- **Fix:** Removed the annotations since they are now used in production code
- **Files modified:** src/extraction.rs
- **Verification:** cargo clippy passes without warnings about unused allows

**2. [Rule 1 - Bug] Fixed clippy pedantic lints**
- **Found during:** Task 1 and Task 2
- **Issue:** cast_possible_truncation on frontmatter line count, map().unwrap_or() pattern, #[ignore] without reason
- **Fix:** Added allow annotation, used map_or, added reason strings to #[ignore]
- **Files modified:** src/parse.rs

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Minor code quality fixes required by strict clippy configuration. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- pulldown-cmark scanner is now the production scanner, old regex scanner retained for comparison only
- Body DiscoveredRefs carry SourceSpan, ready for diagnostic enrichment in future phases
- Phase 05 complete: foundation types, scanner implementation, and production wiring all done

---
*Phase: 05-pulldown-cmark-migration*
*Completed: 2026-03-30*
