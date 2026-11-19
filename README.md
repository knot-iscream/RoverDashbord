# Rover Digital Twin 2.0

Rust backend + the same dashboard. This is a 1:1 replacement of the old
Python backend: every page, button, and live reading works exactly as before,
served from one app on one port. The dashboard pages live in `web/` in this
repo — no other checkout needed.

> The previous Python version is preserved on GitHub under the
> [`v1-python`](https://github.com/knot-iscream/RoverDashbord/releases/tag/v1-python)
> tag, including the ESP32 firmware sources.

## Architecture

```
[ESP32] --MQTT--> [Rust Backend (Axum + rumqttc)] --WebSocket--> [Dashboard (HTML/CSS/JS)]
                       |
                [Calibration Manager] --saves--> [.jsonl + .xlsx]
```

### Components

| Layer | Tech | Location |
|-------|------|----------|
| Firmware | Arduino (ESP32-WROOM-32) | `firmware/ESP32-WROOM-32/` |
| Backend | Axum + rumqttc + rust_xlsxwriter | `src/` |
| Dashboard | Vanilla HTML/CSS/JS | `web/` |
| Desktop shell | Tauri v2 (window + tray + broker sidecar) | `desktop/` |

## Performance (v1 → v2.0)

Measured like-for-like in release builds, same inputs, same machine:

| Workload | v1 (Python) | v2.0 (Rust) | Change |
|---|---|---|---|
| Ingest one telemetry packet | 3171 ns | 310 ns | **-90%** |
| History segments, 10k rows | 66.2 ms | 19.0 ms | **-71%** |
| Excel export, 1k rows | 172 ms | 20.0 ms | **-88%** |

Reproduce the Rust side with `cargo bench`.

**What that did and did not buy.** None of it is visible day to day. At the
firmware's 16 packets/sec the backend now spends about 5 microseconds per
second, where v1 spent about 50 — the latency you actually feel was always
the MQTT round-trip and browser paint, never the backend.

What the 90% bought is **headroom**. On v1, high-rate lidar point clouds,
video streams and a dozen extra sensors were off the table. They are
affordable now.

That headroom is one reason v2.0 ships as a real desktop app: the
backend is no longer the ceiling, so lidar and camera work is no longer
blocked by it. The rest of the app-first move — its own window, tray icon,
background monitoring — has nothing to do with speed.

## Run the app (Windows)

Double-click **`dist\RoverDash\roverdash.exe`** — built once with
`cargo pack` (needs a Rust toolchain, slow first build). Splash, native
window, dashboard. No browser, no address bar, no console.

- X minimizes to the tray instead of quitting; quit from the tray menu.
  A second launch just focuses the running window.
- The app brings its own MQTT broker and starts it when nothing listens
  on 1883. Your data (`rover_config.json`, `history/`) lives beside the
  exe, so the whole folder is portable — copy it to back up or move PCs.
- First launch: Windows SmartScreen warns on the unsigned exe
  (More info → Run anyway, one time).
- Window floor is 1200x800. The vendored pages ship without the old
  mobile bottom nav, which only mattered below that width.

Prefer the old way, or developing? **`cargo launch`** — it starts
the packaged app if present, else runs the backend from source and opens
your browser. Something wrong? **`cargo doctor`** checks everything and tells
you what to do in plain language.

## Start here, from source (developers)

1. Install [Rust stable](https://rustup.rs/) once (and accept the
   Visual Studio Build Tools install it suggests).
2. Install [Mosquitto](https://mosquitto.org/download/) on this PC once
   (the message broker between rover and app) — only needed for
   source runs; the packaged app bundles its own.
3. `cargo run` serves the dashboard at `http://localhost:8000/`

Iterate natively (debug, real data): **`cargo dev`**
(or `cargo run -p roverdash`, or `bacon` for auto-rebuild). Opens the main
window plus a second **Rover Rust View** window pushed live by Rust at ~2Hz
(device presence, WS clients, motor frames — no simulation). `web/` legacy
pages are frozen; future Rust views (lidar 3D as a `wgpu` canvas) land in
the Rust view. Release (`cargo pack`) stays single-window.

Something wrong? **`cargo doctor`** — it checks everything and
tells you what to do in plain language.

## Prerequisites (full build)

| Item | Detail |
|------|--------|
| PC/laptop | Runs backend + broker + dashboard |
| ESP32-WROOM-32 dev board + USB cable | Firmware target (x2 for the 6-motor build: corners + middles) |
| 4x DS18B20 waterproof probes + 4x 4.7kΩ resistors | One probe per motor, 3.3V, pull-up per pin |
| 4x ADXL345 accelerometer modules (GY-291) + 1x TCA9548A I2C mux | Range vibration per corner motor; mux required (all 4 ADXL345 share address 0x53) |
| 4x INA226 current/voltage modules, 10mΩ 1W shunts fitted | Unique I2C address per module via A0/A1 jumpers; stock 0.1Ω shunt replaced |
| 1x 4-channel 6A driver board (RBD-3658, "DC_MOTOR_4", MJKDZ) | Drives the 4 corners: two logic inputs per channel, no enable pin |
| 1x L298N H-bridge board | Drives the 2 blind middles only |
| 6x 37GB 12V 100RPM gear motors | 4 corners monitored, 2 middles blind |
| 3S LiPo 4500mAh 11.1V (XT-60) + 20A fuse + 3S buzzer | Motor/rail supply; full charge is 12.6V, floor 9.0V |
| Arduino IDE + ESP32 core | Firmware flash (libraries listed below) |
| Rust stable + Mosquitto | Backend + broker for source runs |

## Hardware wiring

Full build: 4 corner motors instrumented on ESP-1, 2 blind middle motors
on ESP-2. Pin map is authoritative in
`firmware/ESP32-WROOM-32/config.h` (corners) and
`firmware/ESP32-WROOM-32-MID/config.h` (middles). Do not reuse GPIO 6-11
(tied to internal SPI flash). GPIO 1/3 are UART0. GPIO 34-39 are
input-only.

### Pinout table (ESP-1 corners)

| Motor | ADXL345 (mux ch) | INA226 (addr) | DS18B20 (OneWire) | 6A driver inputs |
|-------|------------------|---------------|-------------------|------------------|
| 1 (FL) | ch 0 | 0x41 | GPIO 5 | IN1=15, IN2=14 |
| 2 (FR) | ch 1 | 0x44 | GPIO 26 | IN3=19, IN4=23 |
| 3 (RL) | ch 2 | 0x45 | GPIO 2 | IN5=33, IN6=27 |
| 4 (RR) | ch 3 | 0x40 | GPIO 4 | IN7=17, IN8=16 |

- Motor PWM is 8-bit (0-255) at 1kHz. Speed command range is -255..255
  (negative = reverse, 0 = coast with both inputs LOW; `MOTOR_STOP_BRAKE`
  switches the stop to a brake on both-HIGH).
- The 6A board has no enable pin: one input takes the duty, the other stays
  low, and the direction is which input pulses. This is the standard
  two-input H-bridge interface (DRV8870/DRV8871, RZ7889, LTK8329), so the
  old L298N `ENA` wires simply come off the harness.
- ADXL345 reports a range (magnitude in g, rest ~= 1.0g). A reading
  counts as vibration when |mag - 1g| exceeds `VIB_THRESHOLD_G` (0.25g
  default, tune on the bench). A missing chip reports `vib_valid:0` and
  the dashboard shows NO SIGNAL, never a fake value.
- Other boards go in sibling `firmware/<board>/` folders. Re-verify the
  pin map against that board before reuse.

### DS18B20 temperature (one bus per motor)

Each motor has its own OneWire pin (5/26/2/4 above). There is no shared
bus and no ROM matching: whichever probe sits on a pin reports as that
motor.

- Each wired pin needs its own **4.7kΩ pull-up to 3.3V**. The firmware
  also re-asserts the internal pull-up before each conversion as backup.
- Probe VCC is **3.3V**. These GPIOs are not 5V-tolerant.
- Reads are async (750ms conversion, 350ms read deadline, 3 retries,
  3-strike hysteresis). A missing or failed probe reports `temp_valid:0`
  and the dashboard shows `--`. Replug self-heals within ~10s, no reboot.

### I2C bus (INA226 + TCA9548A + ADXL345, all share this)

| I2C pin | ESP32 GPIO |
|---------|-----------|
| SDA | GPIO 21 |
| SCL | GPIO 22 |

All 4 INA226 modules, the TCA9548A mux, and (through the mux) the 4
ADXL345 modules connect to the same SDA/SCL lines in parallel.
Each INA226 must have a **unique I2C address** set via its A0/A1 solder
jumpers:

| Address | A0 jumper | A1 jumper | Motor |
|---------|-----------|-----------|-------|
| 0x40 | GND | GND | 4 (RR) |
| 0x41 | VCC | GND | 1 (FL) |
| 0x44 | GND | VCC | 2 (FR) |
| 0x45 | VCC | VCC | 3 (RL) |

The TCA9548A stays at its default `0x70` (A0-A2 open). All four ADXL345
breakouts are strapped SDO→GND so each answers at `0x53`; the mux puts
one on the bus at a time (motor 1→ch 0 … motor 4→ch 3). This is
mandatory: the ADXL345 has only two addresses (0x53/0x1D), so four
cannot share one bus without the mux.

Presence is re-checked every 5s. A chip that answers but returns NaN, or
a bus voltage outside 3-36V, counts as `ina_ok:false` and voltage/current
show as `--`.

### 4-channel 6A driver wiring (corner motors)

One RBD-3658 board (`DC_MOTOR_4`, MJKDZ) drives the four corners. It has two
logic inputs per channel and no enable pin:

- **Pack positive** reaches the board's `3-12V` terminal through the 20 A
  fuse and the master switch. The MID L298N and both buck converters tap the
  same post-fuse rail.
- **Common ground is critical**: ESP32 GND, the 6A board GND, the L298N GND,
  every INA226 GND and the pack negative meet at one point.
- Logic is the 9-pin header `IN1..IN8 + GND`. The 5-pin JST labelled
  `IN1 IN2 IN5 IN6 GND` is a duplicate tap for two channels, not a second
  interface.
- The board makes its own logic supply from the motor rail, so there is no
  separate 5 V pin. Confirm that a 3.3 V ESP32 output actually switches a
  channel before trusting it; if it needs 5 V logic, put a 74HCT245 in
  between.
- PWM on one input with the other held low drives that direction at the
  duty. Both low coasts, both high brakes. The `A B C D` pads are not part
  of this interface and stay as shipped.

### INA226 current sensing (per motor)

Place each INA226 **in series with one motor lead, between the driver
output and the motor terminal**, so it measures the actual load current.
The pack goes to the driver input, not through the INA226.

- Never wire VIN+/VIN- across the motor in parallel. That puts pack voltage
  across the shunt and burns it open (seen on the bench: chip alive, ~7V
  coupled through, ~0A forever).
- The stock 0.1Ω shunt is replaced with a **10mΩ 1W shunt** (10A thermal
  max). Firmware calibrates for 0.01Ω with a 4A cap, which sits above the
  ~2.5A stall of one motor and well under the shunt limit. The old L298N 2A
  channel ceiling no longer applies; the 6A board is rated 6A peak per
  channel. Reverse drive reads negative current; that is normal.
- Bus range is 36V max (was 16V on the old INA219).
- The reading is the PWM-averaged motor terminal voltage, not the pack.
  Near full duty it approximates the pack; at half throttle it reads about
  half. That is why the low-voltage rule only runs at `|speed| >= 200`
  (see `LOW_VOLTAGE_V` in `src/data.rs`).

### Power connections

- **DS18B20**: VCC to 3.3V, GND to GND, DATA to its motor pin with a
  4.7kΩ pull-up to 3.3V. Tape the probe to the motor casing with thermal
  paste.
- **ADXL345 + TCA9548A**: all VCC to 3.3V (never 5V), all GND to GND,
  all SDA/SCL to 21/22 in parallel. ADXL345 SDO→GND on every board
  (all answer at 0x53); mux channels 0-3 go to motors 1-4.
- **INA226**: VCC to 3.3V, GND to GND, SDA/SCL to 21/22.

### Wiring order

1. Wire all INA226 modules + the TCA9548A to the I2C bus first
   (SDA/SCL + addresses, 10mΩ shunts fitted).
2. Connect each DS18B20 DATA pin with its own 4.7kΩ pull-up.
3. Connect the 4 ADXL345 boards through the mux (SDO→GND each).
4. Wire the 6A board's `IN1..IN8` to ESP-1 and the pack rail to its
   `3-12V` input. Ground everything to one point.
5. Run each corner motor lead through its INA226 in series, then to the
   driver output.

### 3S LiPo power tree

One pack feeds the whole rover. The old 12 V SLA is gone.

```
3S LiPo 11.1V 4500mAh (XT-60, JST-XH balance)
  |
  +-- 20A fuse ---- master switch / E-stop
        |
        +-- 6A driver  --> 4 corner motors (INA226 in series per lead)
        +-- L298N      --> 2 middle motors
        +-- XL4016     --> 5.1V 8A --> Pi 5, LiDAR, USB hub -> 3x ESP32
        +-- 12V->6V    --> arm servo rail
```

- Full charge is 12.6 V, nominal 11.1 V, floor 9.0 V (3.0 V per cell).
  The driver's 3-12 V rating is nominal; soak-test one channel at 12.6 V
  before the first run.
- The low-voltage rule warns at 10.2 V (3.4 V per cell) and only while the
  motors are near full duty. Do not rely on it to protect the pack: fit a
  3S low-voltage buzzer on the balance lead (3.5 V per cell) and never
  discharge below 3.0 V per cell.
- 20 A main fuse: four motors stalling near 2.5 A each plus the Pi and the
  arm can reach about 18 A together. 12 AWG for the main run, 18 AWG per
  motor lead.
- Charge with a balance charger (B6-AC class, about an hour at 1C), with
  the pack out of the rover. The B3 is 10 W and takes most of a night.
- Run time is about an hour while driving, several hours parked.
- The L298N dropped 1.4-3 V, so the motors used to see 9-10.5 V. On 11.1 V
  with a ~0.1 V driver they see slightly more and the top speed stays
  flatter across the discharge curve.

### ESP-2 middles (second ESP32) + localization

One more L298N on a second board drives the 2 middle motors with no motor
sensors. The same board also carries the localization subsystem: MPU6050 IMU,
the lidar swing servo (MG996R) and the optional AS5600 angle encoder.
Firmware: `firmware/ESP32-WROOM-32-MID/rover_mid.ino`.

| MID motor | Backend id | L298N pins |
|-----------|-----------|------------|
| Mid 1 | 5 | ENA=13, IN1=15, IN2=14 |
| Mid 2 | 6 | ENB=18, IN3=19, IN4=23 |

| Signal | Pin / address | Notes |
|--------|---------------|-------|
| MPU6050 SDA/SCL | 21 / 22 | address 0x68, 4 Hz on `rover/imu` |
| Servo signal | GPIO 27 | LEDC 50 Hz, MG996R V+ from the 6 V rail, 1000 uF cap at the servo |
| AS5600 | 21 / 22 | address 0x36 (fixed), VCC 3.3 V, DIR tied, diametric magnet on the shaft |
| Hard servo limits | -35 / +35 deg | clamped in code, no MQTT message can exceed them |

Sweeps are step-and-settle: move, wait, publish `settled:1`, hold, next step.
The Pi captures lidar frames only while settled, so MQTT latency cannot smear
the cloud. `{"action":"servo_cal"}` on `rover/lidar/command` runs the AS5600
two-point calibration. Same pack rail and common GND as the corners; own
MQTT namespace `rover/mid/*` (portal AP `Rover-Setup-MID`), so boards never
fight over motor commands.

### ESP-3 ENV (gas, flame, MLX90614, derived O2)

Third board, no motors. Firmware: `firmware/ESP32-WROOM-32-ENV/rover_env.ino`,
portal AP `Rover-Setup-ENV`, namespace `rover/env/*`.

| Input | Wiring |
|-------|--------|
| MQ-7 / MQ-6 / MQ-4 / MQ-8 | ADS1115 0x48 A0..A3, single ended, 2:1 divider each if the ADS runs at 3.3 V |
| MQ-135 (air quality) | ADS1115 0x49 A2, same divider rule |
| Flame magnitude | ADS1115 0x49 A3 |
| Flame direction | GPIO 34, 35, 36, 39, 32 (module digital outs) |
| MLX90614 left / right | 0x5A / 0x5B (re-address one via EEPROM 0xC7), 20 mm shroud tube each |

The galvanic O2 cell is **not fitted** (not purchasable here). `ADS1115 0x49`
A0/A1 are free, and the board publishes `o2_ok:0` so the backend knows no real
cell is behind the number. If a cell is fitted later, publish `o2_ok:1` plus
`o2_mv` and the backend stops estimating on its own.

Gas values are relative (`R0/Rs`, 1.0 = clean air) after
`{"action":"baseline"}` captures the clean-air reference into NVS. MQ cells
need 24-48 h of powered burn-in first; powered time accumulates as
`burn_in_s` on the heartbeat. Serial at 115200 prints raw volts every second
for the bench.

**Derived O2** (`src/o2_estimate.rs`): the frame gains `o2_pct`, `o2_est:1`,
`o2_load` and `o2_src`. The model builds a 0..1 combustion load from the peak
gas ratio (skipped until `baseline_ok:1`), the flame channels and the MLX
object temperature, then maps it onto 20.9 % down to 16.9 %:
`0.35*gas + 0.80*flame + 0.40*heat`, clamped. Attack is 2 s, release 20 s, and
a 10 s feed hole resets the filter. No evaluable input means no estimate at
all, so the card shows `--` instead of a fake clean-air number. The 20.0 /
19.5 warnings apply to it, and the verdict labels it "estimated". Gas ratios
are relative concentrations; a real fire does displace oxygen, but this number
is an indicator, never a measurement.

Live view: the ENV cards live inside the app now. `/` carries the gas
response panel (bars in the node's own green/orange/red with an OK/WARN/ALARM
chip), `/detailed` carries the gas, O2 estimate, flame, heat, node and IMU
sections with 1 min / 10 min / 1 h charts plus the Excel export (Telemetry +
Sensors + IMU sheets). Null values render `--`, never a fabricated zero, so a
cell that has no baseline yet cannot be mistaken for clean air.

No new Arduino libraries: the ADS1115, MLX90614, MPU6050 and AS5600 are all
driven by hand-rolled register code.

## Rover WiFi setup (phone portal, no USB cable, no re-flash)

The rover learns home WiFi through a phone. Do it once; it reconnects by
itself after that. While it has no WiFi it broadcasts its own temporary
network.

1. **Join `Rover-Setup`.** An open network with that name appears next to
   home networks. Connect; a "no internet" warning is expected.
2. **A setup page pops up.** Most phones open it automatically like an
   airport login page. If not, open a browser to `192.168.4.1`.
3. **Pick home WiFi from the scanned list.** The list shows networks the
   rover itself can see.
4. **Type that network password.** It is stored in the rover flash only
   and survives power loss. It never goes into git or `config.h`.
5. **Check the broker IP.** Pre-filled with the last used value. Change it
   only if the PC address changed (run `ipconfig` on the PC, use its IPv4).
6. **Tap Save.** The rover joins home WiFi with no reboot or re-flash.
   `Rover-Setup` disappears until the next time the rover cannot connect
   at boot. It never opens mid-run, so a shaky network cannot interrupt
   a calibration.

## ESP32 firmware (flash once)

Install these Arduino libraries via **Sketch → Include Library →
Manage Libraries…**:

- **OneWire** (Paul Stoffregen)
- **DallasTemperature** (Miles Burton)
- **INA226** (RobTillaart) — INA226 current/voltage
- **Adafruit ADXL345** (+ **Adafruit Unified Sensor**, its dependency)
- **PubSubClient** (knolleary)
- **ArduinoJson** (Benoit Blanchon)
- **WiFiManager** (tzapu) — phone setup portal

> Install OneWire through the Library Manager. Do not copy only
> `OneWire.h` into the sketch folder: the full library ships a `util/`
> subfolder that `OneWire.h` includes, and a header-only copy fails with
> `fatal error: util/OneWire_direct_regtype.h: No such file or directory`.

Leave the WiFi/MQTT lines in `firmware/ESP32-WROOM-32/config.h` at their
placeholder values. They are fallback defaults only; real credentials live
in flash after the phone setup above. Flash straight from this repo.

In Arduino IDE (corners):

1. File → Open → `firmware/ESP32-WROOM-32/rover_dashboard.ino`
2. Tools → Board → ESP32 Dev Module
3. Select the correct COM port
4. Click Upload
5. Open Serial Monitor (115200 baud) and power the rover: expect
   `[Portal] Trying saved WiFi...`, then `[WiFi] Connected` or the
   `Rover-Setup` portal flow above, then `[MQTT] Connected`.

Then the middles board:

1. File → Open → `firmware/ESP32-WROOM-32-MID/rover_mid.ino`
2. Same board + port steps, Upload
3. Serial Monitor (115200 baud): expect the `Rover-Setup-MID` portal on
   first boot, then `[Setup] MID ready` and `rover/mid/*` traffic.

Then the ENV node:

1. File → Open → `firmware/ESP32-WROOM-32-ENV/rover_env.ino`
2. Same board + port steps, Upload
3. Serial Monitor (115200 baud): expect the `Rover-Setup-ENV` portal on
   first boot, then a raw-volts line every second for the bench and
   `rover/env/*` traffic. The live cards are on `/detailed` (sensor
   sections) with the gas panel on the home page.

## MQTT broker

Source runs need a broker on this PC. Install once (`choco install
mosquitto` / `brew install mosquitto` / `sudo apt install mosquitto`),
then `mosquitto -v`. The packaged app skips this: it bundles its own
Mosquitto sidecar and starts it whenever nothing listens on 1883.

## Calibration mode

1. Power all 4 motors.
2. Open `http://localhost:8000/calibration/` (via `cargo launch` or the app).
3. Press **Start** (button changes to **Pause**).
4. Motors warm up for **2 minutes** (countdown shown), then data
   collection starts automatically.
5. Live sensor readings show on the motor cards.
6. Press **Pause** to stop at any time, **Download** for `.xlsx`,
   **Reset** to clear the session.

Resilience: power loss returns the ESP32 to idle, so start a fresh
session. A dropped connection shows a "Connection lost" banner; fix it
and press Start again. Samples flush to disk every 50 records, so a crash
keeps whatever was flushed.

## MQTT topics

| Topic | Direction | Payload |
|-------|-----------|---------|
| `rover/motor/{1..4}` | ESP32 → broker | `{"motor":1,"vibration":0,"vib_valid":1,"ina_ok":1,"temp_valid":1,"voltage":12.3,"current":0.65,"temp":38.2,"speed":200}` (~4Hz) |
| `rover/motor/command` | Backend → ESP32 | `{"motor":1,"speed":200}` / `{"motor":0,"speed":-150}` (0 = all, -255..255) |
| `rover/motor/status` | ESP32 → broker | `{"state":"running","speed":[0,0,200,200]}` (~2s heartbeat, also presence) |
| `rover/calibration/command` | Backend → ESP32 | `{"action":"start"}` / `{"action":"stop"}` |
| `rover/calibration/status` | ESP32 → broker | `{"state":"warmup","warmup_pct":50,"speed_pct":100,"dir":"fwd","step_remaining_s":42}` (~2s) |
| `rover/mid/{1..2}` | ESP-2 → broker | same motor shape as corners, backend remaps to motors 5-6 |
| `rover/mid/status` | ESP-2 → broker | `{"state":"running","source":"mid","speed":[0,0]}` (~2s) |
| `rover/mid/command` | Backend → ESP-2 | same motor command shape, motors 1-2 = backend 5-6 |
| `rover/imu` | ESP-2 → broker | `{"imu_ok":1,"ax":..,"ay":..,"az":..,"gz":..,"yaw":..,"pitch":..,"roll":..}` @4Hz, yaw drifts by design |
| `rover/lidar/servo` | ESP-2 → broker | `{"servo_ok":1,"angle":15.0,"angle_real":14.8,"as5600_ok":1,"cal_ok":1,"settled":1,"sweeping":1,"step":3,"steps":17}` on change + 1Hz |
| `rover/lidar/command` | Backend/Pi → ESP-2 | `{"action":"sweep"}` / `{"action":"stop"}` / `{"action":"goto","angle":15}` / `{"action":"servo_cal"}` |
| `rover/env/telemetry` | ESP-3 → broker | `{"ads_ok":1,"mlx_ok":1,"flame_ok":1,"baseline_ok":1,"o2_ok":0,"co":1.4,"lpg":1.1,"ch4":1.0,"h2":1.2,"air":1.3,"flame_lvl":0.2,"flame_dir":0,"heat_l":34.2,"heat_r":31.7,"heat_l_amb":27.5,"heat_r_amb":27.6}` @1Hz. `o2_ok:0` = no cell; the backend adds `o2_pct`, `o2_est`, `o2_load`, `o2_src` before relaying |
| `rover/env/status` | ESP-3 → broker | `{"state":"running","source":"env","burn_in_s":7200,"baseline_ok":1}` (~2s) |
| `rover/env/command` | Backend → ESP-3 | `{"action":"baseline"}` (clean-air R0 capture into NVS) |

Gas fields are relative (`R0/Rs`, 1.0 = clean air), never ppm. `flame_ok`
means the module answers, not that fire is present: fire is `flame_dir > 0`.
Nulls mean `--` in the UI, same rule as the motor sensors.

During calibration, motor data includes `"calib":1`. Invalid sensors send
`null` for their values plus a `0` validity flag; the dashboard shows `--`
or NO SIGNAL and health math ignores that sensor. Presence flips offline
after ~8s without any rover message.

## Security: localhost-only by design

The app listens on **this PC only** (`127.0.0.1`) — there is no LAN mode and
no way to enable one. No CORS headers are ever sent.

Every button that moves the rover needs the control token (stored in a
cookie automatically, or `?token=` / `Authorization: Bearer`). This is not
about remote attackers — it stops any website you visit from driving the
motors behind your back. Read-only views (telemetry, history, export) stay
open. The Setup page can display and regenerate the token at any time.

## Settings live in the app

No files or settings to edit by hand. Open the **Setup page**
(`http://localhost:8000/setup/`) to see the connection status and change the
broker address/port — it applies instantly, no restart. (The app port needs
a restart; the page says so.)

Your home WiFi is **not** entered here: power the rover on, join the
`Rover-Setup` WiFi from your phone, pick your home network once — the rover
remembers it by itself.

## Deliberate divergences from v1

2.0 is byte-identical to v1 except where v1 was defective (each pinned by a
test — nothing changed silently):

- **Own MQTT echoes ignored.** v1 treated the backend's own
  `rover/motor/command` echo as telemetry: junk history rows plus a blanked
  motor readout on every button press. 2.0 drops command topics.
- **Float speeds recorded.** v1's `int(x or 0)` handled `128.0`; a strict
  port would read it as 0 and drop all history. 2.0 coerces like Python.
- **Baselines actually load.** v1 keyed file baselines by string but looked
  them up by int, so reloaded baselines never applied (only session-set ones
  worked). 2.0 parses keys to motor ids.
- **Export temp files deleted** (v1 leaked one per export) and written to
  the system temp dir; history/custom-dir reads use the store dir (v1's
  custom dir was half-ignored).
- **Security model is new** (localhost default, token gate, no CORS) — v1
  allowed any site to drive the motors. See above.

## For developers

- Backend: `cargo run` (port `PORT` env, default `8000`), tests: `cargo test`
  (54: 22 unit + 10 API + 9 MQTT + 5 history + 4 WS + 4 golden fixtures), benches:
  `cargo bench`. CI runs fmt, clippy (`-D warnings`), and tests on
  Windows + Linux.
- Measured vs v1 (release, like-for-like): ingest 3171ns → 310ns (~10x),
  10k-row segments 66.2ms → 19.0ms (~3.5x), 1k-row export 172ms → 20.0ms
  (~8.6x). At 16 msg/s the backend costs ~5µs/s — the bottleneck was and is
  the broker round-trip plus browser paint, not the backend.
- Layout: `src/` is also a library (`src/lib.rs`, `build_router`) so
  integration tests exercise the real route table; `tests/fixtures/` holds
  golden outputs captured from the original Python modules.
- API + WebSocket protocol mirror the old Python server 1:1; dashboard pages
  live in `web/` (v1 copy minus the mobile bottom nav, which only mattered
  below the 1200px window floor).
- ESP32 firmware (C++) lives in `firmware/ESP32-WROOM-32/` — flash it from the
  Arduino IDE (WiFi code stays placeholder; the rover learns real credentials
  via its phone portal). Its MQTT contract (topics, payloads, timing) is the
  source of truth both sides follow. DS18B20 pins are `5/26/2/4` for the
  full 4-probe build; a slot set to `-1` stays disabled and reports
  `temp_valid:0`. Re-flash and bench-check after any pin change.
- A local `Inspiration/` folder (not committed) holds the design references
  the dashboard look was based on.
- History samples append to day-sharded `history/history_YYYY-MM-DD.jsonl`;
  calibration baselines persist to `calibration_baseline.json`.
- License: MIT (see `LICENSE`). Changelog: `CHANGELOG.md`.
