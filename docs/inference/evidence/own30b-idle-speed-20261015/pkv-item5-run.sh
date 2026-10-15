#!/usr/bin/env bash
# #514 persistent-KV item 5 (two 30B processes at once, ~71 GB) on an IDLE GB10, pinned binary
# ~/handoffs/pin-bin-afe59ae1 (omega 4799f9eb), script from main ca695de9. Same step as
# pkv514-chip-20261014/run-all.sh item5-two-process, under one quietlock hold; nothing is killed.
set -u
R=$HOME/workspace/hive-worktrees/sc-idle-speed
E=${E:?evidence dir}
BIN=$HOME/handoffs/pin-bin-afe59ae1/aien-serve-http
export OWN30B_MODEL_DIR=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
mkdir -p "$E"
{
  echo "commit $(git -C $R rev-parse HEAD) dirty_files=$(git -C $R status --porcelain | grep -v '^?? docs/inference/evidence/' | wc -l)"
  echo "binary sha256 $(sha256sum $BIN | cut -d' ' -f1)"
  echo "omega $(cat $R/omega.lock) driver $(nvidia-smi --query-gpu=driver_version --format=csv,noheader) kernel $(uname -r)"
  echo "start $(date -u +%FT%TZ)"; grep MemAvailable /proc/meminfo
  nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv
} > "$E/ground.txt"
step() { local name=$1; shift; echo "== $name $(date -u +%T)"; quietlock hold --owner IDLESPEED --minutes 20 --reason "PKV item 5 $name on omega 4799f9eb (idle GPU)" -- "$@" > "$E/$name.log" 2>&1; local rc=$?; echo "$name rc=$rc" | tee -a "$E/rc.txt"; grep -E "^SNAP|^test |test result|panicked|PASS|FAIL|VERDICT" "$E/$name.log" | head -40; }
step item5-two-process env PKV_CHECK_PORT=18098 PKV_CHECK_PORT2=18099 PKV_CHECK_EPHEMERAL_KEY=1 $R/scripts/own30b-pkv-two-process.sh $E/two-process $BIN
echo "end $(date -u +%FT%TZ)" >> "$E/ground.txt"
