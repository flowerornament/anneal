#!/usr/bin/env python3.13
"""Run the on-demand known-item search-ranker ablation benchmark."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = REPO_ROOT / ".design/evidence/2026-07-15-search-ranker-ablation.md"
QUERIES = {
    "handles": (
        "? *handle{corpus:corpus, source:source, id:handle, file:file, "
        "summary:summary, status:status, namespace:namespace, area:area, kind:kind}."
    ),
    "edges": "? *edge{corpus:corpus, source:source, from:from, to:to, kind:kind}.",
    "meta": (
        "? *meta{corpus:corpus, source:source, handle:handle, key:key, value:value}."
    ),
    "config": "? *config{key:key, value:value}.",
    "content": (
        "? *content{corpus:corpus, source:source, handle:handle, "
        "span_id:span_id, text:text}."
    ),
    "spans": (
        "? *span{corpus:corpus, source:source, handle:handle, "
        "id:span_id, summary:summary}."
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Benchmark anneal's private search ranker with known-item retrieval."
    )
    parser.add_argument("corpus", type=Path, help="corpus root used as benchmark evidence")
    parser.add_argument(
        "--anneal",
        type=Path,
        default=REPO_ROOT / "target/debug/anneal",
        help="anneal binary used to extract the temporary fixture",
    )
    parser.add_argument(
        "--output", type=Path, default=DEFAULT_OUTPUT, help="Markdown evidence path"
    )
    return parser.parse_args()


def run_query(anneal: Path, corpus: Path, query: str) -> list[dict[str, Any]]:
    command = [
        str(anneal),
        "-e",
        query,
        "--format=ndjson",
        "--root",
        str(corpus),
    ]
    completed = subprocess.run(
        command,
        check=True,
        capture_output=True,
        text=True,
        timeout=600,
    )
    return [json.loads(line) for line in completed.stdout.splitlines() if line]


def write_fixture(path: Path, anneal: Path, corpus: Path) -> None:
    fixture: dict[str, Any] = {"corpus_root": str(corpus.resolve())}
    for name, query in QUERIES.items():
        print(f"extracting {name}...", file=sys.stderr, flush=True)
        fixture[name] = run_query(anneal, corpus, query)
    path.write_text(json.dumps(fixture, separators=(",", ":")), encoding="utf-8")


def run_benchmark(fixture: Path, output: Path) -> dict[str, Any]:
    env = os.environ.copy()
    env["ANNEAL_RANK_BENCH_FIXTURE"] = str(fixture)
    env["ANNEAL_RANK_BENCH_OUTPUT"] = str(output)
    subprocess.run(
        [
            "cargo",
            "test",
            "--release",
            "-p",
            "anneal-core",
            "known_item_ranker_benchmark",
            "--",
            "--ignored",
            "--nocapture",
        ],
        cwd=REPO_ROOT,
        env=env,
        check=True,
        timeout=1200,
    )
    return json.loads(output.read_text(encoding="utf-8"))


def percentage(value: float) -> str:
    return f"{value * 100:.2f}%"


def render_markdown(result: dict[str, Any]) -> str:
    lines = [
        "# Search Ranker Known-Item Ablation — 2026-07-15",
        "",
        "## Method",
        "",
        f"Corpus: `{result['corpus_root']}`",
        "",
        f"Cases: {result['cases']} source files; {result['queries']} queries "
        "(one exact and one morphology-preserving query per file).",
        "",
        f"Relevance oracle: {result['relevance']}. This is proxy relevance, not a human judgment set.",
        "Each query uses three deterministic rare content terms from the source file summary.",
        "The benchmark runs in-process against `SearchIndex`; CLI extraction and Datalog time are excluded.",
        f"The baseline ordering matched ordinary `SearchIndex` + `DefaultRanker` ordering for "
        f"{result['baseline_equivalence_cases']} spread queries.",
        "",
        "## Results",
        "",
        "| Lane | MRR | Delta | R@1 | Delta | R@5 | Delta | R@10 | Delta |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for lane in result["lanes"]:
        metrics = lane["metrics"]
        delta = lane["delta_from_baseline"]
        lines.append(
            "| {lane} | {mrr:.4f} | {dmrr:+.4f} | {r1} | {dr1} | {r5} | {dr5} | {r10} | {dr10} |".format(
                lane=lane["lane"],
                mrr=metrics["mrr"],
                dmrr=delta["mrr"],
                r1=percentage(metrics["recall_at_1"]),
                dr1=percentage(delta["recall_at_1"]),
                r5=percentage(metrics["recall_at_5"]),
                dr5=percentage(delta["recall_at_5"]),
                r10=percentage(metrics["recall_at_10"]),
                dr10=percentage(delta["recall_at_10"]),
            )
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "A negative ablation delta is evidence that the removed heuristic helps this proxy task; "
            "a zero delta only says this corpus/query construction did not distinguish it.",
            "These measurements do not justify tuning by themselves.",
            "",
            "Run on demand with:",
            "",
            "```bash",
            "python3.13 scripts/benchmark-search-ranker.py /path/to/corpus",
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    corpus = args.corpus.resolve()
    anneal = args.anneal.resolve()
    if not corpus.is_dir():
        raise SystemExit(f"error: corpus is not a directory: {corpus}")
    if not anneal.is_file():
        raise SystemExit(f"error: anneal binary not found: {anneal}")

    with tempfile.TemporaryDirectory(prefix="anneal-rank-bench-") as temp_dir:
        temp = Path(temp_dir)
        fixture = temp / "fixture.json"
        result_path = temp / "result.json"
        write_fixture(fixture, anneal, corpus)
        result = run_benchmark(fixture, result_path)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(render_markdown(result), encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
