# WHOLE-SYSTEM-E2E held-fault rows: result of run 2 (the declared run)

Rows: `FAULTS-v1.md` as amended by `FAULTS-v1.1.md`. Harness: `scripts/whole_system_e2e_faults.sh`, commit
at the commit recorded in every receipt as `repo_commit`. Run folder:
`runs/FAULTS-20261010T144049Z/` (8 chain receipts, `chain/SHA256SUMS`, `VERDICT_ROWS.txt`, per-row artifacts and
daemon logs, both pre-registration files as run). Run 1, `runs/FAULTS-20261010T143418Z/`, is kept as it fell (it
stopped being the declared run because of two harness defects named in `FAULTS-v1.1.md`; its first row scored FAIL,
its two control rows passed for the wrong reason). Host: the Spark, Linux, CPU-reference backend, CAND-4 model
(Llama 3.2 1B, weights and tokenizer digests equal `release/candidate.toml`), a release build with the cargo
feature `fault-hold`, 2026-10-10 14:40 to 14:48 UTC. **No PASS claim for the program.**

## Rows (OBSERVED from the receipts)

| row | status | kill point | files before kill / after restart | duplicates | what happened |
|---|---|---|---|---|---|
| E6-before_intent | PASS | before_intent | 0 / 1 | 0 | daemon SIGKILL (137) while the executor held; released executor exited 1, no file, no intent; restart; the same grant finished `DONE` once; third execute `AlreadySpent` |
| E6-after_intent | PASS | after_intent | 0 / 1 | 0 | daemon and executor SIGKILL (137, 137); restart reconcile line `NOT_DONE 1`; the same grant `AlreadySpent`, file absent (outcome c of the pre-registration, as predicted); a new grant finished once |
| E6-after_write | PASS | after_write | 1 / 1 | 0 | both SIGKILL; file present once before restart; restart reconcile `DONE 1`; the same grant `AlreadySpent`; ledger one intent, `DONE`; file stat unchanged |
| CTRL-E6 | PASS | after_write | 1 / 1 | 0 | journal, jspace and machine.id removed, desk key kept, record mark kept; daemon started (`Reconcile: no compose home yet`); execute of the same grant refused `E_MARK_TRUNCATED` ("journal holds 0 records, its record mark says ..."); file untouched |
| CTRL-E6b | PASS | after_write | 1 / 1 | 0 | as CTRL-E6 and the record mark removed too; daemon started; execute refused `NotAuthorized` ("not a verified authorization"); file untouched |
| FAULT-reconcile_error | PASS | reconcile_error | 0 / 1 | 0 | start line `Reconcile: refused: fault hold reconcile_error ...`; execute refused `ReconcileFailed`, no file, no intent; `compose reconcile` ok; the same grant then `DONE` once; next `AlreadySpent` |
| FAULT-reconcile_panic | PASS | reconcile_panic | 0 / 1 | 0 | daemon aborted by itself (rc 134), log names the hold; with nothing serving, execute failed to connect; clean restart; the same grant `DONE` once |

PRE passed (CPU-reference backend line, digests equal the candidate record, fixture proposal committed).

## INFERRED

1. The crash boundaries of the compose effect path hold on the E2E fixture (desk required, real model proposal):
   one effect per grant at all three execute holds and both start-up holds, matching the next-phase-2 rows C2a,
   C2c, C2d, C7a and C7c. This is the CTRL-E6 half of contract step E6 (state removed gives a named refusal) and
   the held-kill half of E6.
2. Losing the objective state is detected by name only while the record mark survives (`E_MARK_TRUNCATED`). If the
   mark is removed as well, the home opens fresh without a loss report at start (`no compose home yet`), and
   safety rests on the grant being gone (`NotAuthorized`). No second effect either way, but the second case is a
   weaker guard than the first; whether total loss of both should be reported at start is for lane L2/L3 owners.
3. The after_intent outcome is "AlreadySpent, file absent, new grant needed". The objective does not finish on
   the old grant; the harness finishes it with a second desk approval (two approvals counted). The contract's
   wording "finish the same objective" is met by the second grant, not the first.

## NOT CLAIMED

Nothing about the accelerator path, an installed artifact, the release build (the hold points are compiled out of
it), the power-loss or kernel-crash case (SIGKILL only), a hostile operator, the model's prose, or any step of the
contract as PASS. The holds are in the executor (`aien compose execute`); "kill" is SIGKILL of the daemon and the
executor, not of a single process in isolation (that is next-phase-2 C2b/C2d).

## Limits and notes

One host, one model, one attempt per row in the declared run (the fixture proposal is made once and byte-copied
into each row; the proposal binds the workspace path, so every row uses the same workspace path, re-seeded per
row). Evidence text defect: the released executor's `ok:false` answer is recorded as `ok=none` in the
E6-before_intent receipt because the harness read it with jq `.ok // "none"` (jq treats false as absent); the row
verdict used the exit status and the absence of a file and intent, and `artifacts/E6-before_intent/held.json`
shows `"ok":false`. The read was corrected after the run in a commit that changes only that line.
No engine change in this change; no engine bug found, so no issue opened.
