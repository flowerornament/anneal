#!/usr/bin/env bash
# Deterministic installer subprocess controls; no network or real credentials.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
installer="${1:-$root/install.sh}"
bash -n "$installer"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin"
export PATH="$scratch/bin:$PATH"
export INSTALL_DIR="$scratch/install"
export INSTALLER_CALLS="$scratch/calls"
export INSTALLER_VIOLATIONS="$scratch/violations"
export INSTALLER_TEST_TOKEN="anneal_synthetic_test_token"
unset ANNEAL_GITHUB_TOKEN GITHUB_TOKEN

cat > "$scratch/bin/uname" <<'EOF'
#!/usr/bin/env bash
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) exit 1 ;; esac
EOF
cat > "$scratch/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
violation() { echo "$1" >&2; echo violation >> "$INSTALLER_VIOLATIONS"; exit 99; }
printf 'call\n' >> "$INSTALLER_CALLS"
config=false
for arg in "$@"; do
    case "$arg" in
        *"$INSTALLER_TEST_TOKEN"*) violation 'credential in argv' ;;
        --config) config=true ;;
    esac
done
authenticated=false
if [ "$config" = true ]; then
    input="$(cat)"
    if [ "$input" = "header = \"Authorization: Bearer $INSTALLER_TEST_TOKEN\"" ]; then
        authenticated=true
    else
        violation 'unexpected curl config'
    fi
fi
case "${*: -1}" in
    https://api.github.com/repos/flowerornament/anneal/releases/latest) ;;
    *)
        [ "$authenticated" = false ] || violation 'credential sent to download'
        echo 'curl: (22) The requested URL returned error: 404' >&2
        exit 22
        ;;
esac
case "$INSTALLER_MODE" in
    auth-required)
        [ "$authenticated" = true ] || { echo 'curl: (22) The requested URL returned error: 403' >&2; exit 22; }
        ;;
    anonymous)
        [ "$authenticated" = false ] || violation 'unexpected ambient authentication'
        ;;
    lookup-fails)
        echo 'curl: (22) The requested URL returned error: 404' >&2
        exit 22
        ;;
    missing-tag) echo '{}' ; exit 0 ;;
    *) violation 'unknown test mode' ;;
esac
printf '{"tag_name": "v0.26.2"}\n'
EOF
chmod +x "$scratch/bin/uname" "$scratch/bin/curl"

run_case() {
    local name="$1" expected="$2" status=0
    shift 2
    : > "$INSTALLER_CALLS"
    : > "$INSTALLER_VIOLATIONS"
    "$@" > "$scratch/stdout" 2> "$scratch/stderr" || status=$?
    # A pipeline can turn curl's sentinel exit into grep's exit 1.
    [ ! -s "$INSTALLER_VIOLATIONS" ] || { echo "$name: curl contract violation" >&2; exit 1; }
    if grep -Fq "$INSTALLER_TEST_TOKEN" "$scratch/stdout" "$scratch/stderr"; then
        echo "$name: credential leaked" >&2
        exit 1
    fi
    if [ "$expected" = success ]; then
        [ "$status" -eq 0 ] || { echo "$name: expected success, got $status" >&2; exit 1; }
    else
        [ "$status" -ne 0 ] && [ "$status" -ne 99 ] || { echo "$name: expected installer failure, got $status" >&2; exit 1; }
        [ ! -e "$INSTALL_DIR/anneal" ] || { echo "$name: installed on failure" >&2; exit 1; }
    fi
    printf 'installer: %s ok (exit %s)\n' "$name" "$status"
}

run_case unauthenticated-403 failure env INSTALLER_MODE=auth-required bash "$installer" --dry-run
run_case authenticated-latest success env INSTALLER_MODE=auth-required ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --dry-run
grep -Fq 'release: v0.26.2' "$scratch/stdout"
# shellcheck disable=SC2016 # $1 belongs to the child bash, not this shell.
run_case authenticated-piped success env INSTALLER_MODE=auth-required ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash -c 'cat "$1" | bash -s -- --dry-run' _ "$installer"
run_case authenticated-xtrace success env INSTALLER_MODE=auth-required ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash -x "$installer" --dry-run
run_case ambient-token-ignored success env INSTALLER_MODE=anonymous GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --dry-run
run_case anonymous-latest success env INSTALLER_MODE=anonymous bash "$installer" --dry-run
run_case real-lookup-failure failure env INSTALLER_MODE=lookup-fails ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --dry-run
run_case missing-tag failure env INSTALLER_MODE=missing-tag ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --dry-run
run_case print-target success env INSTALLER_MODE=lookup-fails ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --print-target
[ ! -s "$INSTALLER_CALLS" ]
run_case tagged-dry-run success env INSTALLER_MODE=lookup-fails ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --tag nonexistent-test-tag --dry-run
[ ! -s "$INSTALLER_CALLS" ]
run_case missing-tag-download failure env INSTALLER_MODE=lookup-fails ANNEAL_GITHUB_TOKEN="$INSTALLER_TEST_TOKEN" bash "$installer" --tag nonexistent-test-tag
printf 'installer controls passed\n'
