# rover-autopilot

Pi-side autonomy loop. It owns the motor commands only while the operator
lease is `auto`; everything else keeps working when it dies.

The crate is split in two:

- `src/lib.rs` is pure logic: scan bins, sector gaps, occupancy grid, A*,
  pursuit, mission FSM, operator lease, flame localisation. No async, no
  hardware, all covered by unit tests.
- `src/main.rs` is the MQTT shell: it keeps the latest feeds, runs a 5 Hz
  tick, and publishes the motor commands.

## Safety model

1. **Operator lease.** `auto` requires a heartbeat from the laptop backend
   (`rover/control/heartbeat`) at least every 3 s. A retained `auto` older
   than 5 s is ignored, and auto is refused for the first 2 s after boot, so
   a crash-restart cannot re-arm itself. Manual mode stops too when the
   heartbeat disappears.
2. **Firmware command deadman.** Every command that carries `arm_ms` must
   be refreshed within that window, or every motor on the board stops. The
   autopilot sends `arm_ms:600` and republishes at 5 Hz. A plain zero has no
   `arm_ms`, so STOP disarms and stops at once.
3. **One 12 s ramp, always.** Manual and autonomous commands share the same
   quadratic ramp. Auto does not get a shorter one: the frame is the limit.
   The autopilot therefore latches one target per segment and does not
   change it while moving. Emergency avoidance is a stop, not a swerve.
4. **Local reflex first.** The gap finder runs on the 8 Hz scan. The camera
   detector can only add a stop (person in the centre third), never a steer.
5. **Board and scan freshness.** Both motor boards must have spoken within
   3 s, and the scan must be under 1 s old, before any autonomous motion.
   Loss of either goes to `Hold`, with the reason on the state topic.
6. **No return path, no driving.** Link loss, gas alarm or thermal abort go
   to `Return`. If the breadcrumb list is empty, the rover holds instead.
7. **Gas alarm.** The backend publishes `rover/alarm/state` from the same
   verdict the dashboard shows. An `alarm` level clears any waypoint and
   stops: the survey returns home, goal mode holds in place.
8. **Stale goals are ignored.** A retained goal older than 5 s cannot re-arm
   navigation after a restart, same rule as the auto mode.

## Driving model

The 12 s ramp makes continuous steering impossible. Each hop is one latched
segment: the target is chosen once, republished unchanged, and the rover
stops before the next target is picked. A pivot segment ends as soon as the
front is clear. A driving segment that sees a close front stops, then pivots
from zero. Hops are 1.5 m or 15 s, and the mission scans between them.

A waypoint goal takes precedence over the survey. The goal is planned with
the same A* on the nav grid (already inflated by the bridge) and followed by
pure pursuit, re-latching one segment every 1.5 s so the ramp can progress.
The route is replanned every 2 s. A close front, person, fire or gas alarm
stops it. Arrival within `goal_tolerance_m` clears the goal and stops.

This is slower than a continuous controller, on purpose. The ramp is what
keeps the frame in one piece.

## State machine

```
IDLE --start--> HOP --arrived--> SCAN --sweep done--> DECIDE
DECIDE --under limits--> HOP
DECIDE --stop/time limit--> RETURN --goal--> IDLE
HOP/SCAN/DECIDE --heartbeat lost, gas, heat--> RETURN
RETURN --route blocked / pose jump--> HOLD
any --scan stale / motor board offline--> HOLD
HOLD --resume--> DECIDE
any --takeover--> MANUAL --release--> IDLE
```

`RETURN` follows the breadcrumbs recorded during `HOP`, reversed. The local
reflex still checks every step. A blocked route stops the rover; it does not
detour into unknown space.

## MQTT contract

### Subscribed

| Topic | Shape | Use |
|---|---|---|
| `rover/lidar/scan` | `{"points":[[deg,mm,q],...]}` | 2 degree bins, front ±100 deg |
| `rover/pose` | `{"x":..,"y":..,"yaw":..}` | map frame, hector TF bridge |
| `rover/map/grid` | `{"resolution":..,"width":..,"height":..,"origin":[x,y],"rle":[[count,value],...]}` | 0 unknown, 1 free, 2 occupied |
| `rover/scan/state` | `{"phase":"scanning","sweeps":N,...}` | scan cycle gate |
| `rover/control/heartbeat` | `{"role":"operator","ts":ms}` | 3 s lease |
| `rover/autonomy/mode` | `{"mode":"auto|manual|idle","ts":ms}` | retained lease request |
| `rover/env/telemetry` | `{"flame_mask":5,"flame_lvl":0.8,"heat_l":..,"heat_r":..}` | fire mark, thermal abort |
| `rover/perception/detections` | `{"items":[{"label":"person","p":0.8,"x":..,"w":..,"range_m":1.4}]}` | centre-third person stop; `range_m` optional and reported as `person_m` |
| `rover/alarm/state` | `{"level":"ok|warn|alarm|unknown","items":[..],"ts":ms}` | gas alarm: clears the goal, returns or holds |
| `rover/nav/goal` | `{"x":..,"y":..,"ts":ms}` or `{"cancel":true}` | waypoint target, retained; older than 5 s is ignored |
| `rover/motor/status` | ESP status | corner board freshness |
| `rover/mid/status` | ESP status | MID board freshness |

`flame_dir` (1..5) is accepted when `flame_mask` is absent, so an ENV board
without the mask patch still works.

`rover/map/grid` is already inflated by 0.25 m on the Pi bridge, so the
waypoint planner does not inflate it again.

### Published

| Topic | Shape | Rate |
|---|---|---|
| `rover/autonomy/state` | state, mode, reason, stops, front_m, person_m, alarm, goal, path_len, nav, fire, thermal | 2 Hz, not retained |
| `rover/mission/state` | state, reason, stops, return_points | on transition, retained |
| `rover/motor/command` | `{"motor":1..4,"speed":s,"arm_ms":600}` | 5 Hz while a segment is latched |
| `rover/mid/command` | `{"motor":1..2,"speed":s,"arm_ms":600}` | 5 Hz while a segment is latched |

Stop is a plain `{"motor":0,"speed":0}` on both boards, which also disarms
the firmware deadline.

Bank map: left = FL(1), RL(3), ML(5); right = FR(2), RR(4), MR(6). MID is
addressed as 1/2 on its own topic and remapped by the backend to 5/6.

## Configuration

Defaults live in `src/config.rs`. Override them with a JSON file:

```
AUTOPILOT_CONFIG=/home/iscream-pi/rover-autopilot.json rover-autopilot
```

Only the keys present in the file change; everything else keeps its default.
The first bench values to revisit are `max_speed`, `stop_distance_m`,
`max_mps`, `max_hop_m`, `hop_seconds`, `flame_bearing_deg` and
`flame_level_floor_v`.

## Run

```
MQTT_BROKER=127.0.0.1 MQTT_PORT=1883 cargo run -p rover-autopilot --release
```

On the Pi it is meant to be a systemd unit next to `rover-cam` and
`rover-lidar-agent`. It needs no ROS libraries: everything arrives over MQTT.

## Related services

- `nav_bridge.py` publishes pose and the nav grid from the ROS container.
- `rover-perception` publishes the detector boxes with lidar-fused ranges.
- `rover-serial-bridge` carries ESP commands and telemetry over USB.
- The laptop backend publishes the operator heartbeat and the gas alarm
  verdict (`rover/alarm/state`).

## Tests

```
cargo test -p rover-autopilot
cargo clippy -p rover-autopilot --all-targets -- -D warnings
```

The 56 tests cover the open field, wall ahead, left/right gaps, narrow slots,
RLE validation, inflation, A* around a wall, no path, corner cutting, path
smoothing, pursuit signs, the segment latch, the full survey cycle,
link-loss return, blocked return, takeover, pose jump, scan timeout,
motor/scan freshness, mission time limit, flame bearing, weak analog
rejection and consecutive-frame confirmation.
