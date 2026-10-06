#!/usr/bin/env bash
# Fail closed unless this tree may be released as the candidate named in release/candidate.toml.
#   scripts/check-release-candidate.sh            check; prints "candidate=<id>" on success
#   scripts/check-release-candidate.sh --model F  also write the [model] table to file F
# Checks: candidate is set and is not "unknown"; omega.lock equals omega-commit.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
M=release/candidate.toml
[[ -f "$M" ]] || { echo "release gate: $M is missing" >&2; exit 1; }
get() { sed -n "s/^$1 *= *\"\(.*\)\"\$/\1/p" "$M" | head -1; }
cand="$(get candidate)"; omega="$(get omega-commit)"
[[ -n "$cand" && "$cand" != unknown ]] || { echo "release gate: candidate in $M is missing or 'unknown'" >&2; exit 1; }
[[ "$omega" =~ ^[0-9a-f]{40}$ ]] || { echo "release gate: omega-commit in $M is not a full commit hash" >&2; exit 1; }
lock="$(tr -d '[:space:]' < omega.lock)"
[[ "$lock" == "$omega" ]] || { echo "release gate: omega.lock is $lock but candidate $cand names omega $omega. This tree is not $cand and cannot be released as it." >&2; exit 1; }
grep -q '^\[model\]' "$M" || { echo "release gate: no [model] table in $M" >&2; exit 1; }
if [[ "${1:-}" == --model ]]; then sed -n '/^\[model\]/,$p' "$M" | sed '/^$/q' > "${2:?file}"; fi
echo "candidate=$cand"
