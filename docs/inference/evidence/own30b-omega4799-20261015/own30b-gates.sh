#!/bin/bash
# OWN-30B gates on the omega 4799f9eb pin bump (sc branch build/omega-pin-4799f9eb), same commands as #524
# (docs/inference/evidence/own30b-rc-20261014T0153Z and own30b-L2-20261013). One quietlock hold (<=20 min) per run,
# --force-memory (added vs #524): the MAX 8B experiment holds ~36 GB, so MemAvailable is ~68 GB, under the tools' 70 GB guard
# (which exists to refuse a second 30B model); no other 30B process runs. Attempt 1 without it: attempt1-refused-70gb/.
# each under /tmp/aien-gb10.lock. Nothing is killed or timed out.
set -u
SC=$HOME/workspace/sc-pinbump
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
    echo "queued $(date -u +%FT%TZ) sc $(git -C $SC rev-parse HEAD) (main bbca84b1 + omega.lock 4799f9eb + lock lines) omega $(cat $SC/omega.lock) archive-sha256 $ARCH"
    echo "binary $B/$bin sha256 $(sha256sum $B/$bin | cut -d' ' -f1)"
    echo "env ${envs[*]}"
    echo "cmd quietlock hold --owner PINBUMP --minutes 20 -- flock /tmp/aien-gb10.lock env ${envs[*]} $B/$bin $*"
    uptime; free -g | head -2
  } > "$EV/$name.start"
  quietlock hold --owner PINBUMP --minutes 20 --reason "OWN-30B pin bump $name" -- \
    flock /tmp/aien-gb10.lock sh -c 'echo "lock acquired $(date -u +%FT%TZ)" >> "$0.start"; shift; exec "$@"' "$EV/$name" x env "${envs[@]}" "$B/$bin" "$@" \
    > "$EV/$name.raw" 2> "$EV/$name.err"
  local rc=$?
  echo "$rc" > "$EV/$name.rc"
  { echo "ended $(date -u +%FT%TZ)"; free -g | head -2; } > "$EV/$name.end"
  echo "$name rc=$rc"
}
TOK="--prompts 1,2,3,4 --new 16 --layers 0,23,47 --weights fp8 --attn fused --router device --lm-head gemv --prefill auto --force-memory"
run check-full own30b_check AIEN_OMEGA_SPIN_US=2000 -- chip $CK $G $EV/check-full $TOK
run check-wp   own30b_check AIEN_OMEGA_SPIN_US=2000 -- chip $CK $G $EV/check-wp $TOK --tokens-only --whole-prompt --prefill-device-host
run ttft       own30b_ttft  AIEN_OMEGA_SPIN_US=2000 -- $CK $EV/ttft --lens 512,2048 --new-tokens 32 --reps 2 --devhost --force-memory
run gate       own30b_wp_gate AIEN_OMEGA_SPIN_US=2000 -- $CK $EV/gate --lens 512,2048 --ops-layer 0 --mutants --devhost --force-memory
run gate-short own30b_wp_gate AIEN_OMEGA_SPIN_US=2000 -- $CK $EV/gate-short --lens 1,2,20 --ops-layer 0 --mutants --devhost --force-memory
run graph-off  own30b_check AIEN_OMEGA_SPIN_US=2000 AIEN_OMEGA_CTA_BUDGET=64 -- chip $CK $G $EV/graph-off $TOK
run graph-on   own30b_check AIEN_OMEGA_SPIN_US=2000 AIEN_OMEGA_CTA_BUDGET=64 OWN30B_DECODE_GRAPH=1 -- chip $CK $G $EV/graph-on $TOK
( cd "$EV" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )
echo "all done $(date -u +%FT%TZ)"
