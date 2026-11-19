#!/usr/bin/env python3
"""Accumulate /lidar_points and publish a 3D cloud to rover/lidar/cloud.

The Rust home page can render this as a real 3D cloud. OctoMap still builds
its own occupancy map in parallel; this is the feed for the dashboard.
"""
import json
import time

import rospy
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2
import paho.mqtt.client as mqtt

MAX_POINTS = 2000
PUBLISH_EVERY_S = 1.0


def main():
    rospy.init_node("cloud_to_mqtt")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-cloud-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()
    acc = []
    last_publish = 0.0

    def on_cloud(msg):
        nonlocal last_publish
        for p in point_cloud2.read_points(msg, field_names=("x", "y", "z"), skip_nans=True):
            acc.append([round(p[0], 3), round(p[1], 3), round(p[2], 3)])
        if len(acc) > MAX_POINTS:
            del acc[: len(acc) - MAX_POINTS]
        now = time.monotonic()
        if now - last_publish < PUBLISH_EVERY_S:
            return
        last_publish = now
        # Thin evenly across the buffer so a snapshot covers the whole scan.
        step = max(1, len(acc) // MAX_POINTS)
        out = acc[::step][:MAX_POINTS]
        client.publish(
            "rover/lidar/cloud",
            json.dumps({"count": len(out), "points": out}),
        )

    rospy.Subscriber("/lidar_points", PointCloud2, on_cloud, queue_size=1)
    rospy.spin()


if __name__ == "__main__":
    main()
