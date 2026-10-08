---
name: rust-parity-test
description: Use when verifying Rust output matches the original Python backend and JS behavior (health, segments, WS payloads).
---

# Rust Parity Test

Proves the 1:1 translation (AGENTS.md rule 7).

## Rules
- Unit: `cargo test` for `compute_health` (temp/voltage/current/vib/baseline edges, all-invalid -> None, clamp 0-100), warmup math (pct/remaining/auto sweep at 120s), segment splits (dir/speed change, 5s gap).
- Fixture diff: replay recorded `history_*.jsonl` + crafted MQTT packets through both Python (`backend/data_handler.py`) and Rust; `diff` JSON outputs exactly (nulls preserved).
- API diff: hit same endpoints on Python :8000 and Rust :8000, compare bodies (normalize timestamps).
- Frontend (Phase B only): side-by-side screenshots + WS log diff per page.
- Gate: no Phase B page becomes default until all diffs pass.
