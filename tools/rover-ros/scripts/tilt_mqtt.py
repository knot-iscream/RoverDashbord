#!/usr/bin/env python3
"""Bridge the host's MQTT tilt angle (degrees) into ROS as /tilt."""
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

    def on_message(_client, _userdata, msg):
        text = msg.payload.decode("utf-8", "replace").strip()
        try:
            body = json.loads(text)
            angle = float(body["angle"])
        except (ValueError, TypeError, KeyError, json.JSONDecodeError):
            try:
                angle = float(text)
            except (ValueError, TypeError):
                return
        pub.publish(Float32(max(-90.0, min(90.0, angle))))

    client.on_message = on_message
    client.subscribe("rover/lidar/tilt_deg")
    rospy.spin()


if __name__ == "__main__":
    main()
