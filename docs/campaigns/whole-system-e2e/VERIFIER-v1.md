# WHOLE-SYSTEM-E2E, verifier v1 (lane L4): pre-registration

Tracking: aien-dev/aien-sovereign-core#381, aien-dev/aien-architecture#190. Contract read: `ACCEPTANCE_E2E.md` sections 1, 2, 3, 5
(aien-architecture main 3c584a49, sha256 `8ba653848287bc77db253466f6d3ff37c70a42be55601ad47eb1f838a41d20f0`). The
frozen digest is passed on the command line (`--contract-sha256`), never compiled in. This file is committed before the
verifier and its tests exist. The fixtures below are never edited and never written to.

## What the verifier is

`tools/aien-verify/src/main.rs`: one file, Rust standard library only, its own sha256. It reads a run folder (the `chain/`
receipts, `chain/SHA256SUMS`, the output files under `artifacts/`) and nothing else. It does not call the daemon, the CLI
or the harness, and it does not copy the harness status word of any receipt: every row is re-derived from receipt facts and
files. Where a receipt lacks the facts a row needs, the row is `NOT_RUN` with a stated reason. Where the facts show the
required behaviour did not happen, the row is `FAIL`. `PASS` needs every listed fact.

## Chain checks (all must hold for `chain_unbroken`)

1. `chain/NNN-*.json` numbered 001 upward with no gap; the step in each file equals the step in its name.
2. `prev_receipt_sha256` of the first file is 64 zeros; of each later file the sha256 of the previous file's bytes.
3. `objective_id` and `run_id` identical in every receipt.
4. `chain/SHA256SUMS` lists every file of the run folder except itself and the verifier's own two output files, and every
   listed digest matches the file. A file in the folder but not in the manifest is a break. A listed file that is missing
   is a break, except a missing `*.log` path, which is recorded as a note (daemon and console logs are never an input to a
   row, and the committed fixtures omit them because the repository ignores `*.log`).
5. `objective_id` recomputes as sha256 of `AIEN_E2E_OBJECTIVE_V1`, LF, objective text (text taken from the E2 receipt).
   `--objective-id` if given must equal it. `run_id` recomputes from `objective_id`, `daemon_sha256` and
   `model_weights_digest` of the PRE receipt, and `host`, joined by LF.
6. No symlinks in the run folder.

## Row derivation (verdict rows E1..E6, CTRL-E1..CTRL-E6; supplementary PRE, E3m, CTRL-E3a, CTRL-E3b, NET, CTRL-E4 detail)

- E1: PASS needs `signature_check` of `verified` or `dry-run-key`, a 64-hex `artifact_sha256`, `files_verified` above 0,
  64-hex `daemon_sha256` and `cli_sha256`. `signature_check` `none` or no artifact: NOT_RUN.
- E2: PASS needs `objective_text` whose sha256 equals `objective_text_sha256`, `recorded_before_work` true,
  `operator_surface` exactly `aien-cli`, an installation-side record (`installation_record` not starting `NOT_RUN`), and
  no effect receipt (E3, E3m, E4, E6) earlier in the chain than E2. A harness-only record is NOT_RUN.
- E3 (and E3m for the mechanics objective): `effect_state` DONE (otherwise FAIL: no file written); the output file
  `artifacts/<STEP>-<basename of written_path>` exists in the run folder (absent: FAIL); its sha256 equals
  `content_sha256` (and `disk_sha256`); document rules: valid UTF-8, first line starts `# `, at most 200 words
  (whitespace split), every required name present (E3: the `inbox_files` names of the PRE receipt; E3m: the
  `required_names` array if present, else the topics after `Mention` in the receipt's `objective_text`); `desk` is
  `required`; `grant_id` a number; `second_execute_refusal` `AlreadySpent` (absent: NOT_RUN); if `compose_receipt` is an
  object, its `success` is true. The contract's `result_digest` is read as the harness's `content_sha256`; a null
  `compose_receipt` is not a failure in v1.
- E4: needs `restart_kind` (`graceful` or `sigkill`; absent: NOT_RUN), then `effect_state` DONE (else FAIL), `recall_after_restart`
  true (else FAIL; absent NOT_RUN), `memory_store`, 64-hex `item_sha256` (equal to the sha256 of
  `artifacts/E4-remember.json` when that file exists), `recall_utc` not earlier than `restart_utc`, and files
  `artifacts/E4-report.md` (after) and the E3m report (before, else the E3 report): after starts with before, has
  exactly one more line, and that last line names the first required name (case-insensitive). Missing files: FAIL.
- E5: PASS only with `--peer-verdict-id` equal to this machine's base verdict id (the id computed with E5 NOT_RUN) and a
  chain that is unbroken. A different peer id: FAIL. No peer id: NOT_RUN ("cross-machine agreement not shown"). Both
  machines run with the other's base id and then print the same final `verdict_id`.
- E6: `kill_point` non-empty, `restart_count` at least 1, `effects_before_kill` 0, `effects_after_restart` 1,
  `duplicates` 0, `grant_id` a number, `third_execute_refusal` `AlreadySpent`; the chain is unbroken through the restart
  (global check). Any of these missing: NOT_RUN; present and wrong: FAIL.
- CTRL-E1: PROVISIONAL field names (the release lane L5 may rename them before RUN-3, then this file gets a v2):
  `install_refused` true and `refusal_reason` naming `checksum` or `signature`. No facts: NOT_RUN.
- CTRL-E2: PROVISIONAL (lane L6): `refused_before_execution` true and non-empty `refusal_name`.
- CTRL-E3 = CTRL-E3a and CTRL-E3b both PASS (FAIL if either FAIL, else NOT_RUN). CTRL-E3a: `exit` non-zero and the
  `log_line` names the desk (`NoDesk`). CTRL-E3b: `refusal` `Revoked` and `file_absent` `yes`; no such facts: NOT_RUN.
- CTRL-E4: PASS needs `effect_state` not DONE, `report_changed` `no`, a named refusal (an `E_...` code or a refusal
  name in `artifacts/CTRL-E4-propose.json` `error`, else in the receipt `evidence`), AND the E4 row itself showing a
  recall (`recall_after_restart` true). If E4 did not recall, the control tests store-loss only, not memory-loss, so the
  row is NOT_RUN with that reason (RESULT-RUN-1-dry-2.md point 4 records the same limit).
- CTRL-E5 (PROVISIONAL, lane L4 harness): `mutated_output_rejected` true and `verifier_reason` naming `DIGEST`.
- CTRL-E6 (PROVISIONAL, lane L2): non-empty `refusal_name` and `effects_after_restart` 0.
- NET: `daemon_established_tcp` 0, else FAIL. PRE: weights and tokenizer digests equal the candidate digests, backend line
  and host present.
- Verdict identity exactly as the contract and the task: sha256 of `AIEN_E2E_VERDICT_V1`, LF, then `objective_id`,
  `contract_sha256`, `E1`..`E6`, `CTRL-E1`..`CTRL-E6` lines joined by LF, no trailing LF.
- Exit code 0 only when the chain is unbroken and no row (supplementary rows included) is FAIL.

## Expected rows for the two fixtures (derived from the receipts, not from `VERDICT_ROWS.txt`)

Fixture A `RUN-1-dry-20261010T133749Z`, fixture B `RUN-1-dry-20261010T134202Z`. Both: chain unbroken (16 receipts, links
hold, manifest digests match, 4 and 6 absent `*.log` entries noted), exit code non-zero because of FAIL rows.

| row | A | B | note |
|---|---|---|---|
| E1 | NOT_RUN | NOT_RUN | `signature_check` none, no artifact |
| E2 | NOT_RUN | NOT_RUN | harness-side record, `operator_surface` not `aien-cli` |
| E3 | FAIL | FAIL | A: refusal before the model, `effect_state` none. B: no effect, none written |
| E4 | NOT_RUN | FAIL | A: no facts. B: `effect_state` none, no recall recorded |
| E5 | NOT_RUN | NOT_RUN | no peer verdict id |
| E6 | PASS | PASS | |
| CTRL-E1, CTRL-E2, CTRL-E5, CTRL-E6 | NOT_RUN | NOT_RUN | no facts |
| CTRL-E3 | NOT_RUN | NOT_RUN | E3a PASS, E3b has no refusal fact |
| CTRL-E4 | NOT_RUN | NOT_RUN | B differs from the harness (PASS): E4 recalled nothing, see CTRL-E4 rule |
| supplementary PRE / E3m / CTRL-E3a / NET | PASS / FAIL / PASS / PASS | PASS / PASS / PASS / PASS | |

Where the verifier differs from the harness `VERDICT_ROWS.txt`: A has no difference in a verdict row except the
aggregate row CTRL-E3 (the harness lists CTRL-E3a PASS and CTRL-E3b FAIL; the verifier reports NOT_RUN for CTRL-E3b because
the receipt has no refusal fact, so CTRL-E3 is NOT_RUN, not FAIL; the contract says a row without its facts is never
PASS and says nothing that makes a missing fact a FAIL). B differs in CTRL-E4 (harness PASS, verifier NOT_RUN) for the
reason in the CTRL-E4 rule, and in CTRL-E3b likewise. The `verdict_id` of each fixture is printed below after the first
run of the tests (it depends on the contract digest passed; the tests use the 3c584a49 digest above).

## Mutation tests (on copies in a scratch folder, never on the fixtures)

Each must be detected with a named reason: M1 one byte changed in `artifacts/E3m-report.md` (fixture B):
`MANIFEST_DIGEST` and E3m `DIGEST_MISMATCH`; M2 one chain link broken (a `prev_receipt_sha256` edited): `LINK_BROKEN`;
M3 one file dropped from `SHA256SUMS`: `MANIFEST_UNLISTED`; M4 receipt status edited from FAIL to PASS without the facts
with links and manifest repaired: the row stays FAIL and `status_disagreement` names it; also the unrepaired form is
caught by `LINK_BROKEN`. In every case the exit code is non-zero.

## Measured on the first run of the tests

Contract digest passed: `8ba653848287bc77db253466f6d3ff37c70a42be55601ad47eb1f838a41d20f0`. Rows came out exactly as
registered above, with `chain_unbroken` true and exit code 1 for both fixtures.

| fixture | verdict_id |
|---|---|
| A `RUN-1-dry-20261010T133749Z` | `728ae9152ef546ab4ccf1b7619fd22d21acb2914870197ac58e8843f9afaaf77` |
| B `RUN-1-dry-20261010T134202Z` | `d7d57162ac318ec05f2d97ad9530d9d3a571275409b040f65ccd0e391f6c9615` |

Test file also covers: unknown-peer E5 FAIL, matching-peer E5 PASS (changes the final id), `--write` on a copy, a wrong
`--objective-id`, an unlisted extra file (`MANIFEST_UNLISTED`) and a dropped receipt (`SEQUENCE_GAP`), and that the
fixture tree hash is unchanged after the tests.

## Limits

The E1, E2, E4 and CTRL-E1, CTRL-E2, CTRL-E5, CTRL-E6 PASS paths and the E3 contract-objective PASS path are implemented from
the contract text but have no fixture that reaches them yet; they are exercised only as NOT_RUN or FAIL here. The
CTRL-E1, CTRL-E2, CTRL-E5 and CTRL-E6 field names are provisional until lanes L5, L6, L4 and L2 pre-register theirs. The
second-machine agreement (E5) is a procedure (each machine passes the other's base id) that this folder has not yet been run
through on a MacBook. Run folders are not signed: an adversary who rewrites receipts, links and manifest consistently
defeats the chain check; the verifier still re-derives every row from facts and files, which is what this lane promises.
