#!/usr/bin/env bash
# OPEN-MODEL-QWEN3 v7 gate G6: dry run of part 1 on GB10 in its own hold, then g6-gate.sh over D1 D2 D3 (SPEC-v7-DRAFT.md
# Section 5 item 7). Runs BEFORE the freeze (frozen-v7.json may still say DRAFT: a dry run lifts the pin checks and is never
# a verdict). Start detached ONLY on the coordinator's go for the hold:
#   setsid nohup bash run-dry.sh <sc main commit holding the v7 scripts> <oq3-v7-build-<commit> folder> > <log> 2>&1 < /dev/null &
set -u
WRAP_COMMIT=${1:?sc main commit}; export OQ3_BUILD=${2:?build folder}
HERE=$(cd "$(dirname "$0")" && pwd)
V=/home/drakestapleton/workspace/oq3-v7-real; R=$V/records
[ ! -e $V/dry-runs ] || { echo "run-dry: $V/dry-runs exists already (one dry run per freeze candidate; move it aside to rerun)"; exit 2; }
[ -x "$OQ3_BUILD/target/release/aien-cli" ] || { echo "run-dry: no aien-cli in $OQ3_BUILD"; exit 2; }
quietlock check || { echo "run-dry: quiet flag held, not starting"; exit 2; }
mkdir -p $R
[ -d $V/wrap ] || git clone -q https://github.com/aien-dev/aien-sovereign-core $V/wrap || { echo "run-dry: wrapper clone failed"; exit 4; }
git -C $V/wrap fetch -q origin && git -C $V/wrap checkout -q "$WRAP_COMMIT" || { echo "run-dry: checkout $WRAP_COMMIT failed"; exit 4; }
( cd $V/wrap/docs/campaigns/open-model-qwen3 && bash test-v7.sh > $R/test-v7.out 2>&1 ) || { echo "run-dry: test-v7.sh failed at $WRAP_COMMIT"; exit 2; }
cp "$HERE/run-part.sh" $V/run-part.sh; chmod +x $V/run-part.sh; sha256sum $V/run-part.sh "$0" > $R/launcher-dry.sha256
date -u +%FT%TZ > $R/hold-dry-p1.start
quietlock hold --owner laneB-oq3-v7-dry-p1 --minutes 40 --reason "OPEN-MODEL-QWEN3 v7 gate G6 dry run part 1, Qwen3-8B (not scored, sc#384)" -- $V/run-part.sh dry 1 > $R/hold-dry-p1.log 2>&1
echo $? > $R/hold-dry-p1.rc; date -u +%FT%TZ > $R/hold-dry-p1.end
Q=$V/wrap/docs/campaigns/open-model-qwen3
bash $Q/g6-gate.sh $R/dry-out/oq3-v7-DRYRUN-results.jsonl $Q/../scoring/declarations/oq3-v5.decl.json $V/dry-runs D1 D2 D3 > $R/g6-reasons.txt 2>&1
rc=$?; [ $rc = 0 ] && echo "G6 PASS" > $R/g6.txt || echo "G6 FAIL (rc $rc, see g6-reasons.txt)" > $R/g6.txt
cat $R/g6.txt; grep -m1 -o 'CHECKPOINT_SHARDS.*' $V/dry-runs/D1/daemon-1.log 2>/dev/null | cut -c1-200 > $R/model_sha256-line.txt; cat $R/model_sha256-line.txt
