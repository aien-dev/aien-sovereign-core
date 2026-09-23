"""Device kernels for the Qwen3-Coder-A3B routed expert layer on GB10.

Every kernel takes raw device pointers and Int32 extents. Expert weights stay
packed FP8 E4M3 with BF16 inverse scales per 128x128 block; activations are
quantized per row and 128-channel block before each grouped FP8 GEMM.
"""

from std.gpu import block_idx, thread_idx
from std.gpu.primitives import warp
from max.gpu.sync import barrier
from max.gpu.memory import AddressSpace
from std.memory import bitcast, stack_allocation
from std.math import exp, max, abs
from std.sys._assembly import inlined_assembly
from std.sys import _RegisterPackType

comptime EXPERTS = 128
comptime TOP_K = 8
comptime BLOCK = 128

comptime F32Ptr = MutPointer[Float32, MutAnyOrigin]
comptime BF16Ptr = MutPointer[BFloat16, MutAnyOrigin]
comptime U8Ptr = MutPointer[UInt8, MutAnyOrigin]
comptime U32Ptr = MutPointer[UInt32, MutAnyOrigin]
comptime I32Ptr = MutPointer[Int32, MutAnyOrigin]


def router_logits_kernel(logits: F32Ptr, x: BF16Ptr, router: BF16Ptr, hidden: Int32):
    """One warp per (expert, token). FP32 accumulation, no BF16 rounding of logits."""
    var expert = block_idx.x
    var token = block_idx.y
    var lane = thread_idx.x
    var h = Int(hidden)
    var acc = Float32(0.0)
    for k in range(lane, h, 32):
        acc += x[unsafe_offset=token * h + k].cast[DType.float32]() * router[
            unsafe_offset=expert * h + k
        ].cast[DType.float32]()
    acc = warp.sum(acc)
    if lane == 0:
        logits[unsafe_offset=token * EXPERTS + expert] = acc


def route_top8_kernel(ids: I32Ptr, weights: F32Ptr, logits: F32Ptr, status: I32Ptr, tokens: Int32):
    """One lane per token. Ties select the lower expert ID, matching MoeBatchPlan.

    A non-finite logit sets `status[0] = 1` and routes the token to experts 0..7
    with zero weight, so downstream indexing stays in bounds while the host
    rejects the whole call.
    """
    var token = block_idx.x * 128 + thread_idx.x
    if token >= Int(tokens):
        return
    var row = logits.unsafe_offset(token * EXPERTS)
    var finite = True
    for expert in range(EXPERTS):
        var value = row[unsafe_offset=expert]
        # (v - v) is NaN for both NaN and +/-Inf.
        if not (value - value == 0.0):
            finite = False
    if not finite:
        status[unsafe_offset=0] = 1
        for slot in range(TOP_K):
            ids[unsafe_offset=token * TOP_K + slot] = Int32(slot)
            weights[unsafe_offset=token * TOP_K + slot] = 0.0
        return

    var taken_lo: UInt64 = 0
    var taken_hi: UInt64 = 0
    var selected = InlineArray[Int32, TOP_K](fill=0)
    var selected_logits = InlineArray[Float32, TOP_K](fill=0.0)
    for slot in range(TOP_K):
        var best_expert = -1
        var best_logit = Float32(0.0)
        for expert in range(EXPERTS):
            var bit = UInt64(1) << UInt64(expert % 64)
            var taken = (taken_lo & bit) if expert < 64 else (taken_hi & bit)
            if taken != 0:
                continue
            var value = row[unsafe_offset=expert]
            if best_expert < 0 or value > best_logit:
                best_expert = expert
                best_logit = value
        if best_expert < 64:
            taken_lo |= UInt64(1) << UInt64(best_expert)
        else:
            taken_hi |= UInt64(1) << UInt64(best_expert - 64)
        selected[slot] = Int32(best_expert)
        selected_logits[slot] = best_logit
    # The full softmax denominator cancels under top-k renormalization.
    var denominator = Float32(0.0)
    for slot in range(TOP_K):
        denominator += exp(selected_logits[slot] - selected_logits[0])
    for slot in range(TOP_K):
        ids[unsafe_offset=token * TOP_K + slot] = selected[slot]
        weights[unsafe_offset=token * TOP_K + slot] = (
            exp(selected_logits[slot] - selected_logits[0]) / denominator
        )


def group_by_expert_kernel(
    offsets: U32Ptr, order: U32Ptr, restore: U32Ptr, ids: I32Ptr, assignments: Int32
):
    """Single block, one lane per expert: a stable counting sort.

    Grouped rows are ordered by expert, then by token-major assignment index,
    which is exactly MoeBatchPlan's grouped_to_assignment contract.
    """
    var expert = thread_idx.x
    var n = Int(assignments)
    var starts = stack_allocation[EXPERTS, Scalar[DType.uint32], address_space=AddressSpace.SHARED]()
    var count: UInt32 = 0
    for a in range(n):
        if Int(ids[unsafe_offset=a]) == expert:
            count += 1
    starts[unsafe_offset=expert] = count
    barrier()
    if expert == 0:
        var running: UInt32 = 0
        offsets[unsafe_offset=0] = 0
        for e in range(EXPERTS):
            var c = starts[unsafe_offset=e]
            starts[unsafe_offset=e] = running
            running += c
            offsets[unsafe_offset=e + 1] = running
    barrier()
    var position = starts[unsafe_offset=expert]
    for a in range(n):
        if Int(ids[unsafe_offset=a]) == expert:
            order[unsafe_offset=Int(position)] = UInt32(a)
            restore[unsafe_offset=a] = position
            position += 1


def gather_rows_kernel(gathered: F32Ptr, x: BF16Ptr, order: U32Ptr, hidden: Int32):
    """gathered[r] = x[order[r] / TOP_K] in FP32."""
    var row = block_idx.x
    var h = Int(hidden)
    var token = Int(order[unsafe_offset=row]) // TOP_K
    for c in range(thread_idx.x, h, 256):
        gathered[unsafe_offset=row * h + c] = x[unsafe_offset=token * h + c].cast[DType.float32]()


def quantize_fp8_kernel(result: U8Ptr, inverse_scales: F32Ptr, activations: F32Ptr, columns: Int32):
    """Per-row, per-128-channel E4M3 quantization. Grid (columns/128, rows), block 32."""
    var block = block_idx.x
    var row = block_idx.y
    var lane = thread_idx.x
    var cols = Int(columns)
    var base = row * cols + block * 128 + lane * 4
    var values = activations.unsafe_load[width=4](base)
    var maximum = warp.max(max(max(abs(values[0]), abs(values[1])), max(abs(values[2]), abs(values[3]))))
    # E4M3FN's largest finite value is 448. Zero blocks use scale 1.
    var scale = Float32(1.0)
    if maximum > 0.0:
        scale = maximum / 448.0
    if lane == 0:
        inverse_scales[unsafe_offset=row * (cols // 128) + block] = scale
    var quantized = bitcast[DType.uint8, 4]((values / scale).cast[DType.float8_e4m3fn]())
    result.unsafe_store[width=4](base, quantized)


def mma_e4m3(a: SIMD[DType.uint32, 4], b: SIMD[DType.uint32, 2], partial: SIMD[DType.float32, 4]) -> SIMD[DType.float32, 4]:
    var mma = inlined_assembly[
        "mma.sync.aligned.m16n8k32.row.col.f32.e4m3.e4m3.f32 "
        "{$0,$1,$2,$3}, {$4,$5,$6,$7}, {$8,$9}, {$10,$11,$12,$13};",
        _RegisterPackType[Float32, Float32, Float32, Float32],
        constraints="=f,=f,=f,=f,r,r,r,r,r,r,f,f,f,f",
        has_side_effect=False,
    ](a[0], a[1], a[2], a[3], b[0], b[1], partial[0], partial[1], partial[2], partial[3])
    return SIMD[DType.float32, 4](mma[0], mma[1], mma[2], mma[3])


def grouped_fp8_gemm_kernel(
    result: F32Ptr,
    activations: U8Ptr,
    activation_scales: F32Ptr,
    weights: U8Ptr,
    weight_scales: BF16Ptr,
    offsets: U32Ptr,
    expert_ids: I32Ptr,
    out_channels: Int32,
    in_channels: Int32,
):
    """result[r, o] = sum_k a[r, k] * w[expert(r), o, k], grouped by expert.

    Grid (groups, out_channels / 8), block 32. Each warp owns one group and
    eight output columns and walks the group's ragged rows in 16-row MMA tiles.
    Weights are row-major [expert, output, input] packed E4M3.
    """
    var lane = thread_idx.x
    var group_id = block_idx.x
    var begin = Int(offsets[unsafe_offset=group_id])
    var end = Int(offsets[unsafe_offset=group_id + 1])
    if begin == end:
        return
    var expert = Int(expert_ids[unsafe_offset=group_id])
    var out_tile = block_idx.y * 8
    var group = lane // 4
    var quad = lane % 4
    var n_out = Int(out_channels)
    var n_in = Int(in_channels)
    var k_blocks = n_in // 128
    var expert_weights = weights.unsafe_offset(expert * n_out * n_in)
    for row_tile in range(begin, end, 16):
        var total = SIMD[DType.float32, 4](0.0)
        for block in range(k_blocks):
            var partial = SIMD[DType.float32, 4](0.0)
            for k_tile in range(0, 128, 32):
                var a = SIMD[DType.uint32, 4](0)
                var b = SIMD[DType.uint32, 2](0)
                # PTX m16n8k32 operand layout: four packed E4M3 values per register.
                for frag in range(4):
                    var row = row_tile + group + (8 if frag % 2 == 1 else 0)
                    var k0 = block * 128 + k_tile + quad * 4 + (16 if frag >= 2 else 0)
                    if row < end:
                        a[frag] = bitcast[DType.uint32, 1](activations.unsafe_load[width=4](row * n_in + k0))
                for frag in range(2):
                    var k0 = block * 128 + k_tile + quad * 4 + frag * 16
                    b[frag] = bitcast[DType.uint32, 1](
                        expert_weights.unsafe_load[width=4]((out_tile + group) * n_in + k0)
                    )
                partial = mma_e4m3(a, b, partial)
            var w_scale = weight_scales[
                unsafe_offset=(expert * (n_out // 128) + out_tile // 128) * k_blocks + block
            ].cast[DType.float32]()
            for frag in range(4):
                var row = row_tile + group + (8 if frag >= 2 else 0)
                if row < end:
                    total[frag] += partial[frag] * w_scale * activation_scales[unsafe_offset=row * k_blocks + block]
        for frag in range(4):
            var row = row_tile + group + (8 if frag >= 2 else 0)
            if row < end:
                result[unsafe_offset=row * n_out + out_tile + quad * 2 + frag % 2] = total[frag]


def swiglu_kernel(activated: F32Ptr, gate_up: F32Ptr, rows: Int32, intermediate: Int32):
    """activated[r, j] = silu(gate[r, j]) * up[r, j] with gate|up packed per row."""
    var i = block_idx.x * 256 + thread_idx.x
    var inter = Int(intermediate)
    if i >= Int(rows) * inter:
        return
    var row = i // inter
    var column = i % inter
    var gate = gate_up[unsafe_offset=row * 2 * inter + column]
    var up = gate_up[unsafe_offset=row * 2 * inter + inter + column]
    activated[unsafe_offset=i] = gate / (1.0 + exp(-gate)) * up


def weighted_restore_kernel(
    output: BF16Ptr, expert_output: F32Ptr, weights: F32Ptr, restore: U32Ptr, hidden: Int32
):
    """output[t] = sum over slots of weight * expert_output[restore[t, slot]], in slot order."""
    var token = block_idx.x
    var h = Int(hidden)
    for c in range(thread_idx.x, h, 256):
        var acc = Float32(0.0)
        for slot in range(TOP_K):
            var a = token * TOP_K + slot
            acc += weights[unsafe_offset=a] * expert_output[unsafe_offset=Int(restore[unsafe_offset=a]) * h + c]
        output[unsafe_offset=token * h + c] = acc.cast[DType.bfloat16]()
