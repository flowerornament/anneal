# 903i oracle audit summary: self-corpus

- corpus label: `self-corpus`
- git root name: `anneal`
- citation rows: 110
- clean dispositions: 76/110 (69.1%)
- resolver hit-rate: 76/110 (69.1%)
- median per-edge cost: 29.465ms
- p95 per-edge cost: 116.091ms
- split canary: PASS referent-moved-ambiguous fanout=[11]

## Drift buckets

| bucket | rows |
| --- | --- |
| referent-moved-ambiguous | 13 |
| referent-drifted(82) | 9 |
| referent-gone | 6 |
| referent-moved -> head | 8 |
| referent-drifted(10) | 1 |
| referent-unknown | 7 |
| referent-drifted(60) | 1 |
| referent-drifted(52) | 2 |
| referent-drifted(6) | 3 |
| referent-drifted(4) | 14 |
| referent-drifted(2) | 7 |
| referent-drifted(21) | 1 |
| referent-drifted(19) | 33 |
| referent-drifted(1) | 3 |
| referent-intact | 2 |

## Blame lie-rate inputs

- blame coverage: 110/110 (100.0%)
- handle-date fallback: 0/110 (0.0%)
- unknown assertion date: 0/110 (0.0%)
- blame-vs-handle divergence: 10/110 (9.1%)
- disposition delta if handle date used: 1/110 (0.9%)

### Suspicious bulk-date clusters

| date | edge rows | source rows |
| --- | --- | --- |
| 2026-06-06 | 53 | 53 |

## Reverse-edge scan

- CR label references in code/docs: 9
- unique CR labels: 5
- files with CR labels: 4

Top labels:
| label | count |
| --- | --- |
| CR-D | 4 |
| CR-R5 | 2 |
| CR-R6 | 1 |
| CR-D31 | 1 |
| CR-D104 | 1 |

Top files:
| file | count |
| --- | --- |
| crates/anneal-md/src/extract/config.rs | 4 |
| crates/anneal-md/src/extract/adapter.rs | 2 |
| crates/anneal-cli/src/context.rs | 2 |
| crates/anneal-core/src/runtime/introspection.rs | 1 |
