# Rover Digital Twin Dashboard

Digital twin dashboard for a 4-wheel rover with robotic arm. Real-time motor health monitoring inspired by BeamNG damage display UI, styled with the Re:ANIME dark design language.

## Project Structure

```
├── dashboard/                  # Web dashboard pages
│   ├── index.html              # Main: Motor Health HUD + Camera + Arm
│   ├── style.css               # Re:ANIME design system (shared)
│   ├── script.js               # Navigation/scroll logic (shared)
│   ├── app.js                  # SVG HUD animation, camera toggle
│   ├── calibration/            # 12-hour baseline calibration page
│   │   ├── index.html
│   │   └── script.js
│   └── detailed/               # Per-motor graphs & exact values
│       ├── index.html
│       └── script.js
│
├── backend/
│   ├── server.py               # FastAPI + WebSocket server
│   ├── mqtt_handler.py         # MQTT subscriber (paho-mqtt)
│   ├── data_handler.py         # Health estimation engine
│   └── requirements.txt
│
├── firmware/esp32/
│   ├── rover_dashboard.ino     # Main firmware
│   ├── sensors.h               # DHT11 + INA219 + SW-420 drivers
│   ├── mqtt_comms.h            # WiFi + MQTT publish
│   └── config.h                # WiFi/MQTT credentials
│
├── assets/
├── poc/                        # Proof-of-concept (reference)
├── Inspiration/                 # Re:ANIME design reference pages
└── README.md
```

## Pages

| Route | Page | Content |
|-------|------|---------|
| `/dashboard/` | Main Dashboard | Motor Health HUD (SVG car with 4 tire=motor widgets), Camera feed (Normal/LiDAR toggle), Arm movement |
| `/dashboard/calibration/` | Calibration | 12-hour data collection, per-motor baseline, health threshold setup |
| `/dashboard/detailed/` | Detailed View | Per-motor charts (temp, voltage, current, health), exact real-time values |

## Design

- **Style reference**: Re:ANIME (`Inspiration/` directory contains saved pages)
- **Dark theme**: `#000` background, `#18181b` cards, `#f97316` accent
- **Typography**: Inter font, 400–900 weights
- **Interaction**: Glassmorphism header, inset-bordered cards, skewX buttons with shine sweep, pulse-dot status indicators

## Motor Health HUD

Each of the 4 tires on the SVG car icon represents one motor:

| Widget Element | Metric | Visual Behavior |
|----------------|--------|----------------|
| Progress ring | Health % | Stroke-dasharray, green→yellow→red |
| Fill color | Temperature | Blue→cyan→yellow→red gradient overlay |
| WiFi icon | Vibration | Pulse animation when abnormal |
| Zigzag wave | Voltage/Current | Waveform line, color = voltage range |

## Data Flow

```
ESP32 (4× DHT11, INA219, SW-420)
  │  JSON via MQTT every 250ms per motor
  ▼
MQTT Broker (mosquitto or cloud)
  │  Topic: rover/motor/{1..4}
  ▼
FastAPI Backend (mqtt_handler.py)
  │  Health estimation + WebSocket broadcast
  ▼
Browser Dashboard (JS updates SVG/charts in real-time)
```

## Getting Started

### Backend
```bash
cd backend
pip install -r requirements.txt
python server.py
```

### Firmware
1. Install ESP32 board support in Arduino IDE
2. Install libraries: DHT sensor library, Adafruit INA219, PubSubClient, ArduinoJson
3. Edit `config.h` with your WiFi/MQTT credentials
4. Upload `rover_dashboard.ino` to ESP32-WROOM

### Dashboard
Serve the `dashboard/` directory with any HTTP server:
```bash
python -m http.server 8080
```
Then open `http://localhost:8080/dashboard/`

## Hardware

- **MCU**: ESP32-WROOM (upgrade to Raspberry Pi planned)
- **Motors**: 4× 37GB 12V 100Rpm
- **Sensors per motor**: DHT11 (temp), INA219 (voltage/current), SW-420 NC (vibration)
- **Communication**: WiFi → MQTT broker

## WIP / Future

- [ ] LiDAR integration and map overlay on dashboard
- [ ] 3D arm movement visualization
- [ ] Raspberry Pi upgrade for onboard processing
- [ ] Historical data persistence and trend analysis
