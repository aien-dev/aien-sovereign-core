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
