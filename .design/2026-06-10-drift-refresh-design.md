---
status: current
locked: 2026-06-10
date: 2026-06-10
authors: [claude]
reviewed-by: codex (adversarial, 2026-06-10 — REVISE-THEN-LOCK; P0 identity fix + all findings folded)
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
- Keys (final names at implementation, vocabulary discipline applies):
  `code.referent_disposition`, `code.referent_commits_since`,
  `code.referent_moved_to`, `code.referent_evidence_head` (the premise
  fact), and **one `code.referent_move_candidate` row per candidate plus a
  count** — never a JSON blob. **The meta-channel constraint (written down
  so it cannot quietly erode):** `*meta` carries scalar and repeated-scalar
  evidence only. The moment candidate tuples need queryable structure
  (confidence, reason, ranking), that is CR-Fw6's typed `*evidence`
  relation earning its existence — promoted deliberately, not smuggled
  through strings.

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
  `history.jsonl`). The key carries the full premise: `(repo identity/root,
  HEAD, target_path, assertion identity, assertion premise, oracle schema
  version, path-normalization policy version)`. **Assertion premise is a
  typed precedence ladder, preserved in keys and dispositions:**
  `assertion_revision` if present, else `assertion_date`, else an explicit
  `assertion_date_unknown` marker — unknown-date assertions never share a
  cache premise with dated ones, and a handle-date fallback can never
  silently shift bucket distributions.
  **A cache hit is exact only for clean, committed history.** HEAD pins
  git history, not the filesystem: if the target or citing path is dirty
  or untracked, the entry is bypassed and filesystem-dependent fields
  recomputed — or the row carries an explicit `evidence_dirty_worktree`
  degraded premise. Dirty evidence is never called exact. Referenced
  revisions are validated (`git cat-file -e`-class) and degrade honestly
  if GC removed them; detached HEAD is fine; force-push/rebase is fine
  (HEAD changes). Amortized cost: one incremental pass per repo commit,
  near-zero on unchanged repos. GC: entries bounded per HEAD/schema
  version; stale-HEAD entries pruned; a missing cache is simply the cold
  premise.
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

**The identity fix (review P0 — the id-collision smell confirmed real):**
markdown's external code-ref handles today use the raw citation path as
their id, which would collide with code-source file handles for the same
path in the same corpus — a CR-D41 violation the moment both sources load.
The fix: **external code-ref ids are markdown-qualified**
(`external:code:<stable citation identity>`), with the target path/line in
`*meta` (`external_class: "code"`, `target_path`, `target_line`). The
resolver joins **through the edge and the meta, never by treating a handle
id as a resolvable path**:

```
code_ref(spec, ref, path, code_handle, disposition) :=
  *edge{from: spec, to: ref, kind: "Cites", source: "markdown"},
  *handle{id: ref, kind: "external", source: "markdown"},
  *meta{handle: ref, key: "external_class", value: "code"},
  *meta{handle: ref, key: "target_path", value: path},
  *handle{id: code_handle, kind: "file", source: "code", file: f},
  path_resolves(path, f),
  referent_disposition(ref, disposition).
```

This preserves CR-R6 (unresolved edges stored) and CR-D41 (no id ever
doubles as another source's resolvable path). **Owned consequence: this is
an enumerated-intentional surface diff** — existing external handle ids
(visible today in W006 output and any query touching code-ref handles)
change shape; corpora without code refs stay byte-identical. The diff is
enumerated at the 4b gate, per the no-backward-compat stance.

- `path_resolves` = normalization only: line-suffix stripping
  (`foo.rs:63` → `foo.rs`), `./`, the corpus-root/repo-root offset (the
  probe's enclosing-repo logic is the precedent). File-level only, per the
  locked design; nearest-item hints are post-4b.
- Non-resolution is not failure — the unresolved ref's drift meta carries
  the story (gone/moved/unknown).

## 5. The drift surface (the CR-D105 rungs — what 4b is judged on)

Three rungs, no new verb, no new mode (subtractive: `handle` annotations +
`check` + `status` cover the need; `--drift` stays unspent):

1. **`status` arrival line** (only when code citations exist):
   `design→code refs: 76 intact · 74 drifted · 13 moved? · 6 gone` —
   aggregate-first per the arc spec; cold cache renders the PRE-FLIGHT
   premise line instead.
2. **`check` diagnostics**, disposition-typed with the severity split
   stated: `referent-gone` and `referent-moved-ambiguous` cited by an
   *operative* spec are **warnings**; plain `drifted(n)` is **currency
   evidence, not a correctness warning** — it lives in the status profile
   and handle annotations, entering diagnostics only via an explicit
   policy threshold. Never E-class (CR-D103: report drift, do not gate on
   ambiguous staleness). Corpus-scoped aggregate rows per CR-D69.
3. **`handle <spec>` citation annotations**, in place:
   `Cites crates/…/ids.rs  [intact · asserted 2026-06-06]` /
   `Cites src/cli.rs  [moved-ambiguous → 11 candidates]` — each teaching
   its follow-up query (`? assertion_drift("…", t, n).`), the v0.20
   pattern. **Cold cache gets a rung here too**: a spec page with code
   citations shows "drift evidence not built; run …" in the citation
   section — never silently unannotated (CR-D105 connects at every
   surface, not just aggregates).

**Refresh is explicit everywhere**: ordinary `status`/`check`/`handle`
*read* warm evidence and *report* cold evidence as a premise; **only the
explicit refresh path writes the cache**. No surface triggers hidden
extraction-time drift work because it wanted the data.

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

## Open questions — resolved at review
1. **Meta vs `*evidence`** → `*meta` for 4b, scalar/repeated-scalar only
   (one row per move candidate + count); the typed relation is earned the
   moment candidate tuples need queryable structure — the constraint is in
   §1 so the channel cannot erode.
2. **Cache format/GC** → history.jsonl-class envelope; entries bounded per
   HEAD/schema version; stale HEADs pruned; missing cache = cold premise.
   Not trails (evidence is corpus state, not session path).
3. **Refresh trigger** → explicit refresh path only writes the cache
   (`check --refresh-drift`-shaped; final flag at implementation); every
   consumer reads-or-reports, none triggers hidden work.
4. **W-code scope** → gone + moved-ambiguous on operative = warning;
   drifted(n) = currency evidence (status/handle), threshold-gated into
   diagnostics only by explicit policy.
5. **Evidence-HEAD premise** → per-target meta as proposed, plus the
   cache-level premise in the cache key; a corpus-level premise row can be
   derived, not stored twice.
