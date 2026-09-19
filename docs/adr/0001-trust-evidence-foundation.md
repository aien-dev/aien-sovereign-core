# ADR 0001: Trust and Evidence Foundation

- **Status**: Accepted
- **Date**: 2026-09-19
- **Author**: Drake Stapleton <drake.aien@proton.me>
- **Milestone**: milestone/trust-evidence-foundation (Milestone 1)

## Context

AIEN has matured into a native systems research project. Sustained adoption across research laboratories and enterprise deployments requires an unambiguous foundation of legal trust, empirical evidence, and security terminology.

Prior repository iterations contained semantic tensions:
1. Licensing terms combined software runtime rights with reciprocal weight transparency covenants.
2. Performance claims were maintained manually across multiple documentation files, leading to divergent metrics.
3. Security documentation employed absolute phrases like "zero telemetry" and "all secrets in TPM" that conflicted with local diagnostic observability and cross-platform hardware realities.
4. Portability documentation asserted universal support across platforms without explicit verification evidence.

## Decisions

### 1. Separation of Software Licensing from Model and Research Covenants
Core software runtime crates are licensed under standard Apache License 2.0. Dynamic linking components requiring runtime exception use Apache-2.0 with LLVM Exception. Model weights, training datasets, and benchmarks are classified under distinct artifact licenses in `ARTIFACT_LICENSING.md`. Reciprocal weight sharing, distillation rights, and compute cooperation reside in an optional bilateral agreement (`OPEN_COOPERATION_COVENANT.md`). `CONSTITUTION.md` serves as an upstream architectural charter and does not bind downstream commercial software licensees.

### 2. Benchmark JSON as Canonical Performance Authority
The structured benchmark dataset (`benchmarks/data/benchmarks_latest.json`) is the sole authoritative record for all performance metrics. Numbers cannot be cited in public repositories without a corresponding measurement ID and cryptographic commit anchor in the canonical dataset.

### 3. Elimination of Manually Maintained Benchmark Claims
Documentation must not contain manually edited performance numbers. All performance summaries, tables, and comparative charts must be generated directly from `benchmarks_latest.json` through the `sync-docs` tool. Subsystem documentation must reference canonical benchmark IDs rather than floating figures.

### 4. Normative Security Definition: No Unsolicited Outbound Telemetry
The project standardizes on the technical specification: "No unsolicited outbound telemetry." Core daemons never transmit diagnostic, usage, or profiling data to remote endpoints. Local observability, operational metrics, and performance counters remain under operator control on local interfaces.

### 5. Hardware-Backed Secret Protection at Rest
The project standardizes on the contract: "No persistent plaintext secret storage, with platform-appropriate hardware-backed protection where available." Secrets resolve dynamically in memory via the `SecretProvider` abstraction, supporting TPM 2.0 on Linux, Secure Enclave / Keychain on Apple Silicon, and protected volatile memory. Decrypted credentials are never written to disk.

### 6. Evidence-Based Portability and Verification Levels
Portability claims must cite explicit verification levels defined in `docs/PLATFORM_MATRIX.md`:
- `Architected`: Platform abstraction designed in code.
- `Build Verified`: Compiles cleanly for target triple.
- `Test Verified`: Test suite passes on hardware.
- `Runtime Verified`: Daemons execute sustained workloads on hardware.
- `Benchmark Verified`: Telemetry recorded in canonical benchmark dataset.
- `CI Protected`: Automated regression testing active in CI.
Claims of universal support are replaced with the verified status for each specific target.

## Deliberate Non-Goals (Scope Fence)

To maintain focus and auditable milestones, the following activities are excluded from this milestone:
1. No new inference kernels or GPU mathematical routines.
2. No new scheduler algorithms or KV cache allocation features.
3. No AIEN, MAX, and vLLM comparative benchmark sweeps.
4. No performance optimization work.

## Consequences

- Licensing is standardized and machine-verifiable across package registries.
- Documentation drift is prevented by deterministic generation from benchmark data.
- Security and hardware assertions reflect empirical systems reality.
- Contribution gating enforces verified standards without imposing ideological hurdles.
