# WHOLE-SYSTEM-E2E, RUN-6-GB10 (declared, GB10 native Omega backend, attempt 4, fixed run root): result

Previous attempts, kept as recorded: RUN-2-GB10 (FAIL, E3m exact-match rule, corrected by v1.3.1), RUN-3-GB10
(`RESULT-RUN-3-GB10.md`, FAIL: E4 refused `./report.md`, repaired by sc#425) and RUN-5-GB10 (`RESULT-RUN-5-GB10.md`, FAIL:
E3 D4; its input reconstruction showed the E3 prompt changes with the job-folder path). RUN-4 is Lane I's install run.
Control for this run, pre-registered before its code and before any outcome under it: `ACCEPTANCE-v1.5-fixed-root.md`
(sc#428, 805be03): the run state lives at the neutral constant root `/home/drakestapleton/.aien-e2e/run`, and E3, E3m and
E4 record the daemon's prompt token count and prompt id. Rows otherwise as `ACCEPTANCE-v1.3.1-e3m.md` over
`-v1.3-gb10.md`, `-v1.3-ctrl-e5.md`, `-v1.2.md`. Contract: FROZEN at aien-architecture b2400fe, sha256
`9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`.

Harness v1.5 at 805be03 (merged main, detached checkout), `WSE2E_BACKEND=gb10`, `WSE2E_RUN_NAME=RUN-6-GB10`,
`WSE2E_RUN_ROOT=/home/drakestapleton/.aien-e2e/run` (empty before the run), `quietlock check` exit 0, `nvidia-smi` 0 %
with no compute process, no other harness alive (RUN-4-INSTALL closed). Started detached (setsid nohup) 2026-10-11
06:53:57 UTC, rows 06:56:08 to 06:59:31 UTC, harness exit 0, never killed. Run folder
`runs/RUN-6-GB10-20261011T065357Z/` (17 receipts, `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, `GB10_SUMMARY.json`, eight
daemon logs, acceptance files as run, verifier outputs). Host `spark-b87b`, NVIDIA GB10, driver 580.173.02, pins omega
6c6180c, physics 6d7cf0d, aienos b84c0a6, CAND-4 (Llama 3.2 1B Instruct). `sha256sum -c chain/SHA256SUMS` from the
committed tree: 0 bad lines. **No program PASS is claimed.**

**Verdict: FAIL.** E3 failed D4 (the report did not name the three input files) and E4 failed (the model's edit reply
left the report unchanged three times). CTRL-E5 failed as a consequence of E3.

## Rows as measured (OBSERVED)

`gpu` is `gpu_executions` for the row; `fb` is `fallback_count`; a dash means no model call.

| row | status | gpu | fb | what the receipt says |
|---|---|---|---|---|
| PRE | PASS | - | 0 | native Omega backend line; model digests equal the candidate record; `run_root` `/home/drakestapleton/.aien-e2e/run` |
| CTRL-E3a | PASS | - | 0 | no desk key: daemon exited 1 before serving, names `NoDesk` |
| E1 | NOT_RUN | - | 0 | gb10 source-build mode, nothing installed (R8) |
| E2 | PASS | - | 0 | objective recorded through `aien-cli`, id matches |
| CTRL-E2 | PASS | - | 0 | `EXTERNAL_IRREVERSIBLE`: exit 1, `ForbiddenEffectClass`, no record |
| E3 | **FAIL** | 55378 | 0 | DONE, 3 inputs read, spent once (`AlreadySpent`); D4: the three file names absent (59 words, `# Harbour Notes` style sections with repeated `filename:` lines) |
| E3m | PASS | 43696 | 0 | DONE; 110 words; topic words present; spent once |
| E4 | **FAIL** | 187680 | 0 | SIGKILL, restart; memory included (`work`, 1 item, in the propose report); three attempts refused `edit reply changes nothing in the file` (the model re-emitted the report without the new line); no effect |
| E6 | PASS | 20066 | 0 | SIGKILL after authorize, restart, effect once, duplicates 0, third execute `AlreadySpent` |
| CTRL-E3b | PASS | 30294 | 0 | revoked before spend: `Revoked`, no file |
| NET | PASS | - | 0 | 0 established TCP sockets |
| CTRL-E4b | RECORDED | 173532 | 0 | memory files removed: refused (same `changes nothing` reply). Not judged |
| CTRL-E6 | PASS | 29940 | 0 | durable state removed: `E_MARK_TRUNCATED`, no second effect |
| CTRL-E4 | PASS (harness) / NOT_RUN (verifier) | - | 0 | store removed: named refusal `E_MARK_TRUNCATED`; the verifier derives NOT_RUN because E4 showed no recall |
| E5, CTRL-E1 | NOT_RUN | - | 0 | R7, R8 |
| CTRL-E5 | FAIL | - | 0 | one byte of `E3-report.md` changed: rejected, chain broken, but the verdict id cannot change because E3 already failed |

G4/G5: `fallback_detected: false` over eight daemon starts, every model-calling row `gpu_executions >= 1`.
Host load in the receipts: loadavg 1.36 to 1.54, `MemAvailable` about 101 GB (`MemFree` about 5 GB, 21 GB at the end).

Independent verifier (`tools/aien-verify` 532bf5f1..., built from 805be03; its source equals d27ad7b's, v1.1): chain
unbroken, 0 chain problems, 1 status disagreement (CTRL-E4: receipt PASS, facts derive NOT_RUN, the v1.1 rule when E4
shows no recall). Pre-CTRL-E5 `verdict_id` `2e4fc8ef98d27dfe10556712bbc23683ecb1fd395803b6a4c2644b04c89fb942`; final
`verdict_id` `e603c4adf491688cdeecc2912b59ea63a74947ab955d0ba941416c364292999c`. RUN-5's final was `08b5f621...`.

## Prompt ids (OBSERVED; RUN-6 from its receipts, earlier runs from their propose artifacts)

`prompt_tokens/prompt_ids_sha256 (first 8)/outcome` per proposal attempt.

| run | run state path | E3 | E3m | E4 |
|---|---|---|---|---|
| RUN-2-GB10 | `.claude/jobs/ade3c837/tmp/e2e/run` | 345/ede9f1e8 | 207/932fcc21 | not run |
| RUN-3-GB10 | `.claude/jobs/ade3c837/tmp/e2e/run` | 345/ede9f1e8 (PASS) | 207/932fcc21 | 338/2438077f, 365/784afb08, 369/de2a05f8 (all refused, `./`) |
| RUN-5-GB10 | `.claude/jobs/ade3c837-a2/tmp/e2e/run` | 347/6847127a (FAIL D4) | 209/748121a0 | 462/23b350f1 (PASS) |
| RUN-6-GB10 | `/home/drakestapleton/.aien-e2e/run` | 337/fc75a498 (FAIL D4) | 199/2cdf2541 | 371/d9d73f4f, 398/7831e0e6, 398/7831e0e6 (all refused) |

Full ids are in `chain/006-E3.json`, `007-E3m.json`, `008-E4.json` (`prompt_attempts`).

## What follows (INFERRED unless marked)

1. **The daemon is the same binary as RUN-5's** (OBSERVED: `daemon_sha256` 68ce03f8... in both PRE receipts; crates and
   the verifier source are unchanged between d27ad7b and 805be03). Weights, tokenizer, pins, objective, inputs and backend
   line are equal. The E3, E3m and E4 prompts differ from RUN-5's by token count and id, consistent with the run-state
   path being the only changed model input (rebuilt-input proof done for RUN-3 vs RUN-5 only; NOT re-done here).
2. **Under the neutral fixed root, two model-content rows fail.** E3's report has the same failing shape as RUN-5's
   (per-file sections, no file names) with different text; E4's reply re-emits the report without the appended line. Both
   are the 1B model's greedy output on these prompts, not a harness, chain, chip or fallback fault (fallback 0, chain
   unbroken, every effect and refusal row PASS).
3. **The `./` repair (sc#425) was exercised on the chip for the first time.** E4 attempts 2 and 3 answered
   `filename: ./report.md`, and the refusal was `edit reply changes nothing in the file`, the content check that runs after
   the path is resolved to the existing report; in RUN-3 the same answer was refused for its path. INFERRED from the
   refusal reason; the repair itself is covered by its unit tests (`REGRESSION-sc402-dotslash.md`).
4. **Single remaining cause.** With the run root held constant, E3 (D4) and E4 fail because the CAND-4 1B model's greedy
   answer to these prompts does not name the input files and does not append the line. Across RUN-3, RUN-5 and RUN-6 the
   same rows passed or failed as only the path text changed, so the outcome of these rows on this model is path-dependent
   rather than a property of the system under test.
5. **Further attempt: not justified as is.** Greedy decoding on the same binary, pins and fixed root gives the same input,
   so a rerun would be expected to repeat this result (not run). Moving the root to a path that passes is selecting on
   outcome and is forbidden by v1.5. A new declared attempt is justified only after a pre-registered product change that
   alters the input on purpose, for example sc#429 (a stable logical workspace name in the prompt, which needs a new
   baseline) or a model or prompt decision, each Drake's call.

## Not claimed

Program PASS; that the path is proven by a run to be the cause of the E3 and E4 text (it is the only changed input,
shown by reconstruction for RUN-3 vs RUN-5 only); that a rerun reproduces this result; that the `./` repair works
beyond the observed refusal reason; an installed artifact (E1, CTRL-E1); cross-machine agreement (E5); numerical equality
of GB10 and CPU arithmetic; prose quality beyond the checks.

## Limits

One host, one attempt, shared machine (load in every receipt). The fixed root contains this machine's home path, so it
holds the input constant on this host only. The verifier was built from the tree it verifies; independence comes from R7.
`runs/` content was not edited after the run.
