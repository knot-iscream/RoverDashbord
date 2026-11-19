#!/bin/bash
# On the Pi: start the lidar/ROS pipeline only when a scan is needed.
sudo docker start rover-ros 2>/dev/null || sudo docker run -d --name rover-ros \
  --network host \
  --device /dev/ttyUSB0:/dev/ttyUSB0 \
  -v /home/iscream-pi/rover-ros/maps:/root/rover-ros/maps \
  rover-ros:noetic /root/rover-ros/run.sh
