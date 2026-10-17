//! Axum handlers — 1:1 with `server.py` routes, plus `GET/POST /api/config`
//! (backend-owned broker settings, changed from the app's Setup page).

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Method, Request, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::data::coerce_i64;
use crate::mqtt::{broadcast, calibration_payload, spawn_mqtt};
use crate::AppState;

pub async fn health(State(s): State<AppState>) -> Json<Value> {
    let slot = s.mqtt.read().await;
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("BUILD_GIT_HASH"),
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

/// Latest ENV frame (ESP-3) plus the indicative alarm verdict. `null` before
/// the first frame: the UI shows `--`, and a fabricated zero would read as
/// "clean air" and "no fire". Alarms follow the same honesty rule: an
/// unevaluable frame reports `unknown`, never `ok`.
pub async fn env(State(s): State<AppState>) -> Json<Value> {
    let frame = s.env.read().await.clone();
    let alarms = crate::env_alarm::evaluate(&frame);
    Json(json!({"env": frame, "alarms": alarms}))
}

#[derive(Deserialize)]
pub struct EnvHistoryQuery {
    /// Chart window in minutes (the UI offers 1 / 10 / 60). Clamped to the
    /// 2 h ring.
    pub minutes: Option<i64>,
}

/// Rolling ENV samples for the Detailed charts, oldest first. Public like
/// `/api/env` (telemetry reads carry no token), thinned to <= 900 points
/// whatever the window so a 1 h request stays one small payload.
pub async fn env_history(
    State(s): State<AppState>,
    Query(q): Query<EnvHistoryQuery>,
) -> Json<Value> {
    let minutes = q.minutes.unwrap_or(10).clamp(1, 120);
    let samples = s.sensors.lock().unwrap().env_chart(minutes);
    Json(json!({"minutes": minutes, "samples": samples}))
}

/// Latest IMU frame (ESP-2 localization). Same null rule as `/api/env`.
pub async fn imu(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"imu": s.imu.read().await.clone()}))
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
    let speed = coerce_i64(body.get("speed")).clamp(-255, 255);
    // Motors 5-6 live on ESP-2 MID (own namespace); 0 = all six.
    let guard = s.mqtt.read().await;
    let sent = if motor == 0 {
        let a = guard
            .handle
            .publish("rover/motor/command", &json!({"motor": 0, "speed": speed}))
            .await;
        let b = guard
            .handle
            .publish("rover/mid/command", &json!({"motor": 0, "speed": speed}))
            .await;
        a || b
    } else if motor == 5 || motor == 6 {
        guard
            .handle
            .publish(
                "rover/mid/command",
                &json!({"motor": motor - 4, "speed": speed}),
            )
            .await
    } else {
        guard
            .handle
            .publish(
                "rover/motor/command",
                &json!({"motor": motor, "speed": speed}),
            )
            .await
    };
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
    // Clone the dir under a microsecond read lock, then build the workbook on
    // a blocking thread with NO store lock held — MQTT ingest keeps flowing
    // while a big export zips. Temp dir + delete-after-stream (see below).
    let dir = s.history.read().await.dir.clone();
    let path = tokio::task::spawn_blocking(move || {
        crate::export::export_excel_dir(&dir, None, &std::env::temp_dir())
    })
    .await
    .ok()
    .flatten();
    xlsx_response(path)
}

/// Detailed-page export: one workbook with Telemetry (motors, all recorded
/// history) + Sensors (ENV log) + IMU (log). Snapshots the rings under a
/// microsecond lock, then builds on a blocking thread with nothing held.
pub async fn history_export(State(s): State<AppState>) -> Response {
    let dir = s.history.read().await.dir.clone();
    let (env, imu) = {
        let log = s.sensors.lock().unwrap();
        (log.env_snapshot(), log.imu_snapshot())
    };
    let path = tokio::task::spawn_blocking(move || {
        crate::export::export_excel_all(&dir, &env, &imu, None, &std::env::temp_dir())
    })
    .await
    .ok()
    .flatten();
    xlsx_response(path)
}

/// 400 when there is nothing to export (same as Python's export route), else
/// the workbook bytes with the sheet MIME type + attachment name. The temp
/// file is deleted right after reading it.
fn xlsx_response(path: Option<std::path::PathBuf>) -> Response {
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
                let _ = std::fs::remove_file(&p);
                Response::builder()
                    .header(
                        header::CONTENT_TYPE,
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                    )
                    .header(
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{name}\""),
                    )
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
            Err(_) => {
                let _ = std::fs::remove_file(&p);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
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

pub async fn history_segments(
    State(s): State<AppState>,
    Query(q): Query<SegmentsQuery>,
) -> Json<Value> {
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

// ── Token auth: every mutating endpoint + /ws ─────────────────────────────
// The token is accepted three ways (first hit wins): `Authorization: Bearer`,
// `?token=`, or the `rover_token` cookie the server sets on page loads — so
// the vendored pages (plain XHR, no token plumbing) keep working untouched.
// Enforced ALWAYS, not only in LAN mode: with no CORS headers a foreign page
// can't read responses, but simple-request POSTs could still be *sent*.

// Pages whose first GET also plants the cookie (see set_token_cookie).
// Root routes AND their /next aliases: the SPA answers on both, so a first
// visit via either URL must end up with the cookie (the WASM UI's WS
// upgrade and mutations read it).
const ENTRY_PATHS: [&str; 9] = [
    "/",
    "/test",
    "/test/",
    "/detailed",
    "/detailed/",
    "/calibration",
    "/calibration/",
    "/setup/",
    "/setup",
];

/// Entry page test: an exact path from ENTRY_PATHS, or any `/next` route (the
/// WASM SPA) — `/next` stays a prefix rule because the SPA owns its subpaths.
fn is_entry_path(path: &str) -> bool {
    ENTRY_PATHS.contains(&path) || path.starts_with("/next")
}

pub const TOKEN_COOKIE: &str = "rover_token";

fn bearer_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

fn query_token(uri: &Uri) -> Option<String> {
    uri.query()?.split('&').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k == "token").then(|| v.to_string())
    })
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| {
            let (k, v) = c.split_once('=')?;
            (k.trim() == TOKEN_COOKIE).then(|| v.trim().to_string())
        })
}

/// True when the request carries `token` (header, query, or cookie).
/// Tokens are 32 alphanumeric chars, so no URL-decoding edge cases.
pub fn token_valid(req: &Request<Body>, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let h = req.headers();
    bearer_of(h).is_some_and(|t| t.as_str() == token)
        || query_token(req.uri()).is_some_and(|t| t.as_str() == token)
        || cookie_token(h).is_some_and(|t| t.as_str() == token)
}

fn token_cookie_value(token: &str) -> String {
    format!("{TOKEN_COOKIE}={token}; Path=/; SameSite=Lax; Max-Age=31536000; HttpOnly")
}

/// Guard for mutating routes + /ws: 401 without a valid token.
pub async fn require_token(State(s): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let token = s.cfg.read().await.auth_token.clone();
    if token_valid(&req, &token) {
        return next.run(req).await.into_response();
    }
    tracing::warn!(
        "rejected {} {} (bad/missing token)",
        req.method(),
        req.uri().path()
    );
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"status": "error", "detail": "missing or invalid token"})),
    )
        .into_response()
}

/// Plant the cookie on entry-page loads that lack a valid one: first visit
/// (PC), or a phone arriving via the QR link (`?token=` authenticates once,
/// the cookie carries every later XHR).
pub async fn set_token_cookie(
    State(s): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let is_entry = req.method() == Method::GET && is_entry_path(req.uri().path());
    let token = s.cfg.read().await.auth_token.clone();
    let has_cookie = cookie_token(req.headers()).is_some_and(|t| t.as_str() == token);
    let mut resp = next.run(req).await.into_response();
    if is_entry && !has_cookie {
        if let Ok(v) = token_cookie_value(&token).parse() {
            resp.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    resp
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"status": "error", "detail": "missing or invalid token"})),
    )
        .into_response()
}

/// Show the token (Setup page display + QR). Requires the token — no leak.
pub async fn get_token(State(s): State<AppState>, req: Request<Body>) -> Response {
    let token = s.cfg.read().await.auth_token.clone();
    if !token_valid(&req, &token) {
        return unauthorized();
    }
    let mut resp = Json(json!({"status": "ok", "token": token.clone()})).into_response();
    if let Ok(v) = token_cookie_value(&token).parse() {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

/// Rotate the token (Setup page button). Old sessions/QR codes die; the
/// caller's cookie is refreshed in this response.
pub async fn rotate_token(State(s): State<AppState>, req: Request<Body>) -> Response {
    let current = s.cfg.read().await.auth_token.clone();
    if !token_valid(&req, &current) {
        return unauthorized();
    }
    let fresh = crate::config::rotate_token(&mut *s.cfg.write().await);
    let mut resp = Json(json!({"status": "ok", "token": fresh.clone()})).into_response();
    if let Ok(v) = token_cookie_value(&fresh).parse() {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}
