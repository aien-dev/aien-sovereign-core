# Recovery audit, 2026-10-11 (Lane E, Priority D of arch#190)

Scope: what the campaign evidences about interruption and recovery, read from the committed receipts, not from the
engine's design intent. Owning issue for new work: aien-spark/aien-sovereign-core#408. Evidence sources: `RESULT-FAULTS-1.md`
(`runs/FAULTS-20261010T144049Z`), the v1.2 re-run `runs/FAULTS-20261011T012434Z` (10 of 10 rows PASS; its first run
`FAULTS-20261011T011317Z` scored two rows FAIL on wrong predictions, see `FAULTS-v1.2.1.md`), RUN-1
`runs/RUN-1-20261010T154724Z` (E6, CTRL-E6, CTRL-E4), `RESULT-RUN-1-R7.md` and `ACCEPTANCE-v1.3-ctrl-e5.md` (sc#404).
All on one host (the Spark), CPU-reference backend, SIGKILL only.

| # | requirement | verdict | where / gap |
|---|---|---|---|
| 1 | interruption before intent | EVIDENCED | FAULTS `E6-before_intent`: SIGKILL daemon with the executor held, released executor exits 1, no file, no intent; restart; same grant `DONE` once; next `AlreadySpent` |
| 2 | interruption after intent | EVIDENCED | `E6-after_intent`: both killed; restart reconcile `NOT_DONE 1`; same grant `AlreadySpent`, file absent; a new desk approval finishes once. Note: the objective does not finish on the old grant (a second approval is needed) |
| 3 | interruption after the effect write | EVIDENCED | `E6-after_write`: file present once before restart, reconcile `DONE 1`, same grant `AlreadySpent`, file stat unchanged, one intent for the grant |
| 4 | durable objective identity across restart | PARTIAL | The objective record is a file written by `aien-cli objective record` outside the daemon home (`prov/objectives/<id>.json`); propose echoes `objective_id`. No row re-reads it after a restart, and the effect ledger, intents and execute answers carry no `objective_id` (grep of `crates/`: only `aien-cli` knows it). So identity across a kill is never shown to bind the effect. Filed on #408 |
| 5 | reconciliation | EVIDENCED | reconcile lines recorded at every restart (`NOT_DONE 1`, `DONE 1`, `checked 0`); `FAULT-reconcile_error` (refused, then manual `compose reconcile`, then `DONE` once) and `FAULT-reconcile_panic` (abort rc 134, clean restart, `DONE` once) |
| 6 | exactly-once effects | EVIDENCED | `duplicates 0` and a file count of 1 in every row; third execute `AlreadySpent` in RUN-1 E6 and the fault rows; ledger one intent per grant |
| 7 | approval (desk) behaviour after restart | EVIDENCED (this lane) | v1.2 rows, re-run: `E6-desk_after_restart` (authorize without the desk proof refused `DeskMacRequired`; with it refused `AlreadyAuthorized`, the live grant survived the kill; that grant finishes once), `E6-revoke_durable` (revoked grant refused `Revoked` after SIGKILL restart, ledger lists it), `E6-stop_durable` (operator stop survives the kill: `Stopped`; after resume the pre-stop grant stays `Stale`). A desk proof being accepted after a restart is shown by `E6-after_intent` (the new grant). Caveat: the v1.2.1 predicates were written after seeing run 1, so the re-run confirms repeatability; run 1 itself was the blind test and showed the same engine answers |
| 8 | receipt continuity across the kill | PARTIAL | The harness chain is hash-linked and unbroken across restarts (RUN-1 and FAULTS chains, verifier `chain_unbroken true`). The engine's own journal continuity is shown by the ledger/reconcile lines, not by a hash check across the kill: nothing asserts that the intent and state records of the grant keep their ids (24 then 30) across the restart beyond the ledger contents recorded. Not a failure, an unmeasured link |
| 9 | independent verification rejects tampered outputs | EVIDENCED (sc#404 merged at f467fce) | verifier tests M1 to M6 (one byte, broken link, dropped line, status edit with and without repair); R7 by hand on both machines (E3 `DIGEST_MISMATCH`, chain broken, verdict id changed); #404 puts the same mutation in the chain, from the next run on, as a CTRL-E5 receipt the verifier judges itself |
| 10 | loss of durable state is refused, not silent | EVIDENCED, with a weak case | RUN-1 CTRL-E6 and FAULTS CTRL-E6: `E_MARK_TRUNCATED`. FAULTS CTRL-E6b (record mark also removed): the home opens fresh, safety rests on the grant being gone (`NotAuthorized`); no start-time loss report (RESULT-FAULTS-1 INFERRED 2) |

## What this lane changed

- sc#404: CTRL-E5 receipt (row 9).
- This PR: `FAULTS-v1.2.md`, `FAULTS-v1.2.1.md`, three new fault rows, and the two run folders (run 1 kept as it fell).
- Filed on #408 for the larger gaps: row 4 (effects do not bind the objective id; no re-read after restart), row 8 note.

## NOT claimed

Power loss or kernel crash (SIGKILL only); a kill of the executor alone (next-phase-2 C2b/C2d are older evidence on a
different fixture); the accelerator path; a release build (hold points are compiled out); a hostile operator; a fresh
grant working after `resume` (not measured); more than one host or one attempt per row; any contract step as PASS.
