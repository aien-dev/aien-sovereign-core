# Qwen3-Coder-30B-A3B reference kernels (Phase B, GB10 foundry seat).
# Arch: 48 layers, hidden 2048, heads 32Q/4KV (GQA group 8), head_dim 128,
# MoE 128 experts top-8 inter 768, vocab 151936, RoPE theta 1e7,
# RMSNorm eps 1e-6, SwiGLU. Attention + experts FP8-block128 (BF16 inv
# scales); norms/embed/lm_head BF16; per-layer q_norm/k_norm [128].
# Reference math on CPU first (this file); GPU C-ABI later. Every choice
# mirrors crates/aien-inference-abi/src/qwen3_moe.rs decode formulas.

from std.math import sqrt, exp2


def rmsnorm_row(
    x: Span[Float32, _], weight: Span[Float32, _], dst: Span[mut=True, Float32, _], eps: Float32
):
    """dst[i] = x[i] / sqrt(mean(x^2) + eps) * weight[i]. Qwen eps 1e-6."""
    var n = len(x)
    var acc: Float32 = 0.0
    for i in range(n):
        acc += x[i] * x[i]
    var inv = 1.0 / sqrt(acc / Float32(n) + eps)
    for i in range(n):
        dst[i] = x[i] * inv * weight[i]


def rope_rotate_half_pair(
    x0: Float32, x1: Float32, cos: Float32, sin: Float32
) -> Tuple[Float32, Float32]:
    """Qwen rotate_half RoPE pair (same pairing as HF LLaMA)."""
    return (x0 * cos - x1 * sin, x0 * sin + x1 * cos)


def silu(x: Float32) -> Float32:
    return x / (1.0 + exp2(-x * 1.44269504089))


def swiglu_row(gate: Span[Float32, _], up: Span[Float32, _], dst: Span[mut=True, Float32, _]):
    """SwiGLU: dst = silu(gate) * up. Qwen expert width 768."""
    var n = len(dst)
    for i in range(n):
        dst[i] = silu(gate[i]) * up[i]


def repeat_kv_head(
    kv: Span[Float32, _], dst: Span[mut=True, Float32, _], head_dim: Int, groups: Int
):
    """GQA: repeat each KV head `groups` times (Qwen-Coder: 4 KV -> 32 Q, groups=8)."""
    var kv_heads = len(kv) // head_dim
    for h in range(kv_heads):
        for g in range(groups):
            var dbase = (h * groups + g) * head_dim
            var sbase = h * head_dim
            for d in range(head_dim):
                dst[dbase + d] = kv[sbase + d]


def row_gemv(
    dst: Span[mut=True, Float32, _],
    mat: Span[Float32, _],
    vec: Span[Float32, _],
    rows: Int,
    cols: Int,
):
    """Row-major [rows, cols] FP32 GEMV."""
    for r in range(rows):
        var acc: Float32 = 0.0
        var base = r * cols
        for c in range(cols):
            acc += mat[base + c] * vec[c]
        dst[r] = acc


def bf16_to_f32(bits: UInt16) -> Float32:
    """BF16 decode by arithmetic (matches Rust bf16_to_f32 bit-shift)."""
    var sign: Float32 = 1.0
    if (bits & UInt16(0x8000)) != UInt16(0):
        sign = -1.0
    var exp = Int((bits >> 7) & UInt16(0xFF))
    var mant = Float32(Int(bits & UInt16(0x7F)))
    if exp == 0:
        return sign * mant / 128.0 * exp2(Float32(-126))
    return sign * (1.0 + mant / 128.0) * exp2(Float32(exp - 127))


def row_gemv_bf16(
    dst: Span[mut=True, Float32, _],
    mat: Span[UInt16, _],
    vec: Span[Float32, _],
    rows: Int,
    cols: Int,
):
    """Row-major BF16 [rows, cols] GEMV for lm_head/vocab and router."""
    for r in range(rows):
        var acc: Float32 = 0.0
        var base = r * cols
        for c in range(cols):
            acc += bf16_to_f32(mat[base + c]) * vec[c]
        dst[r] = acc


def fp8_e4m3_to_f32(bits: UInt8) -> Float32:
    """E4M3FN decode, bias 7, 0x7f/0xff NaN (matches Rust fp8_e4m3fn_to_f32)."""
    var sign: Float32 = 1.0
    if (bits & UInt8(0x80)) != UInt8(0):
        sign = -1.0
    var exp = Int((bits >> 3) & UInt8(0x0F))
    var mant = Float32(Int(bits & UInt8(0x07)))
    if exp == 15:
        if mant == 7.0:
            var zero: Float32 = 0.0
            return zero / zero
        return sign * (1.0 + mant / 8.0) * exp2(Float32(8))
    if exp == 0:
        return sign * mant * exp2(Float32(-9))
    return sign * (1.0 + mant / 8.0) * exp2(Float32(exp - 7))


def row_gemv_fp8_block128(
    dst: Span[mut=True, Float32, _],
    mat: Span[UInt8, _],
    scales: Span[UInt16, _],
    vec: Span[Float32, _],
    rows: Int,
    cols: Int,
):
    """FP8 [rows, cols] row-major GEMV, cols % 128 == 0. BF16 inv scales laid
    out row-major over 128x128 blocks: scales[(r // 128) * (cols // 128) + b].
    Covers Qwen q/k/v/o projections and expert GEMMs."""
    var k_blocks = cols // 128
    for r in range(rows):
        var acc: Float32 = 0.0
        var base = r * cols
        for b in range(k_blocks):
            var scale = bf16_to_f32(scales[(r // 128) * k_blocks + b])
            for k in range(128):
                acc += fp8_e4m3_to_f32(mat[base + b * 128 + k]) * vec[b * 128 + k] * scale
        dst[r] = acc
