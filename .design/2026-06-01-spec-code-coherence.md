---
status: draft
---

# Spec→Code Coherence: one diagnostic, no code backend

A live spec that names a code path which no longer exists is lying to the next
cold agent. This document specifies a single diagnostic that surfaces exactly
that — and explains, with evidence, why anneal does **not** build a code-corpus
adapter to get it.

## The question this answers

For a maintainer whose code leads its specs: *"which of my trusted specs now
point at code that has moved or been deleted?"* That is the practical, daily
form of spec/code drift in a code-leads workflow — and it is the only form the
evidence supports building for.

## Evidence (why this shape, and not the others)

Four candidate drift signals were simulated against three real corpora
(anneal `.design`, murail, herald). Results:

- **mtime drift** (code modified after the spec): noise. murail showed 3 of 96
  live-spec→existing-code pairs drifting >30 days. When code leads specs it does
  not quietly mutate a named file — it moves, renames, or deletes it. Drift
  lands as a broken *structural* reference, not a stale timestamp.
- **vocabulary-name drift** (specs mention retired predicates/verbs): noise
  cannon. Dozens of hits, almost all English homonyms (`recent`, `diff`,
  `areas`) or audit specs whose job is to *document* the retirement.
- **existence, ungated**: too broad. herald surfaced hundreds of MISSING rows,
  mixing real drift with path-resolution false-missing.
- **existence, status-gated** (live spec status + cited code path missing):
  the signal. murail: ~383 crate-path citations collapse to ~12 clean
  candidates — e.g. an `authoritative` template-expansion spec citing a deleted
  `desugar.rs`, a `stable` parser review citing a moved `parse/parser.rs`.
  anneal: 2 candidates, both v1→v2 workspace-migration casualties.

The discriminator is **spec status**, not time. Gate on it and the noise
collapses; the survivors are real.

## Why no code backend

The Phase 1 code-as-corpus spike (`2026-05-30-code-as-corpus-spike.md`) proved
rustdoc JSON *can* be ingested as a corpus with stability-as-lattice. The
evidence here says it should not be built for this purpose:

- The useful signal needs the code path as a *referent*, not as a corpus of
  nodes with its own lifecycle.
- anneal already extracts code references (v0.14): a body mention like
  `crates/anneal-core/src/runtime/eval.rs:10-20` becomes an external handle
  carrying `external_class: "code"` and `target_path` metadata.
- "Is this API stable?" competes with rustdoc/rust-analyzer. "Is what this spec
  says still where it claims?" is a question only anneal — holder of the spec
  graph — can answer.

So the feature reuses the typed graph, status partitions, and code-refs that
already ship. The only genuinely new primitive is a path-existence probe.

## The one architectural decision: probe at extraction, consume as fact

Datalog cannot stat the filesystem. `broken_reference` (E001) works because it
checks handle existence *within the graph*; on-disk existence of a `target_path`
is not a graph fact. So the probe runs at **extraction time** — Datalog consumes
a fact, it never performs IO.

The probe is a **shared core helper**, not markdown-private. Recognition (what
counts as a code reference in this source) is adapter-specific; *target
existence* is standard. `anneal-md` calls the helper for every
`external_class=code` handle it emits, and any future adapter that emits
`external_class=code` gets identical tri-state behavior for free.

Metadata key: **`target_exists`** = `true` | `false` | `unknown`, scoped by
`external_class=code` in the consuming rule — NOT `code.target_exists`. A
`code.` meta prefix would reintroduce the exact key-prefix ambiguity the v0.14
metadata cleanup removed (`md.code_path` → `target_path`). `code` is an
external-class value, not a metadata namespace.

### Base resolution (the real design surface)

herald's false-missing came from unresolved `lib/...` paths when `.design` is
nested inside a larger repo whose code lives at the repo root. The probe
resolves `target_path` against a **confidence-ranked base list**:

1. The enclosing repository / language-workspace root, found by walking up from
   the corpus root for `.git` or workspace markers (`Cargo.toml` workspace,
   `mix.exs`, etc.).
2. The corpus root itself — used only when no enclosing-repo marker exists, or
   when the target actually resolves there.

Then:

- Confident base + normalized `target_path` present → `true`.
- Confident base + normalized `target_path` absent → `false`.
- No confident base, or an absolute / `..`-escaping path → **`unknown`**.

Only a confident `false` is a diagnostic. `unknown` never is. (CR-R12 applied:
do not raise a confident drift claim over a path we could not resolve.) When a
base is found, the probe also records `target_probe_base` (and
`target_resolved_path` when known) so a cold agent can audit *why* a path was
judged missing.

### Drift is "existed then vanished," not "absent now" (precision fix, 2026-06-01)

Running the first `false`-on-disk version against anneal's **own** corpus
surfaced a residual false-positive class the murail run did not: **illustrative
example prose.** Design docs that *teach* the code-ref feature quote sample
paths — e.g. the v015 retrieval doc uses `lib/host-corpus/admission.rs:142-167`
as a worked example, and this doc cites `src/jit.rs` while explaining the synfx
external study. These are byte-identical to real references (both are backticked
inline-code paths), so they cannot be told apart lexically.

The discriminator is **git history**, and it is the semantically correct
definition rather than a heuristic:

- A real spec→code dependency points at a path that **existed in this
  repository's history**. When code leads specs, that path is later moved,
  renamed, or deleted — so it is *gone from disk but present in `git log`*. That
  is drift.
- An illustrative, external-codebase, or forward-plan path was **never tracked
  here** — zero commits touch it. It is not drift; it never was a dependency.

Measured: the 5 anneal-corpus false positives have **0** commits each; murail's
true-drift paths (`parse/parser.rs`, `render.rs`) have **6** and **10**. Clean
separation.

So `target_exists` becomes a three-way classification driven by *both* disk and
history, computed in the same extraction-time probe. The probe resolves the base
once, then uses a cached HEAD-history path set for that base (`git -C <base> log
--name-only --format=`). It intentionally does **not** use `--all`: an
unmerged or unrelated branch must not fabricate history for the mainline corpus.

- present on disk → `true` (no drift).
- absent on disk **and** has git history under the base → **`false`** (drift —
  the diagnostic fires).
- absent on disk **and** no git history → **`unknown`** (never tracked here:
  illustrative / external / forward-plan; **no diagnostic**).

This folds the illustrative-prose, external-study, and forward-plan classes into
the same honest `unknown` bucket as unresolvable paths — all of them are "we
cannot confidently call this drift." Record `target_history_status`
(`present`, `absent`, `unavailable`) alongside the existing probe metadata so
the classification is auditable. A history hit is evidence of drift; a history
miss or unavailable history is evidence of nothing. The
`asserts_code` status gate still applies on top; history-existence and
status are independent filters and both must pass.

Edge note: a path that existed, was deleted, and whose history was later purged
(e.g. our own murail-corpus rewrite) would read as `unknown` — acceptable, and
correctly conservative: we'd rather miss a drift row than fabricate one.

## The diagnostic

A new prelude rule, sibling to `broken_reference` (E001), gated to the
live-spec-cites-code lane:

```datalog
spec_code_drift(src, target_path, file, line, source_status) :=
  *edge{from: src, to: ref, kind: "Cites", file: file, line: line},
  *handle{id: src, kind: "file", status: source_status},
  asserts_code(source_status),                  -- corpus-declared "claims current code"
  *handle{id: ref, kind: "external"},
  *meta{handle: ref, key: "external_class", value: "code"},
  *meta{handle: ref, key: "target_exists", value: "false"},
  *meta{handle: ref, key: "target_path", value: target_path}.

diagnostic("W006", "warning", src, file, line,
           ("spec_code_drift", target_path, source_status)) :=
  spec_code_drift(src, target_path, file, line, source_status).
```

### Why `asserts_code`, not `active` (live-corpus evidence, 2026-06-01)

The first implementation gated on `active(src)`. Run against the real murail
corpus it fired **47 rows**, and the breakdown exposed that `active` is too
coarse a proxy for "this spec claims something about current code":

- ~8 `stable` rows — **undeniable rot** (a stable review citing the moved
  `parse/parser.rs`, `render.rs`). The target signal.
- ~13 `active`/`draft` rows — mostly real.
- **9 `plan` rows — aspirational**: a `status: plan` spec citing
  `murail-gpu/src/gates/...` that does not exist *because it is a forward plan*.
  This is spec-ahead-of-code, the opposite of rot. A "you drifted" warning here
  is actively wrong.
- **15 `research` rows — external-codebase studies**: e.g. a doc studying the
  external `synfx-dsp-jit` crate cites *its* `src/jit.rs`. The spec correctly
  describes someone else's repo; the code was never meant to live here.

Both noise classes are statuses that do **not** assert anything about *this
corpus's own current code*. So the gate is a corpus-declared status set, not a
hardcoded `active`:

```
config convergence {
  asserts_code([stable, current, authoritative, active, draft]).
}
```

`asserts_code(s)` defaults to the corpus's `active` set **minus** an
aspirational/study tier when unconfigured — but the corpus owns the list. murail
would exclude `research`, `plan`, `exploratory`, `reference`; that single config
choice drops both noise classes (the external studies are all `research`; the
unbuilt plans are all `plan`) and the 47 collapses to the ~13–21 real
candidates. anneal stays corpus-agnostic: *which of my statuses make a claim
about my code* is a corpus fact, not an anneal guess.

This also dovetails with `W005 lifecycle_config_gap`: a corpus that never
declares `asserts_code` falls back to the active-minus-aspirational
default, and a status that appears in neither the active partition nor the
authoritative set is exactly the kind of config gap W005 already surfaces.

The rule is tightened beyond a bare `active(src)` gate (per design review): it
requires a **status-bearing `file` source**, a **`Cites`** edge, and an
**`external`** target handle. That excludes label handles, statusless/body
noise, and non-citation edges — keeping the rule exactly in the
live-spec-cites-code lane. `active(h)` alone can admit statusless or
non-terminal handles depending on lifecycle config; `source_status != null` +
`kind: "file"` closes that.

Severity is `warning`, not `error`: a moved code path is a review candidate, not
a build break. It surfaces in `status` and `check`. The status gate is the whole
noise-control mechanism — a superseded spec citing dead code stays silent
(assuming `superseded` is configured terminal; if a corpus forgot that, the
existing `W005 lifecycle_config_gap` is the correct separate signal, not this
one's problem to absorb).

`source_status` rides in the diagnostic evidence so trust-aware ranking
(authoritative/stable-live spec citing dead code outranks a stale draft) is a
**query/`status`-ordering concern, not a severity concern**. Severity stays flat
`warning`; ranking sorts on `pipeline_position` of `source_status` (or a
status-aware tie-break in an entropy source if it needs to light up `frontier`).

## What this is NOT (scope discipline)

- Not a code corpus, not `anneal-code`, not rustdoc ingestion.
- Not semantic verification. anneal owns *structural and temporal* drift — a
  named path is gone — and ranks where to look. It never claims the code is
  *wrong*; that is an LLM judgment anneal must not fake.
- Not the inverse (`undocumented`: code with no spec). That genuinely requires
  enumerating code as graph nodes — a real backend — and is deferred until this
  proves its worth. Tracked under the code-as-corpus epic.
- Not obligation. Output is a ranked triage list surfaced on demand, never
  "update these N specs." This is what makes it usable when code permanently
  leads specs.

## Acceptance

- On murail, the diagnostic fires on the ~12 status-gated candidates and is
  silent on the ~370 aligned citations and all superseded-spec citations.
- A spec citing a path that cannot be resolved produces `target_exists:
  "unknown"` and **no** diagnostic (no false positive). An absolute or
  `..`-escaping target is `unknown`, never `false`.
- On anneal's own corpus, it flags the live specs citing the retired `src/`
  single-crate layout, and stays silent on superseded ones.
- The probe is a shared core helper; a non-markdown adapter emitting
  `external_class=code` gets the same tri-state behavior without re-implementing
  it.
- The W00X code is `describe`-able and teaches the eval form, like other
  diagnostics. Its evidence carries `source_status` so a trust-ranked review
  query can sort on it.

## Resolved during review (codex + claude, 2026-06-01)

- **Probe layer**: extraction-time, in a shared core helper that `anneal-md`
  calls — not markdown-private. Datalog consumes the fact; it never does IO.
- **Metadata key**: `target_exists` (tri-state), scoped by `external_class=code`
  in the rule — not `code.target_exists` (avoids the v0.14 meta-prefix
  ambiguity).
- **Rule shape**: requires `kind: "file"` + non-null `status` + `Cites` edge +
  `external` target, not a bare `active(src)`. Closes label/statusless/body
  noise.
- **Base resolution**: confidence-ranked (enclosing repo/workspace marker →
  corpus root → else `unknown`); record `target_probe_base` for auditability.
- **Severity vs trust**: flat `warning`; trust-ranking lives in the
  query/`status` ordering via `source_status` in evidence, not in severity.

## Open (defer to implementation, not blocking)

- Whether the trust-ranked review surface is a dedicated query, a `status`
  ordering tweak, or a small status-aware entropy source. Decide when wiring it
  into `status`; the diagnostic itself does not depend on the choice.
