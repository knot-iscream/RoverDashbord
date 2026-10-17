//! Rover backend library — the `digital-twin-dashboard` binary is a thin
//! `main()` over this. Splitting the lib out lets integration tests (and
//! later the Tauri shell) link the backend instead of shelling out to it.

pub mod calibration;
pub mod config;
pub mod data;
pub mod env_alarm;
pub mod export;
pub mod history;
pub mod mqtt;
pub mod routes;
pub mod sensor_log;
pub mod ws;

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Method, Request},
    middleware,
    response::IntoResponse,
    routing::{get, get_service, post},
    Router,
};
use serde_json::Value;
use tower_http::services::{ServeDir, ServeFile};

use calibration::CalibrationManager;
use config::AppConfig;
use data::DataHandler;
use history::HistoryStore;
use mqtt::{DeviceState, MqttSlot};
use sensor_log::SensorLog;

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<tokio::sync::RwLock<DataHandler>>,
    pub history: Arc<tokio::sync::RwLock<HistoryStore>>,
    pub cal: Arc<tokio::sync::RwLock<CalibrationManager>>,
    pub clients: Arc<AtomicUsize>,
    pub motor_status: Arc<tokio::sync::RwLock<Value>>,
    /// Latest raw frame per new feed. `Value::Null` until the board's first
    /// frame: null means `--` in the UI, an empty object would look like real
    /// zeros. Deliberately outside `DataHandler` so motor health math and the
    /// parity goldens stay untouched.
    pub env: Arc<tokio::sync::RwLock<Value>>,
    pub imu: Arc<tokio::sync::RwLock<Value>>,
    /// Rolling ENV + IMU logs behind the Detailed charts and the Excel
    /// export. In-memory: resets with the process, disk persistence is the
    /// planned Pi-session item.
    pub sensors: Arc<std::sync::Mutex<SensorLog>>,
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
            env: self.env.clone(),
            imu: self.imu.clone(),
            sensors: self.sensors.clone(),
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

    // Rust UI (Phase B, opt-in): Leptos WASM build output from
    // `frontend-wasm/` via trunk (`dist/next/`). The SPA bundle answers
    // the four root routes AND /next/* (client picks the view from the
    // path); vanilla pages are deleted, no fallback remains. Static files
    // (style.css, favicon, splash, setup/) still resolve via the fallback.
    // Missing dir (trunk not built yet) serves 404s on these routes.
    let next_root = config::base_dir().join("dist").join("next");
    let next_index = next_root.join("index.html");
    let next_svc = ServeDir::new(&next_root).append_index_html_on_directories(true);
    let next_page = get_service(ServeFile::new(&next_index));

    let cookie_state = state.clone();
    Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/device/status", get(routes::device_status))
        .route("/api/motors", get(routes::motors))
        .route("/api/motor/status", get(routes::motor_status))
        .route("/api/env", get(routes::env))
        .route("/api/env/history", get(routes::env_history))
        .route("/api/imu", get(routes::imu))
        .route("/api/calibration", get(routes::calibration))
        .route("/api/calibration/status", get(routes::calibration_status))
        .route("/api/calibration/export", get(routes::calibration_export))
        .route("/api/history/days", get(routes::history_days))
        .route("/api/history/segments", get(routes::history_segments))
        .route("/api/history/export", get(routes::history_export))
        .route("/api/config", get(routes::get_config))
        .nest_service(
            "/setup",
            ServeDir::new(config::base_dir().join("web/setup")),
        )
        .route("/", next_page.clone())
        .route("/detailed", next_page.clone())
        .route("/detailed/", next_page.clone())
        .route("/test", next_page.clone())
        .route("/test/", next_page.clone())
        .route("/calibration", next_page.clone())
        .route("/calibration/", next_page.clone())
        .route("/next/test", next_page.clone())
        .route("/next/detailed", next_page.clone())
        .route("/next/calibration", next_page)
        .nest_service("/next", next_svc)
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
        .layer(middleware::from_fn(cache_headers))
}

/// Cache policy (stale-bundle class fix, Oct 2026: a heuristically cached
/// index.html pointing at deleted hashed files boots to a black screen).
/// Entry HTML is never stored; content-hashed bundle assets are immutable
/// for a year (safe: the hash changes with the bytes). The shared theme
/// stylesheet revalidates on every load — its bytes change just as often as
/// the HTML, and a heuristically cached copy silently unstyles new markup.
/// Everything else keeps tower/Axum defaults.
async fn cache_headers(req: Request<Body>, next: middleware::Next) -> axum::response::Response {
    // Only cacheable verbs, never the JSON API or the WS handshake: those
    // must always run live, and POST responses are never stored anyway.
    let cacheable = (req.method() == Method::GET || req.method() == Method::HEAD)
        && !req.uri().path().starts_with("/api/");
    let path = req.uri().path().to_string();
    let mut resp = next.run(req).await.into_response();
    if !cacheable {
        return resp;
    }
    let last = path.rsplit('/').next().unwrap_or("");
    let is_html =
        path == "/" || path.ends_with('/') || path.ends_with(".html") || !last.contains('.');
    let is_hashed_asset =
        path.starts_with("/next/") && (path.ends_with(".js") || path.ends_with(".wasm"));
    let value = if is_hashed_asset {
        "public, max-age=31536000, immutable"
    } else if is_html {
        "no-store"
    } else if path.ends_with(".css") {
        "no-cache"
    } else {
        return resp;
    };
    if let Ok(v) = value.parse() {
        resp.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    resp
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
    // ENV / IMU frames start as Null ("never received") rather than an empty
    // object: null renders `--`, an empty object would look like real zeros.
    let env = Arc::new(tokio::sync::RwLock::new(Value::Null));
    let imu = Arc::new(tokio::sync::RwLock::new(Value::Null));
    let sensors = Arc::new(std::sync::Mutex::new(SensorLog::default()));
    let device = Arc::new(tokio::sync::RwLock::new(DeviceState::default()));
    let ctx = mqtt::Ctx {
        data: data.clone(),
        history: history.clone(),
        cal: cal.clone(),
        motor_status: motor_status.clone(),
        env: env.clone(),
        imu: imu.clone(),
        sensors: sensors.clone(),
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
        env,
        imu,
        sensors,
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
