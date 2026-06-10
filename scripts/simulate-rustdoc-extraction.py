#!/usr/bin/env python3
"""Summarize rustdoc JSON as an anneal-code extraction simulation.

The script does not create anneal facts. It measures the handle/edge/lattice
shape rustdoc JSON can support so adapter design starts from evidence.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any


TODO_RE = re.compile(r"\b(TODO|FIXME)\b", re.IGNORECASE)
CRATE_VERSION_RE = re.compile(r"^(?:[A-Za-z_-]+-)?v?(\d+\.\d+\.\d+)$")
PUBLIC_VISIBILITIES = {"public", "default"}


@dataclass(frozen=True)
class ItemSummary:
    item_id: str
    name: str
    kind: str
    path: str
    span_filename: str
    visibility: str
    docs_len: int
    attr_count: int
    deprecated: bool
    deprecation_note: str
    deprecation_since: str


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rustdoc-json", required=True, type=Path)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--crate-name", default="regex")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    data = json.loads(args.rustdoc_json.read_text(encoding="utf-8"))
    source_root = args.source_root.resolve()
    summaries = summarize_items(data)
    containment_edges = collect_containment_edges(data)
    type_edges = collect_type_edges(data)
    doc_link_edges = collect_doc_link_edges(data)
    source = summarize_source_tree(source_root)
    release = summarize_release_cadence(source_root, args.crate_name)

    public_items = [item for item in summaries if item.visibility in PUBLIC_VISIBILITIES]
    deprecated = [item for item in summaries if item.deprecated]
    deprecation_resolvable = [
        item
        for item in deprecated
        if item.deprecation_note and note_mentions_known_item(item.deprecation_note, public_items)
    ]
    since_values = [item.deprecation_since for item in deprecated if item.deprecation_since]

    type_targets = Counter(edge["target"] for edge in type_edges)
    mega_targets = {target: count for target, count in type_targets.items() if count >= 40}
    public_private = Counter(item.visibility for item in summaries)
    kind_counts = Counter(item.kind for item in summaries)

    metrics = {
        "crate": args.crate_name,
        "crate_version": data.get("crate_version"),
        "rustdoc_format_version": data.get("format_version"),
        "includes_private": data.get("includes_private"),
        "scale": {
            "handles_projected": len(summaries),
            "public_handles_projected": len(public_items),
            "edges_projected": len(containment_edges) + len(type_edges) + len(doc_link_edges),
            "containment_edges": len(containment_edges),
            "type_reference_edges": len(type_edges),
            "doc_link_edges": len(doc_link_edges),
            "doc_content_bytes": sum(item.docs_len for item in summaries),
            "source_bytes": source["source_bytes"],
        },
        "items": {
            "by_kind": dict(sorted(kind_counts.items())),
            "by_visibility": dict(sorted(public_private.items())),
            "span_files": len({item.span_filename for item in summaries if item.span_filename}),
        },
        "lifecycle": {
            "deprecated_items": len(deprecated),
            "deprecation_notes": sum(1 for item in deprecated if item.deprecation_note),
            "deprecation_since_values": len(since_values),
            "public_only_rustdoc": not data.get("includes_private", False),
            "source_pub_crate_mentions": source["pub_crate_mentions"],
            "test_files": source["test_files"],
            "generated_markers": source["generated_markers"],
        },
        "currency": {
            "deprecated_items": len(deprecated),
            "deprecated_notes_resolvable_by_name": len(deprecation_resolvable),
            "deprecated_notes_unresolved": len(deprecated) - len(deprecation_resolvable),
        },
        "topic": {
            "type_reference_edges": len(type_edges),
            "unique_type_targets": len(type_targets),
            "top_type_targets": type_targets.most_common(20),
            "mega_targets_40": mega_targets,
            "vec_count": type_targets.get("Vec", 0),
            "option_count": type_targets.get("Option", 0),
            "result_count": type_targets.get("Result", 0),
        },
        "structure_importance": {
            "containment_edges": len(containment_edges),
            "max_containment_depth": max_containment_depth(summaries),
            "impl_items": kind_counts.get("impl", 0),
            "trait_items": kind_counts.get("trait", 0),
            "type_refs_available": True,
            "body_call_edges_available": False,
        },
        "relevance": {
            "named_items": sum(1 for item in summaries if item.name),
            "doc_content_bytes": sum(item.docs_len for item in summaries),
            "items_with_docs": sum(1 for item in summaries if item.docs_len > 0),
            "signature_items": sum(1 for item in summaries if item.kind in {"function", "method", "struct", "enum", "trait", "type_alias"}),
        },
        "recency": {
            "since_values": since_values,
            "since_coverage": len(since_values),
            "git_available": release["git_available"],
            "latest_commit_date": release["latest_commit_date"],
            "first_commit_date": release["first_commit_date"],
        },
        "convergence": {
            "version_tags": release["version_tags"],
            "version_tag_count": len(release["version_tags"]),
            "latest_version_tag": release["latest_version_tag"],
            "release_cadence_signal": release["release_cadence_signal"],
        },
        "obligations": {
            "todo_fixme_count": source["todo_fixme_count"],
            "todo_fixme_files": source["todo_fixme_files"],
        },
        "source": source,
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {args.output}")
    print(json.dumps(metrics["scale"], sort_keys=True))
    return 0


def summarize_items(data: dict[str, Any]) -> list[ItemSummary]:
    paths = {str(key): value for key, value in data.get("paths", {}).items()}
    summaries: list[ItemSummary] = []
    for raw_id, item in data.get("index", {}).items():
        item_id = str(raw_id)
        inner = item.get("inner") or {}
        kind = next(iter(inner.keys()), "unknown")
        path_info = paths.get(item_id, {})
        path = "::".join(path_info.get("path") or []) or item.get("name") or item_id
        deprecation = item.get("deprecation") or {}
        span = item.get("span") or {}
        docs = item.get("docs") or ""
        summaries.append(
            ItemSummary(
                item_id=item_id,
                name=item.get("name") or "",
                kind=kind,
                path=path,
                span_filename=span.get("filename") or "",
                visibility=visibility_name(item.get("visibility")),
                docs_len=len(docs.encode("utf-8")),
                attr_count=len(item.get("attrs") or []),
                deprecated=bool(deprecation),
                deprecation_note=str(deprecation.get("note") or "") if isinstance(deprecation, dict) else "",
                deprecation_since=str(deprecation.get("since") or "") if isinstance(deprecation, dict) else "",
            )
        )
    return summaries


def visibility_name(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return next(iter(value.keys()), "unknown")
    return "unknown"


def collect_containment_edges(data: dict[str, Any]) -> list[dict[str, str]]:
    edges: list[dict[str, str]] = []
    for raw_id, item in data.get("index", {}).items():
        for child in direct_children(item.get("inner") or {}):
            edges.append({"from": str(raw_id), "to": str(child), "kind": "contains"})
    return edges


def direct_children(inner: dict[str, Any]) -> list[str]:
    children: list[str] = []
    for value in inner.values():
        if isinstance(value, dict):
            for key in ("items", "variants", "fields", "impls"):
                for item_id in value.get(key) or []:
                    children.append(str(item_id))
    return children


def collect_type_edges(data: dict[str, Any]) -> list[dict[str, str]]:
    paths = {str(key): value for key, value in data.get("paths", {}).items()}
    edges: list[dict[str, str]] = []
    for raw_id, item in data.get("index", {}).items():
        for target_id, target_path in resolved_paths(item.get("inner")):
            target = target_path or path_name(paths.get(str(target_id), {})) or str(target_id)
            if str(target_id) == str(raw_id):
                continue
            edges.append({"from": str(raw_id), "to": str(target_id), "target": target, "kind": "type_ref"})
    return edges


def resolved_paths(value: Any) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    if isinstance(value, dict):
        if "resolved_path" in value and isinstance(value["resolved_path"], dict):
            resolved = value["resolved_path"]
            out.append((str(resolved.get("id", "")), str(resolved.get("path") or "")))
        for child in value.values():
            out.extend(resolved_paths(child))
    elif isinstance(value, list):
        for child in value:
            out.extend(resolved_paths(child))
    return out


def path_name(path_info: dict[str, Any]) -> str:
    parts = path_info.get("path") or []
    return parts[-1] if parts else ""


def collect_doc_link_edges(data: dict[str, Any]) -> list[dict[str, str]]:
    edges: list[dict[str, str]] = []
    for raw_id, item in data.get("index", {}).items():
        for label, target in (item.get("links") or {}).items():
            edges.append({"from": str(raw_id), "to": str(target), "label": str(label), "kind": "doc_link"})
    return edges


def summarize_source_tree(root: Path) -> dict[str, Any]:
    source_files = [
        path
        for path in root.rglob("*")
        if path.is_file()
        and path.suffix == ".rs"
        and not any(part in {".git", "target"} for part in path.parts)
    ]
    source_bytes = 0
    todo_count = 0
    todo_files = 0
    pub_crate_mentions = 0
    generated_markers = 0
    test_files = 0
    for path in source_files:
        text = path.read_text(encoding="utf-8", errors="ignore")
        source_bytes += len(text.encode("utf-8"))
        matches = TODO_RE.findall(text)
        if matches:
            todo_count += len(matches)
            todo_files += 1
        pub_crate_mentions += text.count("pub(crate)")
        lower = text.lower()
        if "generated" in lower or "@generated" in lower:
            generated_markers += 1
        rel = path.relative_to(root).as_posix()
        if "/tests/" in f"/{rel}" or rel.startswith("tests/") or "#[cfg(test)]" in text:
            test_files += 1
    return {
        "rust_source_files": len(source_files),
        "source_bytes": source_bytes,
        "todo_fixme_count": todo_count,
        "todo_fixme_files": todo_files,
        "pub_crate_mentions": pub_crate_mentions,
        "test_files": test_files,
        "generated_markers": generated_markers,
    }


def summarize_release_cadence(root: Path, crate_name: str) -> dict[str, Any]:
    if not (root / ".git").exists():
        return {
            "git_available": False,
            "first_commit_date": "",
            "latest_commit_date": "",
            "version_tags": [],
            "latest_version_tag": "",
            "release_cadence_signal": "unavailable",
        }
    tags = run_git(root, ["tag", "--list"]).splitlines()
    version_tags = sorted(
        (tag for tag in tags if crate_version_tag(tag, crate_name)),
        key=version_tag_key,
    )
    first = (run_git(root, ["log", "--reverse", "--format=%cs"]).splitlines() or [""])[0]
    latest = run_git(root, ["log", "-1", "--format=%cs"]).strip()
    return {
        "git_available": True,
        "first_commit_date": first,
        "latest_commit_date": latest,
        "version_tags": version_tags[-30:],
        "latest_version_tag": version_tags[-1] if version_tags else "",
        "release_cadence_signal": "tag-history-present" if version_tags else "git-only",
    }


def crate_version_tag(tag: str, crate_name: str) -> bool:
    return bool(re.fullmatch(r"\d+\.\d+\.\d+", tag) or re.fullmatch(rf"{re.escape(crate_name)}-\d+\.\d+\.\d+", tag))


def version_tag_key(tag: str) -> tuple[int, int, int]:
    version = tag.rsplit("-", 1)[-1]
    parts = version.split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        return (0, 0, 0)
    return tuple(int(part) for part in parts)


def run_git(root: Path, args: list[str]) -> str:
    proc = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    return proc.stdout if proc.returncode == 0 else ""


def note_mentions_known_item(note: str, public_items: list[ItemSummary]) -> bool:
    if not note:
        return False
    note_lower = note.lower()
    names = {item.name.lower() for item in public_items if item.name and len(item.name) >= 4}
    return any(name in note_lower for name in names)


def max_containment_depth(items: list[ItemSummary]) -> int:
    max_depth = 0
    for item in items:
        if item.path:
            max_depth = max(max_depth, item.path.count("::") + 1)
    return max_depth


if __name__ == "__main__":
    raise SystemExit(main())
