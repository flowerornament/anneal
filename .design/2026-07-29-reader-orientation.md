---
date: 2026-07-29
status: authoritative
purpose: >-
  Define where Rust comments earn their maintenance cost: module boundaries
  expose owned decisions, supported facades expose contracts and errors, and
  dense implementations carry landmarks without narrating executable code.
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-07-29-anneal-core-public-api-altitude.md
---

# Reader orientation — 2026-07-29

## 1. Problem

The Atlas comment audit found 1,027 of 1,377 pub-ish items without `///`.
That number is a prompt to inspect, not a documentation target. It mixes
supported API, crate-private implementation seams, and helpers whose signatures
already say everything a useful comment could say.

The concrete reader failures are at boundaries:

- the six implementation modules extracted from `anneal-code` by the wno8
  decomposition have no module maps;
- `anneal-code::rustdoc` hides all dependencies behind `use super::*`;
- the 1,260-production-line language parser has no grammar landmarks beyond a
  one-line module header;
- `anneal-core` now has two selected public facades, but their module docs do
  not yet carry the admission and error-phase map that makes the split usable.

Blanket item documentation would increase prose authority without increasing
understanding. The repair targets the places where a new reader must otherwise
reconstruct design intent from control flow.

## 2. Governing evidence

The research graph supplies three constraints:

1. **Information hiding is the correct criterion for decomposing systems into
   modules.** A module comment should name the decision the module owns and
   hides, not retell the order in which its functions run.
2. **Reading an unfamiliar codebase is language learning, not mere symbol
   lookup.** Dense implementations need a small grammar of landmarks so a
   reader can classify what follows before reading details.
3. **Complexity breeds complexity as a secondary effect of failing to
   understand existing code.** Orientation is preventive architecture: when
   ownership is legible, duplicating an existing mechanism is less locally
   attractive.

Comments cannot replace the programmer-held theory. They can expose the stable
part of that theory: ownership, invariants, phase boundaries, and the
authoritative specification to consult.

## 3. Comment contract

### Module boundary

A dense or externally meaningful module begins with `//!` that answers:

1. What decision or representation does this module own?
2. What adjacent concern is deliberately owned elsewhere?
3. Which invariant or specification is load-bearing for changes here?

The comment does not enumerate every function. A module map may name phases
when the file is long enough that those phases are otherwise difficult to
locate.

### Supported facade

Public facade documentation names:

- the consumer class admitted at that altitude;
- the canonical path and the rule against implementation-path imports;
- the major phase-local error families callers must preserve.

The admission rule is the one locked in the public API altitude spec. Comments
must not widen the facade merely by describing an accidental public item.

### Public item

`///` earns its place when it states an invariant, error condition, unit,
authority, or semantic distinction that the signature does not. It does not
earn its place by restating a field or function name.

### Dense private implementation

Private code uses sparse section landmarks before coherent phases. Individual
private helpers remain undocumented unless they carry a non-obvious invariant.
The parser therefore names statement grammar, body atoms, expressions, token
navigation, and lexing; it does not acquire a comment above every parse helper.

## 4. Authority and drift

Executable facts stay executable:

- accepted syntax is defined by the parser and tested against the master
  grammar in Part IV, not copied into prose as another grammar;
- public reachability is enforced by `check-arch.py` and facade fixtures, not
  inferred from comments;
- adapter vocabulary remains in `vocab.rs`; module comments name its ownership
  without duplicating literal tables.

Source citations point to the master spec or a focused design spec. Transitional
language such as "new", "legacy flow", or "backward compatibility" is excluded
unless transition itself is the enduring contract.

## 5. Gate

This slice is complete when:

- all `anneal-code` modules carry responsibility-oriented `//!` maps;
- production `anneal-code` contains no `use super::*`;
- the parser exposes grammar landmarks and documents its public entry/error
  boundary;
- `anneal-core` root and runtime docs state the two-facade admission and
  phase-local error model;
- the simplified public API projection is byte-identical;
- same-document-state CLI artifacts are byte-identical;
- `just check` passes.

The Atlas undocumented-item total is recorded, not optimized. A lower count is
useful only when it corresponds to a reader question that the new comment
actually answers.
