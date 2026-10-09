# Rover Digital Twin 2.0

Rust backend + the same dashboard. This is a 1:1 replacement of the old
Python backend: every page, button, and live reading works exactly as before,
served from one app on one port. The dashboard pages live in `web/` in this
repo — no other checkout needed.

> The previous Python version is preserved on GitHub under the
> [`v1-python`](https://github.com/knot-iscream/RoverDashbord/releases/tag/v1-python)
> tag, including the ESP32 firmware sources.

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

Something wrong? Double-click **`Doctor.bat`** — it checks everything and
tells you what to do in plain language.

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
  live in `web/` (v1 copy minus the mobile bottom nav — see CHANGELOG).
- ESP32 firmware (C++) lives in `firmware/ESP32-WROOM-32/` — flash it from the
  Arduino IDE (WiFi code stays placeholder; the rover learns real credentials
  via its phone portal). Its MQTT contract (topics, payloads, timing) is the
  source of truth both sides follow.
- `Inspiration/` holds the design references the dashboard look is based on.
- History samples append to day-sharded `history/history_YYYY-MM-DD.jsonl`;
  calibration baselines persist to `calibration_baseline.json`.
- License: MIT (see `LICENSE`). Changelog: `CHANGELOG.md`.
