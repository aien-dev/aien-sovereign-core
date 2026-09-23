"""Verify one official FP8 Qwen3-Coder-A3B MoE layer on GB10.

Usage: python verify_real_layer.py /path/to/checkpoint-dir --layer 0
Only the shard containing the selected layer is required.
"""

import argparse
from pathlib import Path
import time

import numpy as np
from max.driver import Accelerator, Buffer, CPU
from max.dtype import DType
from max.engine import InferenceSession
from max.graph import DeviceRef

from checkpoint_fp8 import load_fp8_moe_layer
from qwen3_moe import Qwen3MoeShape, build_fp8_unpack_graph, build_qwen3_moe_graph


def bf16_to_float(bits: np.ndarray) -> np.ndarray:
    return (bits.astype(np.uint32) << 16).view(np.float32)


def fp8_lookup() -> np.ndarray:
    bits = np.arange(256, dtype=np.uint16)
    exponent = (bits >> 3) & 15
    mantissa = bits & 7
    value = np.where(
        exponent == 0,
        np.ldexp(mantissa.astype(np.float32), -9),
        np.ldexp(1.0 + mantissa.astype(np.float32) / 8.0, exponent.astype(int) - 7),
    )
    value = np.where((bits & 128) != 0, -value, value)
    value[(exponent == 15) & (mantissa == 7)] = np.nan
    return value


def dequant(raw: np.ndarray, scales: np.ndarray, lut: np.ndarray) -> np.ndarray:
    rows, columns = raw.shape
    values = lut[raw].reshape(rows // 128, 128, columns // 128, 128)
    scale_f32 = bf16_to_float(scales)
    return (values * scale_f32[:, None, :, None]).reshape(rows, columns)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkpoint_dir", type=Path)
    parser.add_argument("--layer", type=int, default=0)
    args = parser.parse_args()

    started = time.monotonic()
    packed = load_fp8_moe_layer(args.checkpoint_dir, args.layer)
    print(f"checkpoint layer packed in {time.monotonic() - started:.2f}s", flush=True)
    device = Accelerator()
    ref = DeviceRef.from_device(device)
    shape = Qwen3MoeShape(tokens=1)
    session = InferenceSession(devices=[device])
    unpack = session.init(session.compile(build_fp8_unpack_graph(shape, ref)))
    layer = session.init(session.compile(build_qwen3_moe_graph(shape, ref)))
    print("MAX graphs compiled", flush=True)

    router, gate_up, gate_up_scales, down, down_scales = packed.to_device(device)
    resident_gate_up, resident_down = unpack.execute(
        gate_up, gate_up_scales, down, down_scales
    )
    del gate_up, gate_up_scales, down, down_scales
    x = np.ones((1, 2048), dtype=np.float32)
    x_bits = (x.view(np.uint32) >> 16).astype(np.uint16)
    x_device = Buffer.from_numpy(x_bits).view(DType.bfloat16).to(device)
    output = layer.execute(x_device, router, resident_gate_up, resident_down)[0]
    output_bits = output.to(CPU()).view(DType.uint16).to_numpy()
    actual = bf16_to_float(output_bits)[0]
    if not np.isfinite(actual).all():
        raise AssertionError("MAX MoE output contains non-finite values")
    print(f"MAX output shape={actual.shape}, sample={actual[:8]}", flush=True)

    # Independent CPU reference decodes only the eight selected experts.
    router_f32 = bf16_to_float(packed.router_bf16)
    logits = router_f32 @ x[0]
    selected = np.argsort(-logits, kind="stable")[:8]
    scores = np.exp(logits[selected] - logits[selected[0]])
    scores /= scores.sum()
    lut = fp8_lookup()
    expected = np.zeros(2048, dtype=np.float32)
    for expert, score in zip(selected, scores):
        gate = dequant(
            packed.gate_up_fp8[expert, :768],
            packed.gate_up_scales_bf16[expert, :6],
            lut,
        ) @ x[0]
        up = dequant(
            packed.gate_up_fp8[expert, 768:],
            packed.gate_up_scales_bf16[expert, 6:],
            lut,
        ) @ x[0]
        hidden = (gate / (1.0 + np.exp(-np.clip(gate, -80, 80)))) * up
        expert_out = dequant(
            packed.down_fp8[expert], packed.down_scales_bf16[expert], lut
        ) @ hidden
        expected += score * expert_out
    absolute = np.abs(actual - expected)
    relative = absolute / np.maximum(np.abs(expected), 0.05)
    cosine = float(actual @ expected / (np.linalg.norm(actual) * np.linalg.norm(expected)))
    print(
        f"layer={args.layer} selected={selected.tolist()} "
        f"max_abs={absolute.max():.6f} p95_rel={np.percentile(relative, 95):.6f} "
        f"cosine={cosine:.8f}",
        flush=True,
    )
    if cosine < 0.995 or np.percentile(relative, 95) > 0.15:
        raise AssertionError("MAX MoE layer diverges from FP8 CPU reference")


if __name__ == "__main__":
    main()
