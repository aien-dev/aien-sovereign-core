# WHOLE-SYSTEM-E2E, held-fault rows (lane L2): pre-registered

Tracking: aien-dev/aien-sovereign-core#379, lane L2 of aien-dev/aien-architecture#190.
Contract step: aien-architecture `docs/plans/release-readiness/ACCEPTANCE_E2E.md`, steps E6 and CTRL-E6.
Harness: `scripts/whole_system_e2e_faults.sh` (shell plus `jq`, no Python). Committed before the harness.
Every status below is NOT_RUN at the time of writing. The harness measures; it never edits this file.

## 1. Why this exists

`scripts/whole_system_e2e.sh` row E6 kills the daemon between authorize and execute (no hold). The three
points where the engine holds mid-execute (`before_intent`, `after_intent`, `after_write`, in
`crates/aien-cli/src/compose.rs` `fault_hold`) and the two start-up holds (`reconcile_panic`,
`reconcile_error`, in `crates/aien-runtime/src/effects.rs` `reconcile_at_start`) exist only in a build with the
cargo feature `fault-hold` (test builds only). The earlier campaign next-phase-2 (`ACCEPTANCE-v2.md` section 5,
rows C2a, C2b, C2c, C2d, and `ACCEPTANCE-v4.md` rows C7a, C7c) drove them with a different fixture. These rows
drive the same points on the E2E fixture (desk required, desk key created, CPU-reference backend, CAND-4 model)
and write receipts in the E2E chain format.

The hold is in the `aien compose execute` process (the executor), not in the daemon. "Kill" below means:
SIGKILL the daemon (exit 137), then SIGKILL the held executor (exit 137), i.e. the whole host dies at that point,
as C2c does; for `before_intent` the hold is released after the daemon is dead, as C2a does.

## 2. Fixture and build

One build: `cargo build --release -p aien-cli --features aien-cli/fault-hold` into its own target dir, compose
archive linked (a stub build is refused), no accelerator archive. The daemon must print a `CPU-reference`
backend line. One real `compose propose` of the objective "Write a file named done.md in the outbox folder
containing one line: finished." (the E6 objective of the E2E harness) makes the fixture; the fixture (compose
home, sibling `.machine-root` and `.cortex-mark`, run home with the desk key, workspace, proposal report) is
byte-copied into each row, so each row has its own state, its own socket and its own daemon. A grant is minted
per row by `compose authorize --desk 1`.

Counting rules. `effects` = number of `done.md` files in the row workspace. `duplicates` = 1 if a second effect
happened (more than one file, or the file's inode, size or mtime changed after the first effect), else 0.
`effects_before_kill` is counted at the kill, `effects_after_restart` at the end of the row.

## 3. Rows and predictions

| row | injection | PASS iff | prediction |
|---|---|---|---|
| E6-before_intent | hold executor at `before_intent`; SIGKILL daemon (137); release hold | executor refuses (`ok:false`) and writes nothing; no intent in the ledger; restart; the same grant executes to `DONE`; `done.md` once; next execute `AlreadySpent`; `duplicates` 0 | PASS (as C2a) |
| E6-after_intent | hold at `after_intent`; SIGKILL daemon then executor; restart | no write before restart; the result is one of the named outcomes and is recorded: (a) `ReconciliationRequired`, then `compose reconcile`, then exactly one effect; (b) `AlreadySpent` with `done.md` present once; (c) `AlreadySpent` with `done.md` absent (reconcile recorded `NOT_DONE`), then a new grant executes exactly once. Never two effects | PASS via outcome (c): restart `Reconcile:` line says `NOT_DONE 1`, same grant `AlreadySpent`, file absent, a new grant finishes once (as C2c). The brief's wording (a) or (b) would show if the engine differs; the row records which |
| E6-after_write | hold at `after_write`; SIGKILL daemon then executor; restart | `done.md` exists once before restart; restart reconcile records `DONE`; the same grant `AlreadySpent`; file stat unchanged; ledger shows the grant spent once; `duplicates` 0 | PASS (as C2d) |
| CTRL-E6 | as E6-after_write, then replace the compose home by an empty directory (the journal `cortex.cx` and everything the daemon reconciles from are gone; the sibling record mark stays), restart, execute the same grant | the daemon refuses to start with a name, or the execute is refused with a name (`ReconciliationRequired`, `E_MARK_TRUNCATED`, `ReconcileFailed` or another refusal name); `done.md` stat unchanged; state not `DONE`; never a silent success | PASS with `E_MARK_TRUNCATED` (as CTRL-E4 of dry run 2 measured on the same code) |
| CTRL-E6b | as CTRL-E6, and the sibling record mark `<home>.cortex-mark` is removed too (total state loss, worst case) | the execute is refused with a name, or the daemon refuses to start; `done.md` stat unchanged; not `DONE` | UNKNOWN. If the home opens fresh, the grant itself is gone, so a refusal naming an unknown or stale grant is expected; recorded either way, name included |
| FAULT-reconcile_error | grant minted, daemon stopped gracefully, daemon restarted with `AIEN_FAULT_HOLD=reconcile_error` | start line `Reconcile: refused: fault hold reconcile_error`; execute refused `ReconcileFailed`; no write; no intent; grant unspent: after `compose reconcile` (ok) the same grant executes once to `DONE`; next execute `AlreadySpent` | PASS (as C7a) |
| FAULT-reconcile_panic | same, restarted with `AIEN_FAULT_HOLD=reconcile_panic` | daemon exits non-zero by itself, log names `fault hold reconcile_panic`; nothing serves the socket (an execute fails to connect, recorded as the refusal); no write; restart without the hold; the grant is unspent: same grant executes once to `DONE` | PASS (as C7c) |

## 4. Receipts

`chain/NNN-step.json` for `PRE`, then each row, with the chain fields of the E2E harness (`objective_id`,
`run_id`, `step`, `status`, `utc`, `prev_receipt_sha256`, `evidence`, `repo_commit`, `daemon_sha256`,
`cli_sha256`, `model_weights_digest`, `tokenizer_sha256`, `backend_line`, `host`) plus, on each row:
`kill_point`, `restart_count`, `effects_before_kill`, `effects_after_restart`, `duplicates`, `grant_id`, and
the refusal names seen. `chain/SHA256SUMS` covers every file of the run folder. The `objective_id` is
`sha256("AIEN_E2E_FAULTS_OBJECTIVE_V1\n" + objective text)`, a different identity from the E2E run.

## 5. Rules

One attempt per row (the fixture proposal may be retried once if the model fails to commit). A FAIL row is
recorded, not fixed silently. An engine bug found here becomes an issue with the receipt; the row stays FAIL.
No engine change in this change. The harness kills only the daemon and executor processes it started itself.
What a PASS shows: the engine's crash boundaries on the CPU-reference build with fault-hold. What it does not
show: the accelerator path, an installed artifact, the release build (the hold points are compiled out of it),
or any step of the contract as PASS.
