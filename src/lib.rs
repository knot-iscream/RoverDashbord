//! Rover backend library — the `digital-twin-dashboard` binary is a thin
//! `main()` over this. Splitting the lib out lets integration tests (and
//! later the Tauri shell) link the backend instead of shelling out to it.

pub mod calibration;
pub mod config;
pub mod data;
pub mod export;
pub mod history;
pub mod mqtt;
pub mod routes;
pub mod ws;

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use serde_json::Value;
use tower_http::services::ServeDir;

use calibration::CalibrationManager;
use config::AppConfig;
use data::DataHandler;
use history::HistoryStore;
use mqtt::{DeviceState, MqttSlot};

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<tokio::sync::RwLock<DataHandler>>,
    pub history: Arc<tokio::sync::RwLock<HistoryStore>>,
    pub cal: Arc<tokio::sync::RwLock<CalibrationManager>>,
    pub clients: Arc<AtomicUsize>,
    pub motor_status: Arc<tokio::sync::RwLock<Value>>,
    pub device: Arc<tokio::sync::RwLock<DeviceState>>,
    pub mqtt: Arc<tokio::sync::RwLock<MqttSlot>>,
    pub bcast: tokio::sync::broadcast::Sender<Value>,
    pub cfg: Arc<tokio::sync::RwLock<AppConfig>>,
}

impl AppState {
    pub fn ctx(&self) -> mqtt::Ctx {
        mqtt::Ctx {
            data: self.data.clone(),
            history: self.history.clone(),
            cal: self.cal.clone(),
            motor_status: self.motor_status.clone(),
            device: self.device.clone(),
            bcast: self.bcast.clone(),
        }
    }
}

/// The full route table: public reads, token-guarded mutations + /ws, Setup
/// pages, vendored dashboard fallback, cookie planter. Built here (not in
/// main) so integration tests exercise THE table, not a copy — and so a
/// future desktop shell can serve it.
pub fn build_router(state: AppState) -> Router {
    // Public reads (telemetry, history, setup info): no token needed.
    // Everything that MOVES the rover or changes config sits behind
    // require_token — as does /ws, since a forged socket could drive motors.
    let protected = Router::new()
        .route("/api/motor/control", post(routes::motor_control))
        .route("/api/calibration/start", post(routes::calibration_start))
        .route("/api/calibration/stop", post(routes::calibration_stop))
        .route(
            "/api/calibration/baseline/:motor_id",
            post(routes::set_baseline),
        )
        .route("/api/config", post(routes::post_config))
        .route(
            "/api/auth/token",
            get(routes::get_token).post(routes::rotate_token),
        )
        .route("/api/setup/qr.svg", get(routes::setup_qr))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            routes::require_token,
        ));
    let ws_route = Router::new().route("/ws", get(ws::ws_handler)).route_layer(
        middleware::from_fn_with_state(state.clone(), routes::require_token),
    );

    // Dashboard pages vendored in ./web/ (1:1 copy of the v1 pages).
    // WEB_ROOT env overrides for dev. Mounted LAST (fallback).
    let web_root = std::env::var("WEB_ROOT").unwrap_or_else(|_| "web".into());
    let static_svc = ServeDir::new(&web_root).append_index_html_on_directories(true);

    let cookie_state = state.clone();
    Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/device/status", get(routes::device_status))
        .route("/api/motors", get(routes::motors))
        .route("/api/motor/status", get(routes::motor_status))
        .route("/api/calibration", get(routes::calibration))
        .route("/api/calibration/status", get(routes::calibration_status))
        .route("/api/calibration/export", get(routes::calibration_export))
        .route("/api/history/days", get(routes::history_days))
        .route("/api/history/segments", get(routes::history_segments))
        .route("/api/config", get(routes::get_config))
        .nest_service("/setup", ServeDir::new("web/setup"))
        .merge(protected)
        .merge(ws_route)
        .with_state(state)
        // NOTE: fallback must come BEFORE the layers — axum only wraps
        // routes/fallbacks that already exist when .layer() is called.
        // The cookie layer sits last on purpose: entry-page GETs (including
        // the static fallback) plant `rover_token` for later XHR calls.
        .fallback_service(static_svc)
        .layer(middleware::from_fn_with_state(
            cookie_state,
            routes::set_token_cookie,
        ))
}
