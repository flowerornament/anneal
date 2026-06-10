---
status: current
locked: 2026-06-10
date: 2026-06-10
authors: [claude]
reviewed-by: codex (adversarial, 2026-06-10 — REVISE-THEN-LOCK; all 10 findings folded; EEP-48 sim gates the contract before any adapter code)
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
produces it.

**Artifact provenance is a manifest, not an mtime.** mtime is not an honest
freshness oracle (CI restores artifacts, checkouts rewrite mtimes, an
artifact can be built from a different tree). The honest premise contract:
an **artifact manifest** alongside the artifact — tool + language version,
artifact format version, source root, **source revision**, package
identity/version, build command/profile, generated-at. Manifest present →
the premise is a verifiable PRE-FLIGHT fact (artifact revision vs corpus
revision is itself a drift check). Manifest absent → the premise degrades
to **`artifact_revision_unknown`**, declared, never inferred from mtime.
EEP-48 has its own degraded states, stated honestly: docs may live in the
`.beam` `Docs` chunk *or* external `doc/chunks`, and may be **stripped** —
"docs artifact unavailable/stripped" is a declared premise, not an error or
a silent gap. This is the 903i lesson (verified provenance or explicit
unknown) applied to the artifact itself.

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
  The substrate never parses them. Never raw rustdoc numeric ids (unstable
  across builds).
- **Identity is two-keyed.** The handle id (`path#…`) is the corpus
  identity, deliberately *path-currency-sensitive*: a file move changes
  handle identity, and drift/lineage routes it — that is the design, stated.
  The **stable name rides as metadata**: `*meta{key: "code.qualified_name"}`
  (`regex::Match`, `Herald.Agent.run/2`) for cross-file movement, re-export
  reasoning, and name-based joins. Re-exports do **not** mint duplicate
  canonical handles — one definition handle plus `Reexports`
  edges/metadata.
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

**`Implements` carries evidence, not just endpoints** (the impl-collapse is
safe only because of this): implementer, target (or external), file/line,
an implementation **kind** (`trait_impl` / `blanket_impl` / `protocol_impl`
/ `behaviour`), and a display constraint/signature string where available —
`impl<T> Trait for T where …` is not the same fact as
`impl Trait for Concrete` and must not flatten into it. Mechanism (extra
`*edge` fields vs `*meta` keyed by edge `native_id` vs derived helpers) is
decided at the slice design — `*edge` today carries no per-edge attribute
surface beyond identity, so this is a named decision, not an assumption.

Content policy (the scale lever, decided): **v1 content = docs + signatures
only** (~283KB-class — bqqc). Source *bodies* are not ingested; `read` on a
code handle returns signature + docs, and the handle's `file`/`origin_uri`
points at the source for ordinary file reading. Body ingestion is a later,
separately-gated choice.

## 3. Lifecycle and the lattice (per-ecosystem config, per CR-D36)

The v1 lifecycle claim is deliberately modest — bqqc measured `since`
coverage at **zero** on a mature crate, so ordinary crates do *not* yield a
stable/unstable lattice from rustdoc:

- **v1 Rust lifecycle = `deprecated` as terminal where present** +
  `code.class` as authority metadata + release tags as package-level
  evidence. Stability attrs participate only when the artifact actually
  carries trustworthy ones (std-class crates). Elixir likewise: EEP-48
  `:deprecated`/`:since`/`hidden` exist as a standard, but coverage is
  empirical — measured by the EEP-48 sim, never assumed.
- **Code class is metadata, not status**: `*meta{key: "code.class",
  value: "test"|"generated"|"private"|"public-api"}` (layer 2) — class
  qualifies lifecycle *authority* (bqqc: attrs cover the public API only;
  Elixir: `@moduledoc false` marks hidden), it does not occupy the lattice.
  And **`code.class` is not `FactVisibility`**: the runtime's
  Public/Team/Private envelope is actor access control; `code.class:
  "private"` is a *classification fact* visible to ordinary eval. Same
  dangerous word, different axes — named here so it never blurs.
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
- **Line citations resolve to the file, full stop** (`foo.rs:1078` → the
  file handle + at most a nearest-item REPORT hint). Item-level upgrade
  waits for an item span index and an ambiguity policy.
- **Drift dispositions are stored facts from a `SourceDriver`-adjacent
  refresh step** — decided (was open Q3): a check-time provider would make
  query results depend on ambient git at evaluation and bypass the planned
  executor/store contract. A refresh verb may *trigger* the step; the
  output lands as facts through a proper extraction transaction. The named
  hard part: blame decoration of markdown edges must happen **inside the
  markdown batch before merge** (or via an explicit source-owned decoration
  transaction) — a second source never mutates another source's `*edge`
  rows after the fact. Surfaced under `referent_currency_*` /
  `assertion_drift_*` per the arc's vocabulary discipline.

## 5. The CR-D8 amendment (earned by 903i; its own hard-gated slice)

`*edge` gains nullable **`assertion_date`** + **`assertion_revision`** —
named to avoid the live collision: every stored relation already exposes
`revision` through `FactIdentity` (source-fact revision, `facts.rs`), which
is a *different fact* than when an assertion was authored. The names carry
the distinction.

```
*edge{from, to, kind, file, line, assertion_date, assertion_revision,
      corpus, source, generation}        # identity still carries revision
```

- Populated **verified-or-null**. The markdown adapter populates from the
  out-of-band blame pass (903i: 100% coverage, ~28ms median); population
  mode (eager / generation-incremental) is an extraction option, default
  incremental; decoration happens inside the markdown batch (§4).
- Fallback stays distinguishable by *presence* (assertion_date set vs null
  + handle-date fallback at derivation time) — no extra column.
- Within-corpus payoff ships with it: dated `Supersedes` → lineage shows
  *when*.
- Spec edit: CR-D8 stored-relation block + §10 table; registered in Part XV.
- **Blast radius is real and owned as its own slice**: `EdgeFact`,
  `STORED_RELATION_DESCRIPTORS`, `TupleDb::edge_values`, named-row
  projection, stored-field validation/PlanCatalog schemas, introspection/
  describe, every EdgeFact test constructor, the markdown adapter, CLI +
  snapshot/time-scope tests, release docs — gated byte-identical (null
  fields) + perf.

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

**The EEP-48 extraction sim (`anneal-ji1s`) is a BLOCKING pre-slice gate —
it runs before the §2 contract text is final and before any Rust layer-1
code lands.** Otherwise Rust code freezes the contract and Elixir gets
forced into adapter exceptions later. Out-of-band, same discipline: read
the `Docs` chunks from herald's compiled `.beam` files (including the
degraded cases: stripped chunks, external `doc/chunks` fallback), project
handles/edges per §2, and report: item-kind counts, deprecation/`since`/
hidden coverage, typespec coverage (the `UsesType` sparsity expectation,
measured), doc-link shape, scale, and **a contract verdict — which §2
rules the Elixir data bends.** herald is also the second
self-corpus-style milestone: its `.design` is the named mass-staleness
case, so the joint graph lands there with the drift profile leading.

Implementation slicing (gated by the sim):

0. **`ji1s` EEP-48 sim** — the contract gate (above).
1. **CR-D8 `assertion_date`/`assertion_revision`** (schema + store +
   markdown blame population, incremental) — independently valuable,
   smallest, unblocks drift facts; the blast-radius slice, gated hard (§5).
2. **anneal-code layer 1, Rust** (rustdoc-types ingestion → FactBatch; the
   crate skeleton implements `Source` against the sim-proven §2 contract).
3. **Layer 2 classification** (+ lattice profile, obligations, version tags)
   — built tree-sitter/ast-grep-portable from the start.
4. **The resolver + drift refresh step** (joint-graph federation over the
   anneal self-corpus; surfaces lead with the aggregate drift profile;
   `describe code` teaches that code handles are API/documentation handles
   — `read` returns signature + docs, bodies live at `origin_uri`).
5. **anneal-code layer 1, Elixir** (EEP-48 ingestion behind the same
   contract) + the herald joint-graph milestone — the proof the contract
   held.

## Open questions — resolved at review
1. **Impl-collapse** → safe, with the `Implements` evidence vocabulary (§2):
   kind-discriminated (`trait_impl`/`blanket_impl`/`protocol_impl`/
   `behaviour`) + constraint string; the attribute-mechanism choice (edge
   fields vs `*meta` by edge `native_id` vs derived) is the named decision
   of slice 2's design.
2. **Item ids** → two-keyed: `path#<opaque adapter-local id>` as corpus
   identity (path-currency-sensitive by design; drift/lineage routes moves)
   + `code.qualified_name` metadata for stability; re-exports never mint
   duplicate canonical handles; raw rustdoc numeric ids never used.
3. **Drift materialization** → SourceDriver-adjacent refresh step producing
   stored facts in a proper transaction; refresh verb may trigger; never a
   check-time provider; blame decoration happens inside the owning source's
   batch.
4. **Federation minimum** → still open, deliberately: the smallest two-root
   query space for the self-corpus milestone is slice 4's design question.
5. **Slicing order** → CR-D8-first confirmed, with the EEP-48 sim promoted
   to slice 0 — a blocking contract gate before any adapter code.
