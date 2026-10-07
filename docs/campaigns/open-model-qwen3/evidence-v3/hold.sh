#!/usr/bin/env bash
p=$1; R=/home/drakestapleton/workspace/oq3-v3-records
date -u +%T > $R/hold-p$p.start
quietlock hold --owner 031756-oq3-v3-p$p --minutes 20 --reason "OPEN-MODEL-QWEN3 v3 part $p" -- $R/run-part.sh $p > $R/hold-p$p.log 2>&1
echo $? > $R/hold-p$p.rc
date -u +%T > $R/hold-p$p.end
