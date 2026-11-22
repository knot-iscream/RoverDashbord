#!/usr/bin/env python3
"""Publish a level-projected LaserScan for 2D SLAM at the full scan rate.

The lidar is on a tilting bracket. hector_mapping needs a horizontal scan, and
forwarding only near-level scans gives it just a few updates per sweep, so the
pose coasts and drifts. This node projects every scan into the horizontal
plane using the current bracket tilt and publishes it at the full 8 Hz.

The tilt is a rotation about the lidar X axis, matching scan_to_cloud.py:

    x = r*cos(a),  y = r*sin(a)*cos(tilt),  z = r*sin(a)*sin(tilt)

The horizontal range is sqrt(x*x + y*y). Points outside 0.2 to 6 m become NaN
so hector ignores them.
"""
import math

import rospy
from sensor_msgs.msg import LaserScan
from std_msgs.msg import Float32

MIN_RANGE = 0.2
MAX_RANGE = 6.0


def main():
    rospy.init_node("detilt_scan")
    pub = rospy.Publisher("/scan_flat", LaserScan, queue_size=2)
    tilt = {"deg": 0.0}

    def on_tilt(msg):
        tilt["deg"] = msg.data

    def on_scan(msg):
        flat = LaserScan()
        flat.header = msg.header
        flat.angle_min = msg.angle_min
        flat.angle_max = msg.angle_max
        flat.angle_increment = msg.angle_increment
        flat.time_increment = msg.time_increment
        flat.scan_time = msg.scan_time
        flat.range_min = msg.range_min
        flat.range_max = msg.range_max

        t = math.radians(tilt["deg"])
        ct = math.cos(t)
        ranges = []
        a = msg.angle_min
        for r in msg.ranges:
            if math.isfinite(r) and MIN_RANGE <= r <= MAX_RANGE:
                x = r * math.cos(a)
                y = r * math.sin(a) * ct
                ranges.append(math.hypot(x, y))
            else:
                ranges.append(float("nan"))
            a += msg.angle_increment
        flat.ranges = ranges
        flat.intensities = msg.intensities
        pub.publish(flat)

    rospy.Subscriber("/tilt", Float32, on_tilt, queue_size=1)
    rospy.Subscriber("/scan", LaserScan, on_scan, queue_size=2)
    rospy.spin()


if __name__ == "__main__":
    main()
