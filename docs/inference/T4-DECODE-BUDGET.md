# T4 decode budget: follow-up omega work (spec note, 2026-10-06)

Evidence: GB10 run t4-measure-20261006T2217Z (sovereign-core 328a7e9, omega c0369e6, physics 6d7cf0d). Tags: [M] measured, [I] inferred, [C] read in code.

What B covers [C]: `propose_with_retries` starts its clock at entry (`crates/aien-runtime/src/spine.rs:854`);
attempt 1 runs for what is left of B = 29 000 ms. The declared warm-up runs before the accept loop
(`crates/aien-runtime/src/server.rs:158-162`), so it is outside B. In the T4 daemon log, step 1
(124 prompt tokens, 9 644 ms) is that warm-up line, not T4 [M]. T4's own attempt is step 2 on:
prefill 106 tokens 2 075 ms, then decode 180 to 189 ms per token [M]; about 150 tokens fit in B [M].

Need: 231 tokens = 1 from prefill + 230 decode steps (ACCEPTANCE-v6 Section 3.1).
- Today: 2 075 + 230 x 185 = 44 625 ms [I]. Over B by 15.6 s.
- Limit: d < (29 000 - 2 075) / 230 = 117 ms per token; with a 1 000 ms margin, d <= 112 ms.
- Target for this work: d <= 100 ms and prefill <= 2 075 ms: 2 075 + 23 000 = 25 075 ms (3.9 s margin).

Where decode time goes [I] (m=1 medians, 100 calls, Llama-3.2-1B: 16 layers, 2048/512/8192, vocab 128 256):
| per token | cta 64 | cta 256 |
|---|---|---|
| 16 x (q,o 2x2048x2048 + k,v 2x2048x512 + gate,up 2x2048x8192 + down 8192x2048) | 117.5 ms | 70.4 ms |
| logits 2048x128 256 | 32.0 ms | 12.4 ms |
| matmul total | 149.5 ms | 82.8 ms |
Rest of the 185 ms step (attention, elementwise, host, waits) is about 35 ms. At cta 256 alone:
82.8 + 35 = 118 ms, so 2 075 + 230 x 118 = 29 215 ms: still just over B. The cta 256 switch
(`AIEN_OMEGA_CTA_BUDGET=256`, this PR) needs its own sealed receipt and is not enough by itself.

Work item 1, omega: single-row matmul (m = 1). Today m=1 pads to 16 rows (TILE_M 16, `omega_gpu_matmul_api.h:46`)
and every CTA is one warp (`threads_x = 32`, `omega_gpu_matmul_api.c:169`) walking column tiles of 8. Wanted:
a GEMV kernel with several warps per CTA and split-K (each warp reduces a K slice, then a CTA reduction),
for the five decode shapes above. Goal: matmul total <= 55 ms per token (logits <= 8 ms), parity against the
host oracle on every shape, repeat_mismatch 0.

Work item 2, physics: spin wait. `m16/m16_native.c:105` (physics 6d7cf0d) sleeps `usleep(50)`
inside `m16_native_wait_marker_ge` (line 98) on every marker poll; omega waits through it
(`src/omega_gpu_session.c:52`). About 113 matmul launches per token, plus attention and
elementwise launches, each can overshoot by up to one sleep (UNKNOWN size, not measured).
Wanted: spin on the marker with `yield` for a bounded first window, then fall back to the sleep;
the timeout is unchanged. Measure the per-launch wait before and after on the same five shapes.

Acceptance for both: one sealed GB10 T4 step log (AIEN_STEP_LOG=1) on the frozen path showing
prefill + 230 decode steps < 28 000 ms, plus the parity receipt. No change to B, A or max_tokens 256.
