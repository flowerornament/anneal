---
status: draft
date: 2026-06-10
authors: [claude]
bd: anneal-d36e
relates:
  - 2026-06-09-code-and-corpus.md            # the arc spec this realizes (§3 data model, §6 proof plan step 3)
  - 2026-06-10-903i-oracle-audit.md          # evidence: the drift oracle + edge-date amendment
  - 2026-06-10-bqqc-extraction-sim.md        # evidence: the two-layer shape, scale, axis verdicts
  - 2026-05-13-corpus-runtime.md             # §5 Source · §41 handle kinds · CR-D8 · CR-D41 · CR-D36 lattice
---

# anneal-code: the first design pass — 2026-06-10

**For codex adversarial review before implementation slicing.** Both sims are
in; every decision below cites its evidence. This is the v1 adapter + the
cross-corpus resolver + the CR-D8 edge-date amendment — the smallest design
that makes the joint graph real.

**Designed bilingually (Morgan, 2026-06-10): Rust and Elixir together, so the
contract cannot absorb Rust-isms.** The fact mapping (§2) is the
language-neutral contract; everything Rust-specific is explicitly *adapter
policy*, and every contract claim is checked against both ecosystems. Elixir
is not hypothetical — herald is the kinship target (§55) and the named
mass-staleness corpus — and it has the same artifact shape: **EEP-48**, the
BEAM-standard docs chunk compiled into `.beam` files (module/function docs,
signatures, `:deprecated` and `:since` metadata), is to Elixir what rustdoc
JSON is to Rust. Layer 1 stays "ingest a pre-built compiler-blessed
artifact" in both languages; layer 2 (tree-sitter/ast-grep + git) ports
unchanged.

## 1. The shape: a source-composite adapter (evidence-fixed)

bqqc's verdict, adopted: one extractor does not own all axes. `anneal-code`
is **two layers behind one `Source` impl**:

```
                       anneal-code (implements Source, §5)
        ┌────────────────────────┴────────────────────────┐
  LAYER 1: public-API                          LAYER 2: source classification
  reads a PRE-BUILT rustdoc JSON artifact      scans the source tree + git
  (rustdoc-types, format-version pinned)       (pattern class: test/generated/
  → item handles, containment, type-ref         private; TODO/FIXME obligations;
    + doc-link edges, signatures, docs,         file recency class)
    deprecation metadata                       → *meta class rows on handles
```

**Layer 1 ingests an artifact, it does not build one.** The adapter never
shells out to `cargo rustdoc` (nightly-only, slow, nondeterministic at
extraction): the project declares the artifact path —
`source code { rustdoc_json("target/doc/regex.json"). }` — and CI/the user
produces it. Honest premise handling (CR-D103/PRE-FLIGHT): the artifact's
mtime/format_version are extracted as facts; a stale or missing artifact is
a *declared premise state*, not a silent gap. This is the evidence boundary
from the arc spec applied to rustdoc exactly as it was applied to git.

**Layer 2 runs at extraction over the source tree** — cheap, deterministic,
language-portable (tree-sitter/ast-grep-class patterns; plain scans where
they suffice). Its output is *classification metadata*, not handles: code
class (`public-api` / `crate-private` / `test` / `generated`), obligation
markers, file-level recency class.

**SCIP: evaluated, deferred.** rustdoc JSON covers v1's edge needs
(containment, type refs, doc links — bqqc); SCIP is the upgrade path if a
consumer demands body-level reference/call edges, and the natural layer-1
for languages without a rustdoc (it slots behind the same `Source` without
contract change). rust-analyzer crates stay out entirely.

## 2. The fact mapping (the language-neutral contract)

Substrate kinds stay closed (§41) — code maps onto them, it does not extend
them. **The contract row is the left three columns; the per-language columns
prove no Rust-ism leaked:**

| code thing | handle kind | id (contract: adapter-chosen, path-prefixed for items) | Rust | Elixir |
|---|---|---|---|---|
| source file | `file` | repo-relative path | `src/regex.rs` | `lib/herald/agent.ex` |
| **unit** (the language's primary nameable) | `section` | `path#<adapter-local item id>` | `src/regex.rs#Match` | `lib/herald/agent.ex#Herald.Agent` |
| **member** (functions/consts of a unit, if itemized) | `section` | `path#<unit>.<member-id>` | `…#Match::range` | `…#Herald.Agent.run/2` (name/arity is the id — the contract never assumes bare names) |
| package (crate / OTP app) | `file` (root) + `*meta{key:"code.package"}` | root file path | `src/lib.rs` | `mix.exs` |
| version/release | `version` (tags, layer 2) | tag string | `regex-1.12.4` | `herald-0.4.0` |
| external dep | `external` | external name/path | — | — |

Contract rules (language-neutral; each was a latent Rust-ism caught by the
bilingual check):

- **Item identifiers are adapter-local opaque strings** after the `path#`
  prefix — Rust uses type/fn names, Elixir uses `Module` and `fun/arity`.
  The substrate never parses them.
- **Granularity is adapter policy, not contract.** Rust policy: impl blocks
  collapse into the implementing type (bqqc: 756/1,197 projected handles are
  impl ceremony; trait impls become `Implements` edges). Elixir policy:
  module-per-unit (the module IS the unit; `defimpl` maps to `Implements`
  protocol edges — the structural analogy holds without being forced).
- **`file`-kind handles are keyed by repo-relative path** — this is what
  makes the §3b resolver a join (`903i`: resolver hit = path match), and it
  is language-independent. A citation to a path resolves to the file handle,
  never implicitly to its items.

Edge kinds (open strings per CR-D8; population is per-adapter and
coverage-graded — a sparse edge family is REPORT-weak, not absent-and-faked):

| edge | from → to | Rust source | Elixir source |
|---|---|---|---|
| `Cites` | item → item/external (doc links) | rustdoc doc links | EEP-48/ExDoc doc links — same kind as markdown citations deliberately: doc links ARE citations; topic coupling composes unchanged |
| `UsesType` | item → type (declared-type surface) | signatures (rich) | typespecs (**optional in the ecosystem — sparse coverage expected and declared**, never padded) |
| `Implements` | unit → trait/protocol/behaviour | trait impls | `defimpl`, `@behaviour` |
| `DependsOn` | package → package | Cargo.toml | mix.exs |

Content policy (the scale lever, decided): **v1 content = docs + signatures
only** (~283KB-class — bqqc). Source *bodies* are not ingested; `read` on a
code handle returns signature + docs, and the handle's `file`/`origin_uri`
points at the source for ordinary file reading. Body ingestion is a later,
separately-gated choice.

## 3. Lifecycle and the lattice (per-ecosystem config, per CR-D36)

```
config convergence {                      # Rust profile         # Elixir profile
  ordering(["unstable", "stable"]).      # compiler attrs        # @doc since / releases
  active(["unstable"]).
  terminal(["deprecated"]).              # #[deprecated]         # @deprecated (EEP-48)
}
```

- `status` from the artifact's deprecation/stability metadata where present
  (rustdoc attrs; EEP-48 `:deprecated` / `:since` / `hidden`). **Oracle
  strength differs by ecosystem and the dispositions say so** — Rust's
  lattice is compiler-enforced; Elixir's is convention-carried metadata;
  both are declared oracles, graded by coverage (the arc spec's principle:
  weaker oracle → weaker disposition, never papered).
- **Code class is metadata, not status**: `*meta{key: "code.class",
  value: "test"|"generated"|"private"|"public-api"}` (layer 2) — class
  qualifies lifecycle *authority* (bqqc: attrs cover the public API only;
  Elixir: `@moduledoc false` marks hidden), it does not occupy the lattice.
- Deprecation-note successor resolution: REPORT-only hint unless the note
  names a resolvable item (bqqc currency re-grade; and 903i already made
  referent-drift the load-bearing currency oracle, so this is bonus signal).

## 4. The resolver (§3b made concrete)

A derived relation in the federated runtime, no new stored machinery:

```
code_ref(spec, path, code_handle, disposition) :=
  *edge{from: spec, to: path, kind: "Cites", corpus: doc_corpus},
  *handle{id: path, kind: "external", corpus: doc_corpus},
  *handle{id: code_handle, kind: "file", file: f, corpus: code_corpus},
  path_resolves(path, f).            # normalization; corpus/root-qualified
```

- `path_resolves` is the normalization seam (root-prefix stripping, `./`,
  case policy) — keyed by corpus/root per the arc spec; single-repo v1, the
  key space designed for federation now.
- Non-resolution feeds the drift dispositions (903i's seven buckets), never
  errors.
- Drift dispositions are **materialized facts from the out-of-band oracle**
  (the audit machinery productized as a `SourceDriver`-adjacent step or
  check-time provider — never eval-time git), surfaced under the
  `referent_currency_*` / `assertion_drift_*` namespace per the arc's
  vocabulary discipline.

## 5. The CR-D8 amendment (earned by 903i; rides this slice)

`*edge` gains nullable `date` + `revision`:

```
*edge{from, to, kind, file, line, date, revision, corpus, source, generation}
```

- Populated **verified-or-null**. The markdown adapter populates from the
  out-of-band blame pass (903i: 100% coverage, ~28ms median, out-of-band);
  population mode (eager / generation-incremental) is an extraction option,
  default incremental.
- `date_source` stays distinguishable: a blame-populated edge date differs
  from handle-date fallback by *presence* (edge.date set vs null + handle
  fallback at derivation time) — no extra column.
- Within-corpus payoff ships with it: dated `Supersedes` → lineage shows
  *when*.
- Spec edit: CR-D8 stored-relation block + a sentence in §10's table;
  registered as a CR-D in Part XV.

## 6. Acceptance (the gates for the implementation slices)

- **Self-corpus joint graph stands up**: anneal `.design` (markdown) +
  `crates/` (anneal-code) under federation; `code_ref` resolves the 110
  audited citations at ≥ the audit's hit-rate (76/110), with the remainder
  carrying their drift dispositions — the audit table is the differential
  baseline.
- The split canary holds in the *product*: `src/cli.rs` citations surface
  `referent-moved-ambiguous`, never a clean head.
- regex-as-corpus extracts within the bqqc scale projections (≤1.3× on
  handles/edges/bytes); load + status within the external-smoke envelope.
- Markdown corpora: byte-identical everywhere (the adapter is additive);
  perf held (the standing gate).
- `axis_of` placement test passes with the new predicates; `describe`
  cards for the code vocabulary; the teaching ladder reaches them.
- No nightly toolchain required at anneal runtime (artifact ingestion only).

## 7. The third sim, then slicing

**Before the contract locks: the EEP-48 extraction sim (the Elixir twin of
bqqc), run against herald.** Out-of-band, same discipline: read the `Docs`
chunks from herald's compiled `.beam` files, project handles/edges per §2,
and report: item-kind counts, deprecation/`since`/hidden coverage, typespec
coverage (the `UsesType` sparsity expectation, measured), doc-link shape,
scale, and **a contract verdict — which §2 rules the Elixir data bends.**
herald is also the second self-corpus-style milestone: its `.design` is the
named mass-staleness case, so the joint graph lands there with the drift
profile leading (arc spec §2).

Implementation slicing (proposed; codex re-slices at review):

1. **CR-D8 edge date/revision** (schema + store + markdown blame population,
   incremental) — independently valuable, smallest, unblocks drift facts.
2. **anneal-code layer 1, Rust** (rustdoc-types ingestion → FactBatch; the
   crate skeleton implements `Source` against the §2 contract).
3. **Layer 2 classification** (+ lattice profile, obligations, version tags)
   — built tree-sitter/ast-grep-portable from the start.
4. **The resolver + drift materialization** (joint-graph federation over the
   anneal self-corpus; surfaces lead with the aggregate drift profile).
5. **anneal-code layer 1, Elixir** (EEP-48 ingestion behind the same
   contract) + the herald joint-graph milestone — the proof the contract
   held.

## Open questions for review
1. The Rust impl-collapse policy: right call, or does it lose load-bearing
   structure (trait-impl docs, blanket impls)? What does `Implements` need
   to carry in both languages?
2. Item-handle id scheme: `path#<adapter-local id>` (resolver-friendly;
   collision risk on Rust re-exports? Elixir multi-module files?) vs
   fully-qualified names (stable, but breaks the file-keyed join) — and
   does the Elixir 1-module-per-file convention paper over a contract gap?
3. Drift-fact materialization: `SourceDriver` step vs check-time provider vs
   a dedicated refresh verb — where does the out-of-band oracle live in the
   product (it cannot be eval-time)?
4. Federation surface: §53 defers multi-root UX — what is the *minimum*
   federation needed for the self-corpus milestone (two roots, one query
   space), and does it stay behind a flag?
5. Slicing order: is CR-D8-first right, and where exactly should the EEP-48
   sim land — before slice 2 (contract gate) or parallel to it?
