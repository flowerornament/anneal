#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
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
    ROOT / "crates/anneal-code/Cargo.toml",
    ROOT / "crates/anneal-cli/Cargo.toml",
    ROOT / "crates/anneal-core/Cargo.toml",
    ROOT / "crates/anneal-lang/Cargo.toml",
    ROOT / "crates/anneal-mcp/Cargo.toml",
    ROOT / "crates/anneal-md/Cargo.toml",
]
CHANGELOG_INTRO_MARKER = (
    "All notable changes to `anneal` are documented in this file.\n\n"
)
UNRELEASED_HEADING = "## Unreleased"
CACHE_NAME = "flowerornament"
CACHE_URI = f"https://{CACHE_NAME}.cachix.org"
CACHE_PUBLIC_KEY = (
    "flowerornament.cachix.org-1:gSODgIXgfRANrEGITBOF8XWaEKNy8hkNGfRVwqUG46c="
)
CACHE_PIN_REVISIONS = 3


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


def flake_package_systems() -> list[str]:
    text = read_text(ROOT / "flake.nix")
    match = re.search(r"(?m)^\s*systems = \[(?P<body>[^]]+)\];$", text)
    if match is None:
        fail("could not find package systems in flake.nix")
    return re.findall(r'"([^"]+)"', match.group("body"))


def cache_workflow_systems(job: str) -> list[str]:
    text = read_text(ROOT / ".github/workflows/nix-cache.yml")
    match = re.search(
        rf"(?ms)^  {re.escape(job)}:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)",
        text,
    )
    if match is None:
        fail(f"could not find {job} job in nix-cache.yml")
    return re.findall(r"- system: ([^\n]+)", match.group("body"))


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


def changelog_text_has_entry(text: str, version: str) -> bool:
    pattern = rf"(?m)^## v?{re.escape(version)} - \d{{4}}-\d{{2}}-\d{{2}}$"
    return re.search(pattern, text) is not None


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


def changelog_scaffold(version: str, today: str) -> str:
    return (
        f"## v{version} - {today}\n\n"
        "### Changed\n\n"
        "- TODO: summarize release changes.\n\n"
    )


def changelog_pending_entries(unreleased_block: str) -> list[str]:
    entries = []
    bullets = list(re.finditer(r"(?m)^- ", unreleased_block))
    for bullet in bullets:
        boundary = re.search(
            r"(?m)^(?:- |### )",
            unreleased_block[bullet.end() :],
        )
        end = (
            len(unreleased_block)
            if boundary is None
            else bullet.end() + boundary.start()
        )
        entry = " ".join(unreleased_block[bullet.end() : end].split())
        entries.append(entry)
    return entries


def changelog_insert_entry_text(
    text: str, version: str, today: str
) -> tuple[str, list[str]]:
    if CHANGELOG_INTRO_MARKER not in text:
        raise ValueError("could not find CHANGELOG.md insertion marker")

    unreleased_matches = list(
        re.finditer(rf"(?m)^{re.escape(UNRELEASED_HEADING)}\s*$", text)
    )
    if len(unreleased_matches) != 1:
        raise ValueError("CHANGELOG.md must contain exactly one ## Unreleased section")

    unreleased = unreleased_matches[0]
    next_heading = re.search(r"(?m)^## ", text[unreleased.end() :])
    unreleased_end = (
        len(text) if next_heading is None else unreleased.end() + next_heading.start()
    )
    unreleased_block = text[unreleased.start() : unreleased_end]
    pending_entries = changelog_pending_entries(unreleased_block)

    without_unreleased = text[: unreleased.start()] + text[unreleased_end:]
    marker_end = without_unreleased.index(CHANGELOG_INTRO_MARKER) + len(
        CHANGELOG_INTRO_MARKER
    )
    normalized = (
        without_unreleased[:marker_end]
        + unreleased_block
        + without_unreleased[marker_end:]
    )
    if changelog_text_has_entry(normalized, version):
        return normalized, pending_entries

    insert_at = marker_end + len(unreleased_block)
    updated = (
        normalized[:insert_at]
        + changelog_scaffold(version, today)
        + normalized[insert_at:]
    )
    return updated, pending_entries


def unreleased_warning(version: str, pending_entries: list[str]) -> str | None:
    if not pending_entries:
        return None

    entries = "\n".join(f"  - {entry}" for entry in pending_entries)
    return (
        f"warning: CHANGELOG.md Unreleased still contains "
        f"{len(pending_entries)} entries after scaffolding v{version}:\n"
        f"{entries}\n"
        f"Review whether they belong in v{version}."
    )


def changelog_insert_entry(version: str) -> None:
    text = changelog_text()
    try:
        updated, pending_entries = changelog_insert_entry_text(
            text, version, date.today().isoformat()
        )
    except ValueError as error:
        fail(str(error))

    write_text(ROOT / "CHANGELOG.md", updated)
    warning = unreleased_warning(version, pending_entries)
    if warning is not None:
        print(warning, file=sys.stderr)


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
    print(f"+ {' '.join(cmd)}", flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def capture(cmd: list[str]) -> str:
    print(f"+ {' '.join(cmd)}", flush=True)
    result = subprocess.run(
        cmd,
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return result.stdout.strip()


def command_succeeds(cmd: list[str]) -> bool:
    return (
        subprocess.run(
            cmd,
            cwd=ROOT,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    )


def nix_output_path(system: str) -> str:
    return capture(
        [
            "nix",
            "eval",
            "--accept-flake-config",
            "--raw",
            f".#packages.{system}.default.outPath",
        ]
    )


def nix_derivation_path(system: str) -> str:
    return capture(
        [
            "nix",
            "eval",
            "--accept-flake-config",
            "--raw",
            f".#packages.{system}.default.drvPath",
        ]
    )


def cache_contains(path: str) -> bool:
    # Publication probes the same path before and after pushing, so bypass cached misses.
    return command_succeeds(
        [
            "nix",
            "path-info",
            "--store",
            CACHE_URI,
            "--option",
            "narinfo-cache-negative-ttl",
            "0",
            path,
        ]
    )


def local_store_contains(path: str) -> bool:
    return command_succeeds(["nix", "path-info", path])


def check_cache_system(system: str) -> None:
    if system not in flake_package_systems():
        fail(f"{system} is not advertised by flake.nix")


def build_nix_output(system: str, *, substitutes_only: bool = False) -> str:
    command = [
        "nix",
        "build",
        "--accept-flake-config",
        "--no-link",
        "--print-out-paths",
    ]
    if substitutes_only:
        command.extend(
            [
                "--max-jobs",
                "0",
                "--option",
                "substituters",
                CACHE_URI,
                "--option",
                "trusted-public-keys",
                CACHE_PUBLIC_KEY,
            ]
        )
    command.append(f".#packages.{system}.default")
    output = capture(command)
    paths = output.splitlines()
    if len(paths) != 1:
        fail(f"expected one Nix output for {system}, got {len(paths)}")
    return paths[0]


def cache_summary(*, system: str, derivation: str, output: str, result: str) -> str:
    revision = capture(["git", "rev-parse", "HEAD"])
    return "\n".join(
        [
            f"### Anneal Nix cache: {system}",
            "",
            f"- revision: `{revision}`",
            f"- version: `{cargo_version()}`",
            f"- derivation: `{derivation}`",
            f"- output: `{output}`",
            f"- result: {result}",
            f"- pin: `anneal-{system}` (last {CACHE_PIN_REVISIONS} revisions)",
        ]
    )


def emit_cache_summary(summary: str) -> None:
    print(summary)
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path is not None:
        with Path(summary_path).open("a", encoding="utf-8") as file:
            file.write(f"{summary}\n")


def publish_nix_cache(system: str) -> None:
    check_cache_system(system)
    derivation = nix_derivation_path(system)
    expected_output = nix_output_path(system)
    was_cached = cache_contains(expected_output)
    built_output = build_nix_output(system)
    if built_output != expected_output:
        fail(
            f"Nix output changed during the {system} build: "
            f"expected {expected_output}, got {built_output}"
        )

    run(["cachix", "push", CACHE_NAME, built_output])
    if not cache_contains(built_output):
        fail(f"Cachix did not expose {built_output} after a successful push")
    run(
        [
            "cachix",
            "pin",
            CACHE_NAME,
            f"anneal-{system}",
            built_output,
            "--keep-revisions",
            str(CACHE_PIN_REVISIONS),
        ]
    )
    result = "substituted and republished" if was_cached else "built and published"
    emit_cache_summary(
        cache_summary(
            system=system,
            derivation=derivation,
            output=built_output,
            result=result,
        )
    )


def consume_nix_cache(system: str) -> None:
    check_cache_system(system)
    derivation = nix_derivation_path(system)
    expected_output = nix_output_path(system)
    if not cache_contains(expected_output):
        fail(f"Cachix is missing {system} output {expected_output}")
    if local_store_contains(expected_output):
        fail(
            f"consumer proof started with {expected_output} already in the local store"
        )

    built_output = build_nix_output(system, substitutes_only=True)
    if built_output != expected_output:
        fail(
            f"substitution returned the wrong {system} output: "
            f"expected {expected_output}, got {built_output}"
        )
    run([f"{built_output}/bin/anneal", "--version"])
    emit_cache_summary(
        cache_summary(
            system=system,
            derivation=derivation,
            output=built_output,
            result="substituted from the public cache with local builds disabled",
        )
    )


def verify_release_cache() -> None:
    missing = [
        (system, output)
        for system in flake_package_systems()
        if not cache_contains(output := nix_output_path(system))
    ]
    if missing:
        details = "\n".join(f"  - {system}: {output}" for system, output in missing)
        fail(
            "release outputs are missing from the public Cachix cache:\n"
            f"{details}\n"
            "Wait for the Nix Cache workflow for this commit to succeed, then retry."
        )
    print("all advertised Nix package outputs are present in Cachix")


def verify() -> None:
    versions = {
        **cargo_manifest_versions(),
        **{
            f"Cargo.lock:{name}": version
            for name, version in cargo_lock_versions().items()
        },
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

    package_systems = flake_package_systems()
    for job in ("publish", "consume"):
        cache_systems = cache_workflow_systems(job)
        if cache_systems != package_systems:
            fail(
                f"Nix cache {job} systems do not match flake.nix: "
                f"flake={package_systems}, workflow={cache_systems}"
            )

    if not beads_config_is_public_safe():
        fail(
            ".beads/config.yaml contains a concrete federation.remote; use local configuration instead"
        )

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

    verify_release_cache()

    run(["git", "tag", "-a", tag_name, "-m", tag_name])
    run(["git", "push", "origin", tag_name])
    # Keep the moving release branch pointed at the latest published tag for
    # downstream release-tracking flake inputs.
    run(["git", "branch", "-f", "release", tag_name])
    run(["git", "push", "--force-with-lease", "origin", "release"])


def main() -> None:
    parser = argparse.ArgumentParser(description="Release helper for anneal")
    subparsers = parser.add_subparsers(dest="command", required=True)

    bump_parser = subparsers.add_parser("bump", help="update release versions")
    bump_parser.add_argument("version")

    subparsers.add_parser("verify", help="run release readiness checks")

    tag_parser = subparsers.add_parser("tag", help="create and push a release tag")
    tag_parser.add_argument("version")

    cache_publish_parser = subparsers.add_parser(
        "cache-publish", help="build and publish one native Nix package output"
    )
    cache_publish_parser.add_argument("system")

    cache_consume_parser = subparsers.add_parser(
        "cache-consume", help="prove one Nix package output substitutes"
    )
    cache_consume_parser.add_argument("system")

    subparsers.add_parser(
        "cache-verify", help="verify every advertised Nix package output is cached"
    )

    args = parser.parse_args()
    if args.command == "bump":
        bump(args.version)
    elif args.command == "verify":
        verify()
    elif args.command == "tag":
        tag(args.version)
    elif args.command == "cache-publish":
        publish_nix_cache(args.system)
    elif args.command == "cache-consume":
        consume_nix_cache(args.system)
    else:
        verify_release_cache()


if __name__ == "__main__":
    main()
