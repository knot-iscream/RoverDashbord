#!/bin/bash
# Start the ROS lidar -> point cloud -> OctoMap pipeline inside the container.
set -e
source /opt/ros/noetic/setup.bash
export ROS_MASTER_URI=http://localhost:11311

roscore &
sleep 3

rosrun rplidar_ros rplidarNode \
  _serial_port:=/dev/ttyUSB0 \
  _serial_baudrate:=115200 \
  _frame_id:=laser \
  _angle_compensate:=true &

python3 /root/rover-ros/scripts/tilt_mqtt.py &
python3 /root/rover-ros/scripts/scan_to_cloud.py &
python3 /root/rover-ros/scripts/scan_to_mqtt.py &
python3 /root/rover-ros/scripts/cloud_to_mqtt.py &

sleep 3

rosrun octomap_server octomap_server_node \
  _frame_id:=map \
  _resolution:=0.05 \
  _filter_ground:=false \
  _sensor_model/max_range:=10.0 \
  cloud_in:=/lidar_points &

wait
