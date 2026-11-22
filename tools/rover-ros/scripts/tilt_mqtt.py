#!/usr/bin/env python3
"""Bridge the measured bracket tilt (degrees) from MQTT into ROS as /tilt.

The ESP32 publishes rover/lidar/tilt_measured at 4 Hz with the angle from the
bracket MPU. Using measured tilt stacks each ring at the angle the bracket
actually reached, instead of the angle that was commanded. One update is
published on each settled rising edge, so MPU noise does not create a ring
for every 0.5 degree bucket. Without a bracket MPU this node publishes
nothing, and the cloud stays flat at zero.
"""
import json

import rospy
from std_msgs.msg import Float32
import paho.mqtt.client as mqtt


def main():
    rospy.init_node("tilt_mqtt")
    pub = rospy.Publisher("/tilt", Float32, queue_size=2)
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-tilt-ros")
    client.connect("127.0.0.1", 1883, 30)
    client.loop_start()

    prev_settled = None

    def on_message(_client, _userdata, msg):
        nonlocal prev_settled
        text = msg.payload.decode("utf-8", "replace").strip()
        try:
            body = json.loads(text)
        except json.JSONDecodeError:
            return

        if msg.topic == "rover/lidar/tilt_measured":
            if body.get("ok") != 1:
                return
            settled = body.get("settled")
            angle = body.get("angle")
            if prev_settled is None:
                prev_settled = settled
                return
            # One ROS /tilt update per completed move. Angle noise at 4 Hz
            # would otherwise create a ring for every 0.5 degree bucket.
            if settled == 1 and prev_settled == 0:
                try:
                    value = float(angle)
                except (ValueError, TypeError):
                    prev_settled = settled
                    return
                pub.publish(Float32(max(-90.0, min(90.0, value))))
            prev_settled = settled

    client.on_message = on_message
    client.subscribe("rover/lidar/tilt_measured")
    rospy.spin()


if __name__ == "__main__":
    main()
