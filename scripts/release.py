#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")
ANNEAL_MANIFESTS = [
    ROOT / "Cargo.toml",
    ROOT / "crates/anneal-cli/Cargo.toml",
    ROOT / "crates/anneal-core/Cargo.toml",
    ROOT / "crates/anneal-lang/Cargo.toml",
    ROOT / "crates/anneal-legacy/Cargo.toml",
    ROOT / "crates/anneal-mcp/Cargo.toml",
    ROOT / "crates/anneal-md/Cargo.toml",
]


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


def cargo_manifest_versions() -> dict[str, str]:
    versions = {}
    for manifest in ANNEAL_MANIFESTS:
        data = tomllib.loads(read_text(manifest))
        versions[str(manifest.relative_to(ROOT))] = data["package"]["version"]
    return versions


def cargo_path_dependency_versions() -> dict[str, str]:
    versions = {}
    dependency_re = re.compile(
        r'(?m)^(anneal(?:-[a-z]+)?) = \{ version = "([^"]+)", path = "[^"]+" \}$'
    )
    for manifest in ANNEAL_MANIFESTS:
        text = read_text(manifest)
        for name, version in dependency_re.findall(text):
            key = f"{manifest.relative_to(ROOT)}:{name}"
            versions[key] = version
    return versions


def cargo_lock_versions() -> dict[str, str]:
    text = read_text(ROOT / "Cargo.lock")
    matches = re.findall(
        r'name = "(anneal(?:-[a-z]+)?)"\nversion = "([^"]+)"',
        text,
        re.MULTILINE,
    )
    if not matches:
        fail("could not find anneal package entries in Cargo.lock")
    return dict(matches)


def cargo_lock_version() -> str:
    versions = cargo_lock_versions()
    try:
        return versions["anneal"]
    except KeyError:
        fail("could not find anneal package entry in Cargo.lock")


def flake_version() -> str:
    text = read_text(ROOT / "flake.nix")
    match = re.search(r'(?m)^(\s*)annealVersion = "([^"]+)";$', text)
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
    if match is not None:
        return re.findall(r"`([^`]+)`", match.group(1))

    section = re.search(
        r"(?ms)^Binaries are published for:\n\n(?P<body>(?:- `[^`]+`\n)+)",
        text,
    )
    if section is None:
        fail("could not find release target list in README.md")
    return re.findall(r"`([^`]+)`", section.group("body"))


def beads_config_is_public_safe() -> bool:
    text = read_text(ROOT / ".beads/config.yaml")
    return re.search(r'(?m)^federation\.remote:\s*".+"', text) is None


def changelog_text() -> str:
    return read_text(ROOT / "CHANGELOG.md")


def changelog_has_entry(version: str) -> bool:
    pattern = rf"(?m)^## v?{re.escape(version)} - \d{{4}}-\d{{2}}-\d{{2}}$"
    return re.search(pattern, changelog_text()) is not None


def changelog_entry(version: str) -> str:
    text = changelog_text()
    heading = re.search(
        rf"(?m)^## v?{re.escape(version)} - \d{{4}}-\d{{2}}-\d{{2}}$",
        text,
    )
    if heading is None:
        fail(f"CHANGELOG.md is missing an entry for {version}")

    next_heading = re.search(
        r"(?m)^## v?\d+\.\d+\.\d+ - \d{4}-\d{2}-\d{2}$",
        text[heading.end() :],
    )
    if next_heading is None:
        return text[heading.end() :]
    return text[heading.end() : heading.end() + next_heading.start()]


def changelog_insert_entry(version: str) -> None:
    if changelog_has_entry(version):
        return

    today = date.today().isoformat()
    scaffold = (
        f"## v{version} - {today}\n\n"
        "### Changed\n\n"
        "- TODO: summarize release changes.\n\n"
    )

    text = changelog_text()
    marker = "All notable changes to `anneal` are documented in this file.\n\n"
    if marker not in text:
        fail("could not find CHANGELOG.md insertion marker")
    updated = text.replace(marker, marker + scaffold, 1)
    write_text(ROOT / "CHANGELOG.md", updated)


def changelog_entry_is_ready(version: str) -> bool:
    entry = changelog_entry(version)
    if "TODO:" in entry or "TBD" in entry:
        return False
    return re.search(r"(?m)^- ", entry) is not None


def replace_once(text: str, pattern: str, replacement: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        fail(f"pattern did not match exactly once: {pattern}")
    return updated


def bump(version: str) -> None:
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.2.1")

    cargo_lock = ROOT / "Cargo.lock"
    flake_nix = ROOT / "flake.nix"

    for manifest in ANNEAL_MANIFESTS:
        cargo_text = read_text(manifest)
        cargo_text = replace_once(
            cargo_text,
            r'(?m)^version = "[^"]+"$',
            f'version = "{version}"',
        )
        cargo_text = re.sub(
            r'(?m)^(anneal(?:-[a-z]+)? = \{ version = )"[^"]+"(, path = "[^"]+" \})$',
            rf'\1"{version}"\2',
            cargo_text,
        )
        write_text(manifest, cargo_text)

    lock_text = read_text(cargo_lock)
    lock_text = re.sub(
        r'(name = "anneal(?:-[a-z]+)?"\nversion = )"[^"]+"',
        rf'\1"{version}"',
        lock_text,
    )
    write_text(cargo_lock, lock_text)

    flake_text = read_text(flake_nix)
    flake_text = replace_once(
        flake_text,
        r'(?m)^(\s*)annealVersion = "[^"]+";$',
        rf'\1annealVersion = "{version}";',
    )
    write_text(flake_nix, flake_text)
    changelog_insert_entry(version)

    print(f"updated release version to {version}")
    for manifest in ANNEAL_MANIFESTS:
        print(f"  - {manifest.relative_to(ROOT)}")
    print("  - Cargo.lock")
    print("  - flake.nix")
    print("  - CHANGELOG.md")


def run(cmd: list[str]) -> None:
    print(f"+ {' '.join(cmd)}")
    subprocess.run(cmd, cwd=ROOT, check=True)


def verify() -> None:
    versions = {
        **cargo_manifest_versions(),
        **{f"Cargo.lock:{name}": version for name, version in cargo_lock_versions().items()},
        **cargo_path_dependency_versions(),
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

    version = unique_versions.pop()
    if not changelog_entry_is_ready(version):
        fail(
            "CHANGELOG.md must contain a release entry for "
            f"{version} with at least one bullet and no TODO/TBD placeholders"
        )

    run(["just", "check"])
    run(["just", "build"])
    run(["./target/release/anneal", "--version"])
    run(["./target/release/anneal", "--root", ".design", "check"])

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
