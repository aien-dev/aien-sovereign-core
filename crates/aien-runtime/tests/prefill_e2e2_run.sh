#!/bin/sh
# Driver for the PREFILL-E2E-2 gates (ADR 0028, run by `aien-test run`).
#
# The forge lines for PREFILL-E2E-2 are shell chains with environment
# variables. A gate manifest has no shell and no environment (ADR 0028 P6), so
# every gate runs this one script from the repository root with a fixed
# argument list. The commands below are the forge lines, unchanged, minus the
# HEAD pin (aien-test pins and records the commit itself).
#
#   prefill_e2e2_run.sh main
#       cargo fmt --all -- --check, then cargo clippy -p aien-runtime
#       --all-targets -- -D warnings, then the ignored real-checkpoint test in
#       release mode. Stops at the first failing stage with that stage's exit
#       status (the forge uses &&).
#   prefill_e2e2_run.sh mutant ID MARKER
#       Apply prefill_e2e2_mutants/mutant_ID.patch, run the same test, restore
#       the tree with `git checkout -- .`, and succeed only when the test
#       failed and its output holds MARKER (the forge KILLED rule). A mutant
#       that survives, or fails with another message, exits 1.
#
# Observations ("AIEN-OBS NAME JSON" lines, see tools/aien-test) tell the
# runner what happened; the verdict is derived from the manifest, not here.
#
# Environment (defaults are the paths the forge lines use):
#   AIEN_E2E_CHECKPOINT  TinyLlama-1.1B-Chat-v1.0 directory
#   AIEN_E2E2_OUT_DIR    receipts and mutant logs; must be outside the repository
set -u

here=$(cd "$(dirname "$0")" && pwd) || exit 3
cd "$here/../../.." || exit 3

ckpt=${AIEN_E2E_CHECKPOINT:-$HOME/models/TinyLlama-1.1B-Chat-v1.0}
out=${AIEN_E2E2_OUT_DIR:-$HOME/workspace/test-queue-logs}
sha8=$(git rev-parse HEAD | cut -c1-8)
mkdir -p "$out" || exit 3

# The leading newline keeps the line whole when a test left a partial line.
obs() {
	printf '\nAIEN-OBS %s %s\n' "$1" "$2"
}

run_test() {
	AIEN_E2E_CHECKPOINT="$ckpt" AIEN_E2E2_RECEIPT="$1" \
		cargo test -p aien-runtime --release --test prefill_e2e2 -- --ignored --nocapture
}

run_main() {
	cargo fmt --all -- --check
	rc=$?
	obs fmt_rc "$rc"
	[ "$rc" -eq 0 ] || exit "$rc"
	cargo clippy -p aien-runtime --all-targets -- -D warnings
	rc=$?
	obs clippy_rc "$rc"
	[ "$rc" -eq 0 ] || exit "$rc"
	run_test "$out/AIENTEST-DOGFOOD-receipt-$sha8.json"
	rc=$?
	obs test_rc "$rc"
	exit "$rc"
}

run_mutant() {
	id=${1:-}
	marker=${2:-}
	case "$id" in
	m[1-8]) ;;
	*)
		echo "bad mutant id: $id" >&2
		exit 2
		;;
	esac
	case "$marker" in
	'' | *[!A-Z_]*)
		echo "bad marker: $marker" >&2
		exit 2
		;;
	esac
	patch="crates/aien-runtime/tests/prefill_e2e2_mutants/mutant_$id.patch"
	lg="$out/AIENTEST-DOGFOOD-$sha8-$id.log"
	git apply "$patch" || exit 3
	trap 'git checkout -- .; exit 143' HUP INT TERM
	run_test "$out/AIENTEST-DOGFOOD-receipt-$sha8-$id.json" >"$lg" 2>&1
	rc=$?
	git checkout -- .
	trap - HUP INT TERM
	echo "LEG $id rc=$rc"
	grep -n "$marker" "$lg" | head -3
	obs mutant_test_rc "$rc"
	if [ "$rc" -ne 0 ] && grep -q "$marker" "$lg"; then
		obs mutant_killed true
		echo "MUTANT $id KILLED"
		exit 0
	fi
	obs mutant_killed false
	echo "MUTANT $id SURVIVED or wrong failure"
	tail -5 "$lg"
	exit 1
}

case "${1:-}" in
main) run_main ;;
mutant)
	shift
	run_mutant "$@"
	;;
*)
	echo "usage: $0 main | mutant ID MARKER" >&2
	exit 2
	;;
esac
