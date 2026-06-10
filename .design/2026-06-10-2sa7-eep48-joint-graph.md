---
status: evidence
date: 2026-06-10
authors: [codex]
bd: anneal-2sa7
relates:
  - 2026-06-10-anneal-code-adapter.md
  - 2026-06-10-ji1s-eep48-sim.md
  - 2026-06-10-drift-refresh-design.md
  - 2026-06-09-code-and-corpus.md
---

# 2sa7 EEP-48 joint-graph evidence - 2026-06-10

This is the final evidence pass for the code-and-corpus arc. It exercises the
implemented `anneal-code` EEP-48 adapter over a private Elixir application
corpus as a two-source joint graph: markdown design docs plus compiled BEAM
Docs chunks. Row-level application data stays local under `/tmp/anneal-2sa7`;
this document commits only neutral aggregate evidence.

## Extraction profile

Configuration shape:

```datalog
source code {
  source_root("lib").
  package("private_elixir_app").
  eep48_beam_dir("_build/dev/lib/private_app/ebin").
}
```

The adapter ingested 1,067 BEAM artifacts through the directory declaration.
The aggregate row shape is inside the ji1s simulation envelope.

| measure | implemented EEP-48 adapter | ji1s projection |
| --- | ---: | ---: |
| `code.qualified_name` rows | 13,928 | 13,842 handles |
| `Contains` edges | 13,849 | 12,777 containment edges |
| `Cites` edges from doc markdown | 7,314 | coverage-graded; structured links absent |
| `Implements` edges | 445 | 444 |
| `UsesType` edges | 34 | sparse and coverage-graded |
| hidden rows | 5,480 | 5,479 hidden modules+members |
| deprecated rows | 1,226 | 1,226 |

The `Cites` count is higher than the ji1s structured-link count because the
implementation parses markdown doc text instead of relying on structured Docs
chunk link metadata. That is the locked contract: EEP-48 supplies docs and
metadata; the adapter extracts citations from doc text and treats coverage as
REPORT-weak.

Doc-state distribution:

| doc state | rows |
| --- | ---: |
| hidden | 5,480 |
| none | 4,209 |
| documented | 4,160 |

Hidden documentation remains explicit `code.hidden` / `code.doc_state`
metadata. It is not silently dropped and it is not `FactVisibility`.

## Content budget

The content budget fired on the first real doc-heavy corpus:

| measure | bytes |
| --- | ---: |
| content budget | 1,048,576 |
| docs total | 6,149,717 |
| member docs | 5,812,358 |
| signatures | 420,136 |

Budget disposition:

| disposition | count |
| --- | ---: |
| truncated | 1 |

The degradation is deterministic and visible: aggregate budget metadata lands
on the package root, and member docs are first-paragraph/per-item-capped before
module docs or signatures are sacrificed. Graph facts, handles, and edges are
not budgeted away.

## Joint-graph drift profile

Cold drift refresh over the joint graph took 191.92s on a loaded local
machine. The warm aggregate `drift_profile` query took 24.74s. The committed
evidence is aggregate only:

| bucket | count | share |
| --- | ---: | ---: |
| referent-moved | 595 | 38.5% |
| referent-drifted | 398 | 25.7% |
| referent-intact | 343 | 22.2% |
| referent-unknown | 169 | 10.9% |
| referent-gone | 29 | 1.9% |
| referent-moved-ambiguous | 12 | 0.8% |

The private joint-graph milestone holds: the profile is dominated by moved/
drifted/gone/unknown evidence, so aggregate-first drift surfaces are the right
product shape. Intact references exist and remain visible, but they no longer
swamp the stale-spec story.

## Contract verdict

The bilingual §2 contract survives the EEP-48 implementation without adapter
exceptions:

- EEP-48 and rustdoc JSON both sit behind the same `Source` contract.
- Item identities are path-prefixed adapter-local ids; Elixir members use
  name/arity ids such as `path#Module.fun/2`.
- `code.qualified_name` is mandatory metadata and carries the stable name.
- Behaviours from EEP-48 metadata and protocols from source scanning both
  become kind-discriminated `Implements` evidence.
- EEP-48 doc links are parsed from markdown doc text and stay coverage-graded.
- Hidden/generated/private/test class data is metadata, not visibility.
- Missing/stripped docs chunks are declared degraded premises, while malformed
  Docs chunks are errors.

## Arc retrospective

What held:

- The evidence boundary held. Git/blame/drift work stays out of eval; code
  artifacts are declared inputs to extraction.
- The two-key identity model held under Rust and Elixir, including
  multi-module files and name/arity member ids.
- The content policy was necessary. Rust docs fit comfortably; Elixir member
  docs forced the budget and proved the visible-degradation path.
- The drift oracle generalized from the self-corpus to a private application
  joint graph without a language-specific rewrite.

What bent:

- EEP-48 link evidence is weaker than Rust doc-link evidence. The adapter must
  parse prose markdown, and absence of links is only weak evidence.
- `UsesType` is much sparser in the implemented EEP-48 projection than the
  simulation's optimistic typespec count. It remains useful only as
  coverage-graded structure.
- Cold drift refresh is expensive enough that cache hygiene and explicit
  refresh surfaces remain product concerns, not internal details.

What surprised me:

- The mass-staleness profile is more move-heavy than expected. Rename lineage
  is not just an edge case; it is the leading drift class in the private joint
  graph.
- The content budget is not a theoretical guardrail. It fired immediately on
  the first real Elixir corpus, and it did so without losing graph evidence.
