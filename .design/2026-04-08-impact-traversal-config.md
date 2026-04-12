---
status: draft
date: 2026-04-08
depends-on: anneal-spec.md
---

# Impact Traversal Configuration

## Motivation

`anneal impact` is the "measure twice" command — you run it before editing a
key file to understand what will need review. It's central to the editorial
workflow: edit a theory document, and impact tells you which architecture and
implementation docs inherit from it. This makes it safe to change foundational
material without accidentally leaving downstream docs stale.

The problem is that impact's usefulness depends on it seeing the real
dependency graph, and right now it sees a fraction of it.

### What happened

Anneal 0.6.0 introduced custom edge kinds and scoped W001 to DependsOn only.
This was the right fix — a synthesis document citing terminal research
shouldn't fire stale-reference warnings. But it created a side effect:
edges that were previously DependsOn (and visible to impact) got reclassified
to Cites, Synthesizes, Implements, etc. (and became invisible to impact).

In Herald's corpus, `system-theory.md` has 15 incoming edges from other
documents. `anneal impact system-theory.md` reports 1 dependent. The other
14 connections are Cites and Synthesizes edges that impact doesn't traverse.
An agent running impact before editing system-theory would conclude the blast
radius is minimal — when it's actually the most connected document in the
corpus.

### The underlying design tension

Before 0.6.0, two concerns were conflated in the DependsOn edge kind:

1. **W001 scope** — "warn me if I reference something stale"
2. **Impact scope** — "tell me what needs review if I change this"

0.6.0 correctly separated #1 (W001 fires on DependsOn only). But it didn't
separate #2 — impact still hardcodes the same traversal set. The result is
that corpora which use custom edge kinds for structural relationships (as
the feature was designed to enable) get degraded impact analysis.

## Solution

Complete the separation that 0.6.0 started: make impact's traversal set
independently configurable from W001's scope.

Add a configurable `[impact]` section to `anneal.toml`:

```toml
[impact]
traverse = ["DependsOn", "Supersedes", "Verifies", "Synthesizes", "Implements", "Reconciles"]
```

`traverse` lists the edge kinds that `anneal impact` follows in reverse when
computing direct and indirect dependents. This separates two concerns that
were previously conflated:

- **W001 scope** — which edges fire stale-reference warnings (DependsOn only, already correct since 0.6.0)
- **Impact scope** — which edges represent "if this changes, that needs review" (broader set)

### Behavior

- When `[impact]` section is absent, fall back to current hardcoded behavior
  (DependsOn, Supersedes, Verifies) for backwards compatibility.
- When present, use exactly the listed edge kinds. The user owns the set.
- Edge kind names match the custom kind strings in `[frontmatter.fields.*]`
  config. Case-sensitive, matching the `edge_kind` values.
- Direction is always reverse (impact traces "what depends on this target").

### Scope

This is the only change. No new CLI flags, no changes to `anneal check`,
`anneal map`, or any other command. Just making `impact`'s traversal set
configurable via `anneal.toml`.

### Optional enhancement

If low-effort: also accept `--kind` on the CLI to override the config for
ad-hoc queries. `anneal impact --kind DependsOn,Cites system-theory.md`
would traverse only those two kinds regardless of config. But the config
approach is the primary ask — `--kind` is a convenience if it falls out
naturally.

## Why these edge kinds

| Kind | Why impact should traverse it |
|------|------------------------------|
| DependsOn | Structural dependency — correctness requires target to be current |
| Supersedes | Version chain — superseded doc's dependents transfer to successor |
| Verifies | Verification link — if verified thing changes, verification may be invalid |
| Synthesizes | Synthesis depends on its inputs — if input changes, synthesis needs review |
| Implements | Implementation depends on its spec — if spec changes, impl needs review |
| Reconciles | Reconciliation depends on reconciled docs — same reasoning |

Edge kinds NOT in the default set:

| Kind | Why excluded |
|------|-------------|
| Cites | Informational reference — cited doc changing doesn't invalidate the citation |
| Discharges | Obligation link — discharge status is tracked separately |
| Flags | Drift detection — flags are diagnostic, not structural |
