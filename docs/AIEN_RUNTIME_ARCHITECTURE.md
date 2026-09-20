# AIEN Runtime Architecture Specification

## 1. Foundational Thesis

AIEN turns one AI machine into a persistent, branchable computational world where hundreds or thousands of agents can share context, memory, and inference state efficiently instead of behaving like hundreds or thousands of independent API requests.

The runtime eliminates fragmented multi-daemon infrastructure. Agents are not OS processes. Sequences are not OS threads. Core runtime subsystems are not independent background daemons. AIEN runs as a single compiled native host on hardware silicon, managing sequence lifecycles, copy-on-write memory, and hardware-level tensor execution within a unified execution model.

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                              AIEN RUNTIME                               │
│                                                                         │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │ Sequence Runtime (Single-Owner Scheduler Thread)                  │  │
│  │ - SequenceArena: contiguous generational SequenceRecord slots     │  │
│  │ - Lifecycle: Created -> Prefill -> Decode -> BlockedOnTool -> Done│  │
│  │ - Sub-microsecond fork, cancel, and watermark preemption          │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                  │                                      │
│  ┌───────────────────────────────┴───────────────────────────────────┐  │
│  │ Shared-State & Memory Runtime (aien-kv-cache / cortex-rs)          │  │
│  │ - Physical block COW page table (atomic refcounts per block)      │  │
│  │ - Token prefix radix tree (immutable ancestors, divergent leaves) │  │
│  │ - Content-addressed World State (ObjectId, Version, Capabilities) │  │
│  │ - In-process Cortex memory catalog and ContextComposer            │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                  │                                      │
│  ┌───────────────────────────────┴───────────────────────────────────┐  │
│  │ Native Inference Engine (aien-inference-abi / Blackwell GB10)     │  │
│  │ - BlackwellBatchPlan: ragged prefill rows + decode rows in one M  │  │
│  │ - Paged attention reading physical block table directly           │  │
│  │ - In-kernel GPU sampling returning compact token IDs              │  │
│  │ - Single completion fence per step without intermediate sync      │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                  │                                      │
│  ┌───────────────────────────────┴───────────────────────────────────┐  │
│  │ Autonomous Systems Optimizer (spark-rsi)                          │  │
│  │ - Read-only telemetry consumption and workload replay             │  │
│  │ - 11-stage candidate lifecycle in container jails                 │  │
│  │ - Cryptographic ledger verification (.rsi/ledger.db)              │  │
│  │ - Strict referee vs contestant separation                         │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────┬──────────────────────────────────────┘
                                   │
                   NVIDIA DGX Spark (GB10 / Grace Blackwell)
                   128 GB Unified LPDDR5x Memory Substrate
```

---

## 2. The Six Frozen Invariants

### Invariant 1: Single Trusted Process Model with Strict Thread Ownership

1. Latency-critical state, `SequenceArena`, `Scheduler`, `KvManager`, `ModelRegistry`, `CortexRuntime`, and `CompletionRouter` execute inside a single process address space (`aien-runtime`).
2. `SequenceArena` has exactly one mutable owner: the Scheduler thread. No interior synchronization locks (`Arc<RwLock<SequenceRecord>>`) are permitted inside the hot path.
3. GPU tensor execution is owned by a dedicated GPU submission thread holding the CUDA context and streams.
4. Process boundaries are reserved strictly for untrusted tool execution, sandboxed code execution, third-party extensions, and legacy compatibility adapters. Communication with isolated workers occurs solely over capability-restricted IPC.
5. All internal command and event queues have finite capacity. Admission control rejects excess load before allocating sequence descriptors or physical KV blocks.

### Invariant 2: Immutable World Manifests and Structural Sharing

1. A `World` is an immutable, content-addressed snapshot of logical perception, perception history, capabilities, and staged effects.
2. The `WorldManifest` binds persistent root pointers:
   - `object_root`: root hash of the persistent radix/HAMT object index.
   - `filesystem_root`: content-addressed snapshot of repository files.
   - `token_root`: immutable `TokenChainId` lineage.
   - `memory_root`: Cortex memory catalog reference.
   - `capability_set`: granted capability identifiers.
   - `effect_log`: journal of staged external mutations.
   - `sequence_checkpoint`: optional logical sequence state snapshot.
3. Ephemeral accelerator state (`ExecutionAttachment` / KV tensor memory) sits outside the durable `World`. If GPU state is cleared, it is reconstructed from the immutable `TokenChainId`.
4. Branching duplicates root pointers in logarithmic time. Physical storage uses path-copying persistent trees on append-only NVMe segments.
5. External mutations (git push, network API calls, emails, deployments) are recorded in an `EffectJournal` as `EffectIntent` during speculative execution. Effects are committed only upon explicit branch selection and promotion.
6. Dropped branches are collected via reachability garbage collection, avoiding per-fork atomic reference count storms.

### Invariant 3: Strict GPU Submit/Commit Boundary

1. The Rust runtime owns control-plane state and sequence lifecycle. The GPU owns tensor mathematics, activations, and sampling.
2. During an engine step, neither boundary inspects or mutates intermediate state of the other.
3. Zero host-visible synchronization is permitted between transformer layers in the production path. The complete forward pass and sampling execute on a single nonblocking CUDA stream, terminated by one completion event fence (`cudaEventRecord(step_done, stream)`).
4. Engine steps are transactional:
   - Pre-submission: `kv.reserve(&batch)` allocates speculative physical blocks without advancing sequence logical token cursors.
   - Post-completion: The fence signals, generational `SequenceId` tags are revalidated, `kv.commit(reservations)` finalizes block ownership, and sequence cursors advance.
   - Error: `kv.rollback(reservations)` restores previous logical states and frees reserved blocks.
5. The GPU returns a compact `BatchResult` containing only `(SequenceId, Option<u32> token_id, Option<FinishReason>)`. Vocabulary logits buffers remain resident in GPU workspace.
6. A `SubmittedBatch` holds explicit resource leases over model weights, KV blocks, and reusable ring workspaces until the completion fence clears.

### Invariant 4: In-Process Cortex via ContextComposer

1. `CortexRuntime` runs in-process inside `aien-runtime`. Localhost HTTP/REST communication is forbidden for agent memory retrieval.
2. Cortex operations return immutable handles (`RecallSet<MemoryId>`), never duplicated text buffers.
3. `ContextComposer` applies a `ContextPolicy` (budgeting, max items, minimum score) to select memory items and build an immutable `ContextRevision`.
4. Cortex recall never directly allocates physical KV blocks. The scheduler detects missing prefill spans (`PrefillSpan`) between the sequence cursor and the new `ContextRevision`, scheduling tensor prefill explicitly.
5. In-flight immutability: A submitted batch references one immutable `ContextRevision` per participating sequence. Discovered memories form a successor `ContextRevision` applied at the next scheduler step.
6. Reconstructibility doctrine: Logical memory objects are authoritative durable truth. Vector embeddings, search indexes, and KV blocks are disposable acceleration structures.

### Invariant 5: RSI as an External Constrained Optimizer

1. The Autonomous Systems Optimizer (`spark-rsi`) resides outside the live `aien-runtime` process.
2. Invariant: RSI does not directly mutate live runtime state, live scheduler structures, or live executable memory.
3. Bounded online tuning: Hyperparameters with defined ranges (chunk sizes, batch row limits, memory pressure watermarks) are validated by `runtime.validate_tuning()` and applied exclusively at safe step boundaries.
4. Structural optimization: Code, kernel, allocator, and scheduler modifications execute through the 11-stage candidate lifecycle:
   `OBSERVE -> IDENTIFY -> HYPOTHESIZE -> GENERATE -> BUILD (Jail 1) -> ORACLE -> CERTIFY (Jail 2) -> BENCHMARK -> EVALUATE -> SOAK -> CANARY -> PROMOTE`.
5. Separation of referee and contestant: RSI may propose candidate code, but the benchmark oracle, certification suites, promotion policy, cryptographic ledger (`.rsi/ledger.db`), and TPM roots-of-trust sit in a separate capability domain that RSI cannot modify.
6. Promotion uses clean process drain and cutover, never in-memory binary patching.

### Invariant 6: Operator Control Plane via Typed Local Protocol

1. Operators interact with `aien-runtime` via the `aien` CLI over a private local UNIX domain socket.
2. The wire protocol uses strongly typed, versioned envelopes (`ControlEnvelope`, `ControlCommand`). The CLI never accesses runtime databases, KV files, or memory structures directly.
3. Swarm operations are asynchronous. `LaunchSwarm` returns a `SwarmId` immediately; long-running operations are monitored through non-blocking inspection (`aien swarm inspect`) or filtered event subscriptions (`aien swarm watch`).
4. All mutating commands carry an idempotent `OperationId` preventing duplicate execution upon network or shell reconnection.
5. Subscriptions consume events from bounded ring buffers. Telemetry subscribers operate at lower priority than inference and cannot block scheduler steps.
6. The control plane operates on stable domain entities: `SwarmId`, `WorldId`, `SequenceId`, `ObjectId`, `CapabilityId`.

---

## 3. Subsystem Interface Contracts

### 3.1 Sequence Arena & Scheduler Descriptor

```rust
#[repr(C, align(64))]
pub struct SequenceRecord {
    pub id: SequenceId,          // Index (u32) + Generation (u32)
    pub parent_id: Option<SequenceId>,
    pub world_id: WorldId,
    pub priority: u8,
    pub state: SequenceState,    // Ready, Prefill, Decode, BlockedOnTool, Completed
    pub context_id: ContextRevisionId,
    pub block_table_id: u32,
    pub prefill_cursor: usize,
    pub generated_tokens: usize,
    pub arrival_ticks: u64,
    pub deadline_ticks: Option<u64>,
}
```

### 3.2 Transformer Execution Submit/Commit Contract

```rust
pub trait TransformerBatchExecutor {
    type Fence;

    fn submit(
        &mut self,
        model: &ResidentModel,
        plan: &BlackwellBatchPlan,
        kv: &UnifiedKvTensorPool,
    ) -> Result<SubmittedBatch<Self::Fence>, EngineError>;

    fn poll(&mut self, fence: &Self::Fence) -> Result<FenceStatus, EngineError>;

    fn complete(
        &mut self,
        submitted: SubmittedBatch<Self::Fence>,
    ) -> Result<BatchResult, EngineError>;
}
```

### 3.3 Memory and Context Composition Contract

```rust
pub trait MemoryRuntime: Send + Sync {
    fn recall(&self, query: &RecallQuery) -> Result<RecallSet, MemoryError>;
    fn get(&self, id: MemoryId) -> Result<MemoryObject, MemoryError>;
    fn commit(&mut self, memory: NewMemory) -> Result<MemoryId, MemoryError>;
}

pub struct ContextRevision {
    pub id: ContextRevisionId,
    pub parent: Option<ContextRevisionId>,
    pub segments: Arc<[ContextSegment]>,
    pub total_tokens: usize,
}
```

### 3.4 World Manifest Contract

```rust
pub struct WorldManifest {
    pub id: WorldId,
    pub parent: Option<WorldId>,
    pub object_root: ObjectRoot,
    pub filesystem_root: ObjectRoot,
    pub token_root: TokenChainId,
    pub memory_root: MemoryRoot,
    pub capability_set: CapabilitySetId,
    pub effect_log: EffectLogId,
    pub sequence_checkpoint: Option<SequenceCheckpointId>,
    pub created_at: u64,
    pub provenance: ProvenanceId,
}
```

### 3.5 Operator Control Envelope

```rust
pub struct ControlEnvelope {
    pub protocol_version: u16,
    pub request_id: u64,
    pub operation_id: u128,
    pub operator_session: u64,
    pub command: ControlCommand,
}
```

---

## 4. Verification Gate: Runtime Integration Spine

The integration spine milestone verifies that the six frozen boundaries operate in concert on physical hardware:

1. **Workload Definition**:
   - 1 root World initialized with 32,768 shared prefix tokens.
   - Pinned reference weights: TinyLlama-1.1B BF16 / Qwen2.5-7B BF16.
   - 500 child branches forked from root context.
   - 32 decode tokens generated per branch.
2. **Acceptance Criteria**:
   - 500/500 branches complete successfully with verified token parity.
   - Zero CPU fallback events (`fallback_count == 0`).
   - Zero stale `SequenceId` completions committed.
   - KV page physical sharing ratio $\ge 95\%$.
   - Telemetry emitted to CLI and cryptographic receipt committed to `.rsi/ledger.db`.
