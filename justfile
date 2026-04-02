# anneal — command runner

default:
    @just --list

# All checks: fmt + clippy + test (with timing)
check:
    #!/usr/bin/env bash
    set -euo pipefail
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
    _t clippy  cargo clippy --all-targets
    _t test    cargo test
    echo "--------------------" >&2
    total=0
    while read -r _ ms; do total=$((total + ms)); done < /tmp/anneal-check-times.$$
    printf "  %-12s %d.%02ds\n" "total:" "$((total / 1000))" "$(( (total % 1000) / 10 ))" >&2
    rm -f /tmp/anneal-check-times.$$

# Format (modify in place)
fmt:
    cargo fmt

# Format check (no modification)
fmt-check:
    cargo fmt --check

# Clippy with workspace lints
lint:
    cargo clippy --all-targets

# Run tests
test:
    cargo test

# Smoke-test the exported Home Manager module
test-home-manager-module:
    bash scripts/test-home-manager-module.sh

# Release build
build:
    cargo build --release

# Update release versions in Cargo.toml, Cargo.lock, flake.nix, and scaffold CHANGELOG.md
release-bump version:
    python3 scripts/release.py bump {{version}}

# Release readiness checks: versions, changelog, targets, quality gate, release binary
release-verify:
    python3 scripts/release.py verify

# Create and push an annotated release tag (triggers GitHub release workflow)
release-tag version:
    python3 scripts/release.py tag {{version}}
