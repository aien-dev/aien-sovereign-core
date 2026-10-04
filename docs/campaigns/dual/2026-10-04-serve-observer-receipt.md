# Receipt: DUAL `serve.admit_preempt` read-only observer (DUAL-3a instrumentation)

Program: ARCH-0031 "DUAL, Constraint Pricing for Soft Resource Budgets", §7.3
(decision sites observed through a read-only tap), gate DUAL-3 prerequisite
"at least one registered decision site with a read-only tap".
Site: `serve.admit_preempt` (DUAL_CURRENT_STATE §4), production owner
`aien-scheduler::AienScheduler::build_scheduled_batch`.

**Gate verdict: none claimed.** This receipt records instrumentation
(DUAL-3a). `DUAL_ADVISORY_SCHEDULER` is NOT claimed PASS: no
`DualRecommendation` is produced, no pre-registered workload set with
recommendation-versus-actual analysis exists yet, and no prediction-quality
claim is made. The tap changes no production decision; the evidence below is
that it changes none.

## 1. Identity

| Field | Value |
|---|---|
| Repository | `aien-dev/aien-sovereign-core` |
| Base commit (GitHub `main`) | `91fba8fc0f00fd09fc3e9a6137522af3ae79c4e1` |
| Branch | `dual/serve-observer` |
| Candidate commit | filled in §9 after the final commit |
| Tree | clean at each recorded command (`git status --short` empty) |
| Compiler | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)` |
| Build config | `dev` profile for tests (as CI), `release` profile for the benchmark only |
| Host | NVIDIA DGX Spark, GB10, aarch64, Linux 7.0.0-1019-nvidia, 20 CPUs |
| GPU exercised | **No.** Every test and the benchmark run `MockInferenceBackend` on the host CPU. No GB10 kernel, no NVRM call, no model. |
| Workspace path | fresh clone `~/workspace/dual/sc-observer` |

## 2. What changed (code)

- `crates/aien-scheduler/src/dual_observer.rs` (new): `DecisionObserver` trait
  (`fn observe_admit_preempt(&self, &AdmitPreemptRecord)`, returns `()`), the
  immutable `AdmitPreemptRecord` (site id, step counter, limits, KV readings at
  entry and exit, arena active count, ranked preemption victims, every running
  sequence examined with its verdict, every queue head examined with the exact
  numbers compared, the chosen preempted / decode / prefill-chunk sets, the
  batch outcome), `RecordingObserver`, `CountingObserver`.
- `crates/aien-scheduler/src/lib.rs`: `AienScheduler.observer:
  Option<Arc<dyn DecisionObserver>>` (default `None`),
  `set_decision_observer`, `decision_observer_installed`;
  `build_scheduled_batch` is now a thin wrapper around
  `build_scheduled_batch_traced(&mut Option<AdmitPreemptTrace>)`, which is
  the former body with `if let Some(t) = trace` inserts. No arithmetic,
  ordering, lock, allocation or KV call of the decision moved or changed.
- `crates/aien-scheduler/Cargo.toml`: dev-dependencies `async-trait`, `sha2`
  (both already in `Cargo.lock`); mutant feature
  `dual_observer_mutant_changes_decision` (off; negative control, §5).
- Tests: `tests/dual_common/mod.rs` (deterministic workload set + canonical
  trace), `tests/dual_decision_golden.rs`, `tests/dual_observer_parity.rs`,
  `tests/dual_observer_overhead_bench.rs` (ignored, manual),
  `tests/fixtures/dual_serve_admit_preempt.golden.json`.

Untouched: `aien-kv-cache` (refcounts, copy-on-write, allocation failure
paths, watermark enforcement), `aien-runtime`, `aien-abi-core`. No pressure
or metric authorizes anything; the hook has no return value.

## 3. Pre-registered workload set

Thirteen scripted scenarios in `tests/dual_common/mod.rs::scenarios()`, run
on `MockInferenceBackend` (fixed tokens, no sleep), timing fields excluded
from every trace:

| Scenario | Exercises |
|---|---|
| `admission_basic` | admission of four requests, run to length limit, drain |
| `no_admission_empty_queues` | `Ok(None)` steps, nothing touched |
| `chunked_prefill_continuation` | 300-token prompt, 64-token chunks, 100-token budget, beside a decoding sequence |
| `preemption_watermark` | 6-block pool, watermark 3: lower priority preempted, readmitted |
| `preemption_tie_equal_priority` | equal priorities: stable sort, first in running order is the victim |
| `no_preemption_sufficient_pool` | same requests, 256-block pool, no victim ranked |
| `queue_exhaustion_batch_limit` | 12 requests, batch of 4, admission stops on the batch limit each step |
| `capacity_one_below` | 7 of 8 blocks needed: admitted |
| `capacity_exactly_at` | 8 of 8: admitted |
| `capacity_one_above_refused` | 9 of 8, nothing running: `KV_POOL_EXHAUSTED` error, request stays queued, nothing allocated |
| `capacity_wait_while_running` | shortfall while something runs: waits without error |
| `fanout_32_way` | 32 children share a prefilled parent's blocks, admitted straight into decode |
| `fanout_500_way` | same with 500 children |

Counter wrap: `step_id` is a `u64` incremented with `+=` (unchanged, would
panic in debug at 2^64 steps); the record copies it, nothing else. Not
exercised: no test crosses the wrap because the production code does not
either. The record's `step_id` equals the batch's `step_id` in every test.

## 4. Tests and results

Commands run from the repository root on the candidate tree. Results are
filled in §9 from the actual runs.

| # | Command | Purpose |
|---|---|---|
| T1 | `DUAL_WRITE_GOLDEN=1 cargo test -p aien-scheduler --test dual_decision_golden decisions_match_golden` on a worktree at the base commit, with `tests/dual_common`, `tests/dual_decision_golden.rs` and the two dev-dependencies copied in | before/after: produce per-scenario sha256 of every decision on the base commit |
| T2 | `cargo test -p aien-scheduler --test dual_decision_golden` | candidate decides identically to the base commit; workload deterministic run to run; drained scenarios hold zero KV blocks, zero tables, zero arena records, empty queues |
| T3 | `cargo test -p aien-scheduler --test dual_observer_parity` | observer absent vs `RecordingObserver`: canonical trace bytes identical, KV accounting identical per step, one record per decision, records agree with the batches, preemption / tie / capacity / fan-out / queue-exhaustion / empty-step records carry the right facts |
| T4 | `cargo test -p aien-scheduler --doc` | two `compile_fail` doctests: an observer cannot mutate the record or return a choice |
| T5 | `cargo test -p aien-scheduler -p aien-kv-cache -p aien-runtime -- --test-threads=1` | existing scheduler, KV mutation / copy-on-write / incremental-blocks, runtime World lifecycle, spine, swarm, 500-branch tests |
| T6 | `cargo clippy -p aien-scheduler --all-targets -- -D warnings`; `cargo fmt --all -- --check` | CI invariants |
| T7 | `crumb verify .` | Crumb protocol |

Sanitizers / miri: not used; the repository does not run them.

## 5. Negative control

Two layers.

**By type.** `DecisionObserver::observe_admit_preempt(&self, record:
&AdmitPreemptRecord)` returns `()`. The observer holds no reference to the
scheduler, KV manager or arena. Two `compile_fail` doctests in
`dual_observer.rs` show that (a) mutating the record and (b) returning a
different `AdmitPreemptChoice` do not compile (T4).

**By injected mutant.** Cargo feature `dual_observer_mutant_changes_decision`
makes the admission loop stop after one grant whenever an observer is
installed (a decision change that only the tap's presence triggers). With it
on, the parity test must fail:

```
cargo test -p aien-scheduler --features dual_observer_mutant_changes_decision --test dual_observer_parity observer_presence_changes_no_decision
test observer_presence_changes_no_decision ... FAILED
thread 'observer_presence_changes_no_decision' panicked at crates/aien-scheduler/tests/dual_observer_parity.rs:46:9:
scenario admission_basic: decisions differ with the observer installed
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out
exit=101
```

Full parity file under the mutant: 4 failed (`observer_presence_changes_no_decision`,
`kv_accounting_identical_with_and_without_observer`,
`queue_exhaustion_records_show_the_batch_limit_stopping_admission`,
`fanout_children_enter_decode_on_existing_tables`), 9 passed. The feature is
off by default and never enabled in CI; the mutant code stays in the tree,
disabled, so the control can be rerun (same convention as
`prefill_mutant_no_check`).

## 6. Benchmark: pre-registered threshold (written and committed BEFORE the run)

Command (release profile, host CPU, mock backend, whole workload set per
round, 7 rounds per arm, median reported):

```
cargo test -p aien-scheduler --release --test dual_observer_overhead_bench -- --ignored --nocapture
```

Arms: `absent` (no observer; production configuration), `counting`
(`CountingObserver`: serializes each record to count bytes, keeps nothing),
`recording` (`RecordingObserver`: clones and keeps every record),
`absent_repeat` (noise reference).

Pre-registered acceptance (frozen here, commit before the run):

| Id | Measure | Threshold | Why |
|---|---|---|---|
| B1 | `median(counting) / median(absent)` | **≤ 1.25** | The tap must be cheap enough to stay installed through the DUAL-3 observation period. On the mock backend the step has no model compute, so this ratio is a strict upper bound on the production fraction (real steps are dominated by the forward pass). No prior threshold exists in the repository for this tap; 25 % is the author's choice and it is not moved after the run. |
| B2 | `records` emitted (counting and recording arms) | **== decisions** exactly | one record per decision, no drops, no duplicates |
| B3 | `absent` arm decision count | **== counting arm decision count** | the tap adds or removes no scheduling step |
| B4 | `median(absent_repeat) / median(absent)` | reported, informational | machine noise reference for reading B1 |
| B5 | bytes emitted, heap allocations and bytes (global allocator counter) per arm | reported, no threshold | observer cost in memory terms for the DUAL-3 design |

Not measured here: absent-arm wall time on the candidate against the base
commit. UNVERIFIED by timing; the before/after identity is proven by T1/T2
(decisions) and by reading the diff (the absent path adds one
`Option::is_some()` and passes `None` through).

If B1 fails the receipt says FAIL and the threshold stays.

## 7. Results

Filled in §9.

## 8. Limits

- Mock backend only. No real model, no GPU, no NVRM.
- The tap records the admission / preemption / chunk decision. It does not
  record the `step()` post-processing (token append, finish, KV free on
  finish), which is not a DUAL decision site.
- `RunningVerdict::GateRefusedReprefill`, `DecodeNoTable`, `NoRecord` and
  `AdmissionVerdict::MissingRecord` are reachable only through internal state
  manipulation; the pre-registered workloads do not reach them. UNVERIFIED
  by test in this receipt (the code paths are plain copies of the
  scheduler's own branches).
- Timing of the absent path versus the base commit: UNVERIFIED by
  measurement (see §6).

## 9. Run log (appended after the runs, nothing above this line edited)
