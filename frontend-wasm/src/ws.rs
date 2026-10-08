//! Shared WS client — 1:1 with the vanilla JS `/ws` handling.
//! Message shapes mirror `server.py` broadcasts; pages subscribe via `on_message`.
//! Reconnects after 3s like the original pages. Typed per-motor structs arrive
//! with the first real page rewrite; until then payloads stay `Value`.

use std::collections::HashMap;

use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use serde::Deserialize;
use serde_json::Value;

/// Every message the backend can push. `calibration_status` spreads its fields
/// alongside `type`, hence the flattened remainder (same as the JS spread).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "snapshot")]
    Snapshot { data: Vec<Value> },
    #[serde(rename = "motor_update")]
    MotorUpdate {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
    #[serde(rename = "calibration_status")]
    CalibrationStatus {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
    #[serde(rename = "device_status")]
    DeviceStatus {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
    #[serde(rename = "motor_status")]
    MotorStatus {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
    #[serde(rename = "pong")]
    Pong,
    #[serde(other)]
    Unknown,
}

/// Short human-readable connection state for placeholder pages.
pub fn status_text() -> &'static str {
    "shared client compiled; live subscription starts with the first rewritten page"
}

/// Open `/ws` against the SAME origin (mirrors `window.ROVER` same-origin
/// logic) and call `on_msg` for every parsed message. Never returns.
pub async fn run_forever(on_msg: impl Fn(ServerMsg) + Clone + 'static) {
    loop {
        if let Err(e) = run_once(&on_msg).await {
            web_log(&format!("ws closed ({e}), retrying in 3s"));
        }
        gloo_timers_sleep(3).await;
    }
}

async fn run_once(on_msg: &impl Fn(ServerMsg)) -> Result<(), String> {
    let url = ws_url();
    let mut ws = WebSocket::open(&url).map_err(|e| e.to_string())?;
    // Say hello the way the vanilla pages do (server answers `pong`).
    let _ = ws.send(Message::Text("ping".into())).await;
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(Message::Text(text)) => match serde_json::from_str::<ServerMsg>(&text) {
                Ok(parsed) => on_msg(parsed),
                Err(e) => web_log(&format!("ws: unparsable message ({e})")),
            },
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("stream ended".into())
}

/// `ws(s)://host/ws` derived from the page origin — same rule as `window.ROVER`.
fn ws_url() -> String {
    let window = web_sys::window().expect("window");
    let loc = window.location();
    let proto = loc.protocol().unwrap_or_else(|_| "http:".into());
    let ws_proto = if proto == "https:" { "wss" } else { "ws" };
    let host = loc.host().unwrap_or_else(|_| "localhost:8000".into());
    format!("{ws_proto}://{host}/ws")
}

fn web_log(s: &str) {
    web_sys::console::log_1(&s.into());
}

async fn gloo_timers_sleep(secs: u32) {
    gloo_timers::future::TimeoutFuture::new(secs * 1000).await;
}
