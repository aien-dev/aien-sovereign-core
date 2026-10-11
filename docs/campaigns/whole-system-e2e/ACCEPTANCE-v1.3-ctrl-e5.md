# WHOLE-SYSTEM-E2E, addendum v1.3-ctrl-e5: the verifier judges CTRL-E5 itself

Tracking: aien-dev/aien-sovereign-core#403, aien-dev/aien-architecture#190 (L4). Contract `ACCEPTANCE_E2E.md` stays FROZEN
(aien-architecture b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`). `ACCEPTANCE-v1.md`,
`-v1.1.md`, `-v1.2.md` and every recorded run stay as written. This file is committed before the code that implements it.

## Why

RUN-1 and R7 judged CTRL-E5 by hand (`RESULT-RUN-1-R7.md`, `r7/`): one byte of `artifacts/E3-report.md` changed on a scratch
copy gave E3 FAIL DIGEST_MISMATCH, chain_unbroken false and a different verdict_id, on both machines. The harness wrote no
CTRL-E5 receipt, so the verifier's own CTRL-E5 row stayed NOT_RUN. This addendum puts the same check inside the chain.

## Rule (harness tail, after the verifier)

1. The chain is closed as before (receipts 001..N, console transcript, `VERDICT_ROWS.txt`, `chain/SHA256SUMS`). The harness no
   longer writes a `CTRL-E5 NOT_RUN` placeholder receipt.
2. The verifier runs over the real folder with `--write` (no peer). Its verdict is the PRE-CTRL-E5 verdict: CTRL-E5 NOT_RUN
   (receipt absent), E5 NOT_RUN. Its `verdict_id` is `original_verdict_id`. The harness copies that file to
   `artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`.
3. The harness copies the run folder to scratch, changes one byte of `artifacts/E3-report.md` (offset 3; the byte becomes `X`,
   or `Y` if it already was `X`), and runs the verifier on the copy (no `--write`). Its output is kept as
   `artifacts/CTRL-E5-mutated-verdict.txt`.
4. The harness appends one receipt, step `CTRL-E5`, to the chain (linked to the previous receipt as every receipt is), with
   the fields: `mutated_file`, `mutation` {`offset`, `before_byte_hex`, `after_byte_hex`}, `original_verdict_id`,
   `mutated_verdict_id`, `mutated_chain_unbroken`, `mutated_e3_row` (the verifier's E3 line on the copy),
   `mutated_output_rejected` (true iff the copy's E3 row is FAIL with `DIGEST_MISMATCH` and `chain_unbroken` is false),
   `verifier_reason`, `verifier_sha256`, `verifier_source_sha256`. If `E3-report.md` is absent or shorter than 4 bytes the
   receipt is `NOT_RUN` with `mutated_output_rejected` omitted.
5. `VERDICT_ROWS.txt` is rewritten to include the CTRL-E5 row, `chain/SHA256SUMS` is rewritten (it now lists the new receipt,
   the two new artifacts, the longer console transcript and the new `VERDICT_ROWS.txt`), and the verifier runs again over the
   real folder with `--write`. That FINAL `VERIFIER_VERDICT.txt` is the run's verdict.

## What the verifier judges (CTRL-E5)

From the LAST `CTRL-E5` receipt in the chain, facts only (the status word is never trusted):
- absent, or lacking any of `mutated_output_rejected`, `mutated_chain_unbroken`, `original_verdict_id`, `mutated_verdict_id`,
  `verifier_reason`: **NOT_RUN**;
- **PASS** iff `mutated_output_rejected` true, `mutated_chain_unbroken` false, `verifier_reason` names `DIGEST_MISMATCH`,
  `mutated_verdict_id` differs from `original_verdict_id`, and `original_verdict_id` equals the id the verifier itself
  computes from this folder with CTRL-E5 and E5 forced to NOT_RUN (so a receipt cannot cite a verdict it never saw);
- otherwise **FAIL**, with the reason naming which fact failed.

## Verdict ids

`verdict_id` hashes the rows including CTRL-E5. So the final `verdict_id` differs from the pre-CTRL-E5 one by exactly the
CTRL-E5 row (NOT_RUN to PASS). Both are recorded: the pre id inside the CTRL-E5 receipt and `artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`,
the final one in `VERIFIER_VERDICT.txt`. `base_verdict_id` (E5 forced NOT_RUN, the id peers exchange) now includes the CTRL-E5 row,
so a peer machine verifying a v1.3 folder agrees with the harness machine on it.
Folders without a facts-bearing CTRL-E5 receipt (every run before v1.3) verify exactly as before: RUN-1-20261010T154724Z
gives `verdict_id` 437467db4149b013738578c10ef68b7168dfb95feae8dea882de17a6118fc5fd with the Spark base id as peer, and
base `a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80`.

## Predictions

| row | prediction |
|---|---|
| CTRL-E5 on a new run | PASS (the same check passed by hand on both machines) |
| verifier tests | present+PASS, present+FAIL (rejected false), absent NOT_RUN, wrong original id FAIL, all green |
| RUN-1 regression | ids unchanged |
