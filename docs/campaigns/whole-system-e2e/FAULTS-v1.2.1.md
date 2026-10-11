# WHOLE-SYSTEM-E2E held-fault rows: amendment v1.2.1 (two prediction defects in v1.2)

Tracking: aien-dev/aien-sovereign-core#408. Amends `FAULTS-v1.2.md`. Written after the first run of the v1.2 rows
(`runs/FAULTS-20261011T011317Z/`, kept as it fell) and before the harness change. In that run E6-revoke_durable PASSED
as predicted, and the other two v1.2 rows scored FAIL against predictions that were wrong about engine design, not
because a control failed:

1. **E6-stop_durable.** v1.2 predicted that after `compose resume` the pre-stop grant finishes `DONE`. The engine answers
   `EFFECT_REFUSED Stale: authorization #23 predates operator stop #24` (a stop invalidates every earlier grant by
   design). Observed in run 1 and now the pre-registered rule: PASS iff after the SIGKILL restart the ledger shows the
   stop, execute is refused `Stopped`, `done.md` is absent; and after `resume` the same grant is refused `Stale` and
   `done.md` is still absent (a pre-stop grant never revives). No claim is made that a fresh grant after resume works.
2. **E6-desk_after_restart.** v1.2 predicted a second `authorize --desk 1` after the restart mints a new grant. The engine
   answers `EFFECT_REFUSED AlreadyAuthorized: grant #23 for this proposal is still live; revoke it first`, which is itself
   a durable-state fact (the live grant survived the kill). The without-desk authorize was refused as predicted
   (`DeskMacRequired`). Pre-registered rule now: PASS iff the authorize without the desk proof is refused
   `DeskMacRequired`, the authorize with `--desk 1` is refused `AlreadyAuthorized` naming the surviving grant, and the
   original grant still finishes `DONE` once (next execute `AlreadySpent`). That a desk proof is accepted after a restart
   is shown by row E6-after_intent (the second approval after the restart mints a grant), not by this row.

Because these rules were written after seeing run 1's observations, the rerun only confirms that the behaviour is
repeatable; it is weaker evidence than a prediction made blind, and the report says so. Run 1 stays on record with its
two FAIL scores.
