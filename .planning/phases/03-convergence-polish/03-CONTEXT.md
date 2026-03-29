# Phase 3: Convergence & Polish - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning

<domain>
## Phase Boundary

Add convergence tracking (JSONL snapshots, convergence summary, graph diff), the `status`, `map`, and `diff` commands, and a suggestion engine — the features that make anneal more than a linter. This is the final phase: after this, anneal delivers its full spec.

</domain>

<decisions>
## Implementation Decisions

### Snapshot Infrastructure (CONVERGE-01, CONVERGE-02, CONVERGE-05)
- **D-01:** `snapshot.rs` is a new module. Hand-roll JSONL append/read per spec §15.2 (~30 lines): `serde_json::to_vec` + `\n` + single `write_all` to `O_APPEND` file. Read via `BufReader::lines()` + `serde_json::from_str`, skip unparseable lines (handles truncation from interrupted writes).
- **D-02:** Auto-create `.anneal/` directory on first snapshot write. No init ceremony required. `.anneal/` is already in the default exclusion list (§5.1/KB-D20). CONVERGE-05: if history file is missing, return empty history; if corrupted, skip bad lines and warn to stderr.
- **D-03:** Snapshot schema per spec §10/KB-D17: timestamp, handle counts (total/active/frozen), edge count, state histogram, obligation status (outstanding/discharged/mooted), diagnostic counts, namespace stats (total/open/resolved/deferred per namespace).
- **D-04:** Both `anneal check` and `anneal status` append a snapshot after running (spec §12.1, §12.4 are explicit). The snapshot captures the current graph state at that invocation.

### Convergence Summary (CONVERGE-03)
- **D-05:** Per spec §10.1/KB-D18, compute a one-line convergence signal from snapshot delta: **advancing** (more resolution than creation, obligations caught up, freshness improving), **holding** (balanced), or **drifting** (more creation than resolution, obligations accumulating, freshness declining). The signal compares current snapshot against the most recent previous snapshot.
- **D-06:** If no previous snapshot exists, report "no history" rather than defaulting to a state. First snapshot establishes the baseline.

### Graph Diff (CONVERGE-04)
- **D-07:** `anneal diff` (no args) diffs current graph against the most recent JSONL snapshot. Shows: new handles created, handles whose state changed, obligations created/discharged, edges added/broken, namespace statistics delta — per spec §10.2/KB-D19.
- **D-08:** `anneal diff --days=N` finds the snapshot closest to N days ago and diffs against it. Simple timestamp filter over JSONL history.
- **D-09:** `anneal diff HEAD~N` (git-aware mode) shells out to `git` to reconstruct files at the given ref, re-runs the full parse pipeline against those files, and diffs the resulting graph against the current graph. This gives true structural diff, not just snapshot-based. Implementation: use `git show <ref>:<path>` or `git archive <ref>` to a temp directory, run `build_graph` + `resolve_all` + `infer_lattice` on the temp copy, then diff the two graph states.

### Status Dashboard (CLI-04)
- **D-10:** Per spec §12.4/KB-C4, `anneal status` is a dashboard showing: file/handle/edge counts, active/frozen partition, pipeline histogram (handle counts per ordering level), obligation summary, diagnostic counts, convergence summary (from D-05), and suggestion count. Appends a snapshot (D-04).
- **D-11:** Pipeline histogram requires `lattice.ordering` to be non-empty. If ordering is a flat set (no pipeline), show active/terminal counts instead of a pipeline flow.

### Map Command (CLI-05)
- **D-12:** `anneal map` renders the active knowledge graph in text format (default) or graphviz DOT (`--format=dot`). Text format is Claude's discretion — choose whatever is most useful for terminal output.
- **D-13:** `--concern="name"` extracts the subgraph for a concern group (from config). `--around=<handle> --depth=N` does BFS from the handle to depth N, showing the neighborhood. Both are subgraph extraction before rendering.
- **D-14:** Full active graph may be large. Default text rendering should be useful even for 100+ node graphs — consider grouping by namespace or kind, not just flat lists.

### Suggestions (SUGGEST-01 through SUGGEST-05)
- **D-15:** Suggestions are a fourth severity level (`Suggestion`) in the existing diagnostic system, slotted below `Info`. This reuses the entire diagnostic infrastructure (formatting, --json, filtering).
- **D-16:** `anneal check --suggest` filters to suggestion diagnostics only (parallel to spec's `--stale` and `--obligations` filter flags). `anneal status` counts suggestions and directs users to `anneal check --suggest`.
- **D-17:** Five suggestion types per KB-E8:
  - SUGGEST-01: Orphaned handles — nodes with no incoming edges (excluding File handles, which are roots)
  - SUGGEST-02: Candidate namespaces — recurring label-like patterns not yet in confirmed namespaces
  - SUGGEST-03: Pipeline stalls — state levels with high population and no outgoing DependsOn edges to next level
  - SUGGEST-04: Abandoned namespaces — all members in terminal state or stale beyond freshness threshold
  - SUGGEST-05: Concern group candidates — labels frequently co-occurring across files
- **D-18:** Suggestions go in `checks.rs` alongside existing rules, or a `suggest.rs` module if they grow large enough. Each is a graph query, not a content heuristic (KB-P5).

### Check Command Enhancement
- **D-19:** Add `--suggest`, `--stale`, and `--obligations` filter flags to `anneal check` per spec §12.1. These are additional filters alongside the existing `--errors-only`. `--suggest` shows only suggestions; `--stale` shows only staleness diagnostics (W001); `--obligations` shows only linearity diagnostics.
- **D-20:** `anneal check` now appends a snapshot after running (D-04). This is a new side effect — currently `check` is read-only.

### Claude's Discretion
- Text rendering format for `anneal map` default output (D-12/D-14)
- Snapshot comparison heuristics for advancing/holding/drifting thresholds (D-05)
- Whether suggestions live in `checks.rs` or a separate `suggest.rs` (D-18)
- Error codes for suggestion diagnostics (S001-S005 or similar)
- How to present pipeline histogram in `status` when ordering has many levels

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Specification
- `.design/anneal-spec.md` — Authoritative specification (933 lines, 66 labels). Read sections:
  - §10 (Convergence Tracking, KB-D17): Snapshot format, JSONL design, append semantics
  - §10.1 (Convergence Summary, KB-D18): Advancing/holding/drifting signal computation
  - §10.2 (Graph Diff, KB-D19): Diff semantics — new handles, state changes, obligations, edges, namespace deltas
  - §11 (Derived Capabilities): KB-E4 (pipeline tracking), KB-E8 (suggestions — all five patterns), KB-E10 (convergence monitoring)
  - §12.1 (check command): `--suggest`, `--stale`, `--obligations` filter flags + snapshot append
  - §12.4 (status command, KB-C4): Dashboard layout with pipeline histogram + convergence summary
  - §12.5 (map command, KB-C5): `--concern`, `--around`, `--format=dot`
  - §12.8 (diff command, KB-C8): Three reference modes — default, `--days`, git ref
  - §15.2 (Hand-rolled JSONL): Append/read implementation pattern (~30 lines)
  - §15.3 (Snapshot append): `O_APPEND`, single `write_all`, `BufReader::lines()` on read

### Requirements
- `.planning/REQUIREMENTS.md` — Phase 3 requirements: CONVERGE-01..05, CLI-04/05/08, SUGGEST-01..05

### Phase 2 Code
- `src/checks.rs` — Existing diagnostic system (Severity, Diagnostic, run_checks). Suggestions extend this.
- `src/cli.rs` — Existing CommandOutput pattern, print_json helper. New commands follow this pattern.
- `src/main.rs` — CLI dispatch. New subcommands (Status, Map, Diff) added to Command enum.
- `src/lattice.rs` — Lattice with ordering, active/terminal sets. Used by pipeline histogram and convergence summary.
- `src/graph.rs` — DiGraph traversal methods. Used by map rendering and suggestion queries.

### Test Corpus
- `~/code/murail/.design/` — Primary test corpus. Integration tests validate status/map/diff/suggestions against this corpus.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `checks.rs`: `Severity` enum, `Diagnostic` struct, `run_checks()`, `print_human()` — extend for suggestions
- `cli.rs`: `print_json()`, `CommandOutput` pattern (Serialize + print_human), `build_summary()` — status builds on the summary pattern
- `graph.rs`: `outgoing()`, `incoming()`, `nodes()`, `edges_by_kind()`, `node_count()`, `edge_count()` — all needed for map/suggestions
- `lattice.rs`: `Lattice` with `active`, `terminal`, `ordering`, `observed_statuses` — convergence summary and pipeline histogram
- `impact.rs`: `compute_impact()` with reverse BFS — pattern for map's `--around` subgraph extraction
- `resolve.rs`: `build_node_index()` — reused for map/diff handle lookup

### Established Patterns
- Arena-indexed graph with `NodeId(u32)` — all new code uses this
- `Serialize` on all output structs for `--json`
- `print_human(&self, w: &mut dyn Write)` on all output structs
- `clap::Subcommand` derive for CLI dispatch
- `#[arg(long)]` for flags, global `--json`
- Broken pipe handling in `main()` — all new commands get this for free

### Integration Points
- `main.rs` `Command` enum: add `Status`, `Map`, `Diff` variants
- `main.rs` match dispatch: add arms for new commands, wire snapshot append after check/status
- `snapshot.rs`: new module, declared in `main.rs` with `mod snapshot;`
- `checks.rs` `Severity` enum: add `Suggestion` variant below `Info`
- `checks.rs` `run_checks()`: add suggestion generation calls
- `.anneal/` directory: created by snapshot write, excluded from scan (already in default exclusions)

</code_context>

<specifics>
## Specific Ideas

No specific requirements — the spec is highly prescriptive. Follow it closely. The snapshot format in §10 is exact. The status dashboard layout in §12.4 is exact. The diff output format in §12.8 is exact. The five suggestion types in KB-E8 are exact.

Be ambitious with `anneal diff HEAD~N` — shell out to git, reconstruct the full graph at that ref, produce a true structural diff.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope. This is the final phase of v1.

</deferred>

---

*Phase: 03-convergence-polish*
*Context gathered: 2026-03-29*
