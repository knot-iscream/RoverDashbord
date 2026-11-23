# Lidar tilt / SLAM training notes - 22 Nov 2026

## Why the current cloud was poor

- RPLIDAR A1 is a planar 2D sensor. A tilted bracket creates a derived 3D
  cloud; it does not add a second ranging axis.
- At 40 deg/s tilt and 8 Hz scans the pitch changes about 5 degrees per frame.
  That leaves 8 to 10 cm gaps at 2 m and destroys small detail.
- hector_mapping was receiving tilted scans. It assumes a horizontal scan and
  produced a bad pose and bad TF, which also distorted the 3D cloud.
- Rings were keyed by tilt only, so moving the rig overwrote old viewpoints.
- RViz was showing only the newest scan or a PointCloud2 display that was not
  subscribed because the config had been mangled.

## What the research showed

- Low-cost 2D-to-3D projects use a stepper or servo tilted in small steps,
  commonly 1 degree per 50 ms tick, with an encoder where precision matters.
- Multiple tilted 2D lidars at 16, 23 and 30 degrees give much better room
  coverage than one sensor. We only have one.
- Slower tilt gives denser clouds. Dense VLP-16 experiments go down to
  0.05 deg/s; hobby A1 projects sit near 1 deg steps with short dwells.
- Point-cloud papers filter outliers (LOF or statistical), reject near-field
  noise, and fuse IMU heading with 2D SLAM. Our A1 driver has constant quality
  values, so quality filtering is not useful here.
- OctoMap/TSDF reconstruction shows large surfaces far better than raw rings.

## Changes applied today

- `flat_scan.py` forwards only level scans (within 2 degrees) to hector.
- `scan_to_cloud.py` transforms points by the latest `map <- laser` TF,
  rejects ranges below 0.2 m and above 6 m.
- `cloud_to_mqtt.py` keys rings by tilt plus pose cell, so rig movement adds
  viewpoints instead of overwriting them.
- RViz now displays both `/accumulated_cloud` and
  `/octomap_point_cloud_centers`, and is confirmed subscribed to both.
- Added `python3-serial` and `ros-noetic-hector-slam` to the Dockerfile.

## Automated experiment

Started 20:42, running 110 minutes in
`/home/iscream-pi/training/run.sh` on the Pi. It cycles these combos:

| step | settle | dwell | range |
|---|---|---|---|
| 0.5 | 200 | 200 | -25..25 |
| 1 | 300 | 300 | -25..25 |
| 2 | 500 | 400 | -25..25 |
| 0.5 | 100 | 100 | -25..25 |
| 1 | 500 | 500 | -25..25 |
| 1 | 200 | 200 | -35..35 |
| 0.5 | 150 | 150 | -30..30 |
| 2 | 200 | 200 | -30..30 |

Each run clears the cloud, sweeps, saves the MQTT cloud JSON, and appends
metrics to `training/metrics.csv`. First valid result:
`36224 points, 40 rings, xspan 3.18 m, yspan 3.68 m, zspan 0.93 m,
mean range 1.07 m`.

## Impact fix after the first two-hour run

The user reported that every sweep start slammed the servo from centre to the
minimum angle and hit the mechanical stop. Cause: `startSweep()` called
`gotoDeg(sweep_min)` directly. Fixed by ramping to the first angle with
`rampTo()` and by moving the ramp handling ahead of the sweep state machine in
`update()`. Verified with servo power off: the published angle moved through
-6, -22, then -25 instead of jumping. The user then confirmed a smooth start
with power on. Servo parked at 0 and bracket MPU re-zeroed afterwards.

## Reverse-order cross-check

A fresh ROS container was started to clear hector drift, then one reverse pass
of the same eight configs was launched at 22:53 into `training2/`. Only one
pass, not another two hours, because the first run already repeated the matrix
about twenty times.

The ranking held. Winner in both orders was 0.5 deg step, 200 ms settle, 200 ms
dwell, ±25 deg: 35421 to 36224 points and 40 rings in the first run, 35803
points and 40 rings with z span 1.01 m in the reverse run. Runner-up was
0.5 deg / 150 ms / 150 ms / ±30 deg at 36503 points, 40 rings, z span 1.07 m.
Coarse 2 deg steps produced only 21k to 24k points and 26 to 28 rings, so the
slow fine sweep is the right setting.

## Drift fix and shutdown

The apparent drift was hector pose drift, not bad lidar points. With no wheel
odometry and only a few near-level scans per sweep, the pose coasted and the
cloud ended up about 12 m from the map origin late in the first run. A fresh
ROS restart brought it back near zero.

Permanent fix applied: `detilt_scan.py` replaces `flat_scan.py`. It projects
every scan into the horizontal plane and publishes `/scan_flat` at the full
8 Hz, so hector gets continuous updates during the sweep. Verified after two
sweeps: TF translation stayed at x 0.005 m, y -0.006 m.

At 23:03 the agreed shutdown was done: `rover-rviz` and `rover-ros` stopped,
`lidar_off.py` ran (DTR and RTS high), servo parked at 0, `hold_lidar.py`
stopped, Pi left on with mosquitto active, and the PC dashboard stopped. The
servo red V+ wire is still physically connected; only software parking was
possible remotely.

## 23 Nov training on the new surface

Auto-flat on the softer surface found a 1 deg offset. Commanded 5 published as
ROS `/tilt` 4, confirming the bridge subtraction. `flat_offset.txt` holds 1 and
`tilt_mqtt.py` now reads it at startup.

The 45 minute run was paused once and resumed from the missing pass 4 config
with a fresh timer. It finished at 23:31 with 88 runs across the eight configs
in `training3/`.

Result summary:

| config | runs | avg count | rings | zspan |
|---|---|---|---|---|
| 0.5 / 200 / 200, ±25 | 11 | 40000 | 40.0 | 2.48 |
| 1 / 500 / 500, ±25 | 11 | 40000 | 40.0 | 2.52 |
| 1 / 200 / 200, ±35 | 11 | 40000 | 40.0 | 3.30 |
| 0.5 / 150 / 150, ±30 | 11 | 39878 | 39.0 | 2.92 |
| 1 / 300 / 300, ±25 | 11 | 39407 | 39.2 | 2.42 |
| 2 / 200 / 200, ±30 | 11 | 29718 | 27.5 | 2.91 |
| 0.5 / 100 / 100, ±25 | 11 | 28712 | 26.7 | 2.47 |
| 2 / 500 / 400, ±25 | 11 | 27336 | 25.4 | 2.52 |

Several configs hit the 40000 point cap, so count alone cannot rank them.
Among the capped configs, 0.5 / 200 / 200 keeps 40 rings with the smallest z
span, 2.48, so it remains the best setting. The coarse 2 deg settings lose
about a third of the points and around 13 rings.

The new surface gives larger z spans than the 22 Nov run, roughly 2.5 m versus
1.0 m, and mean range around 1.3 m versus 0.9 m. That is a room/layout change,
not a regression in the sweep logic.

## Final setting put into use

The winning profile was baked into the gitignored temp firmware at 23:49:
`SERVO_SWEEP_MIN_DEG=-25`, `SERVO_SWEEP_MAX_DEG=25`, `SERVO_STEP_DEG=0.5`,
`SERVO_SETTLE_MS=200`, `SERVO_DWELL_MS=200`. The 64-step cap was raised to 128,
because 50 degrees at 0.5 degree steps needs 101 steps. Verified by publishing
`{"action":"sweep"}` with no parameters: the servo reported 101 steps and moved
in 0.5 degree increments.

A continuous non-clearing loop is running from `/home/iscream-pi/mapping_loop.sh`.
It publishes a bare sweep every 46 seconds, never clears the cloud, and saves
`/home/iscream-pi/rover-ros/maps/room-<timestamp>.bt` every five sweeps using
`octomap_saver`. OctoMap had 70156 nodes at 0.05 m resolution when checked.

## Filtering and ring budget, 24 Nov

Raised `cloud_to_mqtt.py` to 120 rings and 100000 points, publish every 1 s.
Rings from repeated sweeps are merged in 0.25 degree angle bins, averaging only
points in the same tiny bin; no gaps are bridged across occlusions.

Added a PCL nodelet chain in `run.sh`, installed with `ros-noetic-pcl-ros`,
`ros-noetic-pcl-conversions` and `ros-noetic-nodelet`:

- `pcl/VoxelGrid`, leaf 0.03 m
- `pcl/StatisticalOutlierRemoval`, mean_k 8, stddev 1.0
- `pcl/RadiusOutlierRemoval`, radius 0.06 m, min_neighbors 2

Chain is `/lidar_points` to `/lidar_voxel` to `/lidar_stat` to
`/lidar_filtered`. Both `cloud_to_mqtt` and `octomap_server` subscribe to
`/lidar_filtered`. Nodelet loads must run in the background and must use the
private `~input` / `~output` remaps; `in` / `out` does not work.

Raised the Rust backend MQTT packet limit from 1 MiB to 4 MiB in `src/mqtt.rs`
so the larger cloud reaches the app. Tests and clippy stayed green.

Verified after two sweeps of the non-clearing loop: broker cloud had 33393
points in 93 rings, z span 0.81 m; app `/api/lidar/cloud` had 37883 points in
101 rings with `mqtt_connected: true`. The point count is lower than the old
capped 40000 because voxel and outlier filters removed noise; the important
change is that nearly all 101 tilt rings survive.

## Floor erasure fix, 24 Nov

The earlier note blaming radius/statistical outlier filters was wrong. Staged
testing showed voxel-only still removed every negative-z point. The cause was
PCL VoxelGrid's default pass-through: `filter_field_name: z`,
`filter_limit_min: 0.0`, `filter_limit_max: 1.0`. It silently discarded the
floor because the lidar is above map z=0.

Fix: widen the voxel filter to `_filter_limit_min:=-2.0` and
`_filter_limit_max:=2.0`. Statistical and radius outlier nodelets were removed
for now; voxel-only still reduced 121k raw points to about 49k during a sweep.

Verified during a sweep:

- raw `/lidar_points`: 121159 points, min z -0.51, 1843 below -0.35 m
- filtered `/lidar_filtered`: 49013 points, min z -0.51, 1375 below -0.35 m

After two sweeps of the resumed non-clearing loop: accumulated cloud had
92738 points, 100 rings, z -1.04 to 1.12, with 9252 points below -0.35 m.
App API showed 95352 points and 100 rings with mqtt connected.

Geometric limit remains: at knee height with ±25 deg tilt, the floor patch is
a ring around the rover, not directly underneath it.

## Reality check

This setup can show walls, floor, ceiling and large objects. It will not
resolve a human silhouette like a real 3D lidar or depth camera. To reach that
level the next hardware step is a 3D sensor, or at least multiple tilted A1s
with accurate extrinsics and a proper SLAM backend.
