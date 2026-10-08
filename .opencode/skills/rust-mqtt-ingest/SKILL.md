---
name: rust-mqtt-ingest
description: Use when porting backend/mqtt_handler.py with rumqttc async MQTT (subscribe, publish, reconnect, device presence).
---

# Rust MQTT Ingest

Ports `D:/Git/DigitalTwinDashboard/backend/mqtt_handler.py` to `rumqttc`.

## Rules
- Async client (`rumqttc::AsyncClient` + `EventLoop::poll`), auto-reconnect with backoff (replace `connect_async` + `reconnect_delay_set(1,30)`).
- Subscribe: `rover/motor/#`, `rover/calibration/status`. Publish: `rover/motor/command {motor,speed}`, `rover/calibration/command {action:start|stop}`.
- Track `connected: Arc<AtomicBool>` from `Incoming::ConnAck`/disconnect events. `publish()` returns false when not connected; check reason code.
- Route in `on_mqtt_message`: ignore topics ending `/command` for presence; `device_last_seen = now()` on any other rover msg; `rover/calibration/status` -> cal manager + broadcast; `rover/motor/status` -> motor_status cache + broadcast; else telemetry.
- Presence: `online = now - last_seen < 8s`, poll every 2s, broadcast `device_status` only on flip.
- Persist gate (with history-store skill): sample only if `speed != 0` OR cal state `warmup|sweep`.
- Verify: kill/restart Mosquitto -> `mqtt_connected` false->true with no restart; motor POST returns `sent:true` when up.
