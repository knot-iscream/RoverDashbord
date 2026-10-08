---
name: windows-rust-ops
description: Use when replacing Rover.bat and Doctor.bat with cargo-based launch and health checks on Windows.
---

# Windows Rust Ops

Replaces `Rover.bat` / `Doctor.bat` / `bat/` with Cargo equivalents.

## Rules
- Launch: `cargo run --release` (binds `0.0.0.0:8000`), poll `/api/health` until 200 then open browser to `http://<server-ip>:8000/`. Log transcript to `Rover.log` equivalent.
- Doctor checks (plain language): `cargo --version`, `/api/health` (`mqtt_connected`, `mqtt_broker`, `device_online`), Mosquitto `C:\Program Files\mosquitto\mosquitto.exe` reachable on 1883, `GET /`, `/calibration/`, `/detailed/`, `/test/` all 200, motor POST `sent:true`.
- Broker host/port via `MQTT_BROKER`/`MQTT_PORT` env, never hardcoded. LAN URL uses `window.location.hostname`, never `localhost`, for remote/phone access.
- Prerequisites (one-time): Rust stable MSVC + VS Build Tools C++ workload + `wasm32-unknown-unknown` target (Phase B only).
