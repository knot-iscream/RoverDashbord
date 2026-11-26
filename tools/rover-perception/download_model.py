#!/usr/bin/env python3
"""Download the OpenCV Zoo NanoDet model into models/.

The model is NanoDet-m-plus-1.5x-416, COCO 80 classes, Apache-2.0.
It is not committed; run this once on the Pi (or copy the .onnx over).
"""

import hashlib
import os
import sys
import urllib.request

URL = (
    "https://media.githubusercontent.com/media/opencv/opencv_zoo/main/"
    "models/object_detection_nanodet/object_detection_nanodet_2022nov.onnx"
)
SHA256 = "4b82da9944b88577175ee23a459dce2e26e6e4be573def65b1055dc2d9720186"
HERE = os.path.dirname(os.path.abspath(__file__))
DEST = os.path.join(HERE, "models", "object_detection_nanodet_2022nov.onnx")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    os.makedirs(os.path.dirname(DEST), exist_ok=True)
    if os.path.exists(DEST) and sha256(DEST) == SHA256:
        print("model already present:", DEST)
        return
    if os.path.exists(DEST):
        print("model hash mismatch, re-downloading")

    part = DEST + ".part"
    print("downloading", URL)
    urllib.request.urlretrieve(URL, part)
    got = sha256(part)
    if got != SHA256:
        os.remove(part)
        raise SystemExit(f"hash mismatch: {got}")
    os.replace(part, DEST)
    print("saved", DEST)


if __name__ == "__main__":
    main()
