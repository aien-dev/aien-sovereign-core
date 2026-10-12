#!/bin/bash
# Builds the smoke example with the pinned real GB10 engine, as scripts/smoke_qwen3_8b.sh build gb10 does.
W=~/workspace/.worktrees/cta1024; cd $W || exit 2
unset AIEN_FORCE_CPU_STUB AIEN_OMEGA_GPU_LIB AIEN_OMEGA_COMPOSE_LIB AIEN_OMEGA_COMPOSE_DIR AIEN_OMEGA_COMPOSE_SHA AIEN_DEV_FALLBACK AIEN_BACKEND
bash scripts/pins.sh --gpu $W/target-gb10/pins > $W/target-gb10-pins.txt 2>&1 || { echo "pins rc=$?"; tail -5 $W/target-gb10-pins.txt; exit 3; }
. $W/target-gb10/pins/env.sh
CARGO_TARGET_DIR=$W/target-gb10 cargo build --release -p aien-runtime --example qwen3_8b_smoke > $W/target-gb10-build.log 2>&1; rc=$?
echo "build rc=$rc"; grep -E "building the (CPU )?stub|developer override|^error" $W/target-gb10-build.log | head -3
ls -la $W/target-gb10/release/examples/qwen3_8b_smoke; echo build-gb10-done
