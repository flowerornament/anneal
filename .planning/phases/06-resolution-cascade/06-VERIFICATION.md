---
phase: 06-resolution-cascade
verified: 2026-03-30T08:00:00Z
status: passed
score: 5/5 success criteria verified
re_verification: true
  previous_status: gaps_found
  previous_score: 4/5
  gaps_closed:
    - "Every diagnostic in --json output carries a SourceSpan (file + line), never null"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Verify root-prefix strip creates graph edges on a corpus with .design/ prefixed paths"
    expected: "anneal check shows fewer E001 errors after cascade resolves .design/foo.md -> foo.md"
    why_human: "Requires a corpus that actually contains .design/-prefixed cross-references in practice. Cascade test suite covers with synthetic data."
  - test: "Verify anneal.toml [check] default_filter = active-only filters same diagnostics as --active-only flag"
    expected: "Running anneal check with the flag and with the config opt-in produce identical output"
    why_human: "Requires a corpus with both active and terminal-status files where the difference is observable."
---

# Phase 6: Resolution Cascade Verification Report

**Phase Goal:** Unresolved references get deterministic "did you mean?" candidates, and all diagnostics carry structured evidence with mandatory source locations
**Verified:** 2026-03-30T08:00:00Z
**Status:** passed
**Re-verification:** Yes — after gap closure (DIAG-01 source locations)

## Goal Achievement

### Observable Truths (from ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `anneal check` on corpus with path mismatches shows "similar handle exists: subdir/foo.md" instead of bare E001 | VERIFIED | Murail corpus: 47 E001 diagnostics show "similar handle exists: TQ-1", "similar handle exists: OQ-1", etc. Human output confirmed. |
| 2 | Resolution cascade resolves root-prefix paths, version stems, and zero-padded labels | VERIFIED | 10 cascade unit tests pass covering all three strategies. Murail corpus shows 47 zero-pad matches (TQ-001 -> TQ-1). cascade_unresolved in resolve.rs implements all three strategies. |
| 3 | Every diagnostic in --json output carries a SourceSpan (file + line), never null | VERIFIED | Murail corpus: 334 total diagnostics, 0 with null file, 0 with null line. All codes (E001, E002, W001-W004, I001, I002, S001-S005) produce non-null file and line. |
| 4 | JSON output changes are additive-only — existing fields preserve type and presence, new fields are nullable | VERIFIED | Fields on first diagnostic: ['severity', 'code', 'message', 'file', 'line', 'evidence']. The 'evidence' field is new and nullable (null when no structured evidence). All prior fields present with unchanged types. |
| 5 | --active-only is configurable via [check] default_filter = "active-only" in anneal.toml | VERIFIED | src/config.rs CheckConfig has default_filter: Option<String>. src/main.rs line 504: `active_only \|\| config.check.default_filter.as_deref() == Some("active-only")`. Four unit tests in config.rs confirm serialization and deserialization. |

**Score:** 5/5 success criteria verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/parse.rs` | ImplausibleRef with line field | VERIFIED | Line 756: `pub(crate) line: u32`. All ImplausibleRef constructions include `line: 1` (frontmatter-sourced). |
| `src/checks.rs` | Evidence enum and evidence field on Diagnostic; all Diagnostic constructions have non-null file and line | VERIFIED | Lines 18-38: Evidence enum with BrokenRef, StaleRef, ConfidenceGap, Implausible variants. Line 59: `pub(crate) line: Option<u32>`. artifact_file helper at line 95. Zero `line: None` or `file: None` in production code (only in test fixtures). |
| `src/resolve.rs` | cascade_unresolved function with root-prefix, version-stem, zero-pad strategies | VERIFIED | Lines 606-663: pub(crate) fn cascade_unresolved. Lines 489-598: try_root_prefix_strip, try_version_stem, try_zero_pad_normalize. CascadeResult struct at lines 20-28. |
| `src/config.rs` | CheckConfig struct with default_filter field | VERIFIED | Lines 147-151: pub(crate) struct CheckConfig with pub(crate) default_filter: Option<String>. AnnealConfig line 104: `pub(crate) check: CheckConfig`. |
| `src/main.rs` | collect_unresolved_owned returns section_ref_file; run_checks call sites updated | VERIFIED | Line 382: collect_unresolved_owned returns (Vec, usize, Option<String>). Three call sites (lines 507, 653, 696) destructure triple and pass section_ref_file.as_deref() to run_checks. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| src/parse.rs | src/checks.rs | PendingEdge.line flows to Diagnostic.line in check_existence | WIRED | checks.rs line 167: `line: edge.line`. PendingEdge.line populated at all construction sites in parse.rs. |
| src/config.rs | src/main.rs | config.check.default_filter read in check command path | WIRED | main.rs line 504-505: `active_only \|\| config.check.default_filter.as_deref() == Some("active-only")` |
| src/main.rs | src/resolve.rs | main calls cascade_unresolved after resolve_all | WIRED | main.rs lines 454-461: resolve::cascade_unresolved call with pre_cascade_index and root_str. |
| src/main.rs | src/checks.rs | cascade_candidates HashMap and section_ref_file passed to run_checks | WIRED | All three run_checks call sites in main.rs pass &cascade_candidates and section_ref_file.as_deref(). cli.rs line 1723 also updated. |
| src/checks.rs | serialization | Diagnostic with evidence field serialized in JSON output; file and line always non-null on corpus | WIRED | Diagnostic derives Serialize. evidence field is Option<Evidence>. Murail corpus: 334 diagnostics, 0 null file, 0 null line. |
| src/main.rs collect_unresolved_owned | src/checks.rs check_existence | section_ref_file threaded as representative file for I001 | WIRED | main.rs line 397-402: graph.node(edge.source).file_path captured as section_ref_file. checks.rs line 132: `file: section_ref_file.map(ToString::to_string)`. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| src/checks.rs check_existence | cascade_candidates | main.rs cascade_results from cascade_unresolved | Yes — computed from graph node_index lookups on actual pending edges | FLOWING |
| src/resolve.rs cascade_unresolved | node_index | build_node_index(&result.graph) | Yes — graph populated by full parse pipeline | FLOWING |
| src/checks.rs check_staleness | handle.file_path | graph node from parse pipeline, with artifact_file fallback for Version handles | Yes — artifact_file resolves Version -> parent File node file_path; verified 0 null file on Murail | FLOWING |
| src/checks.rs check_existence I001 | section_ref_file | collect_unresolved_owned graph.node(edge.source).file_path | Yes — first section-ref source's file_path from parse pipeline | FLOWING |
| src/checks.rs suggest_orphaned S001 | artifact_file fallback | Version handle -> artifact NodeId -> parent file node | Yes — handles Version handles that have file_path: None; verified 0 null file on Murail | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| cascade unit tests pass | `cargo test cascade` | 10 passed, 0 failed | PASS |
| evidence unit tests pass | `cargo test evidence` | 3 passed, 0 failed | PASS |
| full test suite passes | `just check` | 139 passed, 0 failed | PASS |
| zero null file in --json output | Murail corpus --json + python3 filter | 0 diagnostics with null file out of 334 | PASS |
| zero null line in --json output | Murail corpus --json + python3 filter | 0 diagnostics with null line out of 334 | PASS |
| "did you mean?" appears in human output | `anneal check` on Murail | 47 diagnostics show "similar handle exists: ..." | PASS |
| evidence field present in JSON | `anneal check --json` | Fields: ['severity','code','message','file','line','evidence'] | PASS |
| active-only config opt-in wired | grep in main.rs | Line 504-505 merges CLI flag and config.check.default_filter | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| RESOLVE-02 | 06-02-PLAN.md | Resolution cascade: exact -> root-prefix -> version-stem -> zero-pad | SATISFIED | cascade_unresolved in resolve.rs with 10 unit tests covering all strategies |
| RESOLVE-03 | 06-02-PLAN.md | Root-prefix resolution (.design/foo.md -> foo.md) | SATISFIED | try_root_prefix_strip function; creates graph edge on unambiguous match |
| RESOLVE-04 | 06-02-PLAN.md | Version stem resolution (formal-model-v11 -> suggest v17) | SATISFIED | try_version_stem function; returns version candidates for display |
| RESOLVE-05 | 06-02-PLAN.md | Zero-pad label normalization (OQ-01 -> OQ-1) | SATISFIED | try_zero_pad_normalize with COMPOUND_LABEL_RE; 47 zero-pad matches on Murail corpus |
| RESOLVE-06 | 06-02-PLAN.md | Unresolved references carry candidate list for diagnostic enrichment | SATISFIED | CascadeResult.candidates flows to cascade_candidates HashMap in main.rs; consumed in check_existence |
| DIAG-01 | 06-01-PLAN.md + 06-04-PLAN.md | All diagnostics carry mandatory SourceSpan (file + line, never null) | SATISFIED | 334 Murail diagnostics: 0 null file, 0 null line. All codes (E001, E002, W001-W004, I001, I002, S001-S005) produce non-null fields. Frontmatter-sourced diagnostics use line: 1. Aggregate diagnostics use representative file from first relevant handle. |
| DIAG-02 | 06-03-PLAN.md | Introduce Evidence enum on Diagnostic for structured check results | SATISFIED | Evidence enum with BrokenRef, StaleRef, ConfidenceGap, Implausible variants; evidence field on all Diagnostic instances |
| DIAG-03 | 06-03-PLAN.md | E001 includes resolution candidates ("similar handle exists: ...") | SATISFIED | check_existence builds candidate_msg and Evidence::BrokenRef with candidates; 47 diagnostics on Murail |
| DIAG-04 | 06-03-PLAN.md | JSON output changes are additive-only | SATISFIED | New nullable evidence field only; all prior fields (severity, code, message, file, line) unchanged in type and presence |
| DIAG-05 | 06-01-PLAN.md | Human output stays terse (line number is only new addition) | SATISFIED | Diagnostic::print_human does not output evidence field; line appended as ":N" when present |
| UX-01 | 06-01-PLAN.md | --active-only available as config opt-in via [check] default_filter | SATISFIED | CheckConfig.default_filter parsed from anneal.toml; main.rs merges with CLI flag at line 504 |

**Orphaned requirements:** None. All 11 requirement IDs in the phase frontmatter are accounted for.

### Anti-Patterns Found

None in production code. The `line: None` and `file: None` patterns at checks.rs lines 1458-1569 are exclusively inside test fixtures constructing synthetic Diagnostic values for unit tests — not production diagnostic construction paths.

### Human Verification Required

### 1. Root-Prefix Edge Creation on Real Corpus

**Test:** Create a small test corpus with files that reference `.design/foo.md` where the root is `.design/`, then run `anneal check` and verify the reference resolves (E001 disappears).
**Expected:** E001 count decreases — root-prefix strip creates a graph edge for the unambiguous path match.
**Why human:** The Murail corpus does not appear to contain `.design/`-prefixed cross-references in practice. The cascade test suite covers this with synthetic data, but real-corpus behavior should be confirmed.

### 2. Config Opt-In vs CLI Flag Equivalence

**Test:** Create an anneal.toml with `[check]\ndefault_filter = "active-only"` and compare `anneal check` output (with config, no flag) against `anneal check --active-only` (with flag, no config).
**Expected:** Identical diagnostic lists — both paths should apply the same terminal-file filtering.
**Why human:** Requires a corpus with both active and terminal-status files where the difference is observable.

### Gap Closure Summary

The single gap from the initial verification (DIAG-01 — mandatory source locations on all diagnostics) is now closed:

- **What was missing:** W001-W004, I001, I002, S001-S005 all had `line: None` and some had `file: None`.
- **What was delivered in 06-04:**
  - `ImplausibleRef` struct gained a `line: u32` field; W004 uses `Some(r.line)`.
  - W001, W002, W003, S001 diagnostics now use `line: Some(1)` (frontmatter-level, no per-field YAML line numbers available).
  - `collect_unresolved_owned` in main.rs was extended to return `section_ref_file: Option<String>`, threaded through `run_checks` to `check_existence` for I001.
  - S002-S005 use representative file from first relevant handle.
  - E002 and I002 use the label handle's `file_path` with `line: Some(1)`.
  - An `artifact_file` helper was added to resolve Version handle file paths through their `artifact` field (parent File node), fixing 25 S001 and 1 W001 that had null file because Version handles have `file_path: None`.
- **Verification:** Murail corpus: 334 total diagnostics, 0 null file, 0 null line.

---

_Verified: 2026-03-30T08:00:00Z_
_Verifier: Claude (gsd-verifier)_
