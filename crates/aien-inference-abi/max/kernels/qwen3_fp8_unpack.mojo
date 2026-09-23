import extensibility
from extensibility import InputTensor, OutputTensor, ManagedTensorSlice
from max.gpu.host import DeviceContext
from std.gpu import block_idx, thread_idx
from std.utils.index import IndexList


def _unpack_gpu(
    output: ManagedTensorSlice[dtype=DType.bfloat16, rank=3, mut=True, ...],
    packed: ManagedTensorSlice[dtype=DType.float8_e4m3fn, rank=3, ...],
    inverse_scales: ManagedTensorSlice[dtype=DType.bfloat16, rank=3, ...],
    ctx: DeviceContext,
) raises:
    var experts = packed.dim_size(0)
    var rows = packed.dim_size(1)
    var columns = packed.dim_size(2)
    if rows % 128 != 0 or columns % 128 != 0:
        raise Error("FP8 expert dimensions must align to 128")
    if inverse_scales.dim_size(0) != experts or inverse_scales.dim_size(1) != rows // 128 or inverse_scales.dim_size(2) != columns // 128:
        raise Error("FP8 expert scale shape is incompatible with 128x128 blocks")
    var elements = experts * rows * columns

    @parameter
    def unpack_kernel(element_count: Int32, rows_dev: Int32, columns_dev: Int32):
        var linear = block_idx.x * 256 + thread_idx.x
        if linear >= Int(element_count):
            return
        var cols = Int(columns_dev)
        var nrows = Int(rows_dev)
        var expert = linear // (nrows * cols)
        var row = (linear // cols) % nrows
        var col = linear % cols
        var raw = Float32(packed.load[1](IndexList[3](expert, row, col))[0])
        var scale = Float32(inverse_scales.load[1](IndexList[3](expert, row // 128, col // 128))[0])
        output.store[1](
            IndexList[3](expert, row, col),
            SIMD[DType.bfloat16, 1](BFloat16(raw * scale)),
        )

    ctx.enqueue_function[unpack_kernel](
        Int32(elements), Int32(rows), Int32(columns),
        grid_dim=(elements + 255) // 256,
        block_dim=256,
    )


@extensibility.register("aien.qwen3_moe.unpack_fp8_block128")
struct Qwen3UnpackFp8:
    @staticmethod
    def execute[target: StaticString](
        output: OutputTensor[dtype=DType.bfloat16, rank=3, ...],
        packed: InputTensor[dtype=DType.float8_e4m3fn, rank=3, ...],
        inverse_scales: InputTensor[dtype=DType.bfloat16, rank=3, ...],
        ctx: DeviceContext,
    ) raises:
        comptime if target == "gpu":
            _unpack_gpu(output, packed, inverse_scales, ctx)
        else:
            raise Error("Qwen3 FP8 unpack requires a GPU")
