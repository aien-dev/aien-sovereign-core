#!/usr/bin/env bash
# Verify the branch guard in both contexts using throwaway git repositories.
set -euo pipefail
GUARD="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/preflight-branch-guard.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/aien-guard-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
git init -q -b main . && git -c user.name=t -c user.email=t@t commit -q --allow-empty -m init

expect() { # want(0|1) description env...
    local want="$1" desc="$2"; shift 2
    local rc=0
    env "$@" bash "$GUARD" >/dev/null 2>&1 || rc=$?
    [ "$rc" -eq "$want" ] || { echo "FAIL: $desc (exit $rc, wanted $want)"; exit 1; }
    echo "ok: $desc"
}

expect 1 "developer on main is refused"            AIEN_PREFLIGHT_CONTEXT=developer
expect 1 "default context on main is refused"      AIEN_PREFLIGHT_UNUSED=1
expect 0 "merged-tree on main is accepted"         AIEN_PREFLIGHT_CONTEXT=merged-tree
expect 1 "unknown context is refused"              AIEN_PREFLIGHT_CONTEXT=mereged-tree
git checkout -q -b feat/x
expect 0 "developer on feature branch is accepted" AIEN_PREFLIGHT_CONTEXT=developer
git checkout -q --detach
expect 0 "developer on detached PR merge ref is accepted" AIEN_PREFLIGHT_CONTEXT=developer
echo "PREFLIGHT_BRANCH_GUARD_TESTS_PASS"
