#!/usr/bin/env python3
"""Republish the ROS /scan as the dashboard's rover/lidar/scan JSON.

Lets the Rust app keep its live 2D feed while the ROS container owns the
serial port. Same payload shape as tools/rover-lidar/lidar_stream.py.
"""
import json
import math

import rospy
from sensor_msgs.msg import LaserScan
import paho.mqtt.client as mqtt


def main():
    rospy.init_node("scan_to_mqtt")
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-scan-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()

    def on_scan(scan):
        pts = []
        a = scan.angle_min
        for r in scan.ranges:
            if math.isfinite(r) and r > 0.0:
                pts.append([round(math.degrees(a), 2), round(r * 1000.0, 1), 15])
            a += scan.angle_increment
        client.publish(
            "rover/lidar/scan",
            json.dumps({"ok": 1, "model": 24, "count": len(pts), "points": pts}),
        )

    rospy.Subscriber("/scan", LaserScan, on_scan, queue_size=2)
    rospy.spin()


if __name__ == "__main__":
    main()
