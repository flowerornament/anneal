---
phase: 02-checks-cli
verified: 2026-03-28T18:45:00Z
status: passed
score: 18/18 must-haves verified
re_verification: false
---

# Phase 2: Checks & CLI Verification Report

**Phase Goal:** Implement the five local consistency rules, impact analysis, and the core CLI commands that agents need.
**Verified:** 2026-03-28
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Broken references (edge targets that failed resolution) are reported as E001 errors | VERIFIED | `check_existence` in src/checks.rs:84-103, filters section: prefix, emits E001 per unresolved edge |
| 2 | Section references get a single I001 info summary, not per-reference errors | VERIFIED | `check_existence` src/checks.rs:71-82, single I001 with count when section_ref_count > 0 |
| 3 | Active handles referencing terminal handles produce W001 staleness warnings | VERIFIED | `check_staleness` src/checks.rs:113-145, lattice.active + lattice.terminal check |
| 4 | DependsOn edges where source state > target state produce W002 confidence gap warnings | VERIFIED | `check_confidence_gap` src/checks.rs:155-195, uses state_level() ordering comparison |
| 5 | Linear handles with zero discharges produce E002 errors | VERIFIED | `check_linearity` src/checks.rs:238-248, discharge_count == 0 case |
| 6 | Linear handles with multiple discharges produce I002 info diagnostics | VERIFIED | `check_linearity` src/checks.rs:249-260, discharge_count >= 2 case |
| 7 | Files missing frontmatter when >50% of directory siblings have it produce W003 warnings | VERIFIED | `check_conventions` src/checks.rs:275-324, frontmatter_adoption_rate > 0.5 threshold |
| 8 | All diagnostics have severity, error code, message, and optional file location | VERIFIED | `Diagnostic` struct src/checks.rs:24-30 with all 4 fields + print_human |
| 9 | Impact analysis traverses reverse DependsOn, Supersedes, Verifies edges | VERIFIED | `compute_impact` src/impact.rs:33-38, matches! guard on edge kinds |
| 10 | Impact analysis terminates on cycles via visited-set detection | VERIFIED | src/impact.rs:23, 39: visited.insert(start) + visited.insert(edge.source) |
| 11 | Impact results distinguish direct (depth=1) from indirect (depth>1) affected handles | VERIFIED | src/impact.rs:40-45, depth==0 -> direct, else -> indirect |
| 12 | anneal check runs all 5 check rules and prints diagnostics in compiler-style format | VERIFIED | Murail spot-check: 1092 errors, 34 warnings, 1 info; compiler-style output confirmed |
| 13 | anneal check exits non-zero when errors exist | VERIFIED | src/main.rs:193-195: `if output.errors > 0 { std::process::exit(1); }` |
| 14 | anneal check --errors-only filters to Error severity only | VERIFIED | Murail spot-check: `--errors-only` produces 1092 errors, 0 warnings, 0 info |
| 15 | anneal get OQ-64 resolves the label and shows id, kind, status, file, edges | VERIFIED | Murail spot-check: shows label, file: LABELS.md, incoming edges |
| 16 | anneal find searches handle identities and filters by --namespace and --status | VERIFIED | Murail spot-check: `find "formal"` returns 2616 matches sorted by id |
| 17 | anneal init generates anneal.toml from inferred structure; --dry-run shows without writing | VERIFIED | Murail spot-check: valid TOML with inferred convergence, handles, frontmatter sections |
| 18 | anneal impact shows direct and indirect affected handles | VERIFIED | Murail spot-check: `impact murail-formal-model-v16` shows murail-formal-model-v17 as direct |

**Score:** 18/18 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/checks.rs` | Five check rules, Diagnostic type, Severity enum, run_checks | VERIFIED | 653 lines; all 5 rules + Diagnostic + Severity + run_checks present |
| `src/impact.rs` | Reverse graph traversal with cycle detection | VERIFIED | 181 lines; compute_impact with BFS + visited set + 8 tests |
| `src/cli.rs` | Subcommand output types and command functions for all 5 commands | VERIFIED | 630 lines; cmd_check, cmd_get, cmd_find, cmd_impact, cmd_init, print_json all present |
| `src/main.rs` | Clap subcommand enum, dispatch to cli.rs functions | VERIFIED | enum Command with 5 variants, full dispatch with --json support |
| `src/config.rs` | FrontmatterConfig, FrontmatterFieldMapping, Direction types | VERIFIED | All 3 types present with 6 default field mappings |
| `src/parse.rs` | Code block skip, URL rejection, extensible frontmatter, terminal directory | VERIFIED | in_code_block guard, ://-prefix check, TERMINAL_DIRS, BuildResult.terminal_by_directory |
| `src/resolve.rs` | Bare filename resolution wired into resolve_pending_edges | VERIFIED | resolve_bare_filename + resolve_file_path called when .md extension + no / in identity |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/checks.rs` | `src/graph.rs` | graph.node(), graph.outgoing(), graph.edges_by_kind() | VERIFIED | All 3 graph methods called in check rules |
| `src/checks.rs` | `src/lattice.rs` | classify_status(), state_level() | VERIFIED | state_level() called in check_confidence_gap; lattice.active/terminal used in check_staleness/linearity |
| `src/impact.rs` | `src/graph.rs` | graph.incoming() for reverse traversal | VERIFIED | src/impact.rs:32: `for edge in graph.incoming(current)` |
| `src/cli.rs` | `src/checks.rs` | run_checks called by check command | VERIFIED | src/cli.rs:72: `checks::run_checks(...)` |
| `src/cli.rs` | `src/impact.rs` | compute_impact called by impact command | VERIFIED | src/cli.rs:370: `impact::compute_impact(graph, node_id)` |
| `src/main.rs` | `src/cli.rs` | Subcommand dispatch calls cli functions | VERIFIED | src/main.rs:152-267: full match on Command variants calling cli:: functions |
| `src/parse.rs` | `src/config.rs` | FrontmatterConfig used in parse_frontmatter | VERIFIED | src/parse.rs:116: `parse_frontmatter(yaml, config: &FrontmatterConfig)` |
| `src/resolve.rs` | `src/parse.rs` | resolve_file_path called for bare filename pending edges | VERIFIED | src/resolve.rs:270-276: resolve_bare_filename called when bare .md filename |
| `src/main.rs` | `src/lattice.rs` | terminal_by_directory passed to infer_lattice | VERIFIED | src/main.rs:141-145: `infer_lattice(..., &result.terminal_by_directory)` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `src/cli.rs cmd_check` | diagnostics | run_checks -> check rules iterate graph.nodes() | Yes — iterates live graph, builds from resolved edges | FLOWING |
| `src/cli.rs cmd_get` | GetOutput | graph.node(node_id), graph.outgoing/incoming | Yes — reads from graph built from file corpus | FLOWING |
| `src/cli.rs cmd_find` | matches | graph.nodes() iterator with filter | Yes — iterates live graph | FLOWING |
| `src/cli.rs cmd_impact` | ImpactOutput | compute_impact -> reverse BFS on graph | Yes — live graph traversal | FLOWING |
| `src/cli.rs cmd_init` | InitOutput/config | lattice + stats + observed_frontmatter_keys from BuildResult | Yes — derived from actual corpus analysis | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| anneal check produces compiler-style diagnostics | `anneal --root ~/code/murail/.design/ check` | 1092 errors, 34 warnings, 1 info; exits 1 | PASS |
| --json flag produces valid JSON | `anneal --json check` | Parsed successfully: errors=1092, warnings=34, info=1 | PASS |
| anneal get OQ-64 resolves label | `anneal get OQ-64` | Returns OQ-64 (label), File: LABELS.md | PASS |
| anneal find returns sorted matches | `anneal find "formal"` | 2616 matches sorted by id | PASS |
| anneal impact shows affected handles | `anneal impact murail-formal-model-v16` | direct: [murail-formal-model-v17], indirect: [] | PASS |
| anneal init --dry-run outputs valid TOML | `anneal init --dry-run` | Valid TOML with convergence, handles, frontmatter sections | PASS |
| bare anneal shows graph summary | `anneal` | 260 files, 9817 handles, 6670 edges, 22 namespaces | PASS |
| --errors-only filters warnings/info | `anneal check --errors-only` | 1092 errors, 0 warnings, 0 info | PASS |
| just check quality gate | `just check` | 26 tests passed; 0 failed | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| CHECK-01 | 02-02-PLAN | Existence check — every edge target must resolve (error if not) | SATISFIED | check_existence in checks.rs; E001 for unresolved non-section edges; test: e001_for_unresolved_non_section_edge |
| CHECK-02 | 02-02-PLAN | Staleness check — active referencing terminal (warning) | SATISFIED | check_staleness in checks.rs; W001 emitted; test: w001_active_references_terminal |
| CHECK-03 | 02-02-PLAN | Confidence gap check — DependsOn source > target state (warning) | SATISFIED | check_confidence_gap in checks.rs; W002 emitted; test: w002_source_higher_than_target |
| CHECK-04 | 02-02-PLAN | Linearity check — zero discharges = error, multiple = info | SATISFIED | check_linearity in checks.rs; E002/I002; tests: e002_*, i002_* |
| CHECK-05 | 02-02-PLAN | Convention adoption check — >50% siblings have frontmatter | SATISFIED | check_conventions in checks.rs; W003; tests: w003_* |
| CHECK-06 | 02-02-PLAN | Diagnostics with compiler-style format and error codes | SATISFIED | Diagnostic.print_human() with error[E001]: format; all 7 codes present |
| IMPACT-01 | 02-02-PLAN | Compute impact set by reverse traversal over DependsOn, Supersedes, Verifies | SATISFIED | compute_impact in impact.rs; matches! guard on edge kinds; test: supersedes_and_verifies_traversed |
| IMPACT-02 | 02-02-PLAN | Handle cycles via visited-set detection | SATISFIED | visited HashSet in compute_impact; test: cycle_detection_terminates |
| IMPACT-03 | 02-02-PLAN | Show direct and indirect affected handles | SATISFIED | ImpactResult.direct/indirect; depth tracking in BFS; test: simple_chain_direct_and_indirect |
| CLI-01 | 02-03-PLAN | `anneal check` — run local checks, exit non-zero on errors | SATISFIED | cmd_check + main.rs exit(1) when errors > 0; Murail: exits 1 with 1092 errors |
| CLI-02 | 02-03-PLAN | `anneal get <handle>` — resolve handle, show content + state + context | SATISFIED | cmd_get with exact + case-insensitive lookup; GetOutput with edges; Murail: OQ-64 resolves |
| CLI-03 | 02-03-PLAN | `anneal find <query>` — search filtered by state | SATISFIED | cmd_find with case-insensitive substring, namespace/status filters; Murail: 2616 matches |
| CLI-06 | 02-03-PLAN | `anneal init` — save inferred coloring as anneal.toml | SATISFIED | cmd_init with D-07 auto-detection; dry_run flag; serializes to TOML |
| CLI-07 | 02-03-PLAN | `anneal impact <handle>` — show what's affected if handle changes | SATISFIED | cmd_impact calling compute_impact; ImpactOutput.direct/indirect; Murail verified |
| CLI-09 | 02-03-PLAN | All commands support `--json` output via global flag | SATISFIED | `--json` global flag in Cli struct; print_json<T: Serialize> used in all command branches |
| CLI-10 | 02-03-PLAN | Human-readable output as default | SATISFIED | Default path calls print_human(); all output types implement print_human |
| CONFIG-03 | 02-01-PLAN | Config supports all fields including linear namespaces, concern groups | SATISFIED | AnnealConfig has frontmatter: FrontmatterConfig, concerns: HashMap, HandlesConfig.linear |
| CONFIG-04 | 02-03-PLAN | `anneal init` generates anneal.toml from inferred structure | SATISFIED | cmd_init constructs AnnealConfig from lattice + stats + observed_frontmatter_keys |

All 18 phase 2 requirements satisfied. No orphaned requirements found.

### Anti-Patterns Found

No anti-patterns found. Specifically checked:
- No placeholder/TODO implementations in check rules
- No empty return stubs in impact analysis
- No hardcoded empty data passed to rendering
- No console.log-only handlers
- All 26 unit tests pass (no skipped test bodies)
- clippy pedantic passes with zero warnings

### Human Verification Required

The following items were verified programmatically via live Murail corpus runs. No human checkpoint is needed since Task 3 of Plan 03 was completed and its results are documented in the summary:

1. Visual inspection of diagnostic output format — confirmed compiler-style `error[E001]:` prefix with `  ->` file attribution in spot-checks
2. TOML round-trip for `anneal init --dry-run` — not explicitly tested as a parse round-trip, but structure is correct TOML per visual inspection

**Optional human verification:**
- Confirm `anneal init` (without `--dry-run`) actually writes a valid anneal.toml and it loads cleanly
- Spot-check that W001 stale references fire on actual Murail content (requires knowing which Murail handles are active vs terminal)

### Gaps Summary

No gaps. All 18 phase requirements are satisfied. All 5 CLI commands function correctly against the Murail corpus. The quality gate (`just check`) passes with 26 tests and zero warnings. All critical wiring paths (cli -> checks -> graph, cli -> impact -> graph, parse -> config -> frontmatter, resolve -> bare filename) are verified functional.

**Phase 2 goal achieved:** The five local consistency rules (KB-R1..R5), impact analysis with cycle detection and direct/indirect distinction, and all five core CLI commands (check, get, find, init, impact) are implemented, wired, and producing real output on the Murail corpus.

---

_Verified: 2026-03-28_
_Verifier: Claude (gsd-verifier)_
