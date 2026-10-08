---
name: rust-telemetry-core
description: Use when porting data_handler.py health math, calibration_manager.py FSM, and history_store.py JSONL segmentation exactly.
---

# Rust Telemetry Core

Ports `data_handler.py`, `calibration_manager.py`, `history_store.py` with zero behavior drift.

## Rules
- `_num(v,default)` / `_raw(v)`: coerce `None`/`NaN`/missing -> default or `None` for display. Never compare `None > 50`.
- `compute_health`: if NOT (`ina_ok || temp_valid || vibration_valid`) -> `{health: None, anomalies: []}`. Else start 100: `temp>50: -(t-50)*2`; `voltage<11.5: -(11.5-v)*10`; `current>2.0: -(c-2)*20`; `vibration: -5`; baselines `temp>avg+10: -3`, `voltage<avg*0.9: -3`. Clamp `[0,100]`, round 1dp. Baselines in `calibration_baseline.json`.
- `CalibrationManager`: `idle|warmup|sweep` + `Mutex`; `start()` -> warmup + publish start; 120s warmup (`warmup_pct = elapsed/120*100`); auto warmup->sweep; `handle_esp32_status` adopts `speed_pct/direction/step_remaining_s/cycle`; `stop()` resets.
- `HistoryStore`: append `history/history_YYYY-MM-DD.jsonl` `{ts,iso:%Y-%m-%d %H:%M:%S,motor,speed,temp,voltage,current,vibration}`; `days()` reverse-sorted; `segments(day)` splits on motor/dir/speed change or `gap>5s`, aggregates `{start_ts,start_iso:%H:%M:%S,duration_s,speed_pct=|speed|/255*100,direction,avg/max_temp,avg_voltage,avg_current,vibration_count,samples}`.
- Verify with `rust-parity-test`: diff Python vs Rust on recorded packets.
