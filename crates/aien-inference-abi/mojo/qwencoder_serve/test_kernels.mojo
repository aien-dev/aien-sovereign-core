# Qwen3-Coder hand-computed kernel oracles. Run:
#   nice -n 10 mojo run mojo-kernels/test_kernels.mojo
# Imports sibling via explicit path load (same dir, no package needed).

from std.testing import assert_true, TestSuite
from qwencoder_kernels import (
    rmsnorm_row,
    rope_rotate_half_pair,
    silu,
    swiglu_row,
    repeat_kv_head,
    row_gemv,
    bf16_to_f32,
    row_gemv_bf16,
    fp8_e4m3_to_f32,
    row_gemv_fp8_block128,
)


def close(a: Float32, b: Float64, tol: Float64) -> Bool:
    var bb = Float32(b)
    var tt = Float32(tol)
    var d = a - bb
    if d < 0.0:
        d = -d
    return d <= tt


def test_rmsnorm_ones() raises:
    # Qwen eps 1e-6. ones, weight 2.0 -> 2.0
    var x: List[Float32] = [1.0, 1.0, 1.0, 1.0]
    var w: List[Float32] = [2.0, 2.0, 2.0, 2.0]
    var o: List[Float32] = [0.0, 0.0, 0.0, 0.0]
    rmsnorm_row(x, w, o, Float32(1e-6))
    assert_true(close(o[0], 2.0, 1e-4))
    assert_true(close(o[3], 2.0, 1e-4))


def test_rope_identity() raises:
    var r = rope_rotate_half_pair(1.0, 0.0, 1.0, 0.0)
    assert_true(close(r[0], 1.0, 1e-6))
    assert_true(close(r[1], 0.0, 1e-6))


def test_swiglu_zero_gate() raises:
    var g: List[Float32] = [0.0, 1.0]
    var u: List[Float32] = [5.0, 5.0]
    var o: List[Float32] = [0.0, 0.0]
    swiglu_row(g, u, o)
    assert_true(close(o[0], 0.0, 1e-6))
    # silu(1) = 1/(1+e^-1) ~= 0.7310586; *5 ~= 3.65529
    assert_true(close(o[1], 3.65529, 1e-3))


def test_repeat_kv() raises:
    var kv: List[Float32] = [1.0, 2.0]
    var o: List[Float32] = [0.0, 0.0, 0.0, 0.0]
    repeat_kv_head(kv, o, 2, 2)
    assert_true(o[0] == 1.0 and o[1] == 2.0 and o[2] == 1.0 and o[3] == 2.0)


def test_gemv_hand() raises:
    # 2x3 matrix, vec [1,10,100] -> row0 = 1+20+300=321, row1 = 4+50+600=654
    var m: List[Float32] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    var v: List[Float32] = [1.0, 10.0, 100.0]
    var o: List[Float32] = [0.0, 0.0]
    row_gemv(o, m, v, 2, 3)
    assert_true(close(o[0], 321.0, 1e-3))
    assert_true(close(o[1], 654.0, 1e-3))


def test_bf16_decode() raises:
    # 0x3F80 = 1.0, 0xC000 = -2.0 (hand-decoded from sign/exp/mantissa)
    assert_true(close(bf16_to_f32(UInt16(0x3F80)), 1.0, 1e-6))
    assert_true(close(bf16_to_f32(UInt16(0xC000)), -2.0, 1e-6))


def test_bf16_gemv_hand() raises:
    # [[1,2],[3,4]] . [1,1] = [3,7]; BF16 codes 1.0=0x3F80 2.0=0x4000 3.0=0x4040 4.0=0x4080
    var m = List[UInt16]()
    m.append(UInt16(0x3F80))
    m.append(UInt16(0x4000))
    m.append(UInt16(0x4040))
    m.append(UInt16(0x4080))
    var v: List[Float32] = [1.0, 1.0]
    var o: List[Float32] = [0.0, 0.0]
    row_gemv_bf16(o, m, v, 2, 2)
    assert_true(close(o[0], 3.0, 1e-3))
    assert_true(close(o[1], 7.0, 1e-3))


def test_fp8_decode() raises:
    # 0x38 = 1.0 (exp 7, mant 0); 0xBC = -1.5 (sign, exp 7, mant 4); 0x00 = 0.0
    assert_true(close(fp8_e4m3_to_f32(UInt8(0x38)), 1.0, 1e-6))
    assert_true(close(fp8_e4m3_to_f32(UInt8(0xBC)), -1.5, 1e-6))
    assert_true(close(fp8_e4m3_to_f32(UInt8(0x00)), 0.0, 1e-6))


def test_fp8_block128_gemv_hand() raises:
    # 2x128 all 0x38 (=1.0), one scale block of 1.0, vec ones(128) -> 128.0 per row
    var w = List[UInt8]()
    for _ in range(256):
        w.append(UInt8(0x38))
    var s = List[UInt16]()
    s.append(UInt16(0x3F80))
    var v = List[Float32]()
    for _ in range(128):
        v.append(1.0)
    var o: List[Float32] = [0.0, 0.0]
    row_gemv_fp8_block128(o, w, s, v, 2, 128)
    assert_true(close(o[0], 128.0, 1e-2))
    assert_true(close(o[1], 128.0, 1e-2))
    # same weights, scale 2.0 (0x4000) -> 256.0 per row
    var s2 = List[UInt16]()
    s2.append(UInt16(0x4000))
    row_gemv_fp8_block128(o, w, s2, v, 2, 128)
    assert_true(close(o[0], 256.0, 1e-2))
    assert_true(close(o[1], 256.0, 1e-2))


def main() raises:
    TestSuite.discover_tests[__functions_in_module()]().run()
