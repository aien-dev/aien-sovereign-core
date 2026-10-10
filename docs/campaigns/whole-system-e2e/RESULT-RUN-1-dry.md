# WHOLE-SYSTEM-E2E, RUN-1 dry run 1: result

Rows: `ACCEPTANCE-v1.md` (committed b53a5e7 before the harness). Harness: `scripts/whole_system_e2e.sh` at
3885600. Run folder: `runs/RUN-1-dry-20261010T133749Z/` (17 chain receipts, `chain/SHA256SUMS`,
`VERDICT_ROWS.txt`, artifacts, daemon logs). Host: the Spark, Linux, CPU-reference backend, CAND-4 model
(Llama 3.2 1B, weights and tokenizer digests equal `release/candidate.toml`). 2026-10-10 13:37:49 to 13:38:56 UTC.
Milestone: R4 of aien-architecture#190 (harness merged, first dry run recorded). **No PASS claim for the
program; no step of the contract is claimed.**

## Rows as measured (OBSERVED from the chain)

| row | status | what the receipt says |
|---|---|---|
| PRE | PASS | backend line CPU-reference; model digests equal the candidate record; quiet flag not held |
| CTRL-E3a | PASS | with the desk required and no key, the daemon exited 1 before serving; log names the unusable desk key (`NoDesk`) |
| E1 | NOT_RUN | nothing installed in RUN-1 |
| E2 | NOT_RUN | harness-side objective receipt is the chain's fourth file; no installation-side record before work (lane L6, sc#380) |
| E3 | FAIL | propose refused before the model ran: `uncertain requirement ... "under 200 words named report.md in the outbox folder"` |
| E3m | FAIL | propose refused before the model ran: `ambiguous destination: the goal names several files to write (report.md, harbour-notes.txt, orchard-ledger.txt, windmill-log.txt)` |
| E4 | NOT_RUN | E3m produced no report to append to |
| E6 | PASS | authorize, SIGKILL the daemon (exit 137), execute while dead: no write; restart (`Reconcile: checked 0 unsettled`); same grant executes to DONE once; third execute refused `AlreadySpent`; inode and mtime unchanged (duplicates 0). The file landed at the workspace root, not in `outbox/` (recorded, not judged in v1) |
| CTRL-E3b | FAIL | propose refused: `the proposal writes outbox/ctrl.md but the goal named the destination ctrl.md`; the revoke step was never reached |
| NET | PASS | 0 established TCP sockets of the daemon |
| CTRL-E4 | NOT_RUN | E3m did not pass |
| E5, CTRL-E1, CTRL-E2, CTRL-E5, CTRL-E6 | NOT_RUN | lanes L4, L5, L6, L4, L2 |

## What follows (INFERRED, by the stated rules)

1. The chain mechanics work: 17 receipts, each `prev_receipt_sha256` equal to the sha256 of the previous file,
   `objective_id` and `run_id` in every receipt, manifest over the run folder, verdict rows written for the verifier.
2. The interrupt shape of E6 holds on this build for the no-hold variant: an authorized but unexecuted effect
   survives a daemon SIGKILL and is spent exactly once after restart. The held variants are lane L2 (sc#379).
3. Three goal-interpretation limits block E3 before any model work (sc#383): requirement phrasing, input file
   names taken as write targets, folder destination inconsistent. The pre-registered prediction for E3 (FAIL on
   the file-name check because the proposer cannot read files, sc#382) was not reached; the refusal came earlier.
4. E4 and CTRL-E4 remain unmeasured; `ACCEPTANCE-v1.1.md` amends E3m and E4 so a second declared run can reach them.

## Not claimed

Nothing about an installed artifact, a public model, the accelerator path, native AIENOS, a hostile operator, the
model's prose, or any step of the contract as PASS. This run is the first measurement, kept as it fell.

## Limits

One host, one model, one attempt (rule 7). The CTRL-E3b failure means the `Revoked` refusal was not exercised
here (NEXT-PHASE-2 covers it on its own fixture). E6 did not check the written file's folder.
