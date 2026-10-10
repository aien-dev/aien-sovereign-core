# WHOLE-SYSTEM-E2E held-fault rows: amendment v1.1

Amends `FAULTS-v1.md`. Written after the first run of `scripts/whole_system_e2e_faults.sh`
(`runs/FAULTS-20261010T143418Z/`, kept as it fell) and before the harness change. Two defects of the harness,
neither an engine behaviour:

1. **E6-before_intent, executor refusal.** v1 says "executor refuses (`ok:false`)". With the daemon dead the
   executor never gets a JSON answer: it exits 1 and writes the connection error to stderr, so `ok` is absent.
   The first run scored that FAIL on the missing `ok:false`. Amended predicate: the released executor exits
   non-zero, writes no file, and records no intent (stderr text kept as evidence). `ok:false`, when present, also
   counts. Nothing else in the row changes.
2. **CTRL-E6 and CTRL-E6b, what is removed.** The approval desk key lives inside the compose home
   (`<home>/approval-desk.key`). The first run replaced the whole home with an empty folder, so the second daemon
   refused to start because the desk key was missing (the CTRL-E3a refusal), not because the objective state was
   lost. Those two PASS results do not test the control. Amended injection: remove the journal and everything the
   daemon reconciles from (`cortex.cx`, `jspace/`, `machine.id`) and keep `approval-desk.key` in place, so the
   daemon can start and the loss is met at reconcile or at the first execute. CTRL-E6b additionally removes the
   sibling record mark `<home>.cortex-mark`. Predictions are as in v1: CTRL-E6 expects a named refusal
   (`E_MARK_TRUNCATED` predicted); CTRL-E6b is UNKNOWN.

The first run's rows E6-after_intent, E6-after_write, FAULT-reconcile_error and FAULT-reconcile_panic matched
their v1 predictions and are re-measured in run 2 under the same rules. Run 2 is the declared run; run 1 stays on
record with its two defects named here.
