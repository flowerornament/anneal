use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::benchmark_ablation::{self, Ablation};
use super::{
    DEFAULT_LOW_CONFIDENCE_THRESHOLD, DefaultRanker, RankingContext, SearchHandleDocument,
    SearchIndex, SearchQuery, rank_search_hits, search_tokens,
};
use crate::retrieval::SearchSpanScope;

const FIXTURE_ENV: &str = "ANNEAL_RANK_BENCH_FIXTURE";
const OUTPUT_ENV: &str = "ANNEAL_RANK_BENCH_OUTPUT";
const EQUIVALENCE_CASES: usize = 16;
const BENCHMARK_CASES: usize = 32;

#[derive(Debug, Deserialize)]
struct Fixture {
    corpus_root: String,
    handles: Vec<HandleRow>,
    edges: Vec<EdgeRow>,
    meta: Vec<MetaRow>,
    config: Vec<ConfigRow>,
    content: Vec<ContentRow>,
    spans: Vec<SpanRow>,
}

#[derive(Debug, Deserialize)]
struct HandleRow {
    corpus: String,
    source: String,
    handle: String,
    file: String,
    summary: Option<String>,
    status: Option<String>,
    namespace: Option<String>,
    area: Option<String>,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EdgeRow {
    corpus: String,
    source: String,
    from: String,
    to: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct MetaRow {
    corpus: String,
    source: String,
    handle: String,
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ContentRow {
    corpus: String,
    source: String,
    handle: String,
    span_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct SpanRow {
    corpus: String,
    source: String,
    handle: String,
    span_id: String,
    summary: String,
}

impl Fixture {
    fn build_index(&self) -> SearchIndex {
        let mut index = SearchIndex::default();
        for row in &self.handles {
            index.insert_handle(SearchHandleDocument {
                corpus: row.corpus.as_str(),
                source: row.source.as_str(),
                handle: row.handle.as_str(),
                file: row.file.as_str(),
                summary: row.summary.as_deref(),
                status: row.status.as_deref(),
                namespace: row.namespace.as_deref(),
                area: row.area.as_deref(),
                kind: row.kind.as_deref(),
            });
        }
        for row in &self.edges {
            index.insert_edge(
                row.corpus.as_str(),
                row.source.as_str(),
                row.from.as_str(),
                row.to.as_str(),
                row.kind.as_str(),
            );
        }
        for row in &self.meta {
            index.insert_meta(
                row.corpus.as_str(),
                row.source.as_str(),
                row.handle.as_str(),
                row.key.as_str(),
                row.value.as_str(),
            );
        }
        for row in &self.config {
            index.insert_config(row.key.as_str(), row.value.as_str());
        }
        for row in &self.content {
            index.insert_content(
                row.corpus.as_str(),
                row.source.as_str(),
                row.handle.as_str(),
                row.span_id.as_str(),
                row.text.as_str(),
            );
        }
        for row in &self.spans {
            index.insert_span_summary(
                row.corpus.as_str(),
                row.source.as_str(),
                row.handle.as_str(),
                row.span_id.as_str(),
                row.summary.as_str(),
            );
        }
        index
    }
}

#[derive(Clone, Debug)]
struct BenchmarkCase {
    expected: String,
    exact_query: String,
    morphology_query: String,
}

impl BenchmarkCase {
    fn queries(&self) -> [&str; 2] {
        [self.exact_query.as_str(), self.morphology_query.as_str()]
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct Metrics {
    mrr: f64,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
}

#[derive(Debug, Serialize)]
struct LaneResult {
    lane: &'static str,
    metrics: Metrics,
    delta_from_baseline: Metrics,
}

#[derive(Debug, Serialize)]
struct BenchmarkResult {
    corpus_root: String,
    relevance: &'static str,
    cases: usize,
    queries: usize,
    baseline_equivalence_cases: usize,
    lanes: Vec<LaneResult>,
}

#[test]
#[ignore = "on-demand real-corpus ranker benchmark; run scripts/benchmark-search-ranker.py"]
fn known_item_ranker_benchmark() {
    let fixture_path = env::var_os(FIXTURE_ENV).expect("benchmark fixture path is set");
    let output_path = env::var_os(OUTPUT_ENV).expect("benchmark output path is set");
    let fixture = load_fixture(Path::new(&fixture_path));
    let cases = spread_sample(build_cases(&fixture.handles), BENCHMARK_CASES);
    assert!(
        cases.len() >= EQUIVALENCE_CASES,
        "benchmark needs a spread of cases"
    );

    assert_baseline_matches_ordinary_search(&fixture, &cases);

    let lanes = [
        ("baseline", Ablation::Baseline),
        ("without_stemming", Ablation::Stemming),
        ("without_specificity", Ablation::Specificity),
        ("without_field_weights", Ablation::FieldWeights),
        ("without_phrase_ngrams", Ablation::PhraseNgrams),
        ("without_base_match_floor", Ablation::BaseMatchFloor),
        (
            "without_abbreviation_expansion",
            Ablation::AbbreviationExpansion,
        ),
    ];
    let measured = lanes
        .into_iter()
        .map(|(name, ablation)| (name, measure_lane(&fixture, &cases, ablation)))
        .collect::<Vec<_>>();
    let baseline = measured[0].1;
    let lanes = measured
        .into_iter()
        .map(|(lane, metrics)| LaneResult {
            lane,
            metrics,
            delta_from_baseline: metric_delta(metrics, baseline),
        })
        .collect();
    let result = BenchmarkResult {
        corpus_root: fixture.corpus_root,
        relevance: "proxy: source file is relevant for queries derived from its own summary",
        cases: cases.len(),
        queries: cases.len() * 2,
        baseline_equivalence_cases: EQUIVALENCE_CASES * 2,
        lanes,
    };
    let encoded = serde_json::to_vec_pretty(&result).expect("benchmark result serializes");
    fs::write(output_path, encoded).expect("benchmark result is written");
}

fn load_fixture(path: &Path) -> Fixture {
    let bytes = fs::read(path).expect("benchmark fixture is readable");
    serde_json::from_slice(&bytes).expect("benchmark fixture has the expected schema")
}

fn build_cases(handles: &[HandleRow]) -> Vec<BenchmarkCase> {
    let summaries = handles
        .iter()
        .filter(|row| row.kind.as_deref() == Some("file"))
        .filter_map(|row| row.summary.as_deref())
        .map(search_tokens)
        .collect::<Vec<_>>();
    let mut frequencies = BTreeMap::<String, usize>::new();
    for terms in &summaries {
        for term in terms
            .iter()
            .filter(|term| eligible_term(term))
            .collect::<BTreeSet<_>>()
        {
            *frequencies.entry((*term).clone()).or_default() += 1;
        }
    }

    handles
        .iter()
        .filter(|row| row.kind.as_deref() == Some("file"))
        .filter_map(|row| {
            let summary = row.summary.as_deref()?;
            let terms = search_tokens(summary);
            let mut candidates = terms
                .iter()
                .enumerate()
                .filter(|(_, term)| eligible_term(term))
                .map(|(position, term)| {
                    (
                        frequencies.get(term).copied().unwrap_or(usize::MAX),
                        position,
                        term.clone(),
                    )
                })
                .collect::<Vec<_>>();
            candidates.sort();
            candidates.truncate(3);
            if candidates.len() != 3 {
                return None;
            }
            candidates.sort_by_key(|(_, position, _)| *position);
            let exact_terms = candidates
                .into_iter()
                .map(|(_, _, term)| term)
                .collect::<Vec<_>>();
            let morphology_terms = morphology_variant(&exact_terms)?;
            Some(BenchmarkCase {
                expected: row.file.clone(),
                exact_query: exact_terms.join(" "),
                morphology_query: morphology_terms.join(" "),
            })
        })
        .collect()
}

fn eligible_term(term: &str) -> bool {
    term.len() >= 4
        && !matches!(
            term,
            "about"
                | "after"
                | "also"
                | "been"
                | "before"
                | "being"
                | "between"
                | "from"
                | "have"
                | "into"
                | "more"
                | "only"
                | "other"
                | "should"
                | "than"
                | "that"
                | "their"
                | "these"
                | "this"
                | "through"
                | "using"
                | "which"
                | "with"
        )
}

fn morphology_variant(terms: &[String]) -> Option<Vec<String>> {
    let mut variant = terms.to_vec();
    let term = variant
        .iter_mut()
        .find(|term| term.len() > 3 && !term.ends_with('s'))?;
    if term.ends_with('y') && term.len() > 4 {
        term.truncate(term.len() - 1);
        term.push_str("ies");
    } else {
        term.push('s');
    }
    Some(variant)
}

fn spread_sample(cases: Vec<BenchmarkCase>, limit: usize) -> Vec<BenchmarkCase> {
    if cases.len() <= limit {
        return cases;
    }
    let last = cases.len() - 1;
    (0..limit)
        .map(|index| cases[index * last / (limit - 1)].clone())
        .collect()
}

fn assert_baseline_matches_ordinary_search(fixture: &Fixture, cases: &[BenchmarkCase]) {
    let ordinary_index = fixture.build_index();
    let benchmark_index = benchmark_ablation::with(Ablation::Baseline, || fixture.build_index());
    let sample_step = (cases.len() / EQUIVALENCE_CASES).max(1);
    for case in cases.iter().step_by(sample_step).take(EQUIVALENCE_CASES) {
        for query in case.queries() {
            let ordinary = ranked_handles(&ordinary_index, query);
            let benchmark = benchmark_ablation::with(Ablation::Baseline, || {
                ranked_handles(&benchmark_index, query)
            });
            assert_eq!(
                benchmark, ordinary,
                "benchmark baseline must be the ordinary ranker for query {query:?}"
            );
        }
    }
}

fn measure_lane(fixture: &Fixture, cases: &[BenchmarkCase], ablation: Ablation) -> Metrics {
    benchmark_ablation::with(ablation, || {
        let index = fixture.build_index();
        let ranks = cases.iter().flat_map(|case| {
            case.queries().map(|query| {
                ranked_handles(&index, query)
                    .iter()
                    .position(|handle| handle == &case.expected)
                    .map(|position| position + 1)
            })
        });
        metrics(ranks)
    })
}

fn ranked_handles(index: &SearchIndex, query: &str) -> Vec<String> {
    let Some(parsed) = SearchQuery::parse(query) else {
        return Vec::new();
    };
    let hits = index.search_hits(&parsed, None, SearchSpanScope::Any, None, None);
    let ranked = rank_search_hits(
        hits,
        &RankingContext::new(query, DEFAULT_LOW_CONFIDENCE_THRESHOLD),
        &DefaultRanker,
    );
    let mut seen = BTreeSet::new();
    ranked
        .into_iter()
        .filter_map(|hit| {
            let handle = hit.hit().handle().to_owned();
            seen.insert(handle.clone()).then_some(handle)
        })
        .collect()
}

fn metrics(ranks: impl Iterator<Item = Option<usize>>) -> Metrics {
    let ranks = ranks.collect::<Vec<_>>();
    let count = benchmark_count(ranks.len());
    Metrics {
        mrr: ranks
            .iter()
            .filter_map(|rank| rank.map(|rank| 1.0 / benchmark_count(rank)))
            .sum::<f64>()
            / count,
        recall_at_1: recall_at(&ranks, 1),
        recall_at_5: recall_at(&ranks, 5),
        recall_at_10: recall_at(&ranks, 10),
    }
}

fn recall_at(ranks: &[Option<usize>], limit: usize) -> f64 {
    let recalled = ranks
        .iter()
        .filter(|rank| rank.is_some_and(|rank| rank <= limit))
        .count();
    benchmark_count(recalled) / benchmark_count(ranks.len())
}

fn benchmark_count(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("benchmark counts fit in u32"))
}

fn metric_delta(metrics: Metrics, baseline: Metrics) -> Metrics {
    Metrics {
        mrr: metrics.mrr - baseline.mrr,
        recall_at_1: metrics.recall_at_1 - baseline.recall_at_1,
        recall_at_5: metrics.recall_at_5 - baseline.recall_at_5,
        recall_at_10: metrics.recall_at_10 - baseline.recall_at_10,
    }
}
