#!/bin/bash
# Baseline vs pin bump under the SAME machine state (MAX 8B experiment live on the GPU): the #524 binary
# (omega 62833658, ~/handoffs/main-bin-9557e0c1) and this branch's binary (omega 4799f9eb), alternated
# old, new, old, new with the gates' check-full command. One variable: the binary (omega pin).
set -u
EV=${EV:?}; mkdir -p "$EV"
CK=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
G=$HOME/workspace/sc-pinbump/docs/inference/evidence/own30b-f-20261013T1745Z/golden.json
TOK="--prompts 1,2,3,4 --new 16 --layers 0,23,47 --weights fp8 --attn fused --router device --lm-head gemv --prefill auto --force-memory"
OLD=$HOME/handoffs/main-bin-9557e0c1; NEW=$HOME/handoffs/pin-bin-afe59ae1
run() { local name=$1 B=$2
  { echo "queued $(date -u +%FT%TZ) binary $B/own30b_check sha256 $(sha256sum $B/own30b_check | cut -d' ' -f1)"
    echo "max-procs $(pgrep -fc max-env/bin/python)"; nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader; free -g | head -2; } > "$EV/$name.start"
  quietlock hold --owner PINBUMP --minutes 20 --reason "pin bump baseline A/B $name" -- \
    flock /tmp/aien-gb10.lock env AIEN_OMEGA_SPIN_US=2000 $B/own30b_check chip $CK $G $EV/$name $TOK > "$EV/$name.raw" 2> "$EV/$name.err"
  local rc=$?; echo $rc > "$EV/$name.rc"; { echo "ended $(date -u +%FT%TZ)"; nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader; } > "$EV/$name.end"
  echo "$name rc=$rc $(grep -o '^TOKENS p1[^=]*= [0-9.]* ms/token = [0-9.]* tok/s' $EV/$name.raw)"
}
run old-1 $OLD; run new-1 $NEW; run old-2 $OLD; run new-2 $NEW
echo "ab done $(date -u +%FT%TZ)"
