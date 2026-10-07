#!/usr/bin/env bash
# Pre-run gate on sovereign-core 905fdfc (ACCEPTANCE-v2 Section 2).
set -u
cd /home/drakestapleton/workspace/oq3-v2-build/sc
export AIEN_AIENOS_LOCK_REPO=/home/drakestapleton/workspace/aienos-repo AIEN_OMEGA_DIR=/home/drakestapleton/workspace/investigations/2026-10-07-qwen3-decode-matmul/omega-main AIEN_PHYSICS_DIR=/home/drakestapleton/workspace/investigations/2026-10-07-qwen3-decode-matmul/physics CARGO_TARGET_DIR=/home/drakestapleton/workspace/oq3-v2-build/target-gate
cargo fmt --all --check > /home/drakestapleton/workspace/oq3-v2-build/gate-fmt.log 2>&1; echo "fmt rc=$?"
cargo clippy --workspace --all-targets -- -D warnings > /home/drakestapleton/workspace/oq3-v2-build/gate-clippy.log 2>&1; echo "clippy rc=$?"
AIEN_FORCE_CPU_STUB=1 cargo test -p aien-runtime -p aien-cli > /home/drakestapleton/workspace/oq3-v2-build/gate-test.log 2>&1; echo "test rc=$?"
grep -E '^test result' /home/drakestapleton/workspace/oq3-v2-build/gate-test.log | awk '{p+=$4; f+=$6} END {print "tests passed="p" failed="f}'
