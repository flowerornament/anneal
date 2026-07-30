---
title: Predicate Teaching Catalog
date: 2026-07-29
status: locked
purpose: Make predicate-family membership a single authority while preserving every teaching card byte.
---

# Predicate Teaching Catalog — 2026-07-29

## Problem

Runtime predicate teaching is projected through six facets:

- requirements
- relationship
- extra lines
- common joins
- adjacent vocabulary
- executable example

Each facet currently matches predicate names independently. The matches do
more than attach prose: they repeatedly declare which predicates belong to
families such as convergence energy, corpus areas, flow, and dependency
validity. A predicate can therefore belong to a family in one facet and fall
out of it in another without a compiler error or an invariant failure.

The executable-documentation gate catches examples that no longer parse. It
does not catch a missing requirement, join, relationship, or adjacent topic;
those omissions silently make a card less useful.

## Decision

`PredicateFamily` is the sole classifier from predicate name to semantic
teaching family. `RuntimeTeaching` carries all six shared facets consumed by
the introspection builder.

```text
predicate name
      |
      v
PredicateFamily::for_name  <- sole family-membership authority
      |
      v
family defaults + exact predicate details
      |
      v
RuntimeTeaching
      |
      v
describe/examples projections
```

The classifier covers only genuine semantic families. A predicate may remain
unclassified when its teaching is wholly predicate-specific. Family defaults
remove repeated facts; exact predicate details remain explicit when members of
one family honestly need different joins, examples, or explanations.

The introspection builder asks for one `RuntimeTeaching` value. Stored
relations, primitives, verbs, and derived predicates use the same complete
record even when a kind consumes only some fields. A future
facet extends that record and its assembly. It must not add another
name-indexed function beside it.

## Families

The initial families are the repeated concepts already present in the six
facets:

- convergence energy
- corpus area
- blocking
- flow
- dependency validity
- frontmatter mapping aliases
- pipeline stall
- abandoned namespace
- concern pair
- retired obligation
- git recency

This catalog does not claim that every member shares every teaching field.
Family membership states conceptual ownership, not interchangeable output.
Exact predicate details are the declared exception mechanism.

## Authority

Family membership is essential catalog data. Each rendered facet is a
projection derived from that membership plus predicate-specific details.
Keeping the projections as independent name tables turns derived data into
accidental state and permits disagreement.

This follows two research-graph claims:

- "derived data is accidental state because it can always be re-derived from
  essential input on demand"
- "strong redundancy is derivability of one relation's projection from other
  projections in the named set"

## Compatibility

This is a data-model migration, not a teaching-content revision.

- every `describe(name, doc)` row remains byte-identical
- every `examples(name, example)` row remains byte-identical
- public Rust API remains unchanged
- the executable-documentation gate remains zero-tolerance

Full relation snapshots are the primary equivalence gate. Spot checks are not
sufficient because the failure mode is silent omission from one predicate's
card.

## Structural Gate

The catalog is correct when:

1. semantic family membership has one name classifier;
2. the builder consumes one complete teaching record;
3. no exported per-facet predicate lookup functions remain;
4. a new facet must modify the record assembly;
5. current describe and example relations are byte-identical.
