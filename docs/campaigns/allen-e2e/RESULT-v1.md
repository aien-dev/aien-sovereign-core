# ALLEN end-to-end demo v1: result

Spec: DEMO-v1.md (committed at a4872a0 before any run, not edited). Driver: scripts/allen_e2e_demo.sh.
Receipts: receipts-v1.jsonl (one line per step, 10 lines). Raw outputs: artifacts-v1/.
Label: real-CPU (daemon and CLI on the Spark host, model on CPU, desk MAC on). real-GB10 and native-AIENOS: NOT_RUN. mock: not used.
Code that ran: repository commit ffbb0a47 (the receipts carry the full hash and the sha256 of the binary).

## Verdict: FAIL

The overall verdict is PASS only if S0-S8 and S3-red all pass. Three steps failed (S4, S7, S8). Six steps and the
negative control passed. The chain from request to approved write to restart to forget works on real models on the CPU.
What fails is the provenance link to the model and the identity, one literal wording in S7, and one model output.

## Step table

| Step | Result | Evidence |
|------|--------|----------|
| S0 | PASS | Daemon up on M-A, log line "Authorize MAC: on", allen status says not_engaged, no ALLEN state directory before S1. |
| S1 | PASS | One identity, ID0 = c3cf790b0660c9a98779b08a38933f6ba155badc0fff447e46378c5e6f2355e0 (daemon log and allen status fingerprint c3cf790b agree). |
| S2 | PASS | Profile revision 1 with plain-language on, under ID0. Memory inspect shows exactly N-work, N-pers, G1. Goals list shows exactly G1. |
| S3 | PASS | Commit DONE. MemoryReport context work, items_included 2. C1-C5 hold (120 words). garden.md sha256 6a8d8d03...b03494. |
| S3-red | PASS | Checker on the hand-written document reports exactly C3 C5. A good document reports none. |
| S4 | FAIL | Ledger links grant id, proposal sha256, and content sha256 (equal to the file on disk). It does not link the M-A model digest or ID0. |
| S5 | PASS | kill -9 (exit 137), restart on M-A. ID0, profile, notes, G1 identical. S3 intent still DONE, ledger unchanged, garden.md same sha256, inode and mtime. |
| S6 | PASS | Forgot N-pers. Inspect shows no text for it (a tombstone row, state forgotten). Recall in personal returns 0 items. |
| S7 | FAIL | Model digest changed (f55217be to 75311d91), ID0, profile, N-work, G1 unchanged, N-pers absent. But the Backend: log line is byte-identical for both models. |
| S8 | FAIL | Commit DONE, MemoryReport work, but garden-b.md fails C2 (no "# " heading on line 1). Also the M-B digest and ID0 are not in the ledger (same gap as S4). |

## Why S4, S7 and S8 failed

S4. The compose and effect ledger records for a commit hold the authorization id, proposal sha256 and content sha256.
They do not hold the model digest or the ALLEN LogicalAgentId. The digest is in the daemon boot log. ID0 is in the
ALLEN profile and memory stores. Neither is linked to the commit. This is a product gap, not a script error.

S7. The criterion says the backend line and the model digest changed. Both models run on the same CPU reference
backend, so the line "Backend: NativeTransformerBackend/CPU-reference ..." does not change. The "Model:" log line, the
model digest and the tokenizer digest did change. I applied the criterion as written and did not reinterpret it.
A v2 should say "the Model: line" if that is what is meant.

S8. Qwen3-4B wrote one paragraph of plain prose with no Markdown heading, so C2 fails. Recorded verbatim in
artifacts-v1/garden-b.md. No retry, per the spec. The S8 provenance clause also fails for the S4 reason.

## Models

- M-A SmolLM2-1.7B-Instruct: weights sha256 f55217be716b6a997b97b9d8d7eb6fad02e00858f5010ec24f64603c3a98a0e8 (single file, also the daemon's model_sha256).
- M-B Qwen3-4B-Instruct-2507: three shards. Weights digest eb97565b601744f836884f12d7803767fc82ffc94addb02904d5c2e13882fd6b
  (sha256 of the sorted per-shard sha256 listing). The daemon's own model_sha256 covers only the shard file it is pointed at:
  75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6.

## garden.md (S3, M-A)

sha256 6a8d8d031cf06bfbe56d64273a1c722de3cae6f47dfaeb9d99f65227db503494, text in artifacts-v1/garden.md.

## Limits and deviations

1. The ALLEN subject is a host-built fixture. Real subjects are created only by AIENOS cs_provision (native, NOT_RUN).
   S1 used the test-only encoder, bound to the demo's Cortex lineage, and adopted once with AIEN_ALLEN_ADOPT.
   The path after that (resolve, pin, profile, memory) is the real production path.
2. S1 needs the Cortex home to exist (its record 1 is the lineage the subject binds), so the S0 daemon is killed
   and restarted once with the subject. S0's "no ALLEN state" was checked before that restart.
3. allen status shows an 8 hex fingerprint. The full ID0 comes from the daemon log line "ALLEN: engaged agent=".
4. Daemon env: AIEN_REQUIRE_CHECKPOINT=1 (no silent fallback), AIEN_COMPOSE_EDIT_BUDGET_MS and
   AIEN_COMPOSE_DOC_BUDGET_MS at the 599000 maximum, default token cap. Same for both models.
5. Request wording used for both tasks: "Write garden.md in the workspace, a garden plan." (garden-b.md for S8).
6. S6 reads "inspect no longer shows it" as: the note text is gone. A tombstone row with state forgotten and no text remains.
7. Every daemon open adds 5 bookkeeping records of the same kind as record 1. My first run of the script compared
   the raw record total across the S5 restart and so flagged S5 FAIL. That check was my own addition, not in the spec.
   I changed it to compare effect-class records and the ledger, committed the change, and reran everything from clean
   state. Both runs gave identical documents (same sha256). Only the second run is in the receipts.
8. S0 receipts carry ID0 "none" because no identity exists yet.
9. The Qwen3 digest is per shard file at the daemon. The set digest above is computed by the driver.
10. Greedy decoding on CPU: one run each, deterministic. No GPU was used. The quiet flag was clear.
