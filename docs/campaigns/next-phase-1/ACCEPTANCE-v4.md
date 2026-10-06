# NEXT-PHASE-1 campaign v4: acceptance criteria (frozen before any v4 code or run)

```text
campaign_id  = "next-phase-1"
spec_version = 4
status       = FROZEN at the commit that adds this file. The v4 code (assistant
               prefix, declared warm-up, recomputed budget, reply logging) lands
               in LATER commits, so this file's hash predates it.
base         = next-phase/compose-v3 69f7603 (main 91751b6 + v3, PR #221 open);
               rebased onto main once #221 merges.
```

v1, v2 and v3 stand as recorded; their receipts are never edited. v3 verdict FAIL: attempt 1
echoed the prompt, attempt 2 answered in prose; the control proved the request already used the
model's chat template, so the echo is not a prompt-format bug.

## 1. Task, steps, receipts, pass criteria: unchanged

ACCEPTANCE.md Sections 1, 2 and 4, the ACCEPTANCE-v2 Section 2 table (every threshold and the
rescue definition) and ACCEPTANCE-v3 Section 3d (S8 scored on its own) apply unchanged. One
driver launch, one goal, one task, two daemon starts, zero hand input.

## 2. Declared production-path changes (v4)

**(1) Assistant-response prefix.** The `RunComposeTask` "model" Skill starts the assistant turn
with the literal text `filename: ` (9 ASCII characters plus one space). It is part of the
production proposal template (aien-runtime), the same for every goal and every attempt. The
model generates the path and the content after it.
- Template source: TinyLlama-1.1B-Chat-v1.0 `tokenizer_config.json`, sha256
  `7b41ba7d0eb91e77914ca3dafde559ea3e19878769b7e68409e89bed5222e77a`, field `chat_template`;
  with the generation prompt it ends the text with `<|assistant|>` and a newline
  (ACCEPTANCE-v3 Section 2, PROVEN BY TEST in v3).
- Exact bytes sent for attempt 1 (prompt P = `proposal_prompt(goal, workspace, entries)`,
  unchanged since v2):
  `<|user|>\n` + P + `</s>\n<|assistant|>\nfilename: `
  The prefix follows the assistant marker and its newline directly; nothing comes after the
  final space. Encoded with special tokens: BOS first, one EOS (end of the user turn), and the
  tail `... 29989 29958 13 9507 29901 29871` = `|>`, newline, `filename`, `:`, `▁` (MEASURED
  with the real tokenizer: 138 tokens for the v4 workspace path; retry prompt 176).
- Declared risk (INFERRED): the prompt ends on a lone space token (29871), an unusual boundary
  for this tokenizer; the model may continue without a leading space. This is the literal
  prefix as briefed; it is not tuned.
- Parser: unchanged (`check_file_proposal`). The only change is that the Skill reads the reply
  as `filename: ` + generated text, so the parser sees the prefix as the first line's start.
  `text` and `text_sha256` in `proposal_attempts[]` cover that full reply; `tokens` counts the
  generated tokens only.
- If the prefix lands a path but the content is empty or cut at 48 tokens, that is recorded as
  the parser or AEGIS outcome and scored by the unchanged rows (a FAIL row). `max_tokens` is not
  raised during the campaign.

**(2) Declared warm-up at daemon start.** After the model loads and before the daemon serves
any request, it runs one turn of 1 generated token on a fixed warm-up prompt longer than one
prefill chunk (more than 128 tokens, so both the full-chunk and the remainder prefill paths
run), and discards the token. Purpose: pay the measured first-call penalty (5.2 s in v3, inside
the first 128-token prefill step, cause UNKNOWN) before the task window. The daemon prints
`Warm-up: 1 token in <ms> ms over <n> prompt tokens (discarded)`. The driver waits for that line
before it counts a daemon as started, so it falls inside S0 (daemon 1) and S7 (restart, report
only). The receipt records `warm_up_ms` for both daemon starts. Whether the penalty is per
process (paid by any first call) or per prompt shape is UNKNOWN; attempt 1's measured time
shows which.

**(3) Per-attempt budget recomputed from the v3 measurement.** Unchanged inputs (v3 Section 2,
MEASURED): first 128-token prefill chunk 2 839 ms warm; remainder chunk 136 ms + 16.9 ms per
token (INFERRED line through the 7- and 15-token chunks); decode 166.6 ms per token; 60 ms wall
overhead. `max_tokens` = 48 counts generated tokens only, so the 3 prefix tokens leave decode
(47 steps after the first token) unchanged and move into prefill.
- Retry attempt (176 prompt tokens = 128 + 48): 2 839 + (136 + 48 x 16.9 = 947) + 47 x 166.6
  = 7 830 + 60 = **11 676 ms**; per-attempt budget A = **12 000 ms** (rounded up, unchanged
  value; the prefix adds 3 x 16.9 = 51 ms to v3's 11 625 ms).
- Attempt 1 after warm-up (138 = 128 + 10): 2 839 + (136 + 10 x 16.9 = 305) + 7 830 + 60
  = 11 034 ms.
- Skill budget B = 29 000 ms (30 000 ms quiescence wait at omega 62b6a28
  `src/runtime/rx_compose.c:1192`, not raised, less 1 000 ms). Rule unchanged from v3: attempt
  1 always starts; attempt k > 1 starts only if at least A is left; limit = what is left.
- Expected: attempt 1 about 11.0 s leaves 18.0 s, attempt 2 about 11.7 s leaves 6.3 s < 12 s,
  so no attempt 3 unless an attempt ends early at end of sequence. Retry cap stays 3.

**(4) Reply bytes logged.** Every attempt's full reply (prefix + generated text, UTF-8 as
decoded) is written next to the receipts as `replies/<sha256>.txt`, named by its own sha256,
which equals `text_sha256` in the receipt. Never edited afterwards.

## 3. Summary on FAIL

If v4 fails, the receipt summary gets a section "Conclusion on the frozen model input" stating
what the mechanics proved (v1 attempt 4) and what the TinyLlama input cannot do under free
generation, for the handoff to cite.

## 4. Binding and run conditions

As ACCEPTANCE-v3 Section 4: omega.lock 62b6a28 for the GPU engine and librx_compose; GB10 hold
through `quietlock hold --minutes <= 20` with start and release whispers; one driver launch;
the verdict is whatever the rows give; performance is an observation (tokens/s, warm_up_ms).
