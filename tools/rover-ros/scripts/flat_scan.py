#!/usr/bin/env python3
"""Republish only level A1 scans for 2D laser SLAM.

hector_mapping assumes a horizontal LaserScan. The lidar on the tilt bracket
sends scans at many pitch angles, which makes hector estimate a wrong pose.
This node forwards a scan only while the bracket is nearly level, so SLAM gets
a stable 2D stream while the full tilted stream still feeds the 3D cloud.
"""
import rospy
from sensor_msgs.msg import LaserScan
from std_msgs.msg import Float32

MAX_TILT_DEG = 2.0


def main():
    rospy.init_node("flat_scan")
    pub = rospy.Publisher("/scan_flat", LaserScan, queue_size=2)
    tilt = {"deg": 0.0}

    def on_tilt(msg):
        tilt["deg"] = msg.data

    def on_scan(msg):
        if abs(tilt["deg"]) <= MAX_TILT_DEG:
            pub.publish(msg)

    rospy.Subscriber("/tilt", Float32, on_tilt, queue_size=1)
    rospy.Subscriber("/scan", LaserScan, on_scan, queue_size=2)
    rospy.spin()


if __name__ == "__main__":
    main()
