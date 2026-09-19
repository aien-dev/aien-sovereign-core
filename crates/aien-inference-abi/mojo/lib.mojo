# AIEN Sovereign Core Tensor Kernels for NVIDIA DGX Spark GB10
# Native C-ABI shared library exporting hardware-accelerated tensor operations

from std.memory import MutPointer, ImmPointer, Layout, alloc, dealloc
from std.math import sqrt, sin, cos, exp, max

@export("aien_rmsnorm_bf16")
def aien_rmsnorm_bf16(
    x: ImmPointer[BFloat16, ImmutAnyOrigin],
    weight: ImmPointer[BFloat16, ImmutAnyOrigin],
    dst: MutPointer[BFloat16, MutAnyOrigin],
    size: Int32,
    eps: Float32
) abi("c"):
    var sum_sq: Float64 = 0.0
    for i in range(Int(size)):
        var val = Float64(x.unsafe_offset(i).unsafe_load())
        sum_sq += val * val
    var mean_sq = sum_sq / Float64(size)
    var scale = Float32(1.0 / sqrt(mean_sq + Float64(eps)))
    for i in range(Int(size)):
        var val = Float32(x.unsafe_offset(i).unsafe_load())
        var w = Float32(weight.unsafe_offset(i).unsafe_load())
        dst.unsafe_offset(i).unsafe_write(BFloat16(val * scale * w))

@export("aien_rope_bf16")
def aien_rope_bf16(
    q: MutPointer[BFloat16, MutAnyOrigin],
    k: MutPointer[BFloat16, MutAnyOrigin],
    pos: Int32,
    head_dim: Int32,
    num_q_heads: Int32,
    num_kv_heads: Int32,
    theta: Float32
) abi("c"):
    var half_dim = Int(head_dim) // 2
    for h in range(Int(num_q_heads)):
        var head_offset = h * Int(head_dim)
        for i in range(half_dim):
            var idx0 = head_offset + i
            var idx1 = head_offset + i + half_dim
            var exponent = Float64(2 * i) / Float64(head_dim)
            var freq = 1.0 / (Float64(theta) ** exponent)
            var rot = Float64(pos) * freq
            var sin_val = Float32(sin(rot))
            var cos_val = Float32(cos(rot))
            var q0 = Float32(q.unsafe_offset(idx0).unsafe_load())
            var q1 = Float32(q.unsafe_offset(idx1).unsafe_load())
            q.unsafe_offset(idx0).unsafe_write(BFloat16(q0 * cos_val - q1 * sin_val))
            q.unsafe_offset(idx1).unsafe_write(BFloat16(q0 * sin_val + q1 * cos_val))

    for h in range(Int(num_kv_heads)):
        var head_offset = h * Int(head_dim)
        for i in range(half_dim):
            var idx0 = head_offset + i
            var idx1 = head_offset + i + half_dim
            var exponent = Float64(2 * i) / Float64(head_dim)
            var freq = 1.0 / (Float64(theta) ** exponent)
            var rot = Float64(pos) * freq
            var sin_val = Float32(sin(rot))
            var cos_val = Float32(cos(rot))
            var k0 = Float32(k.unsafe_offset(idx0).unsafe_load())
            var k1 = Float32(k.unsafe_offset(idx1).unsafe_load())
            k.unsafe_offset(idx0).unsafe_write(BFloat16(k0 * cos_val - k1 * sin_val))
            k.unsafe_offset(idx1).unsafe_write(BFloat16(k0 * sin_val + k1 * cos_val))

@export("aien_gemv_bf16")
def aien_gemv_bf16(
    x: ImmPointer[BFloat16, ImmutAnyOrigin],
    weight: ImmPointer[BFloat16, ImmutAnyOrigin],
    dst: MutPointer[BFloat16, MutAnyOrigin],
    out_dim: Int32,
    in_dim: Int32
) abi("c"):
    for j in range(Int(out_dim)):
        var row_offset = j * Int(in_dim)
        var sum: Float64 = 0.0
        for i in range(Int(in_dim)):
            var xi = Float64(x.unsafe_offset(i).unsafe_load())
            var wi = Float64(weight.unsafe_offset(row_offset + i).unsafe_load())
            sum += xi * wi
        dst.unsafe_offset(j).unsafe_write(BFloat16(Float32(sum)))

@export("aien_swiglu_bf16")
def aien_swiglu_bf16(
    gate: ImmPointer[BFloat16, ImmutAnyOrigin],
    up: ImmPointer[BFloat16, ImmutAnyOrigin],
    dst: MutPointer[BFloat16, MutAnyOrigin],
    size: Int32
) abi("c"):
    for i in range(Int(size)):
        var g = Float64(gate.unsafe_offset(i).unsafe_load())
        var u = Float64(up.unsafe_offset(i).unsafe_load())
        var silu = g / (1.0 + exp(-g))
        dst.unsafe_offset(i).unsafe_write(BFloat16(Float32(silu * u)))

@export("aien_gqa_bf16")
def aien_gqa_bf16(
    q: ImmPointer[BFloat16, ImmutAnyOrigin],
    k_cache: ImmPointer[BFloat16, ImmutAnyOrigin],
    v_cache: ImmPointer[BFloat16, ImmutAnyOrigin],
    dst: MutPointer[BFloat16, MutAnyOrigin],
    seq_len: Int32,
    num_q_heads: Int32,
    num_kv_heads: Int32,
    head_dim: Int32
) abi("c"):
    if seq_len <= 0:
        return
    var gqa_ratio = Int(num_q_heads) // Int(num_kv_heads)
    var inv_sqrt_d = Float64(1.0 / sqrt(Float64(head_dim)))
    var scores_alloc = alloc(Layout[Float64](count=Int(seq_len)))
    var scores = scores_alloc.unsafe_ptr()

    for h in range(Int(num_q_heads)):
        var kv_head = h // gqa_ratio
        var q_offset = h * Int(head_dim)

        var max_score: Float64 = -1e30
        for t in range(Int(seq_len)):
            var k_offset = (t * Int(num_kv_heads) + kv_head) * Int(head_dim)
            var dot: Float64 = 0.0
            for d in range(Int(head_dim)):
                var qd = Float64(q.unsafe_offset(q_offset + d).unsafe_load())
                var kd = Float64(k_cache.unsafe_offset(k_offset + d).unsafe_load())
                dot += qd * kd
            var s = dot * inv_sqrt_d
            scores.unsafe_offset(t).unsafe_write(s)
            if s > max_score:
                max_score = s

        var sum_exp: Float64 = 0.0
        for t in range(Int(seq_len)):
            var es = exp(scores.unsafe_offset(t).unsafe_load() - max_score)
            scores.unsafe_offset(t).unsafe_write(es)
            sum_exp += es

        var inv_sum = 1.0 / sum_exp
        for t in range(Int(seq_len)):
            var norm_s = scores.unsafe_offset(t).unsafe_load() * inv_sum
            scores.unsafe_offset(t).unsafe_write(norm_s)

        var dst_offset = h * Int(head_dim)
        for d in range(Int(head_dim)):
            var sum: Float64 = 0.0
            for t in range(Int(seq_len)):
                var v_offset = (t * Int(num_kv_heads) + kv_head) * Int(head_dim)
                var vd = Float64(v_cache.unsafe_offset(v_offset + d).unsafe_load())
                sum += scores.unsafe_offset(t).unsafe_load() * vd
            dst.unsafe_offset(dst_offset + d).unsafe_write(BFloat16(Float32(sum)))

    dealloc(scores_alloc^)

@export("aien_rmsnorm_f32")
def aien_rmsnorm_f32(
    x: ImmPointer[Float32, ImmutAnyOrigin],
    weight: ImmPointer[Float32, ImmutAnyOrigin],
    dst: MutPointer[Float32, MutAnyOrigin],
    size: Int32,
    eps: Float32
) abi("c"):
    var sum_sq: Float64 = 0.0
    for i in range(Int(size)):
        var val = Float64(x.unsafe_offset(i).unsafe_load())
        sum_sq += val * val
    var mean_sq = sum_sq / Float64(size)
    var scale = Float32(1.0 / sqrt(mean_sq + Float64(eps)))
    for i in range(Int(size)):
        var val = x.unsafe_offset(i).unsafe_load()
        var w = weight.unsafe_offset(i).unsafe_load()
        dst.unsafe_offset(i).unsafe_write(val * scale * w)

@export("aien_rope_f32")
def aien_rope_f32(
    q: MutPointer[Float32, MutAnyOrigin],
    k: MutPointer[Float32, MutAnyOrigin],
    pos: Int32,
    head_dim: Int32,
    num_q_heads: Int32,
    num_kv_heads: Int32,
    theta: Float32
) abi("c"):
    var half_dim = Int(head_dim) // 2
    for h in range(Int(num_q_heads)):
        var head_offset = h * Int(head_dim)
        for i in range(half_dim):
            var idx0 = head_offset + i
            var idx1 = head_offset + i + half_dim
            var exponent = Float64(2 * i) / Float64(head_dim)
            var freq = 1.0 / (Float64(theta) ** exponent)
            var rot = Float64(pos) * freq
            var sin_val = Float32(sin(rot))
            var cos_val = Float32(cos(rot))
            var q0 = q.unsafe_offset(idx0).unsafe_load()
            var q1 = q.unsafe_offset(idx1).unsafe_load()
            q.unsafe_offset(idx0).unsafe_write(q0 * cos_val - q1 * sin_val)
            q.unsafe_offset(idx1).unsafe_write(q0 * sin_val + q1 * cos_val)

    for h in range(Int(num_kv_heads)):
        var head_offset = h * Int(head_dim)
        for i in range(half_dim):
            var idx0 = head_offset + i
            var idx1 = head_offset + i + half_dim
            var exponent = Float64(2 * i) / Float64(head_dim)
            var freq = 1.0 / (Float64(theta) ** exponent)
            var rot = Float64(pos) * freq
            var sin_val = Float32(sin(rot))
            var cos_val = Float32(cos(rot))
            var k0 = k.unsafe_offset(idx0).unsafe_load()
            var k1 = k.unsafe_offset(idx1).unsafe_load()
            k.unsafe_offset(idx0).unsafe_write(k0 * cos_val - k1 * sin_val)
            k.unsafe_offset(idx1).unsafe_write(k0 * sin_val + k1 * cos_val)

@export("aien_gemv_f32")
def aien_gemv_f32(
    x: ImmPointer[Float32, ImmutAnyOrigin],
    weight: ImmPointer[Float32, ImmutAnyOrigin],
    dst: MutPointer[Float32, MutAnyOrigin],
    out_dim: Int32,
    in_dim: Int32
) abi("c"):
    for j in range(Int(out_dim)):
        var row_offset = j * Int(in_dim)
        var sum: Float64 = 0.0
        for i in range(Int(in_dim)):
            var xi = Float64(x.unsafe_offset(i).unsafe_load())
            var wi = Float64(weight.unsafe_offset(row_offset + i).unsafe_load())
            sum += xi * wi
        dst.unsafe_offset(j).unsafe_write(Float32(sum))

@export("aien_swiglu_f32")
def aien_swiglu_f32(
    gate: ImmPointer[Float32, ImmutAnyOrigin],
    up: ImmPointer[Float32, ImmutAnyOrigin],
    dst: MutPointer[Float32, MutAnyOrigin],
    size: Int32
) abi("c"):
    for i in range(Int(size)):
        var g = Float64(gate.unsafe_offset(i).unsafe_load())
        var u = Float64(up.unsafe_offset(i).unsafe_load())
        var silu = g / (1.0 + exp(-g))
        dst.unsafe_offset(i).unsafe_write(Float32(silu * u))

@export("aien_gqa_f32")
def aien_gqa_f32(
    q: ImmPointer[Float32, ImmutAnyOrigin],
    k_cache: ImmPointer[Float32, ImmutAnyOrigin],
    v_cache: ImmPointer[Float32, ImmutAnyOrigin],
    dst: MutPointer[Float32, MutAnyOrigin],
    seq_len: Int32,
    num_q_heads: Int32,
    num_kv_heads: Int32,
    head_dim: Int32
) abi("c"):
    if seq_len <= 0:
        return
    var gqa_ratio = Int(num_q_heads) // Int(num_kv_heads)
    var inv_sqrt_d = Float64(1.0 / sqrt(Float64(head_dim)))
    var scores_alloc = alloc(Layout[Float64](count=Int(seq_len)))
    var scores = scores_alloc.unsafe_ptr()

    for h in range(Int(num_q_heads)):
        var kv_head = h // gqa_ratio
        var q_offset = h * Int(head_dim)

        var max_score: Float64 = -1e30
        for t in range(Int(seq_len)):
            var k_offset = (t * Int(num_kv_heads) + kv_head) * Int(head_dim)
            var dot: Float64 = 0.0
            for d in range(Int(head_dim)):
                var qd = Float64(q.unsafe_offset(q_offset + d).unsafe_load())
                var kd = Float64(k_cache.unsafe_offset(k_offset + d).unsafe_load())
                dot += qd * kd
            var s = dot * inv_sqrt_d
            scores.unsafe_offset(t).unsafe_write(s)
            if s > max_score:
                max_score = s

        var sum_exp: Float64 = 0.0
        for t in range(Int(seq_len)):
            var es = exp(scores.unsafe_offset(t).unsafe_load() - max_score)
            scores.unsafe_offset(t).unsafe_write(es)
            sum_exp += es

        var inv_sum = 1.0 / sum_exp
        for t in range(Int(seq_len)):
            var norm_s = scores.unsafe_offset(t).unsafe_load() * inv_sum
            scores.unsafe_offset(t).unsafe_write(norm_s)

        var dst_offset = h * Int(head_dim)
        for d in range(Int(head_dim)):
            var sum: Float64 = 0.0
            for t in range(Int(seq_len)):
                var v_offset = (t * Int(num_kv_heads) + kv_head) * Int(head_dim)
                var vd = Float64(v_cache.unsafe_offset(v_offset + d).unsafe_load())
                sum += scores.unsafe_offset(t).unsafe_load() * vd
            dst.unsafe_offset(dst_offset + d).unsafe_write(Float32(sum))

    dealloc(scores_alloc^)
