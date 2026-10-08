//! Phase A: Axum 1:1 port of D:/Git/DigitalTwinDashboard/backend/server.py
//! Single port (default 0.0.0.0:8000, `PORT` env / `rover_config.json`) —
//! `/api/*` + `/ws` + `/setup` first, original pages via static fallback last.

mod calibration;
mod config;
mod data;
mod export;
mod history;
mod mqtt;
mod routes;
mod ws;

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use axum::{middleware::{self, Next}, routing::get, Router};
use axum::response::{IntoResponse, Redirect};
use serde_json::Value;
use tower_http::{cors::CorsLayer, services::{ServeDir, ServeFile}};
use tracing_subscriber::EnvFilter;

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

/// Phase B opt-in: `?ui=next` on a rewritten page redirects to its Leptos
/// build under `/next/*`. Vanilla stays the default; until the first trunk
/// build creates `web/dist`, `/next/*` answers 404 (vanilla unaffected).
const NEXT_PAGES: [&str; 4] = ["/", "/test/", "/detailed/", "/calibration/"];

async fn next_switch(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let path = req.uri().path().to_string();
    let want_next = NEXT_PAGES.contains(&path.as_str())
        && req
            .uri()
            .query()
            .unwrap_or("")
            .split('&')
            .any(|p| p == "ui=next");
    if want_next {
        let target = if path == "/" {
            "/next/".to_string()
        } else {
            format!("/next{path}")
        };
        return Redirect::temporary(&target).into_response();
    }
    next.run(req).await.into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Backend-owned config: rover_config.json < env < defaults. No edits needed.
    let cfg = AppConfig::load();
    let http_port = cfg.http_port;
    let (broker, broker_port) = (cfg.mqtt_broker.clone(), cfg.mqtt_port);

    let (bcast, _) = tokio::sync::broadcast::channel::<Value>(256);
    let data = Arc::new(tokio::sync::RwLock::new(DataHandler::new()));
    let history = Arc::new(tokio::sync::RwLock::new(HistoryStore::new("history")));
    let cal = Arc::new(tokio::sync::RwLock::new(CalibrationManager::new()));
    let motor_status =
        Arc::new(tokio::sync::RwLock::new(Value::Object(Default::default())));
    let device = Arc::new(tokio::sync::RwLock::new(DeviceState::default()));
    let ctx = mqtt::Ctx {
        data: data.clone(),
        history: history.clone(),
        cal: cal.clone(),
        motor_status: motor_status.clone(),
        device: device.clone(),
        bcast: bcast.clone(),
    };
    // MQTT client (auto-reconnects; resubscribes on every ConnAck).
    let slot = mqtt::spawn_mqtt(ctx, &broker, broker_port).await;
    let state = AppState {
        data,
        history,
        cal,
        clients: Arc::new(AtomicUsize::new(0)),
        motor_status,
        device,
        mqtt: Arc::new(tokio::sync::RwLock::new(slot)),
        bcast,
        cfg: Arc::new(tokio::sync::RwLock::new(cfg)),
    };
    // Device presence monitor (broadcasts only on flip, like Python).
    tokio::spawn(mqtt::monitor_device(state.ctx()));

    // Tower ServeDir on the ORIGINAL repo root = guaranteed 1:1 pages.
    // Falls back to ./web/ if copied locally. Mounted LAST (fallback).
    let web_root = std::env::var("WEB_ROOT").unwrap_or_else(|_| "D:/Git/DigitalTwinDashboard".into());
    let static_svc = ServeDir::new(&web_root).append_index_html_on_directories(true);

    let app = Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/device/status", get(routes::device_status))
        .route("/api/motors", get(routes::motors))
        .route(
            "/api/motor/control",
            axum::routing::post(routes::motor_control),
        )
        .route("/api/motor/status", get(routes::motor_status))
        .route("/api/calibration", get(routes::calibration))
        .route(
            "/api/calibration/baseline/:motor_id",
            axum::routing::post(routes::set_baseline),
        )
        .route("/api/calibration/status", get(routes::calibration_status))
        .route(
            "/api/calibration/start",
            axum::routing::post(routes::calibration_start),
        )
        .route(
            "/api/calibration/stop",
            axum::routing::post(routes::calibration_stop),
        )
        .route("/api/calibration/export", get(routes::calibration_export))
        .route("/api/history/days", get(routes::history_days))
        .route("/api/history/segments", get(routes::history_segments))
        .route("/api/config", get(routes::get_config).post(routes::post_config))
        .route("/ws", get(ws::ws_handler))
        .nest_service("/setup", ServeDir::new("web/setup"))
        .nest_service(
            "/next",
            ServeDir::new("web/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("web/dist/index.html")),
        )
        .with_state(state)
        // NOTE: fallback must come BEFORE the layers — axum only wraps
        // routes/fallbacks that already exist when .layer() is called.
        .fallback_service(static_svc)
        .layer(middleware::from_fn(next_switch))
        .layer(CorsLayer::permissive());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], http_port));
    tracing::info!("listening on {addr} (web root: {web_root})");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
