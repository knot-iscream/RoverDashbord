//! Debug-only Rust live view: second window pushed by Rust, not JS.
//!
//! The legacy dashboard in `web/` is frozen (1:1 parity). This window is the
//! iteration surface for Rust-rendered views (lidar 3D lands here next as a
//! `wgpu` canvas). Phase 1 shows real `AppState` text at ~2Hz: device
//! presence, WS client count, motor frames. No `simulateData`, no fetch, no
//! new MQTT topic — it reads the same state `/ws` fans out.
//!
//! Only spawned under `#[cfg(debug_assertions)]` from `main.rs`, so release
//! builds (`package.bat`) keep the single main window the 5-point checklist
//! verified.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::json;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// Open (or focus) the `rust-view` window and start pushing live state into
/// it. Safe to call once at startup; a user-closed window simply ends the
/// push loop.
pub fn spawn_rust_view(app: &tauri::AppHandle, state: digital_twin_dashboard::AppState) {
    if let Some(win) = app.get_webview_window("rust-view") {
        let _ = win.show();
        let _ = win.set_focus();
        push_loop(app.clone(), state);
        return;
    }
    let built =
        WebviewWindowBuilder::new(app, "rust-view", WebviewUrl::App("rust-view.html".into()))
            .title("Rover Rust View (live, real data)")
            .inner_size(760.0, 540.0)
            .min_inner_size(480.0, 360.0)
            .visible(true)
            .build();
    match built {
        Ok(_) => {
            tracing::info!("rust-view window open (debug only)");
            push_loop(app.clone(), state);
        }
        Err(e) => tracing::warn!("rust-view window could not open: {e}"),
    }
}

fn push_loop(app: tauri::AppHandle, state: digital_twin_dashboard::AppState) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        let mut frames: u64 = 0;
        loop {
            tick.tick().await;
            frames += 1;
            let Some(win) = app.get_webview_window("rust-view") else {
                break;
            };
            let latest = state.data.read().await.get_latest();
            let device = state.device.read().await.clone();
            let clients = state.clients.load(Ordering::SeqCst);
            let sample = latest.first().cloned().unwrap_or(serde_json::Value::Null);
            let payload = json!({
                "type": "rust-telemetry",
                "frames": frames,
                "online": device.online,
                "last_seen": device.last_seen.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                "clients": clients,
                "motors": latest.len(),
                "sample": sample,
                "ts": chrono::Utc::now().format("%H:%M:%S").to_string(),
            });
            if win.emit("rust-telemetry", payload).is_err() {
                break;
            }
            let title = format!(
                "Rover Rust View #{} — {} ({} client{})",
                frames,
                if device.online { "ONLINE" } else { "OFFLINE" },
                clients,
                if clients == 1 { "" } else { "s" },
            );
            let _ = win.set_title(&title);
        }
        tracing::info!("rust-view push loop ended (window closed)");
    });
}
