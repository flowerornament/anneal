---
phase: 02-checks-cli
plan: 03
subsystem: cli-surface
tags: [rust, clap, subcommands, cli, json, toml, impact-analysis]

requires:
  - phase: 02-checks-cli/01
    provides: "Extensible FrontmatterConfig, directory convention terminal status, code block skip, bare filename resolution"
  - phase: 02-checks-cli/02
    provides: "Five check rules via run_checks(), compute_impact() reverse BFS, Diagnostic type with print_human()"
provides:
  - "Five CLI subcommands: check, get, find, init, impact"
  - "print_json helper for --json output on all commands"
  - "Clap subcommand dispatch in main.rs with Optional<Command>"
  - "GraphSummary output type for bare anneal invocation"
  - "CheckOutput with severity counting and errors_only filter"
  - "GetOutput with edge summaries for incoming/outgoing"
  - "FindOutput with namespace/status/terminal filters"
  - "ImpactOutput with direct/indirect handle strings"
  - "InitOutput with AnnealConfig generation and dry_run support"
  - "D-07 frontmatter auto-detection via observed_frontmatter_keys"
  - "Node index (HashMap<String, NodeId>) built in main.rs"
  - "Unresolved edge collection with section ref counting"
affects: [03-convergence-polish]

tech-stack:
  added: []
  patterns:
    - "Concrete enum dispatch for CommandOutput (no trait objects due to Serialize not being object-safe)"
    - "Case-insensitive handle lookup: exact match first, then lowercase comparison"
    - "D-07 heuristic field mapping: name-based proposal for unknown frontmatter keys"
    - "collect_unresolved: separate section refs from real broken references"

key-files:
  created:
    - "src/cli.rs"
  modified:
    - "src/main.rs"
    - "src/parse.rs"
    - "src/checks.rs"
    - "src/impact.rs"

key-decisions:
  - "Concrete enum dispatch instead of CommandOutput trait objects (Serialize not object-safe)"
  - "cmd_find searches handle identity strings, not file content (fast, no re-read)"
  - "cmd_get uses exact match then case-insensitive fallback for handle lookup"
  - "Init proposes Cites as default fallback for unknown frontmatter fields"
  - "observed_frontmatter_keys collected via second YAML parse pass in build_graph"
  - "Unresolved edges collected post-resolution by filtering pending_edges against node_index"
  - "GraphSummary moved from main.rs to cli.rs for consistent output pattern"

patterns-established:
  - "print_json<T: Serialize> helper for all --json output"
  - "build_node_index in main.rs for handle lookup shared across commands"
  - "collect_unresolved separates section refs from real broken references for run_checks"

requirements-completed: [CLI-01, CLI-02, CLI-03, CLI-06, CLI-07, CLI-09, CLI-10, CONFIG-04]

duration: 9min
completed: 2026-03-29
---

# Phase 02 Plan 03: CLI Subcommand Surface Summary

**Five CLI commands (check, get, find, init, impact) with clap subcommand dispatch, --json on every command, and D-07 frontmatter auto-detection for anneal init**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-29T05:02:52Z
- **Completed:** 2026-03-29T05:11:28Z
- **Tasks:** 2 complete, 1 awaiting human verification
- **Files modified:** 5

## Accomplishments

- Created src/cli.rs with 5 command implementations: check, get, find, init, impact
- All commands produce valid JSON with `--json` flag and human-readable output by default
- Check exits non-zero when errors exist (1092 errors on Murail corpus)
- Init auto-detects frontmatter fields via D-07 heuristic mapping, generates valid TOML
- Impact shows direct/indirect affected handles via reverse BFS traversal
- Replaced flat Cli struct with clap subcommand dispatch; bare `anneal` still shows summary
- Built node_index and collect_unresolved infrastructure for wiring check pipeline

## Task Commits

1. **Task 1: cli.rs with check, get, find and subcommand dispatch** - `4a07004` (feat)
2. **Task 2: init and impact commands with auto-detection** - `7a5e0c1` (feat)
3. **Task 3: Human verification against Murail corpus** - awaiting checkpoint

## Files Created/Modified

- `src/cli.rs` - NEW: print_json helper, CheckOutput, GetOutput, FindOutput, ImpactOutput, InitOutput, GraphSummary types with print_human methods; cmd_check, cmd_get, cmd_find, cmd_impact, cmd_init, build_summary functions
- `src/main.rs` - Replaced flat Cli with subcommand enum (Check, Get, Find, Init, Impact); build_node_index and collect_unresolved helpers; full dispatch with --json support
- `src/parse.rs` - Added observed_frontmatter_keys to BuildResult; collection via YAML key iteration in build_graph
- `src/checks.rs` - Removed dead_code allows from print_human and run_checks (now consumed by cli.rs)
- `src/impact.rs` - Removed dead_code allow from compute_impact (now consumed by cli.rs)

## Decisions Made

- **Concrete enum dispatch over CommandOutput trait:** Since `serde::Serialize` is not object-safe, each command returns its own concrete output struct. The main match dispatches per-variant, calling print_human or print_json directly. This avoids the trait object limitation identified in Research Pitfall 5.
- **Find searches handle identities only:** Per Research Open Question 1, implemented case-insensitive substring search on handle.id rather than file content search. This covers the primary use case (finding handles by name/label) without re-reading files. File content search can be added later.
- **Init D-07 heuristic:** Unknown frontmatter fields with >= 3 occurrences get proposed mappings based on name heuristics: "affects/impacts" -> DependsOn Inverse, "source/extends/parent" -> DependsOn Forward, "resolves/addresses" -> Discharges Forward, everything else -> Cites Forward. This provides reasonable defaults while the user reviews and edits.
- **observed_frontmatter_keys via second YAML parse:** Rather than modifying parse_frontmatter to return keys (which would change its interface), frontmatter keys are collected separately in build_graph via a second YAML parse. The cost is minimal (~1ms) and keeps the interface clean.
- **GraphSummary moved to cli.rs:** The summary output type and formatting logic moved from main.rs to cli.rs for consistency with the other command output types.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Collapsible if chains in cli.rs and parse.rs**
- **Found during:** Tasks 1 and 2
- **Issue:** Clippy pedantic requires nested if-let chains to be collapsed
- **Fix:** Combined nested ifs into single if-let chains using `&&`
- **Files modified:** src/cli.rs, src/parse.rs
- **Committed in:** 4a07004, 7a5e0c1

**2. [Rule 1 - Bug] Match same arms in propose_mapping**
- **Found during:** Task 2
- **Issue:** Clippy pedantic detected identical match arm bodies for DependsOn Forward
- **Fix:** Merged "source/sources/based-on/builds-on" with "extends/parent" into single arm
- **Files modified:** src/cli.rs
- **Committed in:** 7a5e0c1

---

**Total deviations:** 2 auto-fixed (2 Rule 1 bugs - clippy lints)
**Impact on plan:** Minor code style fixes, no scope change.

## Issues Encountered

None - all implementations compiled and passed tests on first iteration after clippy fixes.

## User Setup Required

None - no external service configuration required.

## Murail Corpus Verification Results

All commands tested against `~/code/murail/.design/` (260 files):

| Command | Status | Key Output |
|---------|--------|------------|
| `anneal` (bare) | Working | 260 files, 9817 handles, 6670 edges, 22 namespaces |
| `anneal check` | Working | 1092 errors, 34 warnings, 1 info; exits non-zero |
| `anneal check --errors-only` | Working | Filters to errors only (0 warnings, 0 info) |
| `anneal --json check` | Working | Valid JSON with diagnostics array |
| `anneal get OQ-64` | Working | Shows label with file and incoming edges |
| `anneal find "formal"` | Working | 2616 matches sorted by id |
| `anneal impact murail-formal-model-v16` | Working | Shows v17 as directly affected |
| `anneal init --dry-run` | Working | Valid TOML with inferred structure |
| `anneal --json init --dry-run` | Working | Valid JSON with config object |

## Next Phase Readiness

- All 5 core commands operational for Phase 3 (status, map, diff are Phase 3 additions)
- GraphSummary output type ready for `anneal status` dashboard extension
- Check pipeline (node_index, collect_unresolved) established for future check rule additions
- Init auto-detection pattern ready for concern group auto-detection
- observed_frontmatter_keys available for future content analysis

## Self-Check: PASSED

All files verified present. All commit hashes verified in git log.

---
*Phase: 02-checks-cli*
*Completed: 2026-03-29*
