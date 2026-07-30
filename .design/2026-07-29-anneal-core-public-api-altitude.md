---
status: locked
date: 2026-07-29
authors: [codex, claude, morgan]
purpose: >
  Defines the supported anneal-core library altitude before its accidental
  implementation-module surface becomes a published contract. The crate root
  is the adapter and provider facade; anneal_core::runtime is the host and
  query facade. Every supported item has one canonical path, implementation
  modules stay private, and boundary fixtures may falsify the two-facade split.
depends-on:
  - 2026-05-13-corpus-runtime.md
---

# anneal-core public API altitude - 2026-07-29

## 1. The measured mismatch

At v0.22.0, `cargo public-api -p anneal-core --simplified` reports:

```
3,034 public API lines
306 public struct, enum, trait, and type entries
18 public implementation modules at the crate root
7 public implementation submodules under runtime
about 34 distinct paths used by sibling crates
```

The ratio is directional evidence, not an API-utilization percentage. Size
prompts the inspection; it does not decide the boundary. The architectural
defect is duplicate authority over reachability: most supported items are
available through both an implementation-module path and a facade re-export,
while many unconsumed implementation details are public at all.

The master spec already names the intended consumers:

- adapters and providers implement the CR-D4 and CR-D5 extension contracts;
- CLI, MCP, and embedding hosts project and execute the substrate.

No anneal crate is currently published to crates.io and the release pipeline
publishes GitHub binaries only. The workspace is therefore the complete current
consumer set. Compile-failure-driven pruning is complete evidence, not a sample
of unknown downstream use.

The immediate risk is the opposite of compatibility breakage:
`anneal-core` does not declare `publish = false`. One
`cargo publish -p anneal-core` could turn the accidental module forest into a
real external contract before the intended boundary exists.

## 2. Governing decision

`anneal-core` exposes exactly two facades:

| Facade | Supported consumer | Responsibility |
|---|---|---|
| crate root | adapters and providers | identities, facts, source extraction, refresh, storage, retrieval, ranking, policy, trails, verbs, and adapter-required provenance |
| `anneal_core::runtime` | surfaces and embedding hosts | grammar, analysis, loading, evaluation, values, rows, explanations, prelude query helpers, and NDJSON |

These are a sum of supported contracts, not a public product of implementation
modules.

Three rules are load-bearing:

1. **Canonical ownership is the gate, not a target reduction percentage.**
   Every supported item has one public path.
2. **There is no public `internal` or `workspace` namespace.** Rust cannot
   enforce workspace-only public visibility, so such a namespace would merely
   rename accidental API.
3. **Duplicate paths receive no compatibility aliases.** Preserving them would
   preserve the defect.

The root remains flat because that is the contract already taught by the master
spec's `Source`, `SourceContext`, and `FactBatch` examples. A third `adapter`
namespace would add ceremony without adding a supported consumer class.

## 3. Facade catalog

### Root facade

The root facade selects contracts from these current implementation modules:

| Current owner | Root-facade contract |
|---|---|
| `ids`, `facts` | corpus/source/generation identities and stored fact batches |
| `source`, `driver`, `store` | adapter extraction, cancellation, refresh transaction, and generation merge |
| `config_schema` | typed runtime configuration declarations consumed by adapters and hosts |
| `retrieval`, `ranking` | CR-D5 provider and ranker contracts plus their default implementations |
| `policy`, `trail`, `verbs` | authorization, trail, and saved-verb extension contracts |
| `project`, `history` | project loading and snapshot history used by surfaces and hosts |
| `path_policy`, `impact` | shared validated path and impact-policy values |
| `target_probe` | code-target drift and provenance required by the markdown adapter class |

`target_probe` is retained because provenance-aware markdown adapters require
it, not because a workspace sibling happens to import it. If that requirement
ceases to describe the adapter class, the contract should narrow again.

`hash`, `lifecycle`, `metadata`, and `visibility` remain private helpers unless
a supported facade signature or declared extension contract proves otherwise.
Workspace convenience alone does not prove a contract.

### Runtime facade

`anneal_core::runtime` selects the grammar, AST, static analysis, parser,
loader, evaluator, value/row/explanation, prelude helper, and NDJSON contracts
needed to embed a query runtime.

The current `analysis`, `ast`, `eval`, `loader`, `ndjson`, `parser`, and
`prelude` submodules become private. Required items are re-exported directly
from `anneal_core::runtime`; the submodule paths are not aliases.

### Private implementation

Every other top-level module and every runtime submodule is private. Public
items not required transitively by a supported signature or directly by a
supported consumer fixture leave the facade.

The implementation keeps an exact ledger:

```
old public path -> canonical public path
old public path -> removed implementation detail
```

This ledger is a courtesy migration map for the workspace, not a deprecation
cycle for external dependents that do not exist.

## 4. Publication policy

`anneal-core` declares `publish = false` with the facade implementation.
Publication is a future explicit decision and this contract is its
prerequisite. Removing that guard requires:

1. an intentional distribution plan for the library crates;
2. public documentation and compatibility policy for both facades;
3. an external consumer that benefits from registry publication;
4. a reviewed `cargo public-api` baseline.

Git and path dependencies can still exercise the documented embedding boundary
without freezing an accidental crates.io contract.

## 5. Boundary tests

Two fixtures test both sufficiency and placement:

1. A third-party-style source adapter imports only the root facade, describes
   itself, builds a `FactBatch`, and participates in a refresh transaction.
2. A host fixture imports only `anneal_core::runtime`, then parses, analyzes,
   evaluates, and renders a query result.

If the adapter fixture needs `runtime`, or the host fixture needs an
implementation-module path, that is evidence that the boundary is misplaced.
The fixture must not be widened merely to make it compile.

Cross-cutting actor and capability values may live at the root when both
consumer classes genuinely share them. The runtime facade may re-export such a
root contract only when doing so does not create a second canonical path.

## 6. Implementation gate

The implementation is complete when:

- `cargo public-api` records the exact before/after path ledger;
- no implementation module path remains public;
- no supported item has two canonical public paths;
- all workspace imports use one of the two facades;
- both boundary fixtures compile and run;
- `publish = false` prevents accidental registry publication;
- `just check` passes;
- help, status, check, search, read, context, handle, impact, lineage, eval,
  describe, JSON, NDJSON, stderr, and exit artifacts are byte-identical.

No implementation LOC refactor belongs to this change. Serialized-boundary
newtypes follow this altitude decision rather than expanding the accidental
surface first.

## 7. Future surface additions

Progressive disclosure will likely add representation-manifest types. That is
expected. After this decision, each addition is made deliberately against one
known facade:

- an adapter-produced representation contract belongs at the root;
- a runtime-produced observation result belongs under `runtime`;
- a renderer-only type belongs in its surface crate and does not enter
  `anneal-core`.

Growth at a declared altitude is not backsliding. Unexamined reachability is.
