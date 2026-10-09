# Rover Digital Twin 2.0

Rust backend + the same dashboard. This is a 1:1 replacement of the old
Python backend: every page, button, and live reading works exactly as before,
served from one app on one port. The dashboard pages live in `web/` in this
repo — no other checkout needed.

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

Something wrong? Double-click **`Doctor.bat`** — it checks everything and
tells you what to do in plain language.

## Phone / LAN access (opt-in)

By default the app listens on **this PC only** (`127.0.0.1`). To open the
dashboard on your phone or another PC:

1. Open the **Setup page** (`http://localhost:8000/setup/`), tick
   *Allow dashboard on the local network*, save, and **restart the app**.
2. Reopen Setup, scan the **QR code** with your phone — it joins with the
   control token baked in, no typing.

Every button that moves the rover needs the token (cookie, `?token=`, or
`Authorization: Bearer`). Read-only views (telemetry, history, export) stay
open. The Setup page can display and regenerate the token at any time.

## Settings live in the app

No files or settings to edit by hand. Open the **Setup page**
(`http://localhost:8000/setup/`) to see the connection status and change the
broker address/port — it applies instantly, no restart. (The app port and
LAN mode need a restart; the page says so.)

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
  (52: unit + API + WS + MQTT + history + golden fixtures), benches:
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
  are served untouched from `web/`.
- ESP32 firmware (C++) lives in `firmware/ESP32-WROOM-32/` — flash it from the
  Arduino IDE (WiFi code stays placeholder; the rover learns real credentials
  via its phone portal). Its MQTT contract (topics, payloads, timing) is the
  source of truth both sides follow.
- `Inspiration/` holds the design references the dashboard look is based on.
- History samples append to day-sharded `history/history_YYYY-MM-DD.jsonl`;
  calibration baselines persist to `calibration_baseline.json`.
- License: MIT (see `LICENSE`). Changelog: `CHANGELOG.md`.
