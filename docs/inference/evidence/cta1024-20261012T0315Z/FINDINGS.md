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

## Batched prompt-read norms and SwiGLU (05:50-05:52Z, one hold, `N=smoke-ctx-ew ctx-leg.sh`)

Same pins as the leg above; the chunked prompt read now normalizes a chunk's rows in one `rmsnorm_rows` launch per layer (two per
layer: input and post-attention), runs the q/k head norm once per tensor per chunk, and the SwiGLU once per chunk (the fused
gate|up rows are un-interleaved into two planes first). Every reply identical to the leg above (p1, p2, p3, p4), fallback 0.

| op (p4) | calls before | calls after | wall before | wall after |
|---|---|---|---|---|
| rmsnorm | 95905 | 5329 | 3.19 s | 0.52 s |
| rmsnorm_heads | 95832 | 5256 | 1.01 s | 0.51 s |
| swiglu | 47916 | 2628 | 1.26 s | 1.12 s |
| rope (still per token) | 47916 | 47916 | 0.58 s | 0.59 s |

p4 51.6 -> 50.2 s. The remaining elementwise cost is not launches: a chunk SwiGLU (128 x 12288 values, 6 MB in and out) takes about
3 ms, a chunk rmsnorm (2 MB) about 1.4 ms, which is the host-to-chip copy each elementwise call makes on both sides (about 4 GB/s), not
chip time (swiglu chip 0.54 s of 1.12 s wall). Keeping those tensors on the chip between launches is a later cut. The prompt read is
now about 44 s: per-token `gqa_attention` 15.9 s, m=128 matmuls 21.4 s, elementwise 2.7 s.

## Prompt-read attention as one rows call per chunk and layer (06:10-06:12Z, N=smoke-ctx-rows)

omega#406 (`omega_gpu_gqa_attention_rows_f32`, branch head 6b524626, tree be9cad0d) plus the sovereign-core
`TensorBackend::gqa_attention_rows`: the paged prompt read hands each chunk's query rows to the chip in one
call per layer (the KV staged once, rows in launches of 1024 CTAs = 32 rows) instead of one `gqa_attention`
call per token. Same binary otherwise as N=smoke-ctx-ew. Replies identical on p1-p4 (output_ids equal).

| leg | p4 (1268-token prompt + 64) | p4 prompt-read attention | p3 (37 + 256) |
|---|---|---|---|
| smoke-ctx-ew (before) | 50.2 s | gqa_attention 45648 calls, 15.9 s wall, 10.4 s chip | 23.0 s |
| smoke-ctx-rows (after) | 36.4 s | gqa_attention_rows 360 calls, 3.0 s wall, 2.8 s chip | 22.5 s |

The launch and staging overhead is gone (wall - chip 5.6 s -> 0.2 s); what is left is the kernel itself at
1268 rows x causal context (2.8 s). p4 now: the three m=128 prefill matmuls 18.2 s wall (12.2 s chip),
attention 3.0 s, elementwise (swiglu + rope + two rmsnorm families) 2.8 s, decode paged attention 1.0 s.
Next by size: the prefill matmul kernel.

Receipts: smoke-ctx-rows.{start,end,rc,raw,err,json,binary.sha256}; built against omega 6b524626 (omega.lock
at that sha for the leg; re-pinned to the squash-merge sha of omega#406, same tree, before this PR merges).

## Prefill matmuls on the blocked kernel (06:53-06:54Z, N=smoke-ctx-mmblock)

omega#407 (`omega_blackwell_codegen_matmul_frag_block`, branch head 0e11df67): for row counts whose 16-row tile
count divides by 2 or 4, one warp owns 4 row tiles x 4 column tiles of the output over all of K (4x4 at m = 128),
so each loaded weight fragment feeds four tensor ops and each activation fragment four, instead of one each.
Decode rows (m = 1) keep the single-tile kernel byte for byte. No sovereign-core code change: the pin moved.
Same binary source as N=smoke-ctx-rows. Replies identical on p1-p4 (output_ids equal).

| leg | p4 (1268-token prompt + 64) | p4 m=128 matmuls (wall / chip) | p3 (37 + 256) |
|---|---|---|---|
| smoke-ctx-rows (before) | 36.4 s | 18.2 s / 12.2 s | 22.5 s |
| smoke-ctx-mmblock (after) | 23.1 s | 8.9 s / 2.0 s | 23.0 s |

Per shape at p4 (324 calls each): k=12288 n=4096 6.2 s / 3.0 s -> 3.7 s / 0.56 s; k=4096 n=24576 8.8 / 7.2 -> 2.6 / 1.0;
k=4096 n=6144 3.2 / 2.0 -> 1.4 / 0.26; k=4096 n=4096 (not timed before) 1.3 / 0.21. Standalone
(`gpu_matmul_api_test --timing --cta-budget 1024`, median of 100): 128x4096x24576 20.9 -> 5.7 ms, 128x12288x4096
11.7 -> 3.2 ms, 128x4096x6144 5.6 -> 2.0 ms; parity sweep 18/18, mutant killed on every shape (omega#407 receipts).

The chip side of the prefill matmuls is now 2.0 s; the 6.9 s above it is the host side of every m=128 call
(packing the 1 MB activation block into fragment order, poisoning and copying out the f32 result, up to
12.6 MB per call). p4 now: prefill matmul host side 6.9 s, chunk attention rows 3.1 s (2.9 s chip), decode m=1
matmuls ~4.4 s for 64 tokens, elementwise (swiglu, rope, two rmsnorm families) 2.7 s, decode paged attention 1.0 s,
prefill matmul chip 2.0 s. Next by size: the host side of the m=128 matmul calls, then elementwise.

Receipts: smoke-ctx-mmblock.{start,end,rc,raw,err,json,binary.sha256}; built against omega 0e11df67 (omega.lock
at that sha for the leg; re-pinned to the squash-merge sha aa1c4af2 of omega#407, same tree 38409d6d). Host-scope
abi-qualify PASS at the pin: abi-qual-host-aa1c4af2.json (records sovereign-core 4c551f34, the commit before this
evidence file and the crumb data were folded in).

## Prefill activations packed on the host in one pass (08:02-08:03Z, N=smoke-ctx-mmpack)

omega#409 (`omega_matmul_frag_pack_a_f32`, squash 1339759c): the f32 activation rows of every m = 128 prompt-read
matmul call are packed straight into HMMA fragment order with an inline round-to-nearest-even bf16 conversion that is
bit for bit the compiler's cast (checked in `gpu_matmul_api_test --host-only` on ties, denormals, inf and NaN
payloads). Before, gcc 13 on aarch64 emitted the `(__bf16)f` cast as a call to the soft-float routine `__truncsfbf2`
per element, after which the packer did six divisions per element: about 2.5 ms per MB of activations, 9.6 ms of host
time on each 128 x 12288 call whose chip time is 1.7 ms. Kernels and launch shapes are unchanged (omega#407). No
sovereign-core code change: the pin moved. Same binary source as N=smoke-ctx-mmblock. Replies identical on p1-p4
(output_ids equal, same hash b2edfcdb).

| leg | p4 (1268-token prompt + 64) | p4 m=128 matmuls (wall / chip) | p3 (37 + 256) |
|---|---|---|---|
| smoke-ctx-mmblock (before) | 23.1 s | 8.9 s / 2.0 s | 23.0 s |
| smoke-ctx-mmpack (after) | 16.9 s | 3.8 s / 2.1 s | 21.3 s |

Per shape at p4 (324 calls each, wall / chip): k=12288 n=4096 3.7 s / 0.56 s -> 1.08 s / 0.59 s; k=4096 n=24576
2.6 / 1.0 -> 1.73 / 1.00; k=4096 n=6144 1.4 / 0.26 -> 0.54 / 0.27; k=4096 n=4096 1.3 / 0.21 -> 0.42 / 0.21. Host
stopwatch of the packer alone (omega#409 receipts, best of 20): 128 x 4096 2.94 -> 0.68 ms, 128 x 12288 8.82 -> 0.91 ms.

p4 now: chunk attention rows 2.9 s (2.7 s chip), decode m=1 matmuls 4.7 s for 64 tokens (chip 4.6 s, the weight
read near the memory roof), prefill matmuls 3.8 s (2.1 s chip; the remaining host side is the f32 result poison and
copy-out, up to 12.6 MB per call), swiglu 1.1 s (0.53 s chip), decode paged attention 0.92 s (0.47 s chip), rope
0.58 s over 47916 launches, rmsnorm 0.52 s, rmsnorm_heads 0.51 s. The prompt read is now about 10 s of the 16.9 s;
the decode token at this context is about 110 ms, of which the paged-attention call stages the layer's KV slices
into scratch on every call (about 5 MB per layer at 1268 tokens) and the small elementwise launches cost about
90 us of wall each. Next by size for the campaign rows: a resident KV pool for the paged attention (no per-call
staging), then fewer and cheaper elementwise launches per token.

Receipts: smoke-ctx-mmpack.{start,end,rc,raw,err,json,binary.sha256}; built against omega 1339759c (omega.lock and
the native-artifacts.lock lines at that sha). Host-scope abi-qualify PASS at the pin: abi-qual-host-1339759c.json.

Stale archive found at this pin, and the rerun. The first leg (07:45Z, p4 17.1 s, same replies) was linked while the
crate build dir reused across pins still held two compose objects (rx_compose.o, rxc_host_abi.o) compiled at the
previous pin: omega#408 grew a Cortex struct in rx_cortex.h and omega's compose-library rule did not include its
objects' dependency files, so they were not rebuilt. The mixed archive failed abi-qualify's compose step (first
rxc_host_run refused with RX_ERR_AUTHORITY, five callbacks tests Code -4); clean rebuilds of the same commit pass
(archive digest 209b95bd reproduced twice from detached checkouts, now the lock line). The smoke example links none
of the compose objects, so its binary digest is the same before and after (d93f117e) and the leg was rerun against
the clean archive for the record: p1-p4 301, 2546, 21337, 16885 ms. Fixes: omega#410 includes the dependency
files; here, both omega crate build scripts stamp the build dir with the omega commit and wipe it when the stamp
differs (fresh_out_dir in aien-abi-conformance, unit-tested), so a reused dir cannot carry objects across pins.

## Decode attention reads a resident KV pool (08:33-08:34Z, N=smoke-ctx-resident)

omega#411 (squash 809c0aa6): the engine hands out one GPU-uncached, host-visible pool
(`omega_gpu_attention_pool_alloc`) and a paged attention call whose pool pointer is that allocation reads the KV
blocks where they live (slot table = physical block ids, slot stride = block stride, base = pool + layer offset).
The kernel is unchanged; its slot offset math is 32-bit, so the engine checks every referenced block id against
that limit per call and stages as before when it does not fit (`info.kv_source` RESIDENT 1 / STAGED 0; for this
model's geometry block 1819 fits and 1820 does not). Sovereign-core side (this branch): the GPU backend asks the
engine for the pool when the shared KV runtime is built (`TensorBackend::alloc_kv_pool`, a `LinuxMemoryKind::External`
buffer released through the engine), with the host-mapped pool and staged attention as the recorded fallback; the
smoke's pool is 1536 blocks (24k tokens) so every id passes. The timing line now ends with
`kv_staged_bytes_total` and `kv_resident_calls_total`. Replies identical on p1-p4 (output_ids equal).

| leg | p4 (1268-token prompt + 64) | p4 decode paged attention (wall / chip) | p3 (37 + 256) |
|---|---|---|---|
| smoke-ctx-mmpack (before) | 16.9 s | 0.92 s / 0.47 s | 21.3 s |
| smoke-ctx-resident (after) | 17.1 s | 0.61 s / 0.60 s | 21.0 s |

Every decode attention call of the run read the pool in place: `kv_resident_calls_total=12312` (342 decode steps
x 36 layers), `fallback_count` 0. The 2.14 GB of `kv_staged_bytes_total` is the prompt-read rows path
(`gqa_attention_rows`, host-gathered contiguous K/V per chunk), which cut E does not touch. The wall saving on the
2268 p4 decode calls is 0.31 s (about 4.8 ms per token at this context); the chip time of the same calls rose from
0.47 to 0.60 s because the kernel now reads GPU-uncached memory instead of the staging scratch, which gives back
part of the copy saved. The p4 total did not move (16.9 -> 17.1 s): the prompt-read rows attention (2.9 -> 3.1 s,
chip 2.7 -> 2.9 s) and the small m=128 matmuls drifted up by about the same amount while the decode m=1 matmul chip
times match to 0.5%, so that drift reads as run-to-run noise [INFERRED, one leg each]. A GPU-cached pool would need
an L2 invalidate before every read because the host writes the new K/V each token (the E1 trap, nvos.h
`NVOS32_ATTR2_GPU_CACHEABLE`); not taken here.

Decode at 1268 tokens of context is now about 105 ms per token, of which the m=1 weight reads are 73 ms (near the
memory roof) and the elementwise launches (swiglu 1.1 s, rope 0.59 s, rmsnorm 0.52 s, rmsnorm_heads 0.52 s over
64 tokens, about 42 ms per token, each launch about 90 us of wall for a few us of chip time) are the largest
remaining lever. Next: fewer launches per token (q+k rope in one launch, q+k head norms in one launch, one swiglu
launch), and the host side of each launch.

Receipts: smoke-ctx-resident.{start,end,rc,raw,err,json,binary.sha256}; built against omega 809c0aa6 (omega.lock and
the native-artifacts.lock lines at that sha). Host-scope abi-qualify at the pin: abi-qual-host-809c0aa6.json.

## Decode attention split over several CTAs per head (09:14-09:18Z, N=smoke-ctx-split)

omega#412 (squash 247f3522, cut F): the paged attention call no longer walks the whole context with one CTA of
128 threads per query head (32 CTAs per layer call at any context). Each (sequence, head) is split over up to 8
CTAs along the context; each sequence derives its own split count from its own context inside the kernel
(largest power of two <= min(8, chunks / 2), chunks = ceil(ctx / hd)), every CTA writes an unnormalised partial
(acc[hd], running max, running sum) and the host combines the partials in the log2 domain without the math
library. A sequence's result never depends on its batch-mates (extra CTAs of shorter sequences write empty
partials that add exact zeros). ABI unchanged (`OmegaGpuAttnInfo` 176 bytes; `ctas_last_launch` shows the
split). Sovereign-core side: pin only. Output tokens identical on p1-p4 (output_ids equal to smoke-ctx-resident).

| leg | p4 (1268-token prompt + 64) | p4 decode paged attention (wall / chip) | p3 (37 + 256) | p3 decode paged attention (wall / chip) |
|---|---|---|---|---|
| smoke-ctx-resident (before) | 17.1 s | 0.61 s / 0.60 s | 21.0 s | 0.41 s / 0.38 s |
| smoke-ctx-split (after) | 16.4 s | 0.38 s / 0.34 s | 21.1 s | 0.46 s / 0.40 s |

At 1268 tokens of context the kernel takes 4 splits of 384 tokens (128 CTAs per layer call): chip time per call
266 -> 150 us, about 35 GB/s of KV reads instead of 20; per token the decode attention falls from 9.6 to 6.0 ms of
wall (chip 9.4 -> 5.3). The 40 ms between wall and chip on the 2268 p4 calls is the host combine (32 heads x 4
partials of 130 floats, about 18 us per call). At short context (p3, at most 3 chunks, one split) the call pays
the partial write and the combine for nothing: +0.1 ms per token of chip and +0.2 ms of wall over 256 tokens,
run-to-run noise aside [OBSERVED, one leg each]. The other operations match the previous leg to 1%.

Correction to the previous section: the "elementwise launches about 42 ms per token" figure was read from the
p4 table, which also holds the 1268-token prompt read (10 chunks x 36 layers of rope, head norms and swiglu).
Per decode token the elementwise cost is small: on p3 (37 + 256 tokens, decode-dominated) rmsnorm 2.5 ms,
swiglu 1.0, rmsnorm_heads 0.8, rope 0.5 per token, against 73.5 ms of m=1 matmuls (187-220 GB/s against the
273 GB/s roof, about 27 us fixed per launch). The next lever is the m=1 matmul bandwidth, not the launches.

Receipts: smoke-ctx-split.{start,end,rc,raw,err,json,binary.sha256}; built against omega 247f3522 (omega.lock
and the native-artifacts.lock lines at that sha). Host-scope abi-qualify at the pin, clean tree:
abi-qual-host-247f3522.json (PASS, dirty=false). Kernel battery receipts (sim and chip, including the first
version's failures) in the decode-speed investigation folder, receipts/cut-f.
