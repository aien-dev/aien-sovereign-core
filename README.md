# AIEN Sovereign Core

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Target](https://img.shields.io/badge/Target-Grace%20Blackwell%20GB10-76B900.svg)](https://www.nvidia.com)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![Modular MAX](https://img.shields.io/badge/Modular-MAX%2026.5-purple.svg)](https://modular.com)

High-performance native agent runtime, ultra-low-latency Axum web gateway, and custom Modular MAX model architectures engineered for the NVIDIA DGX Spark (Grace Blackwell GB10 unified memory architecture).

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

### 3. `modular/nemotron_h_kvexp`
Custom Modular MAX architecture loader and weight adapters:
- Implements KV-cache expansion and weight adapters for serving `nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16` on Grace Blackwell GB10 hardware.
- Engineered for direct upstream contribution to the open-source Modular ecosystem.

### 4. `skills/`
Dynamic YAML-frontmatter runbooks:
- `gpu-telemetry`: High-frequency GB10 thermals, power draw, and unified memory metrics.
- `radicle-sync`: Sovereign P2P code synchronization without centralized forge lock-in.
- `scaffold-project`: Autonomous directory initialization with mandatory lineage crumbs.
- `unslop`: Strict technical writing standard banning em dashes and formulaic AI tropes.
- `modular-upstream`: Upstream contribution and verification protocol for Modular (Mojo/MAX).

---

## License

This project is licensed under the Apache License, Version 2.0. See the [LICENSE](LICENSE) file for details.
