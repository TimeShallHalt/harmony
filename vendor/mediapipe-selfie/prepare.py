"""Normalize the pinned SelfieBarracuda ONNX export for RTen.

Pass the original model and a destination. The transposed convolution uses
explicit zero padding and already produces the declared output dimensions.
The weights, operators and model inputs are preserved.
"""
import hashlib
from pathlib import Path
import sys

import onnx

ORIGINAL_SHA256 = "ce062b4830ec7b2be2a328fcf5f7029b19d1b67b9662ac6ae5f40cb67d0d2bc1"


def main():
    source, destination = map(Path, sys.argv[1:])
    data = source.read_bytes()
    if hashlib.sha256(data).hexdigest() != ORIGINAL_SHA256:
        raise ValueError("Expected the pinned landscape model")
    model = onnx.load_model_from_string(data)
    node, = (node for node in model.graph.node if node.op_type == "ConvTranspose")
    attrs = {attr.name: onnx.helper.get_attribute_value(attr) for attr in node.attribute}
    assert attrs["pads"] == [0, 0, 0, 0] and attrs["output_shape"] == [144, 256]
    keep = [attr for attr in node.attribute if attr.name != "output_shape"]
    del node.attribute[:]
    node.attribute.extend(keep)
    onnx.checker.check_model(model)
    onnx.save_model(model, destination)


if __name__ == "__main__":
    main()
