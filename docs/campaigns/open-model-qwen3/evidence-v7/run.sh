#!/usr/bin/env bash
# OPEN-MODEL-QWEN3 v7 REAL qualification run (SPEC-v7-DRAFT.md Sections 3 and 5 item 8). Start detached ONLY on the
# coordinator's go, after run-dry.sh recorded "G6 PASS" and the freeze PR (frozen-v7.json FROZEN, all pins filled) is merged:
#   setsid nohup bash run.sh <sc main commit that merged the freeze PR> <oq3-v7-build-<commit> folder> > <log> 2>&1 < /dev/null &
# Four parts, each in its own 40-minute hold laneB-oq3-v7-pN (the 8B decodes about half as fast as the 4B: v6 parts took
# 7 to 8 minutes each). Stop if a part's wrapper exits non-zero (part 4 exits 1 on a FAIL verdict: the scorer's verdict, not a crash).
set -u
WRAP_COMMIT=${1:?sc main commit holding the FROZEN frozen-v7.json}; export OQ3_BUILD=${2:?build folder}
V=/home/drakestapleton/workspace/oq3-v7-real; R=$V/records
[ ! -e /home/drakestapleton/workspace/oq3-v7-runs ] || { echo "run: the v7 run base exists already (v7 runs once)"; exit 2; }
[ "$(cat $R/g6.txt 2>/dev/null)" = "G6 PASS" ] || { echo "run: no G6 PASS recorded by run-dry.sh"; exit 2; }
quietlock check || { echo "run: quiet flag held, not starting"; exit 2; }
git -C $V/wrap fetch -q origin && git -C $V/wrap checkout -q "$WRAP_COMMIT" || { echo "run: checkout $WRAP_COMMIT failed"; exit 4; }
grep -q '"status": *"FROZEN' $V/wrap/docs/campaigns/open-model-qwen3/frozen-v7.json || { echo "run: frozen-v7.json at $WRAP_COMMIT is not FROZEN"; exit 2; }
( cd $V/wrap/docs/campaigns/open-model-qwen3 && bash test-v7.sh > $R/test-v7-real.out 2>&1 ) || { echo "run: test-v7.sh failed at $WRAP_COMMIT"; exit 2; }
sha256sum $V/run-part.sh "$0" > $R/launcher-real.sha256
for p in 1 2 3 4; do
  date -u +%FT%TZ > $R/hold-p$p.start
  quietlock hold --owner laneB-oq3-v7-p$p --minutes 40 --reason "OPEN-MODEL-QWEN3 v7 REAL qualification run part $p, Qwen3-8B (frozen-v7 FROZEN, sc#384)" -- $V/run-part.sh real $p > $R/hold-p$p.log 2>&1
  rc=$?; echo $rc > $R/hold-p$p.rc; date -u +%FT%TZ > $R/hold-p$p.end
  [ $rc = 0 ] || { echo "part $p rc=$rc, stopping" > $R/stopped; break; }
done
date -u +%FT%TZ > $R/all.end
