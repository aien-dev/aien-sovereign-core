# AIEN Platform Compatibility Matrix

This document maintains the empirical verification status of AIEN components across target hardware architectures and operating systems.

AIEN follows an evidence-based verification standard. Platforms are labeled according to physical test artifacts rather than theoretical compatibility.

---

## 1. Maturity Level Definitions

| Maturity Tier | Definition |
| :--- | :--- |
| **Architected** | Code path and platform abstraction exist in the repository by design. |
| **Build Verified** | Source code compiles successfully on the target toolchain with zero warnings or errors. |
| **Test Verified** | Unit and integration test suites pass on physical or virtual target machines. |
| **Runtime Verified** | The component has been deployed and executed against live physical silicon. |
| **Benchmark Verified** | Reproducible empirical performance datasets exist in `benchmarks/data/` for this target. |
| **CI Protected** | Automated continuous integration pipelines test the target on every pull request. |

---

## 2. Component Verification Matrix

| Target Architecture / Platform | Kernel & Environment | Build | Runtime | Benchmark | CI Protected | Status |
| :--- | :--- | :---: | :---: | :---: | :---: | :--- |
| **NVIDIA DGX Spark (GB10)** | Linux 6.8+ / Grace Blackwell LPDDR5X | **Yes** | **Yes** | **Yes** | **Yes** | **Primary Reference Platform** |
| **Apple Silicon (macOS aarch64)** | macOS 15+ / M1, M2, M3, M4 | **Yes** | **Yes** | **Partial** | **Yes** | **Validated Runtime Target** |
| **Linux x86_64 (Generic)** | Ubuntu 22.04+, Debian 12+, Fedora | **Yes** | **Yes** | **Partial** | **Yes** | **Validated Runtime Target** |
| **AMD ROCm (Instinct / CDNA)** | ROCm 6.0+ / HIP Compute | **Yes** | **Pending** | **No** | **Pending** | **Architected & Implemented** |
| **Windows x86_64** | Windows 11 / MSVC Toolchain | **Yes** | **Pending** | **No** | **Yes** | **Build Verified** |

---

## 3. Subsystem Breakdown by Platform

### NVIDIA DGX Spark (Grace Blackwell GB10)
- `aien-kv-cache`: Pinned contiguous page-locked unified memory (GB10 LPDDR5X): **Runtime Verified**
- `aien-scheduler`: Continuous batching and chunked prefill: **Runtime Verified**
- `spark-max-cabi`: Modular MAX C-ABI dynamic bridge (`libspark_max.so`): **Runtime Verified** (Tested against MAX 26.5)
- `spark-cockpit-rs`: Axum web gateway and health endpoints: **Runtime & Benchmark Verified**

### Apple Silicon (macOS aarch64)
- `cortex-rs` & `aegis-runtime`: Native binary execution: **Runtime Verified**
- `spark-inquisitor`: Gatekeeper and diff auditor: **Test & Runtime Verified**
- Local hardware secret protection: Supported via Secure Enclave and Keychain APIs.

### AMD ROCm
- Native Rust execution path compiles with HIP bindings.
- Dedicated multi-GPU validation runs on physical cluster hardware are scheduled before upgrading to Runtime Verified.
