---
status: active
---

# Anneal Code Review — 2026-04-08

Comprehensive review of the anneal codebase (v0.5.0, ~17K lines, 18 source files).
Six parallel review passes: type system, architecture, performance, error handling,
code clarity, and testing. Findings are deduplicated and cross-referenced below.

---

## Executive Summary

The codebase is well above average for a single-developer Rust CLI. Strict Clippy
pedantic + deny policy, consistent `pub(crate)` discipline, arena-indexed graph with
O(1) access, spec-traceable comments, and clean pipeline architecture (parse → resolve
→ lattice → check → render) are all strong foundations.

The review found **1 bug**, **6 high-value improvements**, and **~30 medium/low findings**
across the six dimensions. No security vulnerabilities, no unsafe code, no panics
reachable in normal operation.

### Top findings at a glance

| #  | Finding | Severity | Section |
|----|---------|----------|---------|
| 1  | Off-by-one in frontmatter line count (`parse.rs:904`) | **Bug** | §1 |
| 2  | `debug_assert_eq!` guards a correctness invariant in release builds | **Medium** | §1 |
| 3  | `resolved_file` allocates on every call; should return `&Utf8Path` | **Medium** | §3 |
| 4  | `Severity` serialization inconsistency (PascalCase vs lowercase) | **Medium** | §2 |
| 5  | `cli.rs` is 4452 lines — largest file by 2x, natural split points exist | **Medium** | §4 |
| 6  | `run_checks` takes 9 positional args behind `#[allow(too_many_arguments)]` | **Medium** | §4 |
| 7  | 4 modules with zero tests: lattice, graph, obligations, analysis | **Medium** | §6 |
| 8  | 6 stale `#[allow(dead_code)]` and stale Phase 2 comments | **Medium** | §5 |
| 9  | Duplicate `fnv1a_64` implementation across identity.rs and snapshot.rs | **Medium** | §5 |
| 10 | Diagnostic codes are `&'static str` — enum would be exhaustive | **Medium** | §2 |

---

## §1. Bugs and Correctness Risks

### 1.1 Off-by-one in body line numbers (BUG)

**File:** `parse.rs:902-904`

```rust
let frontmatter_line_count = frontmatter_yaml.map_or(0, |yaml| {
    // +2 for the opening and closing --- lines
    yaml.lines().count() as u32 + 2
});
```

The `+2` counts both `---` fences. But `LineIndex::from_content` (extraction.rs:63-65)
documents that `frontmatter_line_count` should include the opening `---` but NOT the
closing `---` — it adds the closing fence itself via `+1` in the `base_line` formula:

```rust
base_line = frontmatter_line_count + 1 + 1  // closing --- + 1-based
```

Trace for `---\nstatus: active\n---\nBody`:
- `yaml.lines().count()` = 1
- Current: `frontmatter_line_count` = 3, `base_line` = 5. Body starts at reported line 5.
- Correct: `frontmatter_line_count` = 2, `base_line` = 4. Body actually starts at line 4.

**Every body-text line number in diagnostics is 1 too high for files with frontmatter.**
Fix: change `+2` to `+1`.

### 1.2 `debug_assert_eq!` guards release-critical invariant

**File:** `parse.rs:930-973`

A `NodeId` placeholder is computed before `graph.add_node()`, and a `debug_assert_eq!`
verifies they match. In release builds, this assertion is compiled away. If a code change
inserts a node between the placeholder calculation and the `add_node` call, pending edges
would silently point to the wrong node with no runtime error.

**Recommendation:** Promote to `assert_eq!`, or restructure to eliminate the placeholder
pattern (add the node first, then build pending edges referencing it).

### 1.3 `expect()` on evidence serialization

**File:** `identity.rs:20`

```rust
serde_json::to_string(value).expect("evidence serializes")
```

Low risk since `Evidence` contains only strings and integers, but this is the only
`expect()` in the diagnostic pipeline. If a future `Evidence` variant contains a type
that fails to serialize, this panics in production rather than degrading gracefully.

---

## §2. Type System and Correctness by Construction

### 2.1 Diagnostic codes should be an enum (MEDIUM)

**File:** `checks.rs:103` — `code: &'static str`

Codes like `"E001"`, `"W001"`, `"S003"` are matched by string throughout
`diagnostic_descriptor()`, `widen_for_code()`, `is_stale_code()`, and
`is_obligation_code()`. A `DiagnosticCode` enum would make the compiler enforce
exhaustive handling, eliminate typo risk, and let `diagnostic_descriptor` become a
simple match arm rather than a string lookup table.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum DiagnosticCode {
    E001, E002, W001, W002, W003, W004, I001, I002,
    S001, S002, S003, S004, S005,
}
```

### 2.2 Severity serialization inconsistency (MEDIUM)

**File:** `checks.rs:77`

The derived `Serialize` produces `"Error"` (PascalCase), but `Severity::as_str()`
returns `"error"` (lowercase). `DiagnosticRecord` manually converts via
`diagnostic.severity.as_str().to_string()`. JSON consumers see different casing
depending on which serialization path is used.

**Fix:** Add `#[serde(rename_all = "lowercase")]` to `Severity`.

### 2.3 `Handle.id` conflates different identity domains (MEDIUM)

**File:** `handle.rs:56`

File identities are paths, label identities are `PREFIX-N`, section identities are
`file#heading`, version identities are `artifact-vN`, external identities are URLs.
All share a single `String` field and a single `HashMap<String, NodeId>` namespace
in `node_index`. A newtype `HandleIdentity` with per-kind variants would prevent
cross-type key collisions and make the identity structure explicit.

### 2.4 Stringly-typed status values (LOW)

`Handle.status` is `Option<String>`. A `Status(String)` newtype would distinguish
validated status strings from arbitrary strings in the lattice sets and comparisons.

### 2.5 `RefHint::Implausible { reason: String }` — reason is always one of four literals (LOW)

An `ImplausibleReason` enum would be cleaner than allocating strings.

### 2.6 `Some(1)` as sentinel for unknown line numbers (LOW)

**File:** `checks.rs` (lines 411, 595, 651, 729, etc.)

Many diagnostics emit `line: Some(1)` when the real line is unknown. This conflates
"actually line 1" with "unknown line." Use `None` for unknown.

### 2.7 `resolved_file` returns `Option<String>` — should borrow (MEDIUM)

**File:** `handle.rs:76-89`

```rust
pub(crate) fn resolved_file(handle: &Handle, graph: &DiGraph) -> Option<String>
```

Allocates a new `String` on every call. Called from checks, queries, obligations,
impact, and CLI rendering — hundreds of times per invocation. Should return
`Option<&Utf8Path>`:

```rust
pub(crate) fn resolved_file<'a>(handle: &'a Handle, graph: &'a DiGraph) -> Option<&'a Utf8Path> {
    handle.file_path.as_deref().or_else(|| match &handle.kind {
        HandleKind::Version { artifact, .. } => graph.node(*artifact).file_path.as_deref(),
        _ => None,
    })
}
```

Callers that need `String` can `.map(ToString::to_string)` at the call site.

### 2.8 `Snapshot.timestamp` is `String` instead of `DateTime<Utc>` (LOW)

**File:** `snapshot.rs:54`

Chrono is already a dependency with serde support. Using `DateTime<Utc>` would enable
date arithmetic without re-parsing.

---

## §3. Performance and Efficiency

The codebase is well-structured for performance: regexes compiled once via `LazyLock`,
arena-indexed graph, read-once I/O, O(V+E) algorithms. No O(n²) traps in hot paths.

### 3.1 `read_latest_snapshot` reads entire history file (MEDIUM)

**File:** `snapshot.rs:307-330`

Reads and parses every JSONL line to find the last one. After a year of daily use
(365+ lines), this is linear in history length. Reading the file backwards to find
the last newline would make it O(1).

### 3.2 `try_version_stem` scans all node_index keys per unresolved edge (MEDIUM)

**File:** `resolve.rs:535-565`

Iterates every key in `node_index` with a regex for each unresolved edge. With 500
nodes and 20 unresolved edges, that's 10K regex evaluations. A pre-built
`HashMap<base_name, Vec<(version, full_key)>>` would make this O(1) per lookup.

### 3.3 `classify_frontmatter_value` called twice per target (LOW)

**File:** `parse.rs:936, 1016`

Each frontmatter edge target is classified twice — once for `PendingEdge` construction
and once for `FileExtraction`. Store the result from the first call and reuse it.

### 3.4 `state_level` uses linear scan in checks (LOW)

**File:** `lattice.rs:168-173`

The `query` module already builds a `HashMap<&str, usize>` via `build_state_levels()`.
The check module should do the same in `check_confidence_gap` for consistency and O(1)
lookup.

### 3.5 Label snippet extraction re-scans full file per label (LOW)

**File:** `extraction.rs:159-176`, called from `parse.rs:1006`

Could be integrated into the cmark scan pass to avoid re-scanning file content.

### 3.6 File scanning is sequential (FUTURE)

**File:** `parse.rs:865`

The `build_graph` file scanning loop is embarrassingly parallel. A two-phase approach
(parallel read + parse, then sequential graph mutation) with rayon would help at
500+ files. Not urgent for current corpus sizes.

### 3.7 `EdgeKind::Custom(String)` inflates Edge size (LOW)

The five well-known variants are zero-size, but `Custom(String)` makes every `Edge` 40
bytes. `Custom(Box<str>)` would save 8 bytes per edge if custom edges are rare.

---

## §4. Architecture and Modularity

### 4.1 `cli.rs` should be split (MEDIUM)

At 4452 lines, `cli.rs` is 26% of the codebase and contains output types, human
rendering, JSON builders, command orchestration, a BFS graph renderer, and a diff
engine. Natural splits:

- `cli/output.rs` — `JsonEnvelope`, `OutputMeta`, `DetailLevel`, `print_json`
- `cli/check.rs` — `cmd_check`, `CheckOutput`
- `cli/status.rs` — `cmd_status`, `StatusOutput`
- `cli/map.rs` — `cmd_map`, BFS neighborhood, DOT renderer (~400 lines)
- `cli/diff.rs` — `cmd_diff`, snapshot comparison (~350 lines)

### 4.2 `run_checks` parameter explosion (MEDIUM)

**File:** `checks.rs:1249-1260`

Takes 9 positional parameters with `#[allow(clippy::too_many_arguments)]`.
`AnalysisContext` already bundles most of these. A `CheckInput` struct or accepting
`&AnalysisContext` directly would clean this up.

### 4.3 `graph.rs` ↔ `handle.rs` circular dependency (LOW)

`resolved_file` in `handle.rs` imports `DiGraph`. Moving it to `resolve.rs` or a
new `handle_ops.rs` would break the cycle.

### 4.4 `lattice.rs` → `parse.rs` layering inversion (LOW)

`infer_lattice` calls `crate::parse::is_terminal_by_heuristic`. This pure function
on a string should live in `lattice.rs` itself.

### 4.5 `parse_frontmatter` returns a 4-tuple (LOW)

**File:** `parse.rs:118-126`

A `FrontmatterParseResult` struct would improve readability at call sites.

### 4.6 Obligation counting duplicated between `snapshot.rs` and `obligations.rs` (LOW)

`snapshot::build_snapshot` reimplements discharge counting logic that already exists
in `obligations.rs`. Should delegate.

---

## §5. Code Clarity and Hygiene

### 5.1 Duplicate `fnv1a_64` implementation (MEDIUM)

**Files:** `identity.rs:3-9`, `snapshot.rs:429-436`

Functionally identical. Extract to a shared utility.

### 5.2 Stale `#[allow(dead_code)]` annotations (MEDIUM)

| File | Item | Status |
|------|------|--------|
| `lattice.rs:14` | `ConvergenceState` | Dead — never used. Remove. |
| `lattice.rs:103` | `classify_status` | Dead — never called. Remove. |
| `lattice.rs:114,124,136` | `FreshnessLevel`, `Freshness`, `compute_freshness` | **Used** by `checks.rs` S004. Remove the `#[allow(dead_code)]`. |
| `graph.rs:120` | `node_mut` | Dead unless Phase 2 still planned. |
| `resolve.rs:65` | `Resolution` enum | Dead — cascade uses `CascadeResult` instead. Remove. |
| `explain.rs:63-142` | 7 structs with allow | Structs are used; only the `Explanation` wrapper enum is dead. |
| `parse.rs:767` | `ExternalRef` | Comment says "when wired" but external URLs ARE wired. Stale. |

### 5.3 Stale Phase 2 comments (LOW)

`lattice.rs:13,102,113,135` and `graph.rs:119` reference "Phase 2" as future work,
but the features they anticipate (CHECK-02 staleness, CHECK-03 confidence gap) are
implemented. Clean up.

### 5.4 Repeated `.file_path.as_ref().map(ToString::to_string)` (LOW)

Appears 15+ times. A `Handle::file_path_str()` helper would reduce noise (though
this becomes moot if `resolved_file` returns `&Utf8Path` per §2.7).

### 5.5 `HashMap<String, usize>` in `summarize_extractions` — keys are literals (LOW)

**File:** `cli.rs:368-375`

Allocates `"label".to_string()` etc. as map keys. `HashMap<&'static str, usize>`
avoids these allocations.

### 5.6 `EdgeKind::from_name` only matches two case variants (LOW)

**File:** `graph.rs:42-50`

Matches `"Cites" | "cites"` but misses `"CITES"`, `"dependsOn"`, etc. Using
`eq_ignore_ascii_case` would be more robust.

---

## §6. Testing and Quality Infrastructure

### 6.1 Test inventory

**208 tests, 206 passing, 2 ignored** (external corpus smoke tests).

| Module | Lines | Tests | Assessment |
|--------|-------|-------|------------|
| cli.rs | 4,452 | 42 | Good |
| checks.rs | 2,395 | 38 | Good |
| parse.rs | 1,871 | 28 | Good |
| query.rs | 1,715 | 11 | Adequate |
| explain.rs | 1,407 | 9 | Adequate |
| snapshot.rs | 959 | 12 | Good |
| resolve.rs | 879 | 10 | Good |
| extraction.rs | 604 | 28 | Good |
| config.rs | 470 | 10 | Adequate |
| impact.rs | 297 | 9 | Good |
| **lattice.rs** | **188** | **0** | **Missing** |
| **graph.rs** | **159** | **0** | **Missing** |
| **obligations.rs** | **144** | **0** | **Missing** |
| **analysis.rs** | **123** | **0** | **Missing** |
| handle.rs | 148 | 1 | Minimal |
| identity.rs | 96 | 3 | Adequate |

### 6.2 Highest-value test additions (prioritized)

1. **`lattice.rs`** — `infer_lattice()` is the convergence foundation. Zero tests is
   the single biggest gap. Cover: empty statuses, config overrides, heuristic detection,
   directory conventions, ordering lookup.

2. **`split_frontmatter()`** — Parse-critical function with zero direct unit tests.
   Edge cases: CRLF, EOF without trailing newline, empty frontmatter, no frontmatter.

3. **`graph.rs`** — Core data structure. Test: node/edge insertion, dual adjacency,
   `edges_by_kind` filtering, `EdgeKind::from_name` round-trip.

4. **`obligations.rs`** — Obligation disposition logic. Test: mooted/outstanding/
   discharged/multi-discharged classification, namespace filtering.

5. **Config `deny_unknown_fields` rejection** — Verify unknown TOML fields produce
   parse errors. Currently only valid configs are tested.

6. **`analysis.rs` path matching** — `matches_scoped_file()` and
   `retain_diagnostics_for_file()` have prefix-stripping edge cases.

### 6.3 Test infrastructure issues

- **Duplicated test helpers**: `make_lattice()`, `make_file_handle()`,
  `make_label_handle()` copied across 5 modules. Extract to shared `#[cfg(test)]`
  factory methods.

- **Raw temp dirs in parse.rs**: 10 tests use `/tmp/anneal_test_*` manually instead
  of the `tempfile` crate (already in dev-dependencies). Fragile on test failure.

- **No snapshot/golden tests**: Output formats (status dashboard, check output, map
  rendering, diff) are user-visible and spec-controlled but have no golden tests.
  `insta` would be high-value here.

- **No fuzz targets**: `split_frontmatter`, `classify_frontmatter_value`,
  `scan_text_for_refs` all process untrusted input and would benefit from fuzz coverage.

### 6.4 CI pipeline (`just check`)

Runs: `cargo fmt --check` → `bash -n install.sh` → `cargo clippy --all-targets` →
`cargo test`. Solid. The Clippy config (deny all + pedantic with 8 targeted allows)
is appropriately strict.

---

## §7. Error Handling

### 7.1 Strategy assessment

`anyhow` is used consistently and appropriately. `?` propagation is clean. Broken-pipe
handling in `main()` is a nice touch. Error messages include file paths via
`anyhow::Context`.

### 7.2 Silent frontmatter failures (LOW)

**File:** `parse.rs:127-129`

Malformed YAML frontmatter silently returns defaults. A user with a typo in their
`status:` field gets no indication. Consider emitting a diagnostic for unparseable
frontmatter.

### 7.3 Non-UTF-8 filenames silently skipped (LOW)

**File:** `parse.rs:848-850`

Correct behavior (the whole codebase uses `Utf8Path`), but users get no indication
that some files were invisible to the scan.

---

## §8. Positive Observations

These are things done well that should be preserved:

- **No `unsafe` code** — denied at the lint level
- **No `&String` or `&Vec<T>` anti-patterns** — consistently uses `&str` and `&[T]`
- **Arena-indexed graph** with `NodeId(u32)` — efficient, cache-friendly
- **Spec traceability** — KB-D1, LATTICE-02, CHECK-06, etc. throughout
- **Clean pipeline architecture** — data flows forward without backtracking
- **`pub(crate)` discipline** — no accidental `pub` leaks
- **`deny_unknown_fields`** on config structs — catches typos at parse time
- **`AnalysisContext` pattern** — bundles pipeline outputs, prevents parameter explosion
- **`Evidence` enum with serde tags** — clean JSON schema
- **Deterministic output** — sorting by handle ID, severity ordering, pagination
- **Test factories** — `Handle::test_file()`, `Handle::test_label()` behind `#[cfg(test)]`
- **Import organization** — consistent std → external → crate ordering

---

## Recommended Action Order

For maximum impact with minimum disruption:

**Quick wins (< 1 hour each):**
1. Fix the line number off-by-one (`+2` → `+1` at `parse.rs:904`)
2. Add `#[serde(rename_all = "lowercase")]` to `Severity`
3. Remove dead code: `ConvergenceState`, `classify_status`, `Resolution` enum
4. Remove stale Phase 2 comments and unjustified `#[allow(dead_code)]`
5. Deduplicate `fnv1a_64` into a shared function

**Medium effort (1-3 hours each):**
6. Change `resolved_file` to return `Option<&Utf8Path>`
7. Add unit tests for `lattice.rs`, `graph.rs`, `obligations.rs`, `split_frontmatter`
8. Promote `debug_assert_eq!` at `parse.rs:973` to `assert_eq!`
9. Introduce `CheckInput` struct to replace 9-argument `run_checks`

**Larger refactors (half-day each):**
10. Split `cli.rs` into a module directory
11. Promote diagnostic codes from `&'static str` to enum
12. Migrate parse.rs temp dir tests to `tempfile`
