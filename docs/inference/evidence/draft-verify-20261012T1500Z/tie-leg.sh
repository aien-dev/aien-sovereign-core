#!/bin/bash
W=~/workspace/.worktrees/cta1024; R=~/workspace/investigations/2026-10-12-qwen3-8b-decode-speed/receipts/draft-verify
BIN=$W/target-gb10/release/examples/qwen3_8b_specverify
V7=$W/docs/campaigns/open-model-qwen3/evidence-v7/run/out/3c5b1083e23732e59923ab0ed289f8951061a96ff1d7ac8873117df8b3522f09.json
N=tie-763; cd $W || exit 2
sha256sum $BIN > $R/$N-binary.sha256; date -u +%FT%TZ > $R/$N.start
env -u AIEN_DEV_FALLBACK AIEN_GPU_BACKEND=omega AIEN_REQUIRE_BLACKWELL=1 AIEN_SPECVERIFY_TIE=763 AIEN_SPECVERIFY_BASE=$R/specverify-1.json "$BIN" "$HOME/models/qwen3-8b" "$V7" gb10 > $R/$N.raw 2> $R/$N.err
echo $? > $R/$N.exit; date -u +%FT%TZ > $R/$N.end; grep "^{" $R/$N.raw > $R/$N.json
