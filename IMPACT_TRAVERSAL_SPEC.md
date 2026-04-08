---
status: draft
date: 2026-04-08
---

# Impact Traversal Configuration

## Problem

`anneal impact` currently hardcodes its traversal set: DependsOn, Supersedes,
and Verifies (in reverse). After edge semantic refinement (anneal 0.6.0), many
structurally meaningful edges are typed as custom kinds (Synthesizes,
Implements, Reconciles) rather than DependsOn — because they shouldn't fire
W001 (it's valid for a synthesis to reference terminal material). But they
ARE structurally meaningful: if the target changes, the source needs review.

This means `anneal impact` now underrepresents actual blast radius. A document
with 15 incoming Cites and Synthesizes edges shows 1 dependent.

## Solution

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
