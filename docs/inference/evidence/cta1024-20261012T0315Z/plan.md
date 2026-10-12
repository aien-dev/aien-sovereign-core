# Plan (state it before acting; record deviations below)
Question: Qwen3-8B decodes at 7.8 tok/s on GB10 (128 ms/token, ~120 GB/s effective over 15.1 GB of bf16 weights; ~/handoffs/2026-10-12-qwen3-8b-speed-bound.md). The chip's memory moves 273 GB/s (docs-read, DGX Spark hardware page). Target 13 tok/s = 77 ms/token = ~196 GB/s. Where is the current fragment-major matmul kernel (omega #330, unroll 4, CTA budget 256) on the 8B shapes, and does more memory-level parallelism move it toward the target?
Frozen candidate: omega origin/main (PINS), tests/gpu_matmul_api_test --timing, shapes of Qwen3-8B (hidden 4096, kv 1024, ffn 12288, vocab 151936): 4096x4096, 4096x1024, 4096x12288, 12288x4096, 4096x151936 at m=1.
Camera: the tool's JSON receipt (median of 100 calls per shape, parity flag). On a hang the process latches by design; the 20-minute hold expires on its own; nothing is killed. Values I cannot interpret are written UNKNOWN.
Negative control: --unroll 2 must be slower than unroll 4 (the instrument can say "no"); parity must stay true on every variant; the mutant mode must FAIL parity.
BASELINE then VARIANTS (one variable each, 3 repeats): baseline unroll 4 cta 256 x3; V1 cta 512 x3; V2 cta 1024 x3; V3 unroll 2 x3 (control). Unroll 8 is NOT run (known to never complete, omega#328/#330 record).
Decision after the data: per-shape GB/s and the per-token sum over 36 layers (+ lm_head); if the sum at the best variant is still > 77 ms, the next cut is a double-buffered pass (loads for pass i+1 issued before the HMMA chain of pass i, alternating scoreboards) in omega_blackwell_codegen.c, red-before-green on the parity fixtures.
## Deviations
## Result (CLAIMS tagged, LIMITS, WHY EASY TO GET WRONG, STATE AFTER, NEXT QUESTION)
