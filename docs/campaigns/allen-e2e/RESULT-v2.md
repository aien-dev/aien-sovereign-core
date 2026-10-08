# ALLEN end-to-end demo v2: result

Spec: DEMO-v2.md (committed at 43de95b before any v2 run, not edited) on top of DEMO-v1.md.
Driver: scripts/allen_e2e_demo.sh with DEMO_VERSION=v2.
Receipts: receipts-v2.jsonl (one line per step, 10 lines). Raw outputs: artifacts-v2/.
Label: real-CPU (daemon and CLI on the Spark host, model on CPU, desk MAC on). real-GB10 and native-AIENOS: NOT_RUN. mock: not used.
Stack: main at f5ba66bf3a1b (includes the provenance link, sc#318) plus demo-only files. Every receipt carries the full commit and the binary sha256.
Run: 2026-10-08, 06:40 to 06:43 UTC.

v1 stays recorded as FAIL (RESULT-v1.md). This run does not change that.

## Verdict: FAIL

S3 and S8 failed, so S4 and S5 had no commit to check. S0, S1, S2, S3-red, S6 and S7 passed.

The cause is one predeclared v2 change meeting a product rule. v2 added the format rules to the request:
"Start with a Markdown heading line that begins with "# ". Use at most 200 words." The runtime reads measurable
requirements out of every request before it lets a model write anything, and it refuses a task when it finds a
requirement it cannot check reliably (requirements_extract.rs: "uncertainty is never treated as satisfied").
It recognized "at most 200 words". It did not recognize "a Markdown heading line that begins with "# "", so it
proposed nothing, on both models, and said exactly why. No file was written and no grant was spent.

This is the designed refusal, not a crash and not a silent skip. It is still a FAIL of the demo as declared.

## Step table

| Step | Result | Evidence |
|------|--------|----------|
| S0 | PASS | Daemon up on M-A (CPU-reference), log line "Authorize MAC: on", allen status not_engaged, no ALLEN state before S1. |
| S1 | PASS | One identity, ID0 = 594c6a53670d2a3c208a715649359cf99499297bf9e4246c5a7201f41abd4b43 (daemon log and allen status fingerprint 594c6a53 agree). |
| S2 | PASS | Profile revision 1 with plain-language on, under ID0. Memory inspect shows exactly N-work, N-pers, G1. Goals list shows exactly G1. |
| S3 | FAIL | Not committed. requirements_recognized ["at most 200 words"], requirements_uncertain ["a Markdown heading line that begins with "# ""]. MemoryReport context work, items_included 2 (the memory did reach the task). One attempt, no retry. |
| S3-red | PASS | Checker on the hand-written document reports exactly C3 C5. The good control reports none. |
| S4 | NOT_RUN | S3 produced no commit, so there was nothing to trace. |
| S5 | NOT_RUN | No S3 commit. The restart itself kept ID0, profile, notes and G1 (artifacts S5a/S5b). |
| S6 | PASS | Forgot N-pers. Inspect shows no text for it (row state forgotten). Recall in personal returns 0 items. 0 files under daemon state still hold the canary. |
| S7 | PASS | kill -9 (exit 137), start on M-B. Model: line and model digest changed (f55217be716b to 75311d91bb08). Backend line recorded. ID0, profile, N-work, G1 unchanged. N-pers still absent. |
| S8 | FAIL | Same refusal as S3 on M-B: "at most 200 words" recognized, the heading rule uncertain, nothing proposed. |

## What v2 settled and what it did not

- Settled: S7 as v2 worded it passes (the Model: line changes on a model swap; v1's backend-line wording was the problem).
- Settled again on main: identity, personalization, kill -9 restart, forget and model swap keep one ALLEN identity and its state.
- Not tested: the provenance link on a real demo commit (S4, S8). sc#318's own live test covers it on SmolLM2
  (generation record 16, model_sha256 f55217be...), but that is not this demo.
- Not tested: whether the models follow a stated heading rule, because the request never reached them.

## Recommendation (not decided here)

The heading rule is a reasonable thing for a person to ask for, and the runtime cannot check it yet. Two honest ways on:

1. Teach the requirement reader "start with a Markdown heading" (a first-line-heading requirement the runtime checks
   after the write), then predeclare v3 as v2 unchanged and run it. Upside: a real product gain and the same request.
   Downside: one product change, review and merge first.
2. Predeclare v3 with request wording the runtime already understands. Upside: fast. Downside: it is a prompt change
   made after seeing a result, which is the kind of retry v1 and v2 both rule out.

Option 1 is the recommended way. No v3 has been written or run.
