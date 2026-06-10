#!/usr/bin/env python3
"""Architecture fitness checks for anneal's crate and VM boundaries."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VM_DIR = ROOT / "crates" / "anneal-core" / "src" / "vm"

ALLOWED_WORKSPACE_DEPS = {
    "anneal": {"anneal-cli"},
    "anneal-code": {"anneal-core"},
    "anneal-lang": set(),
    "anneal-core": {"anneal-lang"},
    "anneal-md": {"anneal-core"},
    "anneal-cli": {"anneal-code", "anneal-core", "anneal-md"},
    "anneal-mcp": {"anneal-core"},
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


def main() -> None:
    check_vm_imports()
    check_workspace_dag()
    print("check-arch: ok")


if __name__ == "__main__":
    main()
