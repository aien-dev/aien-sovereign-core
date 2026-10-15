#!/bin/bash
# OWN-30B speed gates on an IDLE GB10 (MAX 8B experiment paused, :18097 stopped), merged main ca695de9,
# omega 4799f9eb, pinned binaries ~/handoffs/pin-bin-afe59ae1. Same commands as
# own30b-omega4799-20261015/own30b-gates.sh (which copied #524) WITHOUT --force-memory; only the
# speed-relevant runs: check-full, ttft, graph-off, graph-on. One quietlock hold (<=20 min) per run,
# released between runs; each under /tmp/aien-gb10.lock. Nothing is killed or timed out.
set -u
SC=$HOME/workspace/hive-worktrees/sc-idle-speed
B=$HOME/handoffs/pin-bin-afe59ae1
EV=${EV:?evidence dir}
CK=$HOME/models/qwen3-30b-a3b-instruct-2507-fp8
G=$SC/docs/inference/evidence/own30b-f-20261013T1745Z/golden.json
ARCH=0629ad9cec0addd4ccc461f0523c2c3964964f0bafc0b98b678e5080b0373ae5
mkdir -p "$EV"
run() { # run NAME BIN ENV-ASSIGNMENTS... -- ARGS...
  local name=$1 bin=$2; shift 2
  local envs=(); while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
  {
    echo "queued $(date -u +%FT%TZ) sc $(git -C $SC rev-parse HEAD) omega $(cat $SC/omega.lock) archive-sha256 $ARCH"
    echo "binary $B/$bin sha256 $(sha256sum $B/$bin | cut -d' ' -f1)"
    echo "env ${envs[*]}"
    echo "cmd quietlock hold --owner IDLESPEED --minutes 20 -- flock /tmp/aien-gb10.lock env ${envs[*]} $B/$bin $*"
    uptime; free -g | head -2; grep MemAvailable /proc/meminfo
    nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv
    nvidia-smi --query-gpu=utilization.gpu,temperature.gpu,power.draw,pstate --format=csv
  } > "$EV/$name.start"
  quietlock hold --owner IDLESPEED --minutes 20 --reason "OWN-30B idle speed $name" -- \
    flock /tmp/aien-gb10.lock sh -c 'echo "lock acquired $(date -u +%FT%TZ)" >> "$0.start"; shift; exec "$@"' "$EV/$name" x env "${envs[@]}" "$B/$bin" "$@" \
    > "$EV/$name.raw" 2> "$EV/$name.err"
  local rc=$?
  echo "$rc" > "$EV/$name.rc"
  { echo "ended $(date -u +%FT%TZ)"; free -g | head -2; grep MemAvailable /proc/meminfo; } > "$EV/$name.end"
  echo "$name rc=$rc"
}
TOK="--prompts 1,2,3,4 --new 16 --layers 0,23,47 --weights fp8 --attn fused --router device --lm-head gemv --prefill auto"
run check-full own30b_check AIEN_OMEGA_SPIN_US=2000 -- chip $CK $G $EV/check-full $TOK
run ttft       own30b_ttft  AIEN_OMEGA_SPIN_US=2000 -- $CK $EV/ttft --lens 512,2048 --new-tokens 32 --reps 2 --devhost
run graph-off  own30b_check AIEN_OMEGA_SPIN_US=2000 AIEN_OMEGA_CTA_BUDGET=64 -- chip $CK $G $EV/graph-off $TOK
run graph-on   own30b_check AIEN_OMEGA_SPIN_US=2000 AIEN_OMEGA_CTA_BUDGET=64 OWN30B_DECODE_GRAPH=1 -- chip $CK $G $EV/graph-on $TOK
echo "all done $(date -u +%FT%TZ)"
