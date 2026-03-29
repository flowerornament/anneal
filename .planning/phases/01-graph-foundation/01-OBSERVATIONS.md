# Phase 1 Post-Execution Observations

**Date:** 2026-03-29
**Corpus:** Murail `.design/` (260 files, 120 with frontmatter)
**Output:** 9788 handles, 6408 edges, 22 namespaces, 39 version handles, 25 statuses

These observations come from manual testing against the Murail corpus after Phase 1 execution. They document what Phase 2 planning needs to account for — none are Phase 1 bugs.

---

## 1. Pending Edge Resolution is Nearly Broken (Critical for CHECK-01)

**Numbers:** 275 resolved / 3396 unresolved = 7.5% resolution rate.

**Root cause — three identity format mismatches:**

| Pending edge source | Target identity format | Node index key format | Can resolve? |
|---|---|---|---|
| Body §N.N refs | `section:4.1` | `path#heading-slug` | Never |
| Body bare file refs | `summary.md` | `archive/research/summary.md` | Never (no path) |
| Body path file refs | `archive/research/summary.md` | `archive/research/summary.md` | Yes (275 did) |
| Frontmatter superseded-by | `murail-formal-model-v15.md` | `formal-model/history/murail-formal-model-v15.md` | Never (bare name) |
| Frontmatter depends-on | (unused in Murail) | — | Untested |

**Impact on Phase 2:** If CHECK-01 naively reports all 3396 unresolved pending edges as "broken references," the output is pure noise. Phase 2 must either:
- **Fix resolution** — wire `resolve_file_path()` (already exists, unused) for bare filenames, and decide what to do with §N.N refs
- **Filter by resolvability** — only report edges that *could* resolve but didn't (i.e., target looks like a file path with `/` but doesn't exist)
- **Categorize** — separate "unresolvable by design" from "actually broken"

**Breakdown of unresolved:**
- ~2517 section refs (`section:N.N`) — spec-internal numbering, fundamentally different from markdown heading slugs
- ~800+ bare file refs (`summary.md` without directory) — need relative path resolution
- ~6 URL fragments (`com/.../guide.md`) — regex false positives
- ~70 frontmatter field refs — bare filenames pointing to versioned files in subdirectories

## 2. Terminal Status Classification is Empty

**Numbers:** 25 observed statuses, all classified active, 0 terminal.

**Root cause:** `infer_lattice` accepts a `terminal_by_directory` parameter, but `main.rs` passes an empty `HashSet`. No directory convention analysis is wired.

**Murail reality:** At least "superseded" (5 files), "historical" (10), "archived" (1), "retired" (1), "incorporated" (11), "complete" (6) are terminal or near-terminal. The `archive/`, `history/`, and `prior/` directories provide strong signal.

**Impact on Phase 2:** CHECK-03 (confidence gap) compares source state vs target state in the lattice ordering. With no terminal/active distinction, all handles look equivalent — the check can't detect regressions. Phase 2 needs to:
- Walk directories during `build_graph` and tag which statuses appear in `archive/`, `history/`, `prior/`
- Pass that set to `infer_lattice` as `terminal_by_directory`
- Or add heuristic rules (any status named "superseded", "archived", "retired" is terminal)

## 3. Murail Uses Rich Frontmatter That anneal Ignores

**Parsed by anneal (6 fields):** status, updated, superseded-by, depends-on, discharges, verifies

**Used in Murail but not parsed (15+ fields with reference data):**

| Field | Count | Contains |
|---|---|---|
| affects | 32 | Label lists (OQ-14, FM-006) |
| source/sources | 49 | File/URL references |
| supersedes | 7 | File references (active form of superseded-by) |
| parent | 11 | Hierarchy references |
| resolves | 4 | Label/issue references |
| references | 5 | Cross-references |
| extends | 2 | Inheritance references |
| blocks | 2 | Blocking relationships |
| related, informs, downstream, upstream, tracks, supplements | 1-2 each | Various |

**Notably:** `supersedes:` (7 files) is the active counterpart of `superseded-by:` (4 files). anneal parses `superseded-by` but not `supersedes`. The version chain (v11→v17) uses `supersedes:` in frontmatter, but anneal builds it from filename patterns instead — so the chain works, just from a different signal.

**Impact on Phase 2:** These fields are project-specific (Murail conventions), not part of anneal's core model. CONFIG-03 could add extensible frontmatter field mapping. The `affects:` field is particularly valuable — it's the inverse of "depends-on" and appears in 32 files.

## 4. Handle Count is Dominated by Section Handles

**Breakdown:** 260 files + ~9030 section handles + 499 label nodes + 39 version nodes ≈ 9788

**~35 section handles per file.** Section handles are created for every markdown heading. The vast majority of handles in the graph are sections. This is correct behavior but has implications:
- Graph statistics are misleading — "9788 handles" sounds large but is 92% headings
- Phase 2 output needs to distinguish handle kinds in counts/reports
- Section handles have no status, no metadata, no edges (except parent→file). They're structurally present but semantically thin.

## 5. Labels in Code Blocks Are Counted

The plan explicitly specified scanning labels inside code blocks (only headings are skipped in code blocks). If a code example contains `OQ-64`, it creates a file→label edge. This slightly inflates edge counts and could cause Phase 2 checks to see spurious relationships. Minor — the spec should clarify whether this is desired.

## 6. Version Handles Don't Inherit File Status

Version handles are created with `status: None` even when the underlying file has a status. For example, `murail-formal-model-v14.md` has `status: superseded` but its Version handle has no status. This means convergence tracking can't distinguish active from superseded versions through the Version handle — you'd need to traverse to the file handle.

## 7. File Path Regex Catches URLs

The pattern `[a-z0-9_/-]+\.md` matches URL fragments like `com/rust-lang/portable-simd/blob/master/beginners-guide.md`. Only 6 occurrences in Murail — negligible. Could be fixed by requiring the match NOT be preceded by `://` or `http`.

## 8. `version_refs` Are Collected But Never Used

`scan_file` collects body-text version references (`v17`, `v3`) into `ScanResult.version_refs`, but `build_graph` never reads them. Version handles come exclusively from filename pattern matching in `resolve_versions`. The body-text scanning is wasted work. Either remove it or wire it into edge creation (a file mentioning "v17" could create a Cites edge to the version handle).

## 9. `is_excluded` Helper Is Unused

The `is_excluded` function in `parse.rs` duplicates logic that was inlined into the `filter_entry` closure. Dead code — can be removed in Phase 2 cleanup.

---

## Recommendations for Phase 2 Planning

1. **Pending edge resolution (#1) is the highest-priority fix.** CHECK-01 is useless without it. Wire `resolve_file_path` for bare filenames, decide on §-ref handling.
2. **Terminal status classification (#2) blocks CHECK-03.** Relatively easy — add directory scanning in `build_graph`, pass to `infer_lattice`.
3. **Handle kind breakdown (#4) should appear in CLI output.** "500 labels, 39 versions, 260 files, 9000 sections" is more informative than "9788 handles".
4. **Items 5-9 are minor** — cleanup opportunities, not blockers.
