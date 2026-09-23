"""Measure routed Qwen3 expert layer throughput across AIEN branch tokens."""

import argparse
import json
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


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkpoint_dir", type=Path)
    parser.add_argument("--layer", type=int, default=0)
    parser.add_argument("--tokens", type=int, default=500)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--fp8", action="store_true", help="benchmark native FP8 expert GEMMs")
    args = parser.parse_args()
    if args.tokens < 1 or args.repeats < 1:
        parser.error("tokens and repeats must be positive")

    packed = load_fp8_moe_layer(args.checkpoint_dir, args.layer)
    device = Accelerator()
    ref = DeviceRef.from_device(device)
    session = InferenceSession(devices=[device])
    shape = Qwen3MoeShape(tokens=args.tokens)
    if args.fp8:
        layer = session.init(session.compile(build_qwen3_moe_fp8_graph(shape, ref)))
    else:
        unpack = session.init(session.compile(build_fp8_unpack_graph(shape, ref)))
        layer = session.init(session.compile(build_qwen3_moe_graph(shape, ref)))
    router, gate_up, gate_up_scales, down, down_scales = packed.to_device(device)
    if args.fp8:
        weights = (gate_up, gate_up_scales, down, down_scales)
    else:
        resident_gate_up, resident_down = unpack.execute(
            gate_up, gate_up_scales, down, down_scales
        )
        del gate_up, gate_up_scales, down, down_scales
        weights = (resident_gate_up, resident_down)

    rng = np.random.default_rng(42)
    x = rng.normal(0.0, 0.25, (args.tokens, 2048)).astype(np.float32)
    x_bits = (x.view(np.uint32) >> 16).astype(np.uint16)
    x = (x_bits.astype(np.uint32) << 16).view(np.float32)
    x_device = Buffer.from_numpy(x_bits).view(DType.bfloat16).to(device)

    def run_once() -> float:
        started = time.perf_counter()
        output = layer.execute(x_device, router, *weights)[0]
        output.to(CPU())  # Synchronize the device queue for a complete layer time.
        return time.perf_counter() - started

    run_once()
    timings = [run_once() for _ in range(args.repeats)]
    router_f32 = (packed.router_bf16.astype(np.uint32) << 16).view(np.float32)
    logits = x @ router_f32.T
    selected = np.argsort(-logits, axis=1, kind="stable")[:, :8]
    counts = np.bincount(selected.reshape(-1), minlength=128)
    active = counts[counts > 0]
    probability = active / active.sum()
    receipt = {
        "model": "Qwen3-Coder-30B-A3B-Instruct-FP8",
        "expert_compute": "fp8_mma" if args.fp8 else "bf16_grouped",
        "layer": args.layer,
        "tokens": args.tokens,
        "top_k": 8,
        "experts_touched": int(active.size),
        "expert_batch_p50": float(np.percentile(active, 50)),
        "expert_batch_p95": float(np.percentile(active, 95)),
        "routing_entropy_bits": float(-(probability * np.log2(probability)).sum()),
        "layer_latency_ms_median": float(np.median(timings) * 1e3),
        "layer_tokens_per_sec_median": float(args.tokens / np.median(timings)),
        "timings_sec": timings,
        "note": "one MoE layer, excludes attention, norms, embedding and sampling",
    }
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
