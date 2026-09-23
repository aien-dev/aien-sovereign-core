"""C ABI for the Qwen3-Coder-A3B routed expert layer on GB10.

Rust loads libaien_qwen3_moe.so with libloading and passes host pointers.
Status codes: 0 ok, 1 invalid shape, 2 non-finite router logits, 3 device error.
Built by build.sh; no Python or MAX graph API is involved.
"""

from std.time import perf_counter_ns
from max.gpu.host import DeviceContext, DeviceBuffer

from kernels import (
    EXPERTS,
    TOP_K,
    BF16Ptr,
    F32Ptr,
    I32Ptr,
    U32Ptr,
    U8Ptr,
    router_logits_kernel,
    route_top8_kernel,
    group_by_expert_kernel,
    gather_rows_kernel,
    quantize_fp8_kernel,
    grouped_fp8_gemm_kernel,
    swiglu_kernel,
    weighted_restore_kernel,
)

comptime STATUS_OK: Int32 = 0
comptime STATUS_INVALID_SHAPE: Int32 = 1
comptime STATUS_NON_FINITE: Int32 = 2
comptime STATUS_DEVICE_ERROR: Int32 = 3


def _upload[dtype: DType](ctx: DeviceContext, host: MutPointer[Scalar[dtype], MutAnyOrigin], count: Int) raises -> DeviceBuffer[dtype]:
    var device = ctx.enqueue_create_buffer[dtype](count)
    ctx.enqueue_copy(device, DeviceBuffer[dtype](ctx, host, count, owning=False))
    return device


def _download[dtype: DType](ctx: DeviceContext, host: MutPointer[Scalar[dtype], MutAnyOrigin], device: DeviceBuffer[dtype], count: Int) raises:
    ctx.enqueue_copy(DeviceBuffer[dtype](ctx, host, count, owning=False), device)


def _valid_shape(tokens: Int32, hidden: Int32, intermediate: Int32) -> Bool:
    return tokens > 0 and hidden > 0 and intermediate > 0 and hidden % 128 == 0 and intermediate % 128 == 0


struct Workspace(Movable):
    """Per-call device buffers for one layer invocation of `tokens` tokens."""

    var logits: DeviceBuffer[DType.float32]
    var ids: DeviceBuffer[DType.int32]
    var weights: DeviceBuffer[DType.float32]
    var status: DeviceBuffer[DType.int32]
    var offsets: DeviceBuffer[DType.uint32]
    var order: DeviceBuffer[DType.uint32]
    var restore: DeviceBuffer[DType.uint32]
    var expert_ids: DeviceBuffer[DType.int32]
    var gathered: DeviceBuffer[DType.float32]
    var quantized_in: DeviceBuffer[DType.uint8]
    var scales_in: DeviceBuffer[DType.float32]
    var gate_up_out: DeviceBuffer[DType.float32]
    var activated: DeviceBuffer[DType.float32]
    var quantized_mid: DeviceBuffer[DType.uint8]
    var scales_mid: DeviceBuffer[DType.float32]
    var expert_out: DeviceBuffer[DType.float32]
    var output: DeviceBuffer[DType.bfloat16]

    def __init__(out self, ctx: DeviceContext, tokens: Int, hidden: Int, intermediate: Int) raises:
        var rows = tokens * TOP_K
        self.logits = ctx.enqueue_create_buffer[DType.float32](tokens * EXPERTS)
        self.ids = ctx.enqueue_create_buffer[DType.int32](rows)
        self.weights = ctx.enqueue_create_buffer[DType.float32](rows)
        self.status = ctx.enqueue_create_buffer[DType.int32](1)
        self.offsets = ctx.enqueue_create_buffer[DType.uint32](EXPERTS + 1)
        self.order = ctx.enqueue_create_buffer[DType.uint32](rows)
        self.restore = ctx.enqueue_create_buffer[DType.uint32](rows)
        self.expert_ids = ctx.enqueue_create_buffer[DType.int32](EXPERTS)
        self.gathered = ctx.enqueue_create_buffer[DType.float32](rows * hidden)
        self.quantized_in = ctx.enqueue_create_buffer[DType.uint8](rows * hidden)
        self.scales_in = ctx.enqueue_create_buffer[DType.float32](rows * (hidden // 128))
        self.gate_up_out = ctx.enqueue_create_buffer[DType.float32](rows * 2 * intermediate)
        self.activated = ctx.enqueue_create_buffer[DType.float32](rows * intermediate)
        self.quantized_mid = ctx.enqueue_create_buffer[DType.uint8](rows * intermediate)
        self.scales_mid = ctx.enqueue_create_buffer[DType.float32](rows * (intermediate // 128))
        self.expert_out = ctx.enqueue_create_buffer[DType.float32](rows * hidden)
        self.output = ctx.enqueue_create_buffer[DType.bfloat16](tokens * hidden)
        # Groups are indexed by expert directly; empty groups exit immediately.
        var identity = ctx.enqueue_create_host_buffer[DType.int32](EXPERTS)
        ctx.synchronize()
        for e in range(EXPERTS):
            identity[e] = Int32(e)
        ctx.enqueue_copy(self.expert_ids, identity)
        ctx.synchronize()


def _enqueue_routing(ctx: DeviceContext, mut ws: Workspace, tokens: Int) raises:
    ws.status.enqueue_fill(0)
    ctx.enqueue_function[route_top8_kernel](
        ws.ids.unsafe_ptr(), ws.weights.unsafe_ptr(), ws.logits.unsafe_ptr(), ws.status.unsafe_ptr(), Int32(tokens),
        grid_dim=(tokens + 127) // 128, block_dim=128,
    )
    ctx.enqueue_function[group_by_expert_kernel](
        ws.offsets.unsafe_ptr(), ws.order.unsafe_ptr(), ws.restore.unsafe_ptr(), ws.ids.unsafe_ptr(), Int32(tokens * TOP_K),
        grid_dim=1, block_dim=EXPERTS,
    )


def _enqueue_layer(
    ctx: DeviceContext,
    mut ws: Workspace,
    mut x: DeviceBuffer[DType.bfloat16],
    mut router: DeviceBuffer[DType.bfloat16],
    mut gate_up: DeviceBuffer[DType.uint8],
    mut gate_up_scales: DeviceBuffer[DType.bfloat16],
    mut down: DeviceBuffer[DType.uint8],
    mut down_scales: DeviceBuffer[DType.bfloat16],
    tokens: Int,
    hidden: Int,
    intermediate: Int,
) raises:
    var rows = tokens * TOP_K
    ctx.enqueue_function[router_logits_kernel](
        ws.logits.unsafe_ptr(), x.unsafe_ptr(), router.unsafe_ptr(), Int32(hidden),
        grid_dim=(EXPERTS, tokens), block_dim=32,
    )
    _enqueue_routing(ctx, ws, tokens)
    ctx.enqueue_function[gather_rows_kernel](
        ws.gathered.unsafe_ptr(), x.unsafe_ptr(), ws.order.unsafe_ptr(), Int32(hidden),
        grid_dim=rows, block_dim=256,
    )
    ctx.enqueue_function[quantize_fp8_kernel](
        ws.quantized_in.unsafe_ptr(), ws.scales_in.unsafe_ptr(), ws.gathered.unsafe_ptr(), Int32(hidden),
        grid_dim=(hidden // 128, rows), block_dim=32,
    )
    ctx.enqueue_function[grouped_fp8_gemm_kernel](
        ws.gate_up_out.unsafe_ptr(), ws.quantized_in.unsafe_ptr(), ws.scales_in.unsafe_ptr(),
        gate_up.unsafe_ptr(), gate_up_scales.unsafe_ptr(), ws.offsets.unsafe_ptr(), ws.expert_ids.unsafe_ptr(),
        Int32(2 * intermediate), Int32(hidden),
        grid_dim=(EXPERTS, 2 * intermediate // 8), block_dim=32,
    )
    ctx.enqueue_function[swiglu_kernel](
        ws.activated.unsafe_ptr(), ws.gate_up_out.unsafe_ptr(), Int32(rows), Int32(intermediate),
        grid_dim=(rows * intermediate + 255) // 256, block_dim=256,
    )
    ctx.enqueue_function[quantize_fp8_kernel](
        ws.quantized_mid.unsafe_ptr(), ws.scales_mid.unsafe_ptr(), ws.activated.unsafe_ptr(), Int32(intermediate),
        grid_dim=(intermediate // 128, rows), block_dim=32,
    )
    ctx.enqueue_function[grouped_fp8_gemm_kernel](
        ws.expert_out.unsafe_ptr(), ws.quantized_mid.unsafe_ptr(), ws.scales_mid.unsafe_ptr(),
        down.unsafe_ptr(), down_scales.unsafe_ptr(), ws.offsets.unsafe_ptr(), ws.expert_ids.unsafe_ptr(),
        Int32(hidden), Int32(intermediate),
        grid_dim=(EXPERTS, hidden // 8), block_dim=32,
    )
    ctx.enqueue_function[weighted_restore_kernel](
        ws.output.unsafe_ptr(), ws.expert_out.unsafe_ptr(), ws.weights.unsafe_ptr(), ws.restore.unsafe_ptr(), Int32(hidden),
        grid_dim=tokens, block_dim=256,
    )


def _status_flag(ctx: DeviceContext, ws: Workspace) raises -> Int32:
    var host = ctx.enqueue_create_host_buffer[DType.int32](1)
    ctx.enqueue_copy(host, ws.status)
    ctx.synchronize()
    return host[0]


@export("aien_qwen3_moe_forward")
def aien_qwen3_moe_forward(
    tokens: Int32,
    hidden: Int32,
    intermediate: Int32,
    x: BF16Ptr,
    router: BF16Ptr,
    gate_up: U8Ptr,
    gate_up_scales: BF16Ptr,
    down: U8Ptr,
    down_scales: BF16Ptr,
    output: BF16Ptr,
    logits_out: F32Ptr,
    ids_out: I32Ptr,
    weights_out: F32Ptr,
    offsets_out: U32Ptr,
    order_out: U32Ptr,
) abi("C") -> Int32:
    """Run one routed expert layer and return routing metadata for parity checks.

    `output` is [tokens, hidden] BF16. It is written only when status is 0.
    """
    if not _valid_shape(tokens, hidden, intermediate):
        return STATUS_INVALID_SHAPE
    try:
        var t = Int(tokens)
        var h = Int(hidden)
        var i = Int(intermediate)
        var ctx = DeviceContext()
        var ws = Workspace(ctx, t, h, i)
        var x_dev = _upload[DType.bfloat16](ctx, x, t * h)
        var router_dev = _upload[DType.bfloat16](ctx, router, EXPERTS * h)
        var gate_up_dev = _upload[DType.uint8](ctx, gate_up, EXPERTS * 2 * i * h)
        var gate_up_scales_dev = _upload[DType.bfloat16](ctx, gate_up_scales, EXPERTS * (2 * i // 128) * (h // 128))
        var down_dev = _upload[DType.uint8](ctx, down, EXPERTS * h * i)
        var down_scales_dev = _upload[DType.bfloat16](ctx, down_scales, EXPERTS * (h // 128) * (i // 128))
        _enqueue_layer(ctx, ws, x_dev, router_dev, gate_up_dev, gate_up_scales_dev, down_dev, down_scales_dev, t, h, i)
        _download[DType.float32](ctx, logits_out, ws.logits, t * EXPERTS)
        _download[DType.int32](ctx, ids_out, ws.ids, t * TOP_K)
        _download[DType.float32](ctx, weights_out, ws.weights, t * TOP_K)
        _download[DType.uint32](ctx, offsets_out, ws.offsets, EXPERTS + 1)
        _download[DType.uint32](ctx, order_out, ws.order, t * TOP_K)
        ctx.synchronize()
        if _status_flag(ctx, ws) != 0:
            return STATUS_NON_FINITE
        _download[DType.bfloat16](ctx, output, ws.output, t * h)
        ctx.synchronize()
        return STATUS_OK
    except e:
        print("aien_qwen3_moe_forward:", e)
        return STATUS_DEVICE_ERROR


@export("aien_qwen3_moe_benchmark")
def aien_qwen3_moe_benchmark(
    tokens: Int32,
    hidden: Int32,
    intermediate: Int32,
    x: BF16Ptr,
    router: BF16Ptr,
    gate_up: U8Ptr,
    gate_up_scales: BF16Ptr,
    down: U8Ptr,
    down_scales: BF16Ptr,
    warmup: Int32,
    repeats: Int32,
    samples_ns: MutPointer[UInt64, MutAnyOrigin],
) abi("C") -> Int32:
    """Time `repeats` complete layer executions with weights and input resident.

    Each sample is wall time from first enqueue to device synchronization.
    """
    if not _valid_shape(tokens, hidden, intermediate) or warmup < 0 or repeats < 1:
        return STATUS_INVALID_SHAPE
    try:
        var t = Int(tokens)
        var h = Int(hidden)
        var i = Int(intermediate)
        var ctx = DeviceContext()
        var ws = Workspace(ctx, t, h, i)
        var x_dev = _upload[DType.bfloat16](ctx, x, t * h)
        var router_dev = _upload[DType.bfloat16](ctx, router, EXPERTS * h)
        var gate_up_dev = _upload[DType.uint8](ctx, gate_up, EXPERTS * 2 * i * h)
        var gate_up_scales_dev = _upload[DType.bfloat16](ctx, gate_up_scales, EXPERTS * (2 * i // 128) * (h // 128))
        var down_dev = _upload[DType.uint8](ctx, down, EXPERTS * h * i)
        var down_scales_dev = _upload[DType.bfloat16](ctx, down_scales, EXPERTS * (h // 128) * (i // 128))
        ctx.synchronize()
        for _ in range(Int(warmup)):
            _enqueue_layer(ctx, ws, x_dev, router_dev, gate_up_dev, gate_up_scales_dev, down_dev, down_scales_dev, t, h, i)
        ctx.synchronize()
        for sample in range(Int(repeats)):
            var started = perf_counter_ns()
            _enqueue_layer(ctx, ws, x_dev, router_dev, gate_up_dev, gate_up_scales_dev, down_dev, down_scales_dev, t, h, i)
            ctx.synchronize()
            samples_ns[unsafe_offset=sample] = UInt64(perf_counter_ns() - started)
        if _status_flag(ctx, ws) != 0:
            return STATUS_NON_FINITE
        return STATUS_OK
    except e:
        print("aien_qwen3_moe_benchmark:", e)
        return STATUS_DEVICE_ERROR


@export("aien_qwen3_route_top8")
def aien_qwen3_route_top8(
    tokens: Int32,
    logits: F32Ptr,
    ids_out: I32Ptr,
    weights_out: F32Ptr,
    offsets_out: U32Ptr,
    order_out: U32Ptr,
    restore_out: U32Ptr,
) abi("C") -> Int32:
    """Route caller-supplied logits [tokens, 128] on the GPU and group assignments.

    Lets the host feed identical logits to MoeBatchPlan and the device router.
    """
    if tokens < 1:
        return STATUS_INVALID_SHAPE
    try:
        var t = Int(tokens)
        var ctx = DeviceContext()
        var ws = Workspace(ctx, t, 128, 128)
        ctx.enqueue_copy(ws.logits, DeviceBuffer[DType.float32](ctx, logits, t * EXPERTS, owning=False))
        _enqueue_routing(ctx, ws, t)
        _download[DType.int32](ctx, ids_out, ws.ids, t * TOP_K)
        _download[DType.float32](ctx, weights_out, ws.weights, t * TOP_K)
        _download[DType.uint32](ctx, offsets_out, ws.offsets, EXPERTS + 1)
        _download[DType.uint32](ctx, order_out, ws.order, t * TOP_K)
        _download[DType.uint32](ctx, restore_out, ws.restore, t * TOP_K)
        ctx.synchronize()
        if _status_flag(ctx, ws) != 0:
            return STATUS_NON_FINITE
        return STATUS_OK
    except e:
        print("aien_qwen3_route_top8:", e)
        return STATUS_DEVICE_ERROR


@export("aien_qwen3_grouped_fp8_gemm")
def aien_qwen3_grouped_fp8_gemm(
    rows: Int32,
    groups: Int32,
    experts: Int32,
    out_channels: Int32,
    in_channels: Int32,
    activations: U8Ptr,
    activation_scales: F32Ptr,
    weights: U8Ptr,
    weight_scales: BF16Ptr,
    offsets: U32Ptr,
    expert_ids: I32Ptr,
    result: F32Ptr,
) abi("C") -> Int32:
    """Standalone grouped FP8 GEMM for kernel parity tests.

    offsets has groups + 1 entries; expert_ids maps each group to an expert.
    """
    if rows < 1 or groups < 1 or experts < 1 or out_channels % 128 != 0 or in_channels % 128 != 0 or out_channels < 128 or in_channels < 128:
        return STATUS_INVALID_SHAPE
    # The kernel trusts offsets and expert IDs, so reject anything out of range here.
    if offsets[unsafe_offset=0] != 0 or Int(offsets[unsafe_offset=Int(groups)]) != Int(rows):
        return STATUS_INVALID_SHAPE
    for group in range(Int(groups)):
        var id = expert_ids[unsafe_offset=group]
        if id < 0 or id >= experts or offsets[unsafe_offset=group] > offsets[unsafe_offset=group + 1]:
            return STATUS_INVALID_SHAPE
    try:
        var r = Int(rows)
        var g = Int(groups)
        var e = Int(experts)
        var o = Int(out_channels)
        var k = Int(in_channels)
        var ctx = DeviceContext()
        var a_dev = _upload[DType.uint8](ctx, activations, r * k)
        var a_scales_dev = _upload[DType.float32](ctx, activation_scales, r * (k // 128))
        var w_dev = _upload[DType.uint8](ctx, weights, e * o * k)
        var w_scales_dev = _upload[DType.bfloat16](ctx, weight_scales, e * (o // 128) * (k // 128))
        var offsets_dev = _upload[DType.uint32](ctx, offsets, g + 1)
        var ids_dev = _upload[DType.int32](ctx, expert_ids, g)
        var out_dev = ctx.enqueue_create_buffer[DType.float32](r * o)
        ctx.enqueue_function[grouped_fp8_gemm_kernel](
            out_dev.unsafe_ptr(), a_dev.unsafe_ptr(), a_scales_dev.unsafe_ptr(), w_dev.unsafe_ptr(),
            w_scales_dev.unsafe_ptr(), offsets_dev.unsafe_ptr(), ids_dev.unsafe_ptr(), Int32(o), Int32(k),
            grid_dim=(g, o // 8), block_dim=32,
        )
        _download[DType.float32](ctx, result, out_dev, r * o)
        ctx.synchronize()
        return STATUS_OK
    except e:
        print("aien_qwen3_grouped_fp8_gemm:", e)
        return STATUS_DEVICE_ERROR
