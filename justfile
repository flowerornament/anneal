# anneal — command runner

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# All checks: fmt + clippy + test (with timing)
[group('check')]
check:
    #!/usr/bin/env bash
    set -euo pipefail
    # Force console-crate ANSI emission during tests so attribute
    # leaks (e.g. Style::new().bold() on a color-disabled path) can't
    # get silently neutered by non-TTY detection. A Nix build sandbox
    # caught this once; our local gate now reproduces those conditions.
    export CLICOLOR_FORCE=1
    _ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time()*1000'; }
    _t() {
        local label="$1"; shift
        local start end elapsed
        start=$(_ms)
        "$@"
        end=$(_ms)
        elapsed=$((end - start))
        printf "  %-12s %d.%02ds\n" "$label:" "$((elapsed / 1000))" "$(( (elapsed % 1000) / 10 ))" >&2
        echo "$label $elapsed" >> /tmp/anneal-check-times.$$
    }
    rm -f /tmp/anneal-check-times.$$
    echo "--- quality gate ---" >&2
    _t fmt     cargo fmt --check
    _t install bash -n install.sh
    _t arch    just check-arch
    _t clippy  cargo clippy --all-targets
    _t test    cargo test
    # Architecture fitness functions (offline, fast): unused deps + crate-DAG /
    # license / source bans. Guarded so a tool-less env degrades gracefully —
    # run `just audit` for the full check incl. security advisories.
    if command -v cargo-machete >/dev/null 2>&1 && command -v cargo-deny >/dev/null 2>&1; then
        _t machete cargo machete
        _t deny    cargo deny check bans licenses sources
    else
        echo "  (fitness: cargo-machete/cargo-deny not installed; run 'just audit')" >&2
    fi
    echo "--------------------" >&2
    total=0
    while read -r _ ms; do total=$((total + ms)); done < /tmp/anneal-check-times.$$
    printf "  %-12s %d.%02ds\n" "total:" "$((total / 1000))" "$(( (total % 1000) / 10 ))" >&2
    rm -f /tmp/anneal-check-times.$$

# Architecture fitness functions: unused deps + crate-DAG/license/source/advisory bans
[group('check')]
audit:
    cargo machete
    cargo deny check

# Architecture fitness functions that are cheap enough for every local check.
[group('check')]
check-arch:
    python3 scripts/check-arch.py

# Format source files (modify in place)
[group('check')]
fmt:
    cargo fmt

# Verify formatting without modifying
[group('check')]
fmt-check:
    cargo fmt --check

# Clippy with workspace lints
[group('check')]
lint:
    cargo clippy --all-targets

# Run tests
[group('check')]
test:
    cargo test

# Smoke-test the exported Home Manager module
[group('check')]
test-home-manager-module:
    bash scripts/test-home-manager-module.sh

# Release build
[group('build')]
build:
    cargo build --release

# Update release versions in Cargo.toml, Cargo.lock, flake.nix, and scaffold CHANGELOG.md
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.14.1')]
release-bump version:
    python3 scripts/release.py bump {{quote(version)}}

# Release readiness checks: versions, changelog, targets, quality gate, release binary
[group('release')]
release-verify:
    python3 scripts/release.py verify

# Tag and publish a release: pushes the annotated tag, force-updates the `release` branch, triggers the GitHub release workflow
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.14.1')]
[confirm("This will tag, force-update origin/release, and trigger the public GitHub release workflow. Continue?")]
release-tag version:
    python3 scripts/release.py tag {{quote(version)}}
