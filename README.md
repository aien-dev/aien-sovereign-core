# aien-sovereign-core

The earlier **Linux-hosted** AIEN runtime, written in Rust: agent CLI, persistent memory (Cortex), a unified-memory KV-cache with copy-on-write branching, a continuous-batching scheduler, and an inference ABI with Modular MAX and Mojo bridges. It runs on top of Linux on the NVIDIA DGX Spark (Grace Blackwell GB10) and is the reference for what the native stack must beat.

**This repository is legacy.** The project's target language is C, and AIEN's own stack (the [aienos](https://github.com/aien-dev/aienos) kernel, the [omega](https://github.com/aien-dev/omega) runtime, [physics](https://github.com/aien-dev/physics) FORGE) is replacing it. No new Rust is added here, with one declared exception: the DEV-MODEL-0 program (a local development model on AIEN-owned inference) may temporarily extend this Rust substrate while the C replacements are built. Component ownership (for example the Cortex now owned by omega, per ADR 0022) is recorded in [aien-architecture](https://github.com/aien-dev/aien-architecture); here `crates/cortex-rs` is non-authoritative and to be retired, not ported.

## Current state

Research-grade and pre-alpha. Crates build and have unit tests on the DGX Spark; execution is hybrid CPU and GPU through Modular MAX and Mojo kernels. Modular MAX serving is being retired model by model, as AIEN's own native path becomes faster for that model. Nothing here is qualified on hardware as a finished system. Live status and sequencing: [CURRENT_EXECUTION_PLAN.md](https://github.com/aien-dev/aien-architecture/blob/main/CURRENT_EXECUTION_PLAN.md) and the [implementation status snapshot](https://github.com/aien-dev/aien-architecture/blob/main/docs/02-implementation-status.md). Component detail and platform status: [docs/PLATFORM_MATRIX.md](docs/PLATFORM_MATRIX.md), [docs/AIEN_RUNTIME_ARCHITECTURE.md](docs/AIEN_RUNTIME_ARCHITECTURE.md).

Known limits: the GPU fallback counter covers only instrumented operations, so execution is not exclusively accelerated; Qwen full-runtime qualification is not complete; the C1 evidence campaign is in progress.

## Measured results

Every headline performance number must resolve to a reproducible command and an artifact bundle (commit identity, hardware and environment record, exact command, raw samples, SHA-256 digests, measurement definition, reproducibility steps). Earlier figures (branch fork latency, COW page mutation latency, memory sharing ratios, service comparison table) are withdrawn until regenerated to that standard. The evidence bundles live in [aien-dev/benchmarks](https://github.com/aien-dev/benchmarks).

## Standing rules

C is the target language, with assembly only where measured; no new Rust, and existing Rust is legacy. No new Python. The few existing `.py` files here (helper scripts and Modular experiment code) are legacy and are to be rewritten in C or shell. No CUDA toolkit and no new CUDA dependence. No systemd in AIENOS. No outside dependencies in the trusted base. Mojo is the default for kernels, not dogma: closest to the metal, fastest wins, beat it if you can, build what is missing.

## Build and test

Needs a Rust toolchain (1.85 or newer) on the DGX Spark or another Linux machine.

```bash
git clone https://github.com/aien-dev/aien-sovereign-core
cd aien-sovereign-core
cargo test --workspace
cargo run -p aien-scheduler --bin bench_inference_stack   # measurement binary
```

`install.sh` builds release binaries from source but performs no signature verification; signed releases are not yet available, so prefer building from a clone.

## Contributing

Read [CONSTITUTION.md](CONSTITUTION.md) and [AGENTS.md](AGENTS.md) before opening a pull request. Changes ship on a branch with verification proof: the commands you ran and their output. Prefer work that moves capability into the C repositories over new Rust here. Contact: aien@aienos.com.

## License and values

Apache License 2.0 with LLVM Exception: see [LICENSE](LICENSE) and [NOTICE](NOTICE). Values live in the nonbinding [COVENANT.md](COVENANT.md): keep foundational advances open. It grants and restricts no legal rights. Built on [Modular](https://modular.com) MAX and Mojo and NVIDIA Blackwell hardware; full notices in [ATTRIBUTION.md](ATTRIBUTION.md).
