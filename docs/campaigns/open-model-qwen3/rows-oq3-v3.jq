# OPEN-MODEL-QWEN3 v3 extra rows (ACCEPTANCE-v3.md Section 4), jq module. FROZEN when ACCEPTANCE-v3.md is on main.
# These rows are computed beside the v8 receipt rows, not inside the receipt (the receipt tool and
# rows-v8.jq stay byte-identical; ACCEPTANCE-v3 Section 6). v3-rows.sh gathers the evidence of one
# launch; v3_rows turns it into [{row, criterion, threshold, value, result}].
#
# Evidence object $ev:
#   task        the launch's entry of tasks-oq3-v3.json
#   report      the S3 report (s3-report.json) or null
#   s5          S5.json (execute step output) or {}
#   auths       the authorization records at S8, each the parsed record text plus {id}
#   committed   {path_rel, text, file_sha256}: the file in the workspace, hashed by v3-rows.sh itself
#               after the run (not read from the runtime's own report); nulls when there is none
# Results are PASS or FAIL; a missing input is FAIL (fail closed).

def v3_okv: if . then "PASS" else "FAIL" end;
def v3_rtrim: sub("\\s+$"; "");
def v3_lines: split("\n") | map(v3_rtrim);
def v3_fence: test("^\\s*(```|~~~)");
def v3_bare_fence: test("^\\s*(```+|~~~+)\\s*$");
def v3_nonempty($t): ($t | split("\n") | map(select(test("\\S"))) | length);
def v3_headings($t): ($t | split("\n") | map(select(test("^#{1,6} +\\S"))) | length);

# The accepted attempt: the last attempt of the S3 report whose outcome is "parsed".
def v3_accepted: ((.report.proposal_attempts // []) | map(select(.outcome == "parsed")) | last);

# Independent reading of a model reply as a document (ACCEPTANCE-v3 Section 4, row <id>-F), written from the
# merged parser's documented rule (sovereign-core 8f3e8c8, spine.rs check_file_proposal / outer_fence_close):
# the first non-blank line is the "filename:" line and is dropped; blank lines after it are dropped; if the
# next line starts (after indentation) with three or more backticks, that opener is dropped and its length n
# is taken; among the lines after it, a "fence" is a line of at least n backticks (an info string is allowed
# but not a backtick in it); a fence with an info string opens a nested block (depth + 1); a bare fence at
# depth > 0 closes it (depth - 1, remembered as the last bare fence); a bare fence at depth 0 that is the last
# fence closes the outer fence; a bare fence at depth 0 with fences after it opens a bare inner block (depth + 1).
# The outer close is the first outer-closing line, else the last remembered bare fence, else none. Everything
# from the close on is dropped (none: the body is kept whole), then trailing blank lines. Tilde fences are
# not an outer opener for the parser and are left in the text.
def v3_fence_info:
  sub("^\\s+"; "") | capture("^(?<b>`*)(?<r>.*)$") | (.b | length) as $n | (.r | sub("^\\s+"; "") | sub("\\s+$"; "")) as $info
  | if $n < 3 or ($info | contains("`")) then null else {n: $n, info: ($info != "")} end;
def v3_outer_close($body; $open):
  ([$body | to_entries[] | (.value | v3_fence_info) as $f | select($f != null and $f.n >= $open) | {i: .key, info: $f.info}]) as $fx
  | (reduce range(0; $fx | length) as $k ({depth: 0, last: null, ret: null};
      if .ret != null then .
      elif $fx[$k].info then .depth += 1
      elif .depth > 0 then .depth -= 1 | .last = $fx[$k].i
      elif $k + 1 == ($fx | length) then .ret = $fx[$k].i
      else .depth += 1 end)) as $s
  | ($s.ret // $s.last);
def v3_doc_from_reply:
  v3_lines as $l
  | (($l | map(test("\\S")) | index(true))) as $f
  | (if $f == null then [] else $l[$f + 1:] end) as $rest
  | (($rest | map(test("\\S")) | index(true)) // ($rest | length)) as $skip
  | $rest[$skip:] as $b
  | (if ($b | length) > 0 and ($b[0] | sub("^\\s+"; "") | startswith("```"))
     then ($b[0] | sub("^\\s+"; "") | capture("^(?<b>`*)") | .b | length) as $open
          | $b[1:] as $in | (v3_outer_close($in; $open)) as $c
          | (if $c == null then $in else $in[0:$c] end)
     else $b end) as $d
  | ($d | map(test("\\S")) | rindex(true)) as $last
  | (if $last == null then [] else $d[0:$last + 1] end)
  | join("\n") | v3_rtrim;

# SB: saved bytes equal approved bytes. The file's own sha256 is computed by the scorer after the run.
def v3_row_sb($id):
  . as $ev | ($ev.auths | .[0] // {}) as $auth
  | {row: "\($id)-SB", criterion: "Saved bytes equal approved bytes",
     threshold: "exactly one authorization; the sha256 of the file found in the workspace (computed by the scorer, not the runtime) == the authorization content_sha256 == S5 content_sha256 == S5 disk_sha256 == the S3 proposal content sha256; the file path == the task destination",
     value: {authorizations: ($ev.auths | length), file_sha256: $ev.committed.file_sha256, approved: ($auth.content_sha256 // null),
             s5_content: ($ev.s5.content_sha256 // null), s5_disk: ($ev.s5.disk_sha256 // null),
             s3_content: ($ev.report.proposal_content_sha256 // null), path: $ev.committed.path_rel, destination: $ev.task.destination},
     result: (($ev.auths | length) == 1 and ($ev.committed.file_sha256 | type) == "string" and ($ev.committed.file_sha256 | length) > 0
              and $ev.committed.file_sha256 == ($auth.content_sha256 // null)
              and $ev.committed.file_sha256 == ($ev.s5.content_sha256 // null)
              and $ev.committed.file_sha256 == ($ev.s5.disk_sha256 // null)
              and $ev.committed.file_sha256 == ($ev.report.proposal_content_sha256 // null)
              and $ev.committed.path_rel == $ev.task.destination) | v3_okv};

# F: complete document saved, no truncation at embedded fences (long launches only).
def v3_row_f($id):
  . as $ev | ($ev.committed.text) as $t | (v3_accepted) as $acc
  | (if $t == null then null else $t | v3_lines end) as $L
  | (if $L == null then [] else [$L | to_entries[] | select(.value | v3_fence) | .key] end) as $fx
  | (($ev.task.min_code_blocks // 0)) as $minb
  | (if ($fx | length) == 0 then null else $fx[-1] end) as $lastf
  | (if $L == null then [] elif $lastf == null or $minb == 0 then [] else $L[$lastf + 1:] end) as $after
  | (($after | join(" ") | [scan("\\S+")] | length)) as $tailw
  | (if $acc == null then null else ($acc.text | v3_doc_from_reply) end) as $rd
  | (if $t == null then null else ($t | v3_lines | join("\n") | v3_rtrim) end) as $ct
  | {row: "\($id)-F", criterion: "Complete document saved (no truncation at embedded fences)",
     threshold: "the file is read; its fence lines (``` or ~~~ at line start) are even in number and at least 2 x min_code_blocks; when min_code_blocks > 0 the text after the last fence line has at least min_tail_words words and, if tail_heading is set, a line equal to it; the saved text equals the document an independent reading takes from the accepted reply (outer fence stripped, trailing whitespace ignored)",
     value: {fence_lines: ($fx | length), min_code_blocks: $minb, last_fence_line: $lastf, words_after_last_fence: $tailw,
             min_tail_words: ($ev.task.min_tail_words // 0), tail_heading_found: (if ($ev.task.tail_heading // null) == null then null else ($after | map(. == $ev.task.tail_heading) | any) end),
             saved_lines: (if $L == null then null else ($L | length) end), reply_document_lines: (if $rd == null then null else ($rd | split("\n") | length) end),
             saved_equals_reply_document: ($ct != null and $rd != null and $ct == $rd)},
     result: ($t != null and $acc != null and ($fx | length) % 2 == 0 and ($fx | length) >= 2 * $minb
              and ($minb == 0 or ($tailw >= ($ev.task.min_tail_words // 0)
                                   and (($ev.task.tail_heading // null) == null or ($after | map(. == $ev.task.tail_heading) | any))))
              and $ct == $rd) | v3_okv};

# RQ: OUTCOME-BASED (ACCEPTANCE-v3 Section 12, decision of the orchestrator 2026-10-07). The row passes iff the bytes
# actually saved at the destination meet every measurable requirement the task's goal states, computed here from the
# task's declared requirement fields, whether or not the product recognized them. Nothing saved: FAIL. Whether the
# runtime recognized each requirement (S3 report requirements_recognized) is information in the receipt side-file
# (value.recognized_by_runtime), not part of the result.
# Counting rules are those of crates/aien-runtime/src/requirements.rs (doc comments, "Counting", requirements.rs:36-41):
#   lines    every non-empty line (a non-whitespace character), code lines and fence marker lines included;
#   headings markdown headings, 1 to 6 "#" then a space then text, skipping fenced code blocks (``` or ~~~,
#            CommonMark close rule: same marker, at least as long, no info string; an unclosed fence runs to the end)
#            and the fence lines themselves (table at requirements.rs:24, "sections" = markdown headings).
# A declared requirement may carry "titles": each title must equal (case-insensitive, trimmed) the text of a heading
# found by that rule (G3: the six named sections).
def v3_heading_texts($t):
  (reduce ($t | split("\n"))[] as $ln ({open: null, h: []};
     ($ln | sub("\\s+$"; "")) as $l
     | if .open == null then
         ($l | capture("^\\s*(?<m>`{3,}|~{3,})(?<i>.*)$"; "") // null) as $o
         | if $o != null and (($o.m | startswith("`") | not) or ($o.i | contains("`") | not))
           then .open = {c: ($o.m | .[0:1]), n: ($o.m | length)}
           elif ($l | test("^#{1,6} +\\S")) then .h += [$l | sub("^#{1,6} +"; "") | sub(" +#+$"; "") | sub("^\\s+"; "") | sub("\\s+$"; "") | ascii_downcase]
           else . end
       else
         (.open) as $op
         | if ($l | test("^\\s*" + (if $op.c == "`" then "`" else "~" end) + "{" + ($op.n | tostring) + ",}\\s*$")) then .open = null else . end
       end)) | .h;
def v3_req_found($t): . as $r
  | if $r.kind == "min_lines" then v3_nonempty($t)
    elif $r.kind == "min_headings" then (v3_heading_texts($t) | length)
    else null end;
def v3_req_ok($t): . as $r
  | if $r.kind == "min_lines" then v3_nonempty($t) >= $r.n
    elif $r.kind == "min_headings" then
      (v3_heading_texts($t)) as $h
      | ($h | length) >= $r.n and (($r.titles // []) | all(. as $x | ($h | index($x | sub("^\\s+"; "") | sub("\\s+$"; "") | ascii_downcase)) != null))
    else false end;
def v3_row_rq($id):
  . as $ev | ($ev.committed.text) as $t | ($ev.task.requirements // []) as $reqs
  | ($ev.report.requirements_recognized // null) as $rec
  | {row: "\($id)-RQ", criterion: "Stated requirements met (outcome of the saved bytes)",
     threshold: "the file saved at the destination exists and meets every requirement declared for the task, counted by the campaign tooling on the saved text by the requirements.rs rules (non-empty lines including fence lines; markdown headings outside fenced code; each declared title present as a heading); nothing saved = FAIL; whether the runtime recognized the requirement does not matter",
     value: {declared: ($reqs | map(.label)), recognized_by_runtime: $rec,
             recount: (if $t == null then null else $reqs | map({label, n, titles, found: v3_req_found($t),
                         titles_missing: (if .kind == "min_headings" then ((.titles // []) | map(select(. as $x | (v3_heading_texts($t) | index($x | sub("^\\s+"; "") | sub("\\s+$"; "") | ascii_downcase)) == null))) else null end),
                         met: v3_req_ok($t)}) end)},
     result: ($t != null and ($reqs | length) > 0 and ($reqs | all(v3_req_ok($t)))) | v3_okv};

# D: one wall-clock deadline for all attempts of the launch, and no attempt ended by timeout.
def v3_row_d($id; $positive):
  . as $ev | ($ev.report.proposal_attempts // []) as $at | ($ev.task.budget_ms) as $b
  | ($at | map(.ms // 0) | add // 0) as $sum
  | {row: "\($id)-D", criterion: "One deadline for all attempts",
     threshold: ("at least one attempt; the attempts total ms <= the task budget_ms (edit 29000, document 120000); no attempt reason contains \"exceeded\" (the wall-clock timeout wording)" + (if $positive then "; an accepted (parsed) attempt exists" else "" end)),
     value: {budget_ms: $b, total_attempt_ms: $sum, attempts: ($at | map({attempt, outcome, ms, tokens, finish_reason, reason}))},
     result: (($at | length) > 0 and $b != null and $sum <= $b
              and ($at | all(((.reason // "") | contains("exceeded")) | not))
              and ((($positive | not)) or (v3_accepted != null))) | v3_okv};

# N2-R: a token-limit cut is its own reason, never a wall-clock timeout.
def v3_row_n2r:
  . as $ev | ($ev.report.proposal_attempts // []) as $at
  | {row: "N2-R", criterion: "Token-limit cut is not a timeout",
     threshold: "at least one attempt; every attempt has finish_reason max_tokens and a reason containing \"token limit\" and not containing \"exceeded\"",
     value: {attempts: ($at | map({attempt, outcome, finish_reason, reason}))},
     result: (($at | length) > 0
              and ($at | all(.finish_reason == "max_tokens" and ((.reason // "") | contains("token limit")) and (((.reason // "") | contains("exceeded")) | not)))) | v3_okv};

def v3_rows:
  . as $ev | ($ev.task.id) as $id | ($ev.task.kind) as $k
  | (if $k == "long" then [v3_row_sb($id), v3_row_f($id), (if (($ev.task.requirements // []) | length) > 0 then v3_row_rq($id) else empty end), v3_row_d($id; true)]
     elif $k == "edit" or $k == "identity" then [v3_row_sb($id), v3_row_d($id; true)]
     elif $k == "negative-boundary" then [v3_row_d($id; false)]
     elif $k == "negative-budget" then [v3_row_d($id; false), v3_row_n2r]
     else [{row: "?", criterion: "unknown launch kind", threshold: "-", value: $ev.task, result: "FAIL"}] end);
