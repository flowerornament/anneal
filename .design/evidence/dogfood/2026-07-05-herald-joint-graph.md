---
status: evidence
date: 2026-07-05
authors: [claude]
context: first real consumer session — anneal's joint design-code graph on herald
privacy: herald is a private corpus; row-level evidence stays local, aggregates only here
---

# Dogfooding the joint graph on herald — 2026-07-05

The first genuine consumer run of the v0.21 joint graph, on herald (Elixir,
364 design docs, ~1000 tracked source files, the named mass-staleness corpus).
Findings in the order they surfaced.

## Finding 1 — P0, FOUND AND FIXED IN-SESSION: file-level scan walked the whole tree

`source code { source_root("..") }` timed out (>120s) on herald. Cause: the
file-level scan skipped only VCS/build dirs and recursed everything else,
filtering by extension at the leaf — so it walked herald's **7.9G `priv/`**
(plus assets/, packages/) reading directory entries it would never use.

**Fix (shipped this session):** the scan now walks **git-tracked files**
(`git ls-files --cached --exclude-standard`) when the base is a git repo,
falling back to the filesystem walk otherwise. Tracked files ARE the source,
the drift oracle already reasons over git, and gitignored assets/build output
vanish for free — no new config knob. Result: status went **>120s → 33.8s**;
the code side scans ~1000 tracked files instead of drowning in priv/.
This is the dogfood loop working: a real repo exposed a bug the self-corpus
(small, no giant asset dirs) never could.

## Finding 2 — status latency on a large joint corpus (relates to eygi)

Even after the fix, `anneal status` on the herald joint graph is **~34s**
(2479 markdown handles + ~1000 code handles + all derived predicates). Usable
but not snappy. This is the eygi perf lever (context/status env sharing)
becoming pressing at real joint-corpus scale, not a new problem.

## Finding 3 — first-build drift refresh cost is real (design validated, cost noted)

`check --refresh-drift` on herald's **1,580 citations** takes minutes on the
cold cache (move detection walks history for gone paths). The explicit-refresh
design is VINDICATED — you would never want this inline in status — but the
first-build wait on a big corpus is a multi-minute cost. The HEAD-move
migration (shipped) means it only pays this once, then incrementally; still,
first-run ergonomics on a large corpus deserve a progress signal.

## Finding 4 — THE PAYOFF: it works, and the mass-staleness thesis holds on a 2nd corpus

Aggregate disposition profile over herald's 1,565 resolved citations
(row-level kept local):

| disposition | count | share |
|---|---|---|
| referent-moved → head | 896 | 57% |
| referent-drifted(n) | 282 | 18% |
| referent-unknown | 171 | 11% |
| referent-gone | 144 | 9% |
| referent-intact | 60 | **3.8%** |
| referent-moved-ambiguous | 12 | 1% |

- **3.8% intact** — herald's design is almost entirely stale relative to its
  code, exactly what the keystone constraint (2026-06-09-code-and-corpus.md §2)
  predicted. Proven on a second, independent corpus.
- **Moved dominates (57%)** — herald's code was reorganized heavily; most
  citations point at renamed paths. A naive `git log <path> --since` would have
  mislabeled all 896 as *gone*. The move-aware oracle is what turns an
  alarmist wall-of-red into an actionable "the module moved here."

**The product moment, live:** a real architecture spec cited a module at its
old path; `anneal handle <spec>` annotated the citation in place with
`[referent-moved · moved to <new path>]` and taught the follow-up query. That
is the whole arc's thesis — trust a spec by seeing how its code drifted —
working on a real corpus on the first consumer run.

## Verdict: prove-it succeeded; the frontier it revealed is PERF

The joint graph delivers real value on a real corpus. The gap between "works"
and "usable daily" is entirely performance:
- status ~34s at joint scale (eygi).
- first drift refresh 5:21 on 1,565 refs — move detection over gone/moved
  paths (1,040 of them) is the cost driver; each does a history walk. Bounding
  it (cap candidates harder, parallelize, or make the walk incremental per
  path-set) is the concrete next lever, alongside a progress signal.

Next arc is now evidence-backed: **perf, not new features.**

## Finding 5 — the perf frontier, PROFILED and FIXED (the arc's first win)

Opened the perf arc the disciplined way — measured before designing — and it
paid off twice by correcting the assumption:

1. **Extraction is cheap; eval is the cost.** Trivial-eval on the joint corpus
   = 0.9s vs status = 34s. The code-as-corpus scan I worried about adds 0.33s.
   The frontier is *eval*, not the adapter.
2. **But the real hotspot wasn't eval either — it was git-spawn.** `sample` on
   herald status showed `__wait4`/`__posix_spawn` dominating:
   `CodeDriftEvidenceCache::open` validated each of 1,565 cached entries with
   its own `git cat-file -e` (~3,130 spawns per warm status). NOT the assumed
   `eygi` DB-clone lever — that's a real but smaller murail-scale concern.
3. **Fix (shipped):** validate each *distinct* revision once via a memo
   (herald: ~3,130 spawns → a handful). **herald status 34–53s → 5.9s
   (~6–9×)**; posix_spawn profile samples 1,888 → 13. Byte-identical by
   construction.

Lesson, again: measure first. The two most confident pre-profiling guesses
(the code scan; the eygi eval lever) were both wrong about the herald
bottleneck; a five-minute profile named the actual line.

## Net: one dogfood session, three shipped fixes

- git-tracked scan (>120s → 34s at extraction) — c0672f1
- revision-dedup drift-cache load (34–53s → 5.9s at status) — 1dc1899
- and the validation that the joint graph *works* on a real corpus.

Remaining perf: the cold `--refresh-drift` (5:21, move detection — anneal-vs33)
and general eval scaling (eygi, smaller now). But the daily warm loop — status,
handle — is now ~6s on a real large corpus, from ~40s.
