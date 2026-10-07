#!/usr/bin/env bash
# OPEN-MODEL-QWEN3 v4 frozen build: sovereign-core 4d4dfd4 (main with sc#293, #296, #295), omega 01f6a74 (= omega.lock,
# omega#331), physics 6d7cf0d (physics.lock), aienos b84c0a6 (aienos.lock). Real compose + GPU libraries, never the stub.
set -u
B=/home/drakestapleton/workspace/oq3-v4-build
cd $B/sc
export AIEN_AIENOS_LOCK_REPO=/home/drakestapleton/workspace/aienos-repo AIEN_OMEGA_DIR=$B/omega AIEN_PHYSICS_DIR=$B/physics CARGO_TARGET_DIR=$B/target
unset AIEN_FORCE_CPU_STUB AIEN_OMEGA_GPU_LIB AIEN_OMEGA_COMPOSE_LIB AIEN_OMEGA_COMPOSE_DIR AIEN_OMEGA_COMPOSE_SHA
cargo build -vv --release -p aien-cli > $B/build-cli.log 2>&1; echo "aien-cli rc=$?"
cargo build -vv --release -p aien-runtime --example np1_reference --example np1_edit_merge > $B/build-examples.log 2>&1; echo "examples rc=$?"
git -C $B/omega rev-parse HEAD; git -C $B/omega status --short | wc -l; git -C $B/physics rev-parse HEAD; git -C $B/sc rev-parse HEAD; git -C $B/sc status --short | wc -l
sha256sum $B/target/release/aien-cli $B/target/release/examples/np1_reference $B/target/release/examples/np1_edit_merge
