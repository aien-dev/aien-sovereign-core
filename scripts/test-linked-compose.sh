#!/usr/bin/env bash
# Mandatory gate for the tests that need the REAL omega composition library.
#
# The normal `cargo test --workspace` CI step builds the stub (no omega checkout), where every
# compose-linked test is `#[ignore]`, so a failure in them never shows (sovereign-core #306: c25 failed
# on main for days behind the stub). This script builds the library from an omega checkout at
# omega.lock and RUNS those tests, ignored ones included. It fails if the library is not really linked
# or if no linked test ran, so it cannot pass vacuously.
#
# Usage: scripts/test-linked-compose.sh OMEGA_DIR PHYSICS_DIR AIENOS_REPO
#   OMEGA_DIR    omega checkout whose HEAD equals omega.lock
#   PHYSICS_DIR  physics checkout at omega's physics.lock
#   AIENOS_REPO  aienos clone holding omega's aienos.lock commit
# Env: CARGO_TARGET_DIR (optional), LINKED_REPEAT (default 3: how many times the approved_attacks suite runs).
# CPU only: never sets AIEN_OMEGA_DIR (that would link the GPU engine) and unsets the stub/fallback switches.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OMW=$(cd "${1:?OMEGA_DIR}" && pwd); PHW=$(cd "${2:?PHYSICS_DIR}" && pwd); AO=$(cd "${3:?AIENOS_REPO}" && pwd)
die() { echo "test-linked-compose: $*" >&2; exit 3; }
pin=$(tr -d '[:space:]' <"$ROOT/omega.lock")
[ "$(git -C "$OMW" rev-parse HEAD)" = "$pin" ] || die "omega HEAD != omega.lock $pin"
[ "$(git -C "$PHW" rev-parse HEAD)" = "$(tr -d '[:space:]' <"$OMW/physics.lock")" ] || die "physics HEAD != omega physics.lock"
[ "$(git -C "$AO" rev-parse HEAD)" = "$(tr -d '[:space:]' <"$OMW/aienos.lock")" ] || die "aienos HEAD != omega aienos.lock"
REPEAT=${LINKED_REPEAT:-3}
export AIEN_OMEGA_COMPOSE_DIR="$OMW" AIEN_PHYSICS_DIR="$PHW" AIEN_AIENOS_LOCK_REPO="$AO"
unset AIEN_OMEGA_DIR AIEN_OMEGA_GPU_LIB AIEN_OMEGA_COMPOSE_LIB AIEN_OMEGA_COMPOSE_SHA AIEN_FORCE_CPU_STUB AIEN_DEV_FALLBACK
cd "$ROOT"
LOG=$(mktemp); trap 'rm -f "$LOG"' EXIT
run() {  # run a test target with --include-ignored; the tests are serial (one process-wide compose home)
    cargo test "$@" -- --include-ignored --test-threads=1 2>&1 | tee -a "$LOG"
}
: >"$LOG"
run -p aien-omega-compose --test compose
for i in $(seq "$REPEAT"); do
    echo "== approved_attacks run $i/$REPEAT"
    run -p aien-runtime --test approved_attacks_test
done
# Not vacuous: the library was linked (no stub warning) and the tests really ran.
if grep -q "building the stub" "$LOG"; then die "a stub was built: the composition library is not linked"; fi
ran=$(grep -E '^test result: ok\. [0-9]+ passed' "$LOG" | awk '{s += $4} END {print s + 0}')
[ "$ran" -ge $((34 * REPEAT)) ] || die "only $ran linked tests passed (expected at least $((34 * REPEAT)) from approved_attacks)"
if grep -E '^test result:.* [1-9][0-9]* ignored' "$LOG" >/dev/null; then die "tests were still ignored"; fi
echo "test-linked-compose: PASS ($ran linked tests passed, approved_attacks x$REPEAT)"
