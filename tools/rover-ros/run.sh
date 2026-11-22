#!/bin/bash
# Start the ROS lidar -> point cloud -> OctoMap pipeline inside the container.
set -e
export DISABLE_ROS1_EOL_WARNINGS=1
source /opt/ros/noetic/setup.bash
export ROS_MASTER_URI=http://localhost:11311

# The A1 motor runs while DTR is low. rplidarNode lowers it and does not
# reliably raise it on exit, so force it high when the container stops.
cleanup() {
  python3 /root/rover-ros/scripts/lidar_off.py || true
}
trap cleanup EXIT

roscore &
sleep 3

rosrun rplidar_ros rplidarNode \
  _serial_port:=/dev/ttyUSB0 \
  _serial_baudrate:=115200 \
  _frame_id:=laser \
  _angle_compensate:=true &

# Laser SLAM: hector_mapping estimates map -> odom from /scan_flat alone.
# flat_scan only forwards scans taken while the bracket is level, so tilting
# the lidar for the 3D cloud cannot corrupt the 2D pose estimate.
python3 /root/rover-ros/scripts/flat_scan.py &
rosrun tf2_ros static_transform_publisher 0 0 0 0 0 0 odom laser &

rosrun hector_mapping hector_mapping \
  _scan_topic:=/scan_flat \
  _base_frame:=laser \
  _map_frame:=map \
  _odom_frame:=odom \
  _pub_map_odom_transform:=true \
  _pub_odometry:=true \
  _map_resolution:=0.05 \
  _map_size:=1000 &

python3 /root/rover-ros/scripts/tilt_mqtt.py &
python3 /root/rover-ros/scripts/scan_to_cloud.py &
python3 /root/rover-ros/scripts/scan_to_mqtt.py &
python3 /root/rover-ros/scripts/cloud_to_mqtt.py &
python3 /root/rover-ros/scripts/cloud_to_ros.py &

sleep 3

rosrun octomap_server octomap_server_node \
  _frame_id:=map \
  _resolution:=0.05 \
  _filter_ground:=false \
  _sensor_model/max_range:=10.0 \
  cloud_in:=/lidar_points &

wait
