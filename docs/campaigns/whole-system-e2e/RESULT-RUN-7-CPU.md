# WHOLE-SYSTEM-E2E, RUN-7-CPU (declared, CPU reference backend, first run under the logical-workspace prompt): result

Control for this run, pre-registered before its outcome: `ACCEPTANCE-v1.6-logical-workspace.md` (sc#432). It rests on the
product fix sc#431 (the model-visible prompt names the workspace by the constant label `workspace`, never by its path),
issue sc#429. Rows otherwise as `ACCEPTANCE-v1.5-fixed-root.md` over `-v1.3.1-e3m.md`, `-v1.3-gb10.md`, `-v1.3-ctrl-e5.md`,
`-v1.2.md`. Contract: FROZEN at aien-architecture b2400fe, sha256
`9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8` (PRE receipt). Earlier CPU run: RUN-1
(`RESULT-RUN-1.md`), kept as recorded and not replaced. Earlier GB10 attempts: RUN-2, RUN-3, RUN-5, RUN-6.

Product commit `8a05752310f188496ef96280fc851d66f2af9df8` (merged main; contains sc#431, sc#432 and sc#433), detached
checkout. `WSE2E_BACKEND=cpu`, `WSE2E_RUN_NAME=RUN-7-CPU`, `WSE2E_RUN_ROOT=/home/drakestapleton/.aien-e2e/run` (the harness
moved the RUN-6 state aside), `quietlock check` exit 0. Started detached 2026-10-11 14:41:28 UTC; receipts 14:44:07 to
14:56:05 UTC; harness exit about 14:56:05 UTC; never killed. Host `spark-b87b`, CAND-4 (`unsloth/Llama-3.2-1B-Instruct`,
weights `1ff795ff...`, tokenizer `6b9e4e7f...`, both equal to the candidate record). Run folder
`runs/RUN-7-CPU-20261011T144128Z/` (17 receipts in `chain/`, `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, `console.txt`,
`artifacts/`, five acceptance files as run, verifier outputs). **No program PASS is claimed.**

**Verdict: FAIL.** E3 failed D4 (the report does not name the three input files) and E4 failed (the model's edit reply
changes nothing in the file, three attempts). CTRL-E5 failed as a consequence of E3.

## Disclosures (OBSERVED)

- An earlier launcher call at 14:24:22 UTC started NO harness: the script was called without its `run` subcommand, printed
  its usage and exited. No run folder was made and no state was touched. Its log is kept outside the repo
  (`RUN-7-CPU.harness.log.usage-only-no-run`). The measured run is the one started at 14:41:28 UTC.
- Prompt-independence test before the run (control rule 2): `cargo test -p aien-runtime --test logical_workspace_test` with
  `AIEN_TOKENIZER_PATH` set to the CAND-4 tokenizer, on 8a05752: `2 passed; 0 failed`. Output kept as
  `runs/RUN-7-CPU-20261011T144128Z/logical_workspace_test.txt`.
- Two files sit in the run folder but OUTSIDE `chain/SHA256SUMS`, by the control's rules 2 and 6 (both written by hand
  after the run; the harness manifest was closed before them): `logical_workspace_test.txt` and
  `ACCEPTANCE-v1.6-logical-workspace.as-run.md` (a byte copy of the control from main; the harness copied v1.5 and
  this version adds no harness code). The other four `.as-run.md` files are the harness's own copies.
- `sha256sum -c chain/SHA256SUMS`, run from the committed copy of the folder: 99 lines, 0 bad lines.

## Rows as measured (OBSERVED, `VERDICT_ROWS.txt`, `console.txt`, `chain/`)

| row | status | what the receipt says |
|---|---|---|
| PRE | PASS | backend line `NativeTransformerBackend/CPU-reference (Omega GPU engine not linked, explicit fallback)`; host `spark-b87b`; model digests equal the candidate record; `run_root` `/home/drakestapleton/.aien-e2e/run`; `repo_commit` 8a05752 |
| CTRL-E3a | PASS | no desk key: daemon exited 1 before serving, names `NoDesk` |
| E1 | NOT_RUN | CPU run from a source build, nothing installed (R8) |
| E2 | PASS | objective recorded through `aien-cli`, id matches |
| CTRL-E2 | PASS | `EXTERNAL_IRREVERSIBLE`: exit 1, `ForbiddenEffectClass`, no record |
| E3 | **FAIL** | DONE, 3 inputs read, written `outbox/report.md`, second execute `AlreadySpent`; check D4 failed: none of `harbour-notes.txt`, `orchard-ledger.txt`, `windmill-log.txt` appears in the report (59 words; sections `# Harbour Notes`, `# Orchard Ledger`, `# Windmill Log`, each with the first sentence of the file and a stray `filename: outbox/report.md` line) |
| E3m | PASS | DONE; 105 words; topic words present; spent once |
| E4 | **FAIL** | SIGKILL then restart; memory included (`work`, 1 item, `memory.state included`); the second objective's propose did not commit: `edit reply changes nothing in the file`, on all three attempts (`artifacts/E4-propose.json`); no effect |
| E6 | PASS | SIGKILL after authorize, restart, same grant DONE, duplicates 0, third execute `AlreadySpent` |
| CTRL-E3b | PASS | revoked before spend: `Revoked`, no file |
| NET | PASS | 0 established TCP sockets |
| CTRL-E4b | RECORDED | memory files removed: refused with the same `changes nothing` reply, report unchanged. Not judged |
| CTRL-E6 | PASS | durable state removed: `E_MARK_TRUNCATED`, no second effect |
| CTRL-E4 | PASS (harness) / NOT_RUN (verifier) | store removed: named refusal `E_MARK_TRUNCATED` (mark 123, journal 0 records); the verifier derives NOT_RUN because E4 showed no recall |
| E5, CTRL-E1 | NOT_RUN | R7, R8 |
| CTRL-E5 | FAIL | one byte of `E3-report.md` changed: rejected, chain broken, but the verdict id cannot change because E3 already failed |

Backend: every model call ran on the CPU reference backend (`ReferenceCpuBackend`, `native_fallbacks 0` in the E4 attempts'
op report). The "explicit fallback" in the backend line is the CPU build's own label, not a GB10 fallback; this is a CPU
run, so G4/G5 (GB10 executions) do not apply.

Independent verifier (`tools/aien-verify`): chain unbroken, 0 chain problems, 1 status disagreement (CTRL-E4: receipt PASS,
facts derive NOT_RUN, the v1.1 rule). Final `verdict_id`
`e603c4adf491688cdeecc2912b59ea63a74947ab955d0ba941416c364292999c`, equal to its `base_verdict_id`
(`VERIFIER_RECEIPT.json`). RUN-6 recorded the same final id, because the verdict rows E3 FAIL and E4 FAIL are the same.

## New baseline prompt values (OBSERVED, `chain/006-E3.json`, `007-E3m.json`, `008-E4.json`, `prompt_attempts`)

These are the first values under the logical-workspace prompt. Recording only; no row is judged on them.

| row | attempt | prompt_tokens | prompt_ids_sha256 | outcome |
|---|---|---|---|---|
| E3 | 1 | 322 | `588fd1a3598eb10b9c347d81b0f84a458a5b71a86056ddb0f0c8cd6d5954d95a` | parsed (D4 fails on the written report) |
| E3m | 1 | 186 | `bf5c96beece0093c2e44ca616a7c56ae6f3a1f6ef3e0fc8215b7d199cabbafcf` | parsed |
| E4 | 1 | 348 | `5589bd8fd4d450466a640798a1f36c56fe3fd0376a73645f6f9a70b652097e3d` | refused, `edit reply changes nothing in the file` |
| E4 | 2 | 375 | `1b39ef362679d8dbdb5f213ca70b2a004bfd34e2c102b3f0156158991c0d0395` | refused, same reason |
| E4 | 3 | 375 | `1b39ef362679d8dbdb5f213ca70b2a004bfd34e2c102b3f0156158991c0d0395` | refused, same reason |

Earlier values (RUN-6, `RESULT-RUN-6-GB10.md`): E3 337/fc75a498, E3m 199/2cdf2541, E4 371/d9d73f4f, 398/7831e0e6,
398/7831e0e6. Each prompt is shorter than RUN-6's, consistent with the label `workspace` replacing the long path.
RUN-7-GB10, if it ran on the same commit, should show the same ids (not part of this result).

## Negative control (control rule 5), recorded, not judged

Method (OBSERVED, done after the run on the CPU only, no daemon, no chip): the E3 and E3m model inputs were rebuilt with the
product's own functions at 8a05752: the default ALLEN persona block (`PersonaContext::defaults().render()`, joined with a
newline), `spine::task_decision_with_inputs` on a scratch workspace holding the same three inbox files (contents as the
harness writes them, sha256 equal to `001-PRE.json`), the user turn in the model's chat template
(`ChatTokenizer::from_model_dir` on the CAND-4 model folder, `control::try_format_chat`), the compose assistant prefix
`filename:` (`spine::compose_assistant_prefix`), the tokenizer, then `spine::token_ids_sha256`. The persona is `default`
and the memory block is empty (0 items), as the E3 receipt reports. The scratch code was a throwaway test file and is not
committed.

| workspace path used | goal | tokens | prompt_ids_sha256 | equals the run's id |
|---|---|---|---|---|
| `/tmp/r7cpu-recon/throwaway-a/x/y/odd-name` (depth 4, odd name) | E3 | 322 | `588fd1a3...54d95a` | yes |
| `/tmp/r7cpu-recon/b/ws-other` (depth 2, other name) | E3 | 322 | `588fd1a3...54d95a` | yes |
| same two folders | E3m | 186 | `bf5c96be...bafcf` | yes |

Under the run root: a scratch workspace `/home/drakestapleton/.aien-e2e/run/r7cpu-recon-scratch/ws-contract` (E3 goal) gave
322 tokens, `588fd1a3598eb10b9c347d81b0f84a458a5b71a86056ddb0f0c8cd6d5954d95a`, equal to the run's E3 attempt 1. It was
built AFTER the RUN-7-GB10 harness had exited (that run was using the same root until about 15:02 UTC), in a sub-folder that
was deleted straight away; the run root's other content was not touched. All three paths agree with each other and with the
run: the negative control holds.

The first reconstruction attempt used the wrong chat template (Zephyr, from loading the tokenizer file alone) and gave 326
tokens / `6327aca9...`, which does not match the run; it was discarded as a method error and rebuilt through the model folder,
which is how the daemon loads it. Not hidden: the matching result above is the second method.

## What follows (INFERRED unless marked)

1. **The prompt no longer depends on the path, as designed (OBSERVED + control).** The reconstruction gives the same ids
   under three different folder names and depths, equal to what the run recorded for E3 and E3m. Together with the 2 passed
   prompt-independence tests, this supports that the workspace path is out of the model input. It does not prove that
   nothing else host-specific is in it.
2. **With the path out of the prompt, E3 (D4) and E4 still fail on this model.** The E3 report has the same failing shape as
   RUN-5 and RUN-6 (per-file sections with the file's first sentence, no file names); E4's reply re-emits a fresh report
   headed `# Harbour, Orchard, and Windmill` and does not append the requested line, so the content check refuses it. Both
   are the 1B model's greedy output for these prompts. Fallback 0, chain unbroken, every effect and refusal row PASS: not a
   harness, chain or backend fault.
3. **The path-dependence reading from RUN-6 is narrowed, not closed.** RUN-6 inferred that E3 and E4 outcomes on this model
   follow incidental prompt text. RUN-7-CPU shows a changed prompt (new ids) and the same two failures on the CPU. That is
   consistent with the failures being about the model's answer to the task wording rather than the path, but one CPU run
   under one prompt cannot separate the two.
4. **E4's memory was included but the model did not use it as needed.** `memory.state included`, 1 item. The failure reason is
   the content check on the reply, as in RUN-6; this run does not show whether the model could do the edit with a different
   prompt.
5. **CTRL-E5's FAIL is a consequence of E3's FAIL**, as in RUN-5 and RUN-6.
6. **A rerun under the same prompt, binary and pins would be expected to repeat this** (greedy decoding; not run). A new
   declared attempt needs a change on purpose: a prompt or task-wording decision, or a model decision, each Drake's call.
   Issue sc#402 (R6) stays open; sc#429's product part is done by sc#431, and this run is its first measurement.

## Not claimed

Program PASS; that the failing rows are caused by the model rather than by the task wording (not separated); that a rerun
reproduces this result; that RUN-7-CPU and RUN-1 agree (RUN-1 ran an earlier prompt, no row-by-row comparison is made
here); numerical equality of CPU and GB10 arithmetic; an installed artifact (E1, CTRL-E1); cross-machine agreement (E5);
prose quality beyond the checks.

## Limits

One host, one CPU attempt, shared machine. The verifier was built from the tree it verifies; independence comes from R7.
`runs/` content was not edited after the run, apart from the two hand-added files named above. The reconstruction repeats
the daemon's prompt assembly from the product's functions; it is not a capture of the daemon's actual bytes, which are not
recorded (only their token count and id are).
