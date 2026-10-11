# Regression evidence, sc#408 (objective identity across a restart), Lane D, 2026-10-11

Pre-registered before the code change, together with `FAULTS-v1.3.md`. Results are filled in below the plan after the
runs; the plan is not edited.

## Plan

| check | how | expected |
|---|---|---|
| new runtime tests red on main | `crates/aien-runtime/tests/objective_identity_test.rs` and the new ledger unit tests in `effects.rs`, built against main 4bc1594 | do not build (the objective fields and entry point do not exist), error lines recorded |
| new runtime tests green | `cargo test -p aien-runtime` with the linked compose archive | all pass, including the new ones |
| CLI tests | `cargo test -p aien-cli` (objective record tests included) | all pass |
| lint | `cargo clippy -p aien-runtime -p aien-cli --all-targets -- -D warnings` | clean |
| old records load | ledger unit test on records without the new fields; the existing effects tests unchanged | pass |
| RUN-1 verdict unchanged | `tools/aien-verify` on `runs/RUN-1-20261010T154724Z` (contract sha as in `REGRESSION-sc403.md`) before and after | same `verdict_id` as recorded |
| fault rows | `scripts/whole_system_e2e_faults.sh run` (CPU reference, fault-hold build) | v1 to v1.2 rows: ten PASS as before; v1.3 rows as predicted in `FAULTS-v1.3.md` |


## Results (2026-10-11, branch recovery/objective-identity)

| check | result |
|---|---|
| new tests red on main | built against 4bc1594 plus the test commit 0dd4c95 only: do not compile, 11 errors; distinct ones: error[E0425]: cannot find function 'authorize_with_objective' in module 'effects';error[E0432]: unresolved import 'aien_runtime::objective';error[E0609]: no field 'goal_sha256' on type '&CommitRow';error[E0609]: no field 'goal_sha256' on type 'CommitRow';error[E0609]: no field 'objective_id' on type 'effects::IntentRow';error[E0609]: no field 'objective' on type 'Grant';error[E0609]: no field 'objective' on type '&ReconcileOutcome';error[E0609]: no field 'objective' on type 'ReconcileOutcome' |
| new tests green | `objective_identity_survives_a_restart_and_is_re_read` (8 steps: refusals by name, bound grant and intent, restart with the record intact binds the reconcile, record removed refuses a new approval, record altered refuses the intent and writes nothing, the ack names the objective, restart with the record gone reports `ObjectiveMissing` and still settles NOT_DONE, the legacy path shows no objective) ok; `objective_fields_are_additive_on_grants_and_intents` ok; `id_is_the_cli_formula` ok |
| `cargo test -p aien-runtime -p aien-cli` (linked archive sha256 3eb20f5e...) | 604 passed, 0 failed, 23 ignored |
| `cargo clippy -p aien-runtime -p aien-cli --all-targets -- -D warnings` | clean; `cargo fmt --check` clean |
| old records load | the unit test above (grant, intent and commit records without the new fields read as before); every pre-existing effects, minted-grant, desk and provenance test unchanged and passing; the desk wire test still sees the old `ComposeAuthorize` bytes when no objective is given |
| RUN-1 verdict unchanged | `tools/aien-verify` (unchanged by this branch) on a copy of `runs/RUN-1-20261010T154724Z` with its recorded contract digest 9e3d5f4e...: `verdict_id a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80`, chain_unbroken true (as recorded in its VERIFIER_RECEIPT.json) |
| fault rows | `runs/FAULTS-20261011T023133Z` (fault-hold build sha256 1dc984d2..., CPU reference, one attempt): PRE PASS; the ten v1 to v1.2 rows PASS as before; the four v1.3 rows PASS as predicted, see the table below |

| v1.3 row | result | what the receipt shows |
|---|---|---|
| E6-objective_bound_after_intent | PASS | kills 137/137, no file; restart reconcile: intent NOT_DONE, objective 9694e418... record `bound`, `record_unchanged` true; ledger intent and grant G name it; record sha256 equal before and after; same grant `AlreadySpent`; new grant names it, DONE, execute answer names it, the `write_file` receipt's arguments digest recomputes with it; files 1, duplicates 0 |
| E6-objective_missing_after_intent | PASS | record removed after the kill; restart reconcile: intent NOT_DONE (recorded), record `ObjectiveMissing`; `objective show` refused `ObjectiveMissing`; new approval refused `ObjectiveMissing`, no grant; files 0 |
| E6-objective_altered_before_intent | PASS | daemon 137 while held before the intent; released executor rc 1, no file; record rewritten with other text; after the restart execute of the same grant refused `ObjectiveMismatch`, no intent; files 0 |
| E6-objective_after_write | PASS | kills 137/137, file present once; restart reconcile DONE with the objective `bound`; ledger intent DONE naming it; record text digest is the objective's and the id recomputes; same grant `AlreadySpent`; files 1, duplicates 0 |

Deviation from the pre-registered wording, recorded here: row 1 checks the grant text through `compose recall` of the grant record
and the receipt binding through its arguments digest (the receipt file holds digests, not arguments). Limits: one host, one
attempt per row, SIGKILL only, CPU reference with `fault-hold`; the compose archive was a prebuilt copy (sha256 3eb20f5e...) not
rebuilt from the omega pin in this session (CI builds from the pin). `runs/` was not edited; the new run folder is kept as it fell.
