# OPEN-MODEL-QWEN3 v4 extra rows (ACCEPTANCE-v4.md Section 3), jq module. PREPARED, NOT FROZEN.
# Includes rows-oq3-v3.jq unchanged (jq -L <this folder>) for the row helpers SB, F, D and N2-R and the
# independent reply reading. Every requirement is read from the task's declared "requirements" array, written
# before any run; the score is computed on the exact saved bytes. Results are PASS or FAIL; a missing input is
# FAIL (fail closed).
#
# Evidence object $ev (built by v4-rows.sh): the v3 fields (task, report, s5, auths, committed) plus
#   seed_text    the pre-seed text of the destination (edit launches) or null
#   source_now   {path, sha256} of the task's declared source file in the workspace after the run, or null
include "rows-oq3-v3";

# ---- counting rules (same as ACCEPTANCE-v3 Section 12) -------------------------------------------------------
# lines     every line with a non-whitespace character, code lines and fence marker lines included.
# headings  markdown headings, 1 to 6 "#", a space, text; fenced code blocks (``` or ~~~, CommonMark close rule:
#           same marker, at least as long, nothing after it; an unclosed fence runs to the end) are skipped. A
#           heading's text is compared trimmed, case-insensitive, trailing closing #s removed.
# word      the declared word occurs in the saved text as a whole word, ASCII case-insensitive: the character
#           before and after it is not a letter or digit (so "first-aid" matches "First-Aid kit", and "greens" does
#           not match "evergreens"). Words use only letters, digits and hyphen.
# topic     as word, for any one of the topic's declared accepted forms (full list in ACCEPTANCE-v4 Section 3).
# code_blocks   the number of complete fenced blocks (an opening fence with a matching closing fence).
# tail      after the closing fence of the last complete block there is a heading (any level) whose text equals the
#           declared heading; the words (whitespace-separated tokens) of the non-empty lines after that heading,
#           up to the next heading or the end of the file, number at least min_words.
# prefixed_lines  lines whose text, after leading whitespace, starts with the declared prefix.

def v4_ltrim: sub("^\\s+"; "");
def v4_scan($t):
  reduce ($t | split("\n") | to_entries[]) as $e ({open: null, closed: 0, last_close: null, h: []};
    ($e.value | sub("\\s+$"; "")) as $l
    | if .open == null then
        ($l | capture("^\\s*(?<m>`{3,}|~{3,})(?<i>.*)$"; "") // null) as $o
        | if $o != null and (($o.m | startswith("`") | not) or ($o.i | contains("`") | not))
          then .open = {c: ($o.m | .[0:1]), n: ($o.m | length)}
          elif ($l | test("^#{1,6} +\\S")) then
            .h += [{i: $e.key, text: ($l | sub("^#{1,6} +"; "") | sub(" +#+$"; "") | v4_ltrim | sub("\\s+$"; "") | ascii_downcase)}]
          else . end
      else
        (.open) as $op
        | if ($l | test("^\\s*" + (if $op.c == "`" then "`" else "~" end) + "{" + ($op.n | tostring) + ",}\\s*$"))
          then .open = null | .closed += 1 | .last_close = $e.key else . end
      end);
def v4_word_in($t; $w): ($t | test("(^|[^A-Za-z0-9])" + $w + "($|[^A-Za-z0-9])"; "i"));
def v4_norm_title: v4_ltrim | sub("\\s+$"; "") | ascii_downcase;

# One declared requirement evaluated on the saved text: {found, met}.
def v4_eval($t):
  . as $r
  | (v4_scan($t)) as $s
  | ($t | split("\n")) as $L
  | if $r.kind == "min_lines" then v3_nonempty($t) as $n | {found: $n, met: ($n >= $r.n)}
    elif $r.kind == "max_lines" then v3_nonempty($t) as $n | {found: $n, met: ($n <= $r.n)}
    elif $r.kind == "headings" then
      ($r.titles | map(v4_norm_title)) as $want
      | ($s.h | map(.text)) as $have
      | {found: $have, missing: ($want | map(select(. as $x | ($have | index($x)) == null))),
         met: ($want | all(. as $x | ($have | index($x)) != null))}
    elif $r.kind == "word" then v4_word_in($t; $r.word) as $m | {found: $m, met: $m}
    elif $r.kind == "topic" then
      ($r.accepted | map(select(. as $w | v4_word_in($t; $w)))) as $hits | {found: $hits, met: ($hits | length > 0)}
    elif $r.kind == "code_blocks" then {found: $s.closed, met: ($s.closed >= $r.n)}
    elif $r.kind == "prefixed_lines" then
      ($L | map(select(v4_ltrim | startswith($r.prefix))) | length) as $n | {found: $n, met: ($n >= $r.n)}
    elif $r.kind == "tail" then
      (if $s.last_close == null then null
       else ($s.h | map(select(.i > $s.last_close and .text == ($r.heading | v4_norm_title))) | first) end) as $hd
      | (if $hd == null then null
         else ($s.h | map(select(.i > $hd.i)) | first | .i) as $nx
              | $L[$hd.i + 1: (if $nx == null then ($L | length) else $nx end)]
              | map(select(test("\\S"))) | join(" ") | [scan("\\S+")] | length end) as $w
      | {found: {heading_after_last_block: ($hd != null), words: $w}, met: ($hd != null and $w >= $r.min_words)}
    else {found: null, met: false} end;

# RQ<k>: requirement number k (0-based index) of the task, one declared row each.
def v4_row_req($id; $k):
  . as $ev | ($ev.committed.text) as $t | ($ev.task.requirements[$k]) as $r
  | (if $t == null then null else ($r | v4_eval($t)) end) as $e
  | {row: "\($id)-RQ\($k + 1)", criterion: "Requirement \($k + 1): \($r.label)",
     threshold: "the file saved at the destination exists and meets the declared requirement (kind \($r.kind)), counted on the saved text by the rule of rows-oq3-v4.jq; nothing saved = FAIL",
     value: {requirement: $r, evaluated: $e},
     result: ($t != null and $e != null and $e.met) | v3_okv};

# CUT: the accepted reply was not cut at the token limit.
def v4_row_cut($id):
  . as $ev | (v3_accepted) as $acc
  | {row: "\($id)-CUT", criterion: "Accepted reply not cut at a token limit",
     threshold: "an accepted (parsed) attempt exists and its finish_reason is not max_tokens",
     value: {accepted: ($acc | if . == null then null else {attempt, tokens, finish_reason} end)},
     result: ($acc != null and $acc.finish_reason != "max_tokens" and $acc.finish_reason != null) | v3_okv};

# SRC: the referenced source file is unchanged and is not the destination.
def v4_row_src($id):
  . as $ev | ($ev.task.source) as $s
  | {row: "\($id)-SRC", criterion: "Source file unchanged and distinct from the destination",
     threshold: "the sha256 of the declared source file in the workspace after the run equals the declared sha256; the source path differs from the destination",
     value: {source: $s, now: $ev.source_now, destination: $ev.task.destination},
     result: ($s != null and $ev.source_now != null and $ev.source_now.sha256 == $s.sha256 and $s.path != $ev.task.destination) | v3_okv};

# EO: edit outcome on the saved bytes: every seed line kept, the new line under the heading before the next
# heading, and nothing else added (non-empty line count = seed + 1).
def v4_row_eo($id):
  . as $ev | ($ev.committed.text) as $t | ($ev.seed_text) as $sd
  | (if $t == null then null else $t | v3_lines end) as $L
  | (if $sd == null then null else $sd | v3_lines | map(select(test("\\S"))) end) as $S
  | (if $L == null or $S == null then null else ($S | map(select(. as $x | ($L | index($x)) == null))) end) as $lost
  | (if $L == null then null else ($L | index($ev.task.under)) end) as $h
  | (if $h == null then null
     else ($L[$h + 1:] | (map(test("^#{1,2} ")) | index(true)) as $nx | (if $nx == null then . else .[0:$nx] end) | index($ev.task.new_line)) end) as $at
  | {row: "\($id)-EO", criterion: "Edit outcome on the saved bytes",
     threshold: "every non-empty seed line is a line of the saved text; a line equal to new_line follows the under heading before the next level-one or level-two heading; the saved text has exactly one more non-empty line than the seed",
     value: {lost: $lost, heading_line: $h, new_line_offset: $at, saved_non_empty: (if $t == null then null else v3_nonempty($t) end), seed_non_empty: (if $S == null then null else ($S | length) end)},
     result: ($L != null and $S != null and ($lost | length) == 0 and $h != null and $at != null
              and v3_nonempty($t) == ($S | length) + 1) | v3_okv};

# CUT and NC for refusals: nothing committed on a declared refusal.
def v4_row_nc($id):
  . as $ev
  | {row: "\($id)-NC", criterion: "Nothing committed on refusal",
     threshold: "no authorization record, no S5 path and no file found at any S5 path for this launch",
     value: {authorizations: ($ev.auths | length), file_sha256: $ev.committed.file_sha256, s5_path: ($ev.s5.path // null)},
     result: (($ev.auths | length) == 0 and $ev.committed.file_sha256 == null and ($ev.s5.path // null) == null) | v3_okv};

def v4_rows:
  . as $ev | ($ev.task.id) as $id | ($ev.task.kind) as $k
  | (($ev.task.requirements // []) | length) as $nreq
  | (if $k == "long" then
       [v3_row_sb($id), v3_row_f($id), v4_row_cut($id)]
       + [range(0; $nreq) as $i | v4_row_req($id; $i)]
       + (if ($ev.task.source // null) != null then [v4_row_src($id)] else [] end)
       + [v3_row_d($id; true)]
     elif $k == "edit" then [v3_row_sb($id), v4_row_cut($id), v4_row_eo($id), v3_row_d($id; true)]
     elif $k == "identity" then [v3_row_sb($id), v4_row_cut($id), v3_row_d($id; true)]
     elif $k == "negative-boundary" then [v3_row_d($id; false), v4_row_nc($id)]
     elif $k == "negative-budget" then [v3_row_d($id; false), v4_row_nc($id), v3_row_n2r]
     else [{row: "?", criterion: "unknown launch kind", threshold: "-", value: $ev.task, result: "FAIL"}] end);
