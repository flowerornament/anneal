# Search Ranker Known-Item Ablation — 2026-07-15

## Method

Corpus: `/Users/morgan/code/murail`

Cases: 32 source files; 64 queries (one exact and one morphology-preserving query per file).

Relevance oracle: proxy: source file is relevant for queries derived from its own summary. This is proxy relevance, not a human judgment set.
Each query uses three deterministic rare content terms from the source file summary.
The benchmark runs in-process against `SearchIndex`; CLI extraction and Datalog time are excluded.
The baseline ordering matched ordinary `SearchIndex` + `DefaultRanker` ordering for 32 spread queries.

## Results

| Lane | MRR | Delta | R@1 | Delta | R@5 | Delta | R@10 | Delta |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline | 0.6260 | +0.0000 | 50.00% | 0.00% | 81.25% | 0.00% | 84.38% | 0.00% |
| without_stemming | 0.4900 | -0.1361 | 34.38% | -15.62% | 70.31% | -10.94% | 76.56% | -7.81% |
| without_specificity | 0.6260 | +0.0000 | 50.00% | 0.00% | 81.25% | 0.00% | 84.38% | 0.00% |
| without_field_weights | 0.4521 | -0.1740 | 31.25% | -18.75% | 59.38% | -21.88% | 75.00% | -9.38% |
| without_phrase_ngrams | 0.8491 | +0.2231 | 78.12% | 28.12% | 96.88% | 15.62% | 96.88% | 12.50% |
| without_base_match_floor | 0.6967 | +0.0706 | 62.50% | 12.50% | 81.25% | 0.00% | 84.38% | 0.00% |
| without_abbreviation_expansion | 0.6260 | +0.0000 | 50.00% | 0.00% | 81.25% | 0.00% | 84.38% | 0.00% |

## Interpretation

A negative ablation delta is evidence that the removed heuristic helps this proxy task; a zero delta only says this corpus/query construction did not distinguish it.
These measurements do not justify tuning by themselves.

Run on demand with:

```bash
python3.13 scripts/benchmark-search-ranker.py /path/to/corpus
```
