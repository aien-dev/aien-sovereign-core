#!/usr/bin/env bash
# Pre-run gate on sovereign-core 8f3e8c8 (ACCEPTANCE-v3 Section 7).
set -u
B=/home/drakestapleton/workspace/oq3-v3-build
cd $B/sc
export AIEN_AIENOS_LOCK_REPO=/home/drakestapleton/workspace/aienos-repo AIEN_OMEGA_DIR=$B/omega AIEN_PHYSICS_DIR=$B/physics CARGO_TARGET_DIR=$B/target-gate
cargo fmt --all --check > $B/gate-fmt.log 2>&1; echo "fmt rc=$?"
cargo clippy --workspace --all-targets -- -D warnings > $B/gate-clippy.log 2>&1; echo "clippy rc=$?"
AIEN_FORCE_CPU_STUB=1 cargo test -p aien-runtime -p aien-cli > $B/gate-test.log 2>&1; echo "test rc=$?"
grep -E '^test result' $B/gate-test.log | awk '{p+=$4; f+=$6} END {print "tests passed="p" failed="f}'
