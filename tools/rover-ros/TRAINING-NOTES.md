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

## Reality check

This setup can show walls, floor, ceiling and large objects. It will not
resolve a human silhouette like a real 3D lidar or depth camera. To reach that
level the next hardware step is a 3D sensor, or at least multiple tilted A1s
with accurate extrinsics and a proper SLAM backend.
