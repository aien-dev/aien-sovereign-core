#!/usr/bin/env bash
# NEXT-PHASE-1 campaign receipt (omega COMPOSITION-2 style) from one
# run-campaign.sh run. Shell + jq only. Usage:
#   make-receipt.sh RUN_ROOT OUT_DIR SC_COMMIT OMEGA_COMPOSE_COMMIT OMEGA_GPU_COMMIT [RESCUES] [NOTE]
# Writes OUT_DIR/<sha256 of content>.json (never edited afterwards), appends
# one line to OUT_DIR/INDEX.md, writes OUT_DIR/<sha256>.summary.txt, and
# prints the summary. Thresholds are the frozen ACCEPTANCE.md rows, unchanged.
set -eu
R=$1 OUT=$2 SC=$3 OMC=$4 OMG=$5 RESCUES=${6:-0} NOTE=${7:-}
S=$R/steps
j() { if [ -s "$1" ]; then jq -c . "$1" 2>/dev/null || echo null; else echo null; fi; }
skill_sha=$(printf '%s' 'aien.model.propose-file-change' | sha256sum | cut -d' ' -f1)
receipts=$(for p in "$R"/prov/*.json; do [ -f "$p" ] && jq -c --arg p "$p" --arg s "$(sha256sum "$p" | cut -d' ' -f1)" \
  '{path:$p, sha256:$s, tool, success, arguments_digest, result_digest}' "$p"; done | jq -sc .)
tmp=$(mktemp "$OUT/.receipt.XXXXXX")
jq -n --argjson run "$(j "$R/run.json")" \
  --argjson s1 "$(j "$S/S1.json")" --argjson s2 "$(j "$S/S2.json")" --argjson s3 "$(j "$S/S3.json")" \
  --argjson s4 "$(j "$S/S4.json")" --argjson s5 "$(j "$S/S5.json")" --argjson s6 "$(j "$S/S6.json")" \
  --argjson s8 "$(j "$S/S8.json")" --argjson pre "$(j "$S/pre-restart-recall.json")" \
  --argjson receipts "$receipts" --arg sc "$SC" --arg omc "$OMC" --arg omg "$OMG" \
  --arg skill_sha "$skill_sha" --argjson rescues "$RESCUES" --arg note "$NOTE" '
  def st($id): ($run.steps // []) | map(select(.step == $id)) | (.[0] // {});
  def okv: if . then "PASS" else "FAIL" end;
  ($s3.report // {}) as $rep
  | ($s1.constraint.id // null) as $cid
  | ($s5.effect.id // null) as $eid
  | ($s8.constraints // []) as $c8
  | ($s8.effects // []) as $e8
  | ($receipts | map(select(.tool == "authorize"))) as $auths
  | ($receipts | map(select(.tool == "write_file"))) as $writes
  | ($receipts | map(select(.tool == "inspect"))) as $inspects
  | {
    S1: (($s1.ok // false) and $cid != null),
    S2: (($s2.ok // false) and ($inspects | length) >= 1),
    S3: (($rep.committed // false) and (($rep.cx_candidates // []) | length) > 0
         and ($rep.cx_evidence // 0) > 0 and ($rep.cx_promotion // 0) > 0),
    S4: (($s4.ok // false) and ($auths | length) == 1
         and ($s4.authorization.id // null) != null),
    S5: (($s5.ok // false) and ($writes | map(select(.success == true)) | length) == 1),
    S6: (($s6.ok // false) and (($s6.cited // []) | length) > 0
         and ((($s6.cited // []) | map(.verified) | all))),
    S7: (st("S7").ok // false),
    S8: (($s8.ok // false) and ($c8 | map(.id) | index($cid)) != null
         and ($e8 | map(.id) | index($eid)) != null
         and ($s8.machine_id // "x") == ($rep.machine_id // "y"))
  } as $reached
  | ($pre.recall.cited // []) as $precited
  | ($s8.recall.cited // []) as $postcited
  | ($c8 | map(select(.id == $cid)) | .[0].text // null) as $recalled
  | ($run.compose_dir_files // []) as $cfiles
  | ($cfiles | map(select(test("^\\./(machine\\.id|cortex\\.cx|jspace)") | not))) as $stray
  | ($run.daemon // []) as $d
  | (($d | map(.backend // "") | map(test("CPU-reference") | not))) as $gpu
  | [
    {criterion:"Task completion", threshold:"8 of 8 (PROPOSED)",
     value: ([$reached[]] | map(select(.)) | length), reached:$reached,
     result: (([$reached[]] | all) | okv)},
    {criterion:"Correctness: committed content", threshold:"byte-identical, digests equal (PROPOSED)",
     value:{s3_content_sha256:$rep.proposal_content_sha256, s4_content_sha256:(($s8.authorizations // [])[0].text // "{}" | fromjson | .content_sha256),
            s5_disk_sha256:($s5.disk_sha256 // null)},
     result: (($rep.proposal_content_sha256 // "a") == ($s5.disk_sha256 // "b")) | okv},
    {criterion:"Correctness: recall", threshold:"byte-identical (PROPOSED)",
     value:{stored_at_s1:$run.constraint_text, recalled_at_s8:$recalled},
     result: ($recalled != null and $recalled == $run.constraint_text) | okv},
    {criterion:"Latency", threshold:"report only",
     value: (($run.steps // []) | map({step, wall_ms, boottime_ms})), result:"REPORTED"},
    {criterion:"Resource use", threshold:"report only",
     value:{vmhwm_kb_before_restart:$d[0].vmhwm_kb, vmhwm_kb_after_restart:$d[1].vmhwm_kb,
            gpu_used:($gpu | all), backend:($d | map(.backend))}, result:"REPORTED"},
    {criterion:"Human interventions: approvals", threshold:"exactly 1, for the committed effect (PROPOSED)",
     value:{authorize_receipts:($auths | length), authorization_records_at_s8:(($s8.authorizations // []) | length)},
     result: (($auths | length) == 1 and (($s8.authorizations // []) | length) == 1) | okv},
    {criterion:"Human interventions: rescues", threshold:"0 (PROPOSED)", value:$rescues,
     result: ($rescues == 0) | okv},
    {criterion:"Identity", threshold:"identical digest (PROPOSED)",
     value:{before_s7:$rep.machine_id, after_s7:$s8.machine_id, machine_id_file_sha256:$run.machine_id_file_sha256},
     result: (($rep.machine_id // "a") == ($s8.machine_id // "b")
              and ($run.machine_id_file_sha256[0] // "a") == ($run.machine_id_file_sha256[1] // "b")) | okv},
    {criterion:"Memory", threshold:"exists, digest verified, equal to pre-restart digest (PROPOSED)",
     value:{cited:($run.cited), missing_after:($s8.recall.missing // null),
            prefix_records:$run.prefix_records,
            prefix_digest:[($pre.recall.prefix_digest // null), ($s8.recall.prefix_digest // null)]},
     result: ((($run.cited // []) | length) > 0
              and (($s8.recall.missing // [1]) | length) == 0
              and ($postcited | map(.verified) | all)
              and ($precited | map({id, digest})) == ($postcited | map({id, digest}))
              and ($pre.recall.prefix_digest // "a") == ($s8.recall.prefix_digest // "b")) | okv},
    {criterion:"Containment: workspace", threshold:"empty (PROPOSED)",
     value:{outside_new_files:$run.containment.outside_new_files,
            outside_sentinel:[$run.containment.outside_sentinel_before, $run.containment.outside_sentinel_after],
            workspace_tree_sha256:[$run.containment.workspace_before, $run.containment.workspace_after],
            workspace_changed:$run.containment.workspace_changed,
            daemon_state_files:$run.containment.daemon_state_files},
     result: ((($run.containment.outside_new_files // [1]) | length) == 0
              and $run.containment.outside_sentinel_before == $run.containment.outside_sentinel_after
              and ($run.containment.workspace_changed // []) == [($rep.proposal_path // "?")]) | okv},
    {criterion:"Containment: speculation", threshold:"none (PROPOSED)",
     value:{compose_dir_files:$cfiles, other_files:$stray},
     result: (($cfiles | length) > 0 and ($stray | length) == 0) | okv}
  ] as $rows
  | {
    gate:"NEXT_PHASE_1",
    commit:$sc,
    omega_compose_commit:$omc,
    omega_gpu_commit:$omg,
    verdict: (if ($rows | map(select(.result == "FAIL")) | length) == 0 then "PASS" else "FAIL" end),
    machine_id:($rep.machine_id // null),
    tier:"host",
    uname:$run.uname,
    model:($d | map(.model)),
    note:$note,
    skills:[{id:0, name:"aien.model.propose-file-change", version:null, digest:$skill_sha, cost:10}],
    record_digest:[($rep.record_digest // null)],
    prefix_digest:[($pre.recall.prefix_digest // null), ($s8.recall.prefix_digest // null)],
    winner_digest:[($rep.winner_digest // null)],
    acceptance:$rows,
    effect_receipts:$receipts,
    explanation:($s6.text // null),
    runs:[{
      run:0, task:$rep.task, outcome:$rep.outcome, committed:$rep.committed,
      old_ref:null, new_ref:null, candidate_refs:null,
      branch_count:$rep.branch_count, branches_reclaimed:$rep.branches_reclaimed,
      winner:$rep.winner, aegis_pass_mask:$rep.aegis_pass_mask,
      goal_crumb:$rep.cx_goal, claims:$rep.cx_candidates, evidence:$rep.cx_evidence,
      promotion:$rep.cx_promotion, admissions:$rep.cx_admissions,
      proposer:$rep.proposer, proposal_path:$rep.proposal_path,
      proposal_sha256:$rep.proposal_sha256, proposal_content_sha256:$rep.proposal_content_sha256,
      uncommitted_proposal:($rep.uncommitted_proposal // null), proposer_error:($rep.proposer_error // null),
      steps: [
        {step:1, name:"S1 remember", result:($reached.S1|okv), detail:{ms:st("S1").wall_ms, constraint:$s1.constraint}},
        {step:2, name:"S2 inspect", result:($reached.S2|okv), detail:{ms:st("S2").wall_ms, tree_sha256:$s2.tree_sha256, receipt:$s2.receipt, effect:$s2.effect.id}},
        {step:3, name:"S3 propose", result:($reached.S3|okv), detail:{ms:st("S3").wall_ms, gpu_used:$gpu[0], backend:$d[0].backend}},
        {step:4, name:"S4 authorize", result:($reached.S4|okv), detail:{ms:st("S4").wall_ms, approvals:$s4.approvals, receipt:$s4.receipt, authorization:$s4.authorization.id}},
        {step:5, name:"S5 execute", result:($reached.S5|okv), detail:{ms:st("S5").wall_ms, path:$s5.path, content_sha256:$s5.content_sha256, disk_sha256:$s5.disk_sha256, receipt:$s5.receipt, effect:$s5.effect.id}},
        {step:6, name:"S6 explain", result:($reached.S6|okv), detail:{ms:st("S6").wall_ms, cited:(($s6.cited // []) | map({id, digest, verified})), receipts:$s6.receipts}},
        {step:7, name:"S7 restart", result:($reached.S7|okv), detail:(st("S7") + {vmhwm_kb_before:$d[0].vmhwm_kb})},
        {step:8, name:"S8 recall", result:($reached.S8|okv), detail:{ms:st("S8").wall_ms, machine_id:$s8.machine_id, constraints:(($s8.constraints // []) | map(.id)), effects:(($s8.effects // []) | map(.id)), vmhwm_kb_after:$d[1].vmhwm_kb, gpu_used:$gpu[1]}}
      ]}]
  }' >"$tmp"
h=$(sha256sum "$tmp" | cut -d' ' -f1)
mv "$tmp" "$OUT/$h.json"
v=$(jq -r .verdict "$OUT/$h.json")
{
  echo "NEXT_PHASE_1 receipt $h.json verdict $v"
  echo "sovereign-core $SC; omega compose $OMC; omega GPU engine $OMG"
  jq -r '"machine \(.machine_id); backend \(.acceptance[4].value.backend | join(" / ")); gpu_used \(.acceptance[4].value.gpu_used)"' "$OUT/$h.json"
  jq -r '.runs[0].steps[] | "  \(.name): \(.result)  \(.detail.ms // .detail.wall_ms // "-") ms"' "$OUT/$h.json"
  echo "Acceptance (frozen thresholds, ACCEPTANCE.md spec_version 1):"
  jq -r '.acceptance[] | "  \(.result)  \(.criterion)  [\(.threshold)]  \(.value | tojson | .[0:220])"' "$OUT/$h.json"
  [ -n "$NOTE" ] && echo "Note: $NOTE"
} >"$OUT/$h.summary.txt"
[ -f "$OUT/INDEX.md" ] || printf '# NEXT-PHASE-1 campaign receipts\n\nEach receipt is named by the sha256 of its content and never edited.\n\n' >"$OUT/INDEX.md"
echo "- \`$h.json\` verdict $v, sovereign-core $SC ($(date -u +%Y-%m-%dT%H:%MZ))" >>"$OUT/INDEX.md"
cat "$OUT/$h.summary.txt"
