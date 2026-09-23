"""Run with `python crates/aien-inference-abi/max/test_qwen3_moe.py`."""

import unittest

import numpy as np
from max.driver import Accelerator, Buffer, CPU, accelerator_count
from max.dtype import DType
from max.engine import InferenceSession
from max.graph import DeviceRef

from qwen3_moe import Qwen3MoeShape, build_fp8_unpack_graph, build_qwen3_moe_graph


def bf16_buffer(values: np.ndarray, device: Accelerator) -> Buffer:
    bits = (values.astype(np.float32).view(np.uint32) >> 16).astype(np.uint16)
    return Buffer.from_numpy(bits).view(DType.bfloat16).to(device)


def fp8_buffer(raw: np.ndarray, device: Accelerator) -> Buffer:
    assert raw.dtype == np.uint8
    return Buffer.from_numpy(raw).view(DType.float8_e4m3fn).to(device)


def read_bf16(buffer: Buffer) -> np.ndarray:
    bits = buffer.to(CPU()).view(DType.uint16).to_numpy().astype(np.uint32) << 16
    return bits.view(np.float32)


@unittest.skipUnless(accelerator_count() > 0, "requires a MAX accelerator")
class Qwen3MoeGpuTest(unittest.TestCase):
    def test_fp8_upload_and_complete_routed_layer(self) -> None:
        shape = Qwen3MoeShape(tokens=2, hidden=128, intermediate=128)
        device = Accelerator()
        device_ref = DeviceRef.from_device(device)
        session = InferenceSession(devices=[device])
        unpack = session.init(session.compile(build_fp8_unpack_graph(shape, device_ref)))
        layer = session.init(session.compile(build_qwen3_moe_graph(shape, device_ref)))

        # 0x38 is exact FP8 E4M3 1.0. All 128 experts carry identity
        # gate/up/down projections so routing and weighted reduction have
        # a nonzero, independently calculable result.
        gate_up = np.zeros((128, 256, 128), dtype=np.uint8)
        down = np.zeros((128, 128, 128), dtype=np.uint8)
        for expert in range(128):
            for channel in range(128):
                gate_up[expert, channel, channel] = 0x38
                gate_up[expert, 128 + channel, channel] = 0x38
                down[expert, channel, channel] = 0x38
        gate_up_scales = np.ones((128, 2, 1), dtype=np.float32)
        down_scales = np.ones((128, 1, 1), dtype=np.float32)
        resident_gate_up, resident_down = unpack.execute(
            fp8_buffer(gate_up, device),
            bf16_buffer(gate_up_scales, device),
            fp8_buffer(down, device),
            bf16_buffer(down_scales, device),
        )
        self.assertEqual(resident_gate_up.dtype, DType.bfloat16)
        self.assertEqual(resident_down.dtype, DType.bfloat16)
        self.assertEqual(read_bf16(resident_gate_up)[0, 0, 0], 1.0)

        activations = np.ones((2, 128), dtype=np.float32)
        router = np.zeros((128, 128), dtype=np.float32)
        result = layer.execute(
            bf16_buffer(activations, device),
            bf16_buffer(router, device),
            resident_gate_up,
            resident_down,
        )[0]
        actual = read_bf16(result)
        expected = 1.0 / (1.0 + np.exp(-1.0))
        self.assertEqual(actual.shape, (2, 128))
        self.assertLess(np.max(np.abs(actual - expected)), 0.003)


if __name__ == "__main__":
    unittest.main()
