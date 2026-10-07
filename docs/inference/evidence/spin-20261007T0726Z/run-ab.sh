#!/usr/bin/env bash
# A/B: same aien-cli (sc-spin, omega 94a75e2), Qwen3-4B, made-up goals (not campaign tasks).
# Run A: AIEN_OMEGA_SPIN_US unset (omega default 0). Run B: AIEN_OMEGA_SPIN_US=2000. Diagnostic only.
set -u
S=$(cd "$(dirname "$0")" && pwd)
BIN=$S/target/release/aien-cli
M=$HOME/models/qwen3-4b-instruct-2507-cdbee75
sha256sum "$BIN" "$0" $M/*.safetensors $M/model.safetensors.index.json $M/tokenizer.json $M/config.json > $S/identity.txt
mem() { awk '/^MemFree|^Cached:/{printf "%s=%d ",$1,$2/1024}' /proc/meminfo; }
one() {
  local tag=$1 spin=$2 R=$S/run-$1
  mkdir -p $R/ws; printf 'apples are red\n' > $R/ws/README.txt
  ( export AIEN_COMPOSE_DIR=$R/compose AIEN_PROVENANCE_DIR=$R/prov AIEN_RUNTIME_SOCK=$R/aien.sock \
       AIEN_RUNTIME_STATE_DIR=$R/state AIEN_REQUIRE_CHECKPOINT=1 AIEN_REQUIRE_BLACKWELL=1 \
       AIEN_MODEL_PATH=$M/model.safetensors.index.json AIEN_TOKENIZER_PATH=$M/tokenizer.json \
       AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1 AIEN_KV_CONTEXT_TOKENS=4096 AIEN_COMPOSE_MAX_TOKENS=64 AIEN_STEP_LOG=1
    [ -n "$spin" ] && export AIEN_OMEGA_SPIN_US=$spin
    echo "pre $(date -u +%T) $(mem)" > $R/mem.txt
    t0=$(date +%s%3N)
    setsid "$BIN" daemon > $R/daemon.log 2>&1 < /dev/null &
    P=$!
    i=0; while ! grep -q 'Warm-up:' $R/daemon.log && kill -0 $P 2>/dev/null && [ $i -lt 1200 ]; do sleep 0.5; i=$((i+1)); done
    echo "warm-up seen=$(grep -c 'Warm-up:' $R/daemon.log) after $(( $(date +%s%3N)-t0 )) ms" >> $R/mem.txt
    if grep -q 'Warm-up:' $R/daemon.log && kill -0 $P 2>/dev/null; then
      "$BIN" compose propose --goal "Create the file fruits.txt listing three fruits, one per line: apple, pear, plum." --workspace $R/ws > $R/propose-a.json 2> $R/propose-a.err
      "$BIN" compose propose --goal "Create the file colors.txt listing twelve colors, one per line." --workspace $R/ws > $R/propose-b.json 2> $R/propose-b.err
      "$BIN" compose propose --goal "Create the file garden.txt with eight short lines about planning a vegetable garden." --workspace $R/ws > $R/propose-c.json 2> $R/propose-c.err
      "$BIN" compose shutdown > $R/shutdown.json 2>&1
    fi
    i=0; while kill -0 $P 2>/dev/null && [ $i -lt 240 ]; do sleep 0.5; i=$((i+1)); done
    echo "daemon exited=$(kill -0 $P 2>/dev/null && echo no-still-running || echo yes) $(date -u +%T) $(mem)" >> $R/mem.txt )
  echo "run $tag done"
}
one A ""
one B 2000
one A2 ""
one B2 2000
