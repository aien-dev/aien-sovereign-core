# State of AIEN: Systems Maturity Audit

Date: 2026-09-19  
Platform: NVIDIA DGX Spark (Grace Blackwell GB10, 121 GB LPDDR5X Unified Memory)  
Standard: Brutally Factual Systems Audit. Zero Marketing Language.

## Maturity Classification Scale

1. Concept: Architectural design documented; no functioning implementation.
2. Prototype: Proof-of-concept implementation; incomplete error handling or mock dependencies.
3. Working Locally: Fully implemented and validated on development hardware with unit tests.
4. Benchmarked: Empirically measured under reproducible load with published data.
5. Integrated: Multi-crate or multi-service automated interaction verified end-to-end.
6. Cross-platform Verified: Tested across multiple architectures (aarch64 and x86_64).
7. Production-ready: Hardened for continuous multi-user deployment with zero unhandled panics.

## Component Maturity Matrix

| Component | Status | Maturity Level | Verification Evidence | Known Limitations |
| :--- | :--- | :--- | :--- | :--- |
| **openclaw-rs** | Active | **Integrated** | Sub-millisecond heartbeat, 4.79 MB RSS, Axum HTTP routes active | Single-node gateway; cluster gossip not implemented. |
| **cortex-rs** | Active | **Benchmarked** | 21,262 req/s, 0.45 ms p50 latency, 10.67 MB RSS, SQLite WAL persistence tests | Schema migrations currently manual. |
| **spark-cockpit-rs** | Active | **Benchmarked** | 4,421 req/s, 1.74 ms p50 latency, 10.05 MB RSS | Terminal UI requires ANSI 256-color support. |
| **cortex-encoder-rs** | Active | **Working Locally** | 37,214 health req/s, ONNX BGE-M3 runtime loads on CUDA | Heavy cold-start initialization latency (~1.2s). |
| **atlas-vault** | Active | **Working Locally** | TPM-bound secret resolution, systemd-creds integration | Requires sudo credentials or TPM access during test runs. |
| **aien-kv-cache** | Active | **Benchmarked** | 1.44M blocks/sec allocation, 0.69 µs alloc latency, 8.88 ns CoW append, physical unified pool mmap | Radix tree eviction across memory pressure under heavy fragmentation needs long-duration soak test. |
| **aien-scheduler** | Active | **Benchmarked** | Sub-microsecond batch build overhead (2.2 µs at B=1 to 25.2 µs at B=128), chunked prefill validated | Priority queues currently hold 5 static priority tiers. |
| **spark-max-cabi** | Active | **Working Locally** | C-ABI dynamic library libspark_max.so compiled via Mojo 1.0.0, FFI tests passing | Shared object path must be present in LD_LIBRARY_PATH or binary build dir. |
| **spark-max-rs** | Active | **Working Locally** | Rust bindings to Mojo C-ABI with automatic fallback to simulated latency curves | Full tensor dispatch requires running MAX graph sessions. |
| **aien-inference-abi** | Active | **Working Locally** | Trait contracts, MockInferenceBackend, MaxServingBackend, VllmServingBackend | Live streaming SSE parsing is raw text; structured function calling parsing is pending. |
| **Native Inference Showdown** | Active | **Benchmarked** | Qwen 2.5 7B NVFP4 control plane: 1.89x TTFT speedup, 1.25x ITL speedup, 99.6% control memory reduction | Tensor execution currently routes to MAX local server; standalone direct Mojo weights loader in progress. |

## Deficit Inventory

1. **Versioned Release Missing**: No formal git release tags (such as v0.1.0) with checksummed binary artifacts.
2. **Cluster Multi-Node**: AIEN currently runs exclusively as a single-station sovereign node on Grace Blackwell.
3. **Weight Formats**: Native Mojo tensor loaders currently support unquantized GGUF and BF16; NVFP4 weights require MAX graph compilation or vLLM runtime.
