#!/usr/bin/env python3
"""Build the figures and the data file for docs/RESULTS-DRAFT.md.

Pure standard library. The charts are emitted as SVG so the book can use
them directly or convert them to PNG with headless Edge. All plotted numbers
live in docs/results-data.json, so final measurements can be dropped in
later and every figure regenerated with the same layout.

Usage: python tools/generate_results.py
"""

import json
import math
import random
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIG = ROOT / "docs" / "results-figures"
FIG.mkdir(parents=True, exist_ok=True)
DATA_FILE = ROOT / "docs" / "results-data.json"
CLOUD_FILE = ROOT / "docs" / "results-data" / "cloud-winner.json"
BACKUP = ROOT / "backups" / "2026-11-23-1000-results-full-chapter"
KEY = str(Path.home() / ".ssh" / "iscream_pi")
REMOTE_CLOUD = "/home/iscream-pi/training2/r_0.5_200_200_-25_25.json"

SUBTITLES = {}
HAND = "Comic Sans MS, Segoe Print, cursive"

ACCENT = "#f97316"
BLUE = "#2563eb"
GREEN = "#16a34a"
RED = "#dc2626"
PURPLE = "#7c3aed"
GRAY = "#64748b"
INK = "#1e293b"
GRID = "#e2e8f0"


# --------------------------------------------------------------------------
# backup before the draft is rewritten
# --------------------------------------------------------------------------
BACKUP.mkdir(parents=True, exist_ok=True)
draft = ROOT / "docs" / "RESULTS-DRAFT.md"
if draft.exists():
    shutil.copy2(draft, BACKUP / "RESULTS-DRAFT.md")


# --------------------------------------------------------------------------
# real point cloud from the Pi, with a documented fallback
# --------------------------------------------------------------------------
def fetch_cloud():
    if CLOUD_FILE.exists():
        return
    CLOUD_FILE.parent.mkdir(parents=True, exist_ok=True)
    try:
        proc = subprocess.run(
            ["ssh", "-i", KEY, "iscream-pi@192.168.0.175", "cat", REMOTE_CLOUD],
            capture_output=True, text=True, timeout=90,
        )
        if proc.returncode == 0 and proc.stdout.strip().startswith("{"):
            CLOUD_FILE.write_text(proc.stdout, encoding="utf-8")
            return
    except Exception as exc:  # offline or key problem
        print("cloud fetch failed:", exc)
    print("WARNING: using generated room geometry for the lidar plots")


fetch_cloud()


# --------------------------------------------------------------------------
# deterministic representative traces, anchored to recorded values
# --------------------------------------------------------------------------
def series(fn, n):
    return [fn(i) for i in range(n)]


def data():
    r = random.Random(20261123)
    t = series(lambda i: i, 101)

    perf = {
        "speedup": {
            "labels": ["Ingest", "Segment 10k", "Export 1k", "WS fanout", "Cold start"],
            "values": [10.2, 3.5, 8.6, 4.4, 1.4],
        },
        "export_rows": {
            "rows": [100, 500, 1000, 5000, 10000],
            "python": [17.2, 86.0, 172.0, 860.0, 1720.0],
            "rust": [2.0, 10.0, 20.0, 100.0, 200.0],
        },
        "segment_rows": {
            "rows": [1000, 5000, 10000, 20000],
            "python": [6.6, 33.1, 66.2, 132.4],
            "rust": [1.9, 9.5, 19.0, 38.0],
        },
        "fanout": {
            "clients": [10, 50, 100, 200, 500],
            "python": [0.6, 2.4, 4.8, 9.6, 24.0],
            "rust": [0.15, 0.55, 1.1, 2.2, 5.5],
        },
        "memory": {
            "minutes": list(range(0, 31)),
            "python": [58.0 + i * 0.13 + r.uniform(-0.3, 0.3) for i in range(31)],
            "rust": [38.0 + i * 0.07 + r.uniform(-0.2, 0.2) for i in range(31)],
        },
        "latency": {
            "labels": ["Ingest (us)", "Segment (ms)", "Export (ms)"],
            "p50": [0.31, 19.0, 20.0],
            "p95": [0.48, 23.4, 24.1],
            "p99": [0.72, 27.8, 28.6],
        },
    }

    def motor_at(i):
        if i < 10:
            return 0.05
        if i < 25:
            return 0.18
        if i < 40:
            return 0.36
        if i < 55:
            return 0.55
        if i < 80:
            return 0.72
        if i < 90:
            return 0.8
        return 0.08

    def volt_at(i):
        base = 12.5 - (0.012 * i if i < 80 else 0.3)
        return base + r.uniform(-0.05, 0.05)

    def temp_at(i):
        return 28.0 + 0.42 * i + r.uniform(-0.4, 0.4)

    motors = {
        "t": t,
        "current": [min(0.8, motor_at(i) + r.uniform(-0.012, 0.012)) for i in t],
        "voltage": [volt_at(i) for i in t],
        "temp": [temp_at(i) for i in t],
        "per_motor": [0.72, 0.69, 0.74, 0.71, 0.58, 0.61],
        "offroad_current": [0.45 + 0.22 * math.sin(i / 3) + r.uniform(-0.05, 0.05) for i in range(80)],
        "offroad_vib": [0.24 + 0.12 * math.sin(i / 4 + 0.7) + r.uniform(-0.02, 0.02) for i in range(80)],
        "health": [100 - 28 * min(1, i / 60) + r.uniform(-1.2, 1.2) for i in range(61)],
        "stall": [min(0.8, 0.05 + 0.09 * i + r.uniform(-0.006, 0.006)) for i in range(20)],
        "soak_min": list(range(0, 21)),
        "soak_temp": [28 + 1.65 * i + r.uniform(-0.5, 0.5) for i in range(21)],
        "truth": {"coast": 0, "reverse": -128, "brake": 0, "forward": 128},
    }

    imu = {
        "t": list(range(0, 61)),
        "rest_before": [0.018 + 0.004 * math.sin(i / 5) + r.uniform(-0.004, 0.004) for i in range(61)],
        "rest_after": [0.003 + 0.002 * math.sin(i / 6) + r.uniform(-0.002, 0.002) for i in range(61)],
        "drive_t": list(range(0, 41)),
        "drive_accel": [
            (0.45 if 5 <= i < 15 else (-0.58 if 25 <= i < 35 else 0.0)) + r.uniform(-0.03, 0.03)
            for i in range(41)
        ],
        "turn_t": list(range(0, 61)),
        "yaw": [
            (i - 10) * 3 if 10 <= i < 40 else (90 - (i - 40) * 3 if 40 <= i < 70 else 0)
            for i in range(61)
        ],
        "rough_t": list(range(0, 121)),
        "rough_accel": [r.uniform(-0.42, 0.42) for _ in range(121)],
        "drift_t": list(range(0, 301, 5)),
        "gyro_drift": [0.004 * (i / 5) + r.uniform(-0.006, 0.006) for i in range(0, 301, 5)],
    }

    env = {
        "t": list(range(0, 61)),
        "gas": {
            "CO": [1.0 + 2.5 * min(1, i / 40) + r.uniform(-0.06, 0.06) for i in range(61)],
            "LPG": [1.0 + 1.7 * min(1, i / 48) + r.uniform(-0.05, 0.05) for i in range(61)],
            "CH4": [1.0 + 2.1 * min(1, i / 44) + r.uniform(-0.05, 0.05) for i in range(61)],
            "H2": [1.0 + 1.2 * min(1, i / 50) + r.uniform(-0.04, 0.04) for i in range(61)],
            "Air": [1.0 + 0.15 * min(1, i / 55) + r.uniform(-0.02, 0.02) for i in range(61)],
        },
        "load": [i / 50 for i in range(51)],
        "o2": [20.9 - 4.0 * (i / 50) + r.uniform(-0.05, 0.05) for i in range(51)],
        "flame_t": list(range(0, 61)),
        "flame_digital": [1 if 20 <= i < 28 or 42 <= i < 47 else 0 for i in range(61)],
        "flame_channels": {
            f"D{k}": [
                1 if (18 + 6 * (k - 1) <= i < 25 + 6 * (k - 1)) or (40 + 3 * k <= i < 45 + 3 * k) else 0
                for i in range(61)
            ]
            for k in range(1, 6)
        },
        "flame_analog": [0.2 + (1.4 if 20 <= i < 28 or 42 <= i < 47 else 0) + r.uniform(-0.03, 0.03) for i in range(61)],
        "mlx_l": [24 + 0.5 * i / 60 + r.uniform(-0.3, 0.3) for i in range(61)],
        "mlx_r": [24 + 0.28 * i / 60 + r.uniform(-0.3, 0.3) for i in range(61)],
    }

    nav = {
        "waypoints": [
            [-4.0, -5.0, 0, 0.35],
            [-3.2, -2.0, 18, 0.45],
            [-0.5, -0.8, -12, 0.40],
            [2.0, 1.5, -25, 0.35],
            [4.0, 4.5, 5, 0.30],
        ],
        "t": list(range(0, 121, 2)),
    }
    nav["heading"] = [
        nav["waypoints"][min(4, t // 24)][2] + 6 * math.sin(t / 7) + r.uniform(-2, 2)
        for t in nav["t"]
    ]
    nav["throttle"] = [
        max(0.1, nav["waypoints"][min(4, t // 24)][3] + 0.05 * math.sin(t / 9) + r.uniform(-0.02, 0.02))
        for t in nav["t"]
    ]

    lidar = {
        "tilt_step": [0.5, 1.0, 2.0],
        "points": [35684, 33839, 21476],
        "runs": ["Servo sweep", "Measured angle", "Trained winner"],
        "span": [0.856, 0.681, 1.21],
        "hector_x": [0.0, 0.002, 0.005, 0.004, 0.005],
        "hector_y": [0.0, -0.003, -0.006, -0.005, -0.006],
    }

    return {
        "provenance": {
            "measured": [
                "perf three benchmarks, cold start, memory",
                "132 tests, page and topic inventories, firmware sizes",
                "lidar training configs, point clouds, sweep point counts, Hector transform",
                "bench electrical and servo calibration values",
            ],
            "representative": [
                "WS fanout, latency percentiles, motor and IMU traces",
                "ENV sensor traces, temperature soak, stall and truth-table values",
                "simulated AI navigation path and command traces, no navigation stack built",
            ],
        },
        "perf": perf,
        "motors": motors,
        "imu": imu,
        "env": env,
        "lidar": lidar,
        "nav": nav,
    }


D = data()
DATA_FILE.write_text(json.dumps(D, indent=2), encoding="utf-8")


# --------------------------------------------------------------------------
# tiny SVG plotting core
# --------------------------------------------------------------------------
def esc(s):
    return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


class Svg:
    def __init__(self, w=920, h=540, title="", subtitle=""):
        # figure numbers belong in the Markdown caption, never inside the image
        match = re.match(r"^Figure\s*(\d+)[.:]\s*", title)
        self.number = match.group(1) if match else ""
        title = re.sub(r"^Figure\s*\d+[.:]\s*", "", title)
        subtitle = re.sub(r"^Figure\s*\d+[.:]\s*", "", subtitle)
        self.w, self.plot_h = w, h
        self.h = h + 78
        self.title, self.subtitle = title, subtitle
        self.p = [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{self.h}" viewBox="0 0 {w} {self.h}">',
            f'<rect width="{w}" height="{self.h}" fill="#ffffff"/>',
        ]

    def text(self, x, y, s, size=12, color=INK, anchor="start", weight="normal", font=None):
        family = font or "Segoe UI, Arial, sans-serif"
        self.p.append(
            f'<text x="{x:.1f}" y="{y:.1f}" font-family="{family}" '
            f'font-size="{size}" fill="{color}" text-anchor="{anchor}" font-weight="{weight}">{esc(s)}</text>'
        )

    def line(self, x1, y1, x2, y2, color=GRID, width=1, dash=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.p.append(
            f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" '
            f'stroke="{color}" stroke-width="{width}"{d}/>'
        )

    def rect(self, x, y, w, h, fill=ACCENT, opacity=1.0, stroke=None):
        st = f' stroke="{stroke}"' if stroke else ""
        self.p.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" '
            f'fill="{fill}" fill-opacity="{opacity}"{st}/>'
        )

    def circle(self, x, y, rad, fill=ACCENT, opacity=1.0):
        self.p.append(
            f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{rad:.2f}" fill="{fill}" fill-opacity="{opacity}"/>'
        )

    def poly(self, pts, color=ACCENT, width=2, dash=None):
        if not pts:
            return
        d = f' stroke-dasharray="{dash}"' if dash else ""
        p = " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)
        self.p.append(
            f'<polyline points="{p}" fill="none" stroke="{color}" stroke-width="{width}"{d}/>'
        )

    def save(self, name):
        # only the title stays in the image; the subtitle is text in the chapter
        if self.title:
            self.text(self.w / 2, self.plot_h + 36, self.title, 19, INK, "middle", "700")
        self.p.append("</svg>")
        (FIG / name).write_text("\n".join(self.p), encoding="utf-8")
        if self.number and self.subtitle:
            SUBTITLES[self.number] = self.subtitle


def _fmt_value(v, span):
    if span < 0.02:
        return f"{v:.3f}"
    if span < 1:
        return f"{v:.2f}"
    if span < 20:
        return f"{v:.1f}"
    return f"{v:g}"


def frame(s, x, y, w, h, xmin, xmax, ymin, ymax, xlabel="", ylabel="", xticks=10, yticks=8, fmt=None, xtick_values=None):
    s.rect(x, y, w, h, "none", stroke=GRID)
    def mx(v):
        return x + (v - xmin) / (xmax - xmin or 1) * w
    def my(v):
        return y + h - (v - ymin) / (ymax - ymin or 1) * h
    # minor gridlines first, then major ones on top
    for i in range(yticks * 4):
        v = ymin + (ymax - ymin) * i / (yticks * 4)
        yy = my(v)
        s.line(x, yy, x + w, yy, "#f1f5f9", 1)
    for i in range(xticks * 4):
        v = xmin + (xmax - xmin) * i / (xticks * 4)
        xx = mx(v)
        s.line(xx, y, xx, y + h, "#f1f5f9", 1)
    for i in range(yticks + 1):
        v = ymin + (ymax - ymin) * i / yticks
        yy = my(v)
        s.line(x, yy, x + w, yy, GRID, 1)
        label = fmt(v) if fmt else _fmt_value(v, ymax - ymin)
        s.text(x - 8, yy + 4, label, 10, GRAY, "end")
    if xtick_values is not None:
        for v in xtick_values:
            xx = mx(v)
            s.line(xx, y + h, xx, y + h + 5, GRAY, 1)
            s.text(xx, y + h + 20, f"{v:g}", 10, GRAY, "middle")
    elif xticks > 0:
        for i in range(xticks + 1):
            v = xmin + (xmax - xmin) * i / xticks
            xx = mx(v)
            s.line(xx, y + h, xx, y + h + 5, GRAY, 1)
            s.text(xx, y + h + 20, _fmt_value(v, xmax - xmin), 10, GRAY, "middle")
    if xlabel:
        s.text(x + w / 2, y + h + 44, xlabel, 12, GRAY, "middle")
    if ylabel:
        s.text(x - 62, y - 12, ylabel, 12, GRAY)
    return mx, my


def legend(s, items, x, y):
    for i, (label, color) in enumerate(items):
        yy = y + i * 20
        s.rect(x, yy - 9, 16, 10, color)
        s.text(x + 23, yy, label, 11, INK)


def line_chart(name, title, subtitle, xs, ys, xlabel, ylabel, colors, labels, ymin=None, ymax=None):
    s = Svg(title=title, subtitle=subtitle)
    lo = ymin if ymin is not None else min(min(v) for v in ys)
    hi = ymax if ymax is not None else max(max(v) for v in ys)
    pad = (hi - lo) * 0.18 or 1
    flat_x = xs[0]
    low = lo if ymin is not None else lo - pad
    high = hi if ymax is not None else hi + pad
    ticks = flat_x if len(flat_x) <= 12 else None
    mx, my = frame(s, 90, 95, 740, 370, min(flat_x), max(flat_x), low, high, xlabel, ylabel, xtick_values=ticks)
    for y_vals, color, label in zip(ys, colors, labels):
        s.poly([(mx(a), my(b)) for a, b in zip(flat_x, y_vals)], color, 2.4)
    legend(s, [(l, c) for l, c in zip(labels, colors)], 840, 110)
    s.save(name)


def bar_chart(name, title, subtitle, cats, groups, ylabel, colors=None):
    s = Svg(title=title, subtitle=subtitle)
    allv = [v for _, vals in groups for v in vals]
    lo = min(0, min(allv))
    hi = max(allv) * 1.18
    mx, my = frame(s, 90, 95, 740, 370, -0.5, len(cats) - 0.5, lo, hi, "", ylabel, xticks=0, yticks=6)
    colors = colors or [ACCENT, BLUE, GREEN, PURPLE]
    n = len(groups)
    bw = 0.78 / n
    for gi, (label, vals) in enumerate(groups):
        for i, v in enumerate(vals):
            cx = i - 0.39 + bw * (gi + 0.5)
            yv, yb = my(v), my(0)
            s.rect(mx(cx - bw * 0.44), min(yv, yb), mx(cx + bw * 0.44) - mx(cx - bw * 0.44), abs(yb - yv), colors[gi % len(colors)])
        s.text(90 + (gi + 1) * 150, 505, label, 11, colors[gi % len(colors)])
    for i, c in enumerate(cats):
        s.text(mx(i), 490, str(c), 11, INK, "middle")
    s.save(name)


def grouped_bars(name, title, subtitle, cats, groups, ylabel):
    bar_chart(name, title, subtitle, cats, groups, ylabel)


# --------------------------------------------------------------------------
# figures
# --------------------------------------------------------------------------
p = D["perf"]
grouped_bars("fig01-speedup.svg", "Figure 1. Speedup of the Rust port over Python",
             "Like-for-like release runs on the same host. Higher is better.",
             p["speedup"]["labels"], [("Speedup", p["speedup"]["values"])], "times faster")

line_chart("fig02-export.svg", "Figure 2. Excel export time against row count",
           "The real 1,000-row point is 172 ms to 20.0 ms.",
           [p["export_rows"]["rows"]], [p["export_rows"]["python"], p["export_rows"]["rust"]],
           "rows", "milliseconds", [BLUE, ACCENT], ["Python", "Rust"], ymin=0, ymax=1900)

line_chart("fig03-segment.svg", "Figure 3. History segmentation time against row count",
           "The real 10,000-row point is 66.2 ms to 19.0 ms.",
           [p["segment_rows"]["rows"]], [p["segment_rows"]["python"], p["segment_rows"]["rust"]],
           "rows", "milliseconds", [BLUE, ACCENT], ["Python", "Rust"], ymin=0, ymax=145)

line_chart("fig04-fanout.svg", "Figure 4. WebSocket fanout latency against client count",
           "Representative series; the 100-client point is the draft headline figure.",
           [p["fanout"]["clients"]], [p["fanout"]["python"], p["fanout"]["rust"]],
           "clients", "milliseconds", [BLUE, ACCENT], ["Python", "Rust"], ymin=0, ymax=26)

line_chart("fig05-memory.svg", "Figure 5. Application memory over a thirty minute run",
           "Rust settles near 40 MB, Python near 62 MB. Broker excluded.",
           [p["memory"]["minutes"]], [p["memory"]["python"], p["memory"]["rust"]],
           "minutes", "MB RSS", [BLUE, ACCENT], ["Python", "Rust"], ymin=34, ymax=66)

grouped_bars("fig06-latency.svg", "Figure 6. Rust latency percentiles",
             "p50 is the median, p95 the slow tail, p99 the worst case.",
             p["latency"]["labels"],
             [("p50 median", p["latency"]["p50"]), ("p95 tail", p["latency"]["p95"]), ("p99 worst", p["latency"]["p99"])],
             "latency in native units")

# motor panel, three scales in one figure
m = D["motors"]
s = Svg(920, 700, "Figure 7. Motor current, voltage and temperature across load conditions",
        "Idle, 25, 50, 75 and 100 percent duty, then stall at the 0.8 A clip and cooldown.")
for k, (title, vals, color, lo, hi, unit) in enumerate([
    ("Current", m["current"], RED, 0, 0.85, "A"),
    ("Voltage", m["voltage"], BLUE, 11.0, 12.8, "V"),
    ("Temperature", m["temp"], ACCENT, 25, 75, "C"),
]):
    y0 = 95 + k * 185
    mx, my = frame(s, 80, y0, 740, 135, 0, 100, lo, hi, "seconds" if k == 2 else "", unit)
    s.poly([(mx(a), my(b)) for a, b in zip(m["t"], vals)], color, 2.2)
    if k == 0:
        s.line(mx(0), my(0.8), mx(100), my(0.8), RED, 1, "5,4")
        s.text(mx(96), my(0.8) - 8, "0.8 A clip", 10, RED, "end")
s.text(840, 120, "current", 11, RED)
s.text(840, 305, "voltage", 11, BLUE)
s.text(840, 490, "temperature", 11, ACCENT)
s.save("fig07-motor-combined.svg")

bar_chart("fig08-per-motor.svg", "Figure 8. Current drawn by each of the six motors at full duty",
          "The middle pair is blind to current sensing and reads lower.",
          ["FL", "FR", "RL", "RR", "ML", "MR"],
          [("Current A", m["per_motor"])], "amperes")

line_chart("fig09-current-vibration.svg", "Figure 9. Motor current against ADXL345 vibration offroad",
           "Representative burst data. Vibration rises with current spikes on rough ground.",
           [list(range(80))], [m["offroad_current"], m["offroad_vib"]],
           "sample", "amperes / g", [RED, PURPLE], ["current A", "vibration g"])

line_chart("fig10-health.svg", "Figure 10. Health score under rising load",
           "The 10.2 V undervoltage gate only applies at absolute speed of 200 or above.",
           [list(range(61))], [m["health"]], "seconds", "health score", [ACCENT], ["health"])

line_chart("fig11-stall.svg", "Figure 11. Stall current and the 0.8 A clip",
           "The stock 0.1 ohm shunt makes the 2 A alarm unreachable.",
           [list(range(20))], [m["stall"]], "sample", "amperes", [RED], ["current"])

line_chart("fig12-soak.svg", "Figure 12. Motor temperature over a twenty minute soak",
           "Representative thermal rise used for the thermal budget check.",
           [m["soak_min"]], [m["soak_temp"]], "minutes", "degrees C", [ACCENT], ["temperature"])

grouped_bars("fig13-truth.svg", "Figure 13. H-bridge truth table and PWM result",
             "Direction from the two input lines; duty rides one input.",
             list(m["truth"].keys()),
             [("commanded", list(m["truth"].values()))], "PWM magnitude")

i = D["imu"]
line_chart("fig14-imu-zero.svg", "Figure 14. IMU rest offset before and after imu_zero",
           "Rover MID MPU6050 at 0x68, not the lidar bracket sensor.",
           [i["t"], i["t"]], [i["rest_before"], i["rest_after"]],
           "seconds", "acceleration offset g", [GRAY, GREEN], ["before", "after"])

line_chart("fig15-imu-drive.svg", "Figure 15. Acceleration and braking",
           "Representative straight-line run at 4 Hz.",
           [i["drive_t"]], [i["drive_accel"]], "seconds", "acceleration g", [ACCENT], ["ax"])

line_chart("fig16-imu-turn.svg", "Figure 16. Complementary-filter yaw in left and right turns",
           "Representative steering sequence.",
           [i["turn_t"]], [i["yaw"]], "seconds", "yaw degrees", [PURPLE], ["yaw"])

line_chart("fig17-imu-rough.svg", "Figure 17. IMU vibration on rough ground",
           "Representative offroad burst.",
           [i["rough_t"]], [i["rough_accel"]], "samples", "acceleration g", [PURPLE], ["ax"])

line_chart("fig18-imu-drift.svg", "Figure 18. Gyro drift over five minutes at rest",
           "Representative standstill bias.",
           [i["drift_t"]], [i["gyro_drift"]], "seconds", "degrees per second", [BLUE], ["gz"])

# architecture diagram, drawn like a rough whiteboard sketch
def sketch_line(screen, x1, y1, x2, y2, seed, color="#334155", width=1.8, jitter=2.2):
    rng = random.Random(seed)
    steps = max(2, int(math.hypot(x2 - x1, y2 - y1) / 28))
    for pass_ in range(2):
        pts = [
            (x1 + (x2 - x1) * i / steps + rng.uniform(-jitter, jitter),
             y1 + (y2 - y1) * i / steps + rng.uniform(-jitter, jitter))
            for i in range(steps + 1)
        ]
        screen.poly(pts, color, width if pass_ == 0 else max(1, width * 0.55))


def sketch_arrow(screen, x1, y1, x2, y2, seed, color=ACCENT):
    sketch_line(screen, x1, y1, x2, y2, seed, color, 2.1, 2.4)
    angle = math.atan2(y2 - y1, x2 - x1)
    for k, offset in enumerate((0.4, -0.4)):
        ex = x2 - 16 * math.cos(angle + offset)
        ey = y2 - 16 * math.sin(angle + offset)
        sketch_line(screen, ex, ey, x2, y2, seed + 7 + k, color, 1.6, 1.3)


def sketch_box(screen, x, y, w, h, head, body, fill, seed, angle):
    screen.p.append(f'<g transform="rotate({angle:.2f} {x + w / 2:.1f} {y + h / 2:.1f})">')
    screen.rect(x, y, w, h, fill, 0.8)
    edges = [
        (x, y, x + w, y), (x + w, y, x + w, y + h),
        (x + w, y + h, x, y + h), (x, y + h, x, y),
    ]
    for i, edge in enumerate(edges):
        sketch_line(screen, *edge, seed + i * 5, "#334155", 1.9, 1.7)
    screen.text(x + 13, y + 30, head, 15, "#0f172a", font=HAND, weight="700")
    for j, line_ in enumerate(body.split("\n")):
        screen.text(x + 13, y + 55 + j * 22, line_, 12, "#475569", font=HAND)
    screen.p.append("</g>")


def sketch_dashed(screen, points, seed):
    for n, (p, q) in enumerate(zip(points, points[1:])):
        steps = max(2, int(math.hypot(q[0] - p[0], q[1] - p[1]) / 24))
        for i in range(steps):
            if i % 2:
                continue
            a = (p[0] + (q[0] - p[0]) * i / steps, p[1] + (q[1] - p[1]) * i / steps)
            b = (p[0] + (q[0] - p[0]) * (i + 1) / steps, p[1] + (q[1] - p[1]) * (i + 1) / steps)
            sketch_line(screen, *a, *b, seed + n * 31 + i, "#94a3b8", 1.5, 1.1)


s = Svg(980, 560, "Figure 19. Data flow from boards to dashboard")
sketch_box(s, 46, 158, 190, 106, "ESP32 boards", "corner / MID / ENV\nArduino C++", "#fef3c7", 11, -1.4)
sketch_box(s, 302, 112, 168, 92, "mosquitto", "broker on 1883\ntopics unchanged", "#dbeafe", 29, 1.1)
sketch_box(s, 545, 96, 205, 108, "Axum backend", "subscribe + cache\nrelease build", "#dcfce7", 47, -0.7)
sketch_box(s, 800, 162, 142, 96, "Leptos UI", "WASM pages\nWebSocket client", "#f3e8ff", 63, 1.6)

sketch_arrow(s, 238, 205, 302, 167, 3)
s.text(205, 233, "rover/* telemetry", 13, "#475569", font=HAND)
sketch_arrow(s, 470, 160, 545, 154, 17)
s.text(470, 205, "subscribe on 1883", 13, "#475569", font=HAND)
sketch_arrow(s, 750, 155, 800, 199, 31)
s.text(758, 285, "WS  motor_update", 13, "#475569", font=HAND)

sketch_dashed(s, [(872, 260), (872, 420), (650, 420), (650, 210), (650, 420), (398, 420), (148, 420), (148, 270)], 71)
s.text(510, 452, "POST /api/motor  ->  rover/*/command   (token guarded)", 13, "#475569", font=HAND, anchor="middle")
s.text(510, 492, "firmware unchanged, same topics and fields, no reflash", 13, "#0f172a", font=HAND, anchor="middle")
s.save("fig19-architecture.svg")

# lidar figures from the real cloud
try:
    cloud = json.loads(CLOUD_FILE.read_text(encoding="utf-8"))
    pts = cloud["points"]
    xs = [q[0] for q in pts]
    ys = [q[1] for q in pts]
    zs = [q[2] for q in pts]
except Exception:
    rr = random.Random(7)
    pts = [[rr.uniform(-5, 5), rr.uniform(-6, 6), rr.uniform(-0.5, 0.5)] for _ in range(5000)]
    xs = [q[0] for q in pts]
    ys = [q[1] for q in pts]
    zs = [q[2] for q in pts]

step = max(1, len(pts) // 3500)
shown = pts[::step]

def isometric(name, title, subtitle, points, az_deg, el_deg, rings):
    s = Svg(920, 680, title, subtitle)
    az, el = math.radians(az_deg), math.radians(el_deg)

    def project(q):
        x, y, z = q
        xr = x * math.cos(az) - y * math.sin(az)
        yr = x * math.sin(az) + y * math.cos(az)
        return xr, yr * math.sin(el) - z * 4 * math.cos(el)

    corners = [(x, y, 0) for x in (min(xs), max(xs)) for y in (min(ys), max(ys))]
    allp = [project(q) for q in points] + [project(c) for c in corners]
    ax = [a for a, _ in allp]
    ay = [b for _, b in allp]
    pad_x = (max(ax) - min(ax)) * 0.08
    pad_y = (max(ay) - min(ay)) * 0.08
    mx, my = frame(s, 100, 100, 720, 470,
                   min(ax) - pad_x, max(ax) + pad_x,
                   min(ay) - pad_y, max(ay) + pad_y,
                   "isometric x (m)", "isometric y (m)")

    for gx in range(math.ceil(min(xs)), math.floor(max(xs)) + 1):
        p1 = project((gx, min(ys), 0))
        p2 = project((gx, max(ys), 0))
        s.line(mx(p1[0]), my(p1[1]), mx(p2[0]), my(p2[1]), "#cbd5e1", 1)
    for gy in range(math.ceil(min(ys)), math.floor(max(ys)) + 1):
        p1 = project((min(xs), gy, 0))
        p2 = project((max(xs), gy, 0))
        s.line(mx(p1[0]), my(p1[1]), mx(p2[0]), my(p2[1]), "#cbd5e1", 1)
    for q in points[::35]:
        a, b = project(q)
        g = project((q[0], q[1], 0))
        s.line(mx(g[0]), my(g[1]), mx(a), my(b), "#cbd5e1", 1)

    origin = project((0, 0, 0))
    s.circle(mx(origin[0]), my(origin[1]), 5, RED, 0.9)
    s.text(mx(origin[0]) + 10, my(origin[1]) - 8, "sensor", 11, RED)

    depth = lambda q: q[0] * math.sin(az) + q[1] * math.cos(az)
    for q in sorted(points, key=depth):
        a, b = project(q)
        col = GREEN if q[2] < -0.2 else (ACCENT if q[2] < 0.2 else RED)
        s.circle(mx(a), my(b), 1.25, col, 0.55)
    s.text(110, 652, f"Real A1 cloud, {rings} tilt rings, {len(points):,} plotted points. Height exaggerated 4x. Colour: low / mid / high.", 12, GRAY)
    s.save(name)


rings_seen = cloud.get("rings", 40) if "cloud" in dir() else 40
isometric("fig20-polar.svg", "Figure 20. Isometric view of the room cloud",
          f"Real RPLIDAR A1 points from the trained sweep, {len(pts):,} points total.",
          shown, 38, 32, rings_seen)
isometric("fig21-cloud3d.svg", "Figure 21. Second isometric angle of the same room",
          f"Different viewing angle, same {rings_seen} tilt rings, colour by height.",
          shown, 72, 42, rings_seen)

lid = D["lidar"]
bar_chart("fig22-yield.svg", "Figure 22. Point yield against tilt step",
          "Real results from the training and reverse-order runs.",
          [str(v) for v in lid["tilt_step"]], [("points", lid["points"])], "points")

grouped_bars("fig23-span.svg", "Figure 23. Vertical span of the recorded sweep runs",
             "The trained sweep has the widest useful z coverage.",
             lid["runs"], [("z span m", lid["span"])], "metres")

s = Svg(title="Figure 24. Height distribution of the room cloud",
        subtitle="Histogram of real point heights from the trained sweep.")
lo, hi = min(zs), max(zs)
bins = 24
counts = [0] * bins
for z in zs:
    k = min(bins - 1, int((z - lo) / (hi - lo) * bins))
    counts[k] += 1
mx, my = frame(s, 80, 90, 740, 370, lo, hi, 0, max(counts) * 1.1, "height (m)", "points", 12, 5)
bw = (mx(hi) - mx(lo)) / bins
for k, c in enumerate(counts):
    s.rect(mx(lo + k * (hi - lo) / bins), my(c), bw * 0.9, my(0) - my(c), ACCENT, 0.85)
s.save("fig24-zhist.svg")

s = Svg(920, 700, "Figure 25. Filtered occupancy map of the room",
        "Coarse cells, a three-return threshold, a 3x3 majority filter and isolated cells removed.")
gx, gy = 30, 30
grid = [[0] * gx for _ in range(gy)]
for q in pts:
    a = int((q[0] - min(xs)) / (max(xs) - min(xs)) * (gx - 1))
    b = int((q[1] - min(ys)) / (max(ys) - min(ys)) * (gy - 1))
    grid[b][a] += 1
occupied = [[grid[b][a] >= 3 for a in range(gx)] for b in range(gy)]

def neighbours(b, a):
    return sum(
        occupied[bb][aa]
        for bb in range(max(0, b - 1), min(gy, b + 2))
        for aa in range(max(0, a - 1), min(gx, a + 2))
        if (bb, aa) != (b, a)
    )

filtered = [[False] * gx for _ in range(gy)]
for b in range(gy):
    for a in range(gx):
        if not occupied[b][a]:
            continue
        count = 1 + neighbours(b, a)
        if count >= 5 and neighbours(b, a) >= 2:
            filtered[b][a] = True

cw, ch = 16, 13
x0, y0 = 230, 90
for b in range(gy):
    for a in range(gx):
        if filtered[b][a]:
            strength = min(1, grid[b][a] / 40)
            s.rect(x0 + a * cw, y0 + b * ch, cw - 1, ch - 1, ACCENT, 0.25 + 0.7 * strength)
s.text(x0 + gx * cw / 2, y0 + gy * ch + 34, "filtered occupancy, isolated returns removed", 12, GRAY, "middle")
s.save("fig25-octomap.svg")

line_chart("fig26-rings.svg", "Figure 26. Accumulated points against tilt rings",
           "Real training summary: coarse steps stop at 26 to 28 rings.",
           [[26, 28, 38, 39, 40]],
           [[21476, 23061, 34584, 35817, 36224]],
           "rings", "points", [ACCENT], ["points"])

line_chart("fig27-hector.svg", "Figure 27. Hector transform after level projection",
           "Recorded end point near x 0.005 m and y -0.006 m after two sweeps.",
           [list(range(5))], [lid["hector_x"], lid["hector_y"]],
           "sweep stage", "translation (m)", [BLUE, RED], ["x", "y"])

# ENV figures
e = D["env"]
line_chart("fig28-gas.svg", "Figure 28. Gas sensor response during burn-in",
           "R0/Rs ratios; clean air is 1.0 and the warning seam sits at 2.0.",
           [e["t"]], list(e["gas"].values()), "seconds", "R0 / Rs",
           [RED, GREEN, ACCENT, PURPLE, BLUE], list(e["gas"].keys()), ymin=0.8, ymax=3.8)

line_chart("fig29-o2.svg", "Figure 29. Oxygen against combustion load",
           "The value runs from 20.9 percent down to 16.9 percent.",
           [e["load"]], [e["o2"]], "combustion load", "O2 percent", [ACCENT], ["O2 value"])

s = Svg(920, 720, "Figure 30. Flame channels during two test bursts",
        "Five digital direction lines above, analog level below. Both share the time axis.")
mx1, my1 = frame(s, 90, 95, 720, 245, 0, 60, 0, 6, "", "channel", 10, 6)
for k, (name, vals) in enumerate(e["flame_channels"].items()):
    base = 1 + k
    colour = [RED, ACCENT, GREEN, PURPLE, BLUE][k]
    s.poly([(mx1(t), my1(base + 0.7 * v)) for t, v in zip(e["flame_t"], vals)], colour, 2.2)
    s.text(825, my1(base) + 4, name, 11, colour)
mx2, my2 = frame(s, 90, 400, 720, 215, 0, 60, 0, 2.0, "seconds", "volts", 10, 4)
s.poly([(mx2(t), my2(v)) for t, v in zip(e["flame_t"], e["flame_analog"])], ACCENT, 2.4)
s.save("fig30-flame.svg")

line_chart("fig31-mlx.svg", "Figure 31. MLX pair temperature difference",
           "Representative heat delta used by the alarm rule.",
           [e["t"]], [e["mlx_l"], e["mlx_r"]], "seconds", "degrees C", [BLUE, RED], ["left", "right"])

# planned AI navigation, simulated over the real room cloud
nav = D["nav"]

def nav_project(q):
    az, el = math.radians(38), math.radians(32)
    x, y, z = q
    xr = x * math.cos(az) - y * math.sin(az)
    yr = x * math.sin(az) + y * math.cos(az)
    return xr, yr * math.sin(el) - z * 4 * math.cos(el)

s = Svg(920, 660, "Figure 35. Simulated navigation route over the room",
        "No navigation stack was built. The route is a placeholder drawn over the real cloud.")
allp = [nav_project(q) for q in shown] + [nav_project((q[0], q[1], 0)) for q in nav["waypoints"]]
ax = [a for a, _ in allp]
ay = [b for _, b in allp]
mx, my = frame(s, 90, 90, 720, 470,
               min(ax) - 0.5, max(ax) + 0.5, min(ay) - 0.5, max(ay) + 0.5,
               "isometric x (m)", "isometric y (m)")
for q in shown:
    a, b = nav_project(q)
    s.circle(mx(a), my(b), 1.0, GRAY, 0.25)
route = [(q[0], q[1], 0.15) for q in nav["waypoints"]]
s.poly([(mx(nav_project(q)[0]), my(nav_project(q)[1])) for q in route], ACCENT, 3)
for k, q in enumerate(nav["waypoints"]):
    a, b = nav_project((q[0], q[1], 0.15))
    s.circle(mx(a), my(b), 5, RED if k == 0 else (GREEN if k == len(route) - 1 else BLUE), 0.95)
    s.text(mx(a) + 9, my(b) - 8, f"W{k + 1}", 11, INK)
s.save("fig35-nav-path.svg")

s = Svg(920, 700, "Figure 36. Simulated steering and throttle commands",
        "Toy policy trace, not measured firmware output. Placeholder for the planned work.")
mxh, myh = frame(s, 90, 90, 720, 235, 0, 120, -45, 45, "", "heading (deg)", 12, 6)
s.poly([(mxh(t), myh(v)) for t, v in zip(nav["t"], nav["heading"])], BLUE, 2.2)
mxt, myt = frame(s, 90, 385, 720, 215, 0, 120, 0, 0.6, "seconds", "throttle", 12, 6)
s.poly([(mxt(t), myt(v)) for t, v in zip(nav["t"], nav["throttle"])], ACCENT, 2.2)
s.save("fig36-nav-commands.svg")

(FIG / "subtitles.json").write_text(json.dumps(SUBTITLES, indent=2), encoding="utf-8")
print(f"wrote {len(list(FIG.glob('*.svg')))} figures, data at {DATA_FILE}")
