#!/usr/bin/env bash
# CAND-4 harness self-test: explicit boolean false must never read as missing or as true
# (jq's `//` turns false into its alternative). Shell + jq only. Exit 0 iff all hold.
set -u
HERE=$(cd "$(dirname "$0")" && pwd); SCORE=$HERE/../scoring/score-rows.sh
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT; bad=0
t() { if [ "$2" = "$3" ]; then echo "PASS  $1"; else echo "FAIL  $1: got $2, want $3"; bad=1; fi; }
echo '{"backend":"OmegaGb10Backend (native)","s3_committed":false}' >"$T/fj.json"
t "fixture s3_committed=false: C3-control FAIL" "$(jq -r 'if .s3_committed == true and ((.backend // "") | test("OmegaGb10Backend")) then "PASS" else "FAIL" end' "$T/fj.json")" FAIL
t "fixture s3_committed=false: external F0 false" "$(jq -c '{F0: (.s3_committed == true)}' "$T/fj.json")" '{"F0":false}'
t "fixture s3_committed=false: valid false" "$(jq -r '(. // {}).s3_committed == true' "$T/fj.json")" false
echo '{"s3_committed":true,"backend":"x"}' >"$T/ft.json"
t "fixture s3_committed=true: external F0 true" "$(jq -c '{F0: (.s3_committed == true)}' "$T/ft.json")" '{"F0":true}'
# scorer: a declared row with explicit not_applicable false and a result line verdict FAIL is FAIL, not missing
jq -n '{contract:"scoring-v5",campaign:"selftest",rows:[{row:"R",role:"case",reps:1,expected:"PASS",control:null,not_applicable:false}]}' >"$T/d.json"
echo '{"row":"R","rep":1,"verdict":"FAIL","outcome":"explicit"}' >"$T/r.jsonl"
t "scorer: explicit FAIL result is FAIL" "$(bash "$SCORE" "$T/d.json" "$T/r.jsonl" | jq -r .verdict)" FAIL
echo '{"row":"R","rep":1,"verdict":"PASS","outcome":"explicit"}' >"$T/r.jsonl"
t "scorer: explicit PASS result is PASS" "$(bash "$SCORE" "$T/d.json" "$T/r.jsonl" | jq -r .verdict)" PASS
t "scorer: no result line is not PASS" "$(: >"$T/r.jsonl"; bash "$SCORE" "$T/d.json" "$T/r.jsonl" | jq -r .verdict)" NOT_RUN
exit $bad
