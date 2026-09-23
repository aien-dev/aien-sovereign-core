"""Strict, sharded Qwen3-Coder-A3B FP8 expert ingest for MAX.

This reads raw safetensors bytes with mmap. Expert weights remain uint8 FP8
payloads and inverse scales remain BF16 bits until MAX receives them.
"""

from contextlib import ExitStack
from dataclasses import dataclass
import json
import mmap
from pathlib import Path
import struct

import numpy as np
from max.driver import Buffer, Device
from max.dtype import DType


@dataclass
class PackedMoeLayer:
    router_bf16: np.ndarray
    gate_up_fp8: np.ndarray
    gate_up_scales_bf16: np.ndarray
    down_fp8: np.ndarray
    down_scales_bf16: np.ndarray

    def to_device(self, device: Device) -> tuple[Buffer, Buffer, Buffer, Buffer, Buffer]:
        def bf16(bits: np.ndarray) -> Buffer:
            return Buffer.from_numpy(bits).view(DType.bfloat16).to(device)

        def fp8(bits: np.ndarray) -> Buffer:
            return Buffer.from_numpy(bits).view(DType.float8_e4m3fn).to(device)

        return (
            bf16(self.router_bf16),
            fp8(self.gate_up_fp8),
            bf16(self.gate_up_scales_bf16),
            fp8(self.down_fp8),
            bf16(self.down_scales_bf16),
        )


class SafetensorsShard:
    def __init__(self, path: Path) -> None:
        self._file = path.open("rb")
        self._mapping = mmap.mmap(self._file.fileno(), 0, access=mmap.ACCESS_READ)
        if len(self._mapping) < 8:
            raise ValueError(f"truncated safetensors shard: {path}")
        header_size = struct.unpack_from("<Q", self._mapping, 0)[0]
        self._payload_start = 8 + header_size
        if self._payload_start > len(self._mapping):
            raise ValueError(f"invalid safetensors header size: {path}")
        self._header = json.loads(self._mapping[8 : self._payload_start])

    def __enter__(self) -> "SafetensorsShard":
        return self

    def __exit__(self, *_unused: object) -> None:
        self._mapping.close()
        self._file.close()

    def tensor(self, name: str, dtype: str, shape: tuple[int, ...]) -> np.ndarray:
        entry = self._header.get(name)
        if entry is None:
            raise KeyError(f"missing checkpoint tensor: {name}")
        if entry["dtype"] != dtype or tuple(entry["shape"]) != shape:
            raise ValueError(f"checkpoint tensor contract mismatch: {name}: {entry}")
        raw_dtype = {"F8_E4M3": np.uint8, "BF16": np.uint16}[dtype]
        start, end = entry["data_offsets"]
        byte_count = int(np.prod(shape)) * np.dtype(raw_dtype).itemsize
        if start < 0 or end - start != byte_count or self._payload_start + end > len(self._mapping):
            raise ValueError(f"checkpoint tensor byte range mismatch: {name}")
        return np.ndarray(
            shape,
            dtype=raw_dtype,
            buffer=self._mapping,
            offset=self._payload_start + start,
        )


def load_fp8_moe_layer(checkpoint_dir: Path, layer: int) -> PackedMoeLayer:
    """Pack one official FP8 layer without decoding any expert matrix to FP32."""
    if layer < 0 or layer >= 48:
        raise ValueError("Qwen3-Coder-30B-A3B layer must be in [0, 48)")
    index_path = checkpoint_dir / "model.safetensors.index.json"
    index = json.loads(index_path.read_text())
    weight_map = index["weight_map"]
    prefix = f"model.layers.{layer}.mlp"

    with ExitStack() as stack:
        shards: dict[str, SafetensorsShard] = {}

        def tensor(name: str, dtype: str, shape: tuple[int, ...]) -> np.ndarray:
            filename = weight_map.get(name)
            if filename is None:
                raise KeyError(f"missing checkpoint index entry: {name}")
            if filename not in shards:
                shards[filename] = stack.enter_context(SafetensorsShard(checkpoint_dir / filename))
            return shards[filename].tensor(name, dtype, shape)

        router = np.array(tensor(f"{prefix}.gate.weight", "BF16", (128, 2048)))
        gate_up = np.empty((128, 1536, 2048), dtype=np.uint8)
        gate_up_scales = np.empty((128, 12, 16), dtype=np.uint16)
        down = np.empty((128, 2048, 768), dtype=np.uint8)
        down_scales = np.empty((128, 16, 6), dtype=np.uint16)
        for expert in range(128):
            base = f"{prefix}.experts.{expert}"
            for offset, projection in ((0, "gate_proj"), (768, "up_proj")):
                name = f"{base}.{projection}"
                gate_up[expert, offset : offset + 768] = tensor(
                    f"{name}.weight", "F8_E4M3", (768, 2048)
                )
                gate_up_scales[expert, offset // 128 : (offset + 768) // 128] = tensor(
                    f"{name}.weight_scale_inv", "BF16", (6, 16)
                )
            down[expert] = tensor(
                f"{base}.down_proj.weight", "F8_E4M3", (2048, 768)
            )
            down_scales[expert] = tensor(
                f"{base}.down_proj.weight_scale_inv", "BF16", (16, 6)
            )

    return PackedMoeLayer(router, gate_up, gate_up_scales, down, down_scales)
