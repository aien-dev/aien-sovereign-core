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

This primitive expects activations quantized to E4M3 in 128-channel blocks
with a corresponding inverse scale. Activation quantization and integration
with the layer graph are separate work. The standalone GPU parity test is
`test_fp8_grouped_gemm.py`; it checks ragged expert offsets, every MMA
fragment position, both K blocks, and distinct weight and activation scales.
The test is required before using the kernel for model inference.

Reference: [PTX m16n8k32 fragment layout](https://docs.nvidia.com/cuda/parallel-thread-execution/index.html#matrix-fragments-for-mma-m16n8k32).
