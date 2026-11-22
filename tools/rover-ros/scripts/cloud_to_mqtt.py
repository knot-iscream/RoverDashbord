#!/usr/bin/env python3
"""Accumulate /lidar_points into rings keyed by tilt and SLAM pose.

A ring is one tilt slice from one rover position. Keying by pose as well as
tilt means moving the rig adds new viewpoints instead of overwriting the old
ones. The point cloud is cleared over MQTT when the UI asks.
"""
import json
import math
import time

import rospy
import tf2_ros
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2
from std_msgs.msg import Float32
import paho.mqtt.client as mqtt

MAX_RINGS = 40
MAX_TOTAL_POINTS = 40000
PUBLISH_EVERY_S = 0.5


def main():
    rospy.init_node("cloud_to_mqtt")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-cloud-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()
    rings = {}
    current_tilt = 0.0
    last_publish = 0.0
    tf_buffer = tf2_ros.Buffer()
    tf_listener = tf2_ros.TransformListener(tf_buffer)

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
        nonlocal last_publish, rings
        pts = [
            [round(p[0], 3), round(p[1], 3), round(p[2], 3)]
            for p in point_cloud2.read_points(msg, field_names=("x", "y", "z"), skip_nans=True)
        ]
        if not pts:
            return
        pose = (0.0, 0.0, 0.0)
        try:
            trans = tf_buffer.lookup_transform(
                "map", "laser", rospy.Time(0), rospy.Duration(0.5)
            )
            pos = trans.transform.translation
            rot = trans.transform.rotation
            yaw = 2.0 * math.atan2(rot.z, rot.w)
            pose = (round(pos.x * 2.0) / 2.0, round(pos.y * 2.0) / 2.0,
                    round(math.degrees(yaw) / 10.0) * 10.0)
        except (tf2_ros.LookupException,
                tf2_ros.ConnectivityException,
                tf2_ros.ExtrapolationException):
            pass
        key = (round(current_tilt * 2.0) / 2.0, pose[0], pose[1], pose[2])
        rings[key] = pts
        if len(rings) > MAX_RINGS:
            # Keep the centre and both extremes instead of throwing away the
            # rings near zero tilt, which left a hollow fan.
            keys = sorted(rings)
            keep = {
                int(round(i * (len(keys) - 1) / (MAX_RINGS - 1)))
                for i in range(MAX_RINGS)
            }
            rings = {k: rings[k] for i, k in enumerate(keys) if i in keep}
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
