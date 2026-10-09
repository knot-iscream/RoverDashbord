# Changelog

## 2.0 (2026-10-09) — Rust port, hardened

Full 1:1 port of the v1 Python backend (Axum + rumqttc + rust_xlsxwriter),
plus the fixes and hardening listed below. v1 is preserved under the
`v1-python` tag. Dashboard pages are vendored in `web/` — the repo is
self-contained; the Leptos rewrite was dropped (placeholder only, may return
with a genuine reason).

**Independence**
- Dashboard HTML/CSS/JS copied into `web/`; `WEB_ROOT` defaults to it.
- Deleted `frontend-wasm/`, `web/dist/`, and the `?ui=next` switch.

**Bug fixes (each pinned by a test)**
- Own MQTT command echoes no longer corrupt telemetry/history (B1).
- Float speeds coerce like Python instead of reading as 0 (B2).
- Excel exports go to the temp dir and are deleted after streaming; filenames
  carry the pid against same-second collisions (B3).
- Python banker's rounding replicated for health/segments/export (B4).
- Unique MQTT client id per process; 15s keepalive for faster dead-peer
  detection (B5, B6). Broker disconnects clear `mqtt_connected` via the poll
  error path (verified: rumqttc v4 rejects incoming DISCONNECT, so no
  separate arm is needed) — pinned by a fake-broker test.
- Startup log line always prints (B7); buffered history writer (B8);
  deterministic motor order (B9); lock-free streaming export (B10); atomic
  config/baseline writes with visible errors (B11); graceful shutdown and a
  readable bind-conflict message (B15). Dead code and stale comments removed.

**Security**
- Localhost-only by design: binds `127.0.0.1`, no LAN mode exists.
- Permissive CORS deleted; no CORS headers are ever sent.
- 32-char token (generated once, persisted) gates every mutating endpoint
  and `/ws` via cookie / `?token=` / `Authorization: Bearer`. This stops any
  website you visit from driving the motors behind your back.
- Setup page: token display + regenerate.

**Known minor divergences (documented, not fixed)**
- `GET /api/history/segments` without `?day=` answers 400 (axum rejection);
  v1 answered 422 (FastAPI validation). Same meaning, different code.
- A WebSocket client too slow to keep up drops frames instead of being
  disconnected (v1 removed dead clients). Chosen for robustness at 4 Hz.

**Tests & tooling**
- 54 tests: 22 unit + 10 API + 9 MQTT + 5 history + 4 WS + 4 golden
  fixtures captured from the original Python modules.
- `src/lib.rs` + `build_router` so tests exercise the real route table.
- Criterion benches with measured v1-vs-2.0 speedups (10x / 3.5x / 8.6x).
- CI (fmt, clippy `-D warnings`, tests) on Windows + Linux; MIT license.
- `/api/health` reports `version` + `build` (git hash via `build.rs`).

**Desktop app (new)**
- Tauri v2 shell (`roverdash.exe`): native window (min 1200x800), backend
  in-process, splash while starting, tray icon (X minimizes, quit from the
  menu), single-instance. WebView loads the same `http://127.0.0.1:8000`.
- Portable folder `dist/RoverDash/` (built by `package.bat`, gitignored):
  exe + `web/` + bundled Mosquitto sidecar. Data lives beside the exe.
  `Rover.bat` starts the packaged app, with a `cargo run` dev fallback.
- Supervisor: bundled broker auto-starts iff 1883 is free (external broker
  wins); graceful exit stops exactly the sidecar it started.
- Vendored pages minus the mobile bottom nav and its CSS/JS (dead above
  the 1200px window floor — documented here, not silently dropped).

## v1-python — original Python backend

Preserved as-is under the `v1-python` tag (FastAPI + paho-mqtt + openpyxl,
dashboard served from the repo root). See that tag for its own history.
