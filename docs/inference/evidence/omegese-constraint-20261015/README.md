# Omegese constraint receipts (2026-10-15)

Branch `feat/omegese-constraint` (PR #555), new crate `crates/aien-omegese-constraint`, built on main 9196c786 (ADR 0004 step 2, PR #550) and since merged up with main. Host only: no chip run, no GPU, and the 30B engine and the door untouched. Nothing in the workspace depends on the crate; with no constraint, generation is the step 2 loop unchanged.

Oracle: `omegese next --machine`, built from aien-spark/omega 6c1827ba (`crates/aien-omegese-constraint/omegese.lock`; CI builds the same commit). The programs walked are the public dev programs (`tests/one_layer/omegese/dev`) and the arm examples (`tools/exp445/arms/examples`). No held-out task text was read.

All receipts are at code commit 63bd631, after both review passes.

| Gate | Receipt | Result |
|---|---|---|
| Host tests (`cargo test -p aien-omegese-constraint --no-fail-fast`, debug) | `host-tests.txt` | 24 passed, 0 failed, 3 ignored (the ignored ones need the real binary or the tokenizer and run below) |
| Exact-oracle walks on `EchoChainModel` (64-token synthetic table with one- and two-boundary tokens, ` \n` among them), 12 seeds, greedy and sampled | `host-tests.txt`, `exact_walks_write_the_one_program` | every walk writes the one target program inside the fences and ends on the stop token; multi-item tokens were picked; two-boundary tokens were refused and counted |
| Soundness step by step: every allowed token at every masked step checked against the exact oracle, for 2 targets (ending on a newline and not) × 4 framings | `host-tests.txt`, `every_allowed_token_keeps_the_target_reachable` | no allowed token leads nowhere; replies framed as asked |
| Forced closing when no token marks the fence; a header the oracle refuses fails when the constraint is built | `host-tests.txt`, `the_closing_is_forced_when_no_token_marks_it`, `a_header_the_oracle_refuses_fails_at_construction` | pass |
| A refusal goes through the rollback | `host-tests.txt`, `an_oracle_refusal_goes_through_the_rollback` | the history equals what was emitted, every layer at the ledger, the pending head forgotten, warm run equals cold run |
| Every switch off is byte-identical to no constraint (graph on and off, greedy and sampled) | `host-tests.txt`, `every_switch_off_is_byte_identical_to_no_constraint` | same tokens, outcome, call log and head reads; zero oracle calls |
| Fence-only arm; unfenced masked arm | `host-tests.txt`, `the_fence_only_arm_forces_the_fence_and_nothing_else`, `the_unfenced_arm_ends_on_the_stop_token` | the fence alone is forced, the rest is the free run; without a fence the reply is the program and ends on stop, and `finished()` claims completion only where the stop was forced (completion there is `FinishReason::Stop`) |
| Oracle process: a hung process is killed at the deadline; an early exit reports its own message | `host-tests.txt`, `a_hung_oracle_is_killed_at_the_deadline`, `an_early_exit_reports_its_own_message` | pass |
| Real oracle, 40 seeded walks on the synthetic table (`OMEGESE_BIN`, `--ignored`) | `real-oracle-walks.txt` | 35 walks ended on eof and `omegese emit` accepted all 35; 5 were cut by length, completed by `omegese complete`, and accepted; 5685 masked steps, 3.08 oracle calls per step |
| Blank-run equivalence the mask relies on (a run of blanks reads as one space, or as one newline if it holds one) | `real-oracle-walks.txt`, `real_oracle_blank_runs_are_interchangeable` | 3315 prefix pairs, identical item lists (881 with items on both sides) |
| Qwen vocabulary (151669 tokens, `~/models/qwen3-30b-a3b-instruct-2507-fp8/tokenizer.json`) on 25 dev programs, 47 walks (header forced and not), release build | `qwen-vocab-release.txt` | every program written exactly, reply = fence + program + fence; 2279 masked steps, 2232 took Qwen's own next token, the other 47 are the eof steps (the closing fence); 0 two-boundary refusals of Qwen's own token, 0 other refusals; soundness: 22134 allowed continuations checked against the oracle, 0 refused |
| Review MUST, red before green: a blank run holding a newline (` \n`) was checked against its space | `must1-red-run.txt` (the fix reverted on top of the new tests) | red: synthetic walks and the step-by-step check fail on ` \n` after `ret`; on Qwen 1364 of 22879 checked continuations refused. Green at 63bd631 (`host-tests.txt`, `qwen-vocab-release.txt`) |
| Red before green: ten mutants of `constraint.rs`, each alone, at 63bd631 | `mutants-red.txt` | each turns at least one test red: M1 two-boundary tokens allowed; M2 separators unchecked; M3 the part after the separators unchecked; M4 switch off masks instead of `Any`; M5 a refusal read as `Any`; M6 the first separator read raw; M7 eof tokens without a backtick; M8 the closing rewritten from the start; M9 no forced closing; M10 header not checked |
| `cargo clippy -p aien-omegese-constraint --all-targets -- -D warnings`, `cargo fmt --all` | run locally, and CI | clean |

## The mapping rule, measured on Qwen

Of the tokens made only of bytes Omegese text can hold:

- 34823 cross no word boundary;
- 45553 cross one;
- 35 cross two or more.

The 35 are always refused: 72655 token-step refusals were counted over the 2279 masked steps. Qwen's own tokenizer never chose one of them on the dev programs, so the refusal cost nothing there.

## Per-step latency

Wall time of one masked step: its oracle calls (one subprocess each) plus the scan of the vocabulary. The machine was shared with other sessions.

| Build, table | Steps | Calls per step | Median | p95 | Max |
|---|---|---|---|---|---|
| release, Qwen 151669 | 2279 | 6.62 | 13.4 ms | 20.6 ms | 26.5 ms |
| debug, synthetic 64 | 5685 | 3.08 | 2.8 ms | 5.1 ms | 8.3 ms |

`SHA256SUMS` lists the receipts.

CALC-EXEMPT: every figure above is copied from a receipt line, not derived.
