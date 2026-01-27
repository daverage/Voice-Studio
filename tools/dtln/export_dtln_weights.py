#!/usr/bin/env python3
"""Export ONNX DTLN weights into the native binary blob format."""
from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path
from typing import Any, Dict, List, Tuple

import onnx
from onnx import numpy_helper

ROOT = Path(__file__).resolve().parent.parent.parent
ASSETS_DIR = ROOT / "assets" / "dtln"
MODELS_DIR = ROOT / "src" / "assets" / "models" / "dtln"

BLOB_PATH = ASSETS_DIR / "dtln_weights_v1.bin"
MANIFEST_PATH = ASSETS_DIR / "dtln_weights_v1.manifest.json"

# Header constants for the binary format.
MAGIC = b"DTLN"
VERSION = 1
ENDIANNESS = 0  # little-endian
DTYPE_F32 = 0

# Tensor ordering definitions that must stay stable.
TensorDescriptor = Tuple[str, Tuple[int, ...], str]
TENSOR_ORDER: List[TensorDescriptor] = [
    # Stage 1 (model_1.onnx)
    ("lstm_4_W", (1, 512, 257), "stage1"),
    ("lstm_4_R", (1, 512, 128), "stage1"),
    ("lstm_4_B", (1, 1024), "stage1"),
    ("lstm_5_W", (1, 512, 128), "stage1"),
    ("lstm_5_R", (1, 512, 128), "stage1"),
    ("lstm_5_B", (1, 1024), "stage1"),
    ("dense_2/kernel:0", (128, 257), "stage1"),
    ("dense_2/bias:0", (257,), "stage1"),
    # Stage 2 (model_2.onnx)
    ("conv1d_2/kernel:0", (256, 512, 1), "stage2"),
    ("conv1d_3/kernel:0", (512, 256, 1), "stage2"),
    ("model_2/instant_layer_normalization_1/mul/ReadVariableOp/resource:0", (256,), "stage2"),
    ("model_2/instant_layer_normalization_1/add_1/ReadVariableOp/resource:0", (256,), "stage2"),
    ("model_2/lstm_6/MatMul/ReadVariableOp/resource:0", (256, 512), "stage2"),
    ("model_2/lstm_6/MatMul_1/ReadVariableOp/resource:0", (128, 512), "stage2"),
    ("model_2/lstm_6/BiasAdd/ReadVariableOp/resource:0", (512,), "stage2"),
    ("model_2/lstm_7/MatMul/ReadVariableOp/resource:0", (128, 512), "stage2"),
    ("model_2/lstm_7/MatMul_1/ReadVariableOp/resource:0", (128, 512), "stage2"),
    ("model_2/lstm_7/BiasAdd/ReadVariableOp/resource:0", (512,), "stage2"),
    ("model_2/dense_3/Tensordot/Reshape_1:0", (128, 256), "stage2"),
    ("model_2/dense_3/BiasAdd/ReadVariableOp/resource:0", (256,), "stage2"),
]


def load_initializers(model_path: Path) -> Dict[str, onnx.ValueInfoProto]:
    model = onnx.load(model_path)
    return {init.name: init for init in model.graph.initializer}


def float32_bytes_from_initializer(initializer: onnx.TensorProto) -> bytes:
    array = numpy_helper.to_array(initializer)
    return array.astype("float32", copy=False).tobytes(order="C")


def infer_shape(initializer: onnx.TensorProto) -> Tuple[int, ...]:
    return tuple(int(d) for d in initializer.dims)


def main() -> None:
    ASSETS_DIR.mkdir(parents=True, exist_ok=True)

    models: Dict[str, Dict[str, onnx.TensorProto]] = {
        "stage1": load_initializers(MODELS_DIR / "model_1.onnx"),
        "stage2": load_initializers(MODELS_DIR / "model_2.onnx"),
    }

    entries: List[Dict[str, Any]] = []
    for name, expected_shape, stage in TENSOR_ORDER:
        pool = models[stage]
        if name not in pool:
            raise RuntimeError(f"Missing tensor '{name}' in {stage}")
        init = pool[name]
        actual_shape = infer_shape(init)
        if actual_shape != expected_shape:
            raise RuntimeError(
                f"Shape mismatch for '{name}': expected {expected_shape}, got {actual_shape}"
            )
        data = float32_bytes_from_initializer(init)
        entries.append(
            {
                "name": name,
                "shape": list(actual_shape),
                "len": len(data) // 4,
                "bytes": data,
                "stage": stage,
            }
        )

    with open(BLOB_PATH, "wb") as blob:
        blob.write(MAGIC)
        blob.write(struct.pack("<I", VERSION))
        blob.write(struct.pack("<B", ENDIANNESS))
        blob.write(struct.pack("<B", DTYPE_F32))
        blob.write(struct.pack("<I", len(entries)))

        offset = 0
        for entry in entries:
            name_bytes = entry["name"].encode("utf-8")
            blob.write(struct.pack("<H", len(name_bytes)))
            blob.write(name_bytes)
            blob.write(struct.pack("<B", len(entry["shape"])))
            for dim in entry["shape"]:
                blob.write(struct.pack("<I", dim))
            blob.write(struct.pack("<I", offset))
            blob.write(struct.pack("<I", entry["len"]))
            offset += len(entry["bytes"])

        for entry in entries:
            blob.write(entry["bytes"])

    manifest = {
        "version": VERSION,
        "dtype": "f32",
        "tensor_count": len(entries),
        "tensors": [
            {
                "name": entry["name"],
                "shape": entry["shape"],
                "stage": entry["stage"],
                "len": entry["len"],
                "checksum": hashlib.sha256(entry["bytes"]).hexdigest(),
            }
            for entry in entries
        ],
    }

    with open(MANIFEST_PATH, "w", encoding="utf-8") as manifest_file:
        json.dump(manifest, manifest_file, indent=2)

    print(f"Wrote {BLOB_PATH} and {MANIFEST_PATH}")


if __name__ == "__main__":
    main()
