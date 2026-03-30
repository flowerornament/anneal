---
phase: 06-resolution-cascade
plan: 01
subsystem: diagnostics
tags: [line-numbers, pending-edge, check-config, active-only]

requires:
  - phase: 05-pulldown-cmark-migration
    provides: LineIndex and SourceSpan for body scanning line numbers
provides:
  - PendingEdge with line: Option<u32> populated at every construction site
  - E001 diagnostics carry line numbers from PendingEdge
  - CheckConfig struct parsed from anneal.toml [check] section
  - active_only behavior driven by config OR CLI flag
affects: [06-02, 06-03]

tech-stack:
  added: []
  patterns: [config-or-flag merging for CLI behavior]

key-files:
  created: []
  modified: [src/parse.rs, src/checks.rs, src/config.rs, src/main.rs, src/cli.rs]

key-decisions:
  - "ScanResult file_refs/section_refs changed to Vec<(String, u32)> to carry line numbers alongside ref strings"
  - "Frontmatter PendingEdges get line: Some(1) since serde_yaml_ng does not expose per-field line numbers"
  - "CheckConfig.default_filter is Option<String> not enum for future-proofing"

patterns-established:
  - "Config-or-flag merging: config.check.default_filter.as_deref() == Some(value) || cli_flag"

requirements-completed: [DIAG-01, DIAG-05, UX-01]

duration: 7min
completed: 2026-03-30
---

# Phase 06 Plan 01: Diagnostic Line Numbers and Check Config Summary

**PendingEdge carries line numbers into E001 diagnostics, CheckConfig enables active-only opt-in via anneal.toml**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-30T04:46:31Z
- **Completed:** 2026-03-30T04:53:58Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- PendingEdge struct now carries `line: Option<u32>` populated at every construction site (frontmatter = line 1, body = cmark scanner line)
- E001 diagnostics thread PendingEdge.line into Diagnostic.line, giving broken reference errors source locations
- CheckConfig struct with default_filter allows `[check] default_filter = "active-only"` in anneal.toml
- Config opt-in merges with CLI flag: either triggers active-only filtering
- 125 tests pass (4 new), clippy clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Add line field to PendingEdge and thread into Diagnostic** - `964ae97` (feat)
2. **Task 2: Add CheckConfig to AnnealConfig and wire active_only config opt-in** - `3bf5b2f` (feat)

## Files Created/Modified
- `src/parse.rs` - PendingEdge gains line field; ScanResult file_refs/section_refs carry (String, u32) tuples
- `src/checks.rs` - check_existence threads edge.line into E001 Diagnostic.line
- `src/config.rs` - New CheckConfig struct with default_filter; added to AnnealConfig; 4 unit tests
- `src/main.rs` - Check command merges config.check.default_filter with --active-only CLI flag
- `src/cli.rs` - Updated AnnealConfig construction in cmd_init to include check field

## Decisions Made
- ScanResult file_refs/section_refs changed from Vec<String> to Vec<(String, u32)> to carry line numbers. This required updating ~7 test assertions but keeps the data flow clean.
- Frontmatter PendingEdges use line 1 as pragmatic fallback since serde_yaml_ng does not expose per-field YAML line numbers.
- CheckConfig.default_filter is Option<String> (not an enum) to accept future filter values without code changes.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated cli.rs AnnealConfig construction**
- **Found during:** Task 2 (CheckConfig wiring)
- **Issue:** cli.rs cmd_init constructs AnnealConfig manually, missing new `check` field caused compile error
- **Fix:** Added `check: CheckConfig::default()` and imported CheckConfig
- **Files modified:** src/cli.rs
- **Verification:** cargo test passes
- **Committed in:** 3bf5b2f (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary for compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- PendingEdge.line field ready for Phase 06-02 resolution cascade enrichment
- CheckConfig struct ready for future check behavior configuration
- All foundation types in place for diagnostic enrichment pipeline

---
*Phase: 06-resolution-cascade*
*Completed: 2026-03-30*
