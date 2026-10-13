# Persistent KV: metrics and settings

Companion to ADR 0003. Names are final; values are defaults to be confirmed by C6/C7 (host) and N3 (native). "Where reported" names the exact surface. Nothing here is measured yet.

## Settings (host, environment variables, same style as `AIEN_KV_CONTEXT_TOKENS` in `crates/aien-runtime/src/shared_kv.rs:306`)

| name | type and unit | default | effect | parsed in |
|---|---|---|---|---|
| `AIEN_KV_STORE_DIR` | path | unset = persistence off | root of the host object store (ADR 2.4). Unset: no store, no park, no restore, zero behaviour change | `shared_kv.rs` |
| `AIEN_KV_STORE_NAMESPACE` | string, at most 64 bytes | `default` | sharing namespace (ADR 1.2 field 13) | `shared_kv.rs` |
| `AIEN_KV_STORE_QUOTA_BYTES` | bytes, decimal integer | 8 GiB | disk quota counting every object found at recovery | `host_fs.rs` |
| `AIEN_KV_STORE_MAX_OBJECTS` | count | 65536 | index cap at recovery and at runtime (ADR 4.1) | `host_fs.rs` |
| `AIEN_KV_STORE_MAX_INFLIGHT_OPS` | count | 64 | jobs between Submitted and Published, both queues together | `worker.rs` |
| `AIEN_KV_STORE_MAX_INFLIGHT_BYTES` | bytes | 256 MiB | payload bytes between Submitted and Published | `worker.rs` |
| `AIEN_KV_CHUNK_BLOCKS` | blocks | 16 | chunk size in KV blocks (part of the namespace; changing it orphans objects) | `shared_kv.rs` |
| `AIEN_KV_RESTORE_MAX_BLOCKS` | blocks | pool / 4 | upper bound on holder blocks allocated for restores at once | `kv_residency.rs` |
| `AIEN_KV_RESTORE_RETRY_WINDOW_MS` | ms | 60000 | after two failed restores of one chain head within the window, prefill without lookup | `kv_residency.rs` |
| `AIEN_KV_STORE_RECOVERY_MAX_MS` | ms | 5000 | recovery time budget; past it the store opens over-cap | `host_fs.rs` |
| `AIEN_KV_STORE_DRAIN_MS` | ms | 2000 | shutdown drain deadline | `worker.rs` |
| `AIEN_KV_PARK` | `1`/`0` | `1` when the store dir is set | park finished turns | `kv_residency.rs` |
| `AIEN_KV_RESTORE` | `1`/`0` | `1` when the store dir is set | restore at submit | `kv_residency.rs` |

Existing settings that this design reads and never redefines: `AIEN_KV_CONTEXT_TOKENS` (restorable prefix is capped at the operator context), `AIEN_ALLOW_UNCHECKED_KV_MEMORY` (untouched), `AIEN_STEP_LOG` (the step line gains the counters below), `AIEN_RUNTIME_STATE_DIR` (not used as a default store root: persistence must be opted into explicitly).

## Settings (native, compile-time or slab superblock)

| name | unit | default | where |
|---|---|---|---|
| `KV_SLAB_SLOTS` | count | sized at format from region units / slot_units | slab superblock |
| `KV_SLAB_SLOT_UNITS` | 4096-byte units | 1 + ceil(max_chunk_bytes / 4080) | slab superblock |
| `KV_PUMP_BUDGET_CMDS` | disk commands per between-steps call | 2 | `infer.c` |
| `KV_QUEUE_DEPTH` | requests | `DISK_QUEUE_DEPTH` (32) | `disk.h` (reused, not redefined) |

## Metrics (host)

Reported in three places: `RuntimeStatusReport` (spine.rs `status_report`, serialized to the `status` IPC reply), the `AIEN_STEP_LOG=1` step line (counters only), and the benchmark JSON (C9). Units are in the name or the column.

| name | unit | kind | meaning | where |
|---|---|---|---|---|
| `kv_pool_allocated_bytes` | bytes | gauge, constant after start | the one pool allocation (`KvPoolPlan.total_bytes`); never changes when blocks are freed | status |
| `kv_pool_total_blocks`, `kv_pool_free_blocks`, `kv_pool_used_blocks` | blocks | gauge | existing counts, renamed to make the "free does not release RAM" point explicit | status (existing `free_kv_blocks`/`total_kv_blocks` kept as aliases) |
| `kv_pool_holder_blocks` | blocks | gauge | blocks referenced by storage holders (park sources, restore destinations) | status, step line |
| `kv_restore_inflight_blocks` | blocks | gauge | holder blocks of restores not yet adopted | status, step line |
| `kv_store_inflight_ops`, `kv_store_inflight_bytes` | count, bytes | gauge | queue occupancy between Submitted and Published | status, step line |
| `kv_store_persisted_bytes`, `kv_store_persisted_objects` | bytes, count | gauge | index totals, including foreign-namespace objects | status |
| `kv_store_quota_bytes`, `kv_store_max_objects`, `kv_store_over_cap` | bytes, count, bool | gauge | limits and the over-cap flag | status |
| `kv_park_jobs_total`, `kv_park_bytes_total`, `kv_park_failed_total`, `kv_park_dedup_total` | count, bytes | counter | park outcomes | status, step line |
| `kv_park_latency_us_p50`, `kv_park_latency_us_p95` | microseconds | histogram summary (bounded reservoir of 1024) | Submitted to Published | status |
| `kv_restore_jobs_total`, `kv_restore_bytes_total`, `kv_restore_failed_total`, `kv_restore_coalesced_total`, `kv_restore_fallback_prefill_total` | count, bytes | counter | restore outcomes; fallback = miss or failure turned into plain prefill | status, step line |
| `kv_restore_latency_us_p50`, `kv_restore_latency_us_p95` | microseconds | histogram summary | Submitted to adopted (S3) | status |
| `kv_restore_tokens_total`, `kv_prefill_tokens_total` | tokens | counter | tokens restored versus prefilled, so the saved fraction is derivable | status, bench |
| `kv_store_stale_completions_total`, `kv_store_cancelled_total`, `kv_store_queue_full_total`, `kv_store_disk_full_total`, `kv_store_evicted_bytes_total` | count, bytes | counter | ADR 5.3, 5.5, 7.1, 7.4 | status |
| `kv_recovery_elapsed_us`, `kv_recovery_indexed`, `kv_recovery_corrupt_deleted`, `kv_recovery_tmp_deleted`, `kv_recovery_foreign_namespace` | microseconds, counts | gauge, set once at open | `RecoveryReport` | status, startup log line |
| `ttft_cold_us`, `ttft_restored_us` | microseconds | per-request measurement | first token latency with `kv_restore_tokens` = 0 versus > 0 | bench JSON, step line per request when `AIEN_STEP_LOG=1` |
| `decode_tokens_per_s` | tokens/second | per-run | aggregate decode throughput | bench JSON |
| `cache_state` | enum `page_cache`, `device`, `unknown` | label | how the restore bytes were served; `device` only after the drop-caches step ran and was recorded | bench JSON |
| `platform` | enum `hosted`, `native_emulator`, `native_physical` | label | where the numbers came from; `native_physical` only from an authorized Spark run | bench JSON, evidence ledgers |

## Metrics (native)

Printed as one `kv:` line by the stage and recorded in `docs/evidence/kv-native.md` (aienos). Counters are `uint64_t` in the slab state, no clock dependency except `ck` microsecond timestamps where the kernel has them.

| name | unit | meaning |
|---|---|---|
| `kv_slab_slots_total`, `kv_slab_slots_used` | slots | capacity and occupancy after recovery and at report time |
| `kv_slab_used_bytes` | bytes | sum of payload_len of valid slots, any namespace |
| `kv_slab_bounce_bytes` | bytes | the driver's bounce buffer size: the only staging memory native uses |
| `kv_session_kv_bytes` | bytes | `2 * n_layer * capacity * kv_dim * 4` for the open session (allocated memory) |
| `kv_park_cmds`, `kv_restore_cmds` | disk commands | commands issued by the pump per job |
| `kv_park_us`, `kv_restore_us` | microseconds | Submitted to Durable (park) and Submitted to imported (restore) |
| `kv_pump_calls`, `kv_pump_cmds_max` | count | between-steps calls and the largest budget actually used |
| `kv_steps_during_park` | count | decode steps that completed while a park job was in flight (the interleaving evidence of ADR 9.3) |
| `kv_recovery_us`, `kv_recovery_slots_valid`, `kv_recovery_slots_empty`, `kv_recovery_slots_foreign` | microseconds, counts | the bounded header scan |
| `kv_fail_stop` | bool | the queue fail-stopped on a device error since open |
