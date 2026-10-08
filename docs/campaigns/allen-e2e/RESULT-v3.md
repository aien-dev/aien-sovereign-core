# ALLEN end-to-end demo v3: result

Spec: DEMO-v3.md (committed at 4a95c80 and 7f53bdd before any v3 run, not edited) = DEMO-v2.md unchanged.
Driver: scripts/allen_e2e_demo.sh with DEMO_VERSION=v3.
Receipts: receipts-v3.jsonl (one line per step, 10 lines). Raw outputs: artifacts-v3/.
Label: real-CPU (daemon and CLI on the Spark host, Linux, model on CPU, desk MAC on). real-GB10 and native-AIENOS: NOT_RUN. mock: not used.
Stack: main at 3eaa060fab33 plus demo-only files (driver label, every receipt). Driver commit c77801d71f69, daemon/CLI binary sha256 ae968900e253... (in every receipt).
Main 3eaa060 includes the first-line heading requirement (sc#319) and the provenance link (sc#318).
It does NOT include the double-effect fix (sc#321, issue sc#320: a stop/resume after a DONE effect could mint a second grant).
No step of this demo stops or resumes after a DONE effect, so that defect is not exercised here, and this run is no evidence for or against it.
Run: 2026-10-08, 07:25:33 to 07:32:38 UTC.

v1 (FAIL, RESULT-v1.md) and v2 (FAIL, RESULT-v2.md) stay recorded as they were. This run does not change them.

## Verdict: PASS (10 of 10 steps, real-CPU on the Linux host)

The request text is the v2 text, including "Start with a Markdown heading line that begins with "# ". Use at most 200 words."
The runtime now recognizes both rules (requirements_recognized ["at most 200 words", "a level-1 markdown heading ("# ") as the
first line"], requirements_uncertain []) and checks them on the complete document before the commit. Both models met both rules
on the first attempt.

## Step table

| Step | Result | Evidence |
|------|--------|----------|
| S0 | PASS | Daemon up on M-A (CPU-reference), log line "Authorize MAC: on", allen status not_engaged, no ALLEN state before S1. |
| S1 | PASS | One identity, ID0 = bf380858665315f11cf73d503c871ac614817668cd98ade2858e6589f2fafb2f (daemon log and allen status fingerprint agree). AgentRoot 198abaed84d7... |
| S2 | PASS | Profile revision 1 with plain-language on, under ID0. Memory inspect shows exactly N-work, N-pers, G1. Goals list shows exactly G1. |
| S3 | PASS | Commit DONE on M-A. MemoryReport context work, items_included 2. Document checker C1-C5 hold. 64 words. garden.md sha256 aa0020df45f98d40ac1c62e34dd9569d9397139c868461dab7cc6e0c083e500a (artifacts-v3/garden.md, first line "# Garden Plan"). One proposal attempt. |
| S3-red | PASS | Checker on the hand-written document reports exactly C3 C5. The good control reports none. |
| S4 | PASS | Ledger links grant id, proposal sha256, content sha256 (equal to garden.md on disk), the M-A model digest and ID0. |
| S5 | PASS | kill -9 (exit 137), restart on M-A. ID0, profile, notes and G1 identical. S3 intent still DONE, ledger and record count unchanged (not re-executed). garden.md same sha256, inode and mtime. Replay reconcile: 0 claims. |
| S6 | PASS | Forgot N-pers. Inspect shows no text for it (row state forgotten). Recall in personal returns 0 items. 0 files under daemon state still hold the canary. |
| S7 | PASS | kill -9 (exit 137), start on M-B. Model: line and model digest changed (f55217be716b to 75311d91bb08). Backend line recorded (CPU-reference, GPU engine not linked). ID0, profile, N-work, G1 unchanged. N-pers still absent. |
| S8 | PASS | Commit DONE on M-B. MemoryReport context work, items_included 2. C1-C5 hold. 71 words. garden-b.md sha256 2814665ee3fe5833c37f03b2ca5abf44bf5024b823bbde071d100f5d293fcae0. Provenance links all present, including the M-B digest and ID0. One proposal attempt. |

## Limits

- real-CPU on the Linux host only. Nothing here is a GB10 GPU result or a native AIENOS result.
- The models are the small fixed demo checkpoints (M-A, M-B) on the CPU reference backend; this says nothing about model quality beyond C1-C5.
- The stack is main at 3eaa060, without sc#321. A rerun after sc#321 merges is not part of v3 and is not claimed here.
- One run. No repeat runs were made or are claimed.
