# AIEN Neural Runtime

**A sovereign agent and inference runtime.** Native Rust workspace: agent CLI and runtime composition, persistent memory, physical KV-cache management, continuous-batching scheduler, inference ABI with Modular MAX bridges, and telemetry surfaces. Primary reference hardware is NVIDIA DGX Spark (Grace Blackwell GB10).

[![License](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![Modular MAX](https://img.shields.io/badge/Modular-MAX%2026.5-purple.svg)](https://modular.com)

---

## What is implemented today

Demonstrated core, each with tests or runnable targets in this repo:

- `crates/aien-cli`: terminal CLI and orchestrator (initiation sequence, fail-closed policy engine, lifecycle hooks, context compaction).
- `crates/cortex-rs` (port 18080) and `crates/cortex-encoder-rs` (port 18081): persistent knowledge store and ONNX embedding service.
- `crates/aien-kv-cache`: unified-memory page pool with copy-on-write branch forking (`fork_sequence`, `append_token`).
- `crates/aien-scheduler`: continuous batching and chunked prefill, including the `bench_inference_stack` measurement binary.
- `crates/aien-inference-abi` and `crates/aien-inference-service`: tensor backend trait with Blackwell GB10 and CPU reference paths, plus request and event contracts.
- Qwen FP8 MoE execution work in `crates/aien-inference-abi` and Mojo kernels.
- Provenance infrastructure for the C1 evidence campaign (see `docs/plans/public-surface-cleanup.md`).

### Experimental and development areas

Visibly not at core status: AEGIS capability boundary, MCP broker integration, supervisor, debugger, cockpit gateway, distillation pipeline, Harvester. Research concepts (RSI, Dream, higher-level Hive abstractions, self-improvement claims, Open Humanity concepts) live in research and roadmap documents, not here.

---

## Supported hardware

- **NVIDIA DGX Spark (Grace Blackwell GB10)**: primary reference platform. Runtime verified.
- **Apple Silicon (macOS)**: validated runtime target for portable components. See verification matrix.
- **Linux x86_64**: validated runtime target for portable components. See verification matrix.
- **AMD ROCm**: experimental and architected only. Runtime qualification is pending.

Full status per component: [PLATFORM_MATRIX.md](docs/PLATFORM_MATRIX.md).

---

## Architecture

Component contracts and runtime flows: [AIEN_RUNTIME_ARCHITECTURE.md](docs/AIEN_RUNTIME_ARCHITECTURE.md). System context: [AIEN_COMPLETE_SYSTEM_ARCHITECTURE_v2.md](docs/AIEN_COMPLETE_SYSTEM_ARCHITECTURE_v2.md).

---

## Installation

Convenience path (builds release binaries from source; this script performs no signature verification):

```bash
curl -fsSL https://raw.githubusercontent.com/aien-dev/aien-sovereign-core/main/install.sh | bash
```

Signed releases with pinned, verified manifests are tracked follow-up work, not yet available.

## Run a real example

```bash
cargo run -p aien-scheduler --bin bench_inference_stack
```

## Run the tests

```bash
cargo test --workspace
```

---

## Measured Results

Publication rule: every headline performance number must resolve to a
reproducible command and evidence artifact in `benchmarks/`. Figures that do
not yet meet that bar are withdrawn below until their artifact bundles exist.
Previously published microbenchmark figures (branch fork latency, COW page
mutation latency, memory sharing ratios) and the service comparison table are
currently withdrawn for regeneration. The previously cited source file
`benchmarks/data/benchmarks_latest.json` does not exist in this repo, and the
`gb10_canonical` artifact directory lives only in the external
[aien-dev/benchmarks](https://github.com/aien-dev/benchmarks) repository.

To regenerate each class of measurement on GB10 hardware:

```bash
cargo run -p aien-scheduler --bin bench_inference_stack
cargo test -p aien-kv-cache --test cow_branching_tests -- --nocapture
cargo run -p bench_apples_to_apples -- --engines aien,max --concurrency 1,2,4,8,16,32,64 --output-dir benchmarks/data
```

No withdrawn figure returns to this section until its artifact bundle carries
commit identity, hardware and environment record, exact command, raw samples,
SHA-256 digests, measurement definition, and reproducibility steps.

---

## Known Limitations

- The fallback counter records only instrumented GPU operations (matmul, batch matmul, BF16 paged attention, logits). RMSNorm, RoPE, SwiGLU, and GQA paths execute on CPU without incrementing it. Per-operation device provenance is C1 work in progress.
- Execution is hybrid CPU and GPU, not exclusively accelerated.
- Apple Silicon, x86_64, and ROCm targets are qualified only as stated in the platform matrix. ROCm is experimental.
- License transition is in flight: Apache-2.0 governs from the migration commit; historical commits remain as recorded.
- The installer is not yet signature-verifying.
- Headline benchmark figures are withdrawn pending regeneration under the evidence standard above.
- Qwen full-runtime qualification is not yet complete.
- The C1 evidence campaign is in progress.

---

## Subsystems in brief

- `crates/aien-cli`: CLI runtime, initiation sequence, policy engine, hooks, compaction.
- `crates/cortex-rs` / `crates/cortex-encoder-rs`: memory store and embedding service.
- `crates/aien-kv-cache` / `crates/aien-scheduler`: page pool with COW branching, batching scheduler.
- `crates/aien-inference-abi` / `crates/aien-inference-service`: backends and service contracts.
- `crates/spark-max-cabi` / `crates/spark-max-rs`: C-ABI bridge between Mojo kernels and Rust.
- `crates/spark-inquisitor`: pull request checks for secrets and telemetry.
- `crates/spark-cockpit-rs`: HTTP and SSE telemetry gateway.

---

## Mission and values

AIEN is built in the open so independent engineers can reproduce, extend, and challenge it. The full statement of values is [CONSTITUTION.md](CONSTITUTION.md), and the nonbinding open-building ask is [COVENANT.md](COVENANT.md): keep foundational advances open. Research motivations live in `docs/`.

## Attribution

Built on [Modular](https://modular.com) MAX and Mojo, NVIDIA Blackwell hardware, and the Rust ecosystem. Full notices: [ATTRIBUTION.md](ATTRIBUTION.md). Upstream improvements are contributed back rather than forked silently.

## Ecosystem

- Benchmark evidence: [github.com/aien-dev/benchmarks](https://github.com/aien-dev/benchmarks)
- Versioned wire specs: `aien-protocols` (versioning in progress)
- Project site: [drakestapleton.com](https://drakestapleton.com)

## Contributing

Read [CONSTITUTION.md](CONSTITUTION.md) and [AGENTS.md](AGENTS.md) before opening a pull request. Every change ships on a branch with verification proof.

## Contact

- Email: `aien@aienos.com`

## License and Governance

This repository is licensed under the **Apache License, Version 2.0 (with LLVM Exception)**. See [LICENSE](LICENSE) and [NOTICE](NOTICE) for full legal terms and copyright notices. Values and anti-enclosure principles live in the nonbinding [COVENANT.md](COVENANT.md), which grants and restricts no legal rights: keep foundational advances open.

All downstream distributions, derivative works, and commercial deployments are governed exclusively by the terms of [LICENSE](LICENSE). [CONSTITUTION.md](CONSTITUTION.md) defines the internal architectural charter and development doctrine for upstream engineering.
