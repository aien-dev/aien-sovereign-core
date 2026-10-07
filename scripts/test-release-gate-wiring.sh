#!/usr/bin/env bash
# Static guard for where the release gate blocks (merge-owner decision on sovereign-core#268, 2026-10-07):
#   release.yml (tag push): every step that runs scripts/check-release-candidate.sh is fail-closed. The only
#     continue-on-error allowed anywhere in the file is the manual dry run:
#       continue-on-error: ${{ github.event_name == 'workflow_dispatch' && inputs.dry_run }}
#   release-dry-run.yml (every pull request): only the step that runs check-release-candidate.sh on this revision
#     may be non-blocking (main may move ahead of the candidate); the publish guard and the refusal suite
#     (test-release-real-tree.sh) block.
#   scripts/test-release-gate-wiring.sh [release.yml] [release-dry-run.yml]
# Mutation note: the self-checks below inject `continue-on-error: true` into the tag gate step, weaken the allowed
# expression, drop the tag gate, and make the refusal suite non-blocking; each must turn this guard red.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REL="${1:-$ROOT/.github/workflows/release.yml}"
DRY="${2:-$ROOT/.github/workflows/release-dry-run.yml}"
ALLOWED="continue-on-error: \${{ github.event_name == 'workflow_dispatch' && inputs.dry_run }}"

# One line per step: "<continue-on-error value or NONE>\t<step text, joined>". A step starts at "- name:" or
# "- uses:" and ends at the next step, a job key (two-space indent) or a top-level key.
steps() {
    awk '
        function flush() { if (inside) printf "%s\t%s\n", (coe == "" ? "NONE" : coe), run; inside = 0; coe = ""; run = "" }
        /^[[:space:]]*- (name|uses):/ { flush(); inside = 1; next }
        /^([a-z]|  [a-z])[a-zA-Z_-]*:/ { flush() }
        inside && /^[[:space:]]*continue-on-error:/ { sub(/^[[:space:]]*/, ""); coe = $0; next }
        inside { run = run " " $0 }
        END { flush() }
    ' "$1"
}

check_release() {
    local f="$1" gates bad
    gates="$(steps "$f" | grep -c 'check-release-candidate\.sh' || true)"
    [[ "$gates" -ge 2 ]] || { echo "FAIL: $f runs check-release-candidate.sh in $gates step(s), expected the tree and package gates" >&2; return 1; }
    bad="$(grep -nE '^[[:space:]]*continue-on-error:' "$f" | grep -vF "$ALLOWED" || true)"
    [[ -z "$bad" ]] || { echo "FAIL: $f has a continue-on-error other than the manual dry run:" >&2; echo "$bad" >&2; return 1; }
    grep -qE '^[[:space:]]+tags:' "$f" || { echo "FAIL: $f no longer runs on tag push" >&2; return 1; }
}

check_dry() {
    local f="$1" line coe run
    local saw_tree=0 saw_suite=0 saw_guard=0
    while IFS=$'\t' read -r coe run; do
        if [[ "$run" == *test-release-real-tree.sh* ]]; then
            saw_suite=1; [[ "$coe" == NONE ]] || { echo "FAIL: $f refusal suite is not blocking ($coe)" >&2; return 1; }
        elif [[ "$run" == *test-dry-run-workflow.sh* || "$run" == *test-release-gate-wiring.sh* ]]; then
            saw_guard=1; [[ "$coe" == NONE ]] || { echo "FAIL: $f static guard is not blocking ($coe)" >&2; return 1; }
        elif [[ "$run" == *check-release-candidate.sh* ]]; then
            saw_tree=1
        elif [[ "$coe" != NONE ]]; then
            echo "FAIL: $f has a non-blocking step other than the tree report: $run" >&2; return 1
        fi
    done < <(steps "$f")
    [[ $saw_tree == 1 && $saw_suite == 1 && $saw_guard == 1 ]] || { echo "FAIL: $f lacks the tree report, refusal suite or static guard" >&2; return 1; }
}

check_release "$REL"
check_dry "$DRY"

# self-checks: each mutation of a copy must be refused
M="$(mktemp -d)"; trap 'rm -rf "$M"' EXIT
must_refuse() { local what="$1" kind="$2" file="$3"
    if "$kind" "$file" 2>/dev/null; then echo "FAIL: guard does not catch: $what" >&2; exit 1; fi; }
sed '/^[[:space:]]*id: gate$/a\        continue-on-error: true' "$REL" > "$M/r1"
must_refuse "continue-on-error: true on the tag gate" check_release "$M/r1"
sed "s/github.event_name == 'workflow_dispatch' \&\& inputs.dry_run/inputs.dry_run || true/" "$REL" > "$M/r2"
must_refuse "weakened dry-run expression" check_release "$M/r2"
grep -v 'run: scripts/check-release-candidate.sh$' "$REL" > "$M/r3"
must_refuse "tree gate removed" check_release "$M/r3"
sed '/run: bash scripts\/test-release-real-tree.sh/i\        continue-on-error: true' "$DRY" > "$M/d1"
must_refuse "refusal suite made non-blocking" check_dry "$M/d1"
echo "ok: tag release gate is fail-closed (only the manual dry run may continue); dry run blocks on the publish guard and the refusal suite"
