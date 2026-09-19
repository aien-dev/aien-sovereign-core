# AIEN Sovereign Systems API Stability Policy

This document defines the interface stability guarantees, deprecation lifecycles, and semantic versioning commitments for the AIEN Sovereign Native Systems Architecture.

---

## 1. Versioning Commitment

AIEN adheres to Semantic Versioning 2.0.0 (`MAJOR.MINOR.PATCH`):
- **MAJOR**: Incompatible API modifications to Tier 1 contracts.
- **MINOR**: Backward-compatible feature additions, new crate modules, or evolving modifications to Tier 2 interfaces.
- **PATCH**: Backward-compatible bug fixes, performance optimizations, and security patches.

Prior to `v1.0.0`, minor releases (`v0.2.0`, `v0.3.0`) may introduce changes to Tier 2 interfaces as documented below. Tier 1 interfaces maintain stability guarantees across all `v0.x` minor releases.

---

## 2. Interface Stability Tiers

All public APIs, protocols, traits, and RPC routes across AIEN crates belong to one of three tiers:

### Tier 1: Stable Contracts

Tier 1 interfaces define the core interoperability contracts of the ecosystem. External integrations, third-party agents, and downstream tooling may depend on these interfaces with long-term stability guarantees.

**Covered Interfaces:**
1. **Inference Abstraction (`crates/aien-inference-abi`)**:
   - `AienInferenceBackend` async trait: `initialize`, `generate`, `step`, `health_check`.
   - Hardware detection types: `ExecutionSurface`, `CpuTopology`.
   - Structured request and response models: `GenerationRequest`, `GenerationOutput`, `InferenceMetadata`.
2. **Cortex Memory Engine (`crates/cortex-rs`)**:
   - HTTP routes: `POST /api/cortex/write`, `POST /api/cortex/search`, `POST /api/cortex/recall`, `GET /health`.
   - JSON payload contracts: `WritePayload`, `EntityWriteInput`, `ClaimWriteInput`, `CortexReceipt`.
3. **Telemetry & Watchdog Gateways**:
   - `spark-cockpit-rs`: `GET /api/pulse`, `GET /health`.
   - `openclaw-rs`: WebSocket gateway packet frames and auth handshake.
4. **Command Line Interfaces**:
   - `aien`: `--version`, `status`, `eval`.
   - `atlas-vault`: `add`, `get`, `list`, `purge`.

**Guarantee:**
- Breaking changes require a major version increment (`v1.0.0`) or an explicit two-minor-version deprecation window.
- Deprecated items must emit compiler warnings (`#[deprecated]`) stating the migration path and removal target.

---

### Tier 2: Evolving Contracts

Tier 2 interfaces cover internal systems engines that are functional, tested, and benchmarked, but whose exact parameters may be tuned to optimize memory layouts or scheduling efficiency.

**Covered Interfaces:**
1. **Paged KV Cache Engine (`crates/aien-kv-cache`)**:
   - `UnifiedKvTensorPool`: Memory configuration and allocation interfaces.
   - `RadixTree`: Prefix cache lookup and node eviction parameters.
   - `KvBlock`: Internal block representation and reference counting semantics.
2. **Continuous Batching Scheduler (`crates/aien-scheduler`)**:
   - `ContinuousBatchingScheduler`: Step iteration and queue management.
   - `SchedulerConfig`: Prefill chunk size, max batch size, and memory watermark thresholds.
3. **Supervisor and Crash Recovery (`crates/spark-supervisor`)**:
   - Service manifest format and crash backoff calculation curves.

**Guarantee:**
- Patch releases (`v0.1.x`) maintain strict backward compatibility.
- Minor releases (`v0.2.0`, `v0.3.0`) may modify method signatures or configuration fields.
- All breaking changes between minor versions must be accompanied by explicit upgrade notes in `CHANGELOG.md`.

---

### Tier 3: Internal and Experimental Interfaces

Tier 3 interfaces represent private implementation details, dynamic shared library bridges, or emerging hardware adapters.

**Covered Interfaces:**
1. **Dynamic C-ABI Bridge (`crates/spark-max-cabi`)**:
   - Dynamic shared library exports (`libspark_max.so`).
   - C-ABI function pointers: `spark_max_session_create`, `spark_max_forward_dispatch`, `spark_max_session_destroy`.
2. **Direct Hardware Bindings**:
   - Low-level ModelOpt quantization loaders and custom NVFP4 kernel bindings.

**Guarantee:**
- Zero stability guarantees.
- Signatures and symbol names may change across patch releases to align with upstream Modular MAX, Mojo compiler releases, or CUDA driver updates.
- External code should never link directly to Tier 3 FFI symbols; rely instead on Tier 1 `AienInferenceBackend`.

---

## 3. Deprecation and Removal Process

When an interface in Tier 1 or Tier 2 must be replaced:
1. **Deprecation Notice**: The item is marked with `#[deprecated(since = "X.Y.Z", note = "Use NewInterface instead")]`.
2. **Documentation**: Release notes and `docs/API_STABILITY.md` document the recommended replacement.
3. **Grace Period**: The deprecated item remains functional for a minimum of two minor releases (e.g. deprecated in `v0.2.0`, removed no earlier than `v0.4.0`).
4. **Removal**: The code is cleanly removed in the scheduled release.

---

## 4. Hardware and ABI Compatibility Matrix

| Operating System | Target Triple | Tier 1 Support | Tier 2 Support | Tier 3 Support (GPU) |
| :--- | :--- | :--- | :--- | :--- |
| **Linux (NVIDIA DGX Spark)** | `aarch64-unknown-linux-gnu` | Verified | Verified | Active (Mojo/MAX C-ABI) |
| **Linux (Generic x86_64)** | `x86_64-unknown-linux-gnu` | Verified | Verified | Fallback (CPU Kernel) |
| **Linux (Generic aarch64)** | `aarch64-unknown-linux-gnu` | Verified | Verified | Fallback (CPU Kernel) |
| **macOS (Apple Silicon)** | `aarch64-apple-darwin` | Verified | Verified | Fallback (CPU Kernel) |
| **macOS (Intel)** | `x86_64-apple-darwin` | Verified | Verified | Fallback (CPU Kernel) |
| **Windows** | `x86_64-pc-windows-msvc` | Verified | Verified | Fallback (Aligned Heap) |
