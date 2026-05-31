---
status: current
---

# Code-As-Corpus Spike

Question: can rustdoc JSON become an anneal corpus where Rust API stability acts
as a convergence lattice?

Answer: yes, for the shape this spike needed to prove. A checked-in rustdoc JSON
fixture can emit ordinary `FactBatch` rows, the existing runtime can classify
stable / unstable / deprecated API items with a lifecycle configuration, and a
small code-specific rule layer can answer "which API surface should an agent be
careful relying on?"

This is not the `anneal-code` adapter. It is a proof that the substrate can carry
the model.

## Demo

Run:

```bash
cargo run --manifest-path tools/spike-runner/Cargo.toml --bin code_spike
```

The spike reads `tools/spike-runner/fixtures/rustdoc-toy/rustdoc_toy.json`,
builds a `FactBatch`, merges it into `FactStore`, installs lifecycle config, and
evaluates runtime queries over the resulting database.

Observed output summary:

```text
handles=8 edges=6 spans=7 content=7 meta=43
status_histogram: deprecated=1 stable=5 unstable=2

active:
  rustdoc_toy::Buffer::push_unstable        unstable
  rustdoc_toy::experimental_pipeline        unstable
  rustdoc_toy::old_helper                   deprecated

settled:
  rustdoc_toy
  rustdoc_toy::Buffer
  rustdoc_toy::Buffer::new
  rustdoc_toy::Buffer::push_unstable#doctest
  rustdoc_toy::stable_helper

frontier:
  rustdoc_toy::old_helper             energy=10 why=code_deprecated
  rustdoc_toy::experimental_pipeline  energy=7  why=code_unstable
  rustdoc_toy::Buffer::push_unstable  energy=4  why=code_unstable
```

The result is sensible: stable symbols settle, unstable symbols stay active, and
the deprecated symbol is the highest-risk API because it is deprecated, depended
on by another API item, and still has ordinary stale-dependency pressure.

## Mapping

Rustdoc item handles:

- `module`, `struct`, `enum`, `trait`, `function`, and `method` items become
  `*handle` rows.
- Handle ids are Rust paths such as `rustdoc_toy::Buffer::push_unstable`.
- The spike emits `kind = "file"` for all API item handles.
- The real item kind lives in `*meta{key: "code.item_kind"}`.

This `file` mapping is a deliberate staging hack and the first real adapter
design question. It buys immediate participation in existing area/file counts,
file-oriented status output, search/read behavior, and file-kind energy rules.
It distorts the substrate because an API item is not a file; a real adapter must
decide whether code items deserve a new handle kind, a refined existing kind, or
a broader model where source spans carry most code identity.

Stability lifecycle:

- `#[unstable]` -> `status = "unstable"`
- `#[deprecated]` -> `status = "deprecated"`
- `#[stable(since = ...)]` -> `status = "stable"`
- Lifecycle ordering: `unstable -> deprecated -> stable`
- Active: `unstable`, `deprecated`
- Terminal: `stable`

The deprecated choice is intentionally active-but-risky for the spike. It makes
the "least-settled API to avoid relying on" query useful. A production adapter
may want deprecated to be its own flow leaf, or a terminal state with warning
pressure, because semantically it is often a settled API moving backward.

Edges:

- Return-type and doc-link references become `DependsOn`.
- `#[deprecated(note = "use rustdoc_toy::stable_helper")]` becomes
  `rustdoc_toy::stable_helper Supersedes rustdoc_toy::old_helper`.
- A doctest-bearing item emits a small doctest handle and a `Verifies` edge.

The doctest handle is another staging compromise. It proves the relation can be
represented, but a real adapter should decide whether doctests are handles,
spans, metadata, or evidence rows.

## Code-Specific Energy

The existing convergence prelude does not know that "unstable public API with
reverse dependencies" is a code risk. The spike therefore adds an explicit
project-layer rule instead of pretending the signal already exists:

```datalog
api_frontier(h, energy, why)
```

It also extends `entropy` with code-specific sources so ordinary
`frontier(h, energy)` lights up in the demo. This is the right production
pressure point: an `anneal-code` adapter probably needs a small code prelude
module or adapter-provided convergence rules.

## Findings

The shape holds. Rust stability attributes are a real convergence lattice for
agent-facing code navigation:

- Stable API gives a terminal / settled set.
- Unstable API gives an active frontier.
- Deprecated API gives a high-risk active surface.
- Rust paths make good handle ids.
- Rustdoc relationships can become ordinary graph edges.
- Existing runtime queries work once the source emits substrate facts.

The biggest awkward part is not rustdoc JSON. It is substrate vocabulary:
`kind = "file"` works as a staging hack, but it is not the truth. The eventual
adapter design should start with the handle-kind question before chasing scale.

## Punt List

This spike intentionally punts on:

- live rustdoc invocation;
- rustdoc JSON schema stability;
- full type/body dependency extraction;
- private items and visibility policy;
- stdlib scale;
- a CLI command that runs `anneal status` directly over a code root;
- a production code prelude.

No blocker appeared for scaling the concept to a real crate or stdlib subset.
The next step should be a narrow `anneal-code` design that answers the handle
kind question and defines the code-specific convergence rules before adding a
live source adapter.
