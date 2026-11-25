# ROS 2D-to-3D lidar pipeline on the Pi

Runs ROS Noetic + rplidar_ros + octomap_server in a Docker container on the Pi.
The A1 gives the horizontal angle; the servo tilts the bracket and reports the
measured pitch on `rover/lidar/tilt_measured`, which `tilt_mqtt.py` republishes
as `rover/lidar/tilt_deg`.

The lidar only spins while the container runs. Stop it to save power.

## Start / stop

The dashboard controls this through the Pi agent, so no SSH is needed:

```sh
# what the Home page buttons publish
mosquitto_pub -h 127.0.0.1 -t rover/lidar/service/command \
  -m '{"action":"start","token":"<agent.token>"}'
```

`rover-lidar-agent.service` accepts `start`, `stop`, `clear_map` and `status`.
Start boots `rover-ros` and the auto-scan loop; stop kills the loop, stops the
container and runs `lidar_off.py`; clear calls `/octomap_server/reset`.
`lidar_off.py` sends the A1 motor PWM 0 command and then raises DTR and RTS,
because this CP210x clone ignores the control lines on its own. After it, the
agent starts `lidar_hold.py` to keep the port open and the motor off.
It reports retained state on `rover/lidar/service/status`: container, mapping,
hold, USB presence and `http_url`. The token file is
`/home/iscream-pi/rover-ros/agent.token` (mode 600) and must match the app's
`auth_token` in `rover_config.json`.

## Auto scan and history

`sweep_loop.py` watches `rover/motor/#`. When every motor reads 0 for 3 s it
starts a scan session and sweeps; the moment any motor moves it sends
`{"action":"stop"}`, parks the servo and saves the session. One parked period
is one session, and it can contain several 48 s sweeps. A move before the first
sweep finishes marks the session `partial`.

`Auto` is the master switch. On starts `rover-ros` and the loop, off stops
both and runs `lidar_off.py`. The Home page `Start scan` / `Stop scan` button
mirrors the same two calls, so the two controls always agree and there is no
"running flat" state. After the one-shot `lidar_off.py`, the agent starts
`lidar_hold.py`, which keeps `/dev/ttyUSB0` open, sends motor PWM 0 every 2 s
and holds DTR/RTS high. That stops this CP210x clone from floating the control
line and restarting the motor after a few seconds. Start stops the hold before
the container starts so rplidarNode gets the port back. The agent status
reports `hold: true/false`. The loop also reads the retained
`rover/lidar/auto_scan` setting, so it never sweeps when the backend says off.
Live state is retained on `rover/scan/state`. A finished session publishes
`rover/scan/saved`.

Each session saves four files under `maps/scans/<id>/`:

- `meta.json`, timestamps, sweeps, counts, partial flag, label.
- `display.json`, the 10 cm surface cloud the app draws.
- `cloud.ply`, portable point cloud for MeshLab or CloudCompare.
- `map.bt`, the OctoMap for RViz.

`save_scan.py` runs inside the container and writes them. A session with zero
points, for example after a failed lidar start, is discarded instead of
appearing in the history. The agent also serves
the archive on `http://<pi>:8082` with the same bearer token:

- `GET /scans` list of metadata.
- `GET /scans/<id>.zip` the four files in one bundle.
- `GET /scans/<id>/<file>` one file.
- `DELETE /scans/<id>` remove a session.

The laptop backend pulls new sessions automatically and keeps its own copy in
`scans/<id>/` next to the app. The History popup reads that copy, so the app
still has the scans when the Pi is off. A checkpoint `.bt` is written every
five sweeps inside the session folder as a crash safety net; it is not a
history entry.

The Home toolbar also has `Save scan`. It asks the loop to close the current
session now, save it, and start a new one if the rover is still parked. It is
enabled only while a session is scanning. The OctoMap is not cleared by a
save; use `Clear map` for a fresh room.

Restarting the agent kills the active sweep loop with the service cgroup. The
loop saves the session on SIGTERM, then the next Start begins a new one.

Manual fallback:

```sh
# on the Pi
docker start rover-ros              # if it already exists
docker stop rover-ros               # stops the A1 motor too
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
- `rover/lidar/display_cloud` is the light surface feed the app paints: one
  averaged point per 10 cm cell, capped at 8,000 points
- `rover/lidar/service/status` is the retained agent state
- `rover/scan/state` is the retained auto-scan state (armed, scanning, paused)
- `rover/scan/saved` is the one-shot session-saved event

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
- Rotating the app token (Setup page) also invalidates the agent. Copy the new
  `auth_token` from `rover_config.json` to
  `/home/iscream-pi/rover-ros/agent.token` afterwards.
- Restarting `rover-lidar-agent` stops the sweep loop with it (systemd owns the
  service cgroup). It saves the active session on SIGTERM, then Start begins a
  new one. The container itself keeps running.
- The scan HTTP service listens on port 8082 and requires the agent token.
  There is no anonymous access to the archive.
- `octomap_display.py` averages OctoMap voxels into 10 cm buckets and caps the
  feed at 8,000 points. The app draws at most 12,000 cloud points and 12,000
  wire edges.
- RViz was left out of the image to keep it small. Maps are saved as `.bt` and
  the dashboard gets the raw cloud over MQTT.
