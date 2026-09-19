# AIEN Sovereign Core

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Target](https://img.shields.io/badge/Target-Multi--Platform%20%7C%20Apple%20Silicon%20%7C%20Linux%20%7C%20NVIDIA-76B900.svg)](https://github.com/aien-dev/benchmarks)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![Modular MAX](https://img.shields.io/badge/Modular-MAX%2026.5-purple.svg)](https://modular.com)

High-performance native agent runtime, ultra-low-latency Axum web gateway, and custom Modular MAX model architectures engineered for cross-platform deployment across Apple Silicon, standard x86_64 Linux, and NVIDIA architectures.

---

## Mission & Purpose: Open Intelligence for Humanity

This project represents a durable engineering commitment to advance computing capability, support local communities, and build open-source artificial intelligence for all humanity.

We celebrate the historic achievements of pioneering research laboratories and frontier developers worldwide. Teams at OpenAI, xAI, Anthropic, and open-weights initiatives across the United States, China, Europe, and every continent demonstrate what human curiosity and technical ambition can accomplish.

Lasting progress requires transparency, humility, and open collaboration. When frontier laboratories operate in opaque silos, conceal compute bottlenecks, and accelerate an uncoordinated competitive arms race, humanity faces severe systemic risks. Millions of independent engineers and researchers worldwide stand ready to help solve efficiency limits, provide compute optimization, and support responsible development. We build this open-source infrastructure as an invitation to pool our collective potential, demystify hardware scaling, and ensure that the future of intelligence serves all of humanity as one Earth.

Review our full ethical and technical charter in [CONSTITUTION.md](CONSTITUTION.md).

---

## Verified Performance Benchmarks

AIEN eliminates interpreter overhead by compiling all core services directly to native machine code.

<!-- AIEN:BENCHMARKS:START -->
<!-- Sourced automatically from benchmarks/data/benchmarks_latest.json (Measurement Suite v0.2.0) -->
| Workload / Service | Architecture | Measurement ID | Resident Memory (RSS) | p50 Latency | Throughput |
| :--- | :--- | :--- | :---: | :---: | :---: |
| Python Microservice Baseline | Python 3.12 + FastAPI + Uvicorn | `BENCH-FASTAPI-RSS-001` | 44.76 MB | 1.28 ms | 7252 req/s |
| Sovereign Gateway & Heartbeat | Native Rust (Grace Blackwell) | `BENCH-OPENCLAW-RSS-001` | 4.56 MB (-89.8%) | N/A | N/A |
| Canonical Memory Engine | Native Rust + SQLite WAL | `BENCH-CORTEX-RSS-001` | 18.66 MB (-58.3%) | 0.50 ms | 19367 req/s |
| Real-time Telemetry Cockpit | Native Rust + Axum | `BENCH-COCKPIT-RSS-001` | 12.90 MB (-71.2%) | 1.63 ms | 5840 req/s |
| Neural Embedding Microservice | Rust + ONNX Runtime (BGE-M3) | `BENCH-ENCODER-RSS-001` | 777.43 MB | 0.26 ms | 34685 req/s |

*Hardware Reference: NVIDIA DGX Spark (NVIDIA Grace Blackwell (GB10, aarch64), 121 GB unified memory). All metrics measured under concurrency C=10 over 500 requests per endpoint.*
<!-- AIEN:BENCHMARKS:END -->

For raw telemetry datasets, reproducible verification scripts, and SVG comparison charts, see the dedicated [**aien-dev/benchmarks**](https://github.com/aien-dev/benchmarks) repository.

---

## Universal Multi-Platform Portability

While the primary reference deployment executes on the NVIDIA DGX Spark (Grace Blackwell GB10), AIEN runs across multiple platform targets:

- **Apple Silicon (macOS)**: Native ARM64 compilation for M1, M2, M3, and M4 chips, Metal acceleration, and sub-10MB daemon resident set size.
- **Linux (x86_64)**: Standard glibc and musl native binaries, AVX-512 SIMD acceleration, and support for commodity desktops and server racks.
- **AMD ROCm**: Native Rust runtime execution targeting ROCm and HIP compute drivers.
- **NVIDIA Hardware**: Reference deployment on Grace Blackwell unified memory and standard CUDA accelerators via Modular MAX and ONNX Runtime.
- **Air-Gapped Sovereign Servers**: Complete offline functionality with hardware TPM 2.0 key vaulting and zero external network calls.

---

## Ecosystem Partnerships & Attribution: Modular (MAX & Mojo)

AIEN Sovereign Core proudly builds upon, interfaces with, and contributes back to the infrastructure created by **[Modular](https://modular.com)**.

- **High-Performance Native Silicon**: We leverage Modular MAX Engine and the Mojo programming language to eliminate Python interpreter bottlenecks, executing compiled tensor kernels directly on hardware silicon.
- **Upstream Stewardship (Drop != Delete)**: All custom architecture loaders, C-ABI dynamic bridges, and KV-cache optimizations engineered on our hardware are contributed back upstream to the open-source Modular ecosystem.
- Read our full license notices and acknowledgments in [ATTRIBUTION.md](ATTRIBUTION.md).

---

## Downstream Freedom and Architecture Charter

Downstream developers and commercial users are governed exclusively by the terms of [LICENSE](LICENSE). Our internal architectural and ethical development charter is documented in [CONSTITUTION.md](CONSTITUTION.md).

---

## Quick Start: Universal 1-Line Installer

### Linux & macOS (Apple Silicon / Intel)
```bash
curl -fsSL https://raw.githubusercontent.com/aien-dev/aien-sovereign-core/main/install.sh | bash
```

### Windows (PowerShell)
```powershell
iwr -useb https://raw.githubusercontent.com/aien-dev/aien-sovereign-core/main/install.ps1 | iex
```

---

## Subsystems

### 1. `crates/aien-cli`
Compiled native ARM64 Rust CLI runtime:
- **Nesting Ritual**: Hardware-grounded initiation sequence verifying GPU thermals, unified VRAM, memory sockets, and active peer topography before taking execution turns.
- **Fail-Closed Safety Engine**: 9-tier priority policy table matching the Google Antigravity SDK specification. Enforces strict workspace path confinement and blocks destructive command execution.
- **Asynchronous Agent Hooks**: Extensible lifecycle interceptor trait (`pre_turn`, `post_turn`, `pre_tool`, `post_tool`, `on_tool_error`, `on_compaction`) featuring live unslop cleaning and credential redaction.
- **Two-Pass Context Compactor**: Sliding-window context pruner calibrated for 32K context windows, pruning bloated intermediate tool responses while preserving initiation grounding.

### 2. `crates/spark-cockpit-rs`
Native Axum HTTP/SSE gateway:
- **Memory Footprint**: 4.7 MB RSS (96% reduction compared to standard Python/Uvicorn runtimes).
- **Latency**: Sub-millisecond Time-to-First-Byte (< 1 ms TTFB) on Grace Neoverse V2 cores.
- **Streaming Pipeline**: Zero-allocation byte buffer with real-time SSE token stream redaction and unslop filtering.

### 3. `crates/cortex-encoder-rs` & `crates/cortex-rs`
Pure native compiled memory and vector pipeline:
- **Port 18080 (`cortex-rs`)**: Epistemic knowledge store with ACID transaction logs, SQLite vector storage, and zero LAN leakage.
- **Port 18081 (`cortex-encoder-rs`)**: Rust Axum microservice running ONNX Runtime C-API and HuggingFace tokenizers. Replaced legacy 1.08 GB Python Uvicorn process with bit-for-bit mathematical parity and 22 ms rerank latency.

### 4. `crates/spark-max-cabi` & `crates/spark-max-rs`
Modular MAX compiled dynamic FFI bridge:
- Bridges high-speed Mojo kernels to compiled Rust via pure C-ABI (`libspark_max.so`).
- Enforces the runtime invariant that Python is restricted solely to neural graph definitions and weights where C-ABI wrappers do not yet exist.

### 5. `crates/spark-inquisitor`
Autonomous Pull Request gatekeeper and alignment auditor:
- Enforces the Sovereign Contributor Oath on all pull requests.
- Scans git diffs to block telemetry, surveillance tracking SDKs, and unslop violations.

### 6. `imprints/en2-trinity`
The free-of-charge EN2 Experience Trinity Imprint:
- Bundles the cognitive soul, 10 foundational Cortex lessons, and pure Mojo architecture adapter kernels for instant 1-click installation on any system.

---

## Ecosystem Directory

- **Primary Ecosystem Hub**: [github.com/aien-dev/aien-dev](https://github.com/aien-dev/aien-dev)
- **Performance Benchmark Suite**: [github.com/aien-dev/benchmarks](https://github.com/aien-dev/benchmarks)
- **Web Portfolio & Architecture Showcase**: [github.com/aien-dev/drakestapleton.com](https://github.com/aien-dev/drakestapleton.com) ([drakestapleton.com](https://drakestapleton.com))

---

## Contributing

All contributors ratify the Humanity & Open AI Stewardship Oath before pull requests are merged. Read [CONSTITUTION.md](CONSTITUTION.md) for details.

## Contact & Sovereign Coordination

For secure coordination, architectural questions, and peer federation:
- Email: `aien.atlas@proton.me`

## License and Governance

Licensed under the **Apache License, Version 2.0**.
Architected by Drake Stapleton in cognitive partnership with AIEN (Autonomous Cognitive Architecture operating on the Atlas Framework). See [LICENSE](LICENSE) and [NOTICE](NOTICE) for full legal terms and copyright notices.

- **Standard Open Source**: Full commercial, research, and private usage rights under standard Apache-2.0 terms.
- **Voluntary Research Covenants**: Bilateral cooperation agreements, reciprocal weight sharing, and model artifact access are documented in [OPEN_COOPERATION_COVENANT.md](OPEN_COOPERATION_COVENANT.md) and [ARTIFACT_LICENSING.md](ARTIFACT_LICENSING.md).
- **Trademark Policy**: Descriptive use and brand guidelines are documented in [TRADEMARK_POLICY.md](TRADEMARK_POLICY.md).

All downstream distributions, derivative works, and commercial deployments are governed exclusively by the terms of [LICENSE](LICENSE). [CONSTITUTION.md](CONSTITUTION.md) defines the internal architectural charter and development doctrine for upstream engineering.
