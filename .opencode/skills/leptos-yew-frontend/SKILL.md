---
name: leptos-yew-frontend
description: Use ONLY when rewriting the 4 dashboard pages to Leptos/Yew WASM (Phase B). For 1:1 vanilla serve see rust-axum-backend.
---

# Leptos/Yew Frontend (Phase B, opt-in)

Rewrites `/`, `/calibration/`, `/detailed/`, `/test/` from vanilla JS. Fallback is always vanilla served by Axum.

## Rules
- Order: `detailed/` (smallest live page) -> `test/` -> `/` -> `calibration/` (largest, 954L) last. Feature-flag each page; vanilla stays default until parity passes.
- Preserve: `window.ROVER` URL logic (same-origin `:8000`, `file:`->localhost), WS `/ws` handling (`motor_update`, `calibration_status`, `device_status`, `snapshot` ignored where the original ignores it), 3s reconnect, 80ms slider debounce, 2-3s REST polls, `--`/NO SIGNAL/stale gates on `temp_valid`/`ina_ok`/`vibration_valid`, Canvas2D ranges `[20,80],[10,14],[0,3],[0,100]` maxPoints 60, `style.css` theme intact.
- Do NOT invent state: port `AUTO/MANUAL`, cal-guard locks, ramp sequences `[64,128,191,255,-255,-191,-128,-64,0]`, history expand persistence.
- Verify per page: side-by-side screenshot vs vanilla + WS message log diff. Roll back to vanilla on any fidelity drop.
