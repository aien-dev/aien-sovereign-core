# WHOLE-SYSTEM-E2E held-fault rows: addendum v1.3 (objective identity across a restart)

Tracking: aien-dev/aien-sovereign-core#408 (gap row 4 of `RECOVERY-AUDIT-2026-10-11.md`), aien-architecture#190 Priority D.
Amends `FAULTS-v1.md`, `FAULTS-v1.1.md`, `FAULTS-v1.2.md` and `FAULTS-v1.2.1.md` by ADDING four rows and one fixture step;
no existing row, predicate or rule changes, and the recorded runs stay as they are. Committed before the engine change
and before the harness change. Every status below is NOT_RUN at the time of writing.

## Why

The audit found that the objective record (`aien-cli objective record`, sc#388) is a file the CLI writes, that `compose
propose --objective-id` only echoes the id, and that no grant, intent, ack, reconcile record or execute answer carries
it. After a SIGKILL nothing ties the surviving effect to the recorded objective, and nothing re-reads the record.

## Engine change these rows test (sc#408, landed after this file)

- The daemon's compose-commit record carries `goal_sha256` (sha256 of the goal text it composed for).
- `compose authorize` passes the report's `objective_id`. The daemon re-reads `objectives/<id>.json` under
  `AIEN_PROVENANCE_DIR`, checks that the id recomputes from the recorded text and that the recorded text digest equals
  the commit's `goal_sha256`, and writes `objective_id`, `objective_text_sha256`, `objective_record_sha256` into the grant.
  Otherwise it refuses by name (`ObjectiveMissing`, `ObjectiveMismatch`) and writes nothing.
- An intent on an objective-bound grant re-reads the record first (same names, nothing written on refusal) and carries
  `objective_id`; the ack and every reconcile record of that intent carry it; `compose effects` shows it per intent.
- Reconcile (at start and on request) re-reads the record of every unsettled objective-bound intent and reports the
  binding per outcome (`objective: {objective_id, record: bound | ObjectiveMissing | ObjectiveMismatch, ...}`). The
  intent itself is still settled by the world check (an effect is never re-run, so settling is observation); what a
  missing or altered record stops is any further step for that objective: a new intent and a new grant are refused.
- Records without the new fields load exactly as before; a grant minted without an objective id behaves as before.

## Fixture step (v1.3)

Before the fixture propose, `aien-cli objective record --text "$OBJ6"` writes the record into the fixture's provenance
directory, and the propose runs with `--objective-id` (the sc#388 CLI check). The record id is `REC_ID` =
sha256(`AIEN_E2E_OBJECTIVE_V1`, LF, text), distinct from the harness chain id. Rows of v1 to v1.2 use the same proposal
report with the top-level `objective_id` field removed (the `report` object is byte-identical to a propose without
`--objective-id`), so those rows send no objective id and run the legacy path exactly as before. The v1.3 rows use the
report with the id and get a copy of the record in their own provenance directory.

## Rows (fixture, build and counting rules as `FAULTS-v1.md` sections 2 to 5)

| row | injection | PASS iff | prediction |
|---|---|---|---|
| E6-objective_bound_after_intent | objective-bound grant G; executor held at `after_intent`; SIGKILL daemon and executor; record left intact; restart | both kills 137; no file at the kill; the restart reconcile line names the intent with `objective_id` REC_ID and `record` `bound`; `compose effects` lists the intent of G with `objective_id` REC_ID; the record file's sha256 after the restart equals its sha256 before the kill and `objective show REC_ID` returns the same text digest; same grant refused `AlreadySpent`; a new approval G2 (same report) is minted and its grant text names REC_ID, G2 executes `DONE` with `objective_id` REC_ID in the execute answer and in its `write_file` receipt; files 1; duplicates 0 | PASS |
| E6-objective_missing_after_intent | as above, but the record file is removed after the kill and before the restart | both kills 137; no file; the restart reconcile line names the intent with `record` `ObjectiveMissing` (not silent) and the intent is settled by the world check (`NOT_DONE`, recorded); `objective show REC_ID` refused `ObjectiveMissing`; a new approval with the same report is refused `ObjectiveMissing` and mints no grant; files 0 | PASS |
| E6-objective_altered_before_intent | objective-bound grant G; executor held at `before_intent`; SIGKILL daemon; executor released (refused, no intent); the record rewritten in place with other text and a consistently recomputed `objective_text_sha256`; restart; execute G | daemon kill 137; the released executor is refused and writes nothing; after the restart execute G is refused `ObjectiveMismatch`; no intent for G in the ledger; files 0 | PASS |
| E6-objective_after_write | objective-bound grant G; executor held at `after_write`; SIGKILL daemon and executor; restart | both kills 137; file present once at the kill; the restart reconcile line settles the intent `DONE` with `objective_id` REC_ID and `record` `bound`; `compose effects` shows the intent of G `DONE` with `objective_id` REC_ID; the record's text digest equals sha256 of the objective text and REC_ID recomputes from it (harness side); same grant refused `AlreadySpent`; files 1; duplicates 0 | PASS |

The rows of v1 to v1.2 run unchanged in the same run and must score as recorded in `runs/FAULTS-20261011T012434Z`
(ten PASS). Rules as v1 section 5: one attempt per row; a FAIL is recorded, not fixed silently; the run folder is kept
as it falls. A PASS shows the objective binding across a SIGKILL restart of the CPU-reference build with `fault-hold`
on one host. It does not show power loss, a hostile operator with write access to the provenance directory and the
compose home together, the accelerator path, a release build, or any contract step as PASS. The executor killed after
the write leaves no `write_file` receipt; for that row the objective binding is shown by the engine's own reconcile
record and ledger, and the receipt binding by row 1.
