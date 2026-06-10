---
status: draft
date: 2026-06-10
authors: [claude]
bd: anneal-3gk4
relates:
  - 2026-06-09-code-and-corpus.md            # §3c the oracle family · the evidence boundary
  - 2026-06-10-anneal-code-adapter.md        # the locked slicing this is 4a of
  - 2026-06-10-903i-oracle-audit.md          # the evidence: buckets, costs, canary
  - 2026-06-01-spec-code-coherence.md        # W006 — the probe this upgrades
  - 2026-05-13-corpus-runtime.md             # CR-D103/104/105 · CR-Fw6 · §44 .anneal/
---

# Drift refresh, the resolver, and the drift surface — 2026-06-10 (slice 4a)

**For codex adversarial review before 4b implementation.** This is the design
pass the containment demanded: the refresh mechanism as ONE first-class
mechanism, the federation minimum, and the CR-D105 verb-surface rungs —
locked together so 4b implements rather than decides.

## 1. The mechanism: an upgrade, not an invention

The named spaghetti risk was a *second ingestion path*. The design avoids it
by recognizing that the ingestion path **already exists**: the W006 target
probe (`target_probe.rs`, core-owned, enclosing-repo-aware, in-process
cached) already shells git at extraction time and lands evidence as `*meta`
on the external target handles (`target_exists`, `target_history_status`).
4b **upgrades that probe to the 903i oracle** — same channel, same owner,
same extraction phase:

- **From** HEAD-set-membership **to** the seven-bucket drift classification:
  commits-since-assertion (consuming slice 1's `assertion_date` /
  `assertion_revision` on the citing edge), rename/move detection with
  confidence, fanout for splits.
- Evidence lands as **structured `*meta` rows on the external target
  handles** — the existing channel, populated by the source that owns the
  citations. No new stored relation, no CR-D8 amendment #2, no cross-source
  mutation, no second ingestion path. (CR-Fw6's future `*evidence` relation
  is the eventual typed home; `*meta` is the v1 contract, exactly as W006
  established.)
- Suggested keys (final names at implementation, vocabulary discipline
  applies): `code.referent_disposition`, `code.referent_commits_since`,
  `code.referent_moved_to`, `code.referent_move_candidates`,
  `code.referent_evidence_head` (the repo HEAD the evidence was computed
  at — the premise fact).

**The refresh is therefore not a new runtime mechanism at all** — it is the
existing extraction, with a cost policy (§2). The "SourceDriver-adjacent
transaction" the arc spec required turns out to already exist as Phase B;
what 4a adds is the evidence depth and the cache that makes depth affordable.

## 2. The cost policy: tiered, with a HEAD-keyed evidence cache

903i measured ~28ms median per citation. Self-corpus ≈ 3s, murail-class ≈
25s — fine once, unacceptable per-invocation. The policy:

- **Tier 0 (always-on, unchanged):** the existing existence-class probe.
  Cheap, keeps W006 behavior identical.
- **Tier 1 (drift evidence) is cached and incremental:** a persistent
  evidence cache at **`.anneal/drift-evidence.json`** (§44 precedent:
  `history.jsonl`), keyed by `(target_path, assertion_revision-or-date,
  repo HEAD)`. A cache hit is **exact** — HEAD pins history, so cached
  rows never go stale silently; a HEAD move invalidates precisely the
  entries whose answers could change. Amortized cost: one incremental pass
  per repo commit, near-zero on unchanged repos.
- **Cold-cache honesty (CR-D103):** with no cache and tier 1 not yet built,
  surfaces show a PRE-FLIGHT premise — "drift evidence not built; run
  `anneal check --refresh-drift`" (flag name final at review) — never a
  silent 25-second stall and never fabricated dispositions. First
  population is an explicit, declared step; thereafter it is incremental
  and invisible.
- Move detection is bounded: `--follow`-class history walks run only for
  gone paths (903i: 6 + 13 of 110), with candidate caps; `moved-ambiguous`
  is a terminal answer, not a search budget.

Determinism: evidence is a pure function of `(citations, repo history,
HEAD)`; the cache key carries HEAD; trails record generations as always.
Evaluation never touches git — Phase B materializes, Phase D consumes.

## 3. The vocabulary (derived, on the currency axis)

Prelude predicates over the evidence meta — the `referent_currency_*` /
`assertion_drift_*` family per the arc's locked vocabulary discipline
(never "superseded"/"successor" for paths):

```
referent_disposition(target, d)        # the seven buckets, read from meta
assertion_drift(spec, target, n)       # spec cites target; n commits since assertion
referent_moved_head(target, new_path)  # confident chains only
drift_profile(bucket, n)               # the corpus aggregate (status/check feed)
```

All placed via `axis_of(…, "currency")`; describe cards ride 4b; the ut1j
placement test enforces placement mechanically.

## 4. The resolver (cross-SOURCE, and the federation answer)

**The federation minimum is: none.** The self-corpus joint graph is **one
corpus with two sources** — markdown (scan root `.design`) and code (the
rustdoc artifact) — which the v2.0 architecture supports natively
(`SourceContext.roots`, `source` on every fact, CR-D41 corpus-unique ids
across sources; slice 2's CLI already composes both). The resolver
therefore joins across *sources*, not corpora; §53 multi-corpus federation
stays deferred, untouched. The arc spec's "cross-corpus" framing
generalizes later without rework (the key space already carries corpus).

```
code_ref(spec, path, code_handle, disposition) :=
  *edge{from: spec, to: path, kind: "Cites", source: "markdown"},
  *handle{id: path, kind: "external"},
  *handle{id: code_handle, kind: "file", source: "code", file: f},
  path_resolves(path, f).
```

- `path_resolves` = normalization only: line-suffix stripping
  (`foo.rs:63` → `foo.rs`), `./`, the corpus-root/repo-root offset (the
  probe's enclosing-repo logic is the precedent). File-level only, per the
  locked design; nearest-item hints are post-4b.
- Non-resolution is not failure — the unresolved path's drift meta carries
  the story (gone/moved/unknown).

## 5. The drift surface (the CR-D105 rungs — what 4b is judged on)

Three rungs, no new verb, no new mode (subtractive: `handle` annotations +
`check` + `status` cover the need; `--drift` stays unspent):

1. **`status` arrival line** (only when code citations exist):
   `design→code refs: 76 intact · 74 drifted · 13 moved? · 6 gone` —
   aggregate-first per the arc spec; cold cache renders the PRE-FLIGHT
   premise line instead.
2. **`check` diagnostics**, disposition-typed: a new W-code for
   `referent-gone` / `referent-moved-ambiguous` cited by an *operative*
   spec (REPORT-severity; drift counts are information, never E-class —
   a drifted referent is normal life, a gone referent on a live spec is
   a warning). Corpus-scoped aggregate row per CR-D69 where appropriate.
3. **`handle <spec>` citation annotations**, in place:
   `Cites crates/…/ids.rs  [intact · asserted 2026-06-06]` /
   `Cites src/cli.rs  [moved-ambiguous → 11 candidates]` — each teaching
   its follow-up query (`? assertion_drift("…", t, n).`), the v0.20
   pattern.

`context`/`search` annotations: deliberately deferred until post-4b
consumer evidence — the three rungs above are where the spec↔code questions
actually get asked first.

## 6. Acceptance (4b's gates, fixed now)

- The 110 self-corpus citations resolve via `code_ref` at ≥ the 903i
  hit-rate (76/110), with every non-resolution carrying a drift
  disposition.
- **The split canary in the product**: `handle` on a spec citing
  `src/cli.rs` shows `moved-ambiguous`, fanout candidates, and never a
  clean head.
- Disposition distribution on the self-corpus matches the 903i audit table
  (the standing differential oracle) at the same HEAD.
- Cold cache → PRE-FLIGHT lines, zero git cost; warm cache → all three
  rungs live; cache invalidation on HEAD move re-probes only affected
  entries (measured).
- Markdown corpora without code citations: byte-identical everywhere.
- Perf: status/check/context within current envelopes with warm cache;
  the explicit refresh's cost reported, not hidden.
- `axis_of` placement test green; describe cards for the new predicates;
  `anneal check` clean on `.design`.

## Open questions for review
1. `*meta` keys vs promoting CR-Fw6's `*evidence` relation now — is the
   meta channel's string-typing acceptable for one more arc, or does the
   seven-bucket + candidates payload justify the typed relation early?
2. The cache file: JSON-lines vs the history.jsonl envelope; GC policy
   (HEAD-keyed entries from dead HEADs); does it belong in trails instead?
3. The refresh trigger surface: extraction flag (`--refresh-drift`) vs a
   config default (`source code { drift_evidence(on). }`) vs riding
   `check` — which is the honest CR-D105 placement for the *explicit
   first build*?
4. The new W-code's exact scope: gone-on-operative only, or also
   moved-ambiguous-on-operative? Where does drifted(n>threshold) sit —
   diagnostic or status-line-only?
5. Does the evidence-HEAD premise belong in `*meta` per target (proposed)
   or once per corpus (a `*config`-like premise row)?
