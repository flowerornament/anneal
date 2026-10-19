#!/usr/bin/env python3
"""Keep Git launches at the reviewed repository boundaries (anneal-gt1t).

This is a source census, not a general Rust or shell parser. Fixture-only Rust
modules/files and test scripts are exempt. `jj git` selects jj's provider and
is not a Git launch. xtask deliberately stays independent of core.
"""

from __future__ import annotations

import ast
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUST_BUILDERS = {
    ("crates/anneal-core/src/repository.rs", "git_command"),
    ("xtask/src/atlas.rs", "git"),
}
# Mask comments and strings without moving source offsets; brace scopes survive.
RUST_LITERALS = re.compile(
    r'//[^\n]*|/\*.*?\*/|r(?P<hash>\#*)".*?"(?P=hash)|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\'',
    re.DOTALL,
)


def block_end(masked: str, start: int) -> int:
    depth = 0
    for index in range(start, len(masked)):
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth -= 1
            if depth == 0:
                return index + 1
    return len(masked)


def rust_calls(relative: str, source: str) -> list[tuple[int, bool]]:
    masked = RUST_LITERALS.sub(lambda match: " " * len(match[0]), source)
    tests = []
    for match in re.finditer(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{", masked):
        start = masked.index("{", match.start())
        tests.append((match.start(), block_end(masked, start)))
    functions = []
    for match in re.finditer(r"\bfn\s+(\w+)\b[^;{]*\{", masked):
        functions.append((match[1], match.start(), block_end(masked, match.end() - 1)))
    calls = []
    for match in re.finditer(r'\bCommand\s*::\s*new\s*\(\s*"git"\s*\)', source):
        if not masked[match.start():].startswith("Command"):
            continue  # a comment or quoted example
        if any(start <= match.start() < end for start, end in tests):
            continue
        owners = [name for name, start, end in functions if start <= match.start() < end]
        allowed = any((relative, name) in RUST_BUILDERS for name in owners)
        calls.append((source.count("\n", 0, match.start()) + 1, allowed))
    return calls


def python_calls(relative: str, source: str) -> list[tuple[int, bool]]:
    tree = ast.parse(source)
    calls = []
    for node in ast.walk(tree):
        if not isinstance(node, (ast.List, ast.Tuple)) or not node.elts:
            continue
        if not isinstance(node.elts[0], ast.Constant) or node.elts[0].value != "git":
            continue
        allowed = relative == "scripts/git_repository.py" and any(
            isinstance(function, ast.FunctionDef) and function.name == "run"
            and function.lineno <= node.lineno <= function.end_lineno
            for function in ast.walk(tree)
        )
        calls.append((node.lineno, allowed))
    return calls


def shell_calls(relative: str, source: str) -> list[tuple[int, bool]]:
    calls = []
    function = ""
    for line_number, line in enumerate(source.splitlines(), 1):
        if match := re.match(r"(\w+)\(\)\s*\{", line):
            function = match[1]
        if line.startswith("}"):
            function = ""
        if line.lstrip().startswith("#") or re.match(r"\s*require_cmd git\s*$", line):
            continue
        line = re.sub(r"\bjj\s+git\b", "jj_git", line)
        if re.search(r"(?<![\w./+-])git\s", line):
            calls.append((line_number, relative == "scripts/git-repository.sh" and function == "git_at_root"))
    return calls


def census(root: Path) -> tuple[list[str], list[str]]:
    allowed, refused = [], []
    paths = sorted(path for directory in ("crates", "xtask", "scripts") for path in (root / directory).rglob("*")
                   if path.is_file() and not {"target", "__pycache__"}.intersection(path.relative_to(root).parts))
    for path in paths:
        relative = path.relative_to(root).as_posix()
        if path.name.startswith("test-") or path.name == "tests.rs" or "tests" in path.relative_to(root).parts:
            continue
        source = path.read_text() if path.suffix in {".rs", ".py", ".sh"} else ""
        if path.suffix == ".rs":
            calls = rust_calls(relative, source)
        elif path.suffix == ".py":
            calls = python_calls(relative, source)
        elif path.suffix == ".sh":
            calls = shell_calls(relative, source)
        else:
            continue
        for line, blessed in calls:
            (allowed if blessed else refused).append(f"{relative}:{line}")
    return allowed, refused


def main() -> int:
    allowed, refused = census(ROOT)
    for site in allowed:
        print(f"Git boundary: {site}")
    for site in refused:
        print(f"unvalidated Git launch: {site}", file=sys.stderr)
    if len(allowed) != 4:
        print(f"expected four Git builders (two Rust, Python, shell), found {len(allowed)}", file=sys.stderr)
        return 1
    return int(bool(refused))


if __name__ == "__main__":
    raise SystemExit(main())
