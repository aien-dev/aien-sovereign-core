# ADR 0003: Persistent KV cache: versioned chunk identity, storage contract, ownership and recovery

- **Status**: Proposed (2026-10-13). Binding design for sovereign-core #484 and #485 and aienos #367. Implementation cuts: `docs/persistent-kv/CUTS.md`. Metrics and settings: `docs/persistent-kv/METRICS_AND_SETTINGS.md`.
- **Date**: 2026-10-13
- **Author**: Drake Stapleton <aien@aienos.com> (design drafted by the persistent-KV design owner, Fable 5.1)
- **Source read at**: sovereign-core `origin/main` f560be4; aienos `origin/main` 5429602; LMCache `4d69cdc4` (Apache-2.0).
- **Evidence labels**: VERIFIED (read in source at the cited line), INFERRED (follows from source, not run), UNKNOWN (not established). Nothing in this ADR was run on hardware.

## In plain words

Today the runtime computes the model's "working memory" (the KV cache) for every turn from scratch and throws it away when the turn ends (spine.rs:198-230 frees the sequence on `Finished`). This design saves that working memory to disk in fixed-size, self-describing pieces (chunks), each named by a fingerprint of exactly what it contains (which model, which tokens, at which positions, in which byte layout). When the same conversation continues, even after the runtime process restarts, the saved pieces are checked and loaded back instead of being recomputed. Saving and loading happen on a separate worker so the model keeps answering other conversations meanwhile. Nothing in this design changes which answers the model gives: a restored continuation must produce the same greedy tokens as an uninterrupted one, and anything that cannot be proven compatible is recomputed from the conversation history, which stays the authority.

## Context (what exists, verified)

### Host runtime (sovereign-core)

1. KV memory is one pool of fixed blocks. `KvPoolConfig { num_blocks, block_size, num_layers, num_kv_heads, head_dim, dtype }` (aien-kv-cache/src/lib.rs:46-53, VERIFIED). The pool is one allocation (`UnifiedKvTensorPool::allocate`, lib.rs:611-631, VERIFIED); freeing a block returns it to `free_blocks` and does not release memory (`free_sequence`, lib.rs:1413-1431, VERIFIED).
2. Block layout is fixed by `KvLayout::from_config` (lib.rs:169-218, VERIFIED): `head_stride = head_dim * element_bytes`, `token_stride = num_kv_heads * head_stride`, `kv_plane_stride = block_size * token_stride`, `layer_stride = 2 * kv_plane_stride` (K plane then V plane), `block_stride = num_layers * layer_stride`. Element offsets: `block*block_stride + layer*layer_stride + is_value*kv_plane_stride + token*token_stride + head*head_stride + dim*element_bytes` (lib.rs:221-242, VERIFIED). The daemon pool is bf16 (`SHARED_KV_DTYPE = KvDType::Bf16`, aien-runtime/src/shared_kv.rs, VERIFIED by grep; K/V are rounded on write, lib.rs:530-537, VERIFIED).
3. Blocks carry `ref_count`, `num_tokens`, `is_shared` (`KvBlock`, lib.rs:643-648, VERIFIED). `fork_sequence` bumps every parent block's refcount and marks it shared (lib.rs:1165-1210, VERIFIED). `append_token_with_slot` copies on write only when the last block is shared and not full (lib.rs:1234-1255, VERIFIED): sharing a full block costs nothing. `free_sequence` decrements and frees at zero (lib.rs:1413-1431, VERIFIED).
4. `BlockTable { sequence_id, block_ids, total_tokens, prefill_state }` with `PrefillState::{Allocated, PrefillPending, PrefillReady, SharedFrozen}` (lib.rs:658-668, 714-722, VERIFIED). Decode requires a ready table (aien-scheduler/src/lib.rs:26-34, VERIFIED). `prefix_cache` on the manager is dead code (`#[allow(dead_code)]`, lib.rs:804-805, VERIFIED).
5. The manager is shared as `Arc<parking_lot::RwLock<AienKvManager>>` (lib.rs:1685, VERIFIED) between the spine (spine.rs:23) and the backend (`NativeTransformerBackend.kv_manager`, transformer_backend.rs:130).
6. The daemon steps one spine under a `tokio::sync::Mutex` (server.rs:27, 196-230, VERIFIED): one worker, `step` per iteration, 5 ms sleep when idle. `AienScheduler::step` calls `backend.execute_step` with no KV lock held (aien-scheduler/src/lib.rs:1005-1015, VERIFIED) and takes the KV write lock only for short commits (`complete_prefill` at 1027-1032, `append_token` at 1070, `free_sequence` at 1097-1106, VERIFIED).
7. Sequence ids are `slot: u32 << 32 | generation: u32`, generation 0 invalid (aien-scheduler/src/sequence.rs:7-50, VERIFIED). The arena reuses slots with a bumped generation (sequence.rs:329-370, VERIFIED). `SequenceRecord` has `prompt_tokens_prefilled` and `is_prefilled` (sequence.rs:221-222, VERIFIED), which the step uses to drive chunked prefill (lib.rs:1018-1033, VERIFIED).
8. Backend per-sequence state is `SequenceState { tokens, layers }` (aien-inference-abi/src/weights.rs:20-23, VERIFIED); in paged mode `layers` is empty and `tokens.len()` equals the number of cached positions while prefilling (transformer_backend.rs:136-143, VERIFIED). `pending_prefill_token` holds the token sampled after the last prefill chunk (same lines). Sampling is seeded by `decode_sampling_seed(request_id, position)` (transformer_backend.rs:43-45, VERIFIED). `release_sequence` drops backend state and calls `release_branch` (transformer_backend.rs:613-622, VERIFIED).
9. `ModelConfig` carries `model_id, max_sequence_length, block_size, num_layers, num_heads, head_dim, num_kv_heads, rope_theta, rope_scaling: Option<Llama3RopeScaling { factor, low_freq_factor, high_freq_factor, original_max_position_embeddings }>, qk_norm, ...` (aien-abi-core/src/lib.rs:123-160, 45-50, VERIFIED). The daemon records `ModelIdentity { model_sha256, model_digest_kind, model_path, tokenizer_sha256, tokenizer_path }` (aien-runtime/src/generation.rs:24-30, VERIFIED).
10. Memory planning from #239 (PRs #246 and #270): `KvPoolPlan` (shared_kv.rs:130-145), `plan_checked_model_kv` (shared_kv.rs:368-388), operator cap `AIEN_KV_CONTEXT_TOKENS` (shared_kv.rs:306), `AIEN_ALLOW_UNCHECKED_KV_MEMORY` (shared_kv.rs:295), all VERIFIED. This design reuses the plan and adds no second sizing path.
11. Platform traits: `ObjectStore { read(ObjectId) -> ObjectRef, commit(ObjectWrite) -> ObjectId }` (aien-platform/src/objects.rs:17-20), `UnifiedBuffer` (compute.rs:18-28), `PlatformError` (lib.rs:31-43), `WorkQueue` (queue.rs:47-55), all VERIFIED, all `no_std`. `ObjectStore` is synchronous and copies through a borrowed slice; it has no submit/complete split, no capacity, no delete. It is reused as the *identifier and error vocabulary* (`ObjectId`, `PlatformError`), not as the KV store trait (section 3 says why).
12. `RuntimeStatusReport` is built from `kv.metrics()`, `free_block_count`, `allocated_block_count` (spine.rs:472-497, VERIFIED). `KvMetrics { physical_pages, logical_pages, shared_pages, private_pages, cow_faults, bytes_saved_vs_full_copy, used_blocks }` (lib.rs:788-796, VERIFIED).
13. `sha2` and `blake3` are already in `Cargo.lock` (VERIFIED by grep); `crc32c`/`crc32fast` are not.

### Native (aienos)

14. `disk_dev`: `read`/`write` return only after the transfer completed or failed; `flush` returns only after every acknowledged write is persistent; requests over `max_blocks_per_io` are split (native/disk/disk.h:17-24, VERIFIED). `disk_queue`: at most `DISK_QUEUE_DEPTH` 32 requests, drained in order, FLUSH is a barrier, fail-stop until reset (disk.h:60-83, VERIFIED).
15. NVMe driver: every request goes through the bounce buffer, one command outstanding at a time, the driver waits for completion before returning (native/disk/disk_nvme.h:17-19, VERIFIED). Transfer per command `min(bounce bytes, 128 KiB, 4 KiB << MDTS)` (disk_nvme.h:61-64, VERIFIED). `nvme_write_is_power_fail_atomic` exists (disk_nvme.h:216-217, VERIFIED).
16. Store v1 is append-only with a rewritten catalog per transaction; objects up to 64 MiB, 256 per transaction, 4096 catalog entries (native/store/store_v1.h:16-22, VERIFIED). No delete, free or reclaim entry point exists in `store_v1.h` or `store_engine.h` (VERIFIED by grep). `st_transact` requires payload buffers alive and unchanged until it returns (store_engine.h:146-149, VERIFIED).
17. The sealed Store caps application objects at `SS_MAX_PLAINTEXT` 16384 bytes and 8 per transaction (native/store/store_sealed.h:54-56, VERIFIED) and refuses plaintext kinds (`SS_E_FOREIGN_KIND`, store_sealed.h:80, VERIFIED). The M5 envelope itself allows 64 MiB plaintext in chunks up to 64 KiB with a 16-byte tag per chunk (native/m5/m5.h:160-171, VERIFIED). Keys: `m5_hkdf_expand`, `m5_derive_object_key(domain_key, binding, ...)` (m5.h:36, 73, VERIFIED). AGENTS.md section 4 scratchpad rule applies to every decrypt.
18. `torn_slot`: dual-slot torn-write-safe 4096-byte record, 4 units (native/store/torn_slot.h:53-55, VERIFIED).
19. Native inference: `ck_infer_prompt(text, len, cancel_poll, reply)` steps to the reply end calling `cancel_poll()` between steps (native/kernel/include/ck.h:240-262, VERIFIED). The session FFI is one step per call: `aienos_infer_session_open/prompt/step/cancel/tokens/close` (native/kernel/core/infer.c:76-93, VERIFIED). The probe session has `kv_capacity` 64 positions, `max_new` 16 (infer.c:268-274, VERIFIED). The native KV is f32, `k`/`v` each `n_layer * capacity * kv_dim` floats (crates/aienos-infer/src/decode.rs:50-62, VERIFIED), `#![no_std]` (lib.rs:1, VERIFIED). 4096 positions of Llama-3.2-1B is about 256 MiB of f32 KV (aienos-infer-kernel/src/session.rs:74, VERIFIED comment).
20. The kernel scheduler (`core/sched.h`) is run-queue bookkeeping, not preemptive threads with blocking I/O (VERIFIED by reading sched.h:1-60). There is no asynchronous disk completion path in native.

### LMCache (reference, Apache-2.0)

21. `TokenHasher`: rolling prefix hash, `blake3(prefix_hash || tokens as big-endian u32)`, `none_hash` for the first chunk, fixed `chunk_size` (lmcache/v1/multiprocess/token_hasher.py:30-52, 60-70, 183-190, VERIFIED).
22. Dynamic adapter: write to `<final>.tmp.<uuid>` then atomic rename; lookup on miss checks disk and lazily populates the index; capacity counted in `_total_bytes`; `_inflight_stores` dedups same-key stores; delete skips pinned objects (nixl_store_dynamic_l2_adapter.py:4-22, 139-144, 370-399, 420-470, 486-520, VERIFIED). Its weakness, named in #484: capacity counts only objects already discovered (`_execute_lookup_in_the_loop` adds `_total_bytes` on lazy hit, line 503-511, VERIFIED), so an unseen file is free until found.
23. FS adapter: reversible filename from the key, `.tmp` suffix + `os.replace`, unlink of the temp on failure; no `fsync` anywhere in the file (fs_l2_adapter.py:99-135, 518-532, 640-662, VERIFIED; absence of `fsync` VERIFIED by grep).
24. Tests adapted (test_nixl_store_dynamic_l2_adapter.py): `test_salted_objects_recover_independently_after_restart` (745), `test_store_rejected_when_capacity_exceeded` (985), `test_batched_store_reports_failure_when_capacity_exceeded` (1034), `test_secondary_lookup_finds_key` (1113), `test_secondary_lookup_and_load_data` (1138), `test_load_partial_failure_marks_only_successful` (561), `test_delete_skips_pinned_key` (887). Each is re-expressed as a Rust test; none is copied. Attribution: an Apache-2.0 header on `crates/aien-kv-store/src/identity.rs` (the prefix-chain idea) and the test file that adapts the cases, plus a THIRD_PARTY.md row ("LMCache, Apache-2.0, ideas adapted: deterministic rolling prefix keys, completed-object publication by temp+rename, lazy disk recovery; no code copied"). The LMCache clone has a LICENSE and no NOTICE file (VERIFIED by `ls`).

### Prior art in this repository (not depended on)

25. OWN-30B serve branch `own30b/serve` (sc-own30b, unmerged): in-memory prefix reuse for one conversation, `reused = common.min(prompt.len() - 1)` so at least the last prompt token is always recomputed to produce logits (crates/aien-inference-abi/src/qwen3_moe_omega.rs:1932-1935, VERIFIED on that branch), test `prefix_reuse_prefills_only_the_new_tokens_and_matches_a_cold_run` (2187). This ADR adopts the same "never restore the last prompt token" rule (section 6.5) and the same warm-equals-cold test shape. Nothing else overlaps: that branch has no disk, no identity, no sharing across requests.

## Decision drivers

1. Correctness before speed: no byte is reused unless its identity proves compatibility, and validation happens before any allocation or copy.
2. The runtime mutex and the KV lock are never held across storage I/O.
3. Everything is bounded: queue depth, bytes in flight, recovery memory, disk quota including objects never looked up.
4. One record header for host and native, so tooling and tests read both; payloads differ by backend format and are never cross-loaded.
5. The native side keeps the single-command, bounce-buffered NVMe contract and the freestanding no_std rule.
6. Conversation history stays authoritative; the KV store is a cache that may be dropped at any time.

## 1. Versioned chunk identity

### 1.1 Units

- A **chunk** is `chunk_tokens = chunk_blocks * block_size` consecutive positions of one sequence, aligned to a multiple of `chunk_tokens`. `chunk_blocks` is a setting fixed into the namespace (default 16 blocks; with the 16-token blocks of `model_dir.rs` that is 256 tokens, LMCache's default). A chunk is **full** when it holds `chunk_tokens` positions. The last chunk of a parked sequence may be **partial** (`token_count < chunk_tokens`); partial chunks are keyed with their exact token count and therefore only ever match an identical prefix of identical length. Only full chunks are prefix-share candidates.
- Hash everywhere is SHA-256 (host: `sha2`, already in the lockfile; native: argus `sha256.h`, used by `store_v1.h`). blake3 is not used because native has no blake3.

### 1.2 Namespace id (binds everything that must be equal for bytes to be reusable)

`namespace_id = SHA-256( "AIEN-PKV-NS-V1\0" || fields )` where `fields`, in this order, little-endian:

| # | field | type | source |
|---|---|---|---|
| 1 | format_version | u16 = 1 | this ADR |
| 2 | model_sha256 | [u8;32] | `ModelIdentity.model_sha256` decoded from hex (generation.rs:25) |
| 3 | model_digest_kind | u8 len + bytes | `ModelIdentity.model_digest_kind` (generation.rs:26) |
| 4 | model_id | u16 len + bytes | `ModelConfig.model_id` |
| 5 | num_layers, num_kv_heads, head_dim, block_size | 4 x u32 | `ModelConfig` |
| 6 | rope_theta | u32 (f32 bits) | `ModelConfig.rope_theta` |
| 7 | rope_scaling | u8 tag (0 none, 1 llama3) then, if 1: factor f64 bits, low_freq_factor f64 bits, high_freq_factor f64 bits, original_max_position_embeddings u64 | `Llama3RopeScaling` (abi-core lib.rs:45-50) |
| 8 | qk_norm | u8 | `ModelConfig.qk_norm` |
| 9 | dtype | u8 (1 Fp4, 2 Fp8, 3 Bf16, 4 Fp16, 5 Fp32) | `KvDType` |
| 10 | layout | 5 x u64 strides (head, token, kv_plane, layer, block) + u64 element_bytes | `KvLayout` (lib.rs:143-156) |
| 11 | backend_format | u8 (1 = host paged pool v1; 2 = native f32 decode state v1) | section 2.3 |
| 12 | chunk_tokens | u32 | setting |
| 13 | sharing_namespace | u16 len + bytes | setting `AIEN_KV_STORE_NAMESPACE` (host); sealed Store uuid + identity class + subject id (native, section 9) |

Rationale: fields 2-8 cover "which weights, which geometry, which RoPE"; 9-11 cover "which bytes"; 13 is the authorization scope. Two runtimes with different `block_size` or `chunk_tokens` cannot share objects, by design: positions and chunk boundaries are part of identity.

### 1.3 Chunk key (prefix chaining, adapted from LMCache TokenHasher)

```
chunk_key_0 = SHA-256("AIEN-PKV-CHUNK-V1\0" || namespace_id || [0u8;32]     || first_position u32 || token_count u32 || tokens[0..token_count] as u32le)
chunk_key_i = SHA-256("AIEN-PKV-CHUNK-V1\0" || namespace_id || chunk_key_{i-1} || first_position u32 || token_count u32 || tokens[...] as u32le)
```

`first_position = i * chunk_tokens`. The parent key makes a chunk depend on every earlier token, so a chunk can only be found by walking the chain from position 0; the explicit `first_position` makes a position mismatch impossible even if a caller computes the chain wrongly. The RoPE dependence of K is covered because position, theta and scaling are all in the key.

### 1.4 Validation order (before any allocation or copy)

On lookup: compute keys from tokens (pure, no I/O), consult the in-memory index. On load, after the header is read (256 bytes) and before a destination block is allocated: magic, format_version, header_sha256, `namespace_id == ours`, `chunk_key == requested`, `parent_key == expected`, `first_position`, `token_count` in `1..=chunk_tokens`, geometry fields equal to ours, `payload_len == expected_blocks * block_stride_bytes`, file size equals `256 + payload_len`. Only then are destination blocks allocated and the payload read; `payload_sha256` is verified over the bytes as they land in a *staging* region or, on the host, over the destination blocks before the table is published (section 5.4). A failing payload hash deletes the object and reports a miss. A header that fails validation is counted and the object is deleted (it is a cache). Nothing is ever partially published.

## 2. On-disk record format (shared header; payload per backend)

### 2.1 Header (256 bytes, little-endian, identical on host and native)

| offset | size | field |
|---|---|---|
| 0 | 8 | magic `AIENKVC1` |
| 8 | 2 | format_version = 1 |
| 10 | 2 | record_kind = 1 (chunk) |
| 12 | 4 | header_len = 256 |
| 16 | 32 | namespace_id |
| 48 | 32 | chunk_key |
| 80 | 32 | parent_key (zero for the first chunk) |
| 112 | 32 | model_sha256 |
| 144 | 4 | first_position |
| 148 | 4 | token_count |
| 152 | 4 | chunk_tokens |
| 156 | 4 | block_size |
| 160 | 4 | num_layers |
| 164 | 4 | num_kv_heads |
| 168 | 4 | head_dim |
| 172 | 1 | dtype |
| 173 | 1 | backend_format |
| 174 | 2 | flags (bit 0 partial_tail; others zero) |
| 176 | 8 | payload_len |
| 184 | 32 | payload_sha256 |
| 216 | 8 | written_unix_ns (informational, not identity) |
| 224 | 32 | header_sha256 = SHA-256(bytes 0..224) |

### 2.2 Host payload (backend_format 1)

`ceil(token_count / block_size)` blocks, each exactly `block_stride_bytes`, in the pool's own layout (context item 2): `[layer][K plane | V plane][token][head][dim]` bf16. Positions beyond `token_count` in the last block are zero. The writer reads straight from the pinned pool blocks; the reader writes straight into freshly allocated pool blocks. No reshaping, so a load is a sequence of `block_stride_bytes` copies.

### 2.3 Native payload (backend_format 2)

For each layer `l` in `0..num_layers`: K rows for positions `first..first+token_count` (`token_count * kv_dim` f32), then V rows likewise (decode.rs:56-62 holds `k` and `v` as `n_layer * capacity * kv_dim` floats, so the source stride per layer is `capacity * kv_dim` and the chunk is a strided gather). Native and host payloads differ (f32 strided-by-layer versus bf16 paged blocks) and `backend_format` is in the namespace, so one side can never load the other's object. Byte compatibility across host and native is **not** a goal of this milestone: the native model path is a different model file (GGUF Q4_K_M, infer.c) and different arithmetic; its K/V bytes are not the host's. What is shared is the header, the key derivation, the validation order and the test vectors for both.

### 2.4 Host object layout on the filesystem

```
<root>/                      AIEN_KV_STORE_DIR
  objects/<aa>/<64-hex>.kvc  published chunks, sharded by the first byte of chunk_key
  tmp/<64-hex>.<pid>.<job>.part   in-progress writes (never read by anyone)
  STORE_ID                   16 random bytes written once at first open (recovery sanity)
```

Publication (adapted from LMCache dynamic adapter, with the fsync it lacks): write header + payload to `tmp/...part`, `fsync(file)`, `rename` to `objects/aa/<key>.kvc`, `fsync(objects/aa)`. The job reports **Published** only after the directory fsync returned, so on the host Published and Durable coincide (section 3.3).

### 2.5 Native slot layout (slab region)

The KV region is a dedicated contiguous range of 4096-byte units, outside the sealed Store region and its anchor, bound through a `kv_disk_bind(disk, base_lba, units, store_range, anchor_range)` that refuses any overlap with the Store or anchor ranges (same refusal style as `st_disk_bind`, store_engine.h:93-97). It holds `N` fixed slots of `slot_units` each (sized at format time from the namespace geometry: `1 header unit + ceil(max_chunk_bytes / 4080)` units). Slot `s`:

```
unit 0        : slot header: the 256-byte record header (section 2.1) || slot_incarnation u64 || sealed_units u32 || zero padding || unit_sha256 at 4064..4096
units 1..k    : M5 envelope of the payload, chunk_size = 4080 plaintext bytes so each sealed chunk (4080 + 16 tag) is exactly one unit
```

Write order per slot (section 3.4 gives the states): zero the header unit, flush (the slot is now provably empty), write payload units, flush, write the header unit, flush. A header whose `unit_sha256` or record header fails is an empty slot. Recovery reads `N` header units and nothing else (section 4.2). No catalog, no journal, no generation counter: the slab is a cache and claims nothing about history. Why not Store v1: it is append-only with no reclaim (context 16), so a cache in it could only grow or be reformatted; and the sealed Store caps objects at 16 KiB (context 17) while one native chunk of 64 positions of Llama-3.2-1B is 64 KiB per position x 64 = 4 MiB. The slab reuses `disk_dev`, `disk_queue`, the torn-write discipline of writing a self-checking header last, and M5 chunk sealing. It is not a competing identity or journal: it stores nothing authoritative.

## 3. Storage contract (`KvStore` trait)

### 3.1 Why not `aien_platform::ObjectStore`

`ObjectStore::commit` is synchronous and takes a borrowed slice (objects.rs:17-20). A KV store must (a) submit without blocking the caller, (b) read and write pool memory in place without a copy, (c) report four distinct completion stages, (d) enforce capacity and (e) support cancellation. The new trait lives in a new crate `aien-kv-store` (`no_std` + `alloc` for the identity, codec and validation modules; the host adapter behind a `std` feature) and reuses `aien_platform::{ObjectId, PlatformError}`: `ObjectId(pub [u8;32])` carries a `chunk_key`, and every error is a `PlatformError` variant (`QueueFull`, `OutOfMemory` for quota, `ObjectNotFound`, `HardwareFault(String)` for I/O, `InvalidLayout` for validation).

### 3.2 Trait sketch (Rust, binding shape; names final, bodies not)

```rust
pub struct ChunkKey(pub aien_platform::ObjectId);
pub struct NamespaceId(pub [u8; 32]);
pub struct JobId(pub u64);                // never reused within a store lifetime
pub struct Incarnation(pub u64);          // caller's token, echoed in every completion

/// A read-only view of pinned pool blocks; the guard keeps the storage reference
/// (section 5.2) alive until dropped. Dropping it before completion is a bug the
/// worker detects (the job fails with `PlatformError::InvalidLayout`).
pub struct PinnedSource { pub base: *const u8, pub block_bytes: usize, pub blocks: Vec<usize>, guard: HolderGuard }
pub struct PinnedDest   { pub base: *mut u8,   pub block_bytes: usize, pub blocks: Vec<usize>, guard: HolderGuard }

pub struct ChunkHeader { /* exactly the fields of section 2.1 minus the two hashes */ }

pub enum JobStage { Submitted, Completed, Published, Durable }
pub struct Completion { pub job: JobId, pub incarnation: Incarnation, pub key: ChunkKey, pub stage: JobStage,
                        pub result: Result<(), aien_platform::PlatformError>, pub bytes: u64, pub elapsed_us: u64 }

pub struct Capacity { pub quota_bytes: u64, pub used_bytes: u64, pub objects: u64, pub max_objects: u64,
                      pub inflight_ops: u32, pub max_inflight_ops: u32, pub inflight_bytes: u64, pub max_inflight_bytes: u64 }

pub struct RecoveryReport { pub indexed: u64, pub bytes: u64, pub foreign_namespace: u64, pub corrupt_deleted: u64,
                            pub tmp_deleted: u64, pub over_cap: bool, pub elapsed_us: u64 }

pub trait KvStore: Send + Sync {
    fn namespace(&self) -> NamespaceId;
    fn capacity(&self) -> Capacity;
    /// Index only, no I/O, O(keys). `Some(len)` means a published object exists.
    fn lookup(&self, keys: &[ChunkKey]) -> Vec<Option<u64>>;
    /// Bounded: refuses with QueueFull when ops or bytes would exceed the limits,
    /// OutOfMemory when quota (after eviction of unpinned, unneeded objects) cannot fit it.
    fn submit_store(&self, header: ChunkHeader, src: PinnedSource, inc: Incarnation) -> Result<JobId, PlatformError>;
    fn submit_load(&self, key: ChunkKey, expect: &ChunkHeader, dst: PinnedDest, inc: Incarnation) -> Result<JobId, PlatformError>;
    /// Durability barrier for everything Published before the call.
    fn submit_flush(&self, inc: Incarnation) -> Result<JobId, PlatformError>;
    /// Non-blocking: drains at most `max` completions.
    fn poll(&self, max: usize) -> Vec<Completion>;
    /// Cancelled: the job never touched the destination/published nothing. TooLate: it will complete; ignore by incarnation.
    fn cancel(&self, job: JobId) -> CancelOutcome;
    /// Pins against eviction while a restore chain is being assembled (bounded by max_inflight_ops).
    fn pin(&self, keys: &[ChunkKey]) -> Result<PinToken, PlatformError>;
    fn unpin(&self, token: PinToken);
    fn delete(&self, keys: &[ChunkKey]) -> u64;   // skips pinned and in-flight keys
    /// At open, bounded memory and time (section 4).
    fn recover(&self, budget: RecoveryBudget) -> RecoveryReport;
    /// Stops intake, drains or cancels within `deadline` (section 7.5).
    fn shutdown(&self, deadline_ms: u64) -> DrainReport;
}
```

A **deterministic fake adapter** implements the same trait in memory with scripted latencies and faults (slow I/O, queue-full, disk-full, flush failure, torn write, stale completion) so #485 develops against it before the host adapter lands (cuts C3, C5 in CUTS.md).

### 3.3 Stage semantics

| stage | host adapter | native adapter | what the caller may do |
|---|---|---|---|
| Submitted | job accepted; `inflight_ops += 1`, `inflight_bytes += payload_len`; source blocks are pinned | same; bounce buffer not yet touched | nothing; may cancel |
| Completed | bytes written to `tmp/...part` and `fsync(file)` returned | payload units written and `flush` returned | still not visible to lookup; may cancel (temp is unlinked / header stays zero) |
| Published | `rename` + `fsync(dir)` returned; index updated; visible to lookup and to recovery after restart | header unit written; index updated; visible to lookup; **not yet durable** | release the source pins; too late to cancel |
| Durable | same instant as Published (reported as one completion carrying `Durable`) | `flush` after the header returned | claim persistence |

A load reports `Completed` when the payload landed and its hash verified, then `Published` when the caller's table was committed under the KV lock (section 5.4); `Durable` is not a load stage.

### 3.4 Failure semantics

- Any failure before Published leaves the store exactly as it was: no index change, no visible object, temp unlinked, slot header still zero. The last usable published object is never touched by a failing write.
- Disk full (`ENOSPC` on host, no free slot on native) is reported as `PlatformError::OutOfMemory` on submit when the quota says so, or as a Completion with `Err` when the filesystem refuses despite the quota (host). Either way existing objects stay.
- A flush failure (native `flush` error, host `fsync` error) fails the job; on native it also fail-stops the queue (disk.h:69-72) and the adapter enters `Failed` until re-opened; metrics say so.

## 4. Recovery algorithm

### 4.1 Host

At open, before any job is accepted, with `RecoveryBudget { max_objects, max_time_ms }`:

1. Read or create `STORE_ID`.
2. Stream `tmp/`: unlink every `*.part` (bounded by the directory walk; no list is built). Count `tmp_deleted`.
3. Stream `objects/<aa>/` for all 256 shards. For each entry: the name must be 64 hex + `.kvc` (else count `foreign_file`, leave it, do not count toward quota: we did not write it); `stat` for size; read exactly the 256-byte header; validate magic, version, `header_sha256`, name == `chunk_key`, size == 256 + `payload_len`. Invalid: unlink, count `corrupt_deleted`. Valid: insert `(chunk_key, payload_len, mtime, namespace_id)` into the index (56 bytes per entry; `max_objects` default 65536 caps the index at about 4 MiB) and add `payload_len` to `used_bytes` **whether or not the namespace matches ours** (incompatible objects consume quota until evicted; this closes the LMCache gap of context 22). The payload hash is **not** read at recovery (that would read the whole store): it is verified on first load, which is the lazy recovery LMCache does, and a failing load deletes the object.
4. If `max_objects` is reached: stop indexing, set `over_cap = true`, refuse `submit_store` until `delete`/eviction brings the count under the cap, and log one line. Objects past the cap are not served (they are not in the index) and are the first eviction candidates by shard order on the next open.
5. If `max_time_ms` elapses: same as 4 (`over_cap` with reason time).
6. Eviction policy at runtime: LRU by `mtime` then by load time, never pinned, never in-flight, foreign namespaces first.

Memory bound: 256-byte header buffer + index entries (capped) + one directory iterator. Time bound: one `stat` and one 256-byte read per object.

### 4.2 Native

Read the `N` slot header units in order through the bounce buffer (one command each, `N * 4 KiB`), validate each (`unit_sha256`, record header, namespace), build the index of at most `N` entries in a caller-provided workspace (static storage, like `st_workspace`). A header that fails is an empty slot. Nothing else is read. `used_bytes` counts every valid slot regardless of namespace. There is no temp state to clean: a torn write leaves either a zero header (write order, section 2.5) or a header whose hash fails.

## 5. Ownership model

### 5.1 States (per sequence, in a new `KvResidency` map owned by the spine)

```
Resident  --park()-->  Parking{job, holder, inc}  --Published-->  (holder freed) Resident/Released
Parking   --fail/cancel-->  Resident (holder freed, store untouched)
Lookup hit --restore()-->  Restoring{job, holder, inc}  --Completed+commit-->  Resident (table adopted)
Restoring --fail/cancel/stale-->  Failed{reason} --> recompute (prefill) --> Resident
```

`Parked` is a property of the store (objects exist), not of the runtime: this milestone frees the sequence after publication (section 6), so there is no resident-but-parked state to track. `Failed` is sticky per request until the request's recompute path has started, then it is cleared; it exists so a stale completion cannot flip a request back.

### 5.2 Storage references reuse the fork refcount

The storage worker never holds a raw block id. A **holder** is a real block table in `AienKvManager` whose id lives in the reserved slot `u32::MAX` (`SequenceId { slot: u32::MAX, generation: job_id as u32 }`, generation nonzero; arena slots are always below capacity, INFERRED from `with_capacity`, sequence.rs:348-357, so no arena id can collide). Park: `fork_sequence(seq, holder)` under the KV write lock takes one reference on every block (lib.rs:1176-1179). Restore: `allocate_sequence(holder, tokens)` allocates zeroed blocks owned by the holder. Release: `free_sequence(holder)`. Consequences, all from existing code paths:

- A block reaches `free_blocks` only when its refcount hits zero (lib.rs:1419-1425), so a slot is reused only after the GPU side (the sequence tables) and the storage side (the holder) are both done. No new free-list logic.
- Shared-prefix owners stay valid when another owner parks or cancels: the holder is just one more sharer; the existing COW tests already cover the invariants (`crates/aien-kv-cache/tests/cow_branching_tests.rs`).
- A parked sequence that resumes while its park is in flight copies on write only on a partial last block (lib.rs:1234-1255): the worker keeps reading the frozen original.
- `fork_sequence` sets the parent to `SharedFrozen` (lib.rs:1192-1197). That is the intended meaning ("ready and shared, the prefix is immutable").

### 5.3 Incarnation checks

Every job carries `Incarnation = (seq_id u64 as issued by the arena, residency_epoch u32)`; the spine bumps `residency_epoch` on every state transition of that sequence. A completion is committed only if the sequence still exists with the same `seq_id` (its generation makes a reused slot a different id, sequence.rs:7-50) **and** the same epoch. Anything else is counted `kv_store_stale_completions` and its holder is freed. A reused sequence id can therefore never be mutated by an old job.

### 5.4 Short critical sections (the only places the KV lock is taken for storage)

| name | lock | work | bound |
|---|---|---|---|
| S1 park-begin | KV write | `fork_sequence(seq, holder)`; copy `block_ids` for the chunk range | O(blocks) |
| S2 restore-alloc | KV write | `allocate_sequence(holder, tokens)`; refuse if `free < watermark + needed` | O(blocks) |
| S3 restore-commit | KV write | verify incarnation; `complete_prefill(holder)`; `adopt_table(holder -> seq)` (new method: moves the table to `seq`, keeps refcounts); if a same-key restore is coalesced, `fork_sequence(seq, other_seq)` | O(blocks) |
| S4 holder-release | KV write | `free_sequence(holder)` | O(blocks) |
| S5 queue | store index mutex | push/pop/complete | O(1) |

The payload hash on load is verified over the destination blocks *before* S3 (they are holder-owned and invisible to the scheduler until adopted), so no copy to a staging buffer is needed on the host.

### 5.5 Cancellation

- Cancel own waiter only: a request that is cancelled (`CancelSwarm`, `Aborted`, client gone) calls `cancel(job)`. `Cancelled` frees its holder (S4). `TooLate` lets the job finish; the completion is then stale by epoch and dropped, and the holder is freed at that point. Shared work (a same-key load another request is waiting on) is never cancelled: the coalescing table keeps the job alive while any waiter remains; cancelling removes the waiter only.
- Mid-transfer cancel of a store: the worker checks the cancel flag between block copies (bounded granularity = one block); the temp is unlinked; nothing was published.
- Mid-transfer cancel of a load: destination blocks belong to the holder; the holder is freed after the worker acknowledges (the worker never writes into a freed holder: the free happens in S4 only after the worker's acknowledgement, so there is no use-after-free of pool blocks).

## 6. Park and restore in the runtime

### 6.1 Boundary

A sequence is parked at exactly one point: in `AienRuntimeSpine::step`, when a `DecodeOutput::Finished` is processed (spine.rs:198-230), *before* `backend.release_sequence` and before the scheduler's `free_sequence`. At that point the step has returned, the backend no longer reads the sequence, the table is `PrefillReady`/`SharedFrozen` and `total_tokens` covers prompt + generated tokens. Active decode KV stays resident: nothing parks a running sequence (milestone rule).

### 6.2 Park

1. Tokens = backend `SequenceState.tokens` plus the pending prefill token if any (transformer_backend.rs:136-143); the spine already has both via the backend trait (`fork_sequence` reads them the same way, 2272-2290).
2. Compute the key chain for full chunks and the partial tail. Skip chunks whose keys `lookup` already finds or that are in-flight (same-key publication dedup).
3. S1: fork into one holder per chunk (or one holder for the whole range; one per chunk keeps cancellation granular). Submit stores with `PinnedSource` over the holder's blocks. The stores are low priority (section 7.2).
4. Return to the normal release path immediately. The sequence's own references are dropped as today; the holders keep the blocks until `Published`, then S4.

### 6.3 Restore

1. On `submit_request` in the spine, compute the key chain of the prompt, walk it until the first miss, cap the restorable prefix at `prompt.len() - 1` (context 25: the last prompt token is always recomputed so the logits come from this prompt). Also cap at the operator context (`AIEN_KV_CONTEXT_TOKENS`).
2. `pin` the keys; S2 allocates a holder table for the restorable prefix; submit loads (high priority) for all chunks; the request's residency becomes `Restoring`. The request stays in the scheduler's waiting queue with a new `SequencePhase::AwaitingRestore` (added to the scheduler) that `build_scheduled_batch` skips: ready resident sequences keep decoding (the #485 slow-I/O proof).
3. When every chunk has `Completed`: S3 adopts the table into the request's id and marks `prompt_tokens_prefilled = restored` and the backend's `SequenceState.tokens = prompt[..restored]` through a new backend hook `restore_sequence(seq_id, tokens)` (next to `fork_sequence`, same checks). The phase returns to `Waiting`, and the existing chunked prefill continues from `restored` (scheduler lib.rs:1018-1033).
4. Any failure or stale completion: `Failed`, holder freed, phase `Waiting` with `prompt_tokens_prefilled = 0`: a plain prefill. No request is ever refused because of the cache; refusal exists only when history itself is unavailable, which is the generation record's job, not this design's.

### 6.4 Non-tensor state

Positions: reconstructed from chunk order. Pending prefill token: not needed, the restore ends inside the prompt and the next chunk prefill produces a fresh pending token. Sampling state: `SamplingParams` come with every request (abi-core lib.rs:455-460); the sampling seed is `decode_sampling_seed(request_id, position)` with a fresh `request_id` after a restart, so **stochastic continuation is not reproducible across restart and is not claimed**; greedy continuation is. A test for stochastic continuation, if ever wanted, must pin the seed explicitly; that is outside this milestone.

### 6.5 Guarantee: greedy continuation equals the uninterrupted baseline

Holds when the restored bf16 block bytes are identical and the remaining prefill runs on the same backend with the same weights. The test runs a prompt cold, records the greedy tokens, parks, restarts the process, restores the prefix and compares tokens. The backend must prefill the remainder starting at the restored position with the same chunking as the cold run would have used from that position (chunked prefill is already position-based, lib.rs:1018-1033).

## 7. Queues, bounds, fairness, drain

### 7.1 Bounds

Two bounded intake queues (restore, park) and one completion queue, all behind the store's index mutex, each bounded by **ops** (`max_inflight_ops`, default 64) and **bytes** (`max_inflight_bytes`, default 256 MiB, counted as the sum of `payload_len` of every job between Submitted and Published). `submit_*` refuses with `QueueFull`; the spine treats a refused park as "not parked this turn" (the sequence is released as today) and a refused restore as a plain prefill. Restore destination blocks are counted against the pool: S2 refuses when `free_blocks < watermark_blocks + needed` (the scheduler's own watermark, aien-scheduler lib.rs:460-470), so restores can never starve admission; a refused restore falls back to prefill, which the scheduler already admits by its own rules.

### 7.2 Fairness

The worker dispatches by credits: 3 restores then 1 park when both queues are non-empty (restore latency matters to a waiting user; parks free resident memory and must not starve). Within a queue, FIFO by submission.

### 7.3 Coalescing

Same-key loads: the second and later requests register as waiters on the in-flight job; on `Completed` the first waiter's holder is adopted (S3) and every other waiter gets `fork_sequence(first_seq, other_seq)` in the same S3, so the bytes are read once and shared by refcount. Same-key stores: `inflight_stores` set; duplicates are dropped at submit with `Ok(JobId)` of the existing job (the caller only needs "it will be published").

### 7.4 Backpressure and thrash

Eviction only runs inside `submit_store` to make room and evicts at most `payload_len` worth of LRU unpinned objects; keys pinned by an assembling restore chain are never evicted, so a restore cannot be undone by the park it triggered. A sequence whose restore failed twice in a row within `AIEN_KV_RESTORE_RETRY_WINDOW_MS` is prefilled without lookup (counter per namespace+key chain head, bounded map of 1024 entries).

### 7.5 Shutdown drain

`shutdown(deadline_ms)` (default 2000 ms): stop intake; let stores that are already `Completed` publish (rename + dir fsync is bounded); cancel everything still `Submitted` (temps unlinked); cancel all loads (holders freed through S4 by the spine after the worker acknowledges); join the worker before the KV manager is dropped (the worker owns an `Arc` clone of the manager, so the pool outlives every in-flight pointer). Guarantee at exit: every job that reported `Published` is on disk; nothing else exists after restart (recovery deletes temps).

## 8. Lock discipline

Locks, in the only allowed nesting order: spine `tokio::Mutex` (server.rs:27) → KV `RwLock` (lib.rs:1685) → store index `std::Mutex`. Rules:

1. No storage I/O, no `fsync`, no sleep and no `poll` wait happens while the spine lock or the KV lock is held. The worker thread never takes the spine lock.
2. The worker takes the KV lock only in S2, S3, S4 and never while holding the index mutex; it releases the index mutex before any I/O.
3. The spine takes the index mutex only through `submit_*`, `poll`, `cancel`, `lookup`, which are O(1)/O(keys) and never block on I/O.
4. The KV lock is held across S1..S4 for O(blocks) bookkeeping; the existing scheduler already holds it for the same order of work (`complete_prefill`, `free_sequence`).

## 9. Native mapping (aienos #367)

1. **Device contract kept**: every slot write and read is a sequence of `disk_write`/`disk_read` calls of at most `max_blocks_per_io` through the bounce buffer, one command at a time (disk.h:17-24, disk_nvme.h:17-19). The adapter owns no DMA memory: the caller's KV bytes are copied into the bounce buffer by the driver (bounce contract) so DMA visibility is the driver's, not the adapter's; the adapter only guarantees the source slice is unchanged until `disk_write` returns (same rule as `st_transact`, store_engine.h:146-149). Transfer splitting: a 4 MiB chunk is 32 commands of 128 KiB (disk_nvme.h:61-64).
2. **Completed vs durable**: `Completed` = payload units written and `disk_flush` returned; `Published` = header unit written; `Durable` = `disk_flush` after the header returned. Interruption between payload and header leaves a zero header (we zeroed and flushed it first) so recovery sees an empty slot: no partial record is ever exposed. A torn header unit is rejected by `unit_sha256`; `nvme_write_is_power_fail_atomic` is consulted at format time and recorded in the slab superblock as a documented limit, not a check.
3. **Bounded progress**: native has no asynchronous completion (context 15, 20). The adapter is therefore a **step-budgeted state machine**: `kv_slab_pump(budget_cmds)` issues at most `budget_cmds` disk commands (default 2, about 256 KiB) and returns. The inference loop calls it from the between-steps hook that `ck_infer_prompt` already has (`cancel_poll`, ck.h:242-246; the hook is generalised to `between_steps`). Tokens keep being produced while a slot fills over many steps, which is the bounded progress #367 asks for. **Limitation, reported, not hidden**: with a polled single-command driver on the same core, inference and I/O are interleaved, not overlapped. The evidence that distinguishes real interleaving from mere queueing is the per-step timestamp log: decode steps continue (token timestamps advance) while the slab command counter advances between them. True overlap would need a second core or interrupts and is out of scope.
4. **Separate quota and lifetime**: the slab region is a disjoint LBA range (section 2.5); `kv_disk_bind` refuses overlap; the slab is formatted and erased independently of the Store (`st_format` never touches it; wiping the slab never touches the Store or anchor). Cache eviction is slot overwrite inside the slab only. Existing Store/Cortex tests keep their regions and stay valid.
5. **Encryption and authorization preserved**: payload units are M5 chunk-sealed (AES-256-GCM-SIV, 4080-byte plaintext chunks, standard chunk AAD of AGENTS.md section 3 with `object_kind` = `0x0520` KV chunk) under `k_kv = HKDF-Expand(k_domain, "AIENOS-PKV-SLOT-KEY-V1\0")` with the per-object key from `m5_derive_object_key` bound to (namespace_id, chunk_key, slot_incarnation). `k_domain` exists only after the sealed Store is unlocked (store_boot.h), so a slot is ciphertext to anyone without the volume key: cache reuse cannot bypass session authorization. Decrypt follows the private scratchpad rule (AGENTS.md section 4): tag verified before any byte reaches the session's KV. The native `sharing_namespace` is the sealed Store uuid || identity class || the subject id, so a TEST-class Store never serves a PRODUCTION one.
6. **Native inference path**: new FFI in `aienos-infer-kernel` (no_std): `aienos_infer_session_kv_export(layer, pos0, n, out_k, out_v)` and `aienos_infer_session_kv_import(layer, pos0, n, in_k, in_v)` plus `aienos_infer_session_set_pos(n)` so a restored prefix sets `DecodeState.pos` (decode.rs:56-58). Native **CPU** only; native GPU (#327) is not touched and not claimed. The emulator test: boot, prompt, park, `ck_reset`, boot on the same image, restore, continue, compare the golden tokens of `pi5_agent.h` (`CK_AGENT_GOLDEN_*`).
7. **Overlap proof or limitation**: reported as limitation (item 3). Multiple outstanding commands are out of scope; the pump budget is the only tuning knob and is not an acceptance claim.

## 10. Memory accounting

Reported separately, never conflated (names in METRICS_AND_SETTINGS.md):

- `kv_pool_allocated_bytes`: the pool allocation, constant after start (`KvPoolPlan.total_bytes`, shared_kv.rs:144). Freeing blocks never changes it, and the status report says so in its field name.
- `kv_pool_free_blocks`, `kv_pool_used_blocks` (existing `free_block_count`/`allocated_block_count`), `kv_pool_holder_blocks` (blocks referenced by storage holders), `kv_restore_inflight_blocks`.
- `kv_store_inflight_bytes`, `kv_store_inflight_ops` (queue occupancy), `kv_store_persisted_bytes`, `kv_store_persisted_objects`, `kv_store_quota_bytes`.
- Native adds `kv_slab_slots_total/used`, `kv_slab_bounce_bytes` (the driver's bounce size, the only staging memory on native).

## 11. Guarantee matrix

| scenario | host adapter | native adapter | claim |
|---|---|---|---|
| adapter recreation (same process, new `KvStore` over the same dir/slab) | TESTED (C4: recover, lookup, load, bytes equal) | TESTED, host emulation over `disk_file` (N3) | objects published before `close` are served |
| process restart (real child process, `current_exe` pattern of `approved_crash_test.rs:80-89`) | TESTED (C6) | TESTED as emulator reboot on one image (N5) | greedy continuation equals baseline |
| clean reboot | NOT CLAIMED on the host (no reboot test; fsync + rename + dir fsync is the standard recipe, INFERRED sufficient) | TESTED in QEMU only (N5 is a reboot); physical Spark NOT_RUN (#336) | emulator only |
| abrupt power loss | NOT CLAIMED on the host | TESTED in emulation with the `disk_file` power-cut injector at every unit boundary (N4, same method as the store crash tests); physical NOT_RUN | no partial record is ever exposed; durability of `Published`-but-not-`Durable` slots is not claimed |

## Consequences

- New crate `aien-kv-store` (identity, header codec, validation, trait, fake adapter, host adapter). `aien-kv-cache` gains `adopt_table` and the reserved holder slot rule. `aien-scheduler` gains `SequencePhase::AwaitingRestore`. `aien-runtime` gains the residency map, park at `Finished`, restore at submit, status fields. `aien-abi-core` gains `restore_sequence` on the backend trait with a default `Err`.
- Objects are never shared across `block_size`, `chunk_tokens`, dtype, layout, backend or model; a model upgrade orphans the old objects, which still count toward quota until evicted (section 4.1, by design).
- The design does not add resident retention of parked chunks (an L1 of holders kept alive under low pressure). The ownership model allows it later with no format change.
- Native hosted and emulator results do not qualify physical hardware; every physical row stays NOT_RUN until an authorized Spark run.

## Known limits

- Payload SHA-256 on load costs about 1 ms per MiB on one core (INFERRED from typical sha2 throughput, not measured); it runs on the worker, so it adds restore latency, not step latency. A faster integrity check on the host (blake3) would break header parity with native and is not adopted.
- The restorable prefix is capped at `prompt.len() - 1`; one token of prefill is always paid.
- Quota accounting counts foreign-namespace objects but does not read their payloads; an object with a valid header and a corrupt payload occupies quota until its first load deletes it or LRU evicts it.
- `max_objects` default 65536 and the index are per store directory; two daemons sharing one directory are not supported (STORE_ID is advisory, there is no lock file in this milestone).
