# Persistent KV program: implementation cuts

Binding design: `docs/adr/0003-persistent-kv-identity-storage-contract-and-ownership.md` (ADR 0003). Issues: sovereign-core #484, #485; aienos #367. Each cut is one reviewable PR with its own red-before-green tests. "Red" means the test is committed first and fails for the stated reason; "green" is the implementation commit in the same PR. Acceptance boxes are quoted by id from the tables at the end.

## Plan corrections (against `~/handoffs/2026-10-13-persistent-kv-program.md`)

1. **The order "P2 all of #484, then P3 #485" is wrong at the runtime seam.** Parking and restoring in the runtime (#484 scope) cannot be wired without the bounded queue, the worker and the residency state machine (#485 scope): a park that blocks the spine or a restore with no incarnation check would be thrown away later. So the trait and the deterministic fake adapter (C2) come right after identity (C1), the worker and queues (C5) come before the runtime wiring (C6), and C6 is where #484 and #485 meet. The handoff's note that #485 "may start in parallel against a fake adapter" is kept and made concrete: C5 and C8 develop against C2's fake.
2. **The #484 "core" PR in the plan is too big to review** (identity + trait + adapter + recovery + park/restore + recompute + metrics). It is split into C1, C3, C4, C6, C7. Every cut stays under roughly 1500 changed lines including tests.
3. **The native pin goes in aienos, not in sc**, and it pins bytes, not a crate: the header codec test vectors (section "Pin").
4. **Recompute-on-miss is not a separate feature**: a miss or a failed restore leaves `prompt_tokens_prefilled = 0`, which is today's prefill path (ADR 0003 section 6.3 item 4). It is tested in C6, not built.
5. Benchmarks (C9) run on the host CPU backend by default; the GB10 leg is a separate, optional evidence run that must not take the GB10 lock away from the OWN-30B and 8B lanes named in the handoff.

## Dependency graph

```
C1 identity/codec ──> C2 trait + fake ──> C5 worker + queues ──> C6 runtime park/restore ──> C7 restart + greedy + metrics
                 │                   │                                               └──> C8 concurrency through scheduler ──> C9 bench
                 │                   └──> C4 host fs adapter + recovery ─────────────────┘
                 └──> C3 kv-cache ownership primitives ──────────────────────────────────┘ (C6 needs C3, C4, C5)
C1 ──> N1 native header codec + vectors (pin) ──> N2 slab format/recover ──> N3 adapter + fault injection ──> N4 ownership + pump ──> N5 QEMU reboot
```

Parallel lanes once C2 is merged: {C3, C4} and {C5} in parallel; {N1, N2} in parallel with C3..C5. C8 and C9 after C6. N3..N5 need only C1's vectors, not any later sc cut.

## sovereign-core cuts

### C1: identity, header codec, validation (#484)

- Files: new crate `crates/aien-kv-store/` (`Cargo.toml`, `src/lib.rs`, `src/identity.rs`, `src/record.rs`, `src/validate.rs`), `Cargo.toml` workspace member, `THIRD_PARTY.md` row (LMCache ideas), `tests/golden/kv_record_v1.json` (generated vectors: namespace inputs, keys, header bytes).
- `no_std` + `alloc`; depends on `sha2` (default-features off) and `aien-platform`.
- Red tests: `identity_vectors_match_golden` (fails: no crate), `namespace_changes_when_any_field_changes` (every field of ADR 1.2, one at a time), `chunk_key_chain_depends_on_parent_and_position`, `header_roundtrip_256_bytes`, `validate_refuses_before_any_allocation` (a validator that counts allocations is given 12 corruptions: wrong magic, version, header hash, namespace, key, parent, position, token_count 0 and over, geometry, payload_len, truncated file; each refused with the named error and zero allocations).
- Acceptance: A3 (identity half), D1 (bad length/checksum half, shared vectors).

### C2: `KvStore` trait and deterministic fake adapter (#484, #485 shared)

- Files: `crates/aien-kv-store/src/store.rs` (trait, `Completion`, `Capacity`, stages), `src/fake.rs` (in-memory adapter with scripted latency per job, injectable faults: queue-full, disk-full, flush-fail, torn-write, delayed completion, duplicate completion, completion after cancel), `src/pin.rs` (`PinnedSource`/`PinnedDest` guards).
- Red tests: `fake_stages_are_ordered_submitted_completed_published_durable`, `fake_refuses_over_ops_and_over_bytes`, `fake_failed_store_leaves_index_unchanged`, `fake_cancel_before_complete_publishes_nothing`, `fake_cancel_after_complete_is_too_late`, `fake_same_key_store_dedups`, `fake_delete_skips_pinned` (adapted from LMCache `test_delete_skips_pinned_key`).
- Acceptance: B3 (queue-full, disk-full, cancel, duplicate at adapter level), A4 (adapter half).

### C3: kv-cache ownership primitives (#484)

- Files: `crates/aien-kv-cache/src/lib.rs` (`adopt_table(holder, seq)`, `HOLDER_SLOT` rule and `holder_id(job)`, `holder_block_count()` for metrics, `KvBlock` unchanged), `crates/aien-kv-cache/tests/holder_ownership_tests.rs`.
- Red tests: `holder_fork_keeps_blocks_after_owner_free` (fork into holder, free owner, blocks not in free list), `holder_free_returns_blocks_only_at_zero_refcount`, `adopt_table_moves_table_and_keeps_refcounts`, `owner_append_on_shared_partial_block_copies_not_holder` (COW on the owner, holder bytes unchanged), `shared_prefix_sibling_valid_after_holder_free` (two sequences sharing a prefix, one parks and frees, the other still decodes its blocks), `holder_id_never_collides_with_arena_ids` (arena at capacity, every id's slot < u32::MAX).
- Acceptance: A5, B2 (refcount half).

### C4: host filesystem adapter and recovery (#484)

- Files: `crates/aien-kv-store/src/host_fs.rs` (feature `std`: temp + fsync + rename + dir fsync, sharded objects, LRU eviction, quota, `recover`), `src/index.rs` (bounded index), `crates/aien-kv-store/tests/host_fs_tests.rs`, `tests/host_fs_recovery_tests.rs`.
- Red tests: `store_then_recreate_adapter_serves_same_bytes` (A1 adapter-recreation row), `recovery_counts_foreign_namespace_objects_toward_quota` (the LMCache gap), `recovery_deletes_tmp_and_corrupt_headers_only`, `recovery_is_bounded_by_max_objects_and_sets_over_cap`, `recovery_reads_exactly_256_bytes_per_object` (counting reader), `enospc_mid_write_leaves_last_object_intact` (temp dir on a size-limited tmpfs or an injected `write` error through an `FsOps` seam), `fsync_failure_fails_job_and_publishes_nothing`, `truncated_payload_fails_on_load_and_is_deleted`, `lookup_is_index_only_no_io` (strace-free: `FsOps` call counter is zero).
- Acceptance: A1 (adapter recreation), A3 (payload half), A4, A6 (restart accounting), A7 (recreation row).

### C5: storage worker, bounded queues, fairness, coalescing, drain (#485)

- Files: `crates/aien-kv-store/src/worker.rs` (thread, two intake queues, credit scheduler 3:1, completion queue, cancel flags checked per block, `shutdown(deadline)`), `src/coalesce.rs`, `crates/aien-kv-store/tests/worker_tests.rs` (against the fake).
- Red tests: `worker_bounds_ops_and_bytes_in_flight`, `worker_three_restores_then_one_park_when_both_waiting`, `worker_coalesces_same_key_loads_into_one_io` (fake counts one read), `worker_cancel_removes_only_own_waiter`, `worker_stale_completion_is_dropped_by_incarnation`, `worker_drain_publishes_completed_and_cancels_submitted_within_deadline`, `worker_never_touches_dest_after_cancel_ack` (poisoned destination pages after ack, fake asserts no write), `worker_backpressure_refuses_not_grows` (queue length never exceeds the bound under 10x offered load).
- Acceptance: B3, B4 (bounded queues half), shutdown bullet of #485.

### C6: runtime park at Finished, restore at submit, residency states (#484 + #485 seam)

- Files: `crates/aien-runtime/src/kv_residency.rs` (new: states, epochs, lock rules of ADR 8), `crates/aien-runtime/src/spine.rs` (park hook before `release_sequence` at the `Finished` branch; restore hook in `submit_request`; `poll` of completions at the start of `step`; status fields), `crates/aien-runtime/src/shared_kv.rs` (store construction next to `build_shared_kv_runtime_for_model`, settings parsing), `crates/aien-scheduler/src/sequence.rs` (`SequencePhase::AwaitingRestore`), `crates/aien-scheduler/src/lib.rs` (skip that phase in admission; `set_prefilled_prefix(seq, n)`), `crates/aien-abi-core/src/lib.rs` (`restore_sequence` default `Err`), `crates/aien-inference-abi/src/transformer_backend.rs` (`restore_sequence` implementation, same refusals as `fork_sequence`), tests `crates/aien-runtime/tests/kv_park_restore_test.rs`.
- Red tests: `finished_turn_publishes_full_chunks_and_partial_tail`, `park_never_blocks_step` (fake adapter with 500 ms latency; step wall time unchanged within tolerance), `restore_caps_prefix_at_prompt_len_minus_one`, `restore_miss_falls_back_to_plain_prefill_with_no_refusal`, `restore_failure_frees_holder_and_prefills`, `stale_completion_cannot_touch_reused_sequence_id` (arena slot reused with a new generation before the completion lands), `awaiting_restore_is_not_admitted_until_all_chunks_completed`, `restore_refused_below_watermark_falls_back`.
- Acceptance: A1 (restart row prerequisite), A2 (in-process half), A5, B1 (mechanism), B3 (stale completion through the runtime), B5 (routing).

### C7: real restart, greedy equality, metrics and settings (#484)

- Files: `crates/aien-runtime/tests/kv_restart_test.rs` (child process via `std::env::current_exe`, pattern of `approved_crash_test.rs:80-89`; the child stores, exits; the parent recovers and restores), `crates/aien-runtime/src/spine.rs` (`RuntimeStatusReport` new fields), `crates/aien-runtime/src/server.rs` (step log line adds the storage counters), `docs/persistent-kv/METRICS_AND_SETTINGS.md` (kept in sync), `docs/FIRST_RUN.md` (one paragraph: the store directory and how to turn it off).
- Red tests: `process_restart_rediscovers_and_restores_identical_bytes` (A1 restart row), `greedy_continuation_after_restart_matches_uninterrupted_baseline` (A2; reference CPU backend, Llama-3.2-1B or the reference test weights), `wrong_model_digest_namespace_is_never_served_after_restart`, `metrics_report_allocated_bytes_separately_from_free_blocks` (allocated constant while free changes), `guarantee_matrix_rows_are_labelled` (a test that asserts the doc's four rows match the test names present; a doc-code drift check).
- Acceptance: A1, A2, A3, A6, A7 (restart row), A8.

### C8: concurrency proof through scheduler and backend (#485)

- Files: `crates/aien-runtime/tests/kv_concurrency_test.rs`, `crates/aien-scheduler/tests/kv_restore_admission_tests.rs`, fixtures under `crates/aien-runtime/tests/fixtures/kv/`.
- Red tests: `resident_sequence_emits_tokens_before_unrelated_restore_completes` (B1: fake adapter holds the restore 2 s; a resident sequence must emit at least N tokens first, through `AienRuntimeSpine::step` and the real scheduler), `concurrent_park_and_restore_of_shared_prefix_preserve_bytes_and_refcounts` (B2), `fork_then_cancel_parent_during_park_keeps_child_valid` (B2), `duplicate_restore_requests_share_one_load` (B3), `memory_pressure_restore_and_admission_make_progress_without_deadlock` (B4: pool of 8 blocks, 4 sequences, randomized fake latencies, 10 k steps, invariant checks each step), `queue_full_and_disk_full_under_load_never_lose_resident_progress` (B3, B4).
- Acceptance: B1, B2, B3, B4, B5.

### C9: benchmark (#485)

- Files: `crates/aien-scheduler/src/bin/bench_persistent_kv.rs` (1, 2, 4, 8 conversations; fixed prompts of 512 and 2048 tokens; modes resident-only, cold-prefill, restore-from-page-cache, restore-from-device with `posix_fadvise(DONTNEED)` + `echo 3 > drop_caches` optional step recorded as done/not done), `docs/persistent-kv/BENCH.md` (method, machine, NOT_RUN table), `benchmarks/data/` entry per ADR 0001 rule (numbers only via the canonical dataset).
- Red tests: `bench_binary_refuses_to_report_without_mode_and_cache_state_labels` (the output schema requires `cache_state` in {page_cache, device, unknown} and `host|native`).
- Acceptance: B6, B7, B8.

## aienos cuts

### N1: native header codec and golden vectors (the pin) (#367)

- Files: `native/kv/kv_record.h`, `native/kv/kv_record.c` (header encode/decode/validate, key derivation with argus sha256), `native/kv/tests/kv_record_test.c`, `native/kv/tests/golden/kv_record_v1.json` (copied from sc C1), `native/kv/CONTRACT.lock`, `native/kv/README.md`, `native/kv/Makefile` (`make test`, `make freestanding`, `make sanitize`, `make mutants` like `native/disk`).
- Red tests: every golden vector decodes and re-encodes byte for byte; every corruption case of sc C1 is refused with the mirrored error code; `nm -u` empty.
- Acceptance: D1 (bad length/checksum).

### N2: slab format, header-last write order, bounded recovery (#367)

- Files: `native/kv/kv_slab.h`, `kv_slab.c` (`kv_disk_bind` with overlap refusal, `kv_slab_format`, `kv_slab_recover` reading N headers into a caller workspace, slot state machine), tests `native/kv/tests/kv_slab_test.c` over `disk_file`.
- Red tests: `bind_refuses_overlap_with_store_and_anchor`, `recover_reads_exactly_n_header_units` (disk_file op counter), `power_cut_at_every_unit_boundary_never_exposes_partial_record` (the `disk_file` power-cut injector, same method as `store_crash` tests; slot is either empty or whole), `torn_header_is_empty_slot`, `used_bytes_counts_foreign_namespace_slots`.
- Acceptance: D1 (partial writes, flush failure), D2 (format half), D3 (Store region untouched: run the existing store tests on an image that also has a slab).

### N3: native adapter, sealing, fault injection (#367)

- Files: `native/kv/kv_store.c` (the `KvStore` state machine in C: submit, pump, poll, cancel; M5 sealing with 4080-byte chunks, scratchpad decrypt), `native/kv/tests/kv_store_test.c`.
- Red tests: `stages_completed_published_durable_follow_flush_calls` (disk_file flush counter), `flush_failure_fails_job_and_fail_stops`, `queue_exhausted_refuses_with_efull`, `wrong_key_class_cannot_open_slot`, `disk_full_no_free_slot_keeps_existing_slots`, `decrypt_never_copies_before_tag_verify` (mutant GUARD).
- Acceptance: D1 (all five fault classes), D4 (buffer retained until completion).

### N4: ownership, cancellation, pump between steps (#367)

- Files: `crates/aienos-infer-kernel/src/session.rs` (`kv_export`, `kv_import`, `set_pos` FFI), `native/kernel/core/infer.c` (between-steps hook calls `kv_slab_pump(budget)`), `native/kernel/include/ck.h` (hook signature), tests `native/kv/tests/kv_pump_test.c`, `crates/aienos-infer-kernel/tests/kv_export_import.rs`.
- Red tests: `source_slice_unchanged_until_disk_write_returns` (poison after return only), `cancel_mid_slot_leaves_header_zero`, `shutdown_within_deadline_leaves_only_whole_slots`, `tokens_keep_flowing_while_slot_fills_across_steps` (per-step timestamps and command counter both advance; the report line states "interleaved, not overlapped"), `export_import_roundtrip_restores_identical_f32_rows`.
- Acceptance: D4, D5 (with the limitation documented).

### N5: QEMU store, reboot on the same image, restore, continuation (#367)

- Files: `scripts/qemu_ck_kv_restore_test.sh` (disposable image with Store + anchor + slab regions), `native/kernel/svc/pi5_agent.c` or a new `svc/kv_agent.c` stage (park after the reply, restore on the next boot), `docs/adr/0021-native-persistent-kv-slab.md` (short aienos ADR that points at sc ADR 0003 and records the slab region and the pin), evidence ledger `docs/evidence/kv-native.md` (built / emulator / hosted / physical NOT_RUN).
- Red tests: the script fails until the boot-2 log shows the restored prefix and the golden continuation (`CK_AGENT_GOLDEN_*`), and until `used_bytes` after reboot equals before.
- Acceptance: D2, D3, D5, D6 (physical rows NOT_RUN).

## Pin: where it goes and what it pins

`native/kv/CONTRACT.lock` in aienos:

```
format_version = 1
sc_commit = <sha of the sc commit whose C1 generated the vectors>
sc_adr = docs/adr/0003-persistent-kv-identity-storage-contract-and-ownership.md
vectors_sha256 = <sha256 of tests/golden/kv_record_v1.json>
```

It pins the header byte layout, the key derivation strings and field order, the validation error order, and the test vectors. It does not pin a crate, a Rust version or any runtime behaviour. N1's test refuses to run if the vectors file hash differs from the lock. The sc side records the same hash in `crates/aien-kv-store/tests/golden/README.md` so a vector change is a deliberate two-repo bump.

## Acceptance ids

#484: A1 store, real restart, rediscover, restore identical bytes. A2 greedy continuation equals baseline (stochastic separately specified: not claimed, ADR 6.4). A3 wrong model/layout/position/namespace and truncated/corrupt payloads cannot be reused. A4 disk-full, interruption, failed writes never discard the last usable KV/history. A5 shared-prefix owners remain valid on park/cancel. A6 resident, staging and disk budgets bounded including restart accounting. A7 tests distinguish adapter recreation, process restart, clean reboot, power loss. A8 metrics.

#485: B1 slow-I/O progress proof. B2 concurrent park/restore and fork/cancel preserve payloads and refcounts. B3 duplicate loads, stale completions, queue-full, disk-full, mid-transfer cancel. B4 memory pressure: bounded, progress, no deadlock. B5 real routing through scheduler/backend. B6 benchmark 1/2/4/8. B7 resident vs cold vs restore, page-cache vs device, hosted vs native. B8 NOT_RUN recorded.

#367: D1 host fault injection (partial writes, flush failure, bad length/checksum, disk-full, exhausted queues). D2 emulator store, reboot on the same disk, restore, continuation. D3 existing Store/Cortex/boot/update/recovery tests valid. D4 buffers/blocks retained until device completion incl. cancel/shutdown. D5 progress evidence distinguishing inference during I/O from queueing; native limitations documented. D6 report allocated memory, staging bytes, persisted bytes, transfer latency, restart recovery; physical only from an authorized run.
