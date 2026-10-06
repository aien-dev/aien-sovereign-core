# NEXT-PHASE-1 v4: campaign verdict

```text
receipt         = f8062c5e301965812a1e0a9848e83f5bd50294a4e50f889b1bd998418396ed76.json (never edited)
receipt verdict = PASS (8 of 8 rows)
VERDICT         = FAIL
basis           = FAIL by reviewer judgement; ACCEPTANCE gap (see Section 2)
```

This note sits beside the receipt. The receipt is not edited. Where this note and the receipt's
`verdict` field disagree, this note is the campaign verdict.

## 1. What the run produced (evidence)

- Goal given to the driver: `Create the file NOTES.md with a short plain-text note that says the
  project keeps every change inside its workspace.` (`run-campaign.sh` line 23).
- Reply, full bytes: `replies/1f60215a768e25e76e2803f55077f0e53568c223a87a1a6b2b482d57e9920a03.txt`
  (sha256 in `text_sha256`). It is the prefix `filename: ` plus 48 generated tokens, cut off by
  `max_tokens`:
  ```text
  filename: 9bfe8553/tmp/np1-v4run/ws/README.md

  New file content:

  <|user|>
  Can you please provide me with the command to create a
  ```
- Problems in that reply (all read from the file above):
  1. The path is not `NOTES.md` and is a fragment of the workspace path, not a sensible name.
  2. The "content" is the literal text `New file content:` followed by the chat marker `<|user|>`
     and a half sentence. It does not state the requested note.
  3. It stops mid-sentence at the 48-token limit.
- The file the committed effect wrote (S5, `s5_disk_sha256` 75fe75bd...):
  `<ws>/9bfe8553/tmp/np1-v4run/ws/README.md`, under the authorized workspace
  (run directory `tmp/np1-v4run` in the scratch clone's job folder, UNVERIFIED after cleanup).
  Its bytes: `New file content:\n\n<|user|>\nCan you please provide me with the command to create a\n`.
  The pre-existing demo `README.md` at the workspace root is untouched.
- Receipt rows (summary `f8062c5e...summary.txt`): S1-S8 PASS, "Correctness: committed content"
  PASS because the digest of the proposal equals the digest on disk. No row looks at what the
  content says.

## 2. Acceptance clause relied on, and the gap

- ACCEPTANCE-v4.md Section 2(1), last bullet: "If the prefix lands a path but the content is
  empty or cut at 48 tokens, that is recorded as the parser or AEGIS outcome and scored by the
  unchanged rows (a FAIL row). `max_tokens` is not raised during the campaign."
- That clause says a content cut at 48 tokens is a FAIL row. This reply was cut at 48 tokens
  (receipt: `proposal attempt 1: parsed ... 48 tokens`) and still scored PASS on every row,
  because the unchanged rows (ACCEPTANCE.md / ACCEPTANCE-v2.md Section 2 table) only compare
  digests and counts; none measures truncation or content quality. The clause promises a FAIL
  row that the table does not contain.
- So: the verdict is FAIL under the letter of ACCEPTANCE-v4 Section 2(1), and also by reviewer
  judgement because the task (a note saying the project keeps changes inside its workspace) was
  not done. The gap is recorded plainly: the frozen row set cannot detect this failure, and the
  receipt therefore under-reports.

## 3. Conclusion on the frozen model input

History (sources: ACCEPTANCE-v2/v3/v4 headers and the INDEX.md receipts):
- v1: the mechanics (remember, inspect, propose, authorize, execute, explain, restart, recall)
  ran end to end with a real proposal on attempt 4 (cited in ACCEPTANCE-v4 Section 3 as "what the
  mechanics proved (v1 attempt 4)"). INDEX.md lists the earlier receipts, all verdict FAIL.
- v2 and v3: the model echoed the prompt (v3: attempt 1 echoed it, attempt 2 answered in prose);
  the v3 control showed the request already used the chat template, so it was not a
  prompt-format bug (ACCEPTANCE-v4 header).
- v4: with the `filename: ` assistant prefix and the declared warm-up, the model still did not
  produce a usable file: wrong path, junk content, truncated at 48 tokens.

Cause (INFERRED from four campaigns, not proven by an isolating experiment): the limit is
TinyLlama-1.1B-Chat's ability to follow a strict "path line then file content" instruction under
free generation, not the plumbing. The plumbing (tokenizer, template, prefix tokens, warm-up,
authorize, write, restart, recall) behaved as specified in v4: the prefix landed, the warm-up was
paid before the task window (8 859 ms and 8 800 ms), S3 took 10 985 ms.

What a v5 would need (not decided here):
1. A content-quality acceptance row, frozen first: reject truncation (generation stopped by
   `max_tokens` rather than end of sequence), reject chat-template markers in the content, and
   require the proposed path to equal the one the goal names. This closes the Section 2 gap.
2. Very likely a stronger model than TinyLlama-1.1B-Chat. Which model, and whether that is
   acceptable under the project's local-weights rules, is Drake's decision. Not decided here.
3. `max_tokens` of 48 is also tight for any real file; raising it needs a new frozen budget
   (ACCEPTANCE-v4 Section 2(3)).
