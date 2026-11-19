#!/usr/bin/env python3
"""Accumulate /lidar_points and publish a 3D cloud to rover/lidar/cloud.

The Rust home page can render this as a real 3D cloud. OctoMap still builds
its own occupancy map in parallel; this is the feed for the dashboard.
"""
import json

import rospy
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2
import paho.mqtt.client as mqtt

MAX_POINTS = 30000


def main():
    rospy.init_node("cloud_to_mqtt")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-cloud-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()
    acc = []

    def on_cloud(msg):
        for p in point_cloud2.read_points(msg, field_names=("x", "y", "z"), skip_nans=True):
            acc.append([round(p[0], 1), round(p[1], 1), round(p[2], 1)])
        if len(acc) > MAX_POINTS:
            del acc[: len(acc) - MAX_POINTS]
        client.publish(
            "rover/lidar/cloud",
            json.dumps({"count": len(acc), "points": acc}),
        )

    rospy.Subscriber("/lidar_points", PointCloud2, on_cloud, queue_size=2)
    rospy.spin()


if __name__ == "__main__":
    main()
