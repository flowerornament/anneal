---
phase: 04-types-plausibility
plan: 03
subsystem: parsing
tags: [extraction, refhint, discovered-ref, file-extraction, frontmatter, json-output]

requires:
  - phase: 04-01
    provides: "DiscoveredRef, FileExtraction, RefHint, RefSource types in extraction.rs"
  - phase: 04-02
    provides: "classify_frontmatter_value wired into build_graph frontmatter loop"
provides:
  - "FileExtraction constructed per-file in build_graph with DiscoveredRef for every frontmatter target"
  - "CheckOutput includes extractions array in --json output"
  - "BuildResult carries Vec<FileExtraction> for downstream consumers"
affects: [05-body-scanner, 06-resolution]

tech-stack:
  added: []
  patterns: ["additive-only JSON output (new field, no breaking changes)", "dual-pass classification (existing PendingEdge flow + new DiscoveredRef flow)"]

key-files:
  created: []
  modified: [src/parse.rs, src/cli.rs, src/main.rs, src/extraction.rs]

key-decisions:
  - "Dual-pass over field_edges: existing classify/match for PendingEdge/ImplausibleRef/ExternalRef unchanged, new pass populates DiscoveredRef for FileExtraction"
  - "RefSource::Frontmatter field populated from EdgeKind::as_str() since FrontmatterEdge does not store field name"
  - "extractions is a plain Vec (not Option) on CheckOutput -- always present in JSON"

patterns-established:
  - "Additive JSON: new fields added to CheckOutput without breaking existing consumers"
  - "FileExtraction parallel to existing PendingEdge flow -- does not replace it"

requirements-completed: [EXTRACT-01, EXTRACT-02, EXTRACT-05, EXTRACT-06, RESOLVE-01]

duration: 4min
completed: 2026-03-30
---

# Phase 04 Plan 03: Gap Closure Summary

**FileExtraction wired into build_graph with DiscoveredRef per frontmatter target, surfaced in anneal check --json extractions array**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-30T00:34:46Z
- **Completed:** 2026-03-30T00:38:27Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- FileExtraction constructed per-file in build_graph with DiscoveredRef populated for every frontmatter reference target
- CheckOutput includes extractions field serialized in --json output
- Two new integration tests verify extraction population for all reference types (FilePath, External, Implausible)
- All 102 tests pass (100 existing + 2 new), clippy clean, fmt clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Populate FileExtraction per-file in build_graph and surface in CheckOutput JSON** - `b4435fd` (feat)
2. **Task 2: Add integration test verifying DiscoveredRef appears in JSON output** - `15e6eb0` (test)

## Files Created/Modified
- `src/parse.rs` - Added FileExtraction construction per-file in build_graph, extractions field on BuildResult, two integration tests
- `src/cli.rs` - Added extractions field to CheckOutput, updated cmd_check signature
- `src/main.rs` - Updated cmd_check call to pass result.extractions
- `src/extraction.rs` - Updated dead_code comment on RefSource

## Decisions Made
- Used dual-pass over field_edges rather than modifying existing classify/match block -- keeps PendingEdge/ImplausibleRef/ExternalRef flow untouched
- RefSource::Frontmatter field uses EdgeKind::as_str() (e.g., "DependsOn") since FrontmatterEdge does not store the original YAML field name
- extractions is always present in JSON (plain Vec, not Option) since the plan goal requires it in check output

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None - all data flows are wired end-to-end.

## Issues Encountered

- Tests using `tempfile::tempdir()` failed because macOS symlinks `/var` to `/private/var`, causing path mismatch in build_graph walker. Fixed by using `std::env::temp_dir()` pattern matching existing tests.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- FileExtraction and DiscoveredRef now constructed in production code -- Phase 4 verification criteria met
- Body scanner (Phase 5) can extend FileExtraction with RefSource::Body variants
- Resolution (Phase 6) can consume DiscoveredRef.hint for strategy selection

---
*Phase: 04-types-plausibility*
*Completed: 2026-03-30*
