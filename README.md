# Rover Digital Twin Dashboard

Digital twin dashboard for a 4-wheeled rover with sensor-based motor health monitoring.

## Architecture

```
[ESP32] --MQTT--> [Python Backend] --WebSocket--> [Dashboard (HTML/JS)]
                      |
                   [Calibration Manager] --saves--> [.jsonl + .xlsx]
```

### Components

| Layer | Tech | Location |
|-------|------|----------|
| Firmware | Arduino (ESP32-WROOM) | `firmware/esp32/` |
| Backend | FastAPI + Paho MQTT | `backend/` |
| Dashboard | Vanilla HTML/CSS/JS | `dashboard/` |

---

## Prerequisites

Before starting, make sure you have:

- A **PC/laptop** (runs the backend + MQTT broker + dashboard)
- **ESP32-WROOM** dev board with USB cable
- **4x DS18B20** waterproof temperature probes (all on one OneWire bus)
- **4x SW-420** vibration sensors (adjust sensitivity pot per module)
- **4x INA219** voltage/current sensors (with I2C address jumpers)
- **2x L298N** H-Bridge dual motor drivers
- **4x 37GB 12V 100Rpm motors** with a 12V power supply
- **MQTT broker** (install Mosquitto on your PC)
- **Python 3.10+** installed on your PC
- **Arduino IDE** with ESP32 board support installed

---

## Hardware Wiring

### Pinout Table

| Motor | SW-420 (DO pin) | INA219 (I2C addr) | L298N channel |
|-------|-----------------|-------------------|---------------|
| 1 (FL) | GPIO 34 | 0x41 | L298N #1 A (ENA=13, IN1=15, IN2=14) |
| 2 (FR) | GPIO 35 | 0x44 | L298N #1 B (ENB=18, IN3=19, IN4=23) |
| 3 (RL) | GPIO 36 | 0x45 | L298N #2 A (ENA=32, IN1=33, IN2=27) |
| 4 (RR) | GPIO 39 | 0x40 | L298N #2 B (ENB=16, IN3=17, IN4=25) |

- **DS18B20** all four probes share **GPIO 4** (OneWire bus, 4.7kΩ pull-up)
- Motor PWM channel speed is 8-bit (0-255), 1kHz
- SW-420 uses input-only GPIOs 34/35/36/39

**IMPORTANT:** GPIO 6-11 are connected to internal SPI flash on ESP32-WROOM
and CANNOT be used as regular I/O. They are NOT used here. GPIO 1/3 are UART0.

### I2C Bus (INA219 — all 4 share this)

| I2C Pin | ESP32 GPIO |
|---------|-----------|
| SDA | GPIO 21 |
| SCL | GPIO 22 |

All 4 INA219 modules connect to the same SDA/SCL lines in parallel.
Each must have a **unique I2C address** set via its A0/A1 solder jumpers:

| Address | A0 jumper | A1 jumper | Motor       |
|---------|-----------|-----------|-------------|
| 0x40 | GND | GND | 4 (RR) |
| 0x41 | VCC | GND | 1 (FL) |
| 0x44 | GND | VCC | 2 (FR) |
| 0x45 | VCC | VCC | 3 (RL) |

### L298N Wiring (motor driver)

Two L298N boards drive the four motors (one board per two motors):

- **12V motor supply** → L298N `VS` (both boards share it)
- **Common ground**: tie ESP32 GND, L298N `GND`, INA219 GND, and the 12V
  supply GND together (critical!).
- L298N `ENA`/`ENB` and `IN1`-`IN4` → ESP32 GPIOs listed above.
  The L298N logic inputs are high-impedance, so 3.3V ESP32 logic works.
- L298N onboard 5V regulator can power the logic, but **do not** back-feed it
  into the ESP32 5V pin unless the ESP32 is unpowered.

### INA219 current sensing (per motor)

Place the INA219 **in series with the motor lead between the L298N output and
the motor terminal** so it measures the actual load current. The 12V supply
goes to L298N `VS`, not through the INA219.

### Power Connections

- **DS18B20** VCC → 3.3V, GND → GND, DATA → GPIO 4 (**4.7kΩ pull-up to 3.3V**).
  Probe taped to the motor casing with thermal paste for accurate case temp.
- **SW-420** VCC → 3.3V, GND → GND, DO → GPIO 34/35/36/39
- **INA219** VCC → 3.3V, GND → GND (SDA/SCL → 21/22)

### Wiring Order

1. Wire all INA219 modules to the I2C bus first (SDA/SCL)
2. Connect the one DS18B20 OneWire bus to GPIO 4 (+ pull-up)
3. Connect each SW-420 DO pin to its GPIO
4. Wire both L298N boards (VS 12V, IN/ENA to ESP32) and common GND
5. Run each motor lead through its INA219 then to the L298N output

---

## Setup Guide

### Step 1: Install MQTT Broker

**Windows** (using Chocolatey):
```powershell
choco install mosquitto
```

**macOS:**
```bash
brew install mosquitto
```

**Linux:**
```bash
sudo apt install mosquitto mosquitto-clients
```

Start it:
```bash
mosquitto -v
```

Note the **IP address** of this machine (run `ipconfig` / `ifconfig`).
You'll need it in the next steps.

### Step 2: Configure & Upload ESP32

First install these Arduino libraries via **Sketch → Include Library →
Manage Libraries…**:

- **OneWire** (Paul Stoffregen)
- **DallasTemperature** (Miles Burton)
- **Adafruit INA219** (+ **Adafruit BusIO**, its dependency)
- **PubSubClient** (knolleary)
- **ArduinoJson** (Benoit Blanchon)

> **Note:** Install OneWire through the Library Manager — **do not** copy only
> `OneWire.h` into the sketch folder. The full library ships a `util/`
> subfolder (`OneWire_direct_regtype.h`, `OneWire_direct_gpio.h`) that
> `OneWire.h` includes; copying just the header causes a
> `fatal error: util/OneWire_direct_regtype.h: No such file or directory`.

Open `firmware/esp32/config.h` and edit these lines:

```c
// WiFi — set your home/network WiFi
#define WIFI_SSID "YourActualWiFiName"       // ← CHANGE THIS
#define WIFI_PASSWORD "YourActualPassword"    // ← CHANGE THIS

// MQTT — set to the IP of the machine running Mosquitto
#define MQTT_BROKER "192.168.1.50"            // ← CHANGE THIS to your PC's IP
```

Save the file, then in Arduino IDE:
1. File → Open → select `firmware/esp32/rover_dashboard.ino`
2. Tools → Board → ESP32 Dev Module
3. Select the correct COM port
4. Click Upload

### Step 3: Start the Python Backend

On the same PC as the MQTT broker:

```bash
cd backend
pip install -r requirements.txt
python server.py
```

Expected output:
```
[Server] Starting Rover Digital Twin Backend...
[MQTT] Client started on localhost:1883
[MQTT] Connected to broker at localhost:1883 (rc=0)
[MQTT] Subscribed to rover/motor/#
[MQTT] Subscribed to rover/calibration/status
[WS] WebSocket server ready on 0.0.0.0:8000
```

**If your MQTT broker is on a different machine**, edit `backend/server.py`
line ~97 and change `broker="localhost"` to your broker's IP.

### Step 4: Open the Dashboard

Open `dashboard/calibration/index.html` in a browser.

**If the backend is on a different machine**, edit
`dashboard/calibration/script.js` lines 5-6:

```js
var WS_URL = 'ws://192.168.1.50:8000/ws';    // ← your backend IP
var API_BASE = 'http://192.168.1.50:8000';    // ← your backend IP
```

### Step 5: Verify It Works

1. Power on the ESP32
2. Open the Serial Monitor (115200 baud)
3. You should see:
   ```
   [WiFi] Connected. IP: 192.168.x.x
   [MQTT] Connected
   [MQTT] Published to rover/motor/1: {"motor":1,...}
   ```
4. In the dashboard, the status should show "Connected to server"
5. You're ready to calibrate

---

## Calibration Mode

1. Make sure all 4 motors are powered and running
2. Open `dashboard/calibration/` in a browser
3. Press **Start** — button changes to **Pause**
4. Motors warm up for **2 minutes** (countdown displayed)
5. Automatically transitions to **data collection**
6. Live sensor readings show on motor cards
7. Press **Pause** to stop recording at any time
8. Press **Download** to export all collected data as `.xlsx`
9. Press **Reset** to clear session and start over

### Resilience

- **Power loss**: ESP32 reboots to idle. Start a fresh session.
- **Internet loss**: Dashboard shows "Connection lost" banner.
  Fix the connection and press Start again.
- **Data safety**: Samples are flushed to disk every 50 records.
  If the backend crashes mid-session, whatever was flushed is
  available for download.

---

## MQTT Topics

| Topic | Direction | Payload |
|-------|-----------|---------|
| `rover/motor/{1..4}` | ESP32 → Broker | `{"motor":1, "vibration":0, "voltage":12.3, "current":0.65, "temp":38.2}` |
| `rover/motor/command` | Broker → ESP32 | `{"motor":1,"speed":200}` / `{"motor":0,"speed":-150}` (0 = all, -255..255) |
| `rover/motor/status` | ESP32 → Broker | `{"state":"running","speed":[0,0,200,200]}` |
| `rover/calibration/command` | Broker → ESP32 | `{"action":"start"}` / `{"action":"stop"}` |
| `rover/calibration/status` | ESP32 → Broker | `{"state":"warmup","warmup_pct":50}` |

During calibration, motor data includes `"calib":1` to flag recordings.

---

## File Structure

```
firmware/esp32/
  config.h            — WiFi, MQTT, pin assignments
  mqtt_comms.h        — MQTT communication (publish, subscribe, callbacks)
  sensors.h           — DS18B20, INA219, SW-420 sensor abstraction
  motor_driver.h      — 2x L298N motor driver control (PWM + direction)
  rover_dashboard.ino — Main loop + calibration state machine

backend/
  server.py               — FastAPI WebSocket server + REST endpoints
  mqtt_handler.py         — MQTT client (connect, subscribe, publish)
  data_handler.py         — Real-time health computation
  calibration_manager.py  — Calibration sessions, disk flush, Excel export
  calibration_baseline.json — Saved baselines (auto-generated)
  calibration_session.jsonl  — Temp sample buffer (auto-generated)
  requirements.txt        — Python dependencies

dashboard/
  style.css              — Global styles (reanime.to inspired)
  script.js              — Global scripts (dock, navigation)
  index.html             — Main HUD page
  calibration/
    index.html           — Calibration page UI
    script.js            — Calibration logic (WebSocket, toggle, warmup, export)
  detailed/
    index.html           — Detailed telemetry page
    script.js            — Detailed view logic
```
