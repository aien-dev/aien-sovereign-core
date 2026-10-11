# WHOLE-SYSTEM-E2E held-fault rows: addendum v1.2 (control state across a restart)

Tracking: aien-dev/aien-sovereign-core#408, aien-architecture#190 (Priority D, Lane E recovery audit).
Amends `FAULTS-v1.md` and `FAULTS-v1.1.md` by ADDING three rows; no existing row or rule changes, and the recorded
runs (`FAULTS-20261010T143418Z`, `FAULTS-20261010T144049Z`) stay as they are. Committed before the harness change.
Every status below is NOT_RUN at the time of writing.

## Why

`RECOVERY-AUDIT-2026-10-11.md` found that the held-kill rows prove the effect path (one effect per grant) but nothing in
the campaign shows that a human control survives a SIGKILL restart: a revoked grant stays revoked, the kill switch stays
on, and the approval desk still demands its proof. CTRL-E3b (revoke before spend) ran inside one daemon lifetime.

## Rows (fixture, build and counting rules as `FAULTS-v1.md` sections 2 to 5)

| row | injection | PASS iff | prediction |
|---|---|---|---|
| E6-revoke_durable | grant minted; `compose revoke --authorization G --approver drake` returns ok; SIGKILL the daemon (137); restart; execute G | execute refused with the name `Revoked`; `done.md` absent; no intent for G in the ledger; `ledger.revoked` lists G | PASS (the ledger holds `revoked`; CTRL-E3b showed the refusal in one lifetime) |
| E6-stop_durable | grant minted; `compose stop --approver drake` ok; SIGKILL the daemon; restart; execute G; then `compose resume --approver drake`; execute G | the first execute is refused with a name (recorded) and `done.md` is absent; after `resume` the same grant finishes `DONE` once; duplicates 0 | UNKNOWN for the refusal name (expected `Stopped`); PASS otherwise |
| E6-desk_after_restart | grant minted; SIGKILL the daemon; restart; `compose authorize` WITHOUT `--desk`; then `compose authorize --desk 1` | the authorize without the desk proof returns `ok:false` and mints no grant; the one with `--desk 1` returns ok and a new grant id; the first grant (G) still finishes `DONE` once when executed | PASS (desk key is in the home, kept; the proof is checked per authorize) |

Rules as v1 section 5: one attempt per row; a FAIL is recorded, not fixed silently; an engine bug becomes an issue and
the row stays FAIL; no engine change in this change. A PASS shows these controls hold across a SIGKILL restart of the
CPU-reference build with `fault-hold`; it does not show power loss, a hostile operator, the accelerator path, or any
contract step as PASS.
