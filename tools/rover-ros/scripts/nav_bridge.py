#!/usr/bin/env python3
"""Publish the rover pose and a rover-height 2D grid over MQTT.

Pose comes from TF ``map -> laser`` (hector). The grid comes from the second
octomap_server in the ``/nav`` namespace, sliced at -0.18..1.20 m in the lidar
frame: the level ring is at 0, the floor around -0.25, so the lower bound
drops the floor but keeps the ring. The display octomap is untouched.

Wire shapes:
  rover/pose      {"ok":1,"x":..,"y":..,"yaw":..,"stamp":..}
  rover/map/grid  {"frame":"map","resolution":..,"width":..,"height":..,
                   "origin":[x,y],"rle":[[count,value],...]}

Grid values: 0 unknown, 1 free, 2 occupied. Row-major, x fastest, origin is
the lower-left corner of cell (0,0). Occupied cells are inflated by 0.25 m
before the grid is published, so the global planner must not inflate again.
"""

import json
import math
import time

UNKNOWN, FREE, OCCUPIED = 0, 1, 2

MAP_TOPIC = "/nav/projected_map"
POSE_TOPIC = "rover/pose"
GRID_TOPIC = "rover/map/grid"

TARGET_RES = 0.10
INFLATE_M = 0.25
POSE_HZ = 10.0
GRID_HZ = 1.0


def _cell(raw):
    if raw < 0:
        return UNKNOWN
    return OCCUPIED if raw >= 50 else FREE


def build_grid(
    width,
    height,
    resolution,
    origin_x,
    origin_y,
    data,
    target_res=TARGET_RES,
    inflate_m=INFLATE_M,
    frame="map",
):
    """Turn a nav_msgs/OccupancyGrid into the autopilot's RLE payload.

    The source is downsampled to roughly ``target_res``. A cell is occupied
    if any source cell in its block is occupied, free only if the whole block
    is free, unknown otherwise. The result is cropped to the known area with
    a one-cell margin and inflated by ``inflate_m``.
    """
    if width <= 0 or height <= 0 or not data:
        return None
    if resolution <= 0.0:
        return None

    scale = max(1, int(round(target_res / resolution)))
    res = resolution * scale
    w, h = width // scale, height // scale
    if w <= 0 or h <= 0:
        return None

    small = [UNKNOWN] * (w * h)
    for y in range(h):
        for x in range(w):
            occupied = False
            free = True
            for dy in range(scale):
                row = (y * scale + dy) * width
                for dx in range(scale):
                    v = _cell(data[row + x * scale + dx])
                    if v == OCCUPIED:
                        occupied = True
                    elif v != FREE:
                        free = False
            if occupied:
                small[y * w + x] = OCCUPIED
            elif free:
                small[y * w + x] = FREE

    # Crop to the known area plus one cell of margin.
    min_x, min_y, max_x, max_y = w, h, -1, -1
    for y in range(h):
        row = y * w
        for x in range(w):
            if small[row + x] != UNKNOWN:
                if x < min_x:
                    min_x = x
                if x > max_x:
                    max_x = x
                if y < min_y:
                    min_y = y
                if y > max_y:
                    max_y = y
    if max_x < min_x or max_y < min_y:
        return None
    min_x = max(0, min_x - 1)
    min_y = max(0, min_y - 1)
    max_x = min(w - 1, max_x + 1)
    max_y = min(h - 1, max_y + 1)

    cw, ch = max_x - min_x + 1, max_y - min_y + 1
    cropped = [UNKNOWN] * (cw * ch)
    for y in range(ch):
        src_row = (min_y + y) * w + min_x
        dst_row = y * cw
        for x in range(cw):
            cropped[dst_row + x] = small[src_row + x]

    # Inflate occupied cells. Conservative on purpose.
    r_cells = int(math.ceil(inflate_m / res)) if inflate_m > 0.0 else 0
    if r_cells > 0:
        offsets = [
            (dx, dy)
            for dy in range(-r_cells, r_cells + 1)
            for dx in range(-r_cells, r_cells + 1)
            if dx * dx + dy * dy <= r_cells * r_cells
        ]
        occupied = [
            (x, y)
            for y in range(ch)
            for x in range(cw)
            if cropped[y * cw + x] == OCCUPIED
        ]
        for ox, oy in occupied:
            for dx, dy in offsets:
                nx, ny = ox + dx, oy + dy
                if 0 <= nx < cw and 0 <= ny < ch:
                    cropped[ny * cw + nx] = OCCUPIED

    # Run-length encode, row-major.
    rle = []
    run_val = cropped[0]
    run_len = 1
    for v in cropped[1:]:
        if v == run_val:
            run_len += 1
        else:
            rle.append([run_len, run_val])
            run_val = v
            run_len = 1
    rle.append([run_len, run_val])

    return {
        "frame": frame or "map",
        "resolution": round(res, 4),
        "width": cw,
        "height": ch,
        "origin": [
            round(origin_x + min_x * res, 3),
            round(origin_y + min_y * res, 3),
        ],
        "rle": rle,
        "cells": cw * ch,
        "occupied": sum(1 for v in cropped if v == OCCUPIED),
        "stamp": round(time.time(), 2),
    }


class Bridge:
    def __init__(self):
        import paho.mqtt.client as mqtt
        import rospy
        import tf2_ros
        from nav_msgs.msg import OccupancyGrid

        self._rospy = rospy
        self._tf2 = tf2_ros
        self.client = mqtt.Client(
            mqtt.CallbackAPIVersion.VERSION2, client_id="rover-nav-bridge"
        )
        self.client.connect("127.0.0.1", 1883, 30)
        self.client.loop_start()

        self.buffer = tf2_ros.Buffer()
        self.listener = tf2_ros.TransformListener(self.buffer)
        self.grid = None
        self.last_build = 0.0

        rospy.Subscriber(MAP_TOPIC, OccupancyGrid, self.on_map, queue_size=1)
        rospy.Timer(rospy.Duration(1.0 / POSE_HZ), self.publish_pose)
        rospy.Timer(rospy.Duration(1.0 / GRID_HZ), self.publish_grid)
        rospy.loginfo(
            "nav_bridge: map->laser -> %s, %s -> %s",
            POSE_TOPIC,
            MAP_TOPIC,
            GRID_TOPIC,
        )

    def publish_pose(self, _event):
        try:
            tf = self.buffer.lookup_transform(
                "map", "laser", self._rospy.Time(0), self._rospy.Duration(0.05)
            )
        except (
            self._tf2.LookupException,
            self._tf2.ConnectivityException,
            self._tf2.ExtrapolationException,
        ) as e:
            self._rospy.logwarn_throttle(2.0, "nav_bridge: no map->laser: %s", e)
            return

        t = tf.transform.translation
        q = tf.transform.rotation
        siny = 2.0 * (q.w * q.z + q.x * q.y)
        cosy = 1.0 - 2.0 * (q.y * q.y + q.z * q.z)
        yaw = math.degrees(math.atan2(siny, cosy))
        self.client.publish(
            POSE_TOPIC,
            json.dumps(
                {
                    "ok": 1,
                    "x": round(t.x, 4),
                    "y": round(t.y, 4),
                    "yaw": round(yaw, 2),
                    "stamp": round(time.time(), 2),
                }
            ),
        )

    def on_map(self, msg):
        if msg.info.width == 0 or msg.info.height == 0 or not msg.data:
            return
        # The projected map can arrive at the cloud rate. Building the RLE
        # grid is CPU work, so do it once per second and republish the cache.
        now = time.monotonic()
        if now - self.last_build < 1.0:
            return
        self.last_build = now
        grid = build_grid(
            msg.info.width,
            msg.info.height,
            msg.info.resolution,
            msg.info.origin.position.x,
            msg.info.origin.position.y,
            msg.data,
            TARGET_RES,
            INFLATE_M,
            msg.header.frame_id,
        )
        if grid is not None:
            self.grid = grid
            self._rospy.loginfo_throttle(
                10.0,
                "nav_bridge: grid %dx%d %.2f m, %d occupied",
                grid["width"],
                grid["height"],
                grid["resolution"],
                grid["occupied"],
            )

    def publish_grid(self, _event):
        if self.grid is not None:
            self.client.publish(GRID_TOPIC, json.dumps(self.grid))


def main():
    import rospy

    rospy.init_node("nav_bridge")
    Bridge()
    rospy.spin()


if __name__ == "__main__":
    main()
