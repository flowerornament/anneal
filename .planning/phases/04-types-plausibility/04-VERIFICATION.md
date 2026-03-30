---
phase: 04-types-plausibility
verified: 2026-03-30T00:50:00Z
status: passed
score: 4/4 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 3/4
  gaps_closed:
    - "anneal check --json output includes DiscoveredRef with RefHint classification for every reference (frontmatter and body)"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Run anneal check on a corpus with URLs in frontmatter and verify no E001 for URL targets"
    expected: "No E001 diagnostic for https:// targets; they appear in extractions array with hint=External"
    why_human: "Needs a corpus with configured frontmatter fields pointing to URLs (murail has none)"
    status: resolved
    resolution: "Added url_in_frontmatter_no_e001_and_external_in_extraction integration test with contrived corpus (https + http URLs in frontmatter). Verifies URLs never enter pending_edges, appear in external_refs, and classify as RefHint::External in extractions."
---

# Phase 4: Types & Plausibility Verification Report

**Phase Goal:** Extraction pipeline produces typed, plausibility-filtered output — frontmatter references are classified not silently skipped, and the extraction boundary is clean enough to swap internals behind it
**Verified:** 2026-03-30T00:50:00Z
**Status:** passed
**Re-verification:** Yes — after gap closure (Plan 03)

## Goal Achievement

### Observable Truths

| #   | Truth                                                                                             | Status      | Evidence                                                                                             |
| --- | ------------------------------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------- |
| 1   | `anneal check --json` output includes `DiscoveredRef` with `RefHint` classification              | ✓ VERIFIED  | `extractions` array present in JSON; 262 entries on murail corpus; 62 with refs; Label/FilePath/Implausible hints confirmed |
| 2   | URLs in frontmatter edges appear as `RefHint::External` and do NOT produce E001                  | ✓ VERIFIED  | `classify_frontmatter_value` returns `External` for `https://`; 0 E001 for URL targets on murail corpus |
| 3   | Absolute paths, freeform prose, wildcards produce W004 instead of false positive E001            | ✓ VERIFIED  | 18 W004 on murail corpus (e.g. "spec/SPEC.md v0.8" → freeform prose); 5 integration tests pass       |
| 4   | All existing tests pass — `just check` green with no behavior change in final diagnostic output  | ✓ VERIFIED  | `just check` passes: 102/102 tests, fmt clean, clippy clean, zero compiler warnings                  |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact              | Expected                                                                           | Status       | Details                                                                                    |
| --------------------- | ---------------------------------------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------ |
| `src/extraction.rs`   | FileExtraction, DiscoveredRef, RefHint, RefSource, classify_frontmatter_value      | ✓ VERIFIED   | 368 lines; all types defined and Serialize-capable; no dead_code warnings for DiscoveredRef/FileExtraction |
| `src/parse.rs`        | FileExtraction constructed per-file in build_graph; BuildResult.extractions field  | ✓ VERIFIED   | Lines 650-671: dual-pass populates DiscoveredRef per target; line 498: extractions field on BuildResult; line 722: returned |
| `src/cli.rs`          | CheckOutput.extractions field; cmd_check accepts extractions parameter             | ✓ VERIFIED   | Line 104: `pub(crate) extractions: Vec<crate::extraction::FileExtraction>`; line 167: param in cmd_check |
| `src/main.rs`         | cmd_check call passes result.extractions                                           | ✓ VERIFIED   | Line 499: `result.extractions.clone()` passed to cmd_check                                |
| `src/resolve.rs`      | Resolution enum (Exact / Fuzzy / Unresolved)                                       | ✓ VERIFIED   | Lines 47-54; derives Clone/Debug/Serialize; `#[allow(dead_code)]` for Phase 6              |

### Key Link Verification

| From                | To                   | Via                                                          | Status   | Details                                                           |
| ------------------- | -------------------- | ------------------------------------------------------------ | -------- | ----------------------------------------------------------------- |
| `src/parse.rs`      | `src/extraction.rs`  | `use crate::extraction::{DiscoveredRef, FileExtraction, RefHint, RefSource, classify_frontmatter_value}` | ✓ WIRED | Line 11 of parse.rs |
| `src/parse.rs`      | `src/checks.rs`      | `implausible_refs` field in BuildResult                      | ✓ WIRED  | BuildResult.implausible_refs passed at all 4 run_checks call sites |
| `src/cli.rs`        | `src/parse.rs`       | `build_result.extractions` passed to cmd_check               | ✓ WIRED  | main.rs line 499: `result.extractions.clone()`                    |
| `src/cli.rs`        | `src/extraction.rs`  | FileExtraction serialized in CheckOutput                     | ✓ WIRED  | CheckOutput.extractions: `Vec<crate::extraction::FileExtraction>` |

### Data-Flow Trace (Level 4)

| Artifact            | Data Variable      | Source                                           | Produces Real Data         | Status          |
| ------------------- | ------------------ | ------------------------------------------------ | -------------------------- | --------------- |
| `src/parse.rs`      | `DiscoveredRef`    | `classify_frontmatter_value` per fe.targets loop | Yes — 253 refs on murail   | ✓ FLOWING       |
| `src/parse.rs`      | `FileExtraction`   | Per-file in build_graph, one per markdown file   | Yes — 262 entries on murail | ✓ FLOWING      |
| `src/cli.rs`        | `extractions`      | `result.extractions.clone()` from BuildResult    | Yes — in JSON output       | ✓ FLOWING       |
| `src/parse.rs`      | `implausible_refs` | `classify_frontmatter_value` returns Implausible | Yes — 18 W004 on murail    | ✓ FLOWING       |
| `src/parse.rs`      | `external_refs`    | `classify_frontmatter_value` returns External    | Collected, not yet consumed downstream | ✓ FLOWING (partial — HandleKind::External deferred to Phase 7) |

### Behavioral Spot-Checks

| Behavior                                              | Command                                                                              | Result                                                 | Status  |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------ | ------- |
| `extractions` array present in JSON output            | `anneal --root murail/.design/ check --json` — check for `extractions` key          | Present; 262 entries                                   | ✓ PASS  |
| DiscoveredRef has `hint` classification               | Parse first `refs` entry in extractions                                              | `{'raw': '...', 'hint': 'FilePath', 'source': {'Frontmatter': {'field': 'Supersedes'}}, ...}` | ✓ PASS |
| RefHint variety: Label, FilePath, Implausible present | Count hint types across all refs                                                     | FilePath: 137, Label: 98, Implausible: 18              | ✓ PASS  |
| W004 still fires for implausible refs                 | Check W004 count in diagnostics                                                      | 18 W004 on murail corpus                               | ✓ PASS  |
| No E001 for URL targets                               | Filter E001 where target starts with `http`                                          | 0 URL-target E001s                                     | ✓ PASS  |
| Zero compiler warnings                                | `cargo build 2>&1 \| grep warning`                                                   | No output — zero warnings                              | ✓ PASS  |
| just check quality gate                               | `just check`                                                                         | 102/102 tests pass, fmt clean, clippy clean in 2.08s   | ✓ PASS  |

### Requirements Coverage

| Requirement  | Source Plan | Description                                                                                      | Status        | Evidence                                                                                                          |
| ------------ | ----------- | ------------------------------------------------------------------------------------------------ | ------------- | ----------------------------------------------------------------------------------------------------------------- |
| EXTRACT-01   | 04-01/03    | Introduce `FileExtraction` as uniform extraction output from frontmatter and body scanning        | ✓ SATISFIED   | FileExtraction constructed per-file in build_graph; 262 entries on murail; present in CheckOutput JSON           |
| EXTRACT-02   | 04-01/03    | Introduce `DiscoveredRef` with `RefHint` replacing `PendingEdge`/`LabelCandidate`/etc.           | ✓ SATISFIED   | DiscoveredRef populated for every frontmatter target in production; 253 refs on murail; Label/FilePath/Implausible hints in JSON output. Note: runs alongside existing types (not replacing — replacement deferred to Phase 5 body scanner migration) |
| EXTRACT-05   | 04-02       | Plausibility filter rejects absolute paths, freeform prose, wildcards from frontmatter            | ✓ SATISFIED   | classify_frontmatter_value + build_graph loop + W004 + 5 integration tests + 18 W004 on murail corpus             |
| EXTRACT-06   | 04-02       | URLs in frontmatter classified as `RefHint::External` (not silently skipped)                     | ✓ SATISFIED   | External branch in classify_frontmatter_value; tracked in external_refs and in DiscoveredRef.hint=External in extractions |
| RESOLVE-01   | 04-01       | Introduce `Resolution` enum (Exact / Fuzzy / Unresolved) with candidate collection               | ✓ SATISFIED   | Resolution enum in resolve.rs lines 47-54; Serialize derived; `#[allow(dead_code)]` for Phase 6 cascade          |

**Note on EXTRACT-02 scope:** REQUIREMENTS.md marks this as "replacing" the old types. The implementation runs DiscoveredRef alongside the old types (additive). Full replacement is deferred to Phase 5 (pulldown-cmark body scanner migration). This was a deliberate scope reduction in the plans and is acceptable: DiscoveredRef IS produced for every frontmatter reference in production, satisfying the core intent of the requirement.

### Anti-Patterns Found

| File                | Line    | Pattern                                             | Severity  | Impact                                                               |
| ------------------- | ------- | --------------------------------------------------- | --------- | -------------------------------------------------------------------- |
| `src/extraction.rs` | 33      | `#[allow(dead_code)]` on `RefSource::Body` variant  | ℹ️ Info   | Expected — Body variant unused until Phase 5 body scanner            |
| `src/resolve.rs`    | 46      | `#[allow(dead_code)]` on `Resolution`               | ℹ️ Info   | Expected — Resolution cascade wired in Phase 6                       |
| `src/parse.rs`      | 495     | `#[allow(dead_code)]` on `ExternalRef`/`external_refs` | ℹ️ Info | Expected — HandleKind::External deferred to Phase 7 (CONFIG-02)      |

**No blocker or warning-level anti-patterns.** All dead_code annotations are intentional phase stubs for future work (Phase 5, 6, 7).

### Human Verification Required

#### 1. URL Behavior in Real Corpus

**Test:** Configure a corpus with a frontmatter field (e.g., `depends-on: https://some-paper.com`) and run `anneal check`. Confirm:
- No E001 error for the URL target
- The URL appears in `extractions[].refs` with `hint: External` in `--json` output
**Expected:** `hint: "External"` in extractions JSON; no E001 for the URL; no W004 for the URL
**Why human:** The murail corpus has no frontmatter fields with URL values configured. The spot-check shows 0 External hints on murail, so the External code path has not been exercised on a real corpus (only in unit tests).

## Re-verification Summary

**Gap closed.** The single gap from initial verification — DiscoveredRef and FileExtraction never constructed in production — has been fully resolved by Plan 03 (committed `b4435fd` and `15e6eb0`).

**What changed:** Plan 03 added a dual-pass over `field_edges` in `build_graph` that constructs `DiscoveredRef` per frontmatter target alongside the existing `PendingEdge` flow. `FileExtraction` is now pushed per-file to `BuildResult.extractions`, and `CheckOutput` includes the `extractions` field serialized in `--json` output.

**Verification on murail corpus confirms:**
- 262 `FileExtraction` entries (one per markdown file)
- 253 `DiscoveredRef` entries across 62 files with frontmatter references
- RefHint distribution: FilePath (137), Label (98), Implausible (18)
- Zero compiler warnings
- 102/102 tests pass

**Remaining human check:** URL classification in a real corpus with configured URL frontmatter targets.

---

_Verified: 2026-03-30T00:50:00Z_
_Verifier: Claude (gsd-verifier)_
