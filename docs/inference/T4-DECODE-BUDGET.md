# T4 decode budget: measured result and optional follow-up (spec note, 2026-10-06)

Evidence: t4-measure-20261006T2217Z (before) and `docs/inference/evidence/t4fix-20261006T2254Z/` (after,
sovereign-core 8c5294c, omega c0369e6, physics 6d7cf0d). Tags: [M] measured, [I] inferred, [C] read in code.

What B covers [C]: `propose_with_retries` starts its clock at entry (`crates/aien-runtime/src/spine.rs:854`);
attempt 1 runs for what is left of B = 29 000 ms. The declared warm-up runs before the accept loop
(`crates/aien-runtime/src/server.rs:158-162`), so it is outside B. T4 needs 231 tokens = 1 from prefill
+ 230 decode steps (ACCEPTANCE-v6 Section 3.1): prefill + 230 x d < 29 000 ms, d < 117 ms at 2 075 ms prefill.

Result, one T4 launch per budget, AIEN_STEP_LOG=1 [M] (FINDINGS.md there):
| | cta 64 (omega default) | cta 256 |
|---|---|---|
| warm-up (outside B) | 10 508 ms | 9 434 ms |
| T4 prefill | 2 102 ms | 1 660 ms |
| decode mean | 184.7 ms/token | 89.7 ms/token |
| S3 wall | 29 094 ms (timeout) | 23 678 ms, S4-S6 PASS, docs/GUIDE.md 24 lines |
Prefill shapes (106 rows) at 256: 100/100 calls each, repeat_mismatch 0, e2e bad 0/0 [M].
The worker's estimate from m=1 matmul medians was 118 ms/token at 256 [I]; the measured 89.7 is faster,
reason UNKNOWN. Decision (orchestrator): the AIEN daemon default is 256 (`AIEN_OMEGA_CTA_BUDGET` unset);
omega's library default stays 64. Limits: one launch each; token identity 64 vs 256 not compared (64 timed
out); ACCEPTANCE-v7 row R1 (GB10 vs CPU reference ids) checks it.

Margin at 256 [I]: 1 660 + 230 x 89.7 = 22 291 ms of backend time, about 6.7 s under B.

Optional follow-up (no longer needed for T4; more margin, longer replies):
1. omega: single-row matmul (m = 1). Today m=1 pads to 16 rows (TILE_M 16, `omega_gpu_matmul_api.h:46`)
   and every CTA is one warp (`threads_x = 32`, `omega_gpu_matmul_api.c:169`) walking column tiles of 8.
   Wanted: a GEMV kernel with several warps per CTA and split-K (each warp reduces a K slice, then a CTA
   reduction) for q/o, k/v, gate/up, down and logits; parity against the host oracle, repeat_mismatch 0.
2. physics: spin wait. `m16/m16_native.c:105` (physics 6d7cf0d) sleeps `usleep(50)` inside
   `m16_native_wait_marker_ge` (line 98) on every marker poll; omega waits through it
   (`src/omega_gpu_session.c:52`). About 113 matmul launches per token [I], each can overshoot by up to
   one sleep (size UNKNOWN). Wanted: spin with `yield` for a bounded first window, then the sleep; timeout
   unchanged. Measure the per-launch wait before and after.
Acceptance for either: a sealed GB10 T4 step log on the frozen path plus the parity receipt.
No change to B, A or max_tokens 256.
