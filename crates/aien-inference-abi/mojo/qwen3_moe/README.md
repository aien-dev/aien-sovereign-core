# Qwen3-Coder-30B-A3B routed expert layer on GB10

One routed MoE layer of `Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8`, executed
entirely in Mojo on the GB10 with packed FP8 expert weights. Rust loads the
checkpoint, owns the tensor contract, and calls the layer through a C ABI.
There is no Python and no MAX graph API anywhere in the path.

```text
official sharded safetensors
      -> load_qwen3_fp8_moe_layer (Rust): index + header + byte-range validation
      -> ModelCapsule: typed FP8/BF16 tensor views, capsule digest
      -> libaien_qwen3_moe.so (Mojo), per call:
           router logits (FP32 accumulate)
           top-8 routing, non-finite logits fail closed
           stable counting sort by expert (MoeBatchPlan contract)
           gather, FP8 E4M3 activation quantization (per row, 128 channels)
           grouped FP8 gate/up GEMM (mma.sync m16n8k32 e4m3)
           SwiGLU, FP8 quantization, grouped FP8 down GEMM
           weighted inverse reduction to token order, BF16 output
```

## Build and test

```bash
crates/aien-inference-abi/mojo/qwen3_moe/build.sh

# Kernel, routing, fail-closed and synthetic-layer parity (seconds).
AIEN_REQUIRE_GPU_TESTS=1 cargo test -p aien-inference-abi --test qwen3_moe_gpu

# Official checkpoint parity; needs the index plus the shards holding the layers.
AIEN_REQUIRE_GPU_TESTS=1 AIEN_QWEN3_FP8_CHECKPOINT=/path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 \
  AIEN_QWEN3_LAYERS=0,1,23,24,47 AIEN_QWEN3_SEEDS=1,2,3 \
  cargo test -p aien-inference-abi --test qwen3_moe_gpu official -- --nocapture

# Batch-size curve with a reproducible receipt.
cargo run --release -p aien-inference-abi --example qwen3_moe_bench -- \
  /path/to/Qwen3-Coder-30B-A3B-Instruct-FP8 --layer 0 --out receipt.json
```

Without the built library the GPU tests skip with a message.
`AIEN_REQUIRE_GPU_TESTS=1` turns a missing library into a failure.

## What the tests prove

| Test | Claim |
| --- | --- |
| `grouped_fp8_gemm_matches_block_scaled_reference` | MMA fragment mapping, ragged groups, expert remapping and both scale axes against an exact reference. |
| `grouped_fp8_gemm_rejects_out_of_range_groups` | Out-of-range expert IDs and bad offsets are refused before launch. |
| `device_routing_matches_moe_batch_plan_exactly` | Rust router == Mojo router == device permutation == inverse map, over 1024 tokens including full ties and ties across the 64-expert mask split. |
| `device_router_fails_closed_on_non_finite_logits` | NaN and +/-Inf logits return status 2; no output is produced. |
| `layer_rejects_non_finite_input_without_output` | The same through the full layer. |
| `synthetic_layer_with_distinct_experts_matches_fp32_oracle` | Distinct random experts and router, 1/37/130 tokens: device matches an oracle that quantizes activations the same way. |
| `official_checkpoint_layers_match_fp32_oracle` | Official layers against both oracles, tolerances below. |

Two oracles are used. `OracleActivations::Fp8Block128` quantizes activations
exactly as the device does, so only accumulation order separates it from the
device: any indexing, scaling or routing defect shows up there. `Fp32` keeps
activations in FP32 and measures the real cost of FP8 activation quantization.
Both oracles route with the device's own logits, because FP32 summation order
can reorder experts separated by a few ULPs; the logits themselves are checked
against the CPU separately (observed error under 3e-6).

## Official checkpoint tolerances

Frozen from layers 0, 1, 23, 24 and 47, seeds 1 to 3, 16 tokens each, with
limits at about 1.5x to 2x the worst observed error:

| Oracle | Metric | Worst observed | Limit |
| --- | --- | ---: | ---: |
| FP8-emulated | cosine | 0.99999843 | 0.99999 |
| FP8-emulated | max abs / max\|expected\| | 0.33% | 1% |
| FP32 | cosine | 0.99910 (layer 23) | 0.9985 |
| FP32 | p95 relative error | 1.73% (layer 47) | 2.5% |
| FP32 | max abs / max\|expected\| | 3.2% (layer 24) | 5% |
| both | non-finite outputs | 0 | 0 |
| both | device top-8 vs MoeBatchPlan on device logits | identical | identical |

The FP32 gap is the cost of FP8 E4M3 activations in 128-channel blocks, the
W8A8 block scheme this checkpoint is published for. It is larger on middle
layers (cosine about 0.9991) than on layer 0 (about 0.9998).

## Performance

Official layer 0, seed 42, two warmups and five timed runs per point, median
milliseconds for the whole routed layer (routing, grouping, both FP8 GEMMs,
SwiGLU, quantization and reduction). Receipt:
`receipts/gb10_qwen3_a3b_layer0_mojo_fp8_curve_2026-09-23.json`.

| Tokens | This library | Earlier MAX graph, FP8 | Earlier MAX graph, BF16 |
| ---: | ---: | ---: | ---: |
| 1 | 0.565 | 1.086 | 1.131 |
| 8 | 2.506 | 5.451 | 4.626 |
| 16 | 3.816 | 8.682 | 7.986 |
| 32 | 5.523 | 11.581 | 12.011 |
| 64 | 7.774 | 13.328 | 22.336 |
| 128 | 10.780 | 15.969 | 43.875 |
| 256 | 18.193 | 24.992 | 86.080 |
| 500 | 32.170 | 43.529 | 165.718 |
| 1024 | 62.916 | 77.416 | 338.866 |

The timing methods are close but not identical: this library times enqueue to
device synchronize with input resident; the earlier driver also copied the
BF16 output back to the host. Read the comparison as indicative. Tokens per
second here is one layer only; the model has 48 plus attention.

The receipt in `receipts/gb10_qwen3_a3b_layer0_fp8_curve_2026-09-23.json` is
the earlier MAX-graph measurement, kept for its BF16 grouped-GEMM baseline.
That baseline depended on the Python MAX graph API and is no longer built.

## Boundaries

This is one MoE layer. Attention, norms, embeddings, sampling, scheduler
wiring and full-model residency are not implemented here. Each call creates
its device context and uploads weights; a resident-weight handle is the next
step before scheduler integration. Test inputs are seeded random hidden
states, not hidden states from real prompts.
