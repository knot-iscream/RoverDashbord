#!/usr/bin/env python3
"""Compare camera profiles and pick the best one by numbers, not by eye.

Only one process can hold the CSI camera, so stop the stream first:

    sudo systemctl stop rover-cam
    python3 measure.py /tmp/cam-quality
    sudo systemctl start rover-cam

Each candidate gets its own camera instance and a settle delay, then one JPEG
is saved and measured. The table reports mean luma, p5/p50/p95, the quietest
32x32 block (noise floor), mean R/G and B/G ratios (colour cast), and the
exposure and gain the AGC actually chose. A candidate whose exposure sits at
the frame limit is out of light and only more time or more gain will help.
"""

import io
import json
import sys
import time
import traceback
from pathlib import Path

import numpy as np
from PIL import Image

import camera_stream as cs

SETTLE_S = 3.0
BLOCK = 32

CANDIDATES = [
    ("c08", "watch", {"contrast": 0.8}),
    ("c08_s05", "watch", {"contrast": 0.8, "saturation": 0.5}),
    ("c08_s04", "watch", {"contrast": 0.8, "saturation": 0.4}),
    ("c08_manual", "watch", {"contrast": 0.8, "awb": "manual", "gains": "0.935,1.004"}),
    ("c08_manual_s05", "watch", {"contrast": 0.8, "awb": "manual", "gains": "0.935,1.004", "saturation": 0.5}),
]


def block_stats(luma):
    """Median and p90 of 32x32 block standard deviations (spatial grain)."""
    sigmas = []
    h, w = luma.shape
    for y in range(0, h - BLOCK + 1, BLOCK):
        for x in range(0, w - BLOCK + 1, BLOCK):
            sigmas.append(float(luma[y:y + BLOCK, x:x + BLOCK].std()))
    sigmas = np.array(sigmas, dtype=np.float32)
    return float(np.median(sigmas)), float(np.percentile(sigmas, 90))


def temporal_noise(a, b):
    """Median block change between two frames. Static scene, so this is grain."""
    ia = np.asarray(Image.open(io.BytesIO(a)).convert("L"), dtype=np.float32)
    ib = np.asarray(Image.open(io.BytesIO(b)).convert("L"), dtype=np.float32)
    diff = np.abs(ia - ib)
    h, w = diff.shape
    changes = []
    for y in range(0, h - BLOCK + 1, BLOCK):
        for x in range(0, w - BLOCK + 1, BLOCK):
            changes.append(float(diff[y:y + BLOCK, x:x + BLOCK].mean()))
    return float(np.median(np.array(changes, dtype=np.float32)))


def measure(path, second=None):
    img = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32)
    luma = img @ np.array([0.299, 0.587, 0.114], dtype=np.float32)
    mean = img.reshape(-1, 3).mean(axis=0)
    noise_med, noise_p90 = block_stats(luma)
    return {
        "luma": float(luma.mean()),
        "p5": float(np.percentile(luma, 5)),
        "p50": float(np.percentile(luma, 50)),
        "p95": float(np.percentile(luma, 95)),
        "noise_med": noise_med,
        "noise_p90": noise_p90,
        "temporal": temporal_noise(path.read_bytes(), second) if second else float("nan"),
        "r_g": float(mean[0] / max(mean[1], 1e-3)),
        "b_g": float(mean[2] / max(mean[1], 1e-3)),
    }


def run(name, profile, overrides, outdir):
    settings = cs.resolve(profile, overrides, use_env=False)
    cam = None
    try:
        cam = cs.open_camera(settings)
        time.sleep(SETTLE_S)
        buf = io.BytesIO()
        cam.capture_file(buf, format="jpeg")
        time.sleep(0.2)
        second = io.BytesIO()
        cam.capture_file(second, format="jpeg")
        meta = cam.capture_metadata()
    finally:
        if cam is not None:
            cam.stop()
            cam.close()
        # Give libcamera a moment to release the sensor before the next open.
        time.sleep(0.5)

    jpg = outdir / f"{name}.jpg"
    jpg.write_bytes(buf.getvalue())
    (outdir / f"{name}.json").write_text(
        json.dumps(
            {
                "settings": settings,
                "exposure_us": int(meta.get("ExposureTime", 0)),
                "analogue_gain": round(float(meta.get("AnalogueGain", 0)), 2),
                "digital_gain": round(float(meta.get("DigitalGain", 0)), 2),
                "lux": round(float(meta.get("Lux", 0)), 1),
                "colour_temp": int(meta.get("ColourTemperature", 0)),
                "colour_gains": [round(float(g), 3) for g in meta.get("ColourGains", (0, 0))],
            },
            indent=2,
        )
    )
    row = measure(jpg, second.getvalue())
    row.update(
        name=name,
        exposure_ms=round(meta.get("ExposureTime", 0) / 1000, 1),
        gain=round(float(meta.get("AnalogueGain", 0)), 2),
        lux=round(float(meta.get("Lux", 0)), 1),
        cct=int(meta.get("ColourTemperature", 0)),
        gains=[round(float(g), 3) for g in meta.get("ColourGains", (0, 0))],
    )
    return row


def main():
    outdir = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/cam-quality")
    outdir.mkdir(parents=True, exist_ok=True)

    rows = []

    def attempt(name, profile, overrides):
        try:
            rows.append(run(name, profile, overrides, outdir))
            print(f"captured {name}", flush=True)
        except Exception:
            print(f"skipped {name}", flush=True)
            traceback.print_exc()

    for name, profile, overrides in CANDIDATES:
        attempt(name, profile, overrides)

    header = f"{'candidate':18} {'luma':>5} {'p5':>5} {'p50':>5} {'p95':>5} " \
             f"{'n_med':>6} {'n_p90':>6} {'grain':>6} {'R/G':>5} {'B/G':>5} " \
             f"{'exp_ms':>6} {'gain':>5} {'CCT':>5}"
    print()
    print(header)
    print("-" * len(header))
    for row in rows:
        print(
            f"{row['name']:18} {row['luma']:5.1f} {row['p5']:5.1f} {row['p50']:5.1f} "
            f"{row['p95']:5.1f} {row['noise_med']:6.2f} {row['noise_p90']:6.2f} "
            f"{row['temporal']:6.2f} {row['r_g']:5.2f} {row['b_g']:5.2f} "
            f"{row['exposure_ms']:6.1f} {row['gain']:5.2f} {row['cct']:5d}"
        )
    print()
    print(f"images in {outdir}")


if __name__ == "__main__":
    main()
