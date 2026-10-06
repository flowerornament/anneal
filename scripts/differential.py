#!/usr/bin/env python3
"""Compare nonempty command artifacts; see `just differential --help`."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass(frozen=True)
class NonEmptyArtifact:
    mode: str
    exclusions: tuple[str, ...]
    row_count: int
    sha256: str

    def __post_init__(self):
        if self.row_count <= 0:
            raise ValueError("no comparable artifact: row_count must be positive")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fingerprint(root: Path) -> dict:
    """All files, including hidden files and symlink targets; no ignore policy."""
    entries = []

    def visit(path: Path, ancestors: frozenset[Path]):
        resolved = path.resolve(strict=True)
        relative = str(path.relative_to(root))
        link = os.readlink(path) if path.is_symlink() else None
        if path.is_dir():
            if resolved in ancestors:
                raise ValueError(f"cyclic corpus directory: {path}")
            entries.append([relative, "directory", link])
            for child in sorted(path.iterdir()):
                visit(child, ancestors | {resolved})
        elif path.is_file():
            entries.append(
                [relative, "file", link, path.stat().st_size, file_digest(path)]
            )
        else:
            raise ValueError(f"unsupported corpus entry: {path}")

    visit(root, frozenset())
    files = sum(entry[1] == "file" for entry in entries)
    if not files:
        raise ValueError(f"corpus has no files: {root}")
    return {
        "root": str(root),
        "file_count": files,
        "sha256": digest(json_bytes(entries)),
    }


def json_bytes(value) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")


class JsonNumber(str):
    """An exact JSON number token, never coerced through host float precision."""


def row_bytes(value) -> bytes:
    if isinstance(value, JsonNumber):
        return str(value).encode("ascii")
    if isinstance(value, dict):
        return (
            b"{"
            + b",".join(
                json_bytes(key) + b":" + row_bytes(value[key]) for key in sorted(value)
            )
            + b"}"
        )
    if isinstance(value, list):
        return b"[" + b",".join(row_bytes(item) for item in value) + b"]"
    return json_bytes(value)


def strict_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError(f"duplicate JSON field: {name}")
        result[name] = value
    return result


def reject_constant(value):
    raise ValueError(f"nonfinite JSON value: {value}")


def pointer_parts(pointer: str) -> list[str]:
    if not pointer.startswith("/"):
        raise ValueError(f"exclusion must be a JSON pointer starting with /: {pointer}")
    parts = pointer[1:].split("/")
    for part in parts:
        if "~" in part.replace("~0", "").replace("~1", ""):
            raise ValueError(f"invalid JSON pointer escape: {pointer}")
    return [part.replace("~1", "/").replace("~0", "~") for part in parts]


def remove_field(row: dict, parts: list[str]) -> bool:
    parent = row
    for part in parts[:-1]:
        if not isinstance(parent, dict) or part not in parent:
            return False
        parent = parent[part]
    if isinstance(parent, dict) and parts[-1] in parent:
        del parent[parts[-1]]
        return True
    return False


def canonicalize(data: bytes, mode: str, exclusions: tuple[str, ...]):
    removed = dict.fromkeys(exclusions, 0)
    if mode == "raw":
        if exclusions:
            raise ValueError("raw mode does not permit field exclusions")
        count = len(data.splitlines())
        canonical = data
    else:
        rows = []
        for number, line in enumerate(data.splitlines(), 1):
            if not line.strip():
                continue
            try:
                row = json.loads(
                    line,
                    object_pairs_hook=strict_object,
                    parse_constant=reject_constant,
                    parse_int=JsonNumber,
                    parse_float=JsonNumber,
                )
            except (ValueError, UnicodeDecodeError) as error:
                raise ValueError(f"invalid NDJSON row {number}: {error}") from error
            if not isinstance(row, dict):
                raise TypeError(f"NDJSON row {number} is not an object")
            for pointer in exclusions:
                removed[pointer] += remove_field(row, pointer_parts(pointer))
            rows.append(row_bytes(row))
        count = len(rows)
        # Retain repeated rows; sorting is not set conversion.
        canonical = b"".join(row + b"\n" for row in sorted(rows))
    artifact = NonEmptyArtifact(mode, exclusions, count, digest(canonical))
    return artifact, canonical, removed


def execute_side(
    label: str,
    binary: Path,
    args,
    root: Path,
    output: Path,
    pins: list[dict],
    roots: list[Path],
) -> dict:
    side = output / label
    side.mkdir()
    record = {
        "side": label,
        "binary": str(binary),
        "corpus": pins,
        "artifact": None,
        "exit_status": None,
        "stdout_bytes": 0,
        "stderr_bytes": 0,
        "stdout_path": str(side / "stdout"),
        "stderr_path": str(side / "stderr"),
        "refusals": [],
    }
    (side / "stdout").write_bytes(b"")
    (side / "stderr").write_bytes(b"")
    try:
        record["binary_sha256"] = file_digest(binary)
        template = args.command
        # A command without {binary} is interpreted as arguments to the binary.
        template = (
            template
            if any("{binary}" in arg for arg in template)
            else ["{binary}", *template]
        )
        argv = [
            arg.replace("{binary}", str(binary)).replace("{root}", str(root))
            for arg in template
        ]
        record.update(argv=argv, cwd=str(root), host_load=os.getloadavg())
        if [fingerprint(path) for path in roots] != pins:
            raise ValueError("corpus content changed before invocation")
        start = time.monotonic()
        try:
            result = subprocess.run(
                argv, cwd=root, capture_output=True, check=False, timeout=args.timeout
            )
            stdout, stderr, code = result.stdout, result.stderr, result.returncode
        except subprocess.TimeoutExpired as error:
            stdout, stderr, code = error.stdout or b"", error.stderr or b"", None
            record["refusals"].append(f"command timed out after {args.timeout}s")
        record.update(
            wall_seconds=time.monotonic() - start,
            exit_status=code,
            stdout_bytes=len(stdout),
            stderr_bytes=len(stderr),
            stdout_path=str(side / "stdout"),
            stderr_path=str(side / "stderr"),
        )
        (side / "stdout").write_bytes(stdout)
        (side / "stderr").write_bytes(stderr)
        if code != 0:
            record["refusals"].append(
                f"command exit status {code}; inspect {side / 'stderr'}"
            )
        if file_digest(binary) != record["binary_sha256"]:
            record["refusals"].append("binary content changed during invocation")
        if [fingerprint(path) for path in roots] != pins:
            record["refusals"].append("corpus content changed during invocation")
        if not record["refusals"]:
            artifact, canonical, removed = canonicalize(
                stdout, args.mode, tuple(args.exclude)
            )
            record.update(
                artifact=asdict(artifact),
                removed_fields=removed,
                compared_path=str(side / "compared"),
            )
            (side / "compared").write_bytes(canonical)
    except (OSError, ValueError, TypeError) as error:
        record["refusals"].append(str(error))
    return record


def compare(left: NonEmptyArtifact, right: NonEmptyArtifact) -> bool:
    # Count and normalization policy are part of equality, not just annotations.
    if not isinstance(left, NonEmptyArtifact) or not isinstance(
        right, NonEmptyArtifact
    ):
        raise TypeError("comparison requires two nonempty artifacts")
    return left == right


def parser():
    p = argparse.ArgumentParser(
        description=__doc__,
        epilog="""Example:
just differential --left /tmp/before/anneal --right /tmp/after/anneal --root .design
--output /tmp/receipt --mode ndjson -- --root {root} context identity
Output must be a new directory outside all input roots. Primary root and declared
--input-root mounts are content-pinned before/after each invocation. No Git/jj
snapshot or state writes. Exit 0 equal, 1 different, 2 refused. Optional {binary}
command templates support an explicit argv wrapper; its argv is in the receipt.
Exclusions are exact object-field JSON pointers (no wildcards/array deletion).
Timing is observational wall time, not a paired performance experiment.""",
    )
    p.add_argument("--left", type=Path, required=True)
    p.add_argument("--right", type=Path, required=True)
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--input-root", type=Path, action="append", default=[])
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--mode", choices=["ndjson", "raw"], default="ndjson")
    p.add_argument("--exclude", action="append", default=[], metavar="JSON_POINTER")
    p.add_argument("--timeout", type=float, default=300)
    p.add_argument("command", nargs=argparse.REMAINDER)
    return p


def run(args) -> int:
    root = args.root.resolve(strict=True)
    roots = [root, *(path.resolve(strict=True) for path in args.input_root)]
    output = args.output.resolve()
    if any(output.is_relative_to(path) for path in roots):
        raise ValueError("output directory must be outside corpus input roots")
    if output.exists():
        raise ValueError(f"output already exists: {output}")
    if args.timeout <= 0:
        raise ValueError("timeout must be positive")
    args.exclude = sorted(set(args.exclude))
    for exclusion in args.exclude:
        pointer_parts(exclusion)
    if args.mode == "raw" and args.exclude:
        raise ValueError("raw mode does not permit exclusions")
    if args.command[:1] == ["--"]:
        args.command = args.command[1:]
    output.mkdir(parents=True)
    receipt = {
        "schema": "anneal.differential/1",
        "mode": args.mode,
        "declared_exclusions": args.exclude,
        "status": "refused",
        "sides": [],
    }
    try:
        pins = [fingerprint(path) for path in roots]
        for label, path in [("left", args.left), ("right", args.right)]:
            receipt["sides"].append(
                execute_side(label, path.resolve(), args, root, output, pins, roots)
            )
        if all(not side["refusals"] for side in receipt["sides"]):
            artifacts = [
                NonEmptyArtifact(
                    **{
                        **side["artifact"],
                        "exclusions": tuple(side["artifact"]["exclusions"]),
                    }
                )
                for side in receipt["sides"]
            ]
            receipt["status"] = "equal" if compare(*artifacts) else "different"
    except (OSError, ValueError, TypeError) as error:
        receipt["refusal"] = str(error)
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))
    return {"equal": 0, "different": 1, "refused": 2}[receipt["status"]]


def main() -> int:
    try:
        return run(parser().parse_args())
    except (OSError, ValueError, TypeError) as error:
        print(f"differential: refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
