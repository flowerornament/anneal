---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Parser Hardening & UX Polish
status: Defining requirements
last_updated: "2026-03-29"
progress:
  total_phases: 0
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
---

# State: anneal

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-29)

**Core value:** An arriving agent can immediately understand what's settled, what's drifting, what's connected, and where to push next.
**Current focus:** Defining requirements for v1.1

## Current Position

Phase: Not started (defining requirements)
Plan: —
Status: Defining requirements
Last activity: 2026-03-29 — Milestone v1.1 started

## Progress

| Phase | Status | Completed |
|-------|--------|-----------|
| (v1.0) Phase 1: Graph Foundation | Complete | Plans 01, 02, 03 done |
| (v1.0) Phase 2: Checks & CLI | Complete | Plans 01, 02, 03 done |
| (v1.0) Phase 3: Convergence & Polish | Complete | Plans 01, 02, 03, 04, 05 done |

## Decisions

- Namespace stats deferred field left at 0 (per-handle freshness not available at snapshot time)
- Convergence signal: frozen delta vs total delta with obligations tiebreaker
- Module-level dead_code allow for snapshot types not yet consumed by CLI
- Enable serde1 feature on camino for Utf8PathBuf serialization
- Enable serde feature on chrono for NaiveDate serialization
- Use expect() for u32 overflow guard in DiGraph::add_node
- parse_frontmatter returns plain value (not Result) since it never errors
- Same-line keyword proximity for body-text edge kind inference (D-01)
- infer_lattice accepts terminal_by_directory parameter to decouple lattice from filesystem
- Label candidates collected but not resolved -- namespace inference deferred to resolve.rs
- resolve_all returns ResolveStats directly (not Result) since resolution never fails
- Version handles from filename patterns only (*-vN.md), not body text v-refs (too noisy)
- Empty terminal_by_directory for Phase 1 lattice -- directory convention deferred to Phase 2
- False positive rejection: large min number + no 3-consecutive run = rejected
- URL rejection via prefix check (contains "://") not regex lookaround (RegexSet incompatible)
- Terminal-by-directory uses exclusive-presence heuristic (only in terminal dirs, never in non-terminal)
- Table-driven frontmatter parsing via FrontmatterConfig.fields HashMap
- node_mut() keeps dead_code allow since D-09 implemented at construction time
- check_linearity takes &Lattice for terminal status mooting (KB-R4)
- check_conventions groups files by parent directory path string
- Concrete enum dispatch for CLI output (Serialize not object-safe, no trait objects)
- cmd_find searches handle identities only, not file content
- Init D-07 proposes Cites as default fallback for unknown frontmatter fields
- observed_frontmatter_keys collected via second YAML parse in build_graph
- CheckFilters struct encapsulates four boolean filter flags to satisfy clippy pedantic
- Suggestions in checks.rs alongside existing rules (not separate suggest.rs)
- BTreeMap for namespace grouping in S004 ensures deterministic output order
- S004 checks both terminal status AND freshness via compute_freshness
- S005 limits to top 5 co-occurring pairs to avoid noise
- MapOptions struct bundles 8 cmd_map parameters (clippy too_many_arguments)
- File handles always included in default map subgraph regardless of terminal status
- Ord/PartialOrd derives on NodeId for BTreeSet edge deduplication
- ObligationDelta fields keep _delta suffix with clippy allow for JSON schema clarity
- Git ref extraction via git archive | tar for single-subprocess file recovery
- Convergence computed in main.rs (not cli.rs) since it requires snapshot I/O
- Flat lattice shows Active/Terminal counts instead of pipeline histogram (D-11)
- Check arm restructured: compute diagnostics once, build snapshot before filtering, append after output

## Session Log

| Date | Phase | What happened |
|------|-------|---------------|
| 2026-03-28 | — | Project initialized from design conversation. Spec complete (917 lines). |
| 2026-03-28 | 01 | Plan 01 complete: Handle/Config/DiGraph type system with arena-indexed graph, dual adjacency lists, zero-config anneal.toml. |
| 2026-03-28 | 01 | Plan 02 complete: parse.rs with frontmatter split + 5-pattern RegexSet + edge kind inference; lattice.rs with existence/confidence lattice + active/terminal partition + freshness. |
| 2026-03-29 | 01 | Plan 03 complete: resolve.rs with namespace inference + label/version/edge resolution; main.rs pipeline wired end-to-end. Murail corpus: 259 files, 9788 handles, 6408 edges, 22 namespaces. |
| 2026-03-29 | 02 | Plan 01 complete: Foundation repairs -- code block skip, URL rejection, bare filename resolution, extensible FrontmatterConfig with 6 defaults, directory convention terminal status, version status inheritance. Murail: 3191 unresolved (down from 3396). |
| 2026-03-29 | 02 | Plan 02 complete: checks.rs with 5 check rules (KB-R1..R5), 7 diagnostic codes, compiler-style formatting. impact.rs with reverse BFS, cycle detection, direct/indirect distinction. 19 new tests. |
| 2026-03-29 | 02 | Plan 03 complete: cli.rs with 5 subcommands (check, get, find, init, impact). Clap dispatch, --json on all commands, D-07 frontmatter auto-detection. Murail: 1092 errors, 34 warnings, 1 info. |

| 2026-03-29 | 03 | Plan 01 complete: snapshot.rs with Snapshot type, JSONL I/O (append/read), convergence summary (advancing/holding/drifting), Severity::Suggestion variant. 10 new tests. |
| 2026-03-29 | 03 | Plan 02 complete: Five suggestion rules S001-S005 in checks.rs (orphaned, candidate ns, pipeline stalls, abandoned ns, concern groups). CheckFilters struct with --suggest/--stale/--obligations. 12 new tests. |
| 2026-03-29 | 03 | Plan 03 complete: anneal map command with text/DOT rendering, BFS --around, --concern filtering. MapOptions struct, 8 tests. |
| 2026-03-29 | 03 | Plan 04 complete: anneal status dashboard (8 lines matching spec 12.4), snapshot append on status+check (D-04/D-20), convergence tracking. 11 new tests. |

---
*Last updated: 2026-03-29 after Plan 03-04 completion*
