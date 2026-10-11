#!/usr/bin/env bash
# Qwen3-8B correctness smoke (docs/campaigns/open-model-qwen3/SMOKE-8B-v1.md, sc#384).
#   smoke_qwen3_8b.sh verify            sha256 of config.json and the five shards only
#   smoke_qwen3_8b.sh cpu               verify, then the CPU reference leg; records the golden ids
#                                       (refuses to overwrite an existing golden file)
#   smoke_qwen3_8b.sh gb10              verify, then the GB10 leg against the golden ids.
#                                       OWNS THE CHIP: only after the whole-system runs have finished.
# Env: MODEL_DIR (default ~/models/qwen3-8b), OUT (default $PWD/smoke-8b-out).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MODEL_DIR=${MODEL_DIR:-$HOME/models/qwen3-8b}
OUT=${OUT:-$PWD/smoke-8b-out}
GOLDEN=$REPO/docs/campaigns/open-model-qwen3/SMOKE-8B-v1.golden.json
# Values recorded in docs/campaigns/open-model-qwen3/EVAL-qwen3-8b.md (Section 1, branch d1/qwen3-8b-eval).
read -r -d '' SUMS <<'EOS'
f7c4eadfbbf522470667b797a3c89be2524832d2d599797248dc304fff447c30  config.json
31d6a825ae35f11fb85b195b4c42c146c051e446433125a215336abdf95cbf5f  model-00001-of-00005.safetensors
5991236cea6fe21f3d43cab0f0e84448734fbbe0789816202989f2ddc9d18282  model-00002-of-00005.safetensors
c5185c4794be2d8a9784d5753c9922db38df478ce11f9ed0b415b7304d896836  model-00003-of-00005.safetensors
b5ee7de71fbf17db3d5704e0c8f2bc7d005ca9e1d7ca2aeb19827b0cfcaa917a  model-00004-of-00005.safetensors
20c2d6366ab85c90786ccdd829cd2b9e7d30ef3b2ebbb998280e7e4014b542ff  model-00005-of-00005.safetensors
EOS
fail() { echo "SMOKE-8B FAIL: $*" >&2; exit 1; }
verify() {
  [ -d "$MODEL_DIR" ] || fail "no model directory $MODEL_DIR"
  ( cd "$MODEL_DIR" && printf '%s\n' "$SUMS" | sha256sum -c --quiet - ) || fail "sha256 mismatch in $MODEL_DIR"
  echo "sha256 ok: config.json and 5 shards"
}
build() { # $1 = cpu|gb10
  if [ "$1" = gb10 ]; then
    # The pinned GPU engine, as scripts/whole_system_e2e.sh G1 does (env.sh: native libs required, no dev fallback).
    local pins=$OUT/pins
    bash "$REPO/scripts/pins.sh" --gpu "$pins" >"$OUT/pins.txt" 2>&1 || fail "pins.sh --gpu failed, see $OUT/pins.txt"
    ( cd "$REPO" && . "$pins/env.sh" && CARGO_TARGET_DIR="$OUT/target-gb10" \
        cargo build --release -p aien-runtime --example qwen3_8b_smoke ) >"$OUT/build-gb10.log" 2>&1 || fail "build failed, see $OUT/build-gb10.log"
    grep -q -E "building the (CPU )?stub|developer override" "$OUT/build-gb10.log" && fail "stub engine build"
    BIN=$OUT/target-gb10/release/examples/qwen3_8b_smoke
  else
    ( cd "$REPO" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$OUT/target-cpu}" \
        cargo build --release -p aien-runtime --example qwen3_8b_smoke ) >"$OUT/build-cpu.log" 2>&1 || fail "build failed, see $OUT/build-cpu.log"
    BIN=${CARGO_TARGET_DIR:-$OUT/target-cpu}/release/examples/qwen3_8b_smoke
  fi
  [ -x "$BIN" ] || fail "no binary at $BIN"
}
mkdir -p "$OUT"
case "${1:-}" in
  verify) verify ;;
  cpu)
    verify; build cpu
    [ -e "$GOLDEN" ] && fail "golden file exists ($GOLDEN); delete it deliberately to re-record"
    T0=$(date +%s)
    "$BIN" "$MODEL_DIR" cpu >"$OUT/cpu.json" 2>"$OUT/cpu.err" || fail "cpu leg exit $?, see $OUT/cpu.err"
    echo "cpu leg wall seconds: $(( $(date +%s) - T0 ))"
    jq -e '(.leg=="cpu") and (.output_ids|length>0) and (.fallback_count==0)' "$OUT/cpu.json" >/dev/null || fail "cpu result not well formed"
    jq '{golden_for:"SMOKE-8B-v1", source:"CPU reference leg", prompt_sha256, prompt_ids, output_ids, reply}' "$OUT/cpu.json" >"$GOLDEN"
    echo "golden output_ids: $(jq -c .output_ids "$GOLDEN")"
    echo "CPU leg done: golden recorded in $GOLDEN (not a chip verdict)"
    ;;
  gb10)
    verify
    [ -s "$GOLDEN" ] || fail "no golden file $GOLDEN (run the cpu leg first)"
    command -v quietlock >/dev/null && { quietlock check || fail "quiet flag held: another run owns the machine"; }
    pgrep -f whole_system_e2e.sh >/dev/null && fail "a whole-system run is live; the chip is not free"
    build gb10
    env -u AIEN_DEV_FALLBACK -u AIEN_OMEGA_CTA_BUDGET -u AIEN_OMEGA_SPIN_US AIEN_GPU_BACKEND=omega AIEN_REQUIRE_BLACKWELL=1 \
      "$BIN" "$MODEL_DIR" gb10 >"$OUT/gb10.json" 2>"$OUT/gb10.err" || fail "gb10 leg exit $?, see $OUT/gb10.err"
    jq -e '.leg=="gb10"' "$OUT/gb10.json" >/dev/null || fail "gb10 result not well formed"
    [ "$(jq -c .output_ids "$OUT/gb10.json")" = "$(jq -c .output_ids "$GOLDEN")" ] || fail "token ids differ from the CPU golden: $(jq -c .output_ids "$OUT/gb10.json") vs $(jq -c .output_ids "$GOLDEN")"
    [ "$(jq -c .prompt_ids "$OUT/gb10.json")" = "$(jq -c .prompt_ids "$GOLDEN")" ] || fail "prompt ids differ from the golden"
    [ "$(jq .fallback_count "$OUT/gb10.json")" = 0 ] || fail "fallback_count is $(jq .fallback_count "$OUT/gb10.json"), not 0"
    echo "SMOKE-8B PASS (GB10 token ids equal the CPU golden, fallback_count 0)"
    ;;
  *) echo "usage: $0 verify|cpu|gb10" >&2; exit 2 ;;
esac
