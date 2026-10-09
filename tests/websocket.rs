//! WebSocket protocol tests over real TCP: upgrade auth, connect snapshot
//! ordering, ping→pong, live fan-out, multi-client broadcast, client gauge.
//! No broker needed — broadcasts are triggered by direct ingest calls on the
//! same shared state the server holds.

mod common;

use common::TEST_TOKEN;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

async fn serve(tag: &str) -> (String, digital_twin_dashboard::AppState, PathBufGuard) {
    let (state, dir) = common::test_state(tag).await;
    let guard = PathBufGuard(dir);
    let app = digital_twin_dashboard::build_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{addr}/ws"), state, guard)
}

struct PathBufGuard(std::path::PathBuf);
impl Drop for PathBufGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

async fn next_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        match ws.next().await.expect("stream open").expect("no ws error") {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Close(_) => panic!("socket closed unexpectedly"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn upgrade_rejects_without_token() {
    let (url, _s, _g) = serve("ws-auth").await;
    let err = tokio_tungstenite::connect_async(&url).await.unwrap_err();
    match err {
        tokio_tungstenite::tungstenite::Error::Http(r) => assert_eq!(r.status(), 401),
        e => panic!("expected 401, got {e:?}"),
    }
}

#[tokio::test]
async fn connect_order_ping_pong() {
    let (url, _s, _g) = serve("ws-order").await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{url}?token={TEST_TOKEN}"))
        .await
        .unwrap();
    // Empty data → no snapshot; calibration_status then device_status.
    assert_eq!(next_json(&mut ws).await["type"], "calibration_status");
    let dev = next_json(&mut ws).await;
    assert_eq!(dev["type"], "device_status");
    assert_eq!(dev["online"], false);
    // Ping → pong.
    ws.send(Message::Text("ping".into())).await.unwrap();
    assert_eq!(next_json(&mut ws).await["type"], "pong");
}

#[tokio::test]
async fn snapshot_first_when_data_present() {
    let (url, state, _g) = serve("ws-snap").await;
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/motor/1",
        json!({"motor": 1, "speed": 100, "temp": 40.0, "temp_valid": true}),
    )
    .await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{url}?token={TEST_TOKEN}"))
        .await
        .unwrap();
    let snap = next_json(&mut ws).await;
    assert_eq!(snap["type"], "snapshot");
    assert_eq!(snap["data"].as_array().unwrap().len(), 1);
    assert_eq!(snap["data"][0]["id"], 1);
}

#[tokio::test]
async fn live_fanout_to_all_clients() {
    let (url, state, _g) = serve("ws-fan").await;
    // Prime presence BEFORE connecting: the first-ever ingest flips
    // device_status, and that broadcast would otherwise land mid-test.
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/motor/9",
        json!({"motor": 9, "speed": 0}),
    )
    .await;
    let (mut a, _) = tokio_tungstenite::connect_async(format!("{url}?token={TEST_TOKEN}"))
        .await
        .unwrap();
    let (mut b, _) = tokio_tungstenite::connect_async(format!("{url}?token={TEST_TOKEN}"))
        .await
        .unwrap();
    // Drain connect bursts (snapshot from the priming packet, then
    // calibration_status + device_status each).
    for ws in [&mut a, &mut b] {
        let snap = next_json(ws).await;
        assert_eq!(snap["type"], "snapshot");
        assert_eq!(snap["data"].as_array().unwrap().len(), 1);
        assert_eq!(next_json(ws).await["type"], "calibration_status");
        assert_eq!(next_json(ws).await["type"], "device_status");
    }
    // One ingest → both sockets get the same motor_update.
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/motor/3",
        json!({"motor": 3, "speed": -90, "vibration": 1, "vibration_valid": true}),
    )
    .await;
    let ma = next_json(&mut a).await;
    let mb = next_json(&mut b).await;
    for m in [&ma, &mb] {
        assert_eq!(m["type"], "motor_update");
        assert_eq!(m["motor"], 3);
        assert_eq!(m["speed"], -90);
    }
    // Gauge counts both sockets.
    assert_eq!(state.clients.load(std::sync::atomic::Ordering::SeqCst), 2);
}
