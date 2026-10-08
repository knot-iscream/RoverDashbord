---
name: rust-axum-backend
description: Use when building the Axum REST + WebSocket server that ports backend/server.py 1:1 (routes, /ws protocol, static fallback).
---

# Rust Axum Backend

Ports `D:/Git/DigitalTwinDashboard/backend/server.py` to Axum + Tokio.

## Rules
- Single port `0.0.0.0:8000`. Mount `/api/*` + `/ws` first, static fallback (`ServeDir` on original repo root or copied `web/`) LAST with `html=True` equivalent (serve `index.html`).
- Routes (exact JSON shapes as Python): `GET /api/health`, `GET /api/device/status`, `GET /api/motors`, `POST /api/motor/control {motor,speed}`, `GET /api/motor/status`, `GET /api/calibration`, `POST /api/calibration/baseline/{id}`, `POST /api/calibration/start|stop`, `GET /api/calibration/status`, `GET /api/calibration/export`, `GET /api/history/days`, `GET /api/history/segments?day=`.
- WS `/ws`: on connect send `snapshot` + `calibration_status` + `device_status`; live `motor_update` (nulls preserved for `--`), `motor_status`, `calibration_status`, `device_status` on flip only; `ping` -> `pong`. Broadcast via `tokio::sync::broadcast` or fan-out to `axum::extract::ws::WebSocket`.
- Shared state: `Arc<RwLock<DataHandler|CalibrationManager|HistoryStore|DeviceState>>`. CORS `*` (tower-http). Config via `MQTT_BROKER`/`MQTT_PORT` env (default `localhost:1883`).
- Clamp motor speed `[-255,255]` server-side. `/api/motor/control` returns `{status:ok,sent:true}` or `error` with `mqtt_connected` flag.
- Verify: `cargo check`, then `curl /api/health`, `/`, `/calibration/` all 200.
