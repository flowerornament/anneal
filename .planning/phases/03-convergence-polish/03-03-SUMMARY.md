---
phase: 03-convergence-polish
plan: 03
subsystem: cli
tags: [graph-rendering, dot, graphviz, bfs, subgraph, map]

# Dependency graph
requires:
  - phase: 03-01
    provides: snapshot.rs, Severity::Suggestion variant
provides:
  - "`anneal map` command with text and DOT graph rendering"
  - "Subgraph extraction via --concern and --around BFS"
  - "MapOutput struct with Serialize + print_human pattern"
  - "MapOptions struct for parameter bundling (clippy too_many_arguments)"
affects: [03-04, 03-05]

# Tech tracking
tech-stack:
  added: []
  patterns: [BFS neighborhood extraction, grouped text rendering, DOT graph output]

key-files:
  created: []
  modified:
    - src/cli.rs
    - src/main.rs
    - src/handle.rs

key-decisions:
  - "MapOptions struct bundles 8 cmd_map parameters to satisfy clippy too_many_arguments"
  - "File handles always included in default subgraph regardless of terminal status (structural anchors)"
  - "Edge deduplication via BTreeSet with (source, target, kind) key in both rendering and counting"
  - "Ord/PartialOrd derives added to NodeId for BTreeSet edge deduplication"
  - "DOT shapes: note=File, box=Label, ellipse=Section, diamond=Version; terminal nodes grey-filled"
  - "Concern matching uses both starts_with and contains for flexible pattern matching"

patterns-established:
  - "MapOptions struct pattern: bundle many function parameters into an options struct"
  - "render_text groups by kind then namespace for large graph readability"
  - "render_dot produces valid graphviz with shape/color encoding"

requirements-completed: [CLI-05]

# Metrics
duration: 8min
completed: 2026-03-29
---

# Phase 3 Plan 03: Map Command Summary

**`anneal map` with text/DOT rendering, BFS neighborhood extraction via --around, and concern group filtering via --concern**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-29T08:32:14Z
- **Completed:** 2026-03-29T08:40:07Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Implemented `anneal map` rendering knowledge graph in grouped text format (Files, Labels by namespace, Sections, Versions, Edges)
- Implemented `anneal map --format=dot` producing valid graphviz DOT output with shape/color encoding per handle kind
- Implemented `--around=<handle> --depth=N` BFS neighborhood extraction (forward + reverse edges)
- Implemented `--concern=<name>` filtering to concern group patterns from config with one-hop neighbor inclusion
- 8 tests covering all rendering modes and filter combinations

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement cmd_map with text and DOT rendering** - `d99384f` (feat) - TDD with 8 tests
2. **Task 2: Wire Map command into CLI dispatch in main.rs** - `afc9668` (feat)

## Files Created/Modified
- `src/cli.rs` - MapOutput, MapOptions, cmd_map, extract_subgraph, render_text, render_dot, 8 map tests
- `src/main.rs` - Map variant in Command enum, match dispatch with format validation
- `src/handle.rs` - Added Ord/PartialOrd derives on NodeId

## Decisions Made
- Used MapOptions struct to bundle 8 parameters (clippy pedantic enforces max 7 args)
- File handles always included in default active graph rendering regardless of terminal status -- files provide structural anchors
- Edge deduplication via BTreeSet(source, target, kind) ensures consistent counts between JSON metadata and rendered output
- DOT rendering uses shape=note for files, box for labels, ellipse for sections, diamond for versions; terminal nodes colored grey
- Concern pattern matching uses both starts_with and contains for flexible matching (e.g., "OQ" matches "OQ-1" via starts_with)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Edge count deduplication mismatch**
- **Found during:** Task 2 (verification)
- **Issue:** count_subgraph_edges counted raw edges while render_text deduplicated, causing JSON metadata to show higher edge count than rendered text
- **Fix:** Added BTreeSet deduplication to count_subgraph_edges matching render_text logic
- **Files modified:** src/cli.rs
- **Verification:** `anneal --json map --around=OQ-64 --depth=1` edge count matches rendered count
- **Committed in:** afc9668 (Task 2 commit)

**2. [Rule 3 - Blocking] NodeId missing Ord derive for BTreeSet**
- **Found during:** Task 1 (compilation)
- **Issue:** BTreeSet<(NodeId, NodeId, &str)> requires Ord on NodeId which was only Hash/Eq
- **Fix:** Added PartialOrd, Ord derives to NodeId
- **Files modified:** src/handle.rs
- **Verification:** cargo build succeeds, all tests pass
- **Committed in:** d99384f (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correct compilation and accurate output. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## Known Stubs
None -- all functionality is fully wired.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Map command complete with text and DOT formats
- Status command (03-04) can reference map infrastructure for subgraph patterns
- Diff command (03-05) can reuse node_index and graph traversal patterns

## Self-Check: PASSED

All files exist, all commits found, all code artifacts verified.

---
*Phase: 03-convergence-polish*
*Completed: 2026-03-29*
