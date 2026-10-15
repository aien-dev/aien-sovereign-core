#!/usr/bin/env bash
# #514 chip qualification, run under a quietlock hold. Never kills anything it did not start.
set -u
R=~/workspace/sc-pkv514
E=$R/docs/inference/evidence/pkv514-chip-20261014
T=$R/target/release/deps/kv_persist_chip-78f753803b9292f4
BIN=$R/target/release/aien-serve-http
export OWN30B_MODEL_DIR=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
mkdir -p "$E"
{
  echo "commit $(git -C $R rev-parse HEAD) dirty_files=$(git -C $R status --porcelain | grep -v '^?? docs/inference/evidence/' | wc -l)"
  echo "binary sha256 $(sha256sum $BIN | cut -d' ' -f1)"; echo "test sha256 $(sha256sum $T | cut -d' ' -f1)"
  echo "omega $(git -C ~/workspace/pkv514-pins/omega rev-parse HEAD) driver $(nvidia-smi --query-gpu=driver_version --format=csv,noheader) kernel $(uname -r)"
  echo "start $(date -u +%FT%TZ)"; grep MemAvailable /proc/meminfo
} > "$E/ground.txt"
step() { local name=$1; shift; echo "== $name $(date -u +%T)"; quietlock hold --owner PKV514 --minutes 20 --reason "#514 PKV chip $name" -- "$@" > "$E/$name.log" 2>&1; local rc=$?; echo "$name rc=$rc" | tee -a "$E/rc.txt"; grep -E "^SNAP|^test |test result|panicked|PASS|FAIL|VERDICT" "$E/$name.log" | head -40; }
step item1-3-4-roundtrip env PKV_CHIP_SCRATCH=/tmp $T --ignored --exact fused_cache_round_trip_cost_and_restore_latency_on_the_chip --nocapture
step item2a-inprocess-restore $T --ignored --exact restored_turn_two_equals_the_uninterrupted_turn_two_on_the_chip --nocapture
step item2b-chip-check env PKV_CHECK_PORT=18098 PKV_CHECK_EPHEMERAL_KEY=1 $R/scripts/own30b-pkv-chip-check.sh $E/chip-check $BIN
step item5-two-process env PKV_CHECK_PORT=18098 PKV_CHECK_PORT2=18099 PKV_CHECK_EPHEMERAL_KEY=1 $R/scripts/own30b-pkv-two-process.sh $E/two-process $BIN
echo "end $(date -u +%FT%TZ)" >> "$E/ground.txt"
