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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            routes::require_token,
        ));
    let ws_route = Router::new().route("/ws", get(ws::ws_handler)).route_layer(
        middleware::from_fn_with_state(state.clone(), routes::require_token),
    );

    // Dashboard pages: WEB_ROOT env wins, else <base_dir>/web (vendored 1:1
    // copy of the v1 pages). Mounted LAST (fallback).
    let web_root = std::env::var("WEB_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config::base_dir().join("web"));
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
        .nest_service(
            "/setup",
            ServeDir::new(config::base_dir().join("web/setup")),
        )
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

/// Logging init shared by the backend binary and the desktop shell.
pub fn init_logging() {
    // Crate-targeted default: our `listening on …` line always prints, while
    // hyper/tower stay quiet unless RUST_LOG says otherwise.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "digital_twin_dashboard=info".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .init();
}

/// Build full app state: config, shared stores, MQTT client, presence
/// monitor. Used by the backend binary and the desktop shell alike.
pub async fn build_state() -> AppState {
    // Backend-owned config: rover_config.json < env < defaults. No edits needed.
    let cfg = AppConfig::load();
    let (broker, broker_port) = (cfg.mqtt_broker.clone(), cfg.mqtt_port);

    let (bcast, _) = tokio::sync::broadcast::channel::<Value>(256);
    let data = Arc::new(tokio::sync::RwLock::new(DataHandler::new()));
    let history = Arc::new(tokio::sync::RwLock::new(HistoryStore::new(
        config::base_dir().join("history"),
    )));
    let cal = Arc::new(tokio::sync::RwLock::new(CalibrationManager::new()));
    let motor_status = Arc::new(tokio::sync::RwLock::new(Value::Object(Default::default())));
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
    state
}

/// Serve `state` on `addr` until `shutdown` resolves.
pub async fn serve_forever(
    state: AppState,
    addr: std::net::SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}
