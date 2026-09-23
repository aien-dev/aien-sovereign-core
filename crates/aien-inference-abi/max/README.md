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

The MAX execution graph performs:

1. BF16 router matmul, FP32 top-8 routing and normalization in a Mojo GPU op.
2. Token-to-expert permutation and expert offsets with MAX MoE index ops.
3. Grouped BF16 gate/up GEMM, SwiGLU, grouped BF16 down GEMM.
4. Inverse permutation and weighted reduction to the original token order.

The Rust `MoeBatchPlan` provides a deterministic routing oracle, inverse map
and per-expert telemetry. Its 500-token test covers all assignment slots.

## Verification

Run the synthetic GPU parity test:

```bash
python crates/aien-inference-abi/max/test_qwen3_moe.py
```

With the official checkpoint index and the shard containing layer 0:

```bash
OPENBLAS_NUM_THREADS=1 python crates/aien-inference-abi/max/verify_real_layer.py \
  /path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 --layer 0

OPENBLAS_NUM_THREADS=1 python crates/aien-inference-abi/max/benchmark_moe_layer.py \
  /path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 --layer 0 --tokens 500
```

The real-layer verifier compares MAX output against a CPU calculation that
decodes only the eight selected experts. On GB10, layer 0 with an all-ones
input produced cosine similarity 0.99998766 and p95 relative error 2.13%.
The 500-token layer benchmark touched all 128 experts and measured median
165.2 ms across three runs. These are layer measurements, not full-model
generation results.

## Current boundary

MAX 26.5's block-scaled FP8 grouped GEMM rejects SM121a at graph compile
time, so the GB10 path stages each layer in BF16 device memory before grouped
GEMM. The packed FP8 bytes are consumed directly by the Mojo upload kernel.
Full-model residency, attention, tokenizer integration, Rust scheduler wiring
and token generation are outside this layer implementation. The BF16 staging
would require about twice the FP8 expert weight memory if retained across all
48 layers. A native SM121a grouped FP8 kernel is the next performance step.

Sources: [Qwen3-Coder checkpoint](https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8),
[Modular custom ops](https://docs.modular.com/develop/build-custom-ops),
[MAX grouped FP8 API](https://docs.modular.com/api/mojo/linalg/grouped_matmul_sm100_blockwise_fp8/grouped_matmul_sm100_blockwise_scaled_fp8).
