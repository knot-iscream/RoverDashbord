//! Axum handlers — 1:1 with `server.py` routes, plus `GET/POST /api/config`
//! (backend-owned broker settings, changed from the app's Setup page).

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mqtt::{broadcast, calibration_payload, spawn_mqtt};
use crate::AppState;

pub async fn health(State(s): State<AppState>) -> Json<Value> {
    let slot = s.mqtt.read().await;
    Json(json!({
        "status": "ok",
        "clients": s.clients.load(std::sync::atomic::Ordering::SeqCst),
        "mqtt_connected": slot.handle.is_connected(),
        "mqtt_broker": slot.handle.broker,
        "device_online": s.device.read().await.online,
    }))
}

pub async fn device_status(State(s): State<AppState>) -> Json<Value> {
    let mut dev = s.device.write().await;
    dev.refresh();
    let mut p = dev.payload();
    if let Value::Object(out) = &mut p {
        out.remove("type");
    }
    Json(p)
}

pub async fn motors(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"motors": s.data.read().await.get_latest()}))
}

fn coerce_speed(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(0),
        Some(Value::String(str)) => str.trim().parse::<f64>().map(|f| f.trunc() as i64).unwrap_or(0),
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

fn coerce_motor(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64()).unwrap_or(0)
}

/// 1:1 with `api_motor_control`: clamp to [-255, 255]; publish when the
/// broker is up, else the exact error shape (`"detail"`, not `"error"`).
/// Note: on non-numeric `speed` Python raises (500); we coerce to 0 — the
/// dashboard always sends ints.
pub async fn motor_control(State(s): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let motor_raw = body.get("motor").cloned().unwrap_or(json!(0));
    let motor = coerce_motor(body.get("motor"));
    let speed = coerce_speed(body.get("speed")).clamp(-255, 255);
    let sent = s
        .mqtt
        .read()
        .await
        .handle
        .publish("rover/motor/command", &json!({"motor": motor, "speed": speed}))
        .await;
    if sent {
        Json(json!({"status": "ok", "motor": motor_raw, "speed": speed, "sent": true}))
    } else {
        Json(json!({
            "status": "error",
            "detail": "MQTT not connected",
            "sent": false,
            "mqtt_connected": s.mqtt.read().await.handle.is_connected(),
        }))
    }
}

pub async fn motor_status(State(s): State<AppState>) -> Json<Value> {
    Json(s.motor_status.read().await.clone())
}

pub async fn calibration(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"baselines": s.data.read().await.baselines}))
}

pub async fn set_baseline(
    State(s): State<AppState>,
    Path(motor_id): Path<u8>,
    Json(baseline): Json<Value>,
) -> Json<Value> {
    s.data.write().await.set_baseline(motor_id, baseline);
    Json(json!({"status": "ok", "motor": motor_id}))
}

pub async fn calibration_start(State(s): State<AppState>) -> Json<Value> {
    let result = s.cal.write().await.start();
    let _ = s
        .mqtt
        .read()
        .await
        .handle
        .publish("rover/calibration/command", &json!({"action": "start"}))
        .await;
    broadcast(&s.ctx(), calibration_payload(&s.ctx()).await);
    Json(serde_json::to_value(result).unwrap_or(json!({})))
}

pub async fn calibration_stop(State(s): State<AppState>) -> Json<Value> {
    let result = s.cal.write().await.stop();
    let _ = s
        .mqtt
        .read()
        .await
        .handle
        .publish("rover/calibration/command", &json!({"action": "stop"}))
        .await;
    broadcast(&s.ctx(), calibration_payload(&s.ctx()).await);
    Json(serde_json::to_value(result).unwrap_or(json!({})))
}

pub async fn calibration_status(State(s): State<AppState>) -> Json<Value> {
    Json(crate::mqtt::status_value(&s.ctx()).await)
}

pub async fn calibration_export(State(s): State<AppState>) -> Response {
    let path = {
        let store = s.history.read().await;
        crate::export::export_excel(&store, None, std::path::Path::new("."))
    };
    match path {
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No recorded data to export"})),
        )
            .into_response(),
        Some(p) => match std::fs::read(&p) {
            Ok(bytes) => {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "history_export.xlsx".into());
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
                    .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
    }
}

pub async fn history_days(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"days": s.history.read().await.days()}))
}

#[derive(Deserialize)]
pub struct SegmentsQuery {
    pub day: String,
}

pub async fn history_segments(State(s): State<AppState>, Query(q): Query<SegmentsQuery>) -> Json<Value> {
    Json(json!({"day": q.day, "segments": s.history.read().await.segments(&q.day)}))
}

// ── Backend-owned config (no file/env edits by the user) ──

pub async fn get_config(State(s): State<AppState>) -> Json<Value> {
    let cfg = s.cfg.read().await;
    Json(json!({
        "mqtt_broker": cfg.mqtt_broker,
        "mqtt_port": cfg.mqtt_port,
        "http_port": cfg.http_port,
        "http_port_note": "changing the app port needs an app restart (set PORT env or rover_config.json)",
        "mqtt_connected": s.mqtt.read().await.handle.is_connected(),
    }))
}

pub async fn post_config(State(s): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let (broker, port, changed) = {
        let mut cfg = s.cfg.write().await;
        match crate::config::apply_update(&mut cfg, &body) {
            Err(e) => return Json(json!({"status": "error", "detail": e})),
            Ok(changed) => {
                if changed {
                    cfg.save();
                }
                (cfg.mqtt_broker.clone(), cfg.mqtt_port, changed)
            }
        }
    };
    if changed {
        // Restart the MQTT client against the new broker (no app restart).
        let mut slot = s.mqtt.write().await;
        slot.task.abort();
        let fresh = spawn_mqtt(s.ctx(), &broker, port).await;
        slot.handle = fresh.handle;
        slot.task = fresh.task;
    }
    Json(json!({
        "status": "ok",
        "changed": changed,
        "mqtt_broker": broker,
        "mqtt_port": port,
        "mqtt_connected": s.mqtt.read().await.handle.is_connected(),
    }))
}

