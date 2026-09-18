# AIEN Sovereign Core (NVIDIA DGX Spark / Grace Blackwell GB10)

Autonomous Intelligence & Execution Node (AIEN) on NVIDIA DGX Spark hardware.

## Architecture

- **`crates/aien-cli`**: Native ARM64 Rust CLI runtime. Features the Nesting Ritual grounding protocol, a 9-tier priority fail-closed SafetyEngine, asynchronous AgentHooks, and two-pass sliding window ContextCompactor.
- **`crates/spark-cockpit-rs`**: Ultra-low-latency Axum web gateway. Runs with 4.7 MB RSS and sub-millisecond TTFB, providing streaming SSE token batching with real-time unslop and TPM vault redaction.
- **`modular/nemotron_h_kvexp`**: Custom Modular MAX architecture loader for Nemotron 30B KV-cache expansion and weight adapters.
- **`skills/`**: Sovereign skill registry (`gpu-telemetry`, `radicle-sync`, `scaffold-project`, `unslop`, `modular-upstream`).
- **`doctrine/`**: Sovereign operational doctrine and unslop technical voice invariants.
