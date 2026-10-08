---
name: firmware-contract-guard
description: Use when touching anything that speaks to the ESP32 (MQTT topics, payloads, timing). Firmware stays Arduino C++, never rewritten.
---

# Firmware Contract Guard

ESP32 firmware (`D:/Git/DigitalTwinDashboard/firmware/ESP32-WROOM-32/`) is READ-ONLY. Rust matches it, never the reverse.

## Contract (source of truth: config.h + mqtt_comms.h)
- Sub (backend->ESP32): `rover/calibration/command {action:start|stop}`, `rover/motor/command {motor:0-4,speed:-255..255}` (0/all).
- Pub (ESP32->backend): `rover/motor/1..4 {motor,vibration,vib_valid,ina_ok,temp_valid,voltage,current,temp,speed}` @250ms round-robin; `rover/motor/status {state:running|calibrating,speed:[4]}` @2s; `rover/calibration/status {state:idle|warmup|sweep,warmup_pct,speed_pct,dir,cycle,step_remaining_s}` @2s.
- Validity: `temp_valid` (3-strike hysteresis, only M4/GPIO4 wired), `ina_ok` (NaN or busV not in 3-16V -> 0 + nulls), `vib_valid` (HIGH seen in 10s). Nulls mean `--` in UI, never fabricated defaults.
- Timing sync: `CALIB_WARMUP_MS=120000` == Rust 120s; `SAMPLE_INTERVAL_MS=250`. GPIO 6-11 forbidden.
- Never commit WiFi creds; portal NVS `rover` namespace is the device's business.
