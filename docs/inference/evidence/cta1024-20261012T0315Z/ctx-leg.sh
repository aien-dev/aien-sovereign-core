N=${N:-smoke-ctx-cta1024}
#!/bin/bash
# Rebuild the smoke binary with the per-op timing table (pinned GB10 engine), then one leg at the daemon defaults
# (CTA 1024, spin 2000) with AIEN_SMOKE_LONG=1, under one hold. Output: smoke-timing-cta1024.{raw,err,json,rc}.
W=~/workspace/.worktrees/cta1024; E=$W/docs/inference/evidence/cta1024-20261012T0315Z; cd $W || exit 2
unset AIEN_FORCE_CPU_STUB AIEN_OMEGA_GPU_LIB AIEN_OMEGA_COMPOSE_LIB AIEN_OMEGA_COMPOSE_DIR AIEN_OMEGA_COMPOSE_SHA AIEN_DEV_FALLBACK AIEN_BACKEND
. $W/target-gb10/pins/env.sh
CARGO_TARGET_DIR=$W/target-gb10 cargo build --release -p aien-runtime --example qwen3_8b_smoke > $W/target-gb10-build-$N.log 2>&1; rc=$?
echo "build rc=$rc $(grep -cE 'building the (CPU )?stub|developer override' $W/target-gb10-build-$N.log) stub-lines"
[ $rc -eq 0 ] || { grep -A6 "^error" $W/target-gb10-build-$N.log | head -20; echo chain-done; exit 1; }
BIN=$W/target-gb10/release/examples/qwen3_8b_smoke; sha256sum $BIN > $E/$N-binary.sha256
MODEL_DIR=${MODEL_DIR:-$HOME/models/qwen3-8b}; N=${N:-smoke-ctx-cta1024}
leg() { date -u +%FT%TZ > $E/$N.start
  env -u AIEN_DEV_FALLBACK AIEN_SMOKE_LONG=1 AIEN_SMOKE_CTX=1 AIEN_GPU_BACKEND=omega AIEN_REQUIRE_BLACKWELL=1 "$BIN" "$MODEL_DIR" gb10 > $E/$N.raw 2> $E/$N.err; echo $? > $E/$N.rc; date -u +%FT%TZ > $E/$N.end
  grep "^{" $E/$N.raw > $E/$N.json; }
quietlock hold --owner ade3c837-timing --minutes 20 --reason "Qwen3-8B smoke long-context timing, one leg under 5 min, sc#384" -- bash -c "$(declare -f leg); E=$E; N=$N; BIN=$BIN; MODEL_DIR=$MODEL_DIR; leg"
echo "hold rc=$? leg rc=$(cat $E/$N.rc)"; jq -r '"cta=\(.cta_budget) spin=\(.spin_us) fallback=\(.fallback_count) " + (.prompts[]|select(.name=="p3" or .name=="p4")|"\(.name) prompt=\(.prompt_ids|length) ms=\(.ms|round) tokens=\(.output_ids|length) OPS: \(.op_timing)\n")' $E/$N.json; echo chain-done
