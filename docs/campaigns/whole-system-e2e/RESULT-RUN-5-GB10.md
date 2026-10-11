# WHOLE-SYSTEM-E2E, RUN-5-GB10 (declared, GB10 native Omega backend, attempt 3): result

Previous attempts, kept as recorded: RUN-2-GB10 (FAIL, E3m exact-match rule, since corrected by v1.3.1) and RUN-3-GB10
(`RESULT-RUN-3-GB10.md`, FAIL: E4's edit refused because the model wrote `filename: ./report.md`). RUN-4 is Lane I's
install run. Repairs made after RUN-3 and before this run, each pre-registered before its code:
`REGRESSION-sc402-dotslash.md` (sc#425 4272f42: a leading `./` names the same workspace file; every other path form
refused as before) and `VERIFIER-v1.1-e3m.md` (sc#424 d27ad7b, closes sc#423: the verifier's E3m topic check follows the
receipt's `d4_rule`; CTRL-E4 rule kept). Rows: `ACCEPTANCE-v1.3.1-e3m.md` over `-v1.3-gb10.md`, `-v1.3-ctrl-e5.md`,
`-v1.2.md`. Contract: FROZEN at aien-architecture b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`.

Harness `scripts/whole_system_e2e.sh` v1.3.1 at d27ad7b (merged main, detached checkout), `WSE2E_BACKEND=gb10`,
`WSE2E_RUN_NAME=RUN-5-GB10`, `quietlock check` clear, no other GPU process, started detached (setsid nohup) 2026-10-11
05:56:31 UTC, rows 05:59:13 to 06:03:04 UTC, harness exit 0, never killed. Job folder `.claude/jobs/ade3c837-a2` (its own,
so this run could not collide with Lane I's run in the shared job folder; see the E3 finding below). Run folder
`runs/RUN-5-GB10-20261011T055631Z/` (17 receipts, `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, `GB10_SUMMARY.json`, eight
daemon logs, acceptance files as run, verifier outputs). Host `spark-b87b`, NVIDIA GB10, driver 580.173.02, pins omega
6c6180c, physics 6d7cf0d, aienos b84c0a6 (same as RUN-3), CAND-4 (Llama 3.2 1B Instruct). `sha256sum -c chain/SHA256SUMS`
from the committed tree: 0 bad lines. **No program PASS is claimed.**

**Verdict: FAIL.** E3 failed check D4: the report did not name the three input files. CTRL-E5 failed as a consequence
(below). E4 (memory across a SIGKILL restart) PASSED on the chip for the first time, and the verifier judged CTRL-E4 PASS.

## Rows as measured (OBSERVED)

`gpu` is `gpu_executions` for the row; `fb` is `fallback_count`; a dash means no model call.

| row | status | gpu | fb | what the receipt says |
|---|---|---|---|---|
| PRE | PASS | - | 0 | native Omega backend line; model digests equal the candidate record; zero-CUDA gate exit 0 |
| CTRL-E3a | PASS | - | 0 | no desk key: daemon exited 1 before serving, names `NoDesk` |
| E1 | NOT_RUN | - | 0 | nothing installed (R8; Lane I) |
| E2 | PASS | - | 0 | objective recorded through `aien-cli`, id matches |
| CTRL-E2 | PASS | - | 0 | `EXTERNAL_IRREVERSIBLE`: exit 1, `ForbiddenEffectClass`, no record |
| E3 | **FAIL** | 51910 | 0 | DONE, 3 inputs read, spent once (`AlreadySpent`); D4: `harbour-notes.txt`, `orchard-ledger.txt`, `windmill-log.txt` not named (52 words, `## Harbour Notes` style headings instead) |
| E3m | PASS | 60210 | 0 | DONE; 185 words; topic words present; spent once |
| E4 | **PASS** | 90712 | 0 | `remember` ok; SIGKILL (137), restart (`Reconcile: checked 0 unsettled`); second objective DONE on attempt 1 (reply `filename: report.md`); report 7 to 8 lines, appended line names `harbour` |
| E6 | PASS | 20866 | 0 | SIGKILL after authorize; no write while dead; same grant DONE once; third execute `AlreadySpent` |
| CTRL-E3b | PASS | 31094 | 0 | revoked before spend: `Revoked`; `ctrl.md` absent |
| NET | PASS | - | 0 | 0 established TCP sockets |
| CTRL-E4b | RECORDED | 398430 | 0 | memory files removed: three attempts cut at the 400-token limit (`max_tokens`), no proposal. Not judged |
| CTRL-E6 | PASS | 30740 | 0 | home removed: same grant refused `E_MARK_TRUNCATED`; no second effect |
| CTRL-E4 | PASS | - | 0 | store removed: refused by name (Cortex journal 0 records against record mark 128); report unchanged. Verifier: PASS (E4 recalled) |
| E5, CTRL-E1 | NOT_RUN | - | 0 | R7, R8 |
| CTRL-E5 | **FAIL** | - | 0 | byte 3 of `artifacts/E3-report.md` changed on a copy: rejected `DIGEST_MISMATCH`, chain broken, but verdict id `cb72e9d9` to `cb72e9d9` (unchanged) |

GB10 rules, OBSERVED: `fallback_detected` false, 0 fallback lines over all 8 daemon logs, every model attempt's
`native_fallbacks` 0; all 7 serving daemons printed `STRICT strict=true dev_fallback_build=false`; the no-key start
exited at the desk check. Load average 1.2 to 1.8; `MemFree` about 6 GB during the rows (22 GB at the end),
`MemAvailable` about 101 GB (page cache); no GPU memory refusal in any daemon log (omega#327 not seen).

Independent verifier (`tools/aien-verify` 04e8f5d2..., built from d27ad7b, so v1.1): chain unbroken, 0 chain problems,
**0 status disagreements** (RUN-3 had two). Final `verdict_id` `08b5f62132235327df08a343683ce72e75276f891df477a67b6c5b2dfad37ccf`
(covers the live CTRL-E5 receipt); before CTRL-E5 `cb72e9d914e362b17f8f09dec2c0e6319174db5bcea88f31cdaa96f297cb6a25`
(`artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`). Contract rows: E2, E4, E6, CTRL-E2, CTRL-E3, CTRL-E4, CTRL-E6 PASS; E3,
CTRL-E5 FAIL; E1, E5, CTRL-E1 NOT_RUN.

## What follows (INFERRED)

1. **E3's FAIL comes from a changed prompt, not from the repairs' logic.** The proposal prompt carries the absolute
   workspace path (`spine::proposal_prompt`: `Authorized workspace: {workspace}`). RUN-2 and RUN-3 ran under the shared job
   folder `.claude/jobs/ade3c837/tmp/e2e/run/ws-contract`, RUN-1 (CPU) under `.claude/jobs/85679b2b/...` (a folder name of
   the same length; it wrote the same E3 report); this run under `.claude/jobs/ade3c837-a2/...`. OBSERVED after
   the run, on the CPU only (no model, no chip): the E3 model input was rebuilt with the product's own functions on main
   (the default ALLEN persona block, then `spine::task_prompt` on a scratch workspace holding the same three inbox files,
   sha256 equal to the receipts, the scratch path replaced by each run's path, the chat template and the compose assistant
   prefix, then the model tokenizer). It gives 345 tokens, `prompt_ids_sha256` ede9f1e8... for the RUN-3 path and 347
   tokens, 6847127a... for the RUN-5 path: exactly the ids each run recorded. The path occurs once in the prompt. Model
   weights digest 1ff795ff..., tokenizer 6b9e4e7f..., backend line, pins, objective text, inputs, persona (`default`),
   memory (`work`, 0 items) and recognized requirements are equal in the two E3 receipts. So the only difference in the
   model input is the workspace path; greedy decoding (RUN-2 and RUN-3 gave byte-identical E3 reports) then wrote a
   different 52-word report without the file names. INFERRED: the path change caused the text change (greedy decoding
   is deterministic on the same input and engine pins; the daemon binary differs from RUN-3 only by sc#425, which does
   not touch generation; not re-run).
2. So E3's D4 outcome depends on the workspace path text as well as on the goal: a 1B model's answer is sensitive
   to an incidental two-token prompt change (one same-length folder change, RUN-1 to RUN-2, did not change the report). That is the single remaining cause of this FAIL.
3. CTRL-E5 failed only because E3 had already failed: the control changes one byte of `E3-report.md`, which turns E3
   from FAIL (D4) into FAIL (DIGEST_MISMATCH), so the verdict id cannot change. The verifier did reject the copy and
   the chain did break. With E3 passing (RUN-3), the same control passed.
4. The `./` repair was NOT exercised: in this run the model answered `filename: report.md` in E4 (first generated tokens
   ` report.md`). E4's PASS shows recall across a SIGKILL restart on the chip; it does not show the repair working on the
   chip. The repair is covered by unit tests only (`REGRESSION-sc402-dotslash.md`).

## Not claimed

Program PASS; that the `./` repair was exercised on the chip; that the job-folder path is proven by a run (rather than by the input
reconstruction above) to be the cause of E3's text change; an installed artifact (E1, CTRL-E1); cross-machine agreement (E5); numerical equality of
GB10 and CPU arithmetic; prose quality beyond the checks.

## Limits

One host, one attempt, shared machine (load in every receipt). The verifier was built from the tree it verifies;
independence comes from R7. `runs/` content was not edited after the run.
