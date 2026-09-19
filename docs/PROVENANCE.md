# AIEN Architectural Provenance and Fork Disclosure

This document catalogs code provenance across the AIEN project ecosystem, distinguishing upstream community forks from original AIEN technological contributions. Systems engineers and outside auditors can trace every component to its source.

## Upstream Forks and External Lineage

The following repositories under the aien-dev organization are forks of external open-source projects:

1. MojoLlama
   - Upstream Source: a730/MojoLlama (authored by Audrey)
   - Scope: Pure Mojo implementation of LLaMA transformer architecture and weight loading.
   - AIEN Relationship: Reference implementation for Mojo tensor kernels and forward passes. AIEN does not claim authorship of the underlying model implementation. AIEN adaptations focus on C-ABI bindings, unified memory cache structures, and integration with the native Rust scheduler.

2. modular
   - Upstream Source: modularml/modular
   - Scope: Official Modular toolchain examples, runtime configurations, and documentation.
   - AIEN Relationship: Upstream tracking fork for MAX and Mojo compiler updates.

3. NuMojo
   - Upstream Source: North-West-Wind/NuMojo
   - Scope: Numerical computing and multidimensional array library for Mojo.
   - AIEN Relationship: Community numerical primitives used for tensor array utility evaluation.

4. awesome-mojo
   - Upstream Source: Curated community ecosystem directory.
   - AIEN Relationship: Reference tracking fork.

## Original AIEN Technological Contributions

The following systems are original architectures authored and maintained within the AIEN project:

1. aien-sovereign-core
   - aien-inference-abi: Asynchronous native Rust inference trait decoupled from runtime engines, supporting continuous batching, chunked prefill, and dynamic scheduling.
   - aien-kv-cache: Sub-microsecond paged KV block-table allocator featuring radix-tree prefix deduplication, zero-copy subagent sequence branching, Copy-on-Write page copying, and physical unified memory pool (libc::mmap, libc::mlock) on NVIDIA Grace Blackwell.
   - aien-scheduler: Token-budgeted continuous batching scheduler with chunked prefill segmentation and watermark-based memory pressure preemption.
   - spark-max-cabi and spark-max-rs: Zero-copy FFI bridge between native Rust orchestrators and compiled Mojo C-ABI dynamic libraries (libspark_max.so).
   - cortex-rs: Canonical memory store with SQLite WAL persistence, entity-relationship graphs, and sub-microsecond prefix indexing.
   - openclaw-rs: Compiled Rust sovereign control gateway, heartbeat daemon, and event multiplexer.
   - spark-cockpit-rs: Real-time high-frequency hardware monitor, thermal sensor aggregator, and terminal UI.
   - cortex-encoder-rs: Native ONNX Runtime microservice for BGE-M3 text embeddings.
   - atlas-vault: Hardware TPM-bound secret resolution layer, enforcing zero plaintext credentials on disk.

2. benchmarks
   - Native Rust automated benchmark suite measuring process RSS, HTTP throughput, database latency, and neural inference metrics on NVIDIA Grace Blackwell GB10 hardware.

## Attribution Protocol

When citing performance or architectural innovations:
- Attribute LLaMA pure Mojo execution kernels to a730/MojoLlama.
- Attribute Modular MAX compiler and graph execution to Modular Inc.
- Attribute continuous batching, paged unified KV block allocation, subagent zero-copy branching, TPM vaulting, and native Rust daemon infrastructure to AIEN.
