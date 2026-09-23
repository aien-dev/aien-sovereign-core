"""Grouped FP8 E4M3 GEMM for GB10, 16x8x32 warp MMA.

Inputs are grouped token rows, packed row-major [expert, output, input]
checkpoint weights, BF16 inverse scales per 128x128 weight block,
FP32 activation inverse scales per token and 128 input channels, and
expert offsets. FP8 weight storage is never expanded to BF16.
"""

import extensibility
from extensibility import InputTensor, OutputTensor, ManagedTensorSlice
from max.gpu.host import DeviceContext
from std.gpu import block_idx, thread_idx
from std.sys._assembly import inlined_assembly
from std.sys import _RegisterPackType
from std.memory import bitcast
from std.utils.index import IndexList


def mma_e4m3(a: SIMD[DType.uint32, 4], b: SIMD[DType.uint32, 2], partial: SIMD[DType.float32, 4]) -> SIMD[DType.float32, 4]:
    var mma = inlined_assembly[
        "mma.sync.aligned.m16n8k32.row.col.f32.e4m3.e4m3.f32 "
        "{$0,$1,$2,$3}, {$4,$5,$6,$7}, {$8,$9}, {$10,$11,$12,$13};",
        _RegisterPackType[Float32, Float32, Float32, Float32],
        constraints="=f,=f,=f,=f,r,r,r,r,r,r,f,f,f,f",
        has_side_effect=False,
    ](a[0], a[1], a[2], a[3], b[0], b[1], partial[0], partial[1], partial[2], partial[3])
    return SIMD[DType.float32, 4](mma[0], mma[1], mma[2], mma[3])


def _gemm_gpu(
    result: ManagedTensorSlice[dtype=DType.float32, rank=2, mut=True, ...],
    activations: ManagedTensorSlice[dtype=DType.float8_e4m3fn, rank=2, ...],
    activation_scales: ManagedTensorSlice[dtype=DType.float32, rank=2, ...],
    weights: ManagedTensorSlice[dtype=DType.float8_e4m3fn, rank=3, ...],
    weight_scales: ManagedTensorSlice[dtype=DType.bfloat16, rank=3, ...],
    offsets: ManagedTensorSlice[dtype=DType.int32, rank=1, ...],
    ctx: DeviceContext,
) raises:
    var experts = weights.dim_size(0)
    var output_channels = weights.dim_size(1)
    var input_channels = weights.dim_size(2)
    var assignments = activations.dim_size(0)
    if input_channels % 128 != 0 or output_channels % 128 != 0:
        raise Error("FP8 expert dimensions must align to 128")
    if activations.dim_size(1) != input_channels or result.dim_size(0) != assignments or result.dim_size(1) != output_channels:
        raise Error("FP8 grouped GEMM shape mismatch")
    if activation_scales.dim_size(0) != assignments or activation_scales.dim_size(1) != input_channels // 128:
        raise Error("FP8 activation scale shape mismatch")
    if weight_scales.dim_size(0) != experts or weight_scales.dim_size(1) != output_channels // 128 or weight_scales.dim_size(2) != input_channels // 128:
        raise Error("FP8 weight scale shape mismatch")
    if offsets.dim_size(0) != experts + 1:
        raise Error("FP8 expert offset shape mismatch")

    @parameter
    def mma_kernel(expert_count: Int32, out_channels: Int32, in_channels: Int32):
        var lane = thread_idx.x
        var expert = block_idx.x
        var out_tile = block_idx.y * 8
        var group = lane // 4
        var quad = lane % 4
        var begin = Int(offsets.load[1](IndexList[1](expert))[0])
        var end = Int(offsets.load[1](IndexList[1](expert + 1))[0])
        # Each warp owns an expert and eight output columns. It walks the
        # expert's ragged rows in 16-row tiles, so the launch grid is compact.
        for row_tile in range(begin, end, 16):
            var total = SIMD[DType.float32, 4](0.0)
            for block in range(Int(in_channels) // 128):
                var partial = SIMD[DType.float32, 4](0.0)
                for k_tile in range(0, 128, 32):
                    var a = SIMD[DType.uint32, 4](0)
                    var b = SIMD[DType.uint32, 2](0)
                    # PTX m16n8k32 operand layout. Four packed E4M3 values
                    # occupy each 32-bit operand register.
                    for frag in range(4):
                        var row = row_tile + group + (8 if frag % 2 == 1 else 0)
                        var k0 = block * 128 + k_tile + quad * 4 + (16 if frag >= 2 else 0)
                        var packed: UInt32 = 0
                        for byte in range(4):
                            var value: UInt32 = 0
                            if row < end:
                                value = UInt32(bitcast[DType.uint8, 1](activations.load[1](IndexList[2](row, k0 + byte)))[0])
                            packed |= value << UInt32(byte * 8)
                        a[frag] = packed
                    for frag in range(2):
                        var k0 = block * 128 + k_tile + quad * 4 + frag * 16
                        var packed: UInt32 = 0
                        for byte in range(4):
                            var raw = weights.load[1](IndexList[3](expert, out_tile + group, k0 + byte))[0]
                            packed |= UInt32(bitcast[DType.uint8, 1](raw)[0]) << UInt32(byte * 8)
                        b[frag] = packed
                    partial = mma_e4m3(a, b, partial)
                var w_scale = Float32(weight_scales.load[1](IndexList[3](expert, out_tile // 128, block))[0])
                for frag in range(4):
                    var row = row_tile + group + (8 if frag >= 2 else 0)
                    if row < end:
                        total[frag] += partial[frag] * w_scale * activation_scales.load[1](IndexList[2](row, block))[0]
            for frag in range(4):
                var row = row_tile + group + (8 if frag >= 2 else 0)
                if row < end:
                    result.store[1](IndexList[2](row, out_tile + quad * 2 + frag % 2), SIMD[DType.float32, 1](total[frag]))

    ctx.enqueue_function[mma_kernel](
        Int32(experts), Int32(output_channels), Int32(input_channels),
        grid_dim=(experts, output_channels // 8), block_dim=32,
    )


@extensibility.register("aien.qwen3_moe.grouped_fp8_gemm")
struct Qwen3GroupedFp8Gemm:
    @staticmethod
    def execute[target: StaticString](
        result: OutputTensor[dtype=DType.float32, rank=2, ...],
        activations: InputTensor[dtype=DType.float8_e4m3fn, rank=2, ...],
        activation_scales: InputTensor[dtype=DType.float32, rank=2, ...],
        weights: InputTensor[dtype=DType.float8_e4m3fn, rank=3, ...],
        weight_scales: InputTensor[dtype=DType.bfloat16, rank=3, ...],
        offsets: InputTensor[dtype=DType.int32, rank=1, ...],
        ctx: DeviceContext,
    ) raises:
        comptime if target == "gpu":
            _gemm_gpu(result, activations, activation_scales, weights, weight_scales, offsets, ctx)
        else:
            raise Error("Grouped FP8 GEMM requires a GPU")
