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
# detilt_scan projects every tilted scan into the horizontal plane at the full
# scan rate, so hector gets continuous updates instead of coasting between the
# brief level moments of a sweep.
python3 /root/rover-ros/scripts/detilt_scan.py &
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

# PCL filter nodelets. Only voxel grid for now. Its default z pass-through is
# 0..1 m, which silently deleted the floor, so widen the limits to +/-2 m.
rosrun nodelet nodelet manager __name:=pcl_manager &
sleep 3
rosrun nodelet nodelet load pcl/VoxelGrid pcl_manager __name:=voxel_grid \
  '~input:=/lidar_points' '~output:=/lidar_filtered' _leaf_size:=0.03 \
  _filter_limit_min:=-2.0 _filter_limit_max:=2.0 &
sleep 3

python3 /root/rover-ros/scripts/cloud_to_mqtt.py &
python3 /root/rover-ros/scripts/cloud_to_ros.py &
python3 /root/rover-ros/scripts/octomap_display.py &

sleep 3

rosrun octomap_server octomap_server_node \
  _frame_id:=map \
  _resolution:=0.04 \
  _filter_ground:=false \
  _filter_speckles:=false \
  _sensor_model/hit:=0.85 \
  _sensor_model/miss:=0.35 \
  _sensor_model/max_range:=10.0 \
  cloud_in:=/lidar_filtered &

wait
