#!/usr/bin/env bash
# NEXT-PHASE-2 receipt (ACCEPTANCE-v2 section 6) from one fixture F0 and one
# `run-faults.sh cases` run root. Shell + jq only. Usage:
#   make-receipt.sh FIX_ROOT RUN_ROOT OUT_DIR SC_COMMIT OMEGA_COMPOSE_COMMIT GPU_BIN CPU_BIN [NOTE]
# Writes OUT_DIR/<sha256 of content>.json (never edited afterwards), appends one
# line to OUT_DIR/INDEX.md, writes OUT_DIR/<sha256>.summary.txt, prints it.
# Row verdicts: a row PASSes only if every repetition PASSes and its uninjected
# control passed; a failed control or a variant that could not be injected is
# NOT_RUN; NOT_RUN is never PASS.
set -eu
FIX=$1 R=$2 OUT=$3 SC=$4 OMC=$5 GPU=$6 CPU=$7 NOTE=${8:-}
mkdir -p "$OUT"
sha() { sha256sum "$1" | cut -d' ' -f1; }
fixture=$(jq -c --arg fj "$(sha "$FIX/fixture.json")" --arg s3 "$(sha "$FIX/steps/S3.json")" \
  --arg log "$(sha "$FIX/daemon-fixture.log")" \
  '{sha256:.files_sha256, s3_committed, machine_id, proposal_path, backend, daemon_start_ms,
    evidence_digests:[{file:"fixture.json", sha256:$fj}, {file:"steps/S3.json", sha256:$s3}, {file:"daemon-fixture.log", sha256:$log}]}' \
  "$FIX/fixture.json")
tmp=$(mktemp "$OUT/.receipt.XXXXXX")
jq -n --slurpfile runs "$R/results.jsonl" --argjson fixture "$fixture" --arg sc "$SC" --arg omc "$OMC" \
  --arg gpu "$(sha "$GPU")" --arg cpu "$(sha "$CPU")" --arg note "$NOTE" '
  def control_of($row): if ($row | startswith("C3")) then "F0"
    else "control-" + ($row | capture("^(?<c>C[0-9])").c) end;
  ($runs | map(select(.row | startswith("control-")))) as $controls
  | ($fixture.s3_committed == true) as $f0
  | (($fixture.backend // "") | test("OmegaGb10Backend")) as $c3ok
  | [ "C1a","C1b","C2a","C2b","C2c","C2d","C3a","C3b","C4","C5a","C5b","C5c",
      "C6a","C6b","C6c","C6d","C6e","C6f","C6g","C6h" ] as $order
  | ($order | map(. as $row
      | ($runs | map(select(.row == $row))) as $reps
      | (control_of($row)) as $c
      | (if $c == "F0" then $f0 else ($controls | map(select(.row == $c)) | (length > 0 and all(.verdict == "PASS"))) end) as $cok
      | {row:$row, control:$c, control_pass:$cok, reps:($reps | length),
         verdict:(if ($reps | length) == 0 or ($cok | not) or ($reps | any(.verdict == "NOT_RUN")) then "NOT_RUN"
                  elif ($reps | all(.verdict == "PASS")) then "PASS" else "FAIL" end),
         outcomes:($reps | map(.outcome) | unique)})) as $rowv
  | ($rowv + [{row:"C3 control", control:"F0", control_pass:$f0, reps:1,
      verdict:(if $f0 and $c3ok then "PASS" else "FAIL" end),
      outcomes:["gpu daemon with AIEN_REQUIRE_BLACKWELL=1: " + ($fixture.backend // "none")]}]) as $all
  | {
    gate:"NEXT_PHASE_2",
    commit:$sc,
    omega_compose_commit:$omc,
    acceptance_spec:"docs/campaigns/next-phase-2/ACCEPTANCE-v2.md spec_version 2",
    builds:[{name:"gpu", sha256:$gpu}, {name:"cpu-fault", sha256:$cpu}],
    fixture:$fixture,
    verdict:(if $f0 and ($all | all(.verdict == "PASS")) then "PASS" else "FAIL" end),
    row_verdicts:$all,
    controls:($controls | map({row, rep, injected_at, outcome, verdict, checks, evidence_digests})),
    rows:($runs | map(select(.row | startswith("control-") | not))
          | map({row, rep, injected_at, outcome, verdict, checks, evidence_digests})),
    not_proved:[
      "runtime loss of the GPU in a GPU-linked build",
      "omega in-settle crash hooks (A12)",
      "caproot revoke on the effect path (A11)",
      "Cortex and J-Space rolled back together (omega known limit)",
      "torn writes below the file level"],
    note:(if $note == "" then null else $note end)
  }' >"$tmp"
h=$(sha256sum "$tmp" | cut -d' ' -f1)
mv "$tmp" "$OUT/$h.json"
v=$(jq -r .verdict "$OUT/$h.json")
{
  echo "NEXT_PHASE_2 receipt $h.json verdict $v"
  echo "sovereign-core $SC; omega compose $OMC"
  jq -r '.builds[] | "build \(.name) \(.sha256)"' "$OUT/$h.json"
  jq -r '.fixture | "fixture F0 \(.sha256): s3_committed \(.s3_committed), machine \(.machine_id), backend \(.backend)"' "$OUT/$h.json"
  echo "Rows (ACCEPTANCE-v2 section 5; injected variants x3, control once per case):"
  jq -r '.row_verdicts[] | "  \(.verdict)  \(.row)  (\(.reps) run(s), control \(.control) \(if .control_pass then "PASS" else "FAIL" end))  \(.outcomes | join(" | ") | .[0:200])"' "$OUT/$h.json"
  echo "Failed checks:"
  jq -r '(.controls + .rows)[] | . as $r | .checks[] | select(.ok | not) | "  \($r.row) rep \($r.rep): \(.check) \(.value | tojson | .[0:160])"' "$OUT/$h.json"
  echo "Not proved (declared): $(jq -r '.not_proved | join("; ")' "$OUT/$h.json")"
  if [ -n "$NOTE" ]; then echo "Note: $NOTE"; fi
} >"$OUT/$h.summary.txt"
[ -f "$OUT/INDEX.md" ] || printf '# NEXT-PHASE-2 campaign receipts\n\nEach receipt is named by the sha256 of its content and never edited.\n\n' >"$OUT/INDEX.md"
echo "- \`$h.json\` verdict $v, sovereign-core $SC ($(date -u +%Y-%m-%dT%H:%MZ))" >>"$OUT/INDEX.md"
cat "$OUT/$h.summary.txt"
