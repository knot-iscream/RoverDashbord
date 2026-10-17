//! THROWAWAY — WS stream probe for the fake-telemetry visual check.
//!
//! Connects to `/ws` with the backend token, prints the first N messages,
//! and asserts every `motor_update` carries the contract fields with sane
//! types. Ground truth is whatever `fake_telemetry` publishes; this only
//! proves the backend rebroadcasts it intact.
//!
//! DELETE BEFORE HARDWARE DAY. Usage:
//!   cargo run --example ws_probe -- <token> [n=12]

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let token = std::env::args()
        .nth(1)
        .expect("usage: ws_probe <token> [n]");
    let n: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let url = format!("ws://127.0.0.1:8000/ws?token={token}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connect failed (is the backend up?)");
    // Say hello like the dashboard does.
    ws.send(Message::Text("ping".into())).await.ok();
    let mut seen_updates = 0;
    let mut seen_types = std::collections::BTreeSet::new();
    while seen_updates < n {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
            .await
            .expect("ws read timed out — no traffic?")
            .expect("ws stream ended")
            .expect("ws frame error");
        let Message::Text(text) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let t = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("?")
            .to_string();
        seen_types.insert(t.clone());
        println!("{t}: {v}");
        if t == "motor_update" {
            for key in [
                "motor",
                "temp",
                "voltage",
                "current",
                "health",
                "temp_valid",
                "ina_ok",
            ] {
                assert!(v.get(key).is_some(), "motor_update missing field: {key}");
            }
            let m = v["motor"].as_u64().expect("motor numeric");
            assert!((1..=4).contains(&m), "motor id 1-4, got {m}");
            seen_updates += 1;
        }
    }
    eprintln!("[probe] OK: {seen_updates} motor_updates, types: {seen_types:?}");
}
