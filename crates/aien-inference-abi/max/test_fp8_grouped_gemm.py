"""Standalone grouped FP8 tensor-core parity test on GB10."""

from pathlib import Path
import unittest

import numpy as np
from max.driver import Accelerator, Buffer, CPU, accelerator_count
from max.dtype import DType
from max.engine import InferenceSession
from max.graph import DeviceRef, Graph, TensorType, ops


def fp8_buffer(bits: np.ndarray, device: Accelerator) -> Buffer:
    return Buffer.from_numpy(bits.astype(np.uint8)).view(DType.float8_e4m3fn).to(device)


def bf16_buffer(values: np.ndarray, device: Accelerator) -> Buffer:
    bits = (values.astype(np.float32).view(np.uint32) >> 16).astype(np.uint16)
    return Buffer.from_numpy(bits).view(DType.bfloat16).to(device)


@unittest.skipUnless(accelerator_count() > 0, "requires a MAX accelerator")
class GroupedFp8GemmTest(unittest.TestCase):
    def test_ragged_group_and_block_scales(self) -> None:
        device = Accelerator()
        ref = DeviceRef.from_device(device)
        rows, experts, channels = 26, 2, 256

        def forward(activations, activation_scales, weights, weight_scales, offsets, expert_ids):
            return ops.custom(
                name="aien.qwen3_moe.grouped_fp8_gemm",
                device=ref,
                values=[activations, activation_scales, weights, weight_scales, offsets, expert_ids],
                out_types=[TensorType(DType.float32, [rows, channels], device=ref)],
            )[0].tensor

        graph = Graph(
            "aien_grouped_fp8_parity",
            forward=forward,
            input_types=[
                TensorType(DType.float8_e4m3fn, [rows, channels], device=ref),
                TensorType(DType.float32, [rows, 2], device=ref),
                TensorType(DType.float8_e4m3fn, [experts, channels, channels], device=ref),
                TensorType(DType.bfloat16, [experts, 2, 2], device=ref),
                TensorType(DType.uint32, [experts + 1], device=ref),
                TensorType(DType.int32, [experts], device=ref),
            ],
            custom_extensions=[Path(__file__).parent / "kernels"],
        )
        session = InferenceSession(devices=[device])
        model = session.init(session.compile(graph))
        # All values are exactly representable E4M3 values, with distinct
        # rows/columns/experts to expose fragment mapping and grouped offsets.
        values = np.array([0.0, 0.5, 1.0, 2.0, -0.5, -1.0, -2.0], dtype=np.float32)
        bits = np.array([0x00, 0x30, 0x38, 0x40, 0xB0, 0xB8, 0xC0], dtype=np.uint8)
        row_indices = (np.arange(rows)[:, None] * 3 + np.arange(channels)[None, :] * 5) % 7
        weight_indices = (
            np.arange(experts)[:, None, None] * 2
            + np.arange(channels)[None, :, None] * 3
            + np.arange(channels)[None, None, :] * 5
        ) % 7
        a = bits[row_indices]
        w = bits[weight_indices]
        a_scale = np.where(np.arange(rows)[:, None] % 2 == 0, 1.0, 0.5).astype(np.float32) * np.array([1.0, 2.0], dtype=np.float32)
        w_scale = np.array([[[1.0, 0.5], [2.0, 1.0]], [[0.5, 2.0], [1.0, 0.5]]], dtype=np.float32)
        offsets = np.array([0, 19, rows], dtype=np.uint32)
        expert_ids = np.array([1, 0], dtype=np.int32)
        actual = model.execute(
            fp8_buffer(a, device),
            Buffer.from_numpy(a_scale).to(device),
            fp8_buffer(w, device),
            bf16_buffer(w_scale, device),
            Buffer.from_numpy(offsets).to(device),
            Buffer.from_numpy(expert_ids).to(device),
        )[0].to(CPU()).to_numpy()
        expected = np.empty_like(actual)
        for row in range(rows):
            expert = int(expert_ids[0 if row < offsets[1] else 1])
            for output in range(channels):
                total = 0.0
                for block in range(2):
                    av = values[row_indices[row, block * 128 : (block + 1) * 128]]
                    wv = values[weight_indices[expert, output, block * 128 : (block + 1) * 128]]
                    total += float(np.dot(av, wv)) * float(a_scale[row, block]) * float(w_scale[expert, output // 128, block])
                expected[row, output] = total
        np.testing.assert_allclose(actual, expected, rtol=0, atol=2e-4)


if __name__ == "__main__":
    unittest.main()
