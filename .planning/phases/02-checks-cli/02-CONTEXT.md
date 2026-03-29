# Phase 2: Checks & CLI - Context

**Gathered:** 2026-03-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement the five local consistency rules (KB-R1 through KB-R5), impact analysis (reverse graph traversal with cycle detection), and the core CLI commands agents need: check, get, find, init, impact. All commands support --json output via a CommandOutput trait. Includes fixing Phase 1 resolution gaps (bare filename resolution, terminal status classification) that are prerequisites for checks to produce useful output. Includes extensible frontmatter field mapping (CONFIG-03/04) and Phase 1 cleanup items. No convergence tracking (Phase 3). No status/map/diff commands (Phase 3).

</domain>

<decisions>
## Implementation Decisions

### Broken Reference Strategy (Observation #1)
- **D-01:** §N.N section references (~2517 edges) get a single info-level summary diagnostic (I001: "N section references use §N.N notation, not resolvable to heading slugs"). Not per-reference errors — they're a different numbering system that fundamentally can't map to markdown heading slugs.
- **D-02:** Wire the existing `resolve_file_path()` for bare filenames (~870 refs) and frontmatter bare names (~70 refs). Search relative to referring file directory, then root. References that remain unresolved after search are real broken references (E001).
- **D-03:** Fix file path regex to reject URL fragments (~6 false positives) via negative lookbehind for `://`.

### Terminal Status Classification (Observation #2)
- **D-04:** Wire directory convention analysis into `build_graph`. Walk directories during graph construction and tag which statuses appear in `archive/`, `history/`, `prior/` directories. Pass that set to `infer_lattice` as `terminal_by_directory`. Config overrides take precedence. This unblocks CHECK-02 (staleness) and CHECK-03 (confidence gap).

### Frontmatter Extensibility (CONFIG-03)
- **D-05:** Implement fully extensible frontmatter field mapping via `anneal.toml`. All frontmatter fields are configurable — including the 6 current core fields (status, updated, superseded-by, depends-on, discharges, verifies), which become default mappings rather than hardcoded special cases. Config format maps field names to edge kinds with direction (forward/inverse). Example:
  ```toml
  [frontmatter.fields]
  affects = { edge_kind = "DependsOn", direction = "inverse" }
  supersedes = { edge_kind = "Supersedes", direction = "forward" }
  ```
- **D-06:** Zero-config case ships sensible defaults matching the current 6 core fields. Projects opt into additional fields by adding them to anneal.toml.

### Init Auto-Detection (CONFIG-04)
- **D-07:** `anneal init` scans all observed frontmatter keys, identifies ones containing reference-like values (label patterns, file paths, lists of identifiers), and proposes field-to-edge-kind mappings in the generated anneal.toml. User reviews and edits the output.

### Phase 1 Cleanup (Observations #5-9)
- **D-08:** Skip label scanning inside fenced code blocks (``` fences). Eliminates spurious edges from code examples that mention labels like OQ-64.
- **D-09:** Version handles inherit status from their parent file handle. Enables CHECK-02/03 to reason about version convergence state without requiring traversal to the file node.
- **D-10:** Remove unused `version_refs` collection from `scan_file` (collected but never consumed by `build_graph`).
- **D-11:** Remove dead `is_excluded` helper from `parse.rs` (logic already inlined in `filter_entry` closure).

### Claude's Discretion
- Diagnostic formatting details within the compiler-style constraint (CHECK-06): color, alignment, grouping of related diagnostics
- Error code numbering scheme (E001/W001/I001 vs E0001/W0001 etc.)
- Internal architecture of the extensible frontmatter mapping (trait-based, table-driven, etc.)
- How `anneal find` implements full-text search (simple substring, regex, or glob matching)
- Whether `anneal get` shows raw content or formatted output for the resolved handle

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Specification
- `.design/anneal-spec.md` — Authoritative specification (933 lines, 66 labels). Read sections:
  - §7 (Local Checks, KB-D13): Five rules KB-R1 through KB-R5 with exact semantics
  - §7.1 (Theoretical Note): Why local checks suffice without global propagation
  - §8 (Linear Handles, KB-D15): Obligation lifecycle for CHECK-04
  - §9 (Impact Analysis, KB-D16): Reverse traversal semantics for `anneal impact`
  - §12 (Commands): CLI command definitions for check, get, find, init, impact
  - §15 (Implementation): Patterns, dependency rationale, module structure

### Phase 1 Observations
- `.planning/phases/01-graph-foundation/01-OBSERVATIONS.md` — 9 post-execution observations. Items #1 (pending edge resolution), #2 (terminal status), #3 (rich frontmatter), #5-9 (cleanup) are directly addressed by decisions above. **Read this before planning.**

### Phase 1 Summaries
- `.planning/phases/01-graph-foundation/01-03-SUMMARY.md` — Key decisions: `resolve_file_path` exists but unwired, `terminal_by_directory` passed as empty HashSet, version handles from filenames only
- `.planning/phases/01-graph-foundation/01-02-SUMMARY.md` — Key decisions: same-line keyword proximity for edge inference (D-01), `infer_lattice` decoupled from filesystem

### Phase 1 Verification
- `.planning/phases/01-graph-foundation/01-VERIFICATION.md` — 18/18 requirements passed, lists deviations (handle count 9788 vs ~500 estimate, `load_config` signature uses `&Path` not `&Utf8Path`)

### Requirements
- `.planning/REQUIREMENTS.md` — Phase 2 requirements: CHECK-01..06, IMPACT-01..03, CLI-01..03/06..10, CONFIG-03..04

### Test Corpus
- `~/code/murail/.design/` — Primary test corpus (260 files, 120 with frontmatter, 15+ frontmatter field types, 25 status values). Integration tests point here by path.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets (Phase 2 forward stubs from Phase 1)
- `graph.rs`: `node()`, `node_mut()`, `outgoing()`, `incoming()`, `edges_by_kind()` — all exist with `#[allow(dead_code)]`, ready for CHECK rules and impact analysis
- `lattice.rs`: `classify_status()`, `compute_freshness()`, `state_level()`, `frontmatter_adoption_rate()` — all exist, ready for CHECK-02/03/05
- `resolve.rs`: `resolve_file_path()` + `normalize_path()` — exist but unwired, need to be called during edge resolution (D-02)
- `config.rs`: `AnnealConfig` with `convergence`, `handles`, `freshness` sections — will need extension for CONFIG-03 frontmatter field mapping

### Established Patterns
- Arena-indexed graph: `NodeId(u32)` indices, dual adjacency lists (fwd/rev)
- Two-pass resolution: build node index → resolve labels → rebuild index → resolve edges
- `LazyLock<RegexSet>` + individual `LazyLock<Regex>` for scanning
- All types derive `Serialize` for --json support
- `clap::Parser` already on `Cli` struct with `--root` and `--json` flags
- Quality gate: `just check` (fmt + clippy + test) enforced by pre-commit hook

### Integration Points
- `main.rs` line 122: `terminal_by_directory = HashSet::new()` — replace with real directory scanning result
- `main.rs` Cli struct: needs subcommands via `#[command(subcommand)]` (currently flat --root/--json only)
- `parse.rs` `build_graph`: needs code block tracking for label skip (D-08), directory convention collection (D-04)
- `parse.rs` `scan_file`: needs to stop collecting `version_refs` (D-10)
- `resolve.rs` `resolve_pending_edges`: needs to call `resolve_file_path` for bare names (D-02)

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. Follow the spec closely for check rule semantics. The extensible frontmatter mapping (D-05/D-06) is the most architecturally significant addition — design it so zero-config defaults are indistinguishable from the current hardcoded behavior.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 02-checks-cli*
*Context gathered: 2026-03-28*
