#!/bin/bash
# A/B on the real model, one hold: Qwen3-8B smoke binary (built from this branch), today's daemon budget 256
# vs the new default 1024 (setting unset). Same prompts p1/p2 (golden-checked) + p3 long (timing). Nothing is killed.
W=~/workspace/.worktrees/cta1024; E=$W/docs/inference/evidence/cta1024-20261012T0315Z; BIN=$W/target-gb10/release/examples/qwen3_8b_smoke
MODEL_DIR=${MODEL_DIR:-$HOME/models/qwen3-8b}; GOLDEN=$W/docs/campaigns/open-model-qwen3/SMOKE-8B-v1.golden.json
leg() { local name=$1; shift; date -u +%FT%TZ > $E/$name.start
  env -u AIEN_DEV_FALLBACK "$@" AIEN_SMOKE_LONG=1 AIEN_GPU_BACKEND=omega AIEN_REQUIRE_BLACKWELL=1 \
    "$BIN" "$MODEL_DIR" gb10 > $E/$name.raw 2> $E/$name.err; echo $? > $E/$name.rc; date -u +%FT%TZ > $E/$name.end
  grep "^{" $E/$name.raw > $E/$name.json
  echo "$name: rc=$(cat $E/$name.rc) golden_p1p2=$([ "$(jq -c '[.prompts[]|select(.name!="p3")|[.name,.prompt_ids,.output_ids]]' $E/$name.json)" = "$(jq -c '[.prompts[]|[.name,.prompt_ids,.output_ids]]' $GOLDEN)" ] && echo EQUAL || echo DIFFERENT) $(jq -r '"cta_budget=\(.cta_budget) spin_us=\(.spin_us) fallback=\(.fallback_count) " + (.prompts[]|select(.name=="p3")|"p3 tokens=\(.output_ids|length) ms=\(.ms|round) tok/s=\(((.output_ids|length)/(.ms/1000))*100|round/100) finish=\(.finish) sha=\(.output_ids|tostring|@base64|.[0:12])")' $E/$name.json)"
}
leg smoke-cta256 env AIEN_OMEGA_CTA_BUDGET=256
leg smoke-cta1024 env
echo AB-DONE > $E/ab.end
