//! WS `/ws` — 1:1 with `server.py::websocket_endpoint`.
//! On connect: latest-data snapshot (only when non-empty) + calibration
//! status + device presence. Then live `motor_update` / `calibration_status` /
//! `device_status` fan-out plus `ping` → `pong`.

use std::sync::atomic::Ordering;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde_json::{json, Value};

use crate::mqtt::calibration_payload;
use crate::AppState;

pub async fn ws_handler(ws: WebSocketUpgrade, State(s): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle(socket, s))
}

async fn send(
    socket_split_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    v: &serde_json::Value,
) {
    let _ = socket_split_tx.send(Message::Text(v.to_string())).await;
}

/// Tag a stored raw frame with its feed type (`env_update` / `imu_update`),
/// exactly like the live fan-out does. A reconnecting page then has one code
/// path per feed instead of a separate snapshot shape.
fn tagged(kind: &str, frame: Value) -> Value {
    let mut payload = json!({"type": kind});
    if let Value::Object(map) = frame {
        if let Value::Object(out) = &mut payload {
            out.extend(map);
        }
    }
    payload
}

async fn handle(socket: WebSocket, s: AppState) {
    s.clients.fetch_add(1, Ordering::SeqCst);
    let (mut tx, mut rx) = socket.split();
    let mut bcast = s.bcast.subscribe();

    // Snapshot of latest data (only when non-empty, like Python's `if snapshot:`).
    let snapshot = s.data.read().await.get_latest();
    if !snapshot.is_empty() {
        send(&mut tx, &json!({"type": "snapshot", "data": snapshot})).await;
    }
    // New feeds replay as their own update types: nothing is sent until the
    // board has actually spoken (Null = never received, not zero).
    let env = s.env.read().await.clone();
    if !env.is_null() {
        // Same payload as the live fan-out: frame + alarm verdict.
        send(&mut tx, &crate::mqtt::env_update_payload(env)).await;
    }
    let imu = s.imu.read().await.clone();
    if !imu.is_null() {
        send(&mut tx, &tagged("imu_update", imu)).await;
    }
    send(&mut tx, &calibration_payload(&s.ctx()).await).await;
    send(&mut tx, &s.device.read().await.payload()).await;

    loop {
        tokio::select! {
            msg = bcast.recv() => {
                match msg {
                    Ok(payload) => send(&mut tx, &payload).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) if t.trim() == "ping" => {
                        send(&mut tx, &json!({"type": "pong"})).await;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let _ = tx.send(Message::Close(frame)).await;
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }
    s.clients.fetch_sub(1, Ordering::SeqCst);
}
