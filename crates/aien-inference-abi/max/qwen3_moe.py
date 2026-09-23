"""Qwen3-Coder-A3B routed expert layer on MAX and Mojo.

The upload graph accepts FP8 E4M3 checkpoint bytes and 128x128 inverse
scales, converting one layer into BF16 device buffers. No full FP32 model
copy is built. The execution graph uses MAX's grouped BF16 GEMM on GB10.
"""

from dataclasses import dataclass
from pathlib import Path

from max.dtype import DType
from max.graph import DeviceRef, Graph, TensorType, ops
from max.nn.moe.moe import grouped_matmul_ragged, moe_create_indices


@dataclass(frozen=True)
class Qwen3MoeShape:
    tokens: int
    hidden: int = 2048
    intermediate: int = 768
    experts: int = 128
    top_k: int = 8

    def __post_init__(self) -> None:
        if self.tokens < 1 or self.hidden % 128 or self.intermediate % 128:
            raise ValueError("token count must be positive and channels must align to 128")
        if self.experts != 128 or self.top_k != 8:
            raise ValueError("this graph targets Qwen3-Coder-30B-A3B routing")


def build_fp8_unpack_graph(shape: Qwen3MoeShape, device: DeviceRef) -> Graph:
    """Convert raw FP8 checkpoint blocks to BF16 once, entirely on device."""

    def unpack(gate_up, gate_up_scales, down, down_scales):
        def one(packed, scales):
            return ops.custom(
                name="aien.qwen3_moe.unpack_fp8_block128",
                device=device,
                values=[packed, scales],
                out_types=[TensorType(DType.bfloat16, packed.shape, device=device)],
            )[0].tensor

        return one(gate_up, gate_up_scales), one(down, down_scales)

    return Graph(
        "aien_qwen3_coder_a3b_unpack_fp8",
        forward=unpack,
        input_types=[
            TensorType(
                DType.float8_e4m3fn,
                [shape.experts, shape.intermediate * 2, shape.hidden],
                device=device,
            ),
            TensorType(
                DType.bfloat16,
                [shape.experts, shape.intermediate * 2 // 128, shape.hidden // 128],
                device=device,
            ),
            TensorType(
                DType.float8_e4m3fn,
                [shape.experts, shape.hidden, shape.intermediate],
                device=device,
            ),
            TensorType(
                DType.bfloat16,
                [shape.experts, shape.hidden // 128, shape.intermediate // 128],
                device=device,
            ),
        ],
        custom_extensions=[Path(__file__).parent / "kernels"],
    )


def build_qwen3_moe_graph(shape: Qwen3MoeShape, device: DeviceRef) -> Graph:
    """Build a complete Qwen3 MoE layer using the resident BF16 expert buffers."""

    def forward(x, router, gate_up, down):
        logits = ops.cast(x @ ops.transpose(router, 0, 1), DType.float32)
        routed = ops.custom(
            name="aien.qwen3_moe.route_top8",
            device=device,
            values=[logits],
            out_types=[
                TensorType(DType.int32, [shape.tokens, 8], device=device),
                TensorType(DType.float32, [shape.tokens, 8], device=device),
            ],
        )
        ids, probabilities = routed[0].tensor, routed[1].tensor
        order, offsets, restore, expert_ids, usage = moe_create_indices(
            ops.reshape(ids, [-1]), shape.experts
        )
        gathered = ops.gather(
            x,
            ops.cast(ops.floor_div(order, shape.top_k), DType.int32),
            axis=0,
        )
        gate_up_result = grouped_matmul_ragged(gathered, gate_up, offsets, expert_ids, usage)
        gate = gate_up_result[:, : shape.intermediate]
        up = gate_up_result[:, shape.intermediate :]
        activated = gate * ops.sigmoid(gate) * up
        expert_output = grouped_matmul_ragged(activated, down, offsets, expert_ids, usage)
        restored = ops.gather(expert_output, restore, axis=0)
        restored = ops.reshape(restored, [shape.tokens, shape.top_k, shape.hidden])
        weighted = ops.cast(restored, DType.float32) * ops.unsqueeze(
            probabilities, axis=2
        )
        return ops.cast(ops.squeeze(ops.sum(weighted, axis=1), axis=1), DType.bfloat16)

    return Graph(
        "aien_qwen3_coder_a3b_moe_bf16",
        forward=forward,
        input_types=[
            TensorType(DType.bfloat16, [shape.tokens, shape.hidden], device=device),
            TensorType(DType.bfloat16, [shape.experts, shape.hidden], device=device),
            TensorType(DType.bfloat16, [shape.experts, shape.intermediate * 2, shape.hidden], device=device),
            TensorType(DType.bfloat16, [shape.experts, shape.hidden, shape.intermediate], device=device),
        ],
        custom_extensions=[Path(__file__).parent / "kernels"],
    )
