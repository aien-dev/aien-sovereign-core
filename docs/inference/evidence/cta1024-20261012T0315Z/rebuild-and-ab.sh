#!/bin/bash
W=~/workspace/.worktrees/cta1024; E=$W/docs/inference/evidence/cta1024-20261012T0315Z; cd $W || exit 2
unset AIEN_FORCE_CPU_STUB AIEN_OMEGA_GPU_LIB AIEN_OMEGA_COMPOSE_LIB AIEN_OMEGA_COMPOSE_DIR AIEN_OMEGA_COMPOSE_SHA AIEN_DEV_FALLBACK AIEN_BACKEND
. $W/target-gb10/pins/env.sh
CARGO_TARGET_DIR=$W/target-gb10 cargo build --release -p aien-runtime --example qwen3_8b_smoke > $W/target-gb10-build2.log 2>&1; rc=$?
echo "build rc=$rc $(grep -cE 'building the (CPU )?stub|developer override' $W/target-gb10-build2.log) stub-lines"
[ $rc -eq 0 ] || { grep -A6 "^error" $W/target-gb10-build2.log | head -20; echo chain-done; exit 1; }
sha256sum $W/target-gb10/release/examples/qwen3_8b_smoke > $E/smoke-binary.sha256
date -u +%FT%TZ > $E/ab-hold.start
quietlock hold --owner ade3c837-ab-smoke --minutes 20 --reason "Qwen3-8B smoke A/B CTA 256 vs 1024 with daemon spin, under 5 min, sc#384" -- bash $E/ab-smoke.sh > $E/ab-smoke.out 2>&1
echo "hold rc=$?"; cat $E/ab-smoke.out; echo chain-done
