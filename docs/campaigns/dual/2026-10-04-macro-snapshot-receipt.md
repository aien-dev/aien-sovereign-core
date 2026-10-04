# Receipt: DUAL serve observation snapshot (read-only macroscopic adapter, DUAL-3a)

Program: ARCH-0031 "DUAL, Constraint Pricing for Soft Resource Budgets".
Deliverable: an immutable snapshot of the serve path built only from existing
exact counters, with normalized densities and exact deltas, for DUAL
observation. Companion to the `serve.admit_preempt` decision observer
(`2026-10-04-serve-observer-receipt.md`); independent of it (no shared code,
no dependency on that branch).

**Gate verdict: none claimed.** `DUAL_ADVISORY_SCHEDULER` is NOT claimed
PASS. No snapshot field is an authority input anywhere; nothing here can
change a scheduling, admission, preemption or KV decision.

## 1. Identity

| Field | Value |
|---|---|
| Repository | `aien-dev/aien-sovereign-core` |
| Base commit (GitHub `main`) | `91fba8fc0f00fd09fc3e9a6137522af3ae79c4e1` |
| Branch | `dual/macro-snapshot` (from `main`, not stacked) |
| Tree | clean at each recorded command |
| Compiler | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)` |
| Build config | `dev` profile (as CI) |
| Host | NVIDIA DGX Spark, GB10, aarch64, Linux 7.0.0-1019-nvidia, 20 CPUs |
| GPU exercised | **No.** Tests run `MockInferenceBackend` on the host CPU. |

## 2. What changed

New crate `crates/aien-dual-observation` (registered in the workspace
members list; `Cargo.lock` gains its entry). It depends on `aien-scheduler`,
`aien-kv-cache`, `aien-abi-core` and `serde` only, and reads them through
`&AienScheduler`, `&AienKvManager`, `&StepMetrics`, `&ScheduledBatch`.
Living in its own crate means it can reach only public counters; it cannot
touch scheduler or KV internals by construction. No existing crate changed.

Types (`src/lib.rs`):

- `Density { numerator, denominator }` with `value() -> Option<f64>`:
  `None` when `denominator == 0`. Every quotient in the crate is one of these.
- `StepReading` (from `StepMetrics`, timing field left out), `BatchReading`
  (from `ScheduledBatch`: counts and the token sum the scheduler compares to
  `max_batch_tokens`).
- `ServeObservationSnapshot::capture(&scheduler, &kv, last_step, last_batch)`.
  Every field's doc names its source counter: `SchedulerMetrics::total_steps`
  (generation), `waiting_count`, `preempted_count`, `running_count`,
  `SequenceArena::active_count`, the six `SchedulerMetrics` lifetime
  counters, three `SchedulerConfig` limits, `total_block_count`,
  `available_blocks`, `allocated_block_count`, `active_sequence_count`, the
  six `KvMetrics` fields, the pool half of the watermark predicate, and five
  densities (`kv_block`, `kv_sharing`, `running_slot`, `batch_slot`,
  `batch_token`).
- `ObservationDelta::between(earlier, later)`: exact differences (steps,
  arrivals = `admitted_requests` diff, completions, preemptions, prefill and
  decode tokens, signed changes of allocated blocks, running and waiting).
  Refuses a reversed generation, a lifetime counter that went backwards, or a
  different pool size (`DeltaError`).

No interpolation, no estimate, no state kept between calls, no new state
machine. The scheduler's own `step_id` is private and is not duplicated; the
generation is the existing `total_steps` counter (documented).

## 3. Tests

`crates/aien-dual-observation/tests/snapshot_conservation.rs` runs the real
`AienScheduler` and `AienKvManager` with the mock backend through a mixed
workload (admission, 150-token chunked prefill, completion) and a watermark
pressure workload (6-block pool, watermark 3, one preemption), snapshotting
after every submit and every step, and asserts at every snapshot:

| Ledger check | Exact statement |
|---|---|
| KV partition | `physical_pages (ref_count>0) + free_blocks == total_blocks`; `allocated + free == total`; `used_blocks == physical_pages`; `shared + private == physical`; `logical >= physical` |
| Sequence state machine | `arena_active == running + waiting + preempted` |
| Lifetime ledger | `admitted_requests == finished_requests + running + waiting + preempted` (workload submits only through `submit_request`, never cancels; fork children via `fork_sequence` and cancellation are the documented cases where this denominator does not apply) |
| Step reading | `StepMetrics::active_kv_blocks == allocated_block_count` right after the step |
| Densities | numerators and denominators equal the named counters; `kv_below_watermark == free < watermark` |

Plus: deltas equal counter differences and sum over consecutive snapshots to
the whole (arrivals 5, completions 5, prefill tokens 274 = submitted prompt
tokens); refusals for reversed order, foreign pool size and a counter that
went backwards; zero-capacity scheduler (0 blocks, batch size 0): every
density is `None`, not 0; `BatchReading` token sum; serde round trip.

Commands and results are in §5.

## 4. Limits

- Mock backend, host CPU, no GPU, no real model.
- The snapshot is a point reading; the caller supplies `last_step` and
  `last_batch` because the scheduler does not retain them. A capture with
  `None` for both still holds every counter.
- `generation` is `total_steps` (steps that executed a batch), not the
  scheduler's private per-call `step_id`.

## 5. Run log

| Command | Result |
|---|---|
| `cargo test -p aien-dual-observation` | 8 passed, 0 failed (1 unit: zero-denominator rule; 7 integration: ledgers close at every snapshot under plenty and under watermark pressure, deltas exact and additive, reversed / foreign / backwards refused, zero-capacity densities `None`, batch reading sums, serde round trip) |
| `cargo clippy -p aien-dual-observation --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo check --workspace --all-targets` | Finished (new member builds with the workspace) |
| `crumb compile .` then `crumb verify .` before the commit | see commit; the post-commit STALE reading of the root crumb is the tool's `generated_at_commit` behaviour, also present on pristine `main` (documented in the observer receipt §9.1 T7) |

Negative control (by construction): the crate holds `&AienScheduler` and
`&AienKvManager` only; there is no `&mut` path and no return value the
scheduler reads. Nothing to inject.

Candidate commit: the commit carrying this file (see the PR).
