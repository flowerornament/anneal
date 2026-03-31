#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def cargo_version() -> str:
    data = tomllib.loads(read_text(ROOT / "Cargo.toml"))
    return data["package"]["version"]


def cargo_lock_version() -> str:
    text = read_text(ROOT / "Cargo.lock")
    match = re.search(
        r'name = "anneal"\nversion = "([^"]+)"\ndependencies = \[',
        text,
        re.MULTILINE,
    )
    if match is None:
        fail("could not find anneal package entry in Cargo.lock")
    return match.group(1)


def flake_version() -> str:
    text = read_text(ROOT / "flake.nix")
    match = re.search(r'pname = "anneal";\n(?P<indent>\s+)version = "([^"]+)";', text)
    if match is None:
        fail("could not find anneal package version in flake.nix")
    return match.group(2)


def workflow_targets() -> list[str]:
    text = read_text(ROOT / ".github/workflows/release.yml")
    return re.findall(r"- target: ([^\n]+)", text)


def installer_targets() -> list[str]:
    text = read_text(ROOT / "install.sh")
    match = re.search(
        r"SUPPORTED_RELEASE_TARGETS=\(\n(?P<body>(?:\s+\"[^\"]+\"\n)+)\)",
        text,
    )
    if match is None:
        fail("could not find SUPPORTED_RELEASE_TARGETS in install.sh")
    return re.findall(r'"([^"]+)"', match.group("body"))


def readme_targets() -> list[str]:
    text = read_text(ROOT / "README.md")
    match = re.search(r"Binaries available for: (.+)\.", text)
    if match is None:
        fail("could not find release target list in README.md")
    return re.findall(r"`([^`]+)`", match.group(1))


def beads_config_is_public_safe() -> bool:
    text = read_text(ROOT / ".beads/config.yaml")
    return re.search(r'(?m)^federation\.remote:\s*".+"', text) is None


def replace_once(text: str, pattern: str, replacement: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        fail(f"pattern did not match exactly once: {pattern}")
    return updated


def bump(version: str) -> None:
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.2.1")

    cargo_toml = ROOT / "Cargo.toml"
    cargo_lock = ROOT / "Cargo.lock"
    flake_nix = ROOT / "flake.nix"

    cargo_text = read_text(cargo_toml)
    cargo_text = replace_once(
        cargo_text,
        r'(?m)^version = "[^"]+"$',
        f'version = "{version}"',
    )
    write_text(cargo_toml, cargo_text)

    lock_text = read_text(cargo_lock)
    lock_text = replace_once(
        lock_text,
        r'name = "anneal"\nversion = "[^"]+"\ndependencies = \[',
        f'name = "anneal"\nversion = "{version}"\ndependencies = [',
    )
    write_text(cargo_lock, lock_text)

    flake_text = read_text(flake_nix)
    flake_text = replace_once(
        flake_text,
        r'(pname = "anneal";\n)(?P<indent>\s+)version = "[^"]+";',
        rf'\1\g<indent>version = "{version}";',
    )
    write_text(flake_nix, flake_text)

    print(f"updated release version to {version}")
    print("  - Cargo.toml")
    print("  - Cargo.lock")
    print("  - flake.nix")


def run(cmd: list[str]) -> None:
    print(f"+ {' '.join(cmd)}")
    subprocess.run(cmd, cwd=ROOT, check=True)


def verify() -> None:
    versions = {
        "Cargo.toml": cargo_version(),
        "Cargo.lock": cargo_lock_version(),
        "flake.nix": flake_version(),
    }
    unique_versions = set(versions.values())
    if len(unique_versions) != 1:
        details = ", ".join(f"{name}={version}" for name, version in versions.items())
        fail(f"release versions do not match: {details}")

    workflow = workflow_targets()
    installer = installer_targets()
    readme = readme_targets()
    if workflow != installer or workflow != readme:
        fail(
            "release targets do not match across release.yml, install.sh, and README.md: "
            f"workflow={workflow}, install={installer}, readme={readme}"
        )

    if not beads_config_is_public_safe():
        fail(".beads/config.yaml contains a concrete federation.remote; use local configuration instead")

    run(["just", "check"])
    run(["just", "build"])
    run(["./target/release/anneal", "--version"])
    run(["./target/release/anneal", "--root", ".design", "check"])

    version = unique_versions.pop()
    print(f"release verification passed for {version}")
    print(f"release targets: {', '.join(workflow)}")


def tag(version: str) -> None:
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.2.1")
    current = cargo_version()
    if current != version:
        fail(f"Cargo.toml version is {current}, expected {version}")

    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    if status.stdout.strip():
        fail("git working tree must be clean before tagging")

    tag_name = f"v{version}"
    tags = subprocess.run(
        ["git", "tag", "--list", tag_name],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    if tags.stdout.strip():
        fail(f"tag {tag_name} already exists")

    run(["git", "tag", "-a", tag_name, "-m", tag_name])
    run(["git", "push", "origin", tag_name])


def main() -> None:
    parser = argparse.ArgumentParser(description="Release helper for anneal")
    subparsers = parser.add_subparsers(dest="command", required=True)

    bump_parser = subparsers.add_parser("bump", help="update release versions")
    bump_parser.add_argument("version")

    subparsers.add_parser("verify", help="run release readiness checks")

    tag_parser = subparsers.add_parser("tag", help="create and push a release tag")
    tag_parser.add_argument("version")

    args = parser.parse_args()
    if args.command == "bump":
        bump(args.version)
    elif args.command == "verify":
        verify()
    else:
        tag(args.version)


if __name__ == "__main__":
    main()
