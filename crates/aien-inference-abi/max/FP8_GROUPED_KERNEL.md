# Grouped FP8 expert GEMM prototype

`kernels/qwen3_fp8_gemm.mojo` registers `aien.qwen3_moe.grouped_fp8_gemm`.
It consumes grouped FP8 E4M3 activations `[assignments, K]`, FP32 inverse
activation scales `[assignments, K/128]`, checkpoint FP8 E4M3 weights
`[experts, N, K]`, BF16 inverse weight scales `[experts, N/128, K/128]`,
and expert offsets `[experts+1]`. It returns FP32 `[assignments, N]`.
The weights remain packed FP8 throughout. One warp owns one expert and eight
output channels, walks that expert's ragged token rows in groups of 16, and
uses `mma.sync.aligned.m16n8k32.row.col.f32.e4m3.e4m3.f32`. The result of
each 128-channel K block is multiplied by the corresponding activation and
weight inverse scales before accumulation. Both Qwen gate/up and down
projections fit this contract.

`kernels/qwen3_fp8_quantize.mojo` quantizes grouped FP32 activations in
128-channel blocks and writes FP32 inverse scales. The FP8 layer graph in
`qwen3_moe.py` connects it to both expert projections, with SwiGLU between
them. It consumes packed FP8 checkpoint weights directly and does not create
BF16 resident expert matrices.

The standalone GPU parity test is `test_fp8_grouped_gemm.py`. The complete
synthetic layer test is in `test_qwen3_moe.py`. `verify_real_layer.py --fp8`
checks the official layer against an independent CPU expert calculation and
checks GPU expert IDs against the GPU router logits. The available official
checkpoint shard contains layer 0.

## GB10 layer-0 batch curve

Measured September 23, 2026 with the official
`Qwen3-Coder-30B-A3B-Instruct-FP8` layer-0 shard, MAX 26.5, driver
580.173.02, one warmup and three timed runs per point. Inputs use seed 42.
Values are median milliseconds for one routed MoE layer, including routing,
permutation, both expert projections, activation, and inverse reduction.

| Tokens | Native FP8 ms | BF16 grouped ms | FP8 speedup |
| ---: | ---: | ---: | ---: |
| 1 | 1.086 | 1.131 | 1.04x |
| 8 | 5.451 | 4.626 | 0.85x |
| 16 | 8.682 | 7.986 | 0.92x |
| 32 | 11.581 | 12.011 | 1.04x |
| 64 | 13.328 | 22.336 | 1.68x |
| 128 | 15.969 | 43.875 | 2.75x |
| 256 | 24.992 | 86.080 | 3.44x |
| 500 | 43.529 | 165.718 | 3.81x |
| 1024 | 77.416 | 338.866 | 4.38x |

The [benchmark receipt](receipts/gb10_qwen3_a3b_layer0_fp8_curve_2026-09-23.json)
records the checkpoint, source and input digests, hardware and software
versions, and all timed samples. The native FP8 path stays in packed weight
storage. Its advantage begins at
roughly 32 tokens in this curve. These are one-layer results, not model token
generation rates. Attention, scheduler integration, and full-model residency
remain unmeasured. The three-run medians do not establish run-to-run variance.

Reference: [PTX m16n8k32 fragment layout](https://docs.nvidia.com/cuda/parallel-thread-execution/index.html#matrix-fragments-for-mma-m16n8k32).
