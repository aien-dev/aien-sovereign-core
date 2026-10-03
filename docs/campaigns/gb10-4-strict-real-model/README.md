# GB10-4: strict real-model execution (AIEN 0.1 commissioning)

Rule: a production build fails closed. No reference weights for a missing checkpoint, no silent CPU execution behind a requested GPU backend, and a GPU operation that falls back to CPU is fatal. Dev and CI runs opt in explicitly (cargo feature `dev-fallback` or `AIEN_DEV_FALLBACK=1`); the opt-in is recorded in every receipt and the acceptance gate rejects it.

Gate: `crates/aien-inference-runtime/tests/strict_real_model.rs` (ignored test `strict_real_model_gate`, needs `AIEN_E2E_CHECKPOINT` and `AIEN_STRICT_RECEIPT`). Receipt records checkpoint sha256, tokenizer sha256, backend identity, model config, fallback_count, dev opt-in, verdict.

Run 002 on the DGX Spark, 2026-10-03 (`receipt-run002.json`, `run002.summary.txt`): surface NVIDIA GB10, backend `BlackwellGb10Backend (NVIDIA GB10 sm_121 cuBLAS)`, TinyLlama-1.1B-Chat-v1.0, prompt "The DGX Spark is a small computer with a large" -> "amount of processing power. It is designed", fallback_count 0, verdict PASS. [OBSERVED]

Limit: the GB10 backend in this crate is CUDA/cuBLAS; the project decision to keep CUDA out of AIEN (comparison runs only) means the native nvrm/Omega-engine backend is a separate open item. This gate measures strictness and binding, not the backend's provenance.
