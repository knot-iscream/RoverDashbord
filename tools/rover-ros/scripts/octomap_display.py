#!/usr/bin/env python3
"""Publish OctoMap occupied voxel centres as the dashboard display cloud.

The raw A1 cloud is still published on `rover/lidar/cloud` for recording and
RViz. This feed is the light, surface-like view the app paints instead.
"""
import json
import time

import rospy
from sensor_msgs.msg import PointCloud2
from sensor_msgs import point_cloud2
import paho.mqtt.client as mqtt

MAX_POINTS = 12000
PUBLISH_EVERY_S = 1.0


def main():
    rospy.init_node("octomap_display")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-octomap-display")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()

    last_publish = 0.0

    def on_cloud(msg):
        nonlocal last_publish
        now = time.monotonic()
        if now - last_publish < PUBLISH_EVERY_S:
            return
        pts = [
            [round(p[0], 3), round(p[1], 3), round(p[2], 3)]
            for p in point_cloud2.read_points(msg, field_names=("x", "y", "z"), skip_nans=True)
        ]
        if not pts:
            return
        if len(pts) > MAX_POINTS:
            step = len(pts) // MAX_POINTS + 1
            pts = pts[::step][:MAX_POINTS]
        last_publish = now
        payload = {"count": len(pts), "rings": 0, "points": pts}
        client.publish("rover/lidar/display_cloud", json.dumps(payload))

    rospy.Subscriber("/octomap_point_cloud_centers", PointCloud2, on_cloud, queue_size=1)
    rospy.loginfo("octomap_display: publishing rover/lidar/display_cloud")
    rospy.spin()


if __name__ == "__main__":
    main()
