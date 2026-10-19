#!/usr/bin/env bash
# Shared direct-Git boundary. Call require_git_root before using git_at_root.
git_at_root() {
    local root="$1"; shift
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE -u GIT_COMMON_DIR \
        GIT_OPTIONAL_LOCKS=0 git -C "$root" "$@"
}

require_git_root() {
    local root reported
    root="$(cd "$1" && pwd -P)" || return 1
    if ! reported="$(git_at_root "$root" rev-parse --show-toplevel)"; then
        printf 'error: refusing Git at %s; use the colocated checkout (expected .git at %s)\n' "$root" "$root" >&2
        return 1
    fi
    if [[ "$(cd "$reported" && pwd -P)" != "$root" ]]; then
        printf 'error: refusing Git at %s: toplevel is %s; use the colocated checkout (expected .git at %s)\n' \
            "$root" "$reported" "$root" >&2
        return 1
    fi
}
