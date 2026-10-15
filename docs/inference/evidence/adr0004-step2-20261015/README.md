# ADR 0004 step 2 gate receipts (2026-10-15)

Branch `adr0004-step2` (PR #550) on main e0dc0296 (step 1 merged). Host only: no chip run, no quiet flag, the 30B untouched (step 2's gate has no chip item).

| Gate | Receipt | Result |
|---|---|---|
| Host tests, both crates (`cargo test -p aien-inference-abi -p aien-serve-http --features aien-inference-abi/host-test-engine -- --test-threads=1`, stub build), at the last code commit | `host-tests.txt` | 413 passed, 0 failed, 17 ignored; the `aien-inference-abi` lib 224 passed (26 of them the new `generation::` tests) |
| R1 red before green: `a_forward_error_leaves_every_layer_at_the_ledger` on the loop without rollback | `r1-red-run.txt` (commit 42408a8, `rollback()` empty) | red: layer lengths [17, 16] against the ledger's 13; green from 41f0952 on (`host-tests.txt`: [13, 13] and [12, 12]) |
| Review finding, red before green: `a_pick_outside_the_allowed_set_is_an_error` | `allow-pick-red-run.txt` (commit 7bf9a45) | red: the sampler's token 0 left an `Allow([1, 2, 3])` step; green at 66968d5 |
| The controller's four `clear_pending` calls (rewind, start of generate, Force, rollback) each removed alone | `clear-pending-mutants.txt` (at 66968d5) | each removal turns one test red |
| Negative control 1, `CorruptRowKv` (V row of position 5, layer 1 overwritten after the rewind) | `host-tests.txt`, `negative_control_a_corrupted_surviving_row_changes_the_warm_run` | the warm run differs from the cold run in 8 of 8 tokens; the clean warm run equals the cold run |
| Negative control 2, history A's payload restored under tokens B | `host-tests.txt`, `negative_control_restoring_a_under_b_continues_differently` | differs from a cold run on B in 8 of 8 tokens; A's payload under A equals the cold run on A |
| `clippy -D warnings` (`aien-inference-abi` with and without `host-test-engine`, `aien-serve-http`, all targets) | run locally, and CI | clean |

`SHA256SUMS` lists the receipts.
