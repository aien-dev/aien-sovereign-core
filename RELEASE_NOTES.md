# AIEN Sovereign Core v0.1.0 Release Notes

**Status (Drake decision, 2026-10-08): v0.1.0 and v0.1.1 are internal test releases, not public releases.** v0.1.0 carries only `SHA256SUMS.txt` (no signature). v0.1.1 is signed with the old release-signing key (checked 2026-10-08: `ssh-keygen -Y verify` against `docs/release/allowed_signers` gives a good `aien-release` signature), which is not the dedicated offline key a public release requires. See `docs/release/RELEASE_SIGNING.md`.

Initial internal test release of the AIEN Sovereign Native Systems Architecture, establishing a pure compiled runtime for autonomous AI systems across dedicated silicon.

---

## Executive Summary

AIEN enforces a Native Systems Priority: zero Python or Node interpreters in core agent services, gateways, or background daemons. Core runtime daemons execute as compiled native Rust binaries with hardware TPM-bound secret resolution, sub-microsecond memory cache management, continuous batch scheduling, and native neural model execution.

---

## Included Release Binaries

Each platform archive (`sovereign-*.tar.gz` and `sovereign-*.zip`) contains the compiled native binaries:

1. **`aien`**: Sovereign CLI providing host environment probing, system health inspection, and evaluation harnesses.
2. **`aegis-runtime`**: Autonomous agent gateway maintaining a sub-millisecond heartbeat loop in 4.79 MB resident memory.
3. **`cortex-rs`**: Epistemic knowledge graph engine with SQLite WAL persistence, FTS5 lexical search, and vector similarity.
4. **`cortex-encoder-rs`**: High-throughput vector embedding service utilizing ONNX Runtime INT8/FP16 models.
5. **`spark-cockpit-rs`**: Real-time terminal telemetry gateway monitoring host latency distributions and service states.
6. **`spark-supervisor`**: Native watchdog daemon monitoring process lifecycles and executing backoff restarts upon failure.
7. **`spark-debugger`**: Real-time crash diagnostics and core dump inspection daemon.
8. **`bench_inference_stack`**: Active 8-tier performance harness measuring KV allocation, subagent branching, and concurrency pressure sweeps.

---

## Universal Cross-Surface Hardware Support

The runtime includes dynamic silicon probing via `ExecutionSurface::detect()`:

- **NVIDIA DGX Spark (Reference Station)**:
  - Architecture: Grace Blackwell (GB10, aarch64, 121 GB LPDDR5X unified memory).
  - Acceleration: Hardware NVFP4 tensor cores, zero-copy physical KV pools, Modular MAX C-ABI bridge.
- **Apple Silicon (macOS)**:
  - Architecture: Apple M-Series (M1 / M2 / M3 / M4, aarch64, unified memory).
  - Acceleration: POSIX mmap page-locked KV pools, SIMD auto-vectorized loops, deterministic CPU fallback kernels.
- **Generic Linux (x86_64 / aarch64)**:
  - Architecture: Standard AMD/Intel CPUs and ARM servers.
  - Acceleration: Pure native compiled execution, POSIX Copy-on-Write virtual tables, zero external daemon requirements.

---

## Empirical Performance Highlights

- **Control-Plane Memory Footprint**: 4.79 MB to 10.67 MB RSS across core daemons (a 99.6% reduction compared to Python agent frameworks).
- **KV Block Allocation Throughput**: 1.44 million blocks/second (690 ns per allocation).
- **Subagent Sequence Branching**: 1,000 concurrent child sequences forked in 0.41 µs (a 332,810x speedup over physical tensor copying).
- **Copy-on-Write Memory Mutation**: 8.88 ns per distinct token append.
- **Concurrency Pressure Scaling**: Evaluated from C=1 up to C=256 streams sustaining up to 3.12M tokens/sec with 0.49% scheduler overhead.
- **Energy Efficiency**: 10.75W to 10.90W host power draw under load (<0.0001 Joules/token).

---

## Upstream Attribution & Governance

- Pure Mojo LLaMA execution kernels are authored by Audrey (`a730/MojoLlama`).
- Neural graph execution utilizes Modular MAX.
- Licence history: v0.1.0 (tag 2026-09-20) carried the Sovereign Resource Commons License 1.0 (SRCL-1.0): `git show v0.1.0:LICENSE` and its own notes say so. v0.1.1 (tag 2026-10-03) carried Apache-2.0 (`git show v0.1.1:LICENSE`). The Apache wording in this file dates from 2026-09-23 (commit 3419488). The project was relicensed to AGPL-3.0-or-later on 2026-10-04 (commit f571b94, #197); releases cut after that are AGPL-3.0-or-later (`LICENSE`, `NOTICE`, `ARTIFACT_LICENSING.md`). Archives already published keep the terms they carried. Third-party model licences are separate: see `THIRD_PARTY.md`.
- Detailed provenance documentation resides in `docs/PROVENANCE.md`.

---

## SHA-256 Verification

Download the release archive matching your operating system and verify the SHA-256 hash using `SHA256SUMS.txt`:

```bash
sha256sum -c SHA256SUMS.txt
```
