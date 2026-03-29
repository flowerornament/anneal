---
phase: 02-checks-cli
plan: 02
subsystem: analysis-engine
tags: [rust, checks, diagnostics, impact-analysis, reverse-bfs, lattice]

requires:
  - phase: 02-checks-cli/01
    provides: "Code block skip, URL rejection, bare filename resolution, extensible frontmatter config, directory convention terminal status, linear namespace config"
  - phase: 01-graph-foundation
    provides: "DiGraph, Handle types, EdgeKind, parse.rs scanner, resolve.rs resolution, lattice.rs convergence"
provides:
  - "Five local consistency check rules (KB-R1..KB-R5) via run_checks()"
  - "Seven diagnostic codes: E001, E002, W001, W002, W003, I001, I002"
  - "Diagnostic type with severity, code, message, file location"
  - "Compiler-style print_human() formatting per spec section 12.1"
  - "Reverse BFS impact analysis via compute_impact()"
  - "ImpactResult with direct/indirect handle distinction"
  - "Cycle detection via visited-set in impact traversal"
affects: [02-checks-cli/03]

tech-stack:
  added: []
  patterns:
    - "Check rule functions: each takes &DiGraph + context, returns Vec<Diagnostic>"
    - "Severity enum with Ord derive for sort-by-severity"
    - "Reverse BFS with depth tracking for direct/indirect classification"

key-files:
  created:
    - "src/checks.rs"
    - "src/impact.rs"
  modified:
    - "src/main.rs"
    - "src/graph.rs"
    - "src/lattice.rs"

key-decisions:
  - "check_linearity takes &Lattice parameter to check terminal status for mooted obligations"
  - "check_conventions groups by parent directory path string, not by NodeId"
  - "Removed dead_code allows from graph.rs (outgoing, incoming, edges_by_kind) and lattice.rs (state_level, frontmatter_adoption_rate) now consumed by checks"

patterns-established:
  - "Check function signature: fn check_X(graph: &DiGraph, ...) -> Vec<Diagnostic>"
  - "Impact traversal pattern: BFS queue with (NodeId, depth) tuples"

requirements-completed: [CHECK-01, CHECK-02, CHECK-03, CHECK-04, CHECK-05, CHECK-06, IMPACT-01, IMPACT-02, IMPACT-03]

duration: 5min
completed: 2026-03-29
---

# Phase 02 Plan 02: Check Rules and Impact Analysis Summary

**Five local consistency check rules (KB-R1..R5) with 7 diagnostic codes and reverse BFS impact analysis with cycle detection and direct/indirect distinction**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-29T04:53:05Z
- **Completed:** 2026-03-29T04:58:50Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Implemented all five local consistency check rules per spec section 7: existence (E001/I001), staleness (W001), confidence gap (W002), linearity (E002/I002), convention adoption (W003)
- Built compiler-style diagnostic system with severity-sorted output matching spec section 12.1
- Implemented reverse BFS impact analysis with visited-set cycle detection and direct/indirect depth distinction
- 19 new unit tests: 11 for check rules, 8 for impact analysis (26 total with existing)
- Removed dead_code annotations from 5 graph/lattice methods now actively consumed

## Task Commits

1. **Task 1: Implement checks.rs with five check rules and diagnostic types** - `139281e` (feat)
2. **Task 2: Implement impact.rs with reverse graph traversal and cycle detection** - `0959544` (feat)

## Files Created/Modified

- `src/checks.rs` - NEW: Severity enum, Diagnostic struct with print_human, five check functions (check_existence, check_staleness, check_confidence_gap, check_linearity, check_conventions), run_checks entry point, 11 unit tests
- `src/impact.rs` - NEW: ImpactResult struct, compute_impact reverse BFS with cycle detection, 8 unit tests
- `src/main.rs` - Added `mod checks;` and `mod impact;` module declarations
- `src/graph.rs` - Removed dead_code allows from outgoing(), incoming(), edges_by_kind()
- `src/lattice.rs` - Removed dead_code allows from state_level(), frontmatter_adoption_rate()

## Decisions Made

- **check_linearity takes &Lattice:** The plan specified `check_linearity(graph, config)` but terminal status mooting requires lattice access to check `lattice.terminal.contains(status)`. Added `&Lattice` parameter to the function signature.
- **dead_code cleanup scope:** Only removed allows from methods now called by checks.rs/impact.rs. Left allows on `node_mut()`, `classify_status()`, `compute_freshness()`, `ConvergenceState`, `FreshnessLevel`, `Freshness` which are Phase 2 Plan 03 / Phase 3 forward stubs.

## Deviations from Plan

None - plan executed exactly as written. The only adjustment was adding `&Lattice` to `check_linearity` which was necessary for correct mooted-obligation semantics per KB-R4.

## Issues Encountered

None - all tests passed on first implementation. Clippy pedantic caught redundant closures and collapsible if-let chains, all fixed before commit.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `run_checks()` ready for CLI `anneal check` command in Plan 03
- `compute_impact()` ready for CLI `anneal impact` command in Plan 03
- Diagnostic `print_human()` ready for human-readable output
- All types derive Serialize for `--json` output support
- Section ref count and unresolved edges need to be surfaced from resolve pipeline to wire into `run_checks` (Plan 03 will need to modify resolve_all or main.rs to collect these)

## Self-Check: PASSED

All files verified present. All commit hashes verified in git log.

---
*Phase: 02-checks-cli*
*Completed: 2026-03-29*
