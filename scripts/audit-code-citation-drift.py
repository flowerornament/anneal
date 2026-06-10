#!/usr/bin/env python3
"""Audit spec-to-code citation drift out of band.

The audit consumes anneal's existing markdown facts, then asks git about the
referenced code paths. It intentionally does not add product runtime behavior:
git evidence is materialized as an artifact for design review.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import subprocess
import time
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any


QUERY = r'''? *edge{from: src, to: ref, kind: "Cites", file: file, line: line},
  *handle{id: src, status: status, date: date},
  *handle{id: ref, kind: "external"},
  *meta{handle: ref, key: "external_class", value: "code"},
  *meta{handle: ref, key: "target_path", value: path}.'''

ROW_FIELDS = [
    "citation_id",
    "source_handle",
    "source_status",
    "source_date",
    "edge_file",
    "edge_line",
    "edge_date",
    "edge_revision",
    "date_source",
    "target_path",
    "target_exists_now",
    "target_history_status",
    "commits_since_assertion",
    "rename_candidates",
    "split_fanout",
    "resolved_head",
    "resolver_hit",
    "final_disposition",
    "cost_ms",
]

CR_LABEL_RE = re.compile(r"\bCR-[A-Za-z][A-Za-z0-9-]*\b")
SOURCE_EXTENSIONS = {
    ".rs",
    ".py",
    ".sh",
    ".nix",
    ".toml",
    ".yaml",
    ".yml",
    ".js",
    ".jsx",
    ".ts",
    ".tsx",
}


@dataclass(frozen=True)
class Citation:
    source_handle: str
    source_status: str
    source_date: str
    edge_file: str
    edge_line: int
    ref_handle: str
    target_path: str


@dataclass(frozen=True)
class BlameInfo:
    date: str
    revision: str
    source: str


@dataclass(frozen=True)
class MoveInfo:
    candidates: tuple[str, ...]
    split_fanout: int


class AuditError(RuntimeError):
    """Raised when the audit cannot run."""


class Git:
    def __init__(self, root: Path) -> None:
        self.root = root
        self._history: dict[str, str] = {}
        self._exists: dict[str, bool] = {}
        self._commits_since: dict[tuple[str, str], int] = {}
        self._blame: dict[tuple[str, int], BlameInfo] = {}
        self._moves: dict[str, MoveInfo] = {}

    def run(self, args: list[str], *, check: bool = False) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", "-C", str(self.root), *args],
            check=check,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def exists_now(self, rel_path: str) -> bool:
        if rel_path not in self._exists:
            self._exists[rel_path] = (self.root / rel_path).is_file()
        return self._exists[rel_path]

    def history_status(self, rel_path: str) -> str:
        if rel_path not in self._history:
            proc = self.run(["log", "--all", "--format=%H", "--max-count=1", "--", rel_path])
            if proc.returncode != 0:
                self._history[rel_path] = "unavailable"
            elif proc.stdout.strip():
                self._history[rel_path] = "present"
            else:
                self._history[rel_path] = "absent"
        return self._history[rel_path]

    def commits_since(self, rel_path: str, date: str) -> int:
        key = (rel_path, date)
        if key not in self._commits_since:
            proc = self.run(["log", "--all", "--format=%H", f"--since={date}", "--", rel_path])
            self._commits_since[key] = len(nonempty_lines(proc.stdout)) if proc.returncode == 0 else 0
        return self._commits_since[key]

    def blame_line(self, rel_path: str, line: int) -> BlameInfo | None:
        key = (rel_path, line)
        if key in self._blame:
            info = self._blame[key]
            return None if info.source == "missing" else info

        proc = self.run(["blame", "-w", "-M", "-C", "-L", f"{line},{line}", "--", rel_path])
        if proc.returncode != 0 or not proc.stdout.strip():
            self._blame[key] = BlameInfo("", "", "missing")
            return None
        revision = proc.stdout.split(maxsplit=1)[0].lstrip("^")
        date_proc = self.run(["show", "-s", "--format=%cI", revision])
        if date_proc.returncode != 0 or not date_proc.stdout.strip():
            self._blame[key] = BlameInfo("", "", "missing")
            return None
        info = BlameInfo(date_proc.stdout.strip()[:10], revision, "blame")
        self._blame[key] = info
        return info

    def move_info(self, rel_path: str) -> MoveInfo:
        if rel_path in self._moves:
            return self._moves[rel_path]

        proc = self.run(["log", "--all", "--format=%H", "--diff-filter=D", "--", rel_path])
        candidates: set[str] = set()
        for commit in nonempty_lines(proc.stdout):
            show = self.run(["show", "--name-status", "--find-renames", "--find-copies", "--format=", commit])
            if show.returncode != 0:
                continue
            deleted_in_commit = False
            added_paths: list[str] = []
            for line in nonempty_lines(show.stdout):
                fields = line.split("\t")
                code = fields[0]
                if code == "D" and len(fields) >= 2 and fields[1] == rel_path:
                    deleted_in_commit = True
                elif (code.startswith("R") or code.startswith("C")) and len(fields) >= 3 and fields[1] == rel_path:
                    candidates.add(fields[2])
                elif code == "A" and len(fields) >= 2:
                    added_paths.append(fields[1])
            if deleted_in_commit and not candidates:
                candidates.update(path for path in added_paths if looks_like_split_candidate(rel_path, path))

        ordered = tuple(sorted(candidates))
        info = MoveInfo(ordered, len(ordered) if len(ordered) > 1 else 0)
        self._moves[rel_path] = info
        return info


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path, help="Markdown corpus root to audit")
    parser.add_argument("--label", required=True, help="Neutral label used in artifact summaries")
    parser.add_argument("--output-dir", required=True, type=Path, help="Directory for audit artifacts")
    parser.add_argument("--anneal-bin", default="./target/debug/anneal", type=Path)
    parser.add_argument("--summary", type=Path, help="Optional markdown summary path")
    args = parser.parse_args()

    root = args.root.resolve()
    repo_root = git_root(root)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    citations = load_citations(args.anneal_bin, root)
    git = Git(repo_root)
    rows = [audit_citation(args.label, root, repo_root, git, citation) for citation in citations]
    reverse_scan = scan_reverse_edges(repo_root)
    summary = build_summary(args.label, root, repo_root, rows, reverse_scan)

    jsonl_path = args.output_dir / f"{args.label}-rows.jsonl"
    csv_path = args.output_dir / f"{args.label}-rows.csv"
    summary_path = args.summary or args.output_dir / f"{args.label}-summary.md"

    write_jsonl(jsonl_path, rows)
    write_csv(csv_path, rows)
    summary_path.write_text(summary, encoding="utf-8")

    print(f"rows: {len(rows)}")
    print(f"jsonl: {jsonl_path}")
    print(f"csv: {csv_path}")
    print(f"summary: {summary_path}")
    print(stop_rule_report(rows))
    return 0


def load_citations(anneal_bin: Path, root: Path) -> list[Citation]:
    proc = subprocess.run(
        [str(anneal_bin), "--root", str(root), "-e", QUERY, "--format=json"],
        check=False,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if proc.returncode != 0:
        raise AuditError(proc.stderr.strip() or "anneal query failed")
    citations: list[Citation] = []
    for line in nonempty_lines(proc.stdout):
        row = json.loads(line)
        citations.append(
            Citation(
                source_handle=str(row.get("src", "")),
                source_status=str(row.get("status", "")),
                source_date=str(row.get("date", "")),
                edge_file=str(row.get("file", "")),
                edge_line=int(row.get("line", 0)),
                ref_handle=str(row.get("ref", "")),
                target_path=str(row.get("path", "")),
            )
        )
    citations.sort(key=lambda item: (item.edge_file, item.edge_line, item.ref_handle, item.target_path))
    return citations


def audit_citation(label: str, corpus_root: Path, repo_root: Path, git: Git, citation: Citation) -> dict[str, Any]:
    start = time.perf_counter()
    rel_target = normalize_target_path(repo_root, corpus_root, citation.target_path)
    edge_rel = normalize_edge_file(repo_root, corpus_root, citation.edge_file)

    blame = git.blame_line(edge_rel, citation.edge_line) if edge_rel else None
    if blame is not None:
        edge_date = blame.date
        edge_revision = blame.revision
        date_source = "blame"
    elif citation.source_date:
        edge_date = citation.source_date
        edge_revision = ""
        date_source = "handle"
    else:
        edge_date = ""
        edge_revision = ""
        date_source = "unknown"

    if rel_target is None:
        exists_now = "unknown"
        history_status = "unavailable"
        commits_since: int | None = None
        move = MoveInfo((), 0)
        resolver_hit = False
        disposition = "referent-unknown"
    else:
        exists_now_bool = git.exists_now(rel_target)
        exists_now = str(exists_now_bool).lower()
        history_status = git.history_status(rel_target)
        commits_since = git.commits_since(rel_target, edge_date) if edge_date else None
        move = git.move_info(rel_target) if not exists_now_bool and history_status == "present" else MoveInfo((), 0)
        resolver_hit = exists_now_bool
        disposition = classify(exists_now_bool, history_status, commits_since, edge_date, move)

    cost_ms = (time.perf_counter() - start) * 1000
    return {
        "citation_id": citation_id(label, citation),
        "source_handle": citation.source_handle,
        "source_status": citation.source_status,
        "source_date": citation.source_date,
        "edge_file": citation.edge_file,
        "edge_line": citation.edge_line,
        "edge_date": edge_date,
        "edge_revision": edge_revision,
        "date_source": date_source,
        "target_path": rel_target if rel_target is not None else citation.target_path,
        "target_exists_now": exists_now,
        "target_history_status": history_status,
        "commits_since_assertion": commits_since,
        "rename_candidates": list(move.candidates),
        "split_fanout": move.split_fanout,
        "resolved_head": move.candidates[0] if len(move.candidates) == 1 else "",
        "resolver_hit": resolver_hit,
        "final_disposition": disposition,
        "cost_ms": round(cost_ms, 3),
    }


def classify(
    exists_now: bool,
    history_status: str,
    commits_since: int | None,
    assertion_date: str,
    move: MoveInfo,
) -> str:
    if exists_now:
        if not assertion_date:
            return "referent-present-undated"
        if commits_since == 0:
            return "referent-intact"
        return f"referent-drifted({commits_since})"
    if history_status == "present":
        if len(move.candidates) == 1:
            return "referent-moved -> head"
        if len(move.candidates) > 1:
            return "referent-moved-ambiguous"
        return "referent-gone"
    return "referent-unknown"


def normalize_target_path(repo_root: Path, corpus_root: Path, raw_path: str) -> str | None:
    if not raw_path:
        return None
    path = Path(raw_path)
    if path.is_absolute():
        try:
            return path.resolve(strict=False).relative_to(repo_root).as_posix()
        except ValueError:
            try:
                return path.resolve(strict=False).relative_to(corpus_root).as_posix()
            except ValueError:
                return None
    posix = Path(raw_path).as_posix()
    return posix[2:] if posix.startswith("./") else posix


def normalize_edge_file(repo_root: Path, corpus_root: Path, edge_file: str) -> str | None:
    if not edge_file:
        return None
    path = Path(edge_file)
    full = path if path.is_absolute() else corpus_root / path
    try:
        return full.resolve(strict=False).relative_to(repo_root).as_posix()
    except ValueError:
        return None


def looks_like_split_candidate(old_path: str, new_path: str) -> bool:
    old = Path(old_path)
    new = Path(new_path)
    stem = old.stem
    parent = old.parent.as_posix()
    new_posix = new.as_posix()
    return (
        new_posix.startswith(f"{parent}/{stem}/")
        or new_posix.startswith(f"{parent}/{stem}_")
        or new.stem.startswith(f"{stem}_")
    )


def citation_id(label: str, citation: Citation) -> str:
    payload = "|".join(
        [
            label,
            citation.source_handle,
            citation.edge_file,
            str(citation.edge_line),
            citation.ref_handle,
            citation.target_path,
        ]
    )
    return hashlib.sha256(payload.encode()).hexdigest()[:16]


def scan_reverse_edges(repo_root: Path) -> dict[str, Any]:
    labels: Counter[str] = Counter()
    file_counts: Counter[str] = Counter()
    extensions: Counter[str] = Counter()
    for path in repo_root.rglob("*"):
        if should_skip_scan_path(path):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        found = CR_LABEL_RE.findall(text)
        if not found:
            continue
        rel = path.relative_to(repo_root).as_posix()
        file_counts[rel] += len(found)
        extensions[path.suffix or "<none>"] += len(found)
        labels.update(found)
    return {
        "label_references": sum(labels.values()),
        "unique_labels": len(labels),
        "files": len(file_counts),
        "top_labels": labels.most_common(10),
        "top_files": file_counts.most_common(10),
        "extensions": extensions.most_common(),
    }


def should_skip_scan_path(path: Path) -> bool:
    parts = set(path.parts)
    return (
        not path.is_file()
        or ".git" in parts
        or ".beads" in parts
        or "target" in parts
        or ".jj" in parts
        or path.suffix not in SOURCE_EXTENSIONS
        or path.suffix in {".png", ".jpg", ".jpeg", ".gif", ".pdf", ".lock"}
    )


def build_summary(label: str, root: Path, repo_root: Path, rows: list[dict[str, Any]], reverse_scan: dict[str, Any]) -> str:
    distribution = Counter(row["final_disposition"] for row in rows)
    date_sources = Counter(row["date_source"] for row in rows)
    edge_dates = Counter(row["edge_date"] for row in rows if row["edge_date"])
    source_dates = Counter(row["source_date"] for row in rows if row["source_date"])
    resolver_hits = sum(1 for row in rows if row["resolver_hit"])
    handle_deltas = disposition_delta_if_handle_date(repo_root, rows)
    costs = [float(row["cost_ms"]) for row in rows]
    clean = sum(distribution[key] for key in distribution if key.startswith("referent-intact") or key.startswith("referent-drifted"))
    canary = split_canary_status(rows)

    lines = [
        f"# 903i oracle audit summary: {label}",
        "",
        f"- corpus label: `{label}`",
        f"- git root name: `{repo_root.name}`",
        f"- citation rows: {len(rows)}",
        f"- clean dispositions: {clean}/{len(rows)} ({percent(clean, len(rows))})",
        f"- resolver hit-rate: {resolver_hits}/{len(rows)} ({percent(resolver_hits, len(rows))})",
        f"- median per-edge cost: {median(costs):.3f}ms",
        f"- p95 per-edge cost: {percentile(costs, 95):.3f}ms",
        f"- split canary: {canary}",
        "",
        "## Drift buckets",
        "",
        table(["bucket", "rows"], distribution.items()),
        "",
        "## Blame lie-rate inputs",
        "",
        f"- blame coverage: {date_sources['blame']}/{len(rows)} ({percent(date_sources['blame'], len(rows))})",
        f"- handle-date fallback: {date_sources['handle']}/{len(rows)} ({percent(date_sources['handle'], len(rows))})",
        f"- unknown assertion date: {date_sources['unknown']}/{len(rows)} ({percent(date_sources['unknown'], len(rows))})",
        f"- blame-vs-handle divergence: {blame_handle_divergence(rows)}/{date_sources['blame']} ({percent(blame_handle_divergence(rows), date_sources['blame'])})",
        f"- disposition delta if handle date used: {handle_deltas}/{len(rows)} ({percent(handle_deltas, len(rows))})",
        "",
        "### Suspicious bulk-date clusters",
        "",
        table(["date", "edge rows", "source rows"], bulk_date_rows(edge_dates, source_dates, len(rows))),
        "",
        "## Reverse-edge scan",
        "",
        f"- CR label references in code/docs: {reverse_scan['label_references']}",
        f"- unique CR labels: {reverse_scan['unique_labels']}",
        f"- files with CR labels: {reverse_scan['files']}",
        "",
        "Top labels:",
        table(["label", "count"], reverse_scan["top_labels"]),
        "",
        "Top files:",
        table(["file", "count"], reverse_scan["top_files"]),
    ]
    return "\n".join(lines) + "\n"


def disposition_delta_if_handle_date(repo_root: Path, rows: list[dict[str, Any]]) -> int:
    git = Git(repo_root)
    changed = 0
    for row in rows:
        handle_date = str(row["source_date"])
        target = normalize_target_path(repo_root, repo_root, str(row["target_path"]))
        if not handle_date or target is None:
            continue
        exists_now = git.exists_now(target)
        history = git.history_status(target)
        commits = git.commits_since(target, handle_date)
        move = git.move_info(target) if not exists_now and history == "present" else MoveInfo((), 0)
        if classify(exists_now, history, commits, handle_date, move) != row["final_disposition"]:
            changed += 1
    return changed


def blame_handle_divergence(rows: list[dict[str, Any]]) -> int:
    return sum(
        1
        for row in rows
        if row["date_source"] == "blame" and row["source_date"] and row["edge_date"] != row["source_date"]
    )


def bulk_date_rows(edge_dates: Counter[str], source_dates: Counter[str], total: int) -> list[tuple[str, str, str]]:
    dates = set(edge_dates) | set(source_dates)
    rows = [
        (date, str(edge_dates[date]), str(source_dates[date]))
        for date in sorted(dates)
        if edge_dates[date] >= max(10, total // 5) or source_dates[date] >= max(10, total // 5)
    ]
    return rows or [("<none>", "0", "0")]


def split_canary_status(rows: list[dict[str, Any]]) -> str:
    canary_rows = [
        row for row in rows if str(row["target_path"]) == "src/cli.rs" and row["final_disposition"] == "referent-moved-ambiguous"
    ]
    if canary_rows:
        fanouts = sorted({int(row["split_fanout"]) for row in canary_rows})
        return f"PASS referent-moved-ambiguous fanout={fanouts}"
    src_cli_rows = [row for row in rows if str(row["target_path"]) == "src/cli.rs"]
    if src_cli_rows:
        buckets = sorted({str(row["final_disposition"]) for row in src_cli_rows})
        return f"FAIL src/cli.rs buckets={buckets}"
    return "NOT_APPLICABLE no src/cli.rs citation rows"


def stop_rule_report(rows: list[dict[str, Any]]) -> str:
    canary = split_canary_status(rows)
    moved_clean = [
        row
        for row in rows
        if row["target_exists_now"] == "false"
        and row["target_history_status"] == "present"
        and row["final_disposition"] == "referent-moved -> head"
    ]
    move_honesty = "PASS"
    if canary.startswith("FAIL"):
        move_honesty = "FAIL"
    return f"stop_rules: move_honesty={move_honesty}; split_canary={canary}; clean_moves={len(moved_clean)}"


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=ROW_FIELDS)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in ROW_FIELDS})


def table(headers: list[str], rows: Any) -> str:
    materialized = list(rows)
    lines = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join("---" for _ in headers) + " |",
    ]
    for row in materialized:
        values = list(row)
        lines.append("| " + " | ".join(str(value).replace("|", "\\|") for value in values) + " |")
    return "\n".join(lines)


def percent(part: int, whole: int) -> str:
    if whole == 0:
        return "0.0%"
    return f"{(part / whole) * 100:.1f}%"


def percentile(values: list[float], pct: int) -> float:
    if not values:
        return 0.0
    values = sorted(values)
    index = min(len(values) - 1, round((pct / 100) * (len(values) - 1)))
    return values[index]


def median(values: list[float]) -> float:
    return percentile(values, 50)


def nonempty_lines(text: str) -> list[str]:
    return [line for line in text.splitlines() if line.strip()]


def git_root(path: Path) -> Path:
    proc = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "--show-toplevel"],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    if proc.returncode != 0:
        raise AuditError(f"{path} is not inside a git repository")
    return Path(proc.stdout.strip()).resolve()


if __name__ == "__main__":
    raise SystemExit(main())
