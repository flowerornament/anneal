---
phase: 05-pulldown-cmark-migration
plan: 01
subsystem: parsing
tags: [pulldown-cmark, source-span, line-index, extraction]

requires:
  - phase: 04-types-plausibility
    provides: DiscoveredRef and FileExtraction types in extraction.rs
provides:
  - SourceSpan type for attaching file+line to every reference
  - LineIndex for O(log n) byte-offset to line-number conversion with frontmatter offset
  - pulldown-cmark 0.13 dependency available for body scanner
affects: [05-pulldown-cmark-migration]

tech-stack:
  added: [pulldown-cmark 0.13]
  patterns: [binary-search line index, frontmatter-aware line numbering]

key-files:
  created: []
  modified: [src/extraction.rs, src/parse.rs, Cargo.toml]

key-decisions:
  - "LineIndex uses partition_point (binary search) for O(log n) offset-to-line lookup"
  - "frontmatter_line_count excludes closing --- line; base_line adds +1 for it"
  - "pulldown-cmark added with default-features = false; Options configured at parse time"

patterns-established:
  - "SourceSpan: file + line pair attached to DiscoveredRef via Option<SourceSpan>"
  - "LineIndex: from_content(body, frontmatter_line_count) then offset_to_line(byte_offset)"

requirements-completed: [EXTRACT-03, EXTRACT-04]

duration: 3min
completed: 2026-03-30
---

# Phase 5 Plan 01: Foundation Types Summary

**SourceSpan and LineIndex types for pulldown-cmark line tracking, plus pulldown-cmark 0.13 dependency**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-30T03:52:40Z
- **Completed:** 2026-03-30T03:56:17Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- SourceSpan struct (file + line) with Serialize, PartialEq, Eq for attaching source locations to references
- LineIndex with O(log n) binary search converting byte offsets to 1-based line numbers, frontmatter-aware
- pulldown-cmark 0.13 added as dependency (default-features disabled)
- DiscoveredRef extended with optional span field; all construction sites updated

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SourceSpan and LineIndex types with tests** - `09c68fd` (feat)
2. **Task 2: Add pulldown-cmark dependency** - `9375ee1` (chore)

## Files Created/Modified
- `src/extraction.rs` - SourceSpan, LineIndex structs, span field on DiscoveredRef, 10 new tests
- `src/parse.rs` - Updated DiscoveredRef construction with span: None
- `Cargo.toml` - Added pulldown-cmark 0.13 dependency

## Decisions Made
- LineIndex uses `partition_point` (stdlib binary search) rather than manual search for O(log n) lookup
- `frontmatter_line_count` parameter excludes the closing `---` line; `base_line` computation adds +1 for it
- pulldown-cmark added with `default-features = false` since Options are configured at parse time
- `#[allow(dead_code)]` on LineIndex since it's used starting in Plan 02

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Clippy `cast_possible_truncation` lint on `lines_before as u32` -- resolved with inline `#[allow]` since line count cannot exceed u32::MAX in practice

## Known Stubs
None - all types are fully implemented with no placeholder data.

## Next Phase Readiness
- SourceSpan and LineIndex are ready for the pulldown-cmark event walker in Plan 02
- pulldown-cmark 0.13 is available as a dependency
- All 113 tests pass, quality gate green

---
*Phase: 05-pulldown-cmark-migration*
*Completed: 2026-03-30*
