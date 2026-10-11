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

## Results

(filled after the runs)
