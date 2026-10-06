#!/usr/bin/env bash
# GB10 verification for sovereign-core PR "T4 launch budget" (branch inference/t4-warmup-and-cta-budget).
# build: CPU only, outside the hold.  run OUT: inside ONE quietlock hold (orchestrator owns it), ~3 min.
# Baseline first (budget 64 = unset), then one variable (AIEN_OMEGA_CTA_BUDGET=256). Same binary, same T4 goal.
set -u
SC=$HOME/.claude/jobs/665fb972/tmp/t4fix/sc                  # this PR's head
OMEGA=$HOME/.claude/jobs/665fb972/tmp/t4/om                   # clean omega c0369e6 (= omega.lock)
PHYS=$HOME/workspace/hive-worktrees/physics-6d7cf0d           # physics 6d7cf0d
MODEL=$HOME/.cache/huggingface/hub/models--unsloth--Llama-3.2-1B-Instruct/snapshots/5a8abab4a5d6f164389b1079fb721cfab8d7126c
case "${1:?build|run}" in
build)
  [ "$(git -C "$OMEGA" rev-parse HEAD)" = "$(cat "$SC/omega.lock")" ] || { echo "omega HEAD != omega.lock"; exit 1; }
  (cd "$SC" && AIEN_OMEGA_DIR="$OMEGA" AIEN_OMEGA_COMPOSE_DIR="$OMEGA" AIEN_PHYSICS_DIR="$PHYS" \
     AIEN_AIENOS_LOCK_REPO="${AIEN_AIENOS_LOCK_REPO:?set to the aienos b84c0a6 checkout}" \
     cargo build --release -p aien-cli -vv 2>&1 | grep -E "has_omega_gpu|has_omega_compose|static=rx_compose|stub" | sort -u)
  ;;
run)
  OUT=${2:?OUT dir}; mkdir -p "$OUT"
  GOAL="$(jq -r '.tasks[]|select(.id=="T4").goal' "$SC/docs/campaigns/next-phase-1/tasks-v6.json")"
  # Phase 0 (orchestrator review): prefill-sized shapes at cta 256 never ran on the chip; parity gate before T4 at 256.
  P0="106,2048,2048;106,2048,512;106,2048,8192;106,8192,2048"
  OMEGA_GPU_MATMUL_SHAPES="$P0" "$OMEGA/build/gpu_matmul_api_test" --timing --cta-budget 256 --out "$OUT/P0-cta256.json" >"$OUT/P0-cta256.log" 2>&1; echo "P0 cta=256 exit $?"
  grep -h "timing 106x" "$OUT/P0-cta256.log"
  P0OK=$(jq -r "[.shapes[] | (.ok_calls == 100 and .e2e_bad_first == 0 and .e2e_bad_last == 0 and .repeat_mismatch == 0)] | all" "$OUT/P0-cta256.json" 2>/dev/null)
  echo "P0 parity ok: $P0OK"
  BUDGETS=64; [ "$P0OK" = true ] && BUDGETS="64 256"
  for b in $BUDGETS; do
    if [ $b = 64 ]; then unset AIEN_OMEGA_CTA_BUDGET; else export AIEN_OMEGA_CTA_BUDGET=$b; fi
    AIEN_BIN="$SC/target/release/aien-cli" AIEN_MODEL_PATH="$MODEL/model.safetensors" \
    AIEN_TOKENIZER_PATH="$MODEL/tokenizer.json" AIEN_REQUIRE_BLACKWELL=1 AIEN_STEP_LOG=1 \
    AIEN_COMPOSE_MAX_TOKENS=256 NP1_GOAL="$GOAL" \
      bash "$SC/docs/campaigns/next-phase-1/run-campaign.sh" "$OUT/T4-cta$b" >"$OUT/T4-cta$b.driver.out" 2>&1
    echo "T4 cta=$b driver exit $?"
    L="$OUT/T4-cta$b/daemon-1.log"
    grep -h "Omega CTA budget\|Warm-up:" "$L"
    jq -c '.. | objects | select(.step? == "S3") | {S3_wall_ms: .wall_ms}' "$OUT/T4-cta$b/run.json" 2>/dev/null | head -1
    # step 1 = warm-up (outside B); steps 2.. = the T4 attempt (inside B)
    grep -h "^step:" "$L" | awk -F'backend_us=' '{split($2,a," "); n++; us=a[1];
      if(n==1) w=us; else if(n==2) p=us; else {d+=us; k++}}
      END {printf "warmup_ms=%.0f t4_prefill_ms=%.0f decode_steps=%d decode_ms_mean=%.1f logged_t4_backend_ms=%.0f (can include steps after a timeout)\n", w/1e3, p/1e3, k, (k?d/k/1e3:0), (p+d)/1e3}'
  done
  unset AIEN_OMEGA_CTA_BUDGET
  ;;
esac
