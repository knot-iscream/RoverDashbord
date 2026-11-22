#!/usr/bin/env python3
"""Turn each A1 LaserScan plus a bracket tilt into a 3D PointCloud2.

The A1 gives the horizontal angle. The tilt from /tilt rotates the scan plane
into pitch. No angle is invented: tilt comes from the servo/IMU path on the
host, published to rover/lidar/tilt_deg.
"""
import math
import struct

import rospy
from sensor_msgs.msg import LaserScan, PointCloud2, PointField
from std_msgs.msg import Float32


def make_cloud(points, stamp, frame):
    msg = PointCloud2()
    msg.header.stamp = stamp
    msg.header.frame_id = frame
    msg.height = 1
    msg.width = len(points)
    msg.fields = [
        PointField("x", 0, PointField.FLOAT32, 1),
        PointField("y", 4, PointField.FLOAT32, 1),
        PointField("z", 8, PointField.FLOAT32, 1),
    ]
    msg.is_bigendian = False
    msg.point_step = 12
    msg.row_step = 12 * len(points)
    msg.is_dense = True
    buf = bytearray()
    for x, y, z in points:
        buf.extend(struct.pack("<fff", x, y, z))
    msg.data = bytes(buf)
    return msg


class ScanToCloud:
    def __init__(self):
        self.tilt = 0.0
        self.pub = rospy.Publisher("/lidar_points", PointCloud2, queue_size=4)
        rospy.Subscriber("/tilt", Float32, self.on_tilt, queue_size=1)
        rospy.Subscriber("/scan", LaserScan, self.on_scan, queue_size=2)

    def on_tilt(self, msg):
        self.tilt = msg.data

    def on_scan(self, scan):
        t = math.radians(self.tilt)
        cp, sp = math.cos(t), math.sin(t)
        pts = []
        a = scan.angle_min
        for r in scan.ranges:
            if math.isfinite(r) and r >= 0.2:
                x = r * math.cos(a)
                y = r * math.sin(a)
                pts.append((x, y * cp, y * sp))
            a += scan.angle_increment
        if pts:
            self.pub.publish(make_cloud(pts, scan.header.stamp, "map"))


if __name__ == "__main__":
    rospy.init_node("scan_to_cloud")
    ScanToCloud()
    rospy.spin()
