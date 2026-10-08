# Rover Digital Twin 2.0

Rust backend + the same dashboard. This is a 1:1 replacement of the old
Python backend: every page, button, and live reading works exactly as before,
served from one app on one port.

> The previous Python version is preserved on GitHub under the
> [`v1-python`](https://github.com/knot-iscream/RoverDashbord/releases/tag/v1-python)
> tag, including the ESP32 firmware sources.

## Start here (Windows)

1. Install [Rust stable](https://rustup.rs/) once (and accept the
   Visual Studio Build Tools install it suggests).
2. Install [Mosquitto](https://mosquitto.org/download/) on this PC once
   (the message broker between rover and app).
3. Double-click **`Rover.bat`** — it starts the broker if needed, starts the
   app, and opens the dashboard by itself: `http://localhost:8000/`
   (other PCs/phones on the same WiFi: `http://YOUR-PC-IP:8000/`).

Something wrong? Double-click **`Doctor.bat`** — it checks everything and
tells you what to do in plain language.

## Settings live in the app

No files or settings to edit by hand. Open the **Setup page**
(`http://localhost:8000/setup/`) to see the connection status and change the
broker address/port — it applies instantly, no restart.

Your home WiFi is **not** entered here: power the rover on, join the
`Rover-Setup` WiFi from your phone, pick your home network once — the rover
remembers it by itself.

## For developers

- Backend: `cargo run` (port `PORT` env, default `8000`), tests: `cargo test`.
- API + WebSocket protocol mirror the old Python server 1:1; dashboard pages
  are served untouched (Phase A). Opt-in Leptos rebuild lives behind
  `?ui=next` per page (Phase B) — build it with `trunk build` (install
  [trunk](https://trunkrs.dev/) once).
- ESP32 firmware stays C++ and is out of scope here; its MQTT contract
  (topics, payloads, timing) is the source of truth both sides follow.
- History samples append to day-sharded `history/history_YYYY-MM-DD.jsonl`;
  calibration baselines persist to `calibration_baseline.json`.
