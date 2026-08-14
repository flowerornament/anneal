#!/usr/bin/env python3
"""Architecture fitness checks for anneal's crate and VM boundaries."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CRATES_DIR = ROOT / "crates"
CORE_SRC = CRATES_DIR / "anneal-core" / "src"
VM_DIR = CORE_SRC / "vm"

PRIVATE_CORE_MODULES = {
    "config_schema",
    "driver",
    "facts",
    "hash",
    "history",
    "ids",
    "impact",
    "ir",
    "lifecycle",
    "metadata",
    "path_policy",
    "policy",
    "project",
    "ranking",
    "repository",
    "retrieval",
    "source",
    "store",
    "target_probe",
    "time",
    "trail",
    "verbs",
    "visibility",
    "vm",
}

ALLOWED_WORKSPACE_DEPS = {
    "anneal": {"anneal-cli"},
    "anneal-code": {"anneal-core"},
    "anneal-lang": set(),
    "anneal-core": {"anneal-lang"},
    "anneal-md": {"anneal-core"},
    "anneal-cli": {"anneal-code", "anneal-core", "anneal-md"},
    "anneal-mcp": {"anneal-core"},
    "xtask": set(),
}

RAW_AST_TYPES = {"Rule", "Atom", "Body"}


def fail(message: str) -> None:
    print(f"check-arch: {message}", file=sys.stderr)
    sys.exit(1)


def rust_files(path: Path) -> list[Path]:
    return sorted(path.glob("*.rs"))


def ast_use_blocks(source: str) -> list[str]:
    return re.findall(r"use\s+crate::runtime::ast::\{(?P<body>.*?)\};", source, re.DOTALL)


def imported_names(use_block: str) -> set[str]:
    names = set()
    for item in use_block.split(","):
        name = item.strip()
        if not name:
            continue
        name = name.split(" as ", 1)[0].strip()
        if "::" in name:
            name = name.rsplit("::", 1)[-1]
        names.add(name)
    return names


def check_vm_imports() -> None:
    violations = []
    for path in rust_files(VM_DIR):
        rel = path.relative_to(ROOT)
        source = path.read_text()
        for line_number, line in enumerate(source.splitlines(), start=1):
            if "runtime::analysis" in line:
                violations.append(
                    f"{rel}:{line_number}: forbidden VM import edge to runtime::analysis"
                )
            for raw_type in RAW_AST_TYPES:
                if f"crate::runtime::ast::{raw_type}" in line:
                    violations.append(
                        f"{rel}:{line_number}: forbidden raw AST import {raw_type}"
                    )
        for block in ast_use_blocks(source):
            forbidden = imported_names(block) & RAW_AST_TYPES
            if forbidden:
                names = ", ".join(sorted(forbidden))
                violations.append(f"{rel}: forbidden raw AST import(s): {names}")

    if violations:
        fail("\n" + "\n".join(violations))


def check_retired_runtime_representations() -> None:
    retired = {
        "GraphIndex": "primitive oracle must use the PrimitiveIndex contract",
        "NamedRow": "stored rows must use the tuple substrate",
    }
    violations = []
    for path in sorted(CORE_SRC.rglob("*.rs")):
        for line_number, line in enumerate(path.read_text().splitlines(), start=1):
            for name, message in retired.items():
                if re.search(rf"\b{name}\b", line):
                    violations.append(
                        f"{path.relative_to(ROOT)}:{line_number}: {message}"
                    )

    if violations:
        fail("\n" + "\n".join(violations))


def check_primitive_index_contract() -> None:
    path = CORE_SRC / "runtime" / "eval" / "primitive_index.rs"
    source = path.read_text()
    index = re.search(r"struct PrimitiveIndex\s*\{(?P<body>.*?)\n\}", source, re.S)
    if index is None:
        fail("primitive index contract: missing PrimitiveIndex")
    visible_fields = [
        line.strip()
        for line in index.group("body").splitlines()
        if re.match(r"\s*pub(?:\([^)]*\))?\s+", line)
    ]
    if visible_fields:
        fail("primitive index contract: state must remain private")

    expected = {
        "apply_context",
        "from_tuples",
        "scoped_to_snapshot_tuples",
        "tuples",
    }
    actual = set(re.findall(r"^\s*pub\(super\) fn ([a-z_]+)", source, re.M))
    if actual != expected:
        fail(
            "primitive index contract: expected exactly "
            f"{sorted(expected)}, found {sorted(actual)}"
        )


def cargo_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def check_workspace_dag() -> None:
    metadata = cargo_metadata()
    workspace_names = {
        package["name"]
        for package in metadata["packages"]
        if package["id"] in metadata["workspace_members"]
    }
    unexpected_packages = workspace_names - set(ALLOWED_WORKSPACE_DEPS)
    if unexpected_packages:
        fail(
            "workspace package missing from architecture allow-list: "
            + ", ".join(sorted(unexpected_packages))
        )

    violations = []
    for package in metadata["packages"]:
        name = package["name"]
        if name not in workspace_names:
            continue
        allowed = ALLOWED_WORKSPACE_DEPS.get(name, set())
        workspace_deps = {
            dep["name"] for dep in package["dependencies"] if dep["name"] in workspace_names
        }
        for dep in sorted(workspace_deps - allowed):
            violations.append(f"{name} -> {dep} is not an allowed workspace edge")

    if violations:
        fail("\n" + "\n".join(violations))


def public_modules(source: str) -> set[str]:
    return set(
        re.findall(r"^pub mod ([a-zA-Z_][a-zA-Z0-9_]*)\b", source, re.MULTILINE)
    )


def check_core_facades() -> None:
    root_source = (CORE_SRC / "lib.rs").read_text()
    root_modules = public_modules(root_source)
    if root_modules != {"runtime"}:
        fail(
            "anneal-core must expose only the runtime module; found: "
            + ", ".join(sorted(root_modules))
        )

    runtime_modules = public_modules((CORE_SRC / "runtime" / "mod.rs").read_text())
    if runtime_modules:
        fail(
            "anneal-core runtime implementation modules must stay private: "
            + ", ".join(sorted(runtime_modules))
        )

    root_exports = re.findall(r"pub use\s+.*?;", root_source, re.DOTALL)
    if any(
        re.search(
            r"\b(history|SnapshotFact|SnapshotTime|SnapshotEntry|HistoryWarning)\b",
            export,
        )
        for export in root_exports
    ):
        fail("anneal-core snapshot contracts belong under the runtime facade")

    store_source = (CORE_SRC / "store.rs").read_text()
    public_store_signatures = re.findall(
        r"^\s*pub fn\s+\w+\s*\(.*?(?:->\s*.*?)?\s*\{",
        store_source,
        re.MULTILINE | re.DOTALL,
    )
    snapshot_signatures = [
        signature.splitlines()[0].strip()
        for signature in public_store_signatures
        if re.search(r"\b(Snapshot\w*|HistoryWarning|HistoryError)\b", signature)
    ]
    if snapshot_signatures:
        fail(
            "FactStore public signatures must not expose runtime-owned snapshot contracts: "
            + ", ".join(sorted(snapshot_signatures))
        )

    root_pattern = re.compile(
        r"anneal_core::(" + "|".join(sorted(PRIVATE_CORE_MODULES)) + r")::"
    )
    runtime_pattern = re.compile(
        r"anneal_core::runtime::"
        r"(analysis|ast|eval|loader|ndjson|parser|prelude|primitives|schedule)::"
    )
    violations = []
    for crate_dir in sorted(CRATES_DIR.iterdir()):
        if crate_dir.name == "anneal-core":
            continue
        for path in sorted(crate_dir.rglob("*.rs")):
            source = path.read_text()
            for line_number, line in enumerate(source.splitlines(), start=1):
                if root_pattern.search(line) or runtime_pattern.search(line):
                    violations.append(
                        f"{path.relative_to(ROOT)}:{line_number}: "
                        "import anneal-core through its root or runtime facade"
                    )

    if violations:
        fail("\n" + "\n".join(violations))


def main() -> None:
    check_vm_imports()
    check_retired_runtime_representations()
    check_primitive_index_contract()
    check_workspace_dag()
    check_core_facades()
    print("check-arch: ok")


if __name__ == "__main__":
    main()
