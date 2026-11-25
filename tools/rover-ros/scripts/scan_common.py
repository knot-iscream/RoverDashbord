#!/usr/bin/env python3
"""Shared point-cloud helpers for the scan display and the scan archive.

Pure functions only, no ROS or MQTT imports, so both the live display bridge
and the save script can use the same bucketing and PLY writer.
"""
import math

BUCKET_M = 0.1
MAX_POINTS = 8000


def bucket_points(points):
    """Average points into 10 cm cells and cap the result.

    `points` is an iterable of (x, y, z) in metres. The OctoMap grid is 5 cm,
    which is too dense to paint and too dense to keep per scan. One averaged
    point per 10 cm cell keeps furniture edges without the noise.
    """
    buckets = {}
    for x, y, z in points:
        key = (round(x / BUCKET_M), round(y / BUCKET_M), round(z / BUCKET_M))
        acc = buckets.get(key)
        if acc is None:
            buckets[key] = [x, y, z, 1]
        else:
            acc[0] += x
            acc[1] += y
            acc[2] += z
            acc[3] += 1
    out = [
        [round(a[0] / a[3], 3), round(a[1] / a[3], 3), round(a[2] / a[3], 3)]
        for a in buckets.values()
    ]
    if len(out) > MAX_POINTS:
        step = len(out) // MAX_POINTS + 1
        out = out[::step][:MAX_POINTS]
    return out


def write_ply(path, points):
    """Write a binary-free ASCII PLY that MeshLab and CloudCompare open."""
    with open(path, "w", encoding="ascii") as f:
        f.write("ply\nformat ascii 1.0\n")
        f.write("comment RoverDash scan, metres\n")
        f.write("element vertex %d\n" % len(points))
        f.write("property float x\nproperty float y\nproperty float z\n")
        f.write("end_header\n")
        for x, y, z in points:
            f.write("%.3f %.3f %.3f\n" % (x, y, z))


def furniture_count(points):
    """Points in the furniture band, within 2 m, for the history summary."""
    n = 0
    for x, y, z in points:
        if 0.15 <= z <= 0.9 and math.hypot(x, y) <= 2.0:
            n += 1
    return n
