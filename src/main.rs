//! Axum 1:1 port of the v1 Python backend (`backend/server.py`).
//! Single port — `/api/*` + `/ws` + `/setup` first, vendored `web/` pages
//! via static fallback last. Binds 127.0.0.1 by default (`ROVER_LAN=1` or the
//! Setup page opts into 0.0.0.0, needs a restart). No CORS headers are ever
//! sent: our pages are same-origin, and cross-origin JS gets nothing.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use serde_json::Value;
use tracing_subscriber::EnvFilter;

use digital_twin_dashboard::{
    calibration::CalibrationManager,
    config::AppConfig,
    data::DataHandler,
    history::HistoryStore,
    mqtt::{self, DeviceState},
    AppState,
};

#[tokio::main]
async fn main() {
    // Crate-targeted default: our `listening on …` line always prints, while
    // hyper/tower stay quiet unless RUST_LOG says otherwise.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "digital_twin_dashboard=info".into());
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .init();

    // Backend-owned config: rover_config.json < env < defaults. No edits needed.
    let cfg = AppConfig::load();
    let http_port = cfg.http_port;
    let lan = cfg.lan_access;
    let (broker, broker_port) = (cfg.mqtt_broker.clone(), cfg.mqtt_port);

    let (bcast, _) = tokio::sync::broadcast::channel::<Value>(256);
    let data = Arc::new(tokio::sync::RwLock::new(DataHandler::new()));
    let history = Arc::new(tokio::sync::RwLock::new(HistoryStore::new("history")));
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

    let app = digital_twin_dashboard::build_router(state);

    let ip = if lan { [0, 0, 0, 0] } else { [127, 0, 0, 1] };
    let addr = std::net::SocketAddr::from((ip, http_port));
    tracing::info!("listening on {addr} (lan: {lan})");
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                "cannot listen on {addr} ({e}) — is another Rover app already \
                 running? Run Doctor.bat to diagnose."
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutting down");
        })
        .await
    {
        tracing::error!("server error: {e}");
    }
}
