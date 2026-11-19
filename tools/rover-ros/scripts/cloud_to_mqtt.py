#!/usr/bin/env python3
"""Accumulate /lidar_points into one 3D cloud per hand tilt, then publish it.

Each tilt value gets one ring. Changing the tilt keeps the old ring, so a sweep
of -40..+40 builds a real 3D room cloud instead of dropping every ring after
the latest scan. The point cloud is cleared over MQTT when the UI asks.
"""
import json
import time

import rospy
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2
from std_msgs.msg import Float32
import paho.mqtt.client as mqtt

MAX_RINGS = 20
MAX_TOTAL_POINTS = 24000
PUBLISH_EVERY_S = 1.0


def main():
    rospy.init_node("cloud_to_mqtt")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-cloud-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()
    rings = {}
    current_tilt = 0.0
    last_publish = 0.0

    def on_tilt(msg):
        nonlocal current_tilt
        current_tilt = msg.data

    def on_mqtt(_client, _userdata, msg):
        if msg.topic == "rover/lidar/cloud/clear":
            rings.clear()
            print("cloud cleared", flush=True)

    client.on_message = on_mqtt
    client.subscribe("rover/lidar/cloud/clear")

    def on_cloud(msg):
        nonlocal last_publish
        pts = [
            [round(p[0], 3), round(p[1], 3), round(p[2], 3)]
            for p in point_cloud2.read_points(msg, field_names=("x", "y", "z"), skip_nans=True)
        ]
        if not pts:
            return
        key = round(current_tilt * 2.0) / 2.0
        rings[key] = pts
        while len(rings) > MAX_RINGS:
            drop = min(rings, key=lambda k: abs(k))
            del rings[drop]
        now = time.monotonic()
        if now - last_publish < PUBLISH_EVERY_S:
            return
        last_publish = now
        out = []
        for key in sorted(rings):
            out.extend(rings[key])
        if len(out) > MAX_TOTAL_POINTS:
            step = max(1, len(out) // MAX_TOTAL_POINTS)
            out = out[::step][:MAX_TOTAL_POINTS]
        client.publish(
            "rover/lidar/cloud",
            json.dumps({"count": len(out), "rings": len(rings), "points": out}),
        )

    rospy.Subscriber("/tilt", Float32, on_tilt, queue_size=1)
    rospy.Subscriber("/lidar_points", PointCloud2, on_cloud, queue_size=1)
    rospy.spin()


if __name__ == "__main__":
    main()
