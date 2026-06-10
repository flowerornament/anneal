---
status: evidence
date: 2026-06-10
authors: [codex]
bd: anneal-bqqc
relates:
  - 2026-06-09-code-and-corpus.md
  - 2026-06-10-903i-oracle-audit.md
---

# bqqc extraction sim: rustdoc JSON over `regex` — 2026-06-10

This is the second proof-plan sim for the code-and-corpus arc. It is
out-of-band: no adapter code, no product predicates, no eval-time rustdoc or
git. The sim asks what a real `rustdoc --output-format json` artifact can
support if it becomes an anneal source.

Target:

- Crate: `regex`
- Version: `1.12.4`
- Source: public `rust-lang/regex` repository cloned to `/tmp`
- Rustdoc JSON: nightly `rustdoc 1.96.0-nightly`, `cargo rustdoc -p regex --lib --all-features -- -Z unstable-options --output-format json`
- Artifact: `.design/evidence/bqqc/regex-rustdoc-sim.json`

## Scale

The `regex` crate's public library rustdoc JSON is 1.3 MB. The projected
adapter output, if modeled as item handles plus containment/type/doc-link
edges, is:

| measure | regex rustdoc sim |
| --- | ---: |
| projected handles | 1,197 |
| public projected handles | 1,191 |
| projected edges | 3,011 |
| containment edges | 1,629 |
| type-reference edges | 1,193 |
| doc-link edges | 189 |
| rustdoc doc content bytes | 282,783 |
| source tree bytes | 5,507,081 |
| Rust source files | 225 |

Runtime scale reference, using the current release binary:

| corpus | handles | edges | load floor | status/fixpoint |
| --- | ---: | ---: | ---: | ---: |
| anneal self `.design` | 171 | 1,209 | 0.14s | 0.25s |
| external smoke corpus | 1,668 | 15,927 | 0.88s | 2.52s |
| `regex` projected | 1,197 | 3,011 | sub-second likely | below external smoke likely |

Projection caveat: this is a graph-shape estimate, not an adapter benchmark.
The scale risk is not item/edge count; it is content policy. Emitting
rustdoc docs/signatures is small. Emitting every source body as content spans
turns a 283 KB docs source into 5.5 MB of code text before indexes, snippets,
or future item-level spans.

## What comes out

Item kinds:

| kind | count |
| --- | ---: |
| impl | 756 |
| function | 340 |
| assoc_type | 40 |
| struct | 37 |
| module | 8 |
| use | 7 |
| trait | 2 |
| variant | 2 |
| enum | 1 |
| struct_field | 4 |

Visibility is effectively public API only: `rustdoc` was built with
`includes_private=false`. The JSON still contains a few crate-visibility
support items, but it is not a private-code corpus.

## Axis verdicts

| axis | verdict | evidence |
| --- | --- | --- |
| lifecycle | **confirm, but split public API from code class** | rustdoc cleanly exposes public API and deprecation metadata, but not private/test/generated reachability. Source scan found 584 `pub(crate)` mentions and 73 test-bearing files outside the public rustdoc surface. |
| currency | **re-grade weaker** | `regex` has 0 deprecated public items in this sample. `#[deprecated(note)]` remains useful when present, but it cannot be the main currency oracle. Treat as REPORT unless note resolves to an item. |
| topic | **confirm strong, with cap from day one** | 1,193 type-reference edges over only 37 unique type targets. `Option` appears 81 times; crate-local types such as `Match`, `Captures`, and `RegexBuilder` also exceed a 40-edge cap. `Vec` is not the only mega-target. |
| importance / structure | **confirm with a caveat** | Containment is excellent: 1,629 edges, max path depth 4, 756 impl items. Type-reference/use edges are available. Body call edges are not in rustdoc JSON, so call graph needs another source if it matters. |
| relevance | **confirm** | 434 named items, 232 documented items, 282 KB of docs, and 380 signature-bearing items. Names/docs/signatures are a rich retrieval substrate. |
| recency | **reject `since=` as a general oracle** | `since` coverage is 0. Git dates exist, but they date file/repo edits, not item authorship. Recency needs the 903i-style git/assertion distinction or release metadata, not rustdoc attrs. |
| convergence | **confirm release-cadence signal, not snapshot settling** | The repo has main-crate version tags through `1.12.4`; release cadence is available. This is package evolution evidence, not convergence/status settling by itself. |
| obligations | **weak but real if source scan is in scope** | 22 TODO/FIXME mentions across 16 files. Rustdoc JSON alone does not surface them; source scanning does. |

## Specific findings

### Topic cap

The nondiscriminative-target policy must apply to code, but the first cap
cannot hard-code only `Vec`/`Option`. In this sample, the top type targets
are:

| target | refs |
| --- | ---: |
| Option | 81 |
| Match | 80 |
| Captures | 66 |
| RegexSetBuilder | 66 |
| RegexBuilder | 66 |
| Regex | 64 |
| SetMatchesIter | 50 |
| RegexSet | 48 |
| CaptureNames | 48 |
| SubCaptureMatches | 48 |

Some high-degree targets are semantically central crate concepts, not generic
library hubs. The cap should be corpus-relative and axis-local: hide or dampen
mega-targets for pairwise topic coupling, but do not erase them from structure
or importance.

### Rustdoc is not a call graph

Rustdoc JSON gives type references, doc links, containment, signatures, and
impl structure. It does not give body-level call edges. So the first adapter
should not promise "calls" unless it adds a second extractor. The honest first
edge family is "uses in public signature/type surface," not "calls."

### Deprecation is sparse

This crate has no deprecated public rustdoc items in the current sample.
That does not refute deprecation as a currency signal; it refutes relying on
it as the primary code-currency oracle. Code currency still needs git drift,
reachability/export class, and declared deprecation when present.

### Source scanning changes the axis grades

Public rustdoc alone is not enough for lifecycle, obligations, generated/test
classification, or private implementation structure. Source scanning found:

- 225 Rust source files
- 5.5 MB source bytes
- 584 `pub(crate)` mentions
- 73 test-bearing files
- 42 generated-marker textual hits
- 22 TODO/FIXME mentions

Some of those markers are noisy textual hits. They are enough to prove that
source classification is required; they are not yet a final classifier.

## Recommendation

Adapter design is ready for a first design pass, but not for a "rustdoc JSON
is the code corpus" claim.

Build the first `anneal-code` design around two layers:

1. **Public API layer from rustdoc JSON**: crate/module/type/function/trait/
   impl handles, containment, doc links, signatures, type-reference edges,
   docs/signature content, deprecation metadata.
2. **Source-classification layer from source text/git**: private/test/
   generated class, TODO/FIXME obligations, file-level recency, and later
   body-level call edges if the product needs them.

The surprise is that rustdoc JSON is both richer and narrower than expected:
it is excellent for structure/relevance and good enough for type-coupling,
but it is not a whole-code oracle. The code adapter should be explicitly
source-composite from day one, rather than pretending one extractor owns all
axes.
