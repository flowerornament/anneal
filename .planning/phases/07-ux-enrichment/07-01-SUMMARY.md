---
phase: 07-ux-enrichment
plan: 01
subsystem: ux
tags: [rust, clap, serde, config, graph]
requires:
  - phase: 06-resolution-cascade
    provides: enriched diagnostics, typed extraction, resolution candidates
provides:
  - suppress configuration for filtering known false positives by code or code+target
  - external URL handles wired into the graph as first-class nodes
  - default depth 1 for `anneal map --around`
affects: [07-02, 07-03, 07-04, check, map, self-check]
tech-stack:
  added: []
  patterns: [post-check diagnostic suppression, external URL graph nodes with cites edges]
key-files:
  created: []
  modified:
    - src/config.rs
    - src/handle.rs
    - src/main.rs
    - src/cli.rs
    - src/checks.rs
    - src/parse.rs
    - src/snapshot.rs
key-decisions:
  - "Suppressions are applied after run_checks and before snapshot generation so human output and recorded diagnostics stay aligned."
  - "External URLs reuse one graph node per URL identity while each source file still emits its own Cites edge."
patterns-established:
  - "Config additions land as concrete defaulted fields on AnnealConfig so deny_unknown_fields remains useful."
  - "Non-corpus identities can participate in graph navigation without entering label namespace or obligation accounting."
requirements-completed: [CONFIG-01, CONFIG-02, UX-04]
duration: 6 min
completed: 2026-03-31
---

# Phase 07 Plan 01: Config Suppression, External URLs, and Map Depth Summary

**Suppressible diagnostics, external URL graph nodes, and a tighter default `map --around` radius for corpus navigation**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-30T19:19:01-07:00
- **Completed:** 2026-03-31T02:25:14Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Added `[suppress]` config support with global code suppression and targeted code+target rules.
- Introduced `HandleKind::External` and wired frontmatter URLs into the graph as external nodes with `Cites` edges.
- Changed `anneal map --around` to default to depth 1 and updated command initialization to include suppress defaults.

## Task Commits

Each task was committed atomically:

1. **Task 1: Config suppress + HandleKind::External + depth default** - `f5f529a` (test), `979928a` (feat)
2. **Task 2: Wire suppress filter + External handle creation** - `0afa366` (test), `e52cdde` (feat)

## Files Created/Modified
- `src/config.rs` - Added `SuppressConfig`, `SuppressRule`, and `AnnealConfig.suppress` with parsing tests.
- `src/handle.rs` - Added `HandleKind::External` and coverage for `as_str()`.
- `src/main.rs` - Applied suppressions in `check`, `status`, and `diff`, and changed map depth default to 1.
- `src/cli.rs` - Seeded default suppress config in `init` and handled external nodes in map rendering.
- `src/checks.rs` - Added `apply_suppressions` plus regression tests for global and targeted filtering.
- `src/parse.rs` - Reused a single external node per URL and attached file-to-URL `Cites` edges during graph build.
- `src/snapshot.rs` - Added regression coverage showing external handles stay out of namespace and obligation accounting.

## Decisions Made

- Reused one external node per URL to preserve handle identity uniqueness across multiple referencing files.
- Kept suppression as a post-check filter rather than baking it into individual rule implementations, which keeps rule behavior testable and config-driven.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Deduplicated external URL nodes by identity**
- **Found during:** Task 2 (Wire suppress filter + External handle creation)
- **Issue:** Creating a fresh external node for every reference would duplicate handle IDs and make graph lookups ambiguous when the same URL appears in multiple files.
- **Fix:** Added an `external_nodes` index in `build_graph` so each URL maps to one node while every referencing file still creates its own `Cites` edge.
- **Files modified:** `src/parse.rs`
- **Verification:** `cargo test`, `just check`
- **Committed in:** `e52cdde`

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Improved correctness without changing the planned user-facing behavior.

## Issues Encountered

- `cargo test --lib` from the plan was not applicable because `anneal` is a binary-only crate. Used `cargo test` instead.
- `cargo run -- --root .design/ check` completed without crashing but still reports the known self-check broken reference `synthesis/v17.md`; that remains for later Phase 07 work.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ready for `07-02-PLAN.md`; the config and graph primitives it depends on are in place.
- Self-check is still intentionally incomplete until the later Phase 07 closure work lands.

## Self-Check: PASSED

- Verified `.planning/phases/07-ux-enrichment/07-01-SUMMARY.md` exists on disk.
- Verified task commits `f5f529a`, `979928a`, `0afa366`, and `e52cdde` exist in git history.

---
*Phase: 07-ux-enrichment*
*Completed: 2026-03-31*
