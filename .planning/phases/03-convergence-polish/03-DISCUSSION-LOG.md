# Phase 3: Convergence & Polish - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-29
**Phase:** 03-convergence-polish
**Areas discussed:** Snapshot triggers, map output, diff reference points, suggestion presentation

---

## No Interactive Discussion Required

All four identified gray areas were resolved by reading the spec end-to-end:

### 1. Snapshot Trigger & `.anneal/` Directory

Spec §10, §12.1, §12.4 are explicit: both `check` and `status` append snapshots. CONVERGE-05 requires graceful handling of missing/corrupted history. §10: "If `.anneal/history.jsonl` is deleted, nothing breaks." Auto-create `.anneal/` on first write.

**Resolution:** Spec-prescribed. No decision needed.

### 2. `anneal map` Output Format

ROADMAP says "at least text format; --format=dot for graphviz." Text format left to Claude's discretion. `--concern` and `--around` are well-defined graph operations from §12.5.

**Resolution:** Text format is Claude's discretion. Everything else spec-prescribed.

### 3. `anneal diff` Reference Points

Initially considered deferring git-aware `HEAD~N` mode as too complex. User directed: "be ambitious — there's no need to think in terms of effort." All three modes (default, `--days=N`, git ref) are in scope.

**Resolution:** Full implementation per spec §12.8.

### 4. Suggestion Presentation

Spec §12.1 shows `check --suggest` as a filter flag. §12.4 shows `status` counting suggestions. KB-E8 lists five patterns. Each is a graph query per KB-P5.

**Resolution:** Fourth severity level in existing diagnostic system. Spec-prescribed.

## Claude's Discretion

- Text rendering format for `anneal map`
- Advancing/holding/drifting threshold heuristics
- Whether suggestions go in `checks.rs` or `suggest.rs`
- Suggestion error code numbering

## Deferred Ideas

None.
