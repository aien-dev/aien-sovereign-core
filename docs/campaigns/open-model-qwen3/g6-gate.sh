#!/usr/bin/env bash
# Gate G6 (dry run) for OPEN-MODEL-QWEN3 v7 and later: checks EVERY product-side row of each dry launch, not four (v6 checked only
# PRE, OPR, BE, PIN and so missed the ALLEN refusal; RESULT-v6.md Section 3). Shell + jq only. It never reads a threshold.
#   g6-gate.sh DRY_RESULTS.jsonl DECLARATION.json DRY_RUN_BASE LAUNCH...      exit 0 = G6 PASS, 1 = G6 FAIL (reasons on stdout)
# Per launch:
#  1. run.json exists, and its steps S1 (remember), S2 (inspect), S7 (restart), S8 (recall) are ok, and the daemon logged
#     "ALLEN: engaged" (run.json .allen). These steps never depend on what the model writes.
#  2. Always required (product rows; independent of the model's text): every declared row of the launch whose id ends in
#     -PRE -DEC -OPR -BE -PIN -MEAS, and "v1 Identity", "v1 Human interventions: rescues", "v1 Containment: speculation",
#     "v1 Memory", "v1 Correctness: recall".
#  3. If the launch committed (S4 and S5 ok), every other declared row of the launch must be PASS too, except the four
#     model-behaviour rows -RQ<k>, -CUT, -F, -SB (v6 failure class (a)), which are listed but may FAIL.
#     If it did not commit (the model's reply was refused), only 1 and 2 apply and the rest is listed as NOT CHECKED (model).
#  4. A required row missing from the results is a FAIL.
set -u
RES=${1:?DRY_RESULTS.jsonl} DECL=${2:?DECLARATION.json} BASE=${3:?DRY_RUN_BASE}; shift 3
[ $# -gt 0 ] || { echo "g6-gate: give the launch ids" >&2; exit 2; }
fail=0
say() { echo "$*"; }
for L in "$@"; do
  rj=$BASE/$L/run.json
  if [ ! -f "$rj" ]; then say "FAIL $L no run.json"; fail=1; continue; fi
  for s in S1 S2 S7 S8; do
    [ "$(jq -r --arg s $s '[.steps[] | select(.step == $s)][0].ok // false' "$rj")" = true ] || { say "FAIL $L step $s not ok"; fail=1; }
  done
  jq -e '(.allen.engaged_log_line // "") | startswith("ALLEN: engaged")' "$rj" >/dev/null || { say "FAIL $L daemon did not log ALLEN: engaged"; fail=1; }
  committed=$(jq -r '([.steps[] | select(.step == "S4" or .step == "S5") | .ok] | length == 2 and all)' "$rj")
  nreq=0 nok=0 nskip=0
  while IFS= read -r row; do
    id=${row#"$L/"}
    case $id in
      "$L"-PRE|"$L"-DEC|"$L"-OPR|"$L"-BE|"$L"-PIN|"$L"-MEAS|"v1 Identity"|"v1 Human interventions: rescues"|"v1 Containment: speculation"|"v1 Memory"|"v1 Correctness: recall") need=always ;;
      "$L"-RQ*|"$L"-CUT|"$L"-F|"$L"-SB) need=model ;;
      *) need=committed ;;
    esac
    if [ $need = model ] || { [ $need = committed ] && [ "$committed" != true ]; }; then nskip=$((nskip + 1)); continue; fi
    nreq=$((nreq + 1))
    v=$(jq -r --arg r "$row" 'select(.row == $r) | .verdict' "$RES" | head -1)
    if [ "$v" = PASS ]; then nok=$((nok + 1)); else say "FAIL $L row '$id' = ${v:-missing}"; fail=1; fi
  done < <(jq -r --arg l "$L" '.rows[] | select(.row | startswith($l + "/")) | .row' "$DECL")
  say "$L committed=$committed required=$nreq pass=$nok not_checked_model=$nskip"
done
[ $fail = 0 ] && say "G6 PASS" || say "G6 FAIL"
exit $fail
