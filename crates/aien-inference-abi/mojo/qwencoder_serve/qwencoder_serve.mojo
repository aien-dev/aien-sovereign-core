"""Resident-weight Qwen3-Coder-30B-A3B serve library for GB10 (foundry seat).

Companion to the proven `libaien_qwen3_moe.so` pipeline: same kernels and
orchestration (`from lib import ...` is read-only reuse, nothing is modified
there), but weights upload ONCE at startup and stay resident. Per decode step
only the 2048-wide activation row crosses the host/device boundary, so the
~40ms per-call upload cost of the batch-oriented API disappears.

Model shape is fixed (48 layers, hidden 2048, 32Q/4KV, head 128, MoE 128x8x768,
vocab 151936); every buffer is preallocated in `qwencoder_model_create`.
One model per process. Status codes: 0 ok, 1 shape/state, 3 device error.

Build: ./build.sh (needs -I at the MoE mojo dir, read-only).
"""

from max.gpu.host import DeviceContext, DeviceBuffer, HostBuffer
from max.gpu.memory import AddressSpace
from max.gpu.sync import barrier
from std.gpu import block_dim, block_idx, thread_idx
from std.math import exp, exp2, max
from std.memory import Layout, alloc, dealloc, stack_allocation

from kernels import (
    EXPERTS,
    TOP_K,
    BF16Ptr,
    F32Ptr,
    U8Ptr,
)
from lib import Workspace, _enqueue_layer

comptime STATUS_OK: Int32 = 0
comptime STATUS_BAD_STATE: Int32 = 1
comptime STATUS_DEVICE_ERROR: Int32 = 3

comptime LAYERS = 48
comptime HIDDEN = 2048
comptime Q_DIM = 4096
comptime KV_DIM = 512
comptime Q_HEADS = 32
comptime KV_HEADS = 4
comptime HEAD_DIM = 128
comptime GQA_GROUP = 8
comptime INTER = 768
comptime VOCAB = 151936
comptime PROJ_SLOTS = 4
comptime MAX_SEQ = 32768
comptime SM_SCALE = 0.08838834764831845  # 1 / sqrt(128)


def fp8_block128_gemv_kernel(
    result: F32Ptr,
    weights: U8Ptr,
    lut: F32Ptr,
    scales: BF16Ptr,
    vec: F32Ptr,
    rows: Int32,
    cols: Int32,
):
    """One thread per output row. LUT maps E4M3 codes to FP32; BF16 inv scale
    per 128-wide block, scale grid row-major over 128x128 blocks."""
    var row = block_idx.x * block_dim.x + thread_idx.x
    if row >= Int(rows):
        return
    var c = Int(cols)
    var kb = c // 128
    var acc = Float32(0.0)
    var base = row * c
    for b in range(kb):
        var sc = scales[(row // 128) * kb + b].cast[DType.float32]()
        for k in range(128):
            var w = lut[Int(weights[base + b * 128 + k])]
            acc += w * vec[b * 128 + k] * sc
    result[row] = acc


def bf16_gemv_kernel(
    result: F32Ptr, weights: BF16Ptr, vec: F32Ptr, rows: Int32, cols: Int32
):
    """One thread per output row, BF16 weights, FP32 accumulation."""
    var row = block_idx.x * block_dim.x + thread_idx.x
    if row >= Int(rows):
        return
    var c = Int(cols)
    var acc = Float32(0.0)
    var base = row * c
    for k in range(c):
        acc += weights[base + k].cast[DType.float32]() * vec[k]
    result[row] = acc


def kv_append_kernel(
    k_cache: F32Ptr, v_cache: F32Ptr, k_row: F32Ptr, v_row: F32Ptr, pos: Int32
):
    """Appends one token's K/V rows ([512] each) at position pos.
    Grid covers 1024 lanes; d < 512 is K, else V."""
    var d = block_idx.x * block_dim.x + thread_idx.x
    if d >= 2 * KV_DIM:
        return
    var base = Int(pos) * KV_DIM
    if d < KV_DIM:
        k_cache[base + d] = k_row[d]
    else:
        v_cache[base + (d - KV_DIM)] = v_row[d - KV_DIM]


def attn_scores_kernel(
    scores: F32Ptr, q: F32Ptr, k_cache: F32Ptr, seq: Int32, scale: Float32
):
    """One block per Q head. scores[h, t] = dot(q_h, K_t) * scale.
    Lanes stride over t; each lane runs full 128-dots from shared q."""
    var h = block_idx.x
    var lane = thread_idx.x
    var kv_h = h // GQA_GROUP
    var sq = stack_allocation[HEAD_DIM, Float32, address_space=AddressSpace.SHARED]()
    if lane < HEAD_DIM:
        sq[lane] = q[h * HEAD_DIM + lane]
    barrier()
    var n = Int(seq)
    var t = lane
    while t < n:
        var acc = Float32(0.0)
        var kbase = t * KV_DIM + kv_h * HEAD_DIM
        for d in range(HEAD_DIM):
            acc += sq[d] * k_cache[kbase + d]
        scores[h * MAX_SEQ + t] = acc * scale
        t += 128


def attn_combine_kernel(
    result: F32Ptr, scores: F32Ptr, v_cache: F32Ptr, seq: Int32
):
    """One block per Q head, lane d. Softmax over t then weighted sum of V.
    Grid 32 blocks, 128 lanes."""
    var h = block_idx.x
    var lane = thread_idx.x
    var kv_h = h // GQA_GROUP
    var n = Int(seq)
    var mx = Float32(-1e30)
    for t in range(n):
        mx = max(mx, scores[h * MAX_SEQ + t])
    var denom = Float32(0.0)
    var acc = Float32(0.0)
    for t in range(n):
        var w = exp(scores[h * MAX_SEQ + t] - mx)
        denom += w
        acc += w * v_cache[t * KV_DIM + kv_h * HEAD_DIM + lane]
    result[h * HEAD_DIM + lane] = acc / denom


struct QwenServeModel(Movable):
    """All resident weights, one-token scratch, and staging. ~30GB device."""

    var ctx: DeviceContext
    var moe_router: List[DeviceBuffer[DType.bfloat16]]
    var moe_gate_up: List[DeviceBuffer[DType.uint8]]
    var moe_gu_scales: List[DeviceBuffer[DType.bfloat16]]
    var moe_down: List[DeviceBuffer[DType.uint8]]
    var moe_down_scales: List[DeviceBuffer[DType.bfloat16]]
    var proj_w: List[DeviceBuffer[DType.uint8]]
    var proj_s: List[DeviceBuffer[DType.bfloat16]]
    var proj_rows: List[Int]
    var proj_cols: List[Int]
    var lm_head: DeviceBuffer[DType.bfloat16]
    var lut: DeviceBuffer[DType.float32]
    var ws: Workspace
    var kv_k: List[DeviceBuffer[DType.float32]]
    var kv_v: List[DeviceBuffer[DType.float32]]
    var attn_scores: DeviceBuffer[DType.float32]
    var q_stage: DeviceBuffer[DType.float32]
    var attn_out_stage: DeviceBuffer[DType.float32]
    var d4096a: DeviceBuffer[DType.float32]
    var d4096b: DeviceBuffer[DType.float32]
    var d4096c: DeviceBuffer[DType.float32]
    var d2048a: DeviceBuffer[DType.float32]
    var d2048b: DeviceBuffer[DType.float32]
    var d512a: DeviceBuffer[DType.float32]
    var d512b: DeviceBuffer[DType.float32]
    var dlogits: DeviceBuffer[DType.float32]
    var h_attn: HostBuffer[DType.float32]
    var h_q: HostBuffer[DType.float32]
    var h_k: HostBuffer[DType.float32]
    var h_v: HostBuffer[DType.float32]
    var h_o: HostBuffer[DType.float32]  # o-proj download staging, exactly [2048]
    var h_moe: HostBuffer[DType.bfloat16]
    var h_status: HostBuffer[DType.int32]
    var h_logits: HostBuffer[DType.float32]
    var x_stage: DeviceBuffer[DType.bfloat16]
    var vec_stage: DeviceBuffer[DType.float32]
    var out_stage: DeviceBuffer[DType.float32]

    def __init__(out self) raises:
        var ctx = DeviceContext()
        self.ctx = ctx
        self.moe_router = List[DeviceBuffer[DType.bfloat16]]()
        self.moe_gate_up = List[DeviceBuffer[DType.uint8]]()
        self.moe_gu_scales = List[DeviceBuffer[DType.bfloat16]]()
        self.moe_down = List[DeviceBuffer[DType.uint8]]()
        self.moe_down_scales = List[DeviceBuffer[DType.bfloat16]]()
        for _ in range(LAYERS):
            self.moe_router.append(ctx.enqueue_create_buffer[DType.bfloat16](EXPERTS * HIDDEN))
            self.moe_gate_up.append(ctx.enqueue_create_buffer[DType.uint8](EXPERTS * 2 * INTER * HIDDEN))
            self.moe_gu_scales.append(
                ctx.enqueue_create_buffer[DType.bfloat16](EXPERTS * (2 * INTER // 128) * (HIDDEN // 128))
            )
            self.moe_down.append(ctx.enqueue_create_buffer[DType.uint8](EXPERTS * HIDDEN * INTER))
            self.moe_down_scales.append(
                ctx.enqueue_create_buffer[DType.bfloat16](EXPERTS * (HIDDEN // 128) * (INTER // 128))
            )
        self.proj_w = List[DeviceBuffer[DType.uint8]]()
        self.proj_s = List[DeviceBuffer[DType.bfloat16]]()
        self.proj_rows = List[Int]()
        self.proj_cols = List[Int]()
        # Slot order per layer: 0 = q [4096,2048], 1 = k [512,2048],
        # 2 = v [512,2048], 3 = o [2048,4096].
        var slot_rows = List[Int]()
        slot_rows.append(Q_DIM)
        slot_rows.append(KV_DIM)
        slot_rows.append(KV_DIM)
        slot_rows.append(HIDDEN)
        var slot_cols = List[Int]()
        slot_cols.append(HIDDEN)
        slot_cols.append(HIDDEN)
        slot_cols.append(HIDDEN)
        slot_cols.append(Q_DIM)
        for _ in range(LAYERS):
            for s in range(PROJ_SLOTS):
                var r = slot_rows[s]
                var c = slot_cols[s]
                self.proj_w.append(ctx.enqueue_create_buffer[DType.uint8](r * c))
                self.proj_s.append(ctx.enqueue_create_buffer[DType.bfloat16]((r // 128) * (c // 128)))
                self.proj_rows.append(r)
                self.proj_cols.append(c)
        self.lm_head = ctx.enqueue_create_buffer[DType.bfloat16](VOCAB * HIDDEN)
        self.lut = ctx.enqueue_create_buffer[DType.float32](256)
        self.ws = Workspace(ctx, 1, HIDDEN, INTER)
        self.kv_k = List[DeviceBuffer[DType.float32]]()
        self.kv_v = List[DeviceBuffer[DType.float32]]()
        for _ in range(LAYERS):
            self.kv_k.append(ctx.enqueue_create_buffer[DType.float32](MAX_SEQ * KV_DIM))
            self.kv_v.append(ctx.enqueue_create_buffer[DType.float32](MAX_SEQ * KV_DIM))
        self.attn_scores = ctx.enqueue_create_buffer[DType.float32](Q_HEADS * MAX_SEQ)
        self.q_stage = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.attn_out_stage = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.d4096a = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.d4096b = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.d4096c = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.d2048a = ctx.enqueue_create_buffer[DType.float32](HIDDEN)
        self.d2048b = ctx.enqueue_create_buffer[DType.float32](HIDDEN)
        self.d512a = ctx.enqueue_create_buffer[DType.float32](KV_DIM)
        self.d512b = ctx.enqueue_create_buffer[DType.float32](KV_DIM)
        self.dlogits = ctx.enqueue_create_buffer[DType.float32](VOCAB)
        self.h_attn = ctx.enqueue_create_host_buffer[DType.float32](Q_DIM)
        self.h_q = ctx.enqueue_create_host_buffer[DType.float32](Q_DIM)
        self.h_k = ctx.enqueue_create_host_buffer[DType.float32](KV_DIM)
        self.h_v = ctx.enqueue_create_host_buffer[DType.float32](KV_DIM)
        self.h_o = ctx.enqueue_create_host_buffer[DType.float32](HIDDEN)
        self.h_moe = ctx.enqueue_create_host_buffer[DType.bfloat16](HIDDEN)
        self.h_status = ctx.enqueue_create_host_buffer[DType.int32](1)
        self.h_logits = ctx.enqueue_create_host_buffer[DType.float32](VOCAB)
        self.x_stage = ctx.enqueue_create_buffer[DType.bfloat16](HIDDEN)
        self.vec_stage = ctx.enqueue_create_buffer[DType.float32](Q_DIM)
        self.out_stage = ctx.enqueue_create_buffer[DType.float32](VOCAB)
        ctx.synchronize()

    def release(mut self) raises:
        """Drops every resident weight buffer (device memory) by reassigning
        empty containers; field destructors run on the old values. The struct
        shell itself is freed by the caller via `unsafe_free`."""
        self.moe_router = List[DeviceBuffer[DType.bfloat16]]()
        self.moe_gate_up = List[DeviceBuffer[DType.uint8]]()
        self.moe_gu_scales = List[DeviceBuffer[DType.bfloat16]]()
        self.moe_down = List[DeviceBuffer[DType.uint8]]()
        self.moe_down_scales = List[DeviceBuffer[DType.bfloat16]]()
        self.proj_w = List[DeviceBuffer[DType.uint8]]()
        self.proj_s = List[DeviceBuffer[DType.bfloat16]]()
        self.proj_rows = List[Int]()
        self.proj_cols = List[Int]()
        self.lm_head = self.ctx.enqueue_create_buffer[DType.bfloat16](1)


comptime ModelPtr = Pointer[QwenServeModel, MutUntrackedOrigin]


@export("qwencoder_model_create")
def qwencoder_model_create(handle_out: Pointer[ModelPtr, MutUntrackedOrigin]) abi("C") -> Int32:
    try:
        var model = QwenServeModel()
        # FP8 E4M3 LUT on host, then upload (same formula as the CPU oracle).
        var host_lut = model.ctx.enqueue_create_host_buffer[DType.float32](256)
        model.ctx.synchronize()
        for code in range(256):
            var sign = Float32(1.0)
            if (code & 0x80) != 0:
                sign = -1.0
            var exp = (code >> 3) & 0x0F
            var mant = Float32(code & 0x07)
            var value = Float32(0.0)
            if exp == 15 and mant == 7.0:
                value = Float32(0.0) / Float32(0.0)
            elif exp == 0:
                value = sign * mant * exp2(Float32(-9))
            else:
                value = sign * (1.0 + mant / 8.0) * exp2(Float32(exp - 7))
            host_lut[code] = value
        model.ctx.enqueue_copy(model.lut, host_lut)
        model.ctx.synchronize()
        var a = alloc(Layout[QwenServeModel](count=1))
        var p = (a^).unsafe_leak()
        p.unsafe_write(model^)
        handle_out.unsafe_write(p)
        return STATUS_OK
    except e:
        print("qwencoder_model_create:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_model_free")
def qwencoder_model_free(var model: ModelPtr) abi("C") -> Int32:
    try:
        ref m = model[]
        m.release()
        model.unsafe_free()
        return STATUS_OK
    except e:
        print("qwencoder_model_free:", e)
        return STATUS_DEVICE_ERROR


def _upload_copy[dtype: DType](ctx: DeviceContext, dev: DeviceBuffer[dtype], host: MutPointer[Scalar[dtype], MutAnyOrigin], count: Int) raises:
    ctx.enqueue_copy(dev, DeviceBuffer[dtype](ctx, host, count, owning=False))


@export("qwencoder_moe_upload")
def qwencoder_moe_upload(
    model: ModelPtr,
    layer: Int32,
    router: BF16Ptr,
    gate_up: U8Ptr,
    gu_scales: BF16Ptr,
    down: U8Ptr,
    down_scales: BF16Ptr,
) abi("C") -> Int32:
    if layer < 0 or Int(layer) >= LAYERS:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var l = Int(layer)
        _upload_copy[DType.bfloat16](m.ctx, m.moe_router[l], router, EXPERTS * HIDDEN)
        _upload_copy[DType.uint8](m.ctx, m.moe_gate_up[l], gate_up, EXPERTS * 2 * INTER * HIDDEN)
        _upload_copy[DType.bfloat16](
            m.ctx, m.moe_gu_scales[l], gu_scales, EXPERTS * (2 * INTER // 128) * (HIDDEN // 128)
        )
        _upload_copy[DType.uint8](m.ctx, m.moe_down[l], down, EXPERTS * HIDDEN * INTER)
        _upload_copy[DType.bfloat16](
            m.ctx, m.moe_down_scales[l], down_scales, EXPERTS * (HIDDEN // 128) * (INTER // 128)
        )
        m.ctx.synchronize()
        return STATUS_OK
    except e:
        print("qwencoder_moe_upload:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_proj_upload")
def qwencoder_proj_upload(
    model: ModelPtr, layer: Int32, slot: Int32, weights: U8Ptr, scales: BF16Ptr
) abi("C") -> Int32:
    if layer < 0 or Int(layer) >= LAYERS or slot < 0 or Int(slot) >= PROJ_SLOTS:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var idx = Int(layer) * PROJ_SLOTS + Int(slot)
        var r = m.proj_rows[idx]
        var c = m.proj_cols[idx]
        _upload_copy[DType.uint8](m.ctx, m.proj_w[idx], weights, r * c)
        _upload_copy[DType.bfloat16](m.ctx, m.proj_s[idx], scales, (r // 128) * (c // 128))
        m.ctx.synchronize()
        return STATUS_OK
    except e:
        print("qwencoder_proj_upload:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_lm_upload")
def qwencoder_lm_upload(model: ModelPtr, weights: BF16Ptr) abi("C") -> Int32:
    """LM head: BF16 [151936, 2048] host -> resident device buffer."""
    try:
        ref m = model[]
        _upload_copy[DType.bfloat16](m.ctx, m.lm_head, weights, VOCAB * HIDDEN)
        m.ctx.synchronize()
        return STATUS_OK
    except e:
        print("qwencoder_lm_upload:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_moe_forward")
def qwencoder_moe_forward(model: ModelPtr, layer: Int32, x: BF16Ptr, output: BF16Ptr) abi("C") -> Int32:
    """One token through one resident MoE layer. No routing metadata download."""
    if layer < 0 or Int(layer) >= LAYERS:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var l = Int(layer)
        _upload_copy[DType.bfloat16](m.ctx, m.x_stage, x, HIDDEN)
        _enqueue_layer(
            m.ctx,
            m.ws,
            m.x_stage,
            m.moe_router[l],
            m.moe_gate_up[l],
            m.moe_gu_scales[l],
            m.moe_down[l],
            m.moe_down_scales[l],
            1,
            HIDDEN,
            INTER,
        )
        m.ctx.enqueue_copy(m.h_moe, m.ws.output)
        m.ctx.enqueue_copy(m.h_status, m.ws.status)
        m.ctx.synchronize()
        if m.h_status[0] != 0:
            return STATUS_BAD_STATE
        for i in range(HIDDEN):
            output[unsafe_offset=i] = m.h_moe[i]
        return STATUS_OK
    except e:
        print("qwencoder_moe_forward:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_fp8_gemv")
def qwencoder_fp8_gemv(
    model: ModelPtr, layer: Int32, slot: Int32, vec: F32Ptr, output: F32Ptr
) abi("C") -> Int32:
    """O-proj only: [2048,4096] FP8 on persistent staging. The launch shape is
    fixed, so any other slot would read past a smaller host vector."""
    if layer < 0 or Int(layer) >= LAYERS or Int(slot) != PROJ_SLOTS - 1:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var idx = Int(layer) * PROJ_SLOTS + Int(slot)
        _upload_copy[DType.float32](m.ctx, m.d4096a, vec, Q_DIM)
        m.ctx.enqueue_function[fp8_block128_gemv_kernel](
            m.d2048a.unsafe_ptr(),
            m.proj_w[idx].unsafe_ptr(),
            m.lut.unsafe_ptr(),
            m.proj_s[idx].unsafe_ptr(),
            m.d4096a.unsafe_ptr(),
            Int32(HIDDEN),
            Int32(Q_DIM),
            grid_dim=(HIDDEN + 255) // 256,
            block_dim=256,
        )
        m.ctx.enqueue_copy(m.h_o, m.d2048a)
        m.ctx.synchronize()
        for i in range(HIDDEN):
            output[unsafe_offset=i] = m.h_o[i]
        return STATUS_OK
    except e:
        print("qwencoder_fp8_gemv:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_fused_qkv")
def qwencoder_fused_qkv(
    model: ModelPtr, layer: Int32, vec: F32Ptr, q_out: F32Ptr, k_out: F32Ptr, v_out: F32Ptr
) abi("C") -> Int32:
    """Q/K/V projections in one call: one activation upload, three GEMV
    kernels back to back, one synchronize. Slots 0/1/2 of the layer."""
    if layer < 0 or Int(layer) >= LAYERS:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var base = Int(layer) * PROJ_SLOTS
        _upload_copy[DType.float32](m.ctx, m.d2048a, vec, HIDDEN)
        # Slot shapes are fixed: q [4096,2048], k/v [512,2048].
        m.ctx.enqueue_function[fp8_block128_gemv_kernel](
            m.d4096a.unsafe_ptr(),
            m.proj_w[base].unsafe_ptr(),
            m.lut.unsafe_ptr(),
            m.proj_s[base].unsafe_ptr(),
            m.d2048a.unsafe_ptr(),
            Int32(Q_DIM),
            Int32(HIDDEN),
            grid_dim=(Q_DIM + 255) // 256,
            block_dim=256,
        )
        m.ctx.enqueue_function[fp8_block128_gemv_kernel](
            m.d512a.unsafe_ptr(),
            m.proj_w[base + 1].unsafe_ptr(),
            m.lut.unsafe_ptr(),
            m.proj_s[base + 1].unsafe_ptr(),
            m.d2048a.unsafe_ptr(),
            Int32(KV_DIM),
            Int32(HIDDEN),
            grid_dim=(KV_DIM + 255) // 256,
            block_dim=256,
        )
        m.ctx.enqueue_function[fp8_block128_gemv_kernel](
            m.d512b.unsafe_ptr(),
            m.proj_w[base + 2].unsafe_ptr(),
            m.lut.unsafe_ptr(),
            m.proj_s[base + 2].unsafe_ptr(),
            m.d2048a.unsafe_ptr(),
            Int32(KV_DIM),
            Int32(HIDDEN),
            grid_dim=(KV_DIM + 255) // 256,
            block_dim=256,
        )
        m.ctx.enqueue_copy(m.h_q, m.d4096a)
        m.ctx.enqueue_copy(m.h_k, m.d512a)
        m.ctx.enqueue_copy(m.h_v, m.d512b)
        m.ctx.synchronize()
        for i in range(Q_DIM):
            q_out[unsafe_offset=i] = m.h_q[i]
        for i in range(KV_DIM):
            k_out[unsafe_offset=i] = m.h_k[i]
            v_out[unsafe_offset=i] = m.h_v[i]
        return STATUS_OK
    except e:
        print("qwencoder_fused_qkv:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_bf16_gemv")
def qwencoder_bf16_gemv(model: ModelPtr, vec: F32Ptr, output: F32Ptr) abi("C") -> Int32:
    """LM head: BF16 [151936, 2048] resident, FP32 vec in, FP32 logits out."""
    try:
        ref m = model[]
        _upload_copy[DType.float32](m.ctx, m.d2048a, vec, HIDDEN)
        m.ctx.enqueue_function[bf16_gemv_kernel](
            m.dlogits.unsafe_ptr(),
            m.lm_head.unsafe_ptr(),
            m.d2048a.unsafe_ptr(),
            Int32(VOCAB),
            Int32(HIDDEN),
            grid_dim=(VOCAB + 255) // 256,
            block_dim=256,
        )
        m.ctx.enqueue_copy(m.h_logits, m.dlogits)
        m.ctx.synchronize()
        for i in range(VOCAB):
            output[unsafe_offset=i] = m.h_logits[i]
        return STATUS_OK
    except e:
        print("qwencoder_bf16_gemv:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_kv_append")
def qwencoder_kv_append(
    model: ModelPtr, layer: Int32, k_row: F32Ptr, v_row: F32Ptr, pos: Int32
) abi("C") -> Int32:
    """Appends one token's K/V rows ([512] each, RoPE already applied to K)
    into the layer's resident cache at position pos."""
    if layer < 0 or Int(layer) >= LAYERS or pos < 0 or Int(pos) >= MAX_SEQ:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var l = Int(layer)
        _upload_copy[DType.float32](m.ctx, m.d512a, k_row, KV_DIM)
        _upload_copy[DType.float32](m.ctx, m.d512b, v_row, KV_DIM)
        m.ctx.enqueue_function[kv_append_kernel](
            m.kv_k[l].unsafe_ptr(),
            m.kv_v[l].unsafe_ptr(),
            m.d512a.unsafe_ptr(),
            m.d512b.unsafe_ptr(),
            pos,
            grid_dim=(2 * KV_DIM + 255) // 256,
            block_dim=256,
        )
        m.ctx.synchronize()
        return STATUS_OK
    except e:
        print("qwencoder_kv_append:", e)
        return STATUS_DEVICE_ERROR


@export("qwencoder_attn")
def qwencoder_attn(
    model: ModelPtr, layer: Int32, q_row: F32Ptr, output: F32Ptr, seq_len: Int32
) abi("C") -> Int32:
    """One query token against the layer's resident KV (positions [0, seq_len)).
    q_row is [4096] with QK-norm and RoPE applied; output is [4096]."""
    if layer < 0 or Int(layer) >= LAYERS or seq_len <= 0 or Int(seq_len) > MAX_SEQ:
        return STATUS_BAD_STATE
    try:
        ref m = model[]
        var l = Int(layer)
        _upload_copy[DType.float32](m.ctx, m.q_stage, q_row, Q_DIM)
        m.ctx.enqueue_function[attn_scores_kernel](
            m.attn_scores.unsafe_ptr(),
            m.q_stage.unsafe_ptr(),
            m.kv_k[l].unsafe_ptr(),
            seq_len,
            Float32(SM_SCALE),
            grid_dim=Q_HEADS,
            block_dim=128,
        )
        m.ctx.enqueue_function[attn_combine_kernel](
            m.attn_out_stage.unsafe_ptr(),
            m.attn_scores.unsafe_ptr(),
            m.kv_v[l].unsafe_ptr(),
            seq_len,
            grid_dim=Q_HEADS,
            block_dim=128,
        )
        m.ctx.enqueue_copy(m.h_attn, m.attn_out_stage)
        m.ctx.synchronize()
        for i in range(Q_DIM):
            output[unsafe_offset=i] = m.h_attn[i]
        return STATUS_OK
    except e:
        print("qwencoder_attn:", e)
        return STATUS_DEVICE_ERROR
