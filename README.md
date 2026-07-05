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
- **4x DHT11** temperature sensors
- **4x SW-420** vibration sensors (adjust sensitivity pot per module)
- **4x INA219** voltage/current sensors (with I2C address jumpers)
- **4x 37GB 12V 100Rpm motors** with power supply
- **MQTT broker** (install Mosquitto on your PC)
- **Python 3.10+** installed on your PC
- **Arduino IDE** with ESP32 board support installed

---

## Hardware Wiring

### Pinout Table

| Motor | DHT11 (Data pin) | SW-420 (DO pin) | INA219 (I2C addr) |
|-------|------------------|-----------------|-------------------|
| 1 (FL) | GPIO 4 | GPIO 14 | 0x41 |
| 2 (FR) | GPIO 5 | GPIO 27 | 0x44 |
| 3 (RL) | GPIO 16 | GPIO 26 | 0x45 |
| 4 (RR) | GPIO 17 | GPIO 25 | 0x40 |

**IMPORTANT:** GPIO 6-11 are connected to internal SPI flash on ESP32-WROOM
and CANNOT be used as regular I/O. They are NOT used here.

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

### Power Connections

- **DHT11** VCC → 3.3V, GND → GND (optional 4.7k-10k pull-up on data pin)
- **SW-420** VCC → 3.3V, GND → GND, DO → GPIO
- **INA219** VCC → 3.3V, GND → GND
  - IN+/IN- in series with motor power wire (load passes through)
  - VIN+ / VIN- across the battery/motor supply

### Wiring Order

1. Wire all INA219 modules to the I2C bus first (SDA/SCL)
2. Connect each DHT11 data pin to its GPIO (with pull-up if needed)
3. Connect each SW-420 DO pin to its GPIO
4. Power all sensors from ESP32 3.3V and GND
5. Run motor power wires through INA219 terminals

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
4. Motors warm up for **15 minutes** (countdown displayed)
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
| `rover/calibration/command` | Broker → ESP32 | `{"action":"start"}` / `{"action":"stop"}` |
| `rover/calibration/status` | ESP32 → Broker | `{"state":"warmup","warmup_pct":50}` |

During calibration, motor data includes `"calib":1` to flag recordings.

---

## File Structure

```
firmware/esp32/
  config.h            — WiFi, MQTT, pin assignments
  mqtt_comms.h        — MQTT communication (publish, subscribe, callbacks)
  sensors.h           — DHT11, INA219, SW-420 sensor abstraction
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
