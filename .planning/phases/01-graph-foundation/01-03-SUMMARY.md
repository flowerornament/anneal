---
phase: 01-graph-foundation
plan: 03
subsystem: graph
tags: [rust, regex, namespace-inference, handle-resolution, version-handles, cli, clap, serde-json]

# Dependency graph
requires:
  - "01-01: Handle/Config/DiGraph type system"
  - "01-02: Parse pipeline and lattice inference"
provides:
  - "Namespace inference by sequential cardinality (src/resolve.rs)"
  - "Label resolution filtering by confirmed namespaces"
  - "Version handle resolution from *-vN.md naming conventions"
  - "Pending edge resolution from frontmatter-derived references"
  - "File path resolution relative to referring file directory"
  - "resolve_all orchestrator composing full resolution pipeline"
  - "Working CLI binary with --root and --json flags (src/main.rs)"
  - "End-to-end pipeline: config -> build_graph -> resolve_all -> infer_lattice -> print"
  - "Integration test validating Murail corpus (259 files, 9788 handles, 22 namespaces)"
affects: [02-checks-cli]

# Tech tracking
tech-stack:
  added: []
  patterns: [namespace inference by cardinality, version handle supersession chains, node index for identity lookup]

key-files:
  created: []
  modified: [src/resolve.rs, src/main.rs]

key-decisions:
  - "resolve_all returns ResolveStats directly (not Result) since resolution never fails"
  - "Version handles created from filename pattern only (*-vN.md), not body text v-refs (too noisy)"
  - "Empty terminal_by_directory set for Phase 1 lattice -- directory convention deferred to Phase 2"
  - "False positive rejection uses sequential run heuristic: large min number + no 3-consecutive run = rejected"

patterns-established:
  - "Node index: HashMap<String, NodeId> for O(1) identity-to-node lookup"
  - "Two-pass resolution: build index, resolve labels, rebuild index, resolve edges"
  - "Version supersession chain: sorted by version, each node supersedes previous"

requirements-completed: [HANDLE-04, HANDLE-05, HANDLE-06, GRAPH-06]

# Metrics
duration: 5min
completed: 2026-03-29
---

# Phase 01 Plan 03: Resolution and Pipeline Summary

**Namespace inference with false positive rejection, version handle supersession chains, and end-to-end CLI pipeline producing 9788 handles and 6408 edges from 259 Murail files in <200ms**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-29T00:00:30Z
- **Completed:** 2026-03-29T00:05:54Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- resolve.rs: full handle resolution module with namespace inference (N>=3 members, M>=2 files), label filtering, version handle creation from filename conventions, pending edge resolution, and file path resolution
- main.rs: complete Phase 1 CLI pipeline wiring config -> build_graph -> resolve_all -> infer_lattice -> print, with --root and --json flags
- Integration test validates Murail corpus: 259 files, 9788 handles, 6408 edges, 22 namespaces (OQ, FM, A, SR, DG, etc.), zero false positives (SHA, AVX, GPT correctly rejected)
- Version handles: 39 resolved from *-vN.md files with supersession chains
- Performance: <200ms wall time for full pipeline on 259 files in release mode

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement resolve.rs with namespace inference, version handle resolution, and handle resolution** - `42f5fb5` (feat)
2. **Task 2: Wire main.rs pipeline and validate against Murail corpus** - `60dda48` (feat)

## Files Created/Modified
- `src/resolve.rs` - Namespace inference, label resolution, version handle resolution, pending edge resolution, file path resolution, resolve_all orchestrator
- `src/main.rs` - CLI with clap derive (--root, --json), full pipeline wiring, GraphSummary JSON output, human-readable print, Murail integration test

## Decisions Made
- `resolve_all` returns `ResolveStats` directly instead of `Result<ResolveStats>` since resolution never actually fails (clippy pedantic caught unnecessary Result wrapping)
- Version handles are created from filename patterns only (`*-vN.md`), not from body text `v17` references (too noisy -- Murail has 861 matches for `v1` alone)
- Empty `terminal_by_directory` set passed to `infer_lattice` in Phase 1 -- directory convention analysis deferred to Phase 2 when checks module needs it
- False positive rejection uses sequential run heuristic: prefixes with min number > 100 AND no 3-consecutive-number run are rejected (catches SHA-256, AVX-512, CRC-32)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed ResolveStats field name mismatch**
- **Found during:** Task 1
- **Issue:** Local variable `pending_unresolved` didn't match struct field name `pending_edges_unresolved` in shorthand initialization
- **Fix:** Used explicit field: value syntax `pending_edges_unresolved: pending_unresolved`
- **Files modified:** src/resolve.rs
- **Committed in:** 42f5fb5

**2. [Rule 1 - Bug] Fixed clippy pedantic: unnecessary Result wrapping, redundant closure, pass-by-value**
- **Found during:** Task 1
- **Issue:** clippy::unnecessary_wraps on resolve_all, clippy::redundant_closure on map, clippy::needless_pass_by_value on Vec params
- **Fix:** Changed resolve_all to return ResolveStats directly, used method reference, changed Vec params to slices
- **Files modified:** src/resolve.rs
- **Committed in:** 42f5fb5

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Minor code style fixes required by clippy pedantic. No scope creep.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 1 complete: full pipeline from directory scan to graph statistics
- All types derive Serialize for --json support
- Graph structure ready for Phase 2 consistency checks (DiGraph with typed edges, resolved handles)
- Dead code warnings remain for Phase 2+ functions (checks, freshness, impact, etc.)
- Murail corpus validated: 259 files, 9788 handles, 6408 edges, 22 namespaces

## Known Stubs
None -- all Phase 1 functionality is fully wired and operational.

## Self-Check: PASSED

All created files verified present. All commit hashes verified in git log.

---
*Phase: 01-graph-foundation*
*Completed: 2026-03-29*
