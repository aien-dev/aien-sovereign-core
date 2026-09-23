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
from qwen3_moe import (
    Qwen3MoeShape,
    build_fp8_unpack_graph,
    build_qwen3_moe_graph,
    build_qwen3_moe_fp8_graph,
)


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
    parser.add_argument("--fp8", action="store_true", help="use native FP8 expert GEMMs")
    parser.add_argument("--seed", type=int, help="seed a random hidden vector instead of using ones")
    parser.add_argument("--tokens", type=int, default=1)
    args = parser.parse_args()
    if args.tokens < 1:
        parser.error("tokens must be positive")

    started = time.monotonic()
    packed = load_fp8_moe_layer(args.checkpoint_dir, args.layer)
    print(f"checkpoint layer packed in {time.monotonic() - started:.2f}s", flush=True)
    device = Accelerator()
    ref = DeviceRef.from_device(device)
    shape = Qwen3MoeShape(tokens=args.tokens)
    session = InferenceSession(devices=[device])
    if args.fp8:
        layer = session.init(session.compile(build_qwen3_moe_fp8_graph(shape, ref, return_ids=True)))
    else:
        unpack = session.init(session.compile(build_fp8_unpack_graph(shape, ref)))
        layer = session.init(session.compile(build_qwen3_moe_graph(shape, ref)))
    print("MAX graphs compiled", flush=True)

    router, gate_up, gate_up_scales, down, down_scales = packed.to_device(device)
    if args.fp8:
        weights = (gate_up, gate_up_scales, down, down_scales)
    else:
        resident_gate_up, resident_down = unpack.execute(
            gate_up, gate_up_scales, down, down_scales
        )
        del gate_up, gate_up_scales, down, down_scales
        weights = (resident_gate_up, resident_down)
    x = (np.random.default_rng(args.seed).normal(0.0, 0.25, (args.tokens, 2048)).astype(np.float32)
         if args.seed is not None else np.ones((args.tokens, 2048), dtype=np.float32))
    x_bits = (x.view(np.uint32) >> 16).astype(np.uint16)
    x = bf16_to_float(x_bits)
    x_device = Buffer.from_numpy(x_bits).view(DType.bfloat16).to(device)
    outputs = layer.execute(x_device, router, *weights)
    output = outputs[0]
    output_bits = output.to(CPU()).view(DType.uint16).to_numpy()
    actual = bf16_to_float(output_bits)
    if not np.isfinite(actual).all():
        raise AssertionError("MAX MoE output contains non-finite values")
    print(f"MAX output shape={actual.shape}, sample={actual[0, :8]}", flush=True)

    # Independent CPU reference decodes only the eight selected experts.
    router_f32 = bf16_to_float(packed.router_bf16)
    logits = x @ router_f32.T
    selected = np.argsort(-logits, axis=1, kind="stable")[:, :8]
    if args.fp8:
        gpu_ids = outputs[1].to(CPU()).to_numpy()
        gpu_logits = outputs[2].to(CPU()).to_numpy()
        expected_gpu_ids = np.argsort(-gpu_logits, axis=1, kind="stable")[:, :8]
        if not np.array_equal(gpu_ids, expected_gpu_ids):
            token = int(np.flatnonzero(np.any(gpu_ids != expected_gpu_ids, axis=1))[0])
            raise AssertionError(
                f"Mojo router diverges from GPU logits at token {token}: "
                f"Mojo={gpu_ids[token].tolist()} sorted={expected_gpu_ids[token].tolist()}"
            )
        logit_error = float(np.max(np.abs(gpu_logits - logits)))
        print(f"max_router_logit_error={logit_error:.6f}", flush=True)
        if logit_error > 0.02 * max(1.0, float(np.max(np.abs(logits)))):
            raise AssertionError("GPU router matmul diverges from BF16 CPU reference")
        # The GPU's BF16 reduction can reorder experts separated by only a
        # few ULPs. Use its logits for the independent expert computation.
        logits = gpu_logits
        selected = gpu_ids
    lut = fp8_lookup()
    expected = np.zeros_like(actual)
    for token in range(args.tokens):
        scores = np.exp(logits[token, selected[token]] - logits[token, selected[token, 0]])
        scores /= scores.sum()
        for expert, score in zip(selected[token], scores):
            gate = dequant(
                packed.gate_up_fp8[expert, :768],
                packed.gate_up_scales_bf16[expert, :6],
                lut,
            ) @ x[token]
            up = dequant(
                packed.gate_up_fp8[expert, 768:],
                packed.gate_up_scales_bf16[expert, 6:],
                lut,
            ) @ x[token]
            hidden = (gate / (1.0 + np.exp(-np.clip(gate, -80, 80)))) * up
            expert_out = dequant(
                packed.down_fp8[expert], packed.down_scales_bf16[expert], lut
            ) @ hidden
            expected[token] += score * expert_out
    absolute = np.abs(actual - expected)
    relative = absolute / np.maximum(np.abs(expected), 0.05)
    cosine = float(np.sum(actual * expected) / (np.linalg.norm(actual) * np.linalg.norm(expected)))
    print(
        f"layer={args.layer} tokens={args.tokens} selected_first={selected[0].tolist()} "
        f"max_abs={absolute.max():.6f} p95_rel={np.percentile(relative, 95):.6f} "
        f"cosine={cosine:.8f}",
        flush=True,
    )
    if cosine < (0.9995 if args.fp8 else 0.995) or np.percentile(relative, 95) > (0.10 if args.fp8 else 0.15):
        raise AssertionError("MAX MoE layer diverges from FP8 CPU reference")


if __name__ == "__main__":
    main()
