#!/usr/bin/env python3
"""Bridge the servo commanded angle from MQTT into ROS as /tilt.

The bracket MPU is reliable when the platform is still, but its accelerometer
reading is distorted while the servo is moving. The MG996R follows the
commanded angle closely, so stacking rings by rover/lidar/servo.angle gives a
correct 3D cloud during a continuous sweep. The measured angle is still
published by the ESP for static checks.
"""
import json
import os

import rospy
from std_msgs.msg import Float32
import paho.mqtt.client as mqtt


FLAT_OFFSET_FILE = "/root/rover-ros/flat_offset.txt"


def main():
    rospy.init_node("tilt_mqtt")
    try:
        with open(FLAT_OFFSET_FILE) as f:
            flat_offset = float(f.read().strip())
    except (OSError, ValueError):
        flat_offset = 0.0
    rospy.loginfo("tilt flat offset: %.2f deg" % flat_offset)
    pub = rospy.Publisher("/tilt", Float32, queue_size=2)
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-tilt-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()

    def on_message(_client, _userdata, msg):
        try:
            body = json.loads(msg.payload.decode("utf-8", "replace").strip())
        except json.JSONDecodeError:
            return
        if body.get("servo_ok") != 1:
            return
        try:
            angle = float(body["angle"]) - flat_offset
        except (KeyError, ValueError, TypeError):
            return
        pub.publish(Float32(max(-90.0, min(90.0, angle))))

    client.on_message = on_message
    client.subscribe("rover/lidar/servo")
    rospy.spin()


if __name__ == "__main__":
    main()
