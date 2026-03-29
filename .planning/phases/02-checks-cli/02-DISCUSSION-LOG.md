# Phase 2: Checks & CLI - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-28
**Phase:** 02-checks-cli
**Areas discussed:** Broken ref strategy, Rich frontmatter fields, Cleanup scope

---

## Broken Ref Strategy

### §N.N Section References

| Option | Description | Selected |
|--------|-------------|----------|
| Silently skip | Don't report §N.N refs at all — different numbering system, can't map to heading slugs | |
| Info-level note | Report once as I001 summary acknowledging them without flooding output | ✓ |
| Don't create edges | Stop creating pending edges for §N.N refs during scanning | |

**User's choice:** Info-level note
**Notes:** Single summary diagnostic, not per-reference errors.

### Bare Filename Resolution

| Option | Description | Selected |
|--------|-------------|----------|
| Wire resolve_file_path | Use existing function to search relative to referring file, then root. Unresolved = E001 | ✓ |
| Wire + ambiguity warning | Same + W002 for bare names matching multiple files | |
| Filter out bare names | Only report errors for full-path references | |

**User's choice:** Wire resolve_file_path
**Notes:** None — straightforward choice.

---

## Rich Frontmatter Fields

### Frontmatter Expansion Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Add affects: only | Wire `affects:` as inverse DependsOn. 32 files, biggest bang for CHECK-03 | |
| Add affects: + supersedes: | Wire both inverse fields | |
| CONFIG-03 extensible mapping | General mechanism: anneal.toml maps arbitrary fields to edge kinds | ✓ |
| Defer entirely | Keep current 6 fields, frontmatter expansion is Phase 3/v2 | |

**User's choice:** CONFIG-03 extensible mapping
**Notes:** Full generality — any project declares its own field semantics.

### Core Fields Handling

| Option | Description | Selected |
|--------|-------------|----------|
| Keep core 6 hardcoded | Core model fields always parsed, config for extensions only | |
| All fields via config | Everything configurable, core 6 become defaults | ✓ |

**User's choice:** All fields via config
**Notes:** Zero-config defaults match current behavior. Maximum flexibility.

### Init Auto-Detection

| Option | Description | Selected |
|--------|-------------|----------|
| Scan and propose | init detects reference-like frontmatter fields, proposes mappings | ✓ |
| Defaults only | init generates anneal.toml with just core 6 defaults | |

**User's choice:** Scan and propose
**Notes:** None.

---

## Cleanup Scope

| Option | Description | Selected |
|--------|-------------|----------|
| #5 Labels in code blocks | Stop scanning labels inside fenced code blocks | ✓ |
| #6 Version status inherit | Version handles inherit status from parent file | ✓ |
| #7 URL false positives | Negative lookbehind for :// in file path regex | ✓ |
| #8+9 Dead code cleanup | Remove unused version_refs and dead is_excluded | ✓ |

**User's choice:** All four
**Notes:** Clean sweep — all small, several directly affect check quality.

---

## Claude's Discretion

- Diagnostic formatting details (color, alignment, grouping)
- Error code numbering scheme
- Frontmatter mapping internal architecture
- `anneal find` search implementation
- `anneal get` output formatting

## Deferred Ideas

None — discussion stayed within phase scope.
