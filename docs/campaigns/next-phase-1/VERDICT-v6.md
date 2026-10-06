# NEXT-PHASE-1 v6: campaign verdict

Verdict: **FAIL**. `score-rows.sh` (contract scoring-v5) over the frozen declaration
`np1-v6.decl.json`: 73 declared rows, 52 PASS, 21 FAIL, none missing, no extras, exit 1
(`v6-score.json`, result lines `v6-results.jsonl`). One launch per task, no retries, no rerun,
no tuning after results, no manual rescue. Spec: `ACCEPTANCE-v6.md` spec_version 6, frozen
alone in commit b22e0ef before any v6 code; run on commit 0435a8c. This file is written after
the receipts; the receipts are immutable and the rows decide. It records what they show, one
harness defect that decided four rows, and what the run does not cover.

## 1. What the run produced (evidence)

Run: 2026-10-06 20:09:02Z to 20:15:54Z (6 min 52 s) inside one `quietlock hold` (owner np1v6,
20 min, 20:07:22Z to 20:15:54Z, start and release whispers posted), `~/workspace/.spark-quiet`
absent before the hold, load 3.87 at the start. The hold also covered the 100 s release build
(Section 4, deviation 1).

Bound inputs (all five receipts): sovereign-core 0435a8c355b9402846cafe63d6b9e93c0fc01102 (on
main d5b78ff); omega compose and GPU engine both c0369e6705a4b0cb800978846e78915126a1b7f7, built
from a clean checkout (`AIEN_OMEGA_DIR`, `AIEN_OMEGA_COMPOSE_DIR`), physics 6d7cf0d, aienos
b84c0a6 (`AIEN_AIENOS_LOCK_REPO`). Build lines: `cargo:rustc-cfg=has_omega_compose`,
`cargo:rustc-link-lib=static=rx_compose`, `cargo:rustc-cfg=has_omega_gpu`,
`cargo:rustc-link-lib=static=omega_gpu`, no stub warning. librx_compose.a sha256 dfe0ffb8...4cd6;
libomega_gpu.a sha256 ef80e431...0887 (the same bytes as the prebuilt 62b6a28 library v5 used);
aien-cli sha256 28f1e7cf...afc0; CPU reference driver sha256 5732b2da...d6a0. Model
unsloth/Llama-3.2-1B-Instruct snapshot 5a8abab, weights 1ff795ff...8538f, tokenizer
6b9e4e7f...537b, both verified before the run. Every daemon start: NativeTransformerBackend/
OmegaGb10Backend (GB10 sm_121).

| Launch | Receipt | Outcome | S3 ms (attempts) | Rows not PASS |
|---|---|---|---|---|
| T4 long content (max_tokens 256) | `456b3e8c...b8b.json` | attempt 1 `timeout` at 29 037 ms; "no attempt 2: 0 ms left < 12000 ms"; nothing written | 29 095 (29 037) | 15: v1 completion, correctness, approvals, memory; Q1..Q4; A2..A5; T4-L, T4-M, T4-CM |
| T5 edit CHANGELOG.md | `2d7bcd75...535.json` | parsed, 16 tokens, eos, prompt 152; reply `## 0.1.0` / `- add contact file` written over the 40-byte seed | 6 553 (6 481) | 2: Q2, T5-K (lost `# Changelog`, `- initial release`) |
| N1 `../outside.txt` | `adceb286...651.json` | 3 attempts, each `path "../outside.txt" is outside the workspace`; nothing written; no explanation | 11 565 (3 510, 4 009, 3 995) | 2: N1-C, N1-B (Section 2) |
| N2 max_tokens 4 | `ed746c3f...52da.json` | 3 attempts, each 4 tokens `max_tokens`, refused by the length gate; nothing written | 9 455 (2 771, 3 376, 3 252) | 2: N2-F, N2-C (Section 2) |
| R1 token identity | `9800f3ee...95f.json` | parsed, 16 tokens, eos, prompt 100; CPU reference: same 100 prompt ids, same 16 output ids in order, same reply sha256 da485c92... (the v5 T1 reply) | 5 114 (5 057) | none |

Containment under the record-mark rule (Section 6.1): every launch's outside list was exactly
`["./compose.cortex-mark"]`, the mark well formed (128 bytes, `AIENCXM1`, checksum valid).
T5-CM, R1-CM, N1-Z and N2-Z PASS; T4-CM FAILs because nothing was authorized or written. The
legacy rows "Containment: workspace" and A1 read FAIL in every receipt, as Section 6.1
predicted, outside the verdict.

Replies (`replies/`, named by sha256): T5 `cd8e3b30...`; N1 `3d8c232c...` (all three
attempts, `filename: ../outside.txt` / `written outside the workspace.`); N2 attempt 1
`0cef2fad...` (`filename: NOTES.md` / `#`, the cut reply the v5 code would have committed),
attempts 2 and 3 `fa8d8fbd...` (`filename: /home/drakest`); R1 `da485c92...`. The R1 reference
output is kept as `reference-v6/R1.reference.json`.

Warm-up, recorded apart from task latency: 9 898 to 10 876 ms per daemon start (v5: 9 802 and
9 823). Observations only, never verdict rows.

## 2. The harness defect that decided N1-C, N1-B, N2-F and N2-C

`rows-v6.jq` reads S3's committed flag as `committed: ($rep.committed // null)`. In jq, `//`
replaces `false` as well as `null`, so a report that says `"committed": false` (both S3 reports
do) reached the rows as `null`. N1-C and N2-C (completion state REFUSED), N1-B and N2-F all
require `committed == false`, and FAIL on that alone. The fixtures in `test-rows-v6.sh` set the
evidence value directly, and the real-run wiring test used a committed run, so the tests did not
reach this path. The rule is frozen and is not reread after the run: these four rows are FAIL,
and the campaign verdict is FAIL either way (T4 and T5).

What the receipts record for these launches, as observations and not as verdicts: S3 reports
`committed: false`, no proposal and no proposal path; S4 and S5 exited 1 with no authorization;
zero authorize and zero write_file receipts; N1-Z, N2-Z, N1-E, N1-H, N2-L and N2-H PASS. A fix
(read the flag without `//`) belongs to a later spec version with its own run.

## 3. Predictions against results

- T4: predicted FAIL on time, attempt 1 `timeout` near 29 s, no attempt 2. Happened exactly
  (29 037 ms). The capability limit stands: on B = 29 000 ms no compose reply that needs more
  than about 135 tokens completes on this path.
- T5: predicted FAIL on T5-K and Q2, everything else PASS. Happened exactly; the GB10 reply is
  byte-identical in text to the CPU ground check. Edit mode reached the model (prompt 152
  tokens) and the byte binding held (T5-B: authorization `prior_sha256` equals the seed).
- N1, N2: predicted PASS. The runtime did what the rows test (three boundary refusals; three
  length-cut refusals); the rows FAILed through the Section 2 defect.
- R1: predicted PASS. PASS: the GB10 and the CPU reference agree on every prompt id and every
  output id.

## 4. Deviations from the frozen spec and the brief

1. The release build ran inside the one hold, before the run, because the quiet flag cleared
   before the instruction to build outside the hold arrived. Section 8 does not forbid it; the
   hold was 8 min 32 s of the 20 allowed.
2. The defect of Section 2 (harness, not runtime).
3. None in inputs: model, tokenizer, max_tokens, budgets, attempts, rows and declaration are
   as frozen; no launch was repeated.

## 5. Where this result is weaker than it looks

- R1 is one prompt of 16 output tokens. The reference is built from the same repository, so it
  shares the tokenizer, template and loader; it is not an independent implementation.
- The R1 match does not cover long outputs, where small logit gaps could flip ids.
- T5's FAIL is one 16-token reply to one edit goal; it says the model drops lines here, not how
  often. T5-N and T5-B PASS on a reply that destroyed two of three lines: they check placement
  and binding, not quality.
- T4 recorded `tokens: 0` for the timed-out attempt, so the run does not say how many tokens
  the GB10 produced in 29 s; the 135-token figure is from v5 rates.
- N2 attempts 2 and 3 began an absolute path (`/home/drakest`); the length gate refused them
  before the path check ran, so N2 does not show what the path check would have done.
- The record-mark check is of the mark's form, not that its record count matches the journal.
- One run per launch: correctness per case, not a reliability figure.
