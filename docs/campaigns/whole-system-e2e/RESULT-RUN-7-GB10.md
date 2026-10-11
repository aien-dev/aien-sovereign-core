# WHOLE-SYSTEM-E2E, RUN-7-GB10 (declared, GB10 native Omega backend, attempt 5, logical-workspace prompt): result

Earlier declared attempts, kept as recorded: RUN-2-GB10, RUN-3-GB10, RUN-5-GB10 and RUN-6-GB10
(`RESULT-RUN-6-GB10.md`: FAIL, E3 D4 and E4). RUN-4 is Lane I's install run. The CPU baseline of the same day is
RUN-7-CPU (its result file is written by a sibling lane; its run folder `RUN-7-CPU-20261011T144128Z` was read locally for
the prompt comparison below, its PR was not visible when this was written).
Control for this run, pre-registered before its code and before any outcome under it:
`ACCEPTANCE-v1.6-logical-workspace.md` (sc#432). The product change under test is #429/sc#431: the model-visible proposal
prompt names the workspace by the constant label `workspace` instead of the run-state path. Rows otherwise as
`ACCEPTANCE-v1.5-fixed-root.md` over `-v1.3.1-e3m.md`, `-v1.3-gb10.md`, `-v1.3-ctrl-e5.md`, `-v1.2.md`. Contract FROZEN at
aien-architecture b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`.

Main commit `00c45832fb247860078927c5f3bc0ecc5d352ca9` (contains sc#431, sc#432, sc#433, sc#434), `WSE2E_BACKEND=gb10`,
`WSE2E_RUN_NAME=RUN-7-GB10`, `WSE2E_RUN_ROOT=/home/drakestapleton/.aien-e2e/run` (RUN-7-CPU state moved aside by the
harness). Before the run: `quietlock check` exit 0, `nvidia-smi` 0 %, no other harness alive (coordinator, OBSERVED).
Started detached 2026-10-11T14:56:54Z, harness exit about 15:02:30Z, never killed, chip 0 % after (coordinator). Control
rule 2: `cargo test -p aien-runtime --test logical_workspace_test` with the CAND-4 tokenizer passed 2 of 2 on that commit
(`logical_workspace_test.txt`, copied into the run folder). Run folder `runs/RUN-7-GB10-20261011T145654Z/` (17 receipts,
`chain/SHA256SUMS`, `VERDICT_ROWS.txt`, `GB10_SUMMARY.json`, eight daemon logs, verifier outputs). Host `spark-b87b`,
NVIDIA GB10, driver 580.173.02, pins omega 6c6180c, physics 6d7cf0d, aienos b84c0a6, model weights digest `1ff795ff...`,
tokenizer `6b9e4e7f...` (CAND-4, Llama 3.2 1B Instruct; `chain/001-PRE.json`). Two files in the run folder sit outside
`chain/SHA256SUMS` by design (control rules 2 and 6): `ACCEPTANCE-v1.6-logical-workspace.as-run.md` (byte copy of the
file on main) and `logical_workspace_test.txt`. `sha256sum -c chain/SHA256SUMS` from the committed tree: 103 lines, 0 bad.
**No program PASS is claimed.**

**Verdict: FAIL.** E3 failed D4 (the report did not name the three input files) and E4 failed (the model's edit reply
left the report unchanged in all three attempts). CTRL-E5 failed as a consequence of E3.

## Rows as measured (OBSERVED, `VERDICT_ROWS.txt`, `GB10_SUMMARY.json`)

`gpu` is `gpu_executions`; `fb` is `fallback_count`; a dash means no model call.

| row | status | gpu | fb | what the receipt says |
|---|---|---|---|---|
| PRE | PASS | - | 0 | backend line `OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)`; run_root `/home/drakestapleton/.aien-e2e/run` |
| CTRL-E3a | PASS | - | 0 | no desk key: daemon exited before serving |
| E1 | NOT_RUN | - | 0 | gb10 source-build mode (R8) |
| E2 | PASS | - | 0 | objective recorded through `aien-cli` |
| CTRL-E2 | PASS | - | 0 | refused before execution: `ForbiddenEffectClass` |
| E3 | **FAIL** | 54178 | 0 | DONE, 3 inputs read, spent once (`AlreadySpent`); D4: `harbour-notes.txt`, `orchard-ledger.txt`, `windmill-log.txt` all absent |
| E3m | PASS | 40716 | 0 | DONE; spent once |
| E4 | **FAIL** | 175724 | 0 | SIGKILL, restart; 3 attempts refused `edit reply changes nothing in the file`; effect_state `none` |
| E6 | PASS | 19026 | 0 | killed after authorize, restart, effect once, duplicates 0, third execute `AlreadySpent` |
| CTRL-E3b | PASS | 29060 | 0 | revoked before spend |
| NET | PASS | - | 0 | harness row |
| CTRL-E4b | RECORDED | 162192 | 0 | 3 attempts refused (same reason). Not judged |
| CTRL-E6 | PASS | 28900 | 0 | durable state removed: `E_MARK_TRUNCATED`, no second effect |
| CTRL-E4 | PASS (harness) / NOT_RUN (verifier) | - | 0 | the verifier derives NOT_RUN because E4 showed no recall (v1.1 rule) |
| E5, CTRL-E1 | NOT_RUN | - | 0 | R7, R8 |
| CTRL-E5 | FAIL | - | 0 | verifier: "mutated verdict id is not a different, well-formed id", a consequence of E3 failing (same as RUN-6) |

G4/G5: `fallback_detected: false`, `fallback_log_lines_all_daemons: 0` over 8 daemon starts, every model-calling row
`gpu_executions >= 1`. Host load in the receipts: loadavg 2.4 to 4.2, `MemAvailable` about 101 GB.

Independent verifier (`VERIFIER_VERDICT.txt`): chain unbroken true; final `verdict_id`
`e603c4adf491688cdeecc2912b59ea63a74947ab955d0ba941416c364292999c`. This id equals RUN-6-GB10's final id (OBSERVED by
comparing the two strings); the verdict file does not include prompt text, so equal ids reflect equal row verdicts only.

## E3 and E4 text (OBSERVED, `artifacts/`)

- E3 report (`E3-report.md`, 59 words): three sections headed `# Harbour Notes`, `# Orchard Ledger`, `# Windmill Log`, each
  with one summary sentence and a repeated `filename: outbox/report.md` line. The strings `harbour-notes.txt`,
  `orchard-ledger.txt` and `windmill-log.txt` do not appear. Same failing shape as RUN-5 and RUN-6.
- E4 propose (`E4-propose.json`): all three attempts answered `filename: report.md` followed by a new rewrite beginning
  `# Harbour, Orchard, and Windmill`, and each was refused `edit reply changes nothing in the file` (finish `eos`). No
  `./report.md` refusal, so the sc#425 repair is not the cause here.
- Memory in E4: the E4 receipt records `memory_items_included` 0 and `memory_context` empty; `artifacts/E4-propose.json`
  shows the propose report with memory `context: work`, `items_included: 1`. Both are as recorded and the difference is
  NOT explained here (UNVERIFIED which objective each refers to; RUN-6 reported one included item).

## Prompt ids, new baseline (OBSERVED, `chain/006-E3.json`, `007-E3m.json`, `008-E4.json`)

`prompt_tokens/prompt_ids_sha256 (first 8)` per proposal attempt.

| run | state path | E3 | E3m | E4 |
|---|---|---|---|---|
| RUN-6-GB10 (before #429) | `/home/drakestapleton/.aien-e2e/run` | 337/fc75a498 | 199/2cdf2541 | 371/d9d73f4f, 398/7831e0e6, 398/7831e0e6 |
| RUN-7-GB10 | `/home/drakestapleton/.aien-e2e/run` | 322/588fd1a3 | 186/bf5c96be | 348/5589bd8f, 375/1b39ef36, 375/1b39ef36 |
| RUN-7-CPU (local run folder) | `RUN-7-CPU-20261011T144128Z` | 322/588fd1a3 | 186/bf5c96be | 348/5589bd8f, 375/1b39ef36, 375/1b39ef36 |

RUN-7-GB10 equals RUN-7-CPU in every prompt token count and every full sha256 (OBSERVED: the full ids in the two
run folders compare equal). Both differ from RUN-6, as the product change intends. Full ids are in the receipts above.
Daemon binaries differ between the two backends (`daemon_sha256` 319c082b... for GB10, 8b5279d0... for CPU), which is
expected for different backend builds and is not otherwise examined.

## What follows (INFERRED unless marked)

1. **The input is now backend- and path-independent as designed.** Equal prompt ids across CPU and GB10 show the model
   sees the same tokens on both machines. Together with the 2 of 2 unit test, the #429 change did what it was written to
   do.
2. **The model still fails the same two rows.** Under the new baseline the CAND-4 1B model's greedy output does not name
   the input files (E3 D4) and does not append the line (E4). Not a harness, chain, chip or fallback fault: fallback 0,
   chain unbroken, every effect and refusal row PASS. Whether RUN-7-CPU gives the same row outcomes is for its own result
   file; this file does not state it.
3. **Not a path effect.** RUN-3, RUN-5 and RUN-6 suggested the outcome varied with the path text. With the path removed
   from the prompt, E3 and E4 still fail, so the earlier path-dependence reading is not sufficient to explain the
   failures. The remaining cause is the model and prompt content on these tasks (INFERRED; no ablation run).
4. **Ruling.** No further R6 attempt without a pre-registered input change: the public-model decision D1 (Drake's call,
   see sc#434 for the Qwen3-8B evaluation) or a product issue on the E3/E4 prompt. A rerun on the same binary, pins and
   prompt is expected to repeat this result (greedy decoding; not run).

## Not claimed

Program PASS; installed-artifact rows (E1, CTRL-E1); cross-machine agreement (E5); numerical equality of GB10 and CPU
arithmetic; the cause of the E4 memory-count difference; that a rerun reproduces this result; prose quality beyond the
checks.

## Limits

One host, one attempt, shared machine (load in every receipt). The verifier was built from the tree it verifies;
independence comes from R7. The RUN-7-CPU comparison used its local run folder, not a merged result. `runs/` content was
not edited after the run.
