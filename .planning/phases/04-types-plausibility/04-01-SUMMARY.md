---
phase: 04-types-plausibility
plan: 01
subsystem: extraction
tags: [rust, types, regex, classification, plausibility]

requires:
  - phase: 03-convergence-polish
    provides: existing parse.rs types (PendingEdge, ScanResult, LabelCandidate)
provides:
  - FileExtraction struct for uniform per-file extraction output
  - DiscoveredRef with RefHint classification enum
  - RefSource enum (Frontmatter/Body)
  - classify_frontmatter_value function (10-rule plausibility filter)
  - Resolution enum (Exact/Fuzzy/Unresolved) in resolve.rs
affects: [04-02-plausibility-wiring, 05-body-scanner, 06-resolution-cascade]

tech-stack:
  added: []
  patterns: [anchored-regex-for-exact-match, compound-label-prefix-parsing, plausibility-classification-cascade]

key-files:
  created: [src/extraction.rs]
  modified: [src/main.rs, src/resolve.rs]

key-decisions:
  - "Compound label regex supports KB-D1 style prefixes via optional hyphen before digits"
  - "Comma-separated list check ordered before freeform prose check to avoid misclassification"
  - "Case-insensitive .md extension checks satisfy clippy pedantic"

patterns-established:
  - "Classification cascade: first-match-wins rule ordering for frontmatter value classification"
  - "Additive types: new types alongside existing PendingEdge/ScanResult, no removal"

requirements-completed: [EXTRACT-01, EXTRACT-02, RESOLVE-01]

duration: 4min
completed: 2026-03-29
---

# Phase 4 Plan 01: Extraction Types & Classify Summary

**Typed extraction pipeline types (FileExtraction, DiscoveredRef, RefHint) with 10-rule frontmatter value classifier and Resolution enum stub**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-29T20:16:43Z
- **Completed:** 2026-03-29T20:20:55Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Created src/extraction.rs with FileExtraction, DiscoveredRef, RefHint, RefSource types
- Implemented classify_frontmatter_value with 10-rule classification cascade (URLs, absolute paths, wildcards, prose, comma lists, section refs, labels, file paths)
- Added Resolution enum to resolve.rs with Exact/Fuzzy/Unresolved variants
- 18 new tests covering all classification rules and type construction (95 total tests pass)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create extraction.rs with types and classify function** - `dc9f3c1` (feat)
2. **Task 2: Add Resolution enum to resolve.rs** - `3b38f25` (feat)

## Files Created/Modified
- `src/extraction.rs` - New module: FileExtraction, DiscoveredRef, RefHint, RefSource, classify_frontmatter_value
- `src/main.rs` - Added `mod extraction;` declaration
- `src/resolve.rs` - Added Resolution enum and Serialize import

## Decisions Made
- Used compound label regex `^([A-Z][A-Z_]*(?:-[A-Z][A-Z_]*)*)-?(\d+)$` to support prefixes like "KB-D" in "KB-D1" (optional hyphen before trailing digits)
- Reordered comma-separated list check before freeform prose check to prevent false classification (comma lists contain spaces, would match prose rule first)
- Used `std::path::Path::extension()` for case-insensitive .md checks to satisfy clippy pedantic `case_sensitive_file_extension_comparisons`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed label regex for compound prefixes**
- **Found during:** Task 1 (extraction.rs creation)
- **Issue:** Plan specified regex `^([A-Z][A-Z_]*)-(\d+)$` which cannot match "KB-D1" (no hyphen between "D" and "1")
- **Fix:** Used `^([A-Z][A-Z_]*(?:-[A-Z][A-Z_]*)*)-?(\d+)$` with optional hyphen and compound prefix groups
- **Files modified:** src/extraction.rs
- **Verification:** Test `classify_label_compound_prefix` passes with prefix="KB-D", number=1
- **Committed in:** dc9f3c1

**2. [Rule 1 - Bug] Fixed classification rule ordering**
- **Found during:** Task 1 (test failures)
- **Issue:** Comma-separated list check (step 6 in plan) came after freeform prose check (step 5), but comma lists contain spaces and matched prose rule first
- **Fix:** Moved comma-separated list check before freeform prose check
- **Files modified:** src/extraction.rs
- **Verification:** Test `classify_comma_separated_list` passes
- **Committed in:** dc9f3c1

---

**Total deviations:** 2 auto-fixed (2 bugs in plan specification)
**Impact on plan:** Both fixes required for correct classification behavior. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Known Stubs
None - all types are intentionally additive-only for Phase 4 Plan 02 wiring. The `#[allow(dead_code)]` annotations on RefSource and Resolution are expected until wiring in subsequent plans.

## Next Phase Readiness
- Extraction types ready for Plan 02 to wire into build_graph
- Resolution enum ready for Phase 6 cascade logic
- All existing 77 tests unaffected; 18 new tests added

---
*Phase: 04-types-plausibility*
*Completed: 2026-03-29*
