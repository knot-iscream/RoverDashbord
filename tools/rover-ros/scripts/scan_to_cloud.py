#!/usr/bin/env python3
"""Turn each A1 LaserScan plus a bracket tilt into a 3D PointCloud2.

The A1 gives the horizontal angle. The tilt from /tilt rotates the scan plane
into pitch. No angle is invented: tilt comes from the servo/IMU path on the
host, published to rover/lidar/tilt_deg.

The local point is transformed from the laser frame into map using the TF
tree. With hector_mapping publishing map -> odom and a static odom -> laser,
the rover can move while the cloud stays in a consistent map frame.
"""
import math
import struct

import rospy
import tf2_ros
from sensor_msgs.msg import LaserScan, PointCloud2, PointField
from std_msgs.msg import Float32


def rotate(q, v):
    x, y, z = v
    qx, qy, qz, qw = q.x, q.y, q.z, q.w
    tx = 2.0 * (qy * z - qz * y)
    ty = 2.0 * (qz * x - qx * z)
    tz = 2.0 * (qx * y - qy * x)
    return (
        x + qw * tx + qy * tz - qz * ty,
        y + qw * ty + qz * tx - qx * tz,
        z + qw * tz + qx * ty - qy * tx,
    )


def make_cloud(points, stamp):
    msg = PointCloud2()
    msg.header.stamp = stamp
    msg.header.frame_id = "map"
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
        self.tf_buffer = tf2_ros.Buffer()
        self.tf_listener = tf2_ros.TransformListener(self.tf_buffer)
        self.pub = rospy.Publisher("/lidar_points", PointCloud2, queue_size=4)
        rospy.Subscriber("/tilt", Float32, self.on_tilt, queue_size=1)
        rospy.Subscriber("/scan", LaserScan, self.on_scan, queue_size=2)

    def on_tilt(self, msg):
        self.tilt = msg.data

    def on_scan(self, scan):
        try:
            trans = self.tf_buffer.lookup_transform(
                "map", "laser", rospy.Time(0), rospy.Duration(0.5)
            )
        except (tf2_ros.LookupException,
                tf2_ros.ConnectivityException,
                tf2_ros.ExtrapolationException) as exc:
            rospy.logwarn_throttle(2, "no TF map<-laser: %s" % exc)
            return

        t = math.radians(self.tilt)
        cp, sp = math.cos(t), math.sin(t)
        q = trans.transform.rotation
        offset = trans.transform.translation
        pts = []
        a = scan.angle_min
        for r in scan.ranges:
            if math.isfinite(r) and 0.2 <= r <= 6.0:
                x = r * math.cos(a)
                y = r * math.sin(a)
                rx, ry, rz = rotate(q, (x, y * cp, y * sp))
                pts.append((rx + offset.x, ry + offset.y, rz + offset.z))
            a += scan.angle_increment
        if pts:
            self.pub.publish(make_cloud(pts, scan.header.stamp))


if __name__ == "__main__":
    rospy.init_node("scan_to_cloud")
    ScanToCloud()
    rospy.spin()
