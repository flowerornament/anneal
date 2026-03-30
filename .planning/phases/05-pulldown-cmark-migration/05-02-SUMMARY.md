---
phase: 05-pulldown-cmark-migration
plan: 02
subsystem: parse
tags: [pulldown-cmark, markdown, event-walker, body-scanner, wikilinks]

requires:
  - phase: 05-01
    provides: "SourceSpan, LineIndex types, pulldown-cmark 0.13 dependency, span field on DiscoveredRef"
provides:
  - "scan_file_cmark function using pulldown-cmark event walker"
  - "Structural code block and inline code skipping"
  - "Markdown link and wiki-link extraction with SourceSpan"
  - "HTML block regex scanning"
  - "Text concatenation per block before regex matching"
affects: [05-03-parallel-run-comparison]

tech-stack:
  added: []
  patterns: ["pulldown-cmark into_offset_iter for byte offset tracking", "block-level text accumulation before regex scanning"]

key-files:
  created: []
  modified: ["src/parse.rs"]

key-decisions:
  - "classify_body_ref as separate function from classify_frontmatter_value (body refs come from regex/link events, not freeform values)"
  - "scan_text_for_refs helper extracts shared logic between block-end and HTML scanning"
  - "Match same arms lint allowed via attribute for explicit documentation of Code/Math event skipping"

patterns-established:
  - "pulldown-cmark event walker with offset iter: iterate (event, range) pairs for byte-accurate source locations"
  - "Block-level text accumulation: collect text events, scan on block end, for cross-event reference matching"

requirements-completed: [EXTRACT-07, EXTRACT-08, EXTRACT-09, EXTRACT-10, EXTRACT-11]

duration: 5min
completed: 2026-03-30
---

# Phase 05 Plan 02: Body Scanner Summary

**pulldown-cmark event walker replacing regex body scanner with structural code block skipping, markdown/wiki-link extraction, HTML block scanning, and SourceSpan on every body ref**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-30T04:01:35Z
- **Completed:** 2026-03-30T04:06:12Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Implemented `scan_file_cmark` function with pulldown-cmark Parser using ENABLE_HEADING_ATTRIBUTES and ENABLE_WIKILINKS options
- Structural code block and inline code skipping (no regex toggle, no false positives from backticks in inline code)
- Markdown link extraction with fragment stripping, wiki-link extraction via `LinkType::WikiLink`
- HTML block content scanned with existing regex patterns for label/section/file references
- Text events concatenated per block element before regex matching (EXTRACT-08)
- Every body DiscoveredRef carries SourceSpan with accurate line number via LineIndex
- Old `scan_file` preserved for parallel-run comparison in Plan 03
- 17 new tests covering all behavior cases; all 131 tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement scan_file_cmark with pulldown-cmark event walker** - `df1fc8c` (feat)

## Files Created/Modified
- `src/parse.rs` - Added `scan_file_cmark`, `classify_body_ref`, `scan_text_for_refs` functions (+731 lines)

## Decisions Made
- Created `classify_body_ref` as a separate, simpler classifier than `classify_frontmatter_value` since body references come from regex matches and link events, not arbitrary frontmatter strings
- Used `scan_text_for_refs` helper to deduplicate regex scanning logic between paragraph/item block ends and HTML block content
- Applied `#[allow(clippy::match_same_arms)]` to document intentional Code/Math event skipping explicitly rather than letting them fall through to the wildcard arm
- External links (http/https/mailto) and fragment-only links (#anchor) are explicitly skipped in Link event handling

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `scan_file_cmark` is ready for parallel-run comparison in Plan 03
- Both old `scan_file` and new `scan_file_cmark` coexist, producing the same `ScanResult` for backward compatibility
- The new function additionally produces `Vec<DiscoveredRef>` with `SourceSpan` for the typed extraction pipeline

---
*Phase: 05-pulldown-cmark-migration*
*Completed: 2026-03-30*
