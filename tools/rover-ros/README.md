# ROS 2D-to-3D lidar pipeline on the Pi

Runs ROS Noetic + rplidar_ros + octomap_server in a Docker container on the Pi.
The A1 gives the horizontal angle; the tilt comes from `rover/lidar/tilt_deg`
on the local MQTT broker. Until a tilt sensor or servo is wired, tilt is 0, so
the map is a flat slice. Hand tilt plus a published angle makes it 3D.

The lidar only spins while the container runs. Stop the container to save power.

## Start / stop

```sh
# on the Pi
sudo docker start rover-ros          # if it already exists
sudo docker stop rover-ros           # stops the A1 motor too
```

First-time create:

```sh
sudo docker run -d --name rover-ros --network host \
  --device /dev/ttyUSB0:/dev/ttyUSB0 \
  -v /home/iscream-pi/rover-ros/maps:/root/rover-ros/maps \
  rover-ros:noetic /root/rover-ros/run.sh
```

Build the image after editing the Dockerfile or any script:

```sh
sudo docker build -t rover-ros:noetic /home/iscream-pi/rover-ros
```

## Topics

- `/scan` from rplidar_ros (about 8 Hz)
- `/tilt` (deg) from `rover/lidar/tilt_deg` on MQTT
- `/lidar_points` PointCloud2, tilt baked in
- `/octomap_full`, `/octomap_binary`, `/octomap_point_cloud_centers`
- `rover/lidar/scan` and `rover/lidar/cloud` on MQTT for the Rust dashboard

Set the tilt from the PC or the Pi:

```sh
mosquitto_pub -h 192.168.0.175 -t rover/lidar/tilt_deg -m 15
```

## Save a map

```sh
sudo docker exec rover-ros /ros_entrypoint.sh \
  rosrun octomap_server octomap_saver -f /root/rover-ros/maps/room.bt
```

The `.bt` files land in `/home/iscream-pi/rover-ros/maps/` on the Pi host.

## Notes

- The Pi root was changed from `overlayroot=tmpfs` to `overlayroot=disabled` in
  `/boot/firmware/cmdline.txt` because the 4 GB RAM overlay could not hold the
  ROS image. The old value is in `cmdline.txt.overlayroot-backup`. The root is
  now the 48 GB SD card and Docker images survive a reboot.
- Docker uses the `fuse-overlayfs` storage driver (`/etc/docker/daemon.json`).
- `rover-lidar.service` (the old Python bridge) is stopped and disabled. The
  ROS container owns `/dev/ttyUSB0`; do not run both at once.
- RViz was left out of the image to keep it small. Maps are saved as `.bt` and
  the dashboard gets the raw cloud over MQTT.
