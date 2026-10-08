---
name: rust-excel-export
description: Use when porting history_store.py Excel export to rust_xlsxwriter (.xlsx Telemetry sheet).
---

# Rust Excel Export

Ports `history_store.py:export_excel` to `rust_xlsxwriter`.

## Rules
- Output `history_export_{epoch}.xlsx`; sheet `Telemetry`; header exactly `[motor_id,epoch,timestamp,speed,speed_pct,direction,temp,voltage,current,vibration]`.
- Include all days in chronological order. `speed_pct = |speed|/255*100`, `direction = FWD>0/REV<0/IDLE`.
- Empty store -> HTTP 400 `{error: "No recorded data"}` (no file created).
- Verify: export from real `.jsonl`, open in Excel/LibreOffice, compare row count vs `segments` samples.
