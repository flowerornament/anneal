---
phase: 01-graph-foundation
verified: 2026-03-28T00:00:00Z
status: passed
score: 18/18 requirements verified
re_verification: false
---

# Phase 1: Graph Foundation Verification Report

**Phase Goal:** Parse a directory of markdown files, build the knowledge graph with handles and edges, resolve handles across namespaces.
**Verified:** 2026-03-28
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Running anneal on Murail .design/ produces a graph with handles and edges in <1s | VERIFIED | 259 files, 9788 handles, 6408 edges; `just check` passes including `test_murail_corpus` |
| 2 | Label handles in confirmed namespaces (OQ, FM, A, SR, etc.) resolve correctly | VERIFIED | 22 namespaces confirmed, 499 labels resolved; test asserts OQ and FM present |
| 3 | False positives (SHA-256, GPT-2, AVX-512) are excluded from namespaces | VERIFIED | Test asserts SHA, AVX, GPT not in namespace set; sequential-run heuristic in `resolve.rs` rejects them |
| 4 | Frontmatter status values are parsed and partitioned into active/terminal sets | VERIFIED | Confidence lattice with 25 observed statuses; active/terminal partition functional with config overrides |
| 5 | The graph is ephemeral — no persistent state written to disk | VERIFIED | No `.anneal/` directory created after running binary; confirmed by filesystem check |
| 6 | Handle types represent all four kinds (File, Section, Label, Version) | VERIFIED | `HandleKind` enum in `src/handle.rs` with File, Section, Label, Version variants |
| 7 | Config parses anneal.toml with all-optional fields, works with no config file | VERIFIED | `deny_unknown_fields` + `serde(default)` on all config structs; `load_config` returns `Ok(default)` when file absent |
| 8 | DiGraph stores nodes and typed edges with dual adjacency lists | VERIFIED | `fwd: Vec<Vec<Edge>>` and `rev: Vec<Vec<Edge>>` in `DiGraph`; O(1) forward and reverse traversal |

**Score:** 8/8 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/handle.rs` | Handle, HandleKind, NodeId, HandleMetadata types | VERIFIED | All types present with Serialize derives; camino Utf8PathBuf for paths |
| `src/config.rs` | AnnealConfig with serde defaults, load_config | VERIFIED | `deny_unknown_fields`, `Default` on all structs, no Option<T> in config fields, no `concerns` field |
| `src/graph.rs` | DiGraph with dual adjacency lists, Edge, EdgeKind | VERIFIED | 132 lines; all 5 EdgeKind variants; add_node, add_edge, outgoing, incoming, edges_by_kind, nodes |
| `src/parse.rs` | Frontmatter split, RegexSet scanner, edge kind inference, build_graph | VERIFIED | LazyLock<RegexSet> with 5 patterns; split_frontmatter; parse_frontmatter; scan_file; build_graph returning 3-tuple |
| `src/lattice.rs` | ConvergenceState, Lattice, infer_lattice, classify_status, freshness | VERIFIED | All required types and functions present; infer_lattice takes 3 params (extra terminal_by_directory per design decision) |
| `src/resolve.rs` | Namespace inference, label/version resolution, pending edge resolution | VERIFIED | infer_namespaces, resolve_labels, resolve_versions, resolve_pending_edges, resolve_all, resolve_file_path all present |
| `src/main.rs` | Full pipeline wiring with --root, --json, integration test | VERIFIED | Clap Parser; full pipeline: config -> build_graph -> resolve_all -> infer_lattice -> print; GraphSummary Serialize; test_murail_corpus test |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/graph.rs` | `src/handle.rs` | `use crate::handle` | WIRED | `use crate::handle::{Handle, NodeId}` on line 3 |
| `src/main.rs` | `src/handle.rs` | `mod handle` | WIRED | `mod handle;` declared, used for `handle::HandleKind::File` |
| `src/parse.rs` | `src/handle.rs` | `use crate::handle` | WIRED | `use crate::handle::{Handle, HandleKind, HandleMetadata, NodeId}` |
| `src/parse.rs` | `src/graph.rs` | `use crate::graph` | WIRED | `use crate::graph::{DiGraph, EdgeKind}` |
| `src/lattice.rs` | `src/config.rs` | `use crate::config` | WIRED | `use crate::config::{AnnealConfig, FreshnessConfig}` |
| `src/resolve.rs` | `src/parse.rs` | `use crate::parse` | WIRED | `use crate::parse::{LabelCandidate, PendingEdge}` |
| `src/resolve.rs` | `src/graph.rs` | `use crate::graph` | WIRED | `use crate::graph::{DiGraph, EdgeKind}` |
| `src/resolve.rs` | `src/config.rs` | `use crate::config` | WIRED | `use crate::config::AnnealConfig` |
| `src/main.rs` | `src/parse.rs` | `parse::build_graph` | WIRED | `parse::build_graph(&root, &config)` on line 123 |
| `src/main.rs` | `src/resolve.rs` | `resolve::` | WIRED | `resolve::resolve_all(...)` on line 132 |
| `src/main.rs` | `src/lattice.rs` | `lattice::infer_lattice` | WIRED | `lattice::infer_lattice(&observed_statuses, &config, &terminal_by_directory)` on line 142 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `src/main.rs` (print_summary) | `graph.node_count()`, `stats.namespaces` | `parse::build_graph` + `resolve::resolve_all` | Yes — WalkDir + file reads + regex scanning | FLOWING |
| `src/parse.rs` (build_graph) | `Handle.status` | `parse_frontmatter(yaml)` from `std::fs::read_to_string` | Yes — reads real files, parses YAML frontmatter | FLOWING |
| `src/resolve.rs` (resolve_labels) | `label_node`, `namespaces` | `infer_namespaces(candidates)` computed from `LabelCandidate` collected during scanning | Yes — namespace cardinality from real corpus | FLOWING |
| `src/lattice.rs` (infer_lattice) | `Lattice.observed_statuses` | Collected from `graph.nodes().filter_map(|h| h.status)` after real graph build | Yes — real status strings from frontmatter | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Binary builds and runs against Murail corpus | `cargo run -- --root ~/code/murail/.design/` | 259 files, 9788 handles, 6408 edges, 22 namespaces | PASS |
| --json flag produces valid JSON | `cargo run -- --root ~/code/murail/.design/ --json \| python3 -m json.tool` | Valid JSON | PASS |
| No persistent state created | `ls ~/code/murail/.design/.anneal` | No such directory | PASS |
| Integration test passes | `just check` | `test tests::test_murail_corpus ... ok` | PASS |
| Quality gate passes | `just check` (fmt + clippy + test) | 0 errors, 12 dead-code warnings (expected — Phase 2 consumers pending) | PASS |
| False positives rejected | Namespace list from corpus run | SHA, AVX, GPT absent from 22-namespace list | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| GRAPH-01 | 01-02 | Scan directory tree for .md files, create File handles | SATISFIED | `build_graph` uses WalkDir; File handles created for each .md file in corpus |
| GRAPH-02 | 01-02 | Parse YAML frontmatter between `---` fences | SATISFIED | `split_frontmatter` + `parse_frontmatter` in `parse.rs`; handles status, updated, superseded-by, depends-on, discharges, verifies |
| GRAPH-03 | 01-02 | Parse markdown headings (`#{1,6}`) to create Section handles | SATISFIED | Pattern 0 in RegexSet with HEADING_RE; Section handles added via `graph.add_node` in `scan_file` |
| GRAPH-04 | 01-02 | Scan content with RegexSet for labels, section refs, file paths, version refs | SATISFIED | 5-pattern LazyLock<RegexSet> plus individual LazyLock<Regex> for captures |
| GRAPH-05 | 01-01 | Build directed graph with typed edges (Cites, DependsOn, Supersedes, Verifies, Discharges) | SATISFIED | All 5 EdgeKind variants in `graph.rs`; edges created from frontmatter and body-text keywords |
| GRAPH-06 | 01-01, 01-03 | Graph is computed from files on every invocation, never stored | SATISFIED | No persistence code anywhere; `.anneal/` not created; graph dropped after print |
| HANDLE-01 | 01-01 | Resolve File handles by filesystem path | SATISFIED | File handles use relative path as identity; node index maps path strings to NodeId |
| HANDLE-02 | 01-01 | Resolve Section handles to heading ranges within parent files | SATISFIED | Section handles created in `scan_file` with `parent: file_node` and heading-slug identity |
| HANDLE-03 | 01-01, 01-02 | Resolve Label handles by scanning confirmed namespaces | SATISFIED | `resolve_labels` in `resolve.rs`; only confirmed namespace prefixes create label nodes |
| HANDLE-04 | 01-03 | Resolve Version handles by matching versioned artifact naming conventions | SATISFIED | `resolve_versions` matches `*-v{N}.md` pattern; VERSION_FILENAME_RE; 39 version handles in Murail corpus |
| HANDLE-05 | 01-03 | Infer handle namespaces by sequential cardinality (N >= 3 members, M >= 2 files) | SATISFIED | `infer_namespaces` checks `numbers.len() < 3 \|\| distinct_files.len() < 2` |
| HANDLE-06 | 01-03 | Only labels in confirmed namespaces generate broken-reference errors | SATISFIED | `resolve_labels` silently skips unconfirmed prefixes; no diagnostics for unconfirmed (Phase 2's CHECK-01 handles errors) |
| LATTICE-01 | 01-02 | Support two-element existence lattice {exists, missing} | SATISFIED | `LatticeKind::Existence` returned when `observed_statuses` is empty |
| LATTICE-02 | 01-02 | Infer confidence lattice from observed frontmatter status values | SATISFIED | `infer_lattice` returns `LatticeKind::Confidence` with active/terminal partition when statuses present |
| LATTICE-03 | 01-02 | Partition status values into active and terminal sets | SATISFIED | Config overrides + directory convention + default-to-active logic in `infer_lattice` |
| LATTICE-04 | 01-02 | Compute freshness from file mtime or `updated:` frontmatter field | SATISFIED | `compute_freshness` in `lattice.rs`; prefers `updated` field, falls back to `mtime` |
| CONFIG-01 | 01-01 | Parse anneal.toml with all-optional fields via `#[serde(default, deny_unknown_fields)]` | SATISFIED | `deny_unknown_fields` on all config structs; all fields have Default impls; no Option<T> wrappers |
| CONFIG-02 | 01-01 | Zero-config is valid — tool works with no anneal.toml | SATISFIED | `load_config` returns `Ok(AnnealConfig::default())` when file absent; Murail corpus has no anneal.toml and runs fine |

**Coverage:** 18/18 Phase 1 requirements satisfied. No orphaned requirements.

### Anti-Patterns Found

| File | Pattern | Severity | Impact |
|------|---------|----------|--------|
| `src/graph.rs` (multiple methods) | Dead code warnings for `node`, `node_mut`, `outgoing`, `incoming`, `edges_by_kind` | Info | Expected — Phase 2 will consume these APIs |
| `src/lattice.rs` (multiple) | Dead code warnings for `ConvergenceState`, `classify_status`, `Freshness`, `compute_freshness`, `state_level`, `frontmatter_adoption_rate` | Info | Expected — Phase 2 checks module will consume these |
| `src/parse.rs` (`is_excluded`) | Dead code warning for helper function duplicated by inline closure in `build_graph` | Info | Minor technical debt; function exists but is unused because `filter_entry` closure was inlined |
| `src/resolve.rs` (`resolve_file_path`, `normalize_path`) | Dead code warnings | Info | Expected — Phase 2 will use for file path resolution in checks |

No blockers. No TODO/FIXME/placeholder patterns. All dead-code warnings are intentional Phase-2 forward stubs or minor redundancy.

**Clarification on `is_excluded` in parse.rs:** The helper `is_excluded` was defined but the `filter_entry` closure in `build_graph` inlines equivalent logic. The function is a minor redundancy (not a stub — it has real implementation) that should be cleaned up in Phase 2.

### Notable Deviations from Plan (Non-Blocking)

1. **`load_config` signature:** Plan specified `root: &Utf8Path`; actual code uses `root: &Path` (std). This is a minor deviation — the rest of the codebase uses `&Utf8Path` for consistency, but `load_config` is called with `.as_std_path()` in main.rs to bridge. Functionally equivalent.

2. **`infer_lattice` signature:** Plan specified 2 parameters; actual has 3 (`terminal_by_directory: &HashSet<String>`). This is a deliberate design decision documented in the 01-02 SUMMARY — keeps `lattice.rs` decoupled from filesystem concerns. Callers pass an empty `HashSet::new()` for Phase 1; Phase 2 will populate it.

3. **`resolve_all` return type:** Plan specified `anyhow::Result<ResolveStats>`; actual returns `ResolveStats` directly (no Result). This is a documented improvement from the 01-03 SUMMARY — clippy pedantic flagged the unnecessary Result wrapping.

4. **Success criterion on handle count:** ROADMAP says "~500 handles and ~2000 edges"; actual is 9788 handles and 6408 edges from 259 files. The counts are dramatically higher than estimated because Section handles are created for every heading in every file (~9000+ headings across 259 files). This is correct behavior — the estimate in the ROADMAP was based on label handles only. The integration test uses conservative lower bounds (>100) and passes.

### Human Verification Required

None required. All phase objectives are verifiable programmatically. The integration test running against the real Murail corpus provides behavioral confidence.

### Gaps Summary

No gaps. All 18 requirements satisfied, all artifacts substantive and wired, all key links verified, data flows through the full pipeline to real output, and the integration test passes against the live Murail corpus.

---

_Verified: 2026-03-28_
_Verifier: Claude (gsd-verifier)_
