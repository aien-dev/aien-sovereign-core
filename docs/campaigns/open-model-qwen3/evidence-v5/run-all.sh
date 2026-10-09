#!/usr/bin/env bash
# Parts 1..4 in order, each in its own 20-minute hold 166657-oq3-v5-pN. Stops if a part's wrapper exits non-zero.
R=/home/drakestapleton/workspace/oq3-v5-real/records; mkdir -p $R
for p in 1 2 3 4; do
  date -u +%T > $R/hold-p$p.start
  quietlock hold --owner 166657-oq3-v5-p$p --minutes 20 --reason "OPEN-MODEL-QWEN3 v5 REAL qualification run part $p (frozen, Drake 2026-10-09)" -- /home/drakestapleton/workspace/oq3-v5-real/run-part.sh $p > $R/hold-p$p.log 2>&1
  rc=$?; echo $rc > $R/hold-p$p.rc; date -u +%T > $R/hold-p$p.end
  [ $rc = 0 ] || { echo "part $p rc=$rc, stopping" > $R/stopped; break; }
done
date -u +%T > $R/all.end
