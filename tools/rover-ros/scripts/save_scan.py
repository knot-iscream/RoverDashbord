#!/usr/bin/env python3
"""Save one scan session: OctoMap .bt, display JSON, PLY and metadata.

Runs inside the rover-ros container. The sweep loop invokes it on the host
with `docker exec` when a parked session ends. `--checkpoint` only writes a
safety .bt every few sweeps and does not create a history entry.
"""
import argparse
import json
import os
import subprocess

import rospy
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2

from scan_common import bucket_points, furniture_count, write_ply

DISPLAY_TOPIC = "/octomap_point_cloud_centers"
WAIT_S = 6.0


def save_octomap(path):
    """Call the stock OctoMap saver. A missing map is not fatal."""
    env = os.environ.copy()
    env.setdefault("DISABLE_ROS1_EOL_WARNINGS", "1")
    try:
        out = subprocess.run(
            ["rosrun", "octomap_server", "octomap_saver", path],
            capture_output=True,
            text=True,
            timeout=40,
            env=env,
        )
        if out.returncode != 0:
            print("octomap_saver failed: %s" % (out.stderr or out.stdout).strip())
            return False
        return os.path.exists(path)
    except (OSError, subprocess.SubprocessError) as exc:
        print("octomap_saver error: %s" % exc)
        return False


def read_cloud():
    """Return (points, raw_count) from the latest OctoMap surface message."""
    try:
        msg = rospy.wait_for_message(DISPLAY_TOPIC, PointCloud2, timeout=WAIT_S)
    except rospy.ROSException:
        return [], 0
    raw = [
        (p[0], p[1], p[2])
        for p in point_cloud2.read_points(
            msg, field_names=("x", "y", "z"), skip_nans=True
        )
    ]
    return raw, len(raw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--id", required=True)
    ap.add_argument("--start", default="")
    ap.add_argument("--end", default="")
    ap.add_argument("--sweeps", type=int, default=0)
    ap.add_argument("--partial", action="store_true")
    ap.add_argument("--checkpoint", action="store_true")
    args = ap.parse_args()

    os.makedirs(args.dir, exist_ok=True)
    rospy.init_node("scan_saver", anonymous=True)

    if args.checkpoint:
        ok = save_octomap(os.path.join(args.dir, "checkpoint.bt"))
        print("checkpoint %s" % ("saved" if ok else "skipped"))
        return

    raw, raw_count = read_cloud()
    display = bucket_points(raw)
    display_path = os.path.join(args.dir, "display.json")
    with open(display_path, "w", encoding="utf-8") as f:
        json.dump({"count": len(display), "rings": 0, "points": display}, f)

    ply_path = os.path.join(args.dir, "cloud.ply")
    write_ply(ply_path, display)

    map_path = os.path.join(args.dir, "map.bt")
    map_ok = save_octomap(map_path)

    def size(path):
        try:
            return os.path.getsize(path)
        except OSError:
            return 0

    meta = {
        "id": args.id,
        "start": args.start,
        "end": args.end,
        "sweeps": args.sweeps,
        "partial": bool(args.partial),
        "label": "",
        "source": "auto",
        "points": len(display),
        "voxels": raw_count,
        "furniture_points": furniture_count(raw),
        "map_ok": map_ok,
        "files": {
            "map_bt": size(map_path),
            "cloud_ply": size(ply_path),
            "display_json": size(display_path),
        },
    }
    with open(os.path.join(args.dir, "meta.json"), "w", encoding="utf-8") as f:
        json.dump(meta, f, indent=2)
    print(json.dumps(meta))


if __name__ == "__main__":
    main()
