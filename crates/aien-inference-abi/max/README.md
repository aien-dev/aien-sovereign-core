# Qwen3-Coder-30B-A3B MoE layer on Mojo and MAX

This directory contains an executable MoE layer for the official
`Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8` checkpoint. It is an accelerator
implementation, separate from the existing dense FP32 executor.

The checkpoint reader validates safetensors names, dtypes, shapes and byte
ranges against the Qwen3-Coder-A3B layer contract. It mmaps one shard and
packs 128 experts into FP8 E4M3 gate/up and down tensors. Router and 128x128
inverse scales stay BF16. The Mojo upload kernel reads FP8 bytes and BF16
scales on the GPU. It creates BF16 resident expert buffers once per layer,
without constructing a complete FP32 model copy.

The MAX execution graphs perform:

1. BF16 router matmul, FP32 top-8 routing and normalization in a Mojo GPU op.
2. Token-to-expert permutation and expert offsets with MAX MoE index ops.
3. Grouped gate/up GEMM, SwiGLU, and grouped down GEMM. The native FP8 graph
   quantizes activations per 128 channels and reads packed FP8 expert weights
   directly; the baseline graph uses resident BF16 expert buffers.
4. Inverse permutation and weighted reduction to the original token order.

The Rust `MoeBatchPlan` provides a deterministic routing oracle, inverse map
and per-expert telemetry. Its 500-token test covers all assignment slots.

## Verification

Run the synthetic GPU parity test:

```bash
python crates/aien-inference-abi/max/test_qwen3_moe.py
python crates/aien-inference-abi/max/test_fp8_grouped_gemm.py
```

With the official checkpoint index and the shard containing layer 0:

```bash
OPENBLAS_NUM_THREADS=1 python crates/aien-inference-abi/max/verify_real_layer.py \
  /path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 --layer 0 --fp8 --seed 1 --tokens 8

OPENBLAS_NUM_THREADS=1 python crates/aien-inference-abi/max/benchmark_moe_layer.py \
  /path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 --layer 0 --tokens 500 --fp8
```

The real-layer verifier compares MAX output against a CPU calculation that
decodes only the eight selected experts. On GB10, the native FP8 graph at
layer 0 with eight seeded random tokens produced cosine similarity 0.99977535
and p95 relative error 0.732%. At 500 tokens, the native FP8 layer measured
43.529 ms versus 165.718 ms for the BF16 baseline, a 3.81x speedup. The
full batch curve and receipt are in [FP8_GROUPED_KERNEL.md](FP8_GROUPED_KERNEL.md).
These are layer measurements, not full-model generation results.

## Current boundary

MAX 26.5's block-scaled FP8 grouped GEMM rejects SM121a at graph compile
time. The native Mojo kernel executes FP8 MMA on GB10 and keeps expert weights
packed, using 603,979,776 bytes for one layer's expert matrices versus
1,207,959,552 bytes for BF16 staging. Full-model residency, attention,
tokenizer integration, Rust scheduler wiring and token generation are outside
this layer implementation. The checkpoint ingestion and graph driver remain
Python experiments pending integration with the Rust ModelCapsule path.

Sources: [Qwen3-Coder checkpoint](https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8),
[Modular custom ops](https://docs.modular.com/develop/build-custom-ops),
[MAX grouped FP8 API](https://docs.modular.com/api/mojo/linalg/grouped_matmul_sm100_blockwise_fp8/grouped_matmul_sm100_blockwise_scaled_fp8).
