# WHOLE-SYSTEM-E2E, RUN-2-GB10 (declared, GB10 native Omega backend): result

Rows: `ACCEPTANCE-v1.3-gb10.md` (committed before the harness change, sc#406) over `ACCEPTANCE-v1.2.md`, plus
`ACCEPTANCE-v1.3-ctrl-e5.md` (sc#404). Contract: `docs/plans/release-readiness/ACCEPTANCE_E2E.md` in aien-architecture,
frozen at b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`. Harness:
`scripts/whole_system_e2e.sh` v1.3 at 6729590 (merged main; sc#406 squash), `WSE2E_BACKEND=gb10`, started detached at
2026-10-11 03:05:54 UTC, rows 03:08:05 to 03:09:23 UTC, harness exit 0. Run folder:
`runs/RUN-2-GB10-20261011T030554Z/` (17 chain receipts linked by `prev_receipt_sha256`, `chain/SHA256SUMS`,
`VERDICT_ROWS.txt`, `GB10_SUMMARY.json`, artifacts, five daemon logs, the acceptance files as run, the zero-CUDA gate
output, the pins log, and the verifier's `VERIFIER_VERDICT.txt`, `VERIFIER_RECEIPT.json` and
`artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`). Host: the Spark (`spark-b87b`), NVIDIA GB10, driver 580.173.02, CAND-4
model (Llama 3.2 1B Instruct; weights and tokenizer digests equal `release/candidate.toml`). Milestone: R6 of
aien-architecture#190, owning issue sc#402. One declared attempt; nothing under the run folder was edited after the
run; `sha256sum -c chain/SHA256SUMS` from the committed tree reports 0 bad lines.

**Verdict: FAIL.** One judged row failed (E3m, document rule D4), so E4 and the two memory controls that depend on it
did not run. The GB10 backend rules held in every receipt: no fallback anywhere, every model call reached the chip.

## Rows as measured (OBSERVED from the chain and `GB10_SUMMARY.json`)

`gpu` is `gpu_executions` for the row (Omega `chip_calls` growth in that daemon; the first observation in a daemon
counts from its start, warm-up included). `fb` is `fallback_count`. A dash means the row made no model call.

| row | status | gpu | fb | what the receipt says |
|---|---|---|---|---|
| PRE | PASS | - | 0 | backend line `NativeTransformerBackend/OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)`; model digests equal the candidate record; quiet flag not held |
| CTRL-E3a | PASS | - | 0 | desk required and no key: the GB10 daemon exited 1 before serving; log names `NoDesk` |
| E1 | NOT_RUN | - | 0 | nothing installed; RUN-3 (R8) measures E1 |
| E2 | PASS | - | 0 | objective recorded through `aien-cli` before any effect receipt; id equals the harness id |
| CTRL-E2 | PASS | - | 0 | `EXTERNAL_IRREVERSIBLE` declared: exit 1, refusal `ForbiddenEffectClass`, no record |
| E3 | PASS | 82014 | 0 | state DONE; `outbox/report.md`; 3 inputs read; 146 words; names present; second execute `AlreadySpent`. The report is byte-identical to RUN-1's |
| E3m | **FAIL** | 36382 | 0 | state DONE; `report.md` written (79 words); check D4: required name `windmill` missing (the heading says `Windmill`, the body never names it; the model stopped at end-of-text after 101 tokens); second execute `AlreadySpent` |
| E4 | NOT_RUN | - | 0 | harness rule: E4 builds on E3m's report; E3m did not pass |
| E6 | PASS | 16066 | 0 | SIGKILL after authorize; execute while dead wrote nothing; restart; same grant DONE once; third execute `AlreadySpent` |
| CTRL-E3b | PASS | 26488 | 0 | grant revoked before spend: execute refused `Revoked`; `ctrl.md` absent |
| NET | PASS | - | 0 | 0 established TCP sockets of the daemon |
| CTRL-E4b | NOT_RUN | - | 0 | depends on E3m (harness rule) |
| CTRL-E6 | PASS | 16100 | 0 | SIGKILL after authorize, compose home removed: same grant refused `E_MARK_TRUNCATED`; no second effect |
| CTRL-E4 | NOT_RUN | - | 0 | depends on E3m (harness rule) |
| E5, CTRL-E1 | NOT_RUN | - | 0 | E5 is the second-machine verdict (R7); CTRL-E1 needs a release artifact (R8) |
| CTRL-E5 | PASS | - | 0 | first live run of the sc#404 tail: byte 3 of `artifacts/E3-report.md` changed on a scratch copy; verifier rejected it (`DIGEST_MISMATCH`), chain broken, verdict id moved 883caa9e to 7818d861; judged by the verifier |

GB10 rules (ACCEPTANCE-v1.3-gb10 G1 to G6), OBSERVED:

- G1: built from `scripts/pins.sh --gpu` (omega 6c6180c, physics 6d7cf0d, aienos b84c0a6, each equal to its lock);
  `scripts/zero-cuda-gate.sh` on the built `aien-cli`: `ZERO_CUDA PASS`, exit 0 (`artifacts/zero-cuda-gate.txt`).
- G2: all 4 serving daemon starts printed the native Omega backend line and
  `STRICT strict=true dev_fallback_build=false require_checkpoint=true`. The fifth start (CTRL-E3a, no desk key, GPU
  environment set) exited at the desk check before it printed a backend line, as the row requires.
- G3 and G4: `fallback_detected` false in every receipt and in `GB10_SUMMARY.json`; 0 fallback lines in every daemon
  log; every model attempt's `native_fallbacks` 0 and `reference_runs` 0; `OP_REPORT` lists every op as native.
- G5: every model-calling row (E3, E3m, E6, CTRL-E3b, CTRL-E6) has `gpu_executions` of at least 16066.
- G6: pins, driver line, zero-CUDA exit, `backend_mode gb10` and `host_load` in every receipt. Load average 1.0 to 1.3;
  `MemFree` about 17 GB while serving, `MemAvailable` about 96 GB. No GPU memory refusal (omega#327) occurred.

Model call time (the `ms` of each proposal attempt), RUN-1 on CPU vs this run on GB10: E3 114755 vs 8684, E3m 107999
vs 4096 (the GB10 text is shorter), E6 17801 vs 1317, CTRL-E3b 13303 vs 1608, CTRL-E6 12914 vs 1477.

Independent verifier (`tools/aien-verify`, sha256 `16d45e7edcb4a141dcf66c1787e724fd31a157168fd534d72787c7d88ebd56b5`,
built from this tree): chain unbroken, 0 chain problems, 0 status disagreements, `objective_id`
`093e66ec92a6b428b49a0aa2a75678e9fdb155f423e1e7c71c5ae9fef11a60b1`.

- Final `verdict_id` (covers the CTRL-E5 receipt): `af9e744cc3187400e00599159999f357f53c1df867556f62c8f71a552662800c`
  (also its `base_verdict_id`; no peer verdict given).
- Before CTRL-E5 was appended: `883caa9eebe6fd7af0de9e76b98646845bda1943673ac640f31af6d6c41e3eba`
  (`artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`).
- The verifier's own lines agree: `E3m FAIL: document rules: D4 required name(s) missing: ["windmill"]`; contract row
  E4 `NOT_RUN: receipt carries no restart facts`.

## What follows (INFERRED, by the stated rules)

1. On this host the whole E2E path runs on the GB10 native Omega engine with no CUDA and no fallback, in one declared
   attempt: every model call went to the chip and every effect, refusal and recovery row that ran gave the same result
   as on CPU. Model calls were 8 to 26 times faster than RUN-1.
2. E3m's FAIL is a property of the generated text, not of the effect machinery: the file was written once, the digest
   matched, the grant was spent once. It is still a FAIL under the frozen rules, and it is recorded as one.
3. Why the text differs from RUN-1 is UNKNOWN. Two things changed at once: the backend arithmetic, and the prompt. The
   proposal prompt names the workspace path (`crates/aien-runtime/src/spine.rs` `proposal_prompt`, line
   `Authorized workspace: {workspace}`), and the job folder differed (RUN-1 `.../jobs/85679b2b/...`, this run
   `.../jobs/ade3c837/...`). The prompts differ by one token (E3m 208 vs 207 tokens, E3 346 vs 345). E3's output was
   byte-identical anyway; E3m's output diverged at generated token 11. A CPU run with this run's workspace path would
   separate the two causes; it was not run (one declared attempt, and it would not change this verdict).
4. The CTRL-E5 tail works live: the verifier judged the row, the final `verdict_id` covers its receipt, and the
   pre-CTRL-E5 id is kept.

## Not claimed

Nothing about an installed artifact (E1, CTRL-E1), cross-machine agreement (E5), memory recall on the chip (E4,
CTRL-E4, CTRL-E4b did not run), the public model, native AIENOS, a hostile operator, the quality of the model's prose
beyond the checks, numerical equality of GB10 and CPU arithmetic, or the whole-system qualification as complete.
`gpu_executions` counts Omega chip calls; it is not a measure of work per call.

## Limits

- One host, one model, one attempt (rule 7). The Spark was shared; load and memory are in every receipt.
- A shakedown on the same build before the declared attempt (outside `runs/`, not a declared attempt) started the
  GB10 daemon with CAND-4 and shut it down cleanly; it is not evidence for any row.
- The verifier was built from the same tree it verifies. Independence comes from R7.
- The compose home lives under the job's temporary folder; the artifacts copied into the run folder are the evidence.
- `zero_cuda_gate_exit` is a string `"0"` in the receipts (the shell exit code passed through `jq --arg`).

## Next

- A new declared attempt (RUN-2-GB10 attempt 2, new folder with a note) after the E3m cause is known. Candidate
  next step: a CPU run of E3m with this run's workspace path to see whether the prompt or the arithmetic moved the text.
  Whether to change the fixture or the harness is a separate, pre-registered decision; the frozen rows do not change.
- R7 (second machine) can verify this folder as it verified RUN-1.
