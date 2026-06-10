---
status: evidence
date: 2026-06-10
authors: [codex]
bd: anneal-903i
relates:
  - 2026-06-09-code-and-corpus.md
  - 2026-06-08-currency.md
---

# 903i oracle audit: spec-to-code referent drift — 2026-06-10

This is the evidence pass for `anneal-903i`. It runs out of band: anneal
emits the existing `Cites` edges to external code handles, and
`scripts/audit-code-citation-drift.py` materializes git/blame/history
evidence into artifacts. No eval-time git, no prelude changes, no product
surface changes.

Artifacts:

- Self-corpus rows: `.design/evidence/903i/self-corpus-rows.jsonl`
- Self-corpus summary: `.design/evidence/903i/self-corpus-summary.md`
- External-sample rows: local only, `/tmp/anneal-903i-external/`

The live row counts are higher than the scoping estimates: 110 self-corpus
rows and 913 external-sample rows.

## Stop-rule verdicts

**Move honesty: PASS.** Missing historical paths are not collapsed into clean
successors. The oracle emits degraded dispositions (`referent-moved-ambiguous`,
`referent-gone`, `referent-unknown`) instead of laundering them into intact
or drifted.

**Split canary: PASS.** The self-corpus `src/cli.rs` citations, split by
commit `328408b` into `src/cli/*.rs`, classify as
`referent-moved-ambiguous` with fanout 11. This is the required answer:
there is no clean head route through a file split.

## Self-corpus findings

- Citation rows: 110
- Exact clean dispositions: 76/110 (69.1%)
- Resolver hit-rate: 76/110 (69.1%)
- Blame coverage: 110/110 (100.0%)
- Blame-vs-handle-date divergence: 10/110 (9.1%)
- Disposition delta if handle date is used instead: 1/110 (0.9%)
- Median per-edge cost: 29.465ms
- p95 per-edge cost: 116.091ms

Grouped dispositions:

| group | rows |
| --- | ---: |
| referent-intact | 2 |
| referent-drifted | 74 |
| referent-moved -> head | 8 |
| referent-moved-ambiguous | 13 |
| referent-gone | 6 |
| referent-unknown | 7 |

Reverse-edge scan: 9 CR label references in source-like files, across 4 files
and 5 unique labels. This is real but sparse; it is not enough to replace the
forward citation audit.

## External-sample findings

The external sample's row-level artifact stays local. Aggregate numbers are
safe to commit:

- Citation rows: 913
- Exact clean dispositions: 838/913 (91.8%)
- Resolver hit-rate: 838/913 (91.8%)
- Blame coverage: 913/913 (100.0%)
- Blame-vs-handle-date divergence: 486/913 (53.2%)
- Disposition delta if handle date is used instead: 381/913 (41.7%)
- Median per-edge cost: 27.626ms
- p95 per-edge cost: 79.054ms

Grouped dispositions:

| group | rows |
| --- | ---: |
| referent-intact | 113 |
| referent-drifted | 725 |
| referent-moved -> head | 11 |
| referent-gone | 2 |
| referent-unknown | 62 |

There were no source-like CR label references in the external sample's reverse
scan.

## Recommendation

The oracle earns productization for the first product slice, with the authority
boundary from the design intact:

- `referent-intact` and `referent-drifted(n)` are the only clean exact-path
  dispositions.
- `referent-moved -> head`, `referent-moved-ambiguous`, `referent-gone`, and
  `referent-unknown` stay REPORT/UNKNOWN. They may teach follow-up queries,
  but they must not become gate-clean evidence.
- Git rename/split evidence is referent-currency evidence, not author-declared
  `Supersedes`; product language should keep saying "referent moved/drifted,"
  never "successor" or "superseded" for path movement.

The `*edge` nullable `date` + `revision` amendment earns CR-D8. Blame is
available on both samples, the measured cost is acceptable for out-of-band
materialization, and the external-sample handle-date substitution would change
41.7% of dispositions. Handle date is not a safe assertion-time oracle.

The surprise is positive: the hostile sample is not mostly unresolvable. It is
mostly exact-path drifted, with a small but important degraded tail. That means
the first product surface should lead with aggregate drift profiles and exact
drift counts, then make moved/gone/unknown rows explorable rather than blocking
the arc on perfect move reconstruction.
