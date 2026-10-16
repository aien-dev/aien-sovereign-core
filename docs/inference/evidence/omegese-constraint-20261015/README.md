# Omegese constraint receipts (2026-10-15)

Branch `feat/omegese-constraint`, new crate `crates/aien-omegese-constraint` on main 9196c786 (ADR 0004 step 2 merged, PR #550). Host only: no chip run, no GPU, the 30B engine and the door untouched. Nothing in the workspace depends on the crate; with no constraint, generation is the step 2 loop unchanged.

Oracle: `omegese next --machine` built from aien-spark/omega 6c1827ba (`crates/aien-omegese-constraint/omegese.lock`; CI builds the same commit). Programs walked: the public dev programs (`tests/one_layer/omegese/dev`) and the arm examples (`tools/exp445/arms/examples`). No held-out task text was read.

| Gate | Receipt | Result |
|---|---|---|
| Host tests (`cargo test -p aien-omegese-constraint`, debug), at commit 45ef8f8 | `host-tests.txt` | 18 passed, 0 failed, 3 ignored (the ignored ones need the real binary or the tokenizer and run below) |
| Exact-oracle walks on `EchoChainModel` (64-token synthetic table with one- and two-boundary tokens), 12 seeds greedy and sampled | `host-tests.txt`, `exact_walks_write_the_one_program` | every walk writes the one target program inside the fences and ends on the stop token; multi-item tokens were picked; two-boundary tokens were refused and counted |
| A refusal goes through the rollback | `host-tests.txt`, `an_oracle_refusal_goes_through_the_rollback` | the history equals what was emitted, every layer at the ledger, the pending head forgotten, warm run equals cold run |
| Every switch off is byte-identical to no constraint (graph on and off, greedy and sampled) | `host-tests.txt`, `every_switch_off_is_byte_identical_to_no_constraint` | same tokens, outcome, call log and head reads; zero oracle calls |
| Fence-only arm | `host-tests.txt`, `the_fence_only_arm_forces_the_fence_and_nothing_else` | the fence is forced, the rest is the free run |
| Real oracle, 40 seeded walks on the synthetic table (`OMEGESE_BIN`, `--ignored`) | `real-oracle-walks.txt` | 35 walks ended on eof and `omegese emit` accepted all 35; 5 cut by length, completed by `omegese complete`, accepted; 5865 masked steps, 3.12 oracle calls per step |
| Blank-run equivalence the mask relies on (a run of blanks reads as one space, or one newline if it holds one) | `real-oracle-walks.txt`, `real_oracle_blank_runs_are_interchangeable` | 3315 prefix pairs, identical item lists (881 with items on both sides) |
| Qwen vocabulary (151669 tokens, `~/models/qwen3-30b-a3b-instruct-2507-fp8/tokenizer.json`) on 25 dev programs, 47 walks (header forced and not), release build | `qwen-vocab-release.txt` | every program written exactly, reply = fence + program + fence; 2279 masked steps, 2232 took Qwen's own next token, the other 47 are the eof steps (the closing fence); 0 two-boundary refusals of Qwen's own token, 0 other refusals |
| Same, debug build (before the census line was added) | `qwen-vocab-debug.txt` | same counts |
| Red before green: five mutants of `constraint.rs`, each alone, at 45ef8f8 | `mutants-red.txt` | each turns at least one test red (M1 two-boundary tokens allowed; M2 separators unchecked; M3 the part after the separators unchecked; M4 switch off masks instead of `Any`; M5 a refusal read as `Any`) |
| `cargo clippy -p aien-omegese-constraint --all-targets -- -D warnings`, `cargo fmt --all` | run locally, and CI | clean |

## The mapping rule, measured on Qwen

Of the tokens made only of bytes Omegese text can hold, 34823 cross no word boundary, 45553 cross one, and 35 cross two or more. The 35 are refused at every step (72655 token-step refusals counted over the 2279 masked steps); Qwen's own tokenizer never chose one of them on the dev programs, so the refusal cost nothing there.

## Per-step latency

Wall time of one masked step: its oracle calls (one subprocess each) plus the scan of the vocabulary.

| Build, table | Steps | Calls per step | Median | p95 | Max |
|---|---|---|---|---|---|
| release, Qwen 151669 | 2279 | 6.61 | 11.4 ms | 17.3 ms | 43.5 ms |
| debug, Qwen 151669 | 2279 | 6.61 | 64.9 ms | 75.6 ms | 212.6 ms |
| debug, synthetic 64 | 5865 | 3.12 | 2.6 ms | 4.7 ms | 21.9 ms |

`SHA256SUMS` lists the receipts.

CALC-EXEMPT: every figure above is copied from a receipt line, not derived.
