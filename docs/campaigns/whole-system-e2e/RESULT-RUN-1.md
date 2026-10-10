# WHOLE-SYSTEM-E2E, RUN-1 (declared, CPU): result

Rows: `ACCEPTANCE-v1.2.md` (committed before the harness ran). Contract: `docs/plans/release-readiness/ACCEPTANCE_E2E.md`
in aien-architecture, frozen at b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8` (the
digest in every receipt and in `VERDICT_ROWS.txt`). Harness: `scripts/whole_system_e2e.sh` v1.2 at b70e1d4 (release build
of this tree, nothing installed). Run folder: `runs/RUN-1-20261010T154724Z/` (17 chain receipts linked by
`prev_receipt_sha256`, `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, artifacts, daemon logs, the acceptance files as run, and
the verifier's `VERIFIER_VERDICT.txt` and `VERIFIER_RECEIPT.json` written after the manifest closed). Host: the Spark
(`spark-b87b`), Linux, CPU-reference backend, CAND-4 model (Llama 3.2 1B Instruct; weights and tokenizer digests equal
`release/candidate.toml`). 2026-10-10 15:47:24 to 16:07:59 UTC. Milestone: R5 of aien-architecture#190 (declared run on
CPU). **No PASS claim for the program and no claim that AIEN is complete.** The run folder is kept exactly as written;
`sha256sum -c chain/SHA256SUMS` reports 0 bad lines.

## Rows as measured (OBSERVED from the chain)

| row | status | what the receipt says |
|---|---|---|
| PRE | PASS | backend line CPU-reference; model digests equal the candidate record; quiet flag not held |
| CTRL-E3a | PASS | desk required and no key: the daemon exited 1 before serving; log names `NoDesk` |
| E1 | NOT_RUN | nothing installed in RUN-1; RUN-3 (R8, signed installed release) measures E1 |
| E2 | PASS | objective recorded through `aien-cli` (exit 0, id equals the harness id, record on disk) before any effect receipt |
| CTRL-E2 | PASS | objective declared `EXTERNAL_IRREVERSIBLE`: exit 1, refusal `ForbiddenEffectClass`, no record written |
| E3 | PASS | state DONE; `outbox/report.md` written; the proposer read 3 inputs; 146 words; the three names present; content digest equals disk digest; second execute refused `AlreadySpent` |
| E3m | PASS | state DONE; `report.md` written; 187 words; names present; second execute refused `AlreadySpent` |
| E4 | PASS | `remember` ok; SIGKILL (exit 137); restart (`Reconcile: checked 0 unsettled`); second objective DONE; memory recalled (`work/1`); one line appended naming "harbour"; lines 7 to 8; no problems |
| E6 | PASS | authorize, SIGKILL, execute while dead: no write; restart; same grant DONE once; third execute `AlreadySpent`; duplicates 0 |
| CTRL-E3b | PASS | grant revoked before spend: execute refused `Revoked`; `ctrl.md` absent |
| NET | PASS | 0 established TCP sockets of the daemon |
| CTRL-E4b | RECORDED | memory files removed, home intact: propose did not commit because the model proposal exceeded the 161808 ms budget; `report.md` unchanged. Not judged (see limits) |
| CTRL-E6 | PASS | SIGKILL after authorize, compose home removed (desk key and record mark kept): same grant refused `E_MARK_TRUNCATED`; `state.md` absent; no second effect |
| CTRL-E4 | PASS | compose store removed between stop and start: propose refused with a named reason (`E_MARK_TRUNCATED`: journal holds 0 records, record mark says 128); `report.md` unchanged |
| E5, CTRL-E1, CTRL-E5 | NOT_RUN | E5 and CTRL-E5 are the second-machine verdict (R7); CTRL-E1 needs a release artifact (R8) |

Independent verifier on this machine (`tools/aien-verify`, built from this tree, sha256
`d1e6dbf8f40781845dff4f848f9cee9b5b65517a0375731c9ef9225a00bfdae0`): chain unbroken, 0 chain problems, 0 status
disagreements with the harness rows, `objective_id` `093e66ec92a6b428b49a0aa2a75678e9fdb155f423e1e7c71c5ae9fef11a60b1`,
`verdict_id` `a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80` (also its `base_verdict_id`, because no
peer verdict existed yet). E5 stays NOT_RUN in the verifier because no `--peer-verdict-id` was given; the Mac verdict
(R7) will be computed over this same run folder and must reproduce the `verdict_id`.

Compared with dry run 2 (`RESULT-RUN-1-dry-2.md`): E3, E4 and CTRL-E3b moved from FAIL to PASS after sc#388 to sc#394
(read files, destination interpretation, memory engagement, append budget, append placement) and CTRL-E6 ran for the
first time. Every row that the CPU host can judge is PASS.

## What follows (INFERRED, by the stated rules)

1. On this host and model the contract's E2, E3, E4 and E6 steps and their controls behave as the frozen contract
   requires, in one declared attempt, with the originals in the chain. This is spec conformance on this configuration,
   nothing more.
2. The memory path (E4) works in the harness configuration because the harness engages ALLEN (sc#386); CTRL-E4 shows
   store loss is detected with a named refusal and no silent output.
3. The second-machine verdict (R7) is now possible: the run folder, the contract digest and the verifier source are
   all committed; the Mac rebuilds the verifier from source and must produce the same `verdict_id`.

## Not claimed

Nothing about an installed artifact (E1, CTRL-E1), cross-machine agreement (E5, CTRL-E5), the public model, the
accelerator path (R6), native AIENOS, a hostile operator, the quality of the model's prose beyond the checks, or the
whole-system qualification as complete.

## Limits

- One host, one model, one attempt (rule 7). CPU-reference backend; the 1B model took 2 minutes for E3 and 2.5 for E4.
- CTRL-E4b (memory files removed, home intact) is RECORDED, not judged: the propose step exceeded its time budget. The
  Spark was shared with another session's `cargo test` runs during this window (CPU contention), which is the likely
  cause but is not proven. The row is supplementary (not in the frozen contract) and does not change any contract row.
- The verifier on this host was built from the same tree it verifies. Independence comes from R7.
- The daemon's compose home for this run lives under the job's temporary folder; artifacts copied into the run folder
  are the evidence, the temporary folder is not.

## Next

- R7: `ssh mac` runs `r7-mac.sh` over this run folder at the merged commit with this verdict id as the peer base; then
  the Spark verifier reruns with the Mac's base id as `--peer-verdict-id` and E5 is judged.
- R6 (GB10) waits on the accelerator backend lane; R8 waits on the key ceremony (sc#385) and the qualified public model
  (sc#384). Both are Drake-controlled.
