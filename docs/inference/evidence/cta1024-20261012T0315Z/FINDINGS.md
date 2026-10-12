# Matmul CTA budget 1024 on GB10: Qwen3-8B decode shapes (2026-10-12, hold ade3c837-decode-speed 03:15Z)

Tool: omega `tests/gpu_matmul_api_test --timing` built from omega main 781e98d8 (physics lock pinned, PINS), sha256 ef7ca687...;
shapes `OMEGA_GPU_MATMUL_SHAPES="1,4096,4096;1,4096,1024;1,4096,12288;1,12288,4096;1,4096,151936"` (Qwen3-8B q/o, k/v, gate/up, down,
lm_head at m=1, bf16). Median of 100 calls per shape. Effective GB/s = weight bytes / median. Receipts: `*.json`, logs `*.log`, exit codes `*.rc`.

| shape (k x n) | MB | budget 64 (omega default) | 512 | 1024 |
|---|---|---|---|---|
| 4096x4096 | 33.5 | 0.822 ms, 41 GB/s | 0.211 ms, 159 | 0.212 ms, 158 (grid capped at 512) |
| 4096x1024 | 8.4 | 0.312 ms, 27 | 0.211 ms, 40 (grid 128) | 0.211 ms, 40 (launch floor) |
| 4096x12288 | 100.7 | 2.251 ms, 45 | 0.522 ms, 193 | 0.420 ms, 240 |
| 12288x4096 | 100.7 | 2.154 ms, 47 | 0.526 ms, 191 | 0.526 ms, 191 (grid capped at 512) |
| 4096x151936 | 1245 | 24.3 ms, 51 | 6.31 ms, 197 | 6.91 ms, 180 |

Runs: base-r1..3 (budget 64), cta512-r1..3, cta1024-r1..3, unroll2-r1..3 (control: unroll 2 is no faster than 4, 41 to 58 GB/s),
mutant (wrong kernel, last K step dropped): parity FAIL as required, "mutant killed", rc 0. Parity ok and repeat_mismatch 0 on every timing run.
The lm_head line is reported FAIL by the tool only because its median exceeds the tool's 5 ms ceiling check; its parity passed.

Per generated token, matmul only (36 layers: 2x q/o, 2x k/v, 2x gate/up, 1x down; + lm_head), from the medians above:
64: 36 x 9.12 + 24.3 = 353 ms; 512: 36 x 2.20 + 6.3 = 85.5 ms; 1024: 36 x 2.00 + 6.9 = 79 ms. The daemon served v7 at 256 (not in this
sweep; the 2026-10-07 sweep at 256 gave about 120 GB/s on the 4B shapes, matching the 128 ms/token observed in v7).
The end-to-end check is `ab-smoke.sh` (smoke binary, budget 256 vs 1024, golden ids p1/p2, p3 tok/s): results in smoke-cta*.json.
Hardware figure: 273 GB/s LPDDR5X (https://docs.nvidia.com/dgx/dgx-spark/hardware.html, docs-read 2026-10-12).

## End-to-end check on the real model (smoke binary, 03:30-03:38Z, two holds of under 4 minutes each)

`ab-smoke.sh`: `qwen3_8b_smoke` built from this branch with the pinned GB10 engine (`build-gb10.sh`, sha256 in `smoke-binary.sha256`),
Qwen/Qwen3-8B, greedy, prompts p1/p2 of SMOKE-8B-v1 (golden-checked) plus p3 "Describe how a bicycle works, in about 250 words."
(256 tokens, `AIEN_SMOKE_LONG=1`). Leg 1 `AIEN_OMEGA_CTA_BUDGET=256` (the daemon default until this change), leg 2 unset (new default 1024).

| binary | budget | spin window | p1 (30 prompt tokens, 1 out) | p3 256 tokens | p3 tok/s | decode-only ms/token (p3 minus p1) | p1/p2 vs golden | p3 ids |
|---|---|---|---|---|---|---|---|---|
| nospin-attempt (no spin applied) | 256 | 50 us sleep-poll | 819 ms | 40 223 ms | 6.36 | 155 | EQUAL | A |
| nospin-attempt | 1024 | 50 us sleep-poll | - | 37 825 ms | 6.77 | 145 | EQUAL | A |
| final (`smoke-cta256.json`) | 256 | 2000 us (daemon default) | 819 ms | 30 513 ms | 8.39 | 116 | EQUAL | A |
| final (`smoke-cta1024.json`) | 1024 | 2000 us | 701 ms | 26 117 ms | 9.80 | 100 | EQUAL | A |

fallback_count 0 in every leg; the p3 reply ids are identical across all four legs (one token sequence "A"). The first pair is kept in
`nospin-attempt/` with a note: the smoke example did not yet apply the daemon's spin window, so those legs timed a path the daemon never runs.
The smoke example now applies both daemon defaults (budget and spin) and records them in its JSON (`cta_budget`, `spin_us`).

Verdict: 256 -> 1024 gives +17% decode on the real model (116 -> 100 ms/token), answers unchanged. The 13 tok/s target (77 ms/token) is
not reached by this change alone; the next candidates are fusing the q/k/v and gate/up launches (the k/v shape sits on the launch floor)
and the n=4096 shapes whose grid is capped at n/8 = 512 blocks. `stub-attempt/` is an INVALID record (binary without the engine, rc 3, no chip time).

## Where a decode token's time goes (per-op timing, 03:43-03:47Z, one hold, `timing-leg.sh`)

Same binary plus a per-operation timing table in the GPU backend (wall time around each chip call and the chip's own
launch-to-completion time), reset before p3. Budget 1024, spin 2000 us, fallback 0, p1/p2 equal to the golden, p3 26 666 ms for
37 prompt tokens + 256 generated (255 decode steps after the prefill token). Full table: `smoke-timing-ops.txt`.

| op (per decode step, 36 layers) | calls/step | wall ms/step | chip ms/step | effective GB/s (chip) |
|---|---|---|---|---|
| matmul gate+up 4096x12288 (2/layer, 100.7 MB each) | 72 | 36.4 | 34.0 | 213 |
| matmul down 12288x4096 (100.7 MB) | 36 | 19.5 | 16.6 | 218 |
| matmul q+o 4096x4096 (2/layer, 33.5 MB each) | 72 | 14.2 | 12.2 | 198 |
| matmul k+v 4096x1024 (2/layer, 8.4 MB each) | 72 | 9.3 | 7.4 | 82 (grid 128 blocks: too small to fill the chip) |
| lm_head 4096x151936 (1245 MB) | 1 | 6.6 | 6.5 | 192 |
| paged_attention (gathered: Fp32 pool, host gathers K/V every call) | 36 | 6.6 | 5.2 | grows with context |
| rmsnorm (2/layer + final) | 83 | 2.9 | 2.5 | |
| swiglu | 36 | 1.0 | 0.6 | |
| rmsnorm_heads (q/k norm) | 72 | 0.9 | 0.7 | |
| rope | 36 | 0.5 | 0.3 | |
| sum | 516 | 97.9 | 86.0 | |

Decode wall per step (p3 minus the p1 prompt read, over 255 steps) is 101.8 ms, so the chip calls account for 98 of it; host work outside
the backend is about 4 ms. Matmuls are 86 ms wall (80 chip) for 15.1 GB per token: 176 GB/s average against the 273 GB/s ceiling and
the 240 GB/s the kernel reaches on a well-filled grid. The 13 tok/s target needs 77 ms per step: 25 ms to find.

Prefill (37 tokens) runs attention per token (gqa_attention 1332 = 37 x 36 calls, 46 ms) and the m=37 matmuls at 2 to 4x the m=1 time;
the prompt read is about 0.7 s and is not the bound here.

Next fixes, in order of ms saved: (1) one fused q/k/v matmul per layer (n=6144, 50.3 MB at ~213 GB/s = 0.24 ms vs 0.46 ms now: ~8 ms/step);
(2) one fused gate+up matmul (n=24576: ~6 ms/step at 240 GB/s); (3) bf16 KV pool so attention uses the paged kernel instead of the host
gather (5 ms here, much more on the 700 to 1000-token document tasks of the campaign); (4) split-K for the n=4096 shapes (grid capped at 512).

## Fused q|k|v and gate|up launches (03:51-03:55Z, one hold, `N=smoke-fused-cta1024 timing-leg.sh`)

Same settings (budget 1024, spin 2000 us), the resident loader also uploads one stacked q|k|v matrix (n=6144) and one gate|up matrix
(n=24576) per layer and the forward paths launch those (`smoke-fused-cta1024.json`, binary sha in `smoke-fused-cta1024-binary.sha256`).
p1/p2 equal to the golden, p3 ids identical to every earlier leg, fallback 0. p3 24 260 ms (was 26 666): decode 92.4 ms/step (was 101.8).

| op (per decode step) | before: wall ms | after: wall ms | chip GB/s after |
|---|---|---|---|
| q, k, v -> q\|k\|v (n=6144, 50.3 MB) | 7.1 + 9.3 = 16.4 | 10.1 | 224 |
| gate, up -> gate\|up (n=24576, 201 MB) | 36.4 | 34.8 | 219 |
| o (4096x4096) | 7.1 | 7.2 | 196 |
| down (12288x4096) | 19.5 | 19.6 | 217 |
| lm_head | 6.6 | 6.4 | 199 |
| paged_attention (gathered) | 6.6 | 6.0 | |
| rmsnorm + swiglu + rmsnorm_heads + rope | 5.3 | 5.3 | |
| matmul total | 86.0 | 78.1 | 15.1 GB at 193 GB/s average |

Launches per step 516 -> 408. The two matrices that still lag are the n=4096 shapes (o, down: grid capped at 512 blocks) and the
host-gathered attention. Cumulative today: 116 -> 92 ms/step (8.4 -> 10.8 tok/s on p3), replies unchanged.

## bf16 KV pool: paged attention kernel instead of the host gather (04:02-04:07Z, one hold, `N=smoke-bf16kv-cta1024 timing-leg.sh`)

Same settings and fused launches; the shared KV pool is now stored as bf16 (`SHARED_KV_DTYPE = Bf16` in `aien-runtime/src/shared_kv.rs`),
so decode attention runs `paged_attention_bf16` on the chip instead of gathering an f32 pool on the host, and the pool takes half the
bytes (`smoke-bf16kv-cta1024.json`, binary sha in `smoke-bf16kv-cta1024-binary.sha256`). p1/p2 equal to the golden, fallback 0.
p3 23 275 ms (was 24 260): decode 88.7 ms/step (was 92.4), 11.3 tok/s.

Numerics: K and V are rounded to bf16 when written, so a long greedy reply may take a different branch. p3's 256 ids equal the fused leg
for the first 158 tokens, then diverge at token 159 (both replies stay on topic; the pinned golden p1/p2 are unchanged). This is a
declared numerics change, recorded here; the campaign golden covers p1/p2 and still holds.

| op (per decode step) | before: wall ms | after: wall ms |
|---|---|---|
| attention: gathered f32 -> paged_attention_bf16 (9180 calls) | 6.0 | 4.6 (chip 3.7) |
| gate\|up (n=24576) | 34.8 | 34.6 |
| down (12288x4096) | 19.6 | 19.6 |
| q\|k\|v (n=6144) | 10.1 | 10.1 |
| o (4096x4096) | 7.2 | 7.2 |
| lm_head | 6.4 | 6.7 |
| rmsnorm + swiglu + rmsnorm_heads + rope | 5.3 | 5.2 |

The attention saving grows with context (37-token prompt here; the campaign's document tasks run 700 to 1000 tokens, where the host gather
was linear in context). Cumulative today: 116 -> 88.7 ms/step (8.4 -> 11.3 tok/s on p3). Remaining to the 77 ms target: the n=4096
shapes (o, down: grid capped at 512 blocks, ~5 ms at 240 GB/s), the small elementwise launches (~5 ms), lm_head (~1 ms).

## Long context: where a campaign token's time goes (04:55-05:00Z, one hold, `N=smoke-ctx-cta1024 ctx-leg.sh`)

Same build as the bf16 KV leg plus a fourth smoke prompt (`AIEN_SMOKE_CTX=1`: 32 numbered library rules, 1268 prompt tokens, 64 new
tokens; the campaign's document tasks decode at 700 to 1000 tokens of context). Golden p1/p2 equal, p3 ids identical to the bf16 KV leg,
fallback 0. The smoke's p3 (37-token prompt) does not measure what the campaign pays; p4 does.

| prompt | prompt tokens | new tokens | wall | per decode token |
|---|---|---|---|---|
| p3 | 37 | 256 | 23.2 s | 88.7 ms |
| p4 | 1268 | 64 | 63.7 s (prompt read about 56 s, decode about 7 s) | about 110 ms |

Decode at 1268 context (per token): matmuls about 71 ms (unchanged), `paged_attention_bf16` 2268 calls, 1884 ms wall: 0.83 ms per
call, 30 ms per token (4.6 ms at 37 tokens). The call reads 1268 keys x 8 kv heads x 128 x 2 (K and V) bf16 = 5.2 MB in 0.83 ms:
about 6 GB/s against a 273 GB/s memory. The kernel launches one CTA per q head (32 CTAs of 128 threads, thread t owns token t), so a
warp's K loads touch 32 different token rows per instruction; it is latency and sector bound, not bandwidth bound.

Prompt read of 1268 tokens (about 56 s, 44 ms per prompt token): `gqa_attention` 45 648 calls (one per token per layer) 24.5 s wall;
matmuls at m=128 15.7 s chip (the fragment kernel re-reads B once per 16-row tile: 8x the weight traffic, no reuse, about 1 TFLOP/s);
per-token elementwise launches: rmsnorm 95 905 calls 3.2 s, swiglu 47 916 calls 1.3 s, rmsnorm_heads 95 832 calls 1.0 s, rope 0.6 s.

What this means for the campaign (v7 D1: first reply 95.8 s of the 120 s budget): about 45 s of that is the prompt read and the rest
is decode at about 110 ms per token. The 13 tok/s target from the speed-bound memo was computed on the short-context decode and would
not by itself bring D1 inside its budget. The fixes that move the campaign, in order of size: (1) the attention kernel's memory
access pattern (prompt read 24.5 s -> about 1 s, decode attention 30 ms -> about 2 ms per token at 1000 context); (2) batched
elementwise launches in the prompt read (about 6 s); (3) a prefill matmul kernel with B reuse (15.7 s -> a few s).

## Attention kernel with batched loads, on the real model (05:32-05:34Z, one hold, `N=smoke-ctx-attn ctx-leg.sh`)

Same smoke binary source as the long-context leg, omega pin 6c6180cf -> c74fe6e5 (omega#405: the attention kernel generator issues its
global and shared loads in batches of up to four per scoreboard instead of waiting after each one; between the two pins only the omega
Makefile changed among the GPU library sources). Build from the pinned checkout, fallback 0, CTA budget 1024. Every reply is identical
to the previous leg: p1, p2, p3 (256 tokens) and p4 (64 tokens) output ids equal, so the kernel change is numerically silent here.

| prompt | prompt tokens | new tokens | wall before | wall after |
|---|---|---|---|---|
| p3 | 37 | 256 | 23.2 s | 23.0 s |
| p4 | 1268 | 64 | 63.7 s | 51.6 s |

Attention launches, before -> after (wall, chip in brackets):

| op | prompt | calls | before | after | per call after |
|---|---|---|---|---|---|
| `paged_attention_bf16` (decode at 1268 context) | p4 | 2268 | 1884 ms (1446) | 925 ms (481) | 0.41 ms wall, 0.21 ms chip |
| `paged_attention_bf16` (decode at 37 to 293 context) | p3 | 9180 | 1238 ms (988) | 803 ms (555) | 0.087 ms wall |
| `gqa_attention` (prompt read, one launch per token) | p4 | 45648 | 24.5 s (18.8) | 15.5 s (9.9) | 0.34 ms wall, 0.22 ms chip |

Decode attention at 1268 context went from 30 ms to 14.7 ms per token (chip time 3.0x faster, wall 2.0x: the fixed launch and wait
cost, about 0.2 ms per call, is now half of the wall time, so the next gain in decode attention comes from fewer launches, not a faster
kernel). The prompt read went from about 56 s to about 45 s. [INFERRED, high] decode at campaign context is now about 95 ms per token
(110 ms minus the attention saving); the matmuls (71 ms) dominate again.

Where the 45 s prompt read goes now (p4, wall): m=128 prefill matmuls 20.2 s (8.8 + 6.1 + 3.0 + 2.3; the fragment kernel re-reads B per
16-row tile), per-token `gqa_attention` 15.5 s (9.9 s chip; the rest is the host K/V upload each call makes), per-token elementwise
6.0 s (rmsnorm 3.2, swiglu 1.3, rmsnorm_heads 1.0, rope 0.6). For v7 D1 (95.8 s first reply) this leg predicts about 88 s
[INFERRED, medium]: inside the 120 s budget but leaving under the 30 s a second attempt needs, so v8 stays cut. Next fixes, by size:
(1) a prefill matmul with B reuse (20 s -> a few s); (2) prompt-read attention over the chip-resident bf16 pool in one launch per chunk
per layer instead of one per token (15.5 s -> about 1 s); (3) batched elementwise launches per chunk (6 s -> under 1 s).
