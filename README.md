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
`package.bat` (needs a Rust toolchain, slow first build). Splash, native
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

Prefer the old way, or developing? Double-click **`Rover.bat`** — it starts
the packaged app if present, else runs the backend from source and opens
your browser. Something wrong? **`Doctor.bat`** checks everything and tells
you what to do in plain language.

## Start here, from source (developers)

1. Install [Rust stable](https://rustup.rs/) once (and accept the
   Visual Studio Build Tools install it suggests).
2. Install [Mosquitto](https://mosquitto.org/download/) on this PC once
   (the message broker between rover and app) — only needed for
   source runs; the packaged app bundles its own.
3. `cargo run` serves the dashboard at `http://localhost:8000/`

Iterate natively (debug, real data): double-click **`Rover-dev.bat`**
(or `cargo run -p roverdash`, or `bacon` for auto-rebuild). Opens the main
window plus a second **Rover Rust View** window pushed live by Rust at ~2Hz
(device presence, WS clients, motor frames — no simulation). `web/` legacy
pages are frozen; future Rust views (lidar 3D as a `wgpu` canvas) land in
the Rust view. Release (`package.bat`) stays single-window.

Something wrong? Double-click **`Doctor.bat`** — it checks everything and
tells you what to do in plain language.

## Prerequisites (full build)

| Item | Detail |
|------|--------|
| PC/laptop | Runs backend + broker + dashboard |
| ESP32-WROOM-32 dev board + USB cable | Firmware target |
| 4x DS18B20 waterproof probes + 4x 4.7kΩ resistors | One probe per motor, 3.3V, pull-up per pin |
| 4x SW-420 vibration modules | 3.3V only, sensitivity pot per module |
| 4x INA219 current/voltage modules | Unique I2C address per module via A0/A1 jumpers |
| 2x L298N H-bridge boards | One board per two motors |
| 4x 37GB 12V 100RPM gear motors + 12V supply | Load side |
| Arduino IDE + ESP32 core | Firmware flash (libraries listed below) |
| Rust stable + Mosquitto | Backend + broker for source runs |

## Hardware wiring

Full build: all four motors instrumented. Pin map is authoritative in
`firmware/ESP32-WROOM-32/config.h`. Do not reuse GPIO 6-11 (tied to
internal SPI flash). GPIO 1/3 are UART0. GPIO 34-39 are input-only.

### Pinout table

| Motor | SW-420 (DO) | INA219 (addr) | DS18B20 (OneWire) | L298N channel |
|-------|-------------|---------------|-------------------|---------------|
| 1 (FL) | GPIO 34 | 0x41 | GPIO 5 | L298N #1 A (ENA=13, IN1=15, IN2=14) |
| 2 (FR) | GPIO 35 | 0x44 | GPIO 26 | L298N #1 B (ENB=18, IN3=19, IN4=23) |
| 3 (RL) | GPIO 36 | 0x45 | GPIO 2 | L298N #2 A (ENA=32, IN1=33, IN2=27) |
| 4 (RR) | GPIO 39 | 0x40 | GPIO 4 | L298N #2 B (ENB=25, IN3=17, IN4=16) |

- Motor PWM is 8-bit (0-255) at 1kHz. Speed command range is -255..255
  (negative = reverse, 0 = active brake with both IN pins LOW).
- SW-420 uses input-only GPIOs 34/35/36/39. A reading counts as valid
  only if the pin was seen HIGH within the last 10s; a floating or
  disconnected pin reads LOW forever and shows as NO SIGNAL, never as
  vibration.
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

### I2C bus (INA219, all 4 share this)

| I2C pin | ESP32 GPIO |
|---------|-----------|
| SDA | GPIO 21 |
| SCL | GPIO 22 |

All 4 INA219 modules connect to the same SDA/SCL lines in parallel.
Each must have a **unique I2C address** set via its A0/A1 solder jumpers:

| Address | A0 jumper | A1 jumper | Motor |
|---------|-----------|-----------|-------|
| 0x40 | GND | GND | 4 (RR) |
| 0x41 | VCC | GND | 1 (FL) |
| 0x44 | GND | VCC | 2 (FR) |
| 0x45 | VCC | VCC | 3 (RL) |

Presence is re-checked every 5s. A chip that answers but returns NaN, or
a bus voltage outside 3-16V, counts as `ina_ok:false` and voltage/current
show as `--`.

### L298N wiring (motor drivers)

Two L298N boards drive the four motors (one board per two motors):

- **12V motor supply** goes to L298N `VS` (both boards share it).
- **Common ground is critical**: tie ESP32 GND, both L298N `GND`,
  all INA219 GND, and the 12V supply GND together. Without it the logic
  inputs float and motors misbehave.
- L298N `ENA`/`ENB` and `IN1`-`IN4` go to the ESP32 GPIOs in the pinout
  table. The inputs are high-impedance, so 3.3V ESP32 logic drives them.
- The L298N onboard 5V regulator can power its own logic, but **do not**
  back-feed it into the ESP32 5V pin while the ESP32 is powered.

### INA219 current sensing (per motor)

Place each INA219 **in series with one motor lead, between the L298N
output and the motor terminal**, so it measures the actual load current.
The 12V supply goes to L298N `VS`, not through the INA219.

- Never wire VIN+/VIN- across the motor in parallel. That puts 12V
  across the 0.1Ω shunt and burns it open (seen on the bench: chip alive,
  ~7V coupled through, ~0A forever).
- Stock INA219 tops out at ±3.2A. Reverse drive reads negative current;
  that is normal and not a fault.

### Power connections

- **DS18B20**: VCC to 3.3V, GND to GND, DATA to its motor pin with a
  4.7kΩ pull-up to 3.3V. Tape the probe to the motor casing with thermal
  paste.
- **SW-420**: VCC to 3.3V (never 5V), GND to GND, DO to its motor GPIO.
  Set the sensitivity pot per module; the module rests HIGH when quiet.
- **INA219**: VCC to 3.3V, GND to GND, SDA/SCL to 21/22.

### Wiring order

1. Wire all INA219 modules to the I2C bus first (SDA/SCL + addresses).
2. Connect each DS18B20 DATA pin with its own 4.7kΩ pull-up.
3. Connect each SW-420 DO pin to its GPIO.
4. Wire both L298N boards (VS 12V, IN/ENA to ESP32) and the common GND.
5. Run each motor lead through its INA219 in series, then to the L298N output.

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
- **Adafruit INA219** (+ **Adafruit BusIO**, its dependency)
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

In Arduino IDE:

1. File → Open → `firmware/ESP32-WROOM-32/rover_dashboard.ino`
2. Tools → Board → ESP32 Dev Module
3. Select the correct COM port
4. Click Upload
5. Open Serial Monitor (115200 baud) and power the rover: expect
   `[Portal] Trying saved WiFi...`, then `[WiFi] Connected` or the
   `Rover-Setup` portal flow above, then `[MQTT] Connected`.

## MQTT broker

Source runs need a broker on this PC. Install once (`choco install
mosquitto` / `brew install mosquitto` / `sudo apt install mosquitto`),
then `mosquitto -v`. The packaged app skips this: it bundles its own
Mosquitto sidecar and starts it whenever nothing listens on 1883.

## Calibration mode

1. Power all 4 motors.
2. Open `http://localhost:8000/calibration/` (via `Rover.bat` or the app).
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
