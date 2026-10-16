//! Shared backend-wire logic: same-origin URL resolution, WS reconnect,
//! REST helpers. Mirrors `window.ROVER` logic from the vanilla pages so the
//! Rust UI talks to the exact same Axum routes.

use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// Resolve the API base. Priority: `window.ROVER.apiBase`, else same-origin
/// (when served from Axum on :8000), `:8080 -> :8000` legacy, `file:` ->
/// `http://localhost:8000`. Never hardcodes a LAN IP.
pub fn api_base() -> String {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return "http://localhost:8000".to_string(),
    };
    // window.ROVER.apiBase when the backend injects it.
    if let Ok(rover) = js_sys::Reflect::get(&window, &JsValue::from_str("ROVER")) {
        if !rover.is_undefined() {
            if let Ok(base) = js_sys::Reflect::get(&rover, &JsValue::from_str("apiBase")) {
                if let Some(s) = base.as_string() {
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
        }
    }
    let location = window.location();
    let protocol = location.protocol().unwrap_or_default();
    if protocol == "file:" {
        return "http://localhost:8000".to_string();
    }
    if let (Ok(host), Ok(port)) = (location.hostname(), location.port()) {
        if !host.is_empty() {
            // Legacy dev loop ran the page on :8080 against backend :8000.
            let p = if port == "8080" {
                "8000".to_string()
            } else {
                port
            };
            if p.is_empty() {
                return format!("http://{host}:8000");
            }
            return format!("http://{host}:{p}");
        }
    }
    "http://localhost:8000".to_string()
}

/// WS URL derived from the API base: `http` -> `ws`, path `/ws`.
/// The auth cookie (`rover_token`) rides the upgrade automatically
/// same-origin; no query token needed.
pub fn ws_url() -> String {
    let base = api_base();
    let ws = base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{ws}/ws")
}

/// `?motor=N` deep-link (1-6), else `default`.
pub fn motor_param(default: u8) -> u8 {
    let Some(window) = web_sys::window() else {
        return default;
    };
    let search = window.location().search().unwrap_or_default();
    if search.is_empty() {
        return default;
    }
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok();
    if let Some(p) = params {
        if let Some(v) = p.get("motor") {
            if let Ok(n) = v.parse::<u8>() {
                if (1..=6).contains(&n) {
                    return n;
                }
            }
        }
    }
    default
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct MotorSample {
    #[serde(default)]
    pub temp: Option<f64>,
    #[serde(default)]
    pub voltage: Option<f64>,
    #[serde(default)]
    pub current: Option<f64>,
    #[serde(default)]
    pub health: Option<f64>,
    #[serde(default = "default_true")]
    pub temp_valid: bool,
    #[serde(default = "default_true")]
    pub ina_ok: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceStatus {
    pub online: bool,
}

/// Fetch `GET {api}/api/device/status` -> `online`. Errors mean offline
/// (same as vanilla: non-200 / exception -> offline badge).
pub async fn fetch_device_online() -> bool {
    let url = format!("{}/api/device/status", api_base());
    let resp = gloo_net::http::Request::get(&url).send().await;
    match resp {
        Ok(r) if r.ok() => r
            .json::<DeviceStatus>()
            .await
            .map(|d| d.online)
            .unwrap_or(false),
        _ => false,
    }
}

/// `performance.now()` ms — vanilla freshness clock (monotonic, not wall).
pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// GET a JSON API path, `None` on any failure (vanilla ignores errors).
pub async fn get_json(path: &str) -> Option<serde_json::Value> {
    let url = format!("{}{path}", api_base());
    let resp = gloo_net::http::Request::get(&url).send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.json::<serde_json::Value>().await.ok()
}

/// POST a JSON API path, parsed response or `None` (vanilla ignores errors).
pub async fn post_json(path: &str, body: serde_json::Value) -> Option<serde_json::Value> {
    let url = format!("{}{path}", api_base());
    let req = gloo_net::http::Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .ok()?;
    let resp = req.send().await.ok()?;
    if !resp.ok() {
        return None;
    }
    resp.json::<serde_json::Value>().await.ok()
}

/// POST `/api/motor/control {motor,speed}`. The response is ignored, exactly
/// like vanilla (fire and forget; the token cookie rides same-origin).
pub async fn post_motor(motor: u8, speed: i32) {
    let url = format!("{}/api/motor/control", api_base());
    let body = serde_json::json!({"motor": motor, "speed": speed});
    if let Ok(req) = gloo_net::http::Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
    {
        let _ = req.send().await;
    }
}
