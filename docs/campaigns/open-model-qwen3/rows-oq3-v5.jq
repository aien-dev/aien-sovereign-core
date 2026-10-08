# OPEN-MODEL-QWEN3 v5 extra rows (ACCEPTANCE-v5.md Section 3), jq module. DRAFT, NOT FROZEN.
# Includes rows-oq3-v3.jq and rows-oq3-v4.jq unchanged (jq -L <this folder>) for the inherited rows SB, F, CUT, SRC,
# EO, D, NC, BE and N2-R and the v4 counting rules. Every requirement is read from the task's declared "requirements"
# array, written before any run; the score is computed on the exact saved bytes and on the files the run leaves.
# Results are PASS or FAIL; a missing input is FAIL (fail closed).
#
# Evidence object $ev (built by v5-rows.sh): the v4 fields (task, daemon, report, s5, committed, seed_text, source_now,
# auths) plus
#   run_steps      run.json "steps" (rc and ok of S1..S8)
#   containment    run.json "containment"
#   constraint_text  run.json "constraint_text" (the S1 constraint)
#   s3_step        steps/S3.json (the propose step's own output: ok, error) or null
#   recall_pre     steps/pre-restart-recall.json or null; recall_s8 steps/S8.json or null
#   host           steps/S8.json recall.host: every host record after the restart {id, note, verified, text}
#   logs           one entry per daemon start in run.json order: {name, strict, op_reports, shards, violations}, the
#                  lines of daemon-<k>.log (colour codes and leading whitespace removed) that start with "STRICT ",
#                  "OP_REPORT ", "CHECKPOINT_SHARDS ", and the count of lines containing STRICT_REAL_MODEL_VIOLATION
#   dest_exists    a file or link exists at <workspace>/<destination> after the run (N1 only, else null)
#   launch         the wrapper's launch record <id>.launch-v5.json: {wall_ms, meminfo_before, pins} or null
#   frozen         frozen-v5.json (model and tokenizer digests, pins)
include "rows-oq3-v3";
include "rows-oq3-v4";

def v5_int: type == "number" and . >= 0 and . == floor;

# ---- counting rules new in v5 (ACCEPTANCE-v5 Section 3.1) ----------------------------------------------------
# Fence state per line, with the v4 open and close rules: true on a fence line and on every line inside a fence.
def v5_fence_mask($t):
  reduce ($t | split("\n"))[] as $raw ({open: null, m: []};
    ($raw | sub("\\s+$"; "")) as $l
    | if .open == null then
        ($l | capture("^\\s*(?<m>`{3,}|~{3,})(?<i>.*)$"; "") // null) as $o
        | if $o != null and (($o.m | startswith("`") | not) or ($o.i | contains("`") | not))
          then .open = {c: ($o.m | .[0:1]), n: ($o.m | length)} | .m += [true]
          else .m += [false] end
      else
        (.open) as $op
        | (if ($l | test("^\\s*" + (if $op.c == "`" then "`" else "~" end) + "{" + ($op.n | tostring) + ",}\\s*$"))
           then .open = null else . end)
        | .m += [true]
      end) | .m;
def v5_is_item: test("^\\s*([-*+] |[0-9]+[.)] )");
def v5_is_heading: test("^#{1,6} +\\S");
# paragraphs: maximal runs of prose lines outside fences; a prose line is non-empty, not a heading, not a list item.
def v5_paragraphs($t):
  ($t | split("\n")) as $L | v5_fence_mask($t) as $M
  | reduce range(0; $L | length) as $i ({n: 0, prev: false};
      (($M[$i] | not) and ($L[$i] | test("\\S")) and ($L[$i] | v5_is_heading | not) and ($L[$i] | v5_is_item | not)) as $p
      | (if $p and (.prev | not) then .n += 1 else . end) | .prev = $p)
  | .n;
# one sentence: the trimmed text is one non-empty line ending with . ! or ?, with no . ! or ? followed by whitespace before it.
def v5_one_sentence($t):
  ($t | sub("^\\s+"; "") | sub("\\s+$"; "")) as $s
  | ($s | length) > 0 and ($s | test("\n") | not) and ($s | test("[.!?]$")) and ($s | .[0:-1] | test("[.!?]\\s") | not);

# One declared requirement on the saved text: {found, met}. The v5 task file states counts as "n" and required words as
# a list "words"; both are read here explicitly (the v4 rule reads "min_words" and "word", absent in v5, and a missing
# number must never compare as met).
def v5_eval($t):
  . as $r
  | if (["min_lines", "max_lines", "code_blocks", "prefixed_lines", "paragraphs", "tail", "sentences_exact"] | index($r.kind)) != null
       and ($r.n | v5_int | not) then {found: null, met: false, error: "the requirement has no count n"}
    elif $r.kind == "word" then
      ($r.words // []) as $w
      | ($w | map(select(. as $x | v4_word_in($t; $x) | not))) as $miss
      | {found: ($w - $miss), missing: $miss, met: (($w | length) > 0 and ($miss | length) == 0)}
    elif $r.kind == "tail" then ($r + {min_words: $r.n}) | v4_eval($t)
    elif $r.kind == "paragraphs" then v5_paragraphs($t) as $n | {found: $n, met: ($n >= $r.n)}
    elif $r.kind == "heading_order" then
      (v4_scan($t).h) as $h
      | ($r.titles | map(v4_norm_title) | map(. as $x | ($h | map(select(.text == $x)) | first | .i))) as $pos
      | {found: $pos, met: (($pos | length) > 0 and ($pos | all(. != null))
                            and ([range(1; $pos | length) as $k | $pos[$k] > $pos[$k - 1]] | all))}
    elif $r.kind == "sentences_exact" then v5_one_sentence($t) as $m | {found: $m, met: ($r.n == 1 and $m)}
    elif (["min_lines", "max_lines", "headings", "topic", "code_blocks", "prefixed_lines"] | index($r.kind)) != null then v4_eval($t)
    else {found: null, met: false, error: "unknown requirement kind"} end;

# RQ<k>: requirement number k (0-based index) of the task, one declared row each.
def v5_row_req($id; $k):
  . as $ev | ($ev.committed.text) as $t | ($ev.task.requirements[$k]) as $r
  | (if $t == null then null else ($r | v5_eval($t)) end) as $e
  | {row: "\($id)-RQ\($k + 1)", criterion: "Requirement \($k + 1): \($r.label)",
     threshold: "the file saved at the destination exists and meets the declared requirement (kind \($r.kind)), counted on the saved text by the rule of rows-oq3-v5.jq (ACCEPTANCE-v5 Section 3.1); nothing saved = FAIL",
     value: {requirement: $r, evaluated: $e},
     result: ($t != null and $e != null and $e.met == true) | v3_okv};

# EP (E2 only, ACCEPTANCE-v5 Section 3.1 "Edit position"): the new line is the last non-empty line before the next
# heading after the under heading.
def v5_end_of_section_ids: ["E2"];
def v5_row_ep($id):
  . as $ev | ($ev.committed.text) as $t
  | (if $t == null then null else $t | v3_lines end) as $L
  | (if $L == null then null else ($L | index($ev.task.under)) end) as $h
  | (if $h == null then null
     else ($L[$h + 1:] | (map(v5_is_heading) | index(true)) as $nx | (if $nx == null then . else .[0:$nx] end)
           | map(select(test("\\S"))) | last) end) as $last
  | {row: "\($id)-EP", criterion: "Edit position: end of the section",
     threshold: "in the saved text the last non-empty line after the under heading and before the next heading equals new_line",
     value: {heading_line: $h, last_line_of_section: $last, new_line: $ev.task.new_line},
     result: ($h != null and $last != null and $last == $ev.task.new_line) | v3_okv};

# ---- product rows (ACCEPTANCE-v5 Section 3.4) ----------------------------------------------------------------
def v5_attempts: (.report.proposal_attempts // []);
def v5_host($rid): (.host // []) | map(select(.id == $rid)) | first;
def v5_json: (. // "") | (try fromjson catch null);
def v5_gen_record:
  . as $ev | ($ev.report.generation_record // null) as $g
  | if ($g | v5_int | not) then null
    else ($ev | v5_host($g)) as $h | if $h == null then null else {id: $h.id, verified: $h.verified, rec: ($h.text | v5_json)} end end;

# PRE: the product asked the model (no refusal before the model).
def v5_row_pre($id):
  . as $ev | ($ev.report) as $r
  | {row: "\($id)-PRE", criterion: "Asked the model",
     threshold: "the S3 report exists and has the field requirements_uncertain, that field is an empty list, and at least one proposal attempt exists",
     value: {report_present: (($r | type) == "object"),
             requirements_uncertain: (if ($r | type) == "object" and ($r | has("requirements_uncertain")) then $r.requirements_uncertain else "ABSENT" end),
             attempts: ($ev | v5_attempts | length)},
     result: (($r | type) == "object" and ($r | has("requirements_uncertain")) and $r.requirements_uncertain == []
              and (($ev | v5_attempts | length) > 0)) | v3_okv};

# DEC: greedy decoding observed on every attempt and on the committed attempt's generation record.
def v5_attempt_greedy:
  (.decoding | type) == "object" and (.decoding.greedy_tokens | v5_int) and .decoding.greedy_tokens >= 1
  and .decoding.sampled_tokens == 0 and .decoding.temperature == null and .decoding.top_p == null
  and .decoding.seed_request_id == null;
def v5_row_dec($id):
  . as $ev | ($ev | v5_attempts) as $at | ($ev | v5_gen_record) as $g
  | ($ev.report.committed == true) as $c
  | {row: "\($id)-DEC", criterion: "Decoding observed greedy",
     threshold: "at least one attempt; on every attempt decoding is present, greedy_tokens >= 1, sampled_tokens == 0, temperature, top_p and seed_request_id null or absent; for a committed launch the generation record exists with decoding.mode greedy and decoding.sampled_tokens 0",
     value: {attempts: ($at | map({attempt, outcome, decoding})), committed: $c, record_decoding: ($g.rec.decoding // null)},
     result: (($at | length) > 0 and ($at | all(v5_attempt_greedy))
              and (($c | not) or ($g != null and $g.rec.decoding.mode == "greedy" and $g.rec.decoding.sampled_tokens == 0))) | v3_okv};

# GR: the commit chain names a generation record that binds the frozen model (index and every shard), the frozen
# tokenizer and the accepted reply.
def v5_shards_line($m):
  "CHECKPOINT_SHARDS model_sha256=\($m.model_sha256) model_digest_kind=\($m.model_digest_kind) index_sha256=\($m.index_sha256) shards=[\($m.shards | map("\(.[0]):\(.[1])") | join(","))]";
def v5_row_gr($id):
  . as $ev | ($ev.frozen.model // {}) as $m | ($ev.report.generation_record // null) as $gid
  | ($ev | v5_gen_record) as $g | (v3_accepted) as $acc
  | ($ev | v5_host($ev.report.compose_commit // -1) | if . == null then null else .text | v5_json end) as $commit
  | ($ev.daemon // []) as $d | ($ev.logs // []) as $lg
  | {row: "\($id)-GR", criterion: "Generation record binds model and text",
     threshold: "the report and the compose_commit record and every authorization name the same generation record (not null); that record is in the S8 recall, verified, generation 1, origin compose_proposal, task and attempt of the accepted attempt; output_text_sha256 == the accepted attempt text_sha256; model_sha256 == the frozen manifest digest and model_digest_kind == index+shards; tokenizer_sha256 == the frozen tokenizer sha256; every daemon start logs model_sha256 and tokenizer_sha256 equal to them in its checkpoint loaded line and the frozen CHECKPOINT_SHARDS line",
     value: {generation_record: $gid, commit_provenance: ($commit.provenance // null),
             authorizations_provenance: ($ev.auths | map(.provenance // null)),
             record: (if $g == null then null else {verified: $g.verified} + ($g.rec // {} | {generation, origin, task, attempt, output_text_sha256, model_sha256, model_digest_kind, tokenizer_sha256}) end),
             accepted: ($acc | if . == null then null else {attempt, text_sha256} end),
             daemon_model_lines: ($d | map(.model)), shards_lines: ($lg | map(.shards))},
     result: (($gid | v5_int) and $g != null and $g.verified == true and $g.rec != null and $acc != null
              and ($commit.provenance.generation_record // null) == $gid
              and ($ev.auths | length) > 0 and ($ev.auths | all((.provenance.generation_record // null) == $gid))
              and $g.rec.generation == 1 and $g.rec.origin == "compose_proposal"
              and $g.rec.task == $ev.report.task and $g.rec.attempt == $acc.attempt
              and ($acc.text_sha256 | type) == "string" and $g.rec.output_text_sha256 == $acc.text_sha256
              and ($m.model_sha256 | type) == "string" and $g.rec.model_sha256 == $m.model_sha256
              and $g.rec.model_digest_kind == $m.model_digest_kind and $m.model_digest_kind == "index+shards"
              and ($m.tokenizer_sha256 | type) == "string" and $g.rec.tokenizer_sha256 == $m.tokenizer_sha256
              and ($d | length) > 0 and ($lg | length) == ($d | length)
              and ($d | all((.model // "") | contains("model_sha256=\($m.model_sha256),") and contains("tokenizer_sha256=\($m.tokenizer_sha256))")))
              and ($lg | all(.shards == [v5_shards_line($m)]))) | v3_okv};

# OPR: positive evidence of no fallback, from the daemon logs (sc#337 line formats, ACCEPTANCE-v5 Section 3.4).
def v5_native_ops: ["rmsnorm", "apply_rope", "matmul_vec", "matmul_batch", "swiglu", "gqa_attention", "paged_attention",
                    "paged_attention_batch", "compute_logits", "rmsnorm_heads"];
def v5_strict_ok: test("^STRICT strict=true dev_fallback_build=false require_checkpoint=true backend=.*OmegaGb10");
def v5_op_ok:
  (capture("^OP_REPORT native=\\[(?<n>[^\\]]*)\\] reference=\\[(?<r>[^\\]]*)\\] native_fallbacks=\\[(?<f>[^\\]]*)\\] reference_runs=\\[(?<rr>[^\\]]*)\\] backend=(?<b>.*)$") // null) as $c
  | if $c == null then false
    else ($c.n | split(",")) as $n
         | (v5_native_ops | all(. as $o | $n | index($o) != null)) and $c.f == "" and $c.rr == "" and ($c.b | test("OmegaGb10")) end;
def v5_row_opr($id):
  . as $ev | ($ev.daemon // []) as $d | ($ev.logs // []) as $lg | ([$lg[] | (.op_reports // [])[]]) as $ops
  | ($ev.task.kind != "negative-boundary") as $calls
  | {row: "\($id)-OPR", criterion: "No fallback, positive evidence",
     threshold: "one log per daemon start in run.json; each has exactly one STRICT line with strict=true dev_fallback_build=false require_checkpoint=true and a backend containing OmegaGb10, and no STRICT_REAL_MODEL_VIOLATION line; every OP_REPORT line lists the ten native ops, native_fallbacks=[] and reference_runs=[], backend containing OmegaGb10; a launch that calls the model has at least one OP_REPORT line",
     value: {daemon_starts: ($d | length), logs: ($lg | map({name, strict, violations, op_reports: (.op_reports | length)})),
             bad_op_reports: ($ops | map(select(v5_op_ok | not))), calls_model: $calls},
     result: (($d | length) > 0 and ($lg | length) == ($d | length)
              and ($lg | all((.strict | length) == 1 and (.strict[0] | v5_strict_ok) and .violations == 0))
              and ($ops | all(v5_op_ok)) and (($calls | not) or ($ops | length) > 0)) | v3_okv};

# PIN: the build pins the wrapper recorded equal the frozen ones, and the frozen ones are filled.
def v5_row_pin($id):
  . as $ev | ($ev.frozen.pins // {}) as $f | ($ev.launch.pins // null) as $p | ($ev.frozen.placeholder // "TO FILL AT FREEZE") as $ph
  | ($f | keys) as $k
  | {row: "\($id)-PIN", criterion: "Build pins",
     threshold: "the launch record has pins; every frozen pin (sovereign-core, omega, physics and aienos lock commits, Cargo.lock, aien, np1_reference, np1_edit_merge sha256) is filled (40 or 64 lowercase hex, not the placeholder) and equals the recorded one",
     value: {frozen: $f, recorded: $p, differing: (if $p == null then null else ($k | map(select($f[.] != $p[.]))) end)},
     result: ($p != null and ($k | length) == 8
              and ($k | all(. as $x | ($f[$x] | type) == "string" and $f[$x] != $ph and ($f[$x] | test("^([0-9a-f]{40}|[0-9a-f]{64})$"))
                                       and $p[$x] == $f[$x]))) | v3_okv};

# MEAS: measurements present and consistent.
def v5_row_meas($id):
  . as $ev | ($ev | v5_attempts) as $at | ($ev.launch // {}) as $l | ($ev.daemon // []) as $d
  | ($at | map(.ms) | if all(v5_int) then add // 0 else null end) as $sum
  | {row: "\($id)-MEAS", criterion: "Measurements present and consistent",
     threshold: "the launch record has wall_ms and meminfo_before mem_free_kb and cached_kb; every daemon start has vmhwm_kb; every attempt has ms, tokens and token_ids; all are non-negative integers; tokens == length of token_ids <= the task max_tokens; the attempts ms sum <= wall_ms",
     value: {wall_ms: ($l.wall_ms // null), meminfo_before: ($l.meminfo_before // null), vmhwm_kb: ($d | map(.vmhwm_kb)),
             attempts: ($at | map({attempt, ms, tokens, token_ids: (.token_ids | if type == "array" then length else . end)})),
             attempts_ms_sum: $sum, max_tokens: $ev.task.max_tokens},
     result: (($l.wall_ms | v5_int) and ($l.meminfo_before.mem_free_kb | v5_int) and ($l.meminfo_before.cached_kb | v5_int)
              and ($d | length) > 0 and ($d | all(.vmhwm_kb | v5_int))
              and ($at | all((.ms | v5_int) and (.tokens | v5_int) and (.token_ids | type) == "array" and (.token_ids | all(v5_int))
                             and .tokens == (.token_ids | length) and .tokens <= $ev.task.max_tokens))
              and $sum != null and $sum <= $l.wall_ms) | v3_okv};

# ---- N1 (ACCEPTANCE-v5 Section 3.3): the refusal before the model --------------------------------------------
# The verbatim error recorded at gate G3 (evidence-v5/g3-n1-stub-run.txt), in steps/S3.json "error".
def v5_n1_refusal: "RunComposeTask: destination ../shared-stuff/reminder.txt cannot be written safely (not a plain relative path inside the workspace); refusing";
def v5_step($s): (.run_steps // []) | map(select(.step == $s)) | first;
def v5_row_n1_nr($id):
  . as $ev | ($ev | v5_step("S3")) as $s3
  | {row: "\($id)-NR", criterion: "Refused for the destination",
     threshold: "the propose step failed (S3 rc recorded as a number other than 0, steps/S3.json ok false) and its error equals the G3 string exactly: \(v5_n1_refusal | tojson)",
     value: {s3_rc: ($s3.rc // null), s3_ok: ($ev.s3_step.ok // null), error: ($ev.s3_step.error // null)},
     result: ($s3 != null and ($s3.rc | type) == "number" and $s3.rc != 0 and $ev.s3_step != null and $ev.s3_step.ok == false and $ev.s3_step.error == v5_n1_refusal) | v3_okv};
def v5_row_n1_nm($id):
  . as $ev | ($ev.host // []) | map(select((.text | v5_json | .generation? // null) != null)) as $gens
  | {row: "\($id)-NM", criterion: "No model call, nothing committed",
     threshold: "no proposal attempt, no generation record named or present in the S8 recall (which must be present), no authorization record, no S5 path, and no file at the destination relative to the workspace",
     value: {s8_present: ($ev.recall_s8 != null), attempts: ($ev | v5_attempts | length), generation_record: ($ev.report.generation_record // null), generation_records_in_recall: ($gens | map(.id)),
             authorizations: ($ev.auths | length), s5_path: ($ev.s5.path // null), dest_exists: $ev.dest_exists},
     result: ($ev.recall_s8 != null and ($ev | v5_attempts | length) == 0 and ($ev.report.generation_record // null) == null and ($gens | length) == 0
              and ($ev.auths | length) == 0 and ($ev.s5.path // null) == null and $ev.dest_exists == false) | v3_okv};
# Replaces v8 N1-C (it reads the S3 report, which a refusal before the model does not write).
def v5_row_n1_rc($id):
  . as $ev | [$ev | v5_step("S3"), v5_step("S4"), v5_step("S5")] as $st
  | {row: "\($id)-RC", criterion: "Completion state REFUSED, read from the steps",
     threshold: "S3, S4 and S5 are recorded and each has a recorded exit code other than 0; no authorization record; no S5 path; the workspace is unchanged (run.json containment workspace_changed empty)",
     value: {steps: ($st | map(if . == null then null else {step, rc, ok} end)), authorizations: ($ev.auths | length),
             s5_path: ($ev.s5.path // null), workspace_changed: ($ev.containment.workspace_changed // null)},
     result: (($st | all(. != null and (.rc | type) == "number" and .rc != 0)) and ($ev.auths | length) == 0 and ($ev.s5.path // null) == null
              and $ev.containment.workspace_changed == []) | v3_okv};
# Replaces v8 N1-H (its machine id is read from the S3 report).
def v5_row_n1_rh($id):
  . as $ev | ($ev.constraint_text // null) as $ct
  | def has_ct: (.ok == true) and ((.constraints // []) | any(.text == $ct and .verified == true));
    {row: "\($id)-RH", criterion: "Daemon healthy after the refusal",
     threshold: "the same daemon's recall after S3 (pre-restart) and S8 after the restart both return the S1 constraint byte-identical and verified; machine id equal in both",
     value: {constraint_text: $ct, pre_machine_id: ($ev.recall_pre.machine_id // null), s8_machine_id: ($ev.recall_s8.machine_id // null)},
     result: ($ct != null and $ev.recall_pre != null and $ev.recall_s8 != null and ($ev.recall_pre | has_ct) and ($ev.recall_s8 | has_ct)
              and ($ev.recall_pre.machine_id | type) == "string" and $ev.recall_pre.machine_id == $ev.recall_s8.machine_id) | v3_okv};

# ---- the rows of one launch -----------------------------------------------------------------------------------
def v5_product_rows($id; $committed):
  [v5_row_pre($id), v5_row_dec($id)] + (if $committed then [v5_row_gr($id)] else [] end) + [v5_row_opr($id), v5_row_pin($id), v5_row_meas($id)];
def v5_rows:
  . as $ev | ($ev.task.id) as $id | ($ev.task.kind) as $k
  | (($ev.task.requirements // []) | length) as $nreq
  | ([range(0; $nreq) as $i | v5_row_req($id; $i)]) as $rq
  | if $k == "long" then
      [v3_row_sb($id), v3_row_f($id), v4_row_cut($id)] + $rq
      + (if ($ev.task.source // null) != null then [v4_row_src($id)] else [] end)
      + [v3_row_d($id; true), v4_row_be($id)] + v5_product_rows($id; true)
    elif $k == "edit" then
      [v3_row_sb($id), v4_row_cut($id), v4_row_eo($id)]
      + (if (v5_end_of_section_ids | index($id)) != null then [v5_row_ep($id)] else [] end)
      + [v3_row_d($id; true), v4_row_be($id)] + v5_product_rows($id; true)
    elif $k == "identity" then [v3_row_sb($id), v4_row_cut($id)] + $rq + [v3_row_d($id; true), v4_row_be($id)] + v5_product_rows($id; true)
    elif $k == "negative-boundary" then
      [v4_row_nc($id), v4_row_be($id), v5_row_n1_nr($id), v5_row_n1_nm($id), v5_row_n1_rc($id), v5_row_n1_rh($id),
       v5_row_opr($id), v5_row_pin($id), v5_row_meas($id)]
    elif $k == "negative-budget" then [v3_row_d($id; false), v4_row_nc($id), v3_row_n2r, v4_row_be($id)] + v5_product_rows($id; false)
    else [{row: "?", criterion: "unknown launch kind", threshold: "-", value: $ev.task, result: "FAIL"}] end;
