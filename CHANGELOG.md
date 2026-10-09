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
  detection (B5, B6).
- Startup log line always prints (B7); buffered history writer (B8);
  deterministic motor order (B9); lock-free streaming export (B10); atomic
  config/baseline writes with visible errors (B11); graceful shutdown and a
  readable bind-conflict message (B15). Dead code and stale comments removed.

**Security**
- Binds `127.0.0.1` by default; LAN mode is opt-in (`ROVER_LAN=1` or Setup
  page, needs restart).
- Permissive CORS deleted; no CORS headers are ever sent.
- 32-char token (generated once, persisted) gates every mutating endpoint
  and `/ws` via cookie / `?token=` / `Authorization: Bearer`.
- Setup page: LAN toggle, token display + regenerate, QR join code.

**Tests & tooling**
- 52 tests: 21 unit + 10 API + 8 MQTT + 5 history + 4 WS + 4 golden
  fixtures captured from the original Python modules.
- `src/lib.rs` + `build_router` so tests exercise the real route table.
- Criterion benches with measured v1-vs-2.0 speedups (10x / 3.5x / 8.6x).
- CI (fmt, clippy `-D warnings`, tests) on Windows + Linux; MIT license.
- `/api/health` reports `version` + `build` (git hash via `build.rs`).

## v1-python — original Python backend

Preserved as-is under the `v1-python` tag (FastAPI + paho-mqtt + openpyxl,
dashboard served from the repo root). See that tag for its own history.
