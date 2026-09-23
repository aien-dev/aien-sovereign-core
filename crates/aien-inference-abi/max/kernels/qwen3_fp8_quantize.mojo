"""Dynamic per-row, 128-channel E4M3 activation quantization."""

import extensibility
from extensibility import InputTensor, OutputTensor, ManagedTensorSlice
from max.gpu.host import DeviceContext
from std.gpu import block_idx, thread_idx
from std.gpu.primitives import warp
from std.math import abs, max
from std.utils.index import IndexList


def _quantize_gpu(
    result: ManagedTensorSlice[dtype=DType.float8_e4m3fn, rank=2, mut=True, ...],
    inverse_scales: ManagedTensorSlice[dtype=DType.float32, rank=2, mut=True, ...],
    activations: ManagedTensorSlice[dtype=DType.float32, rank=2, ...],
    ctx: DeviceContext,
) raises:
    var rows = activations.dim_size(0)
    var columns = activations.dim_size(1)
    if columns % 128 != 0 or result.dim_size(0) != rows or result.dim_size(1) != columns:
        raise Error("FP8 activation quantization requires rows with 128-channel blocks")
    if inverse_scales.dim_size(0) != rows or inverse_scales.dim_size(1) != columns // 128:
        raise Error("FP8 activation inverse-scale shape mismatch")

    @parameter
    def quantize_kernel(row_count: Int32, column_count: Int32):
        var block = block_idx.x
        var row = block_idx.y
        var lane = thread_idx.x
        var base = block * 128 + lane * 4
        var values = SIMD[DType.float32, 4](0.0)
        var maximum = Float32(0.0)
        for i in range(4):
            var value = activations.load[1](IndexList[2](row, base + i))[0]
            values[i] = value
            maximum = max(maximum, abs(value))
        maximum = warp.max(maximum)
        # E4M3FN's largest finite value is 448. Zero blocks use scale 1.
        var scale = Float32(1.0)
        if maximum > 0.0:
            scale = maximum / 448.0
        if lane == 0:
            inverse_scales.store[1](IndexList[2](row, block), SIMD[DType.float32, 1](scale))
        for i in range(4):
            result.store[1](
                IndexList[2](row, base + i),
                SIMD[DType.float8_e4m3fn, 1](Float8_e4m3fn(values[i] / scale)),
            )

    ctx.enqueue_function[quantize_kernel](
        Int32(rows), Int32(columns),
        grid_dim=(columns // 128, rows), block_dim=32,
    )


@extensibility.register("aien.qwen3_moe.quantize_fp8_block128")
struct Qwen3QuantizeFp8:
    @staticmethod
    def execute[target: StaticString](
        result: OutputTensor[dtype=DType.float8_e4m3fn, rank=2, ...],
        inverse_scales: OutputTensor[dtype=DType.float32, rank=2, ...],
        activations: InputTensor[dtype=DType.float32, rank=2, ...],
        ctx: DeviceContext,
    ) raises:
        comptime if target == "gpu":
            _quantize_gpu(result, inverse_scales, activations, ctx)
        else:
            raise Error("FP8 activation quantization requires a GPU")
