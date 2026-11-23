# Changelog

## 4.1.0a-dev / 4.1.0b-stable - 2026-11-23 - torque ramp, schematic addresses

- All six motors now ease into a new speed over 12 s with a quadratic
  ease-in. Slow at first, larger steps near the end, so the holders are not
  jolted. STOP is immediate and calibration bypasses the ramp. Corner and MID
  firmware report the applied speed in telemetry, not just the command.
- INA226 addressing follows the wiring schematic after all: U2 0x41, U7 0x44,
  U6 0x45 and U9 0x40 on the main I2C bus. The TCA9548A carries only the four
  ADXL345 on channels 0-3. The corner firmware no longer selects the mux for
  INA begin, recheck or reads. This reverses the v4.0 decision below.
- Two builds from the same source: `4.1.0a-dev` and `4.1.0b-stable`. Cargo
  holds valid semver (`4.1.0-dev.1` and `4.1.0`) while the tags, zip names
  and UI labels carry the a/b names. VBS still needs wiring on the real
  board; the schematic export has all four VBUS pins open.

## v4.0-dev1 - 2026-11-21 - standalone dev build, no Raspberry Pi

- New `dev/4.0-standalone` branch off v4.0. Stable v4.0 and `master` are
  untouched. Version is `4.0.1-dev1` in all four Cargo manifests, the lock
  file and the Tauri window title, so `/api/health` tells the two builds
  apart. The stale `2.0.2` in `tauri.conf.json` was corrected in the process.
- `cargo pack-dev` runs the normal pack, then adds `DEV/` to the portable
  folder: the three Arduino sketch folders with the dev WiFi preload applied,
  a launcher, a library list, the wiring PDF and a start guide, and zips it
  as `RoverDash-4.0-dev1-portable.zip`. No git clone, no Pi, no broker setup.
- Dev firmware bakes `Xenfier_5G` into all three sketches. A `DEV_WIFI_PRELOAD`
  branch in each `wifi_portal.h` joins that network before WiFiManager runs,
  so a fresh board is online without phone setup. If it fails, the unchanged
  `Rover-Setup` portal still opens. Credentials live in an ignored
  `dev-overlay/dev.env` and never enter git; tracked firmware stays byte-for-
  byte v4.0 outside the packaging step.
- `DEV-START.bat` / `dev-start.ps1` self-elevate, add an inbound firewall rule
  for the bundled Mosquitto on 1883, detect the PC's active LAN IPv4, write it
  into the three dev `config.h` broker lines, and launch the app. The friends
  then flash the sketches from Arduino IDE. No compiled firmware is shipped.
- App defaults are unchanged: no `rover_config.json` in the zip, first run
  starts the bundled broker on `0.0.0.0:1883` and works entirely offline.
  All pages are present; LiDAR and camera show their offline states without
  the Pi. Setup page carries a small DEV badge.

## v4.0 - 2026-11-21 - 4-channel 6A corners, 3S LiPo rail, derived O2

- INA226 modules keep the stock 0.1 Ω R100 shunt. This release first put all
  four at 0x40 behind TCA9548A channels 4-7 while the ADXL345 kept 0-3; the
  4.1.0 entry above reverses that in favour of the schematic addresses. The
  firmware re-applies the shunt calibration on reconnect. The bench calibration is
  0.1 Ω / 0.8 A, so current clips at 0.8 A and the 2 A over-current rule
  cannot fire. VBS goes to the 12 V rail, not the PWM motor terminal.
- Water pump on ESP-3: GPIO 25 drives an IRLZ44N low-side switch with a
  100 Ω gate resistor, 10 kΩ pull-down, 2 A branch fuse, 1N5408 flyback and
  100 µF at the pump. `rover/env/command` accepts
  `{"action":"pump","on":1,"seconds":10}`, defaults to a 10 s burst, caps at
  60 s and stops itself if the broker dies. `pump:0/1` rides telemetry and
  the heartbeat. New `POST /api/pump` and a THROW WATER button on `/control`.
- `docs/WIRING.md` rewritten around the bench handover: part ref key, split
  power trees, one current path per corner motor with both legs, exact mux
  SD/SC pairs, the pump switch table and the handover order. 470 µF quantity
  corrected to 2, C9 moved to the 6 V servo rail, flame D4 and A3 marked
  planned.

- The 5-way flame module (RBD-0423) is active LOW on D1..D5, so the ENV
  firmware now reads LOW as a trigger. The module runs from 3.3 V and its A3
  analog output feeds the ADS1115 directly, so the flame channel loses its
  divider and the resistor count drops from twelve to ten. `flame_ok` now
  means the analog channel is readable, since idle HIGH proves nothing.
- No AS5600 angle encoder is fitted, so the measured-angle feature is gone.
  The MID firmware drops the encoder driver, its two-point calibration and
  the `servo_cal` command; a sweep settles on time and publishes the
  commanded angle only. `angle_real` is never sent, and `as5600_ok` and
  `cal_ok` stay 0 for payload compatibility. The Calibration page loses its
  Servo row and `sensor_cal` loses the `servo` key. A second MPU6050 at 0x69
  is wired for a future bracket-tilt measurement but is not read yet.
- Corner motors move off the L298N onto one RBD-3658 4-channel 6 A board
  (`DC_MOTOR_4`, MJKDZ). Two logic inputs per channel, no enable pin: PWM
  rides one input, the other stays low, direction is which input pulses.
  The pin map reuses the old direction wires (`IN1..IN8` on GPIO
  15/14, 19/23, 33/27, 17/16) and the four `ENA` wires come off the harness.
  `motor_driver.h` gains a 500 us direction dead time; zero means coast,
  `MOTOR_STOP_BRAKE` switches it to brake. The MID board keeps its L298N and
  `rover/mid/*` is untouched.
- Power rail is now a 3S LiPo 4500 mAh (11.1 V nominal, 12.6 V full) feeding
  the motors, the Pi buck and the arm buck. README documents the tree, the
  20 A fuse, the 12 AWG main run and the balance-charger requirement.
- Health math: low voltage is 10.2 V (3.4 V per cell) and only evaluated at
  `|speed| >= 200`, because the INA226 reads the PWM-averaged motor terminal
  voltage and the old 11.5 V line was tuned for a sagging 12 V SLA.
  `UNDER_LOAD_DUTY`, `LOW_VOLTAGE_V` and `OVER_CURRENT_A` are named
  constants; `tests/fixtures/health.json` gains a half-throttle case and the
  low-voltage cases now run at full duty.
- ENV board: the galvanic O2 cell is gone (`ADS_PGA_O2`, `o2_v0_mv`, the
  `o2_mv`/`o2_pct` fields and the NVS reference removed). It publishes
  `o2_ok:0` so a future cell can take the field back without a code change.
- New `src/o2_estimate.rs`: derived O2 from the gas ratios, flame channels
  and MLX pair, applied once at ENV ingest so `/api/env`, `env_update`, the
  chart ring, the export and the alarm verdict all share one number. 20.9 %
  down to 16.9 %, 2 s attack / 20 s release, reset after a 10 s feed hole, no
  estimate when nothing is evaluable. `env_alarm` labels it "O2 low
  (estimated)" and sets `"estimated": true`; a real `o2_ok:1` frame keeps the
  plain label.
- Calibration page drops the O2 row (a model output has nothing to capture),
  `SENSOR_KEYS` is now 5. The Excel Sensors sheet keeps `o2_mv` for old
  workbooks and adds `o2_est`.
- Detailed page O2 card becomes "O2 estimate" with a `~` on the value, a
  model-load row and the active inputs. Charts and the home gas panel are
  otherwise unchanged.

## Unreleased — 6-motor sensors + ENV node (INA226 + ADXL345 + ESP-2 MID + ESP-3 ENV)

- ESP-1 corners: INA219 → INA226 (stock 0.1 Ω R100 shunt, 0.8 A cap as revised on 20 Nov, see the top entry),
  SW-420 → 4x ADXL345 (GY-291) via TCA9548A mux, all at 0x53 on channels
  0-3. Telemetry gains additive `vib_mag/ax/ay/az` (health math
  unchanged, still gated on the `vibration` bool).
- New `firmware/ESP32-WROOM-32-MID/`: ESP-2 drives the 2 blind middle
  motors on `rover/mid/*` (backend ids 5-6, commands routed by
  `POST /api/motor`). No sensors: validity flags 0, readings `--`.
- Motor Test page gains ML/MR tabs (motors 5-6); other pages stay
  4-corner until hardware day proves the middles.
- New `firmware/ESP32-WROOM-32-ENV/`: ESP-3 reads 5 MQ cells as relative
  `R0/Rs` (1.0 = clean air, no ppm claims), a galvanic O2 cell, the analog
  flame level plus 5 direction lines, and an MLX90614 pair; clean-air
  baseline and `burn_in_s` persist in NVS. MID board gains the MPU6050
  (`rover/imu`, 4 Hz) and the lidar swing servo with a hard ±35° clamp plus
  AS5600 feedback (`rover/lidar/servo`, `rover/lidar/command`). No new
  Arduino libraries, all register-level drivers.
- Backend relays the new feeds: `GET /api/env` + `GET /api/imu` (public,
  `null` before the first frame) and `env_update` / `imu_update` /
  `env_status` over `/ws`, replayed on connect. Env and IMU frames are kept
  out of the motor path, so motor health math and history stay untouched.
- ENV cards live in the Rust UI: the home page gains a gas response panel
  (bars in the node's own green/orange/red, plus an OK/WARN/ALARM chip), the
  Detailed page gains gas, O2, flame, MLX heat, node and IMU sections with
  1 min / 10 min / 1 h charts. The standalone `/fire` page is gone; both old
  paths now 404 (pinned by a test). Null reads render `--`, never a
  fabricated zero.
- `/style.css` revalidates on every load (`no-cache`) and the theme link now
  carries `?v=3`: the app webview once kept a heuristically cached
  stylesheet and rendered the new panels as plain unstyled text, while every
  fresh client looked correct.
- Sensor charts place points on a real time axis (their `at` stamp; live
  frames are stamped on arrival) and break the line on feed holes wider than
  5 s, so range switches stop stretching holes and thinned history. Range
  fetches take a ticket so a slow response can't overwrite a newer range.
- Every `env_update` (WS live + reconnect replay) carries the same `alarms`
  verdict `/api/env` serves, so the UI colors from one source of truth.
- Backend keeps rolling in-memory logs: ENV 2 h at 1 Hz + IMU 2 h at 4 Hz;
  `GET /api/env/history?minutes=N` serves them thinned to <=900 points for
  the charts. Disk persistence is the Pi-session item.
- `GET /api/history/export` (the Detailed page's *Export Excel* button): one
  workbook with Telemetry (motors) + Sensors (ENV log) + IMU sheets. The
  calibration page's own export is untouched.
- `GET /api/env` now ships an indicative alarm verdict beside the raw frame
  (`alarms`: O2 low, gas `R0/Rs` bars, flame direction, MLX heat delta; levels
  `ok` / `warn` / `alarm` / `unknown`). A null never alarms, `baseline_ok:0`
  marks the gas rules uncalibrated, and a frame with nothing evaluable stays
  `unknown` — never `ok`. Indicative only, not life-safety.
- Desktop shell no longer spawns the bundled mosquitto sidecar when
  `mqtt_broker` points at a remote host (Pi-phase prep); the local default is
  unchanged, and the probe now follows the configured `mqtt_port`.
- Motor telemetry now passes an explicit `rover/motor/*` / `rover/mid/*`
  allowlist: an unknown topic is dropped with a log line instead of being
  filed as a motor sample.
- README wiring rewritten for the new sensors + MID board.

## 2.0.2 (2026-10-14) — bug-free cutover release

- Cache headers kill the black-screen class: entry HTML `no-store`,
  hashed bundle immutable. Poisoned WebView caches need one hard reload.
- App versions aligned at 2.0.2 (health endpoint, Tauri metadata).
- Portable zip rebuilt clean (no token config, current bundle).
- KiCad motor driver sources (`PCB/`) published with the app.
- `cargo launch | dev | doctor | pack` (`xtask/`) replace the batch
  launchers with one language: real sleeps, identical headless behavior,
  unit-tested response parsers.
- Boot splash generated by `desktop/build.rs` from a Rust constant.

## 3.0 (2026-10-14) — full Rust UI, portable release

- All four dashboard pages ported to Leptos WASM (`frontend-wasm/`):
  home (live telemetry with sim fallback), detailed, motor test, calibration
  (warmup/sweep, manual rows, history timeline). Shared shell: dock,
  Ctrl+K palette, Tauri frameless controls. Vanilla fallback deleted;
  `/`, `/detailed/`, `/test/`, `/calibration/` and `/next/*` serve the
  Rust bundle (`style.css`, setup page, splash kept).
- WASM bundle fixed at ~2.3MB (`.cargo/config.toml` pins `-C
  strip=symbols` for the wasm target; dev bundle ships).
- Portable `dist/RoverDash/` (~5.7MB zipped): release exe + bundle +
  sidecar broker. Desktop main window opens the dashboard root.
- GitHub Pages deployment removed (site unpublished).

## Unreleased — docs + full-build wiring

- `README.md` rebuilt as a single expanded doc: architecture, prerequisites,
  full pinout, per-motor DS18B20, I2C jumpers, L298N, INA219 series, power,
  wiring order, phone portal, ESP32 flash steps, calibration, MQTT topics,
  localhost-only security, file tree (`src/` + `web/` + `firmware/`).
  Replaces the stale shared-GPIO4 claim with the per-motor design and the
  full MQTT payloads with validity flags.
- Firmware `config.h`: DS18B20 full build `5/26/2/4` (was `-1/-1/-1/4`
  bench). Each pin needs its own 4.7k pull-up to 3.3V. Re-flash and
  bench-check pending (all 4 motors `temp_valid:1`).

## 2.0 (2026-10-09) — Rust port, hardened

Full 1:1 port of the v1 Python backend (Axum + rumqttc + rust_xlsxwriter),
plus the fixes and hardening listed below. v1 is preserved under the
`v1-python` tag. Dashboard pages are vendored in `web/` — the repo is
self-contained; the Leptos rewrite was dropped (placeholder only, may return
with a genuine reason).

**Independence**
- Dashboard HTML/CSS/JS copied into `web/`; `WEB_ROOT` defaults to it.
- Deleted `frontend-wasm/`, `web/dist/`, and the `?ui=next` switch.

**Bug fixes (each pinned by a test)**
- Own MQTT command echoes no longer corrupt telemetry/history (B1).
- Float speeds coerce like Python instead of reading as 0 (B2).
- Excel exports go to the temp dir and are deleted after streaming; filenames
  carry the pid against same-second collisions (B3).
- Python banker's rounding replicated for health/segments/export (B4).
- Unique MQTT client id per process; 15s keepalive for faster dead-peer
  detection (B5, B6). Broker disconnects clear `mqtt_connected` via the poll
  error path (verified: rumqttc v4 rejects incoming DISCONNECT, so no
  separate arm is needed) — pinned by a fake-broker test.
- Startup log line always prints (B7); buffered history writer (B8);
  deterministic motor order (B9); lock-free streaming export (B10); atomic
  config/baseline writes with visible errors (B11); graceful shutdown and a
  readable bind-conflict message (B15). Dead code and stale comments removed.

**Security**
- Localhost-only by design: binds `127.0.0.1`, no LAN mode exists.
- Permissive CORS deleted; no CORS headers are ever sent.
- 32-char token (generated once, persisted) gates every mutating endpoint
  and `/ws` via cookie / `?token=` / `Authorization: Bearer`. This stops any
  website you visit from driving the motors behind your back.
- Setup page: token display + regenerate.

**Known minor divergences (documented, not fixed)**
- `GET /api/history/segments` without `?day=` answers 400 (axum rejection);
  v1 answered 422 (FastAPI validation). Same meaning, different code.
- A WebSocket client too slow to keep up drops frames instead of being
  disconnected (v1 removed dead clients). Chosen for robustness at 4 Hz.

**Tests & tooling**
- 54 tests: 22 unit + 10 API + 9 MQTT + 5 history + 4 WS + 4 golden
  fixtures captured from the original Python modules.
- `src/lib.rs` + `build_router` so tests exercise the real route table.
- Criterion benches with measured v1-vs-2.0 speedups (10x / 3.5x / 8.6x).
- CI (fmt, clippy `-D warnings`, tests) on Windows + Linux; MIT license.
- `/api/health` reports `version` + `build` (git hash via `build.rs`).

**Desktop app (new)**
- Tauri v2 shell (`roverdash.exe`): native window (min 1200x800), backend
  in-process, splash while starting, tray icon (X minimizes, quit from the
  menu), single-instance. WebView loads the same `http://127.0.0.1:8000`.
- Portable folder `dist/RoverDash/` (built by `package.bat`, gitignored):
  exe + `web/` + bundled Mosquitto sidecar. Data lives beside the exe.
  `Rover.bat` starts the packaged app, with a `cargo run` dev fallback.
- Supervisor: bundled broker auto-starts iff 1883 is free (external broker
  wins); graceful exit stops exactly the sidecar it started.
- Vendored pages minus the mobile bottom nav and its CSS/JS (dead above
  the 1200px window floor — documented here, not silently dropped).

## v1-python — original Python backend

Preserved as-is under the `v1-python` tag (FastAPI + paho-mqtt + openpyxl,
dashboard served from the repo root). See that tag for its own history.
