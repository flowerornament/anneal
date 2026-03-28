# Phase 1: Graph Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-28
**Phase:** 01-graph-foundation
**Areas discussed:** Edge kind inference

---

## Area Selection

| Option | Description | Selected |
|--------|-------------|----------|
| Edge kind inference | Should Phase 1 infer edge kinds from context keywords or default everything to Cites? | Yes |
| Label definition sites | How to pick the 'definition site' for labels without config? | |
| Test strategy | Unit fixtures vs integration against real corpus vs both? | |
| Minimal CLI skeleton | What stats to print, human-only or also --json? | |

**User selected:** Edge kind inference only.

---

## Edge Kind Inference

### Q1: How should Phase 1 handle edge kind inference?

| Option | Description | Selected |
|--------|-------------|----------|
| Keyword inference from the start (Recommended) | Implement keyword-based inference in Phase 1. Graph is richer immediately and Phase 2 checks have real DependsOn edges. | Yes |
| All-Cites default, defer inference | Every edge is Cites in Phase 1. Simpler but Phase 2 scope grows. | |
| Frontmatter only, no keyword scan | Infer from frontmatter fields but not body text keywords. Middle ground. | |

**User's choice:** Keyword inference from the start
**Notes:** User chose the recommended option — full inference including both frontmatter fields and body-text context keywords.

### Q2: Keyword proximity rule for body text

| Option | Description | Selected |
|--------|-------------|----------|
| Same line only (Recommended) | Keyword must appear on the same line as the reference. Simple, predictable. | |
| Same paragraph | Keyword anywhere in same paragraph. Catches more but risks false positives. | |
| You decide | Let Claude choose based on what works during implementation. | Yes |

**User's choice:** You decide
**Notes:** Claude has discretion on the proximity rule — should choose based on what works best during implementation and testing.

---

## Claude's Discretion

- Keyword proximity rule for body-text edge kind inference

## Deferred Ideas

None — discussion stayed within phase scope.
