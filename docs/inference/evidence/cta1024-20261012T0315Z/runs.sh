#!/bin/bash
# Decode-speed investigation 2026-10-12: fragment-major matmul (omega main 781e98d8) on the Qwen3-8B shapes at m=1.
# Baseline x3, then one variable per run x3, then the mutant parity control. Receipts in runs/. Nothing is killed.
I=~/workspace/investigations/2026-10-12-qwen3-8b-decode-speed; T=$I/omega/build/gpu_matmul_api_test; R=$I/runs
export OMEGA_GPU_MATMUL_SHAPES="1,4096,4096;1,4096,1024;1,4096,12288;1,12288,4096;1,4096,151936"
run() { local name=$1; shift; date -u +%FT%TZ > $R/$name.start; "$T" --timing "$@" --out $R/$name.json > $R/$name.log 2>&1; echo $? > $R/$name.rc; date -u +%FT%TZ > $R/$name.end; }
for r in 1 2 3; do run base-r$r; done
for r in 1 2 3; do run cta512-r$r --cta-budget 512; done
for r in 1 2 3; do run cta1024-r$r --cta-budget 1024; done
for r in 1 2 3; do run unroll2-r$r --unroll 2; done
date -u +%FT%TZ > $R/mutant.start; "$T" --mutant --out $R/mutant.json > $R/mutant.log 2>&1; echo $? > $R/mutant.rc; date -u +%FT%TZ > $R/mutant.end
echo ALL-DONE > $R/all.end
