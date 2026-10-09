//! Shared integration-test scaffolding: hermetic app state with no broker.
//! Every helper uses temp dirs and `Default` constructors — tests never touch
//! `./history`, `./rover_config.json`, or the real MQTT port.
//!
//! Items are shared across test targets, so each target sees some as unused.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use digital_twin_dashboard::{
    calibration::CalibrationManager, config::AppConfig, data::DataHandler, history::HistoryStore,
    mqtt, AppState,
};
use serde_json::Value;
use tokio::sync::{broadcast, RwLock};

pub const TEST_TOKEN: &str = "test-token-0123456789abcdef";

pub fn tmpdir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dtd-it-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

pub struct Fixture {
    pub ctx: mqtt::Ctx,
    pub data: Arc<RwLock<DataHandler>>,
    pub history: Arc<RwLock<HistoryStore>>,
    pub cal: Arc<RwLock<CalibrationManager>>,
    pub motor_status: Arc<RwLock<Value>>,
    pub device: Arc<RwLock<mqtt::DeviceState>>,
    pub rx: broadcast::Receiver<Value>,
    pub dir: PathBuf,
}

/// Ingest-level fixture: call `mqtt::on_mqtt_message(&f.ctx, ...)` directly,
/// then inspect `f.*` and drain `f.rx`. No broker, no TCP.
pub fn fixture(tag: &str) -> Fixture {
    let dir = tmpdir(tag);
    let (bcast, rx) = broadcast::channel(256);
    let data = Arc::new(RwLock::new(DataHandler::default()));
    let history = Arc::new(RwLock::new(HistoryStore::new(&dir)));
    let cal = Arc::new(RwLock::new(CalibrationManager::new()));
    let motor_status = Arc::new(RwLock::new(Value::Object(Default::default())));
    let device = Arc::new(RwLock::new(mqtt::DeviceState::default()));
    let ctx = mqtt::Ctx {
        data: data.clone(),
        history: history.clone(),
        cal: cal.clone(),
        motor_status: motor_status.clone(),
        device: device.clone(),
        bcast: bcast.clone(),
    };
    let _ = bcast;
    Fixture {
        ctx,
        data,
        history,
        cal,
        motor_status,
        device,
        rx,
        dir,
    }
}

/// Full app state for route/WS tests. The MQTT client points at a dead port
/// and its retry loop is aborted: handlers see `mqtt_connected: false`,
/// exactly like "broker down" in production.
pub async fn test_state(tag: &str) -> (AppState, PathBuf) {
    let f = fixture(tag);
    let slot = mqtt::spawn_mqtt(f.ctx.clone(), "127.0.0.1", 9).await;
    slot.task.abort();
    let cfg = AppConfig {
        auth_token: TEST_TOKEN.into(),
        ..AppConfig::default()
    };
    let state = AppState {
        data: f.data,
        history: f.history,
        cal: f.cal,
        clients: Arc::new(AtomicUsize::new(0)),
        motor_status: f.motor_status,
        device: f.device,
        mqtt: Arc::new(RwLock::new(slot)),
        bcast: f.ctx.bcast.clone(),
        cfg: Arc::new(RwLock::new(cfg)),
    };
    (state, f.dir)
}

/// Drain all pending broadcasts (non-blocking).
pub fn drain(rx: &mut broadcast::Receiver<Value>) -> Vec<Value> {
    let mut out = vec![];
    while let Ok(v) = rx.try_recv() {
        out.push(v);
    }
    out
}
