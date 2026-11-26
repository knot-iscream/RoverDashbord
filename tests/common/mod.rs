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
    mqtt, sensor_log::SensorLog, AppState,
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
    pub env: Arc<RwLock<Value>>,
    pub imu: Arc<RwLock<Value>>,
    pub servo: Arc<RwLock<Value>>,
    pub lidar: Arc<RwLock<Value>>,
    pub lidar_cloud: Arc<RwLock<Value>>,
    pub lidar_display_cloud: Arc<RwLock<Value>>,
    pub lidar_service: Arc<RwLock<Value>>,
    pub scan_state: Arc<RwLock<Value>>,
    pub pose: Arc<RwLock<Value>>,
    pub nav_grid: Arc<RwLock<Value>>,
    pub autonomy: Arc<RwLock<Value>>,
    pub mission: Arc<RwLock<Value>>,
    pub perception: Arc<RwLock<Value>>,
    pub manual: Arc<std::sync::Mutex<digital_twin_dashboard::manual::ManualHold>>,
    pub scans: Arc<RwLock<digital_twin_dashboard::scan_store::ScanStore>>,
    pub scan_sync_lock: Arc<tokio::sync::Mutex<()>>,
    pub o2: Arc<std::sync::Mutex<digital_twin_dashboard::o2_estimate::O2Estimator>>,
    pub sensors: Arc<std::sync::Mutex<SensorLog>>,
    pub device: Arc<RwLock<mqtt::DeviceState>>,
    pub rx: broadcast::Receiver<Value>,
    pub dir: PathBuf,
}

/// Ingest-level fixture: call `mqtt::on_mqtt_message(&f.ctx, ...)` directly,
/// then inspect `f.*` and drain `f.rx`. No broker, no TCP.
pub fn fixture(tag: &str) -> Fixture {
    let dir = tmpdir(tag);
    // Point file-backed state at this test's temp dir. Process-global, so a
    // parallel test may redirect a write into its own temp dir instead — both
    // are cleaned up, and no test ever READS these files, so it cannot affect
    // assertions. The repo-root files stay untouched.
    std::env::set_var("ROVER_CONFIG", dir.join("rover_config.json"));
    std::env::set_var("ROVER_CALIBRATION", dir.join("calibration_baseline.json"));
    std::env::set_var("ROVER_SENSOR_BASELINE", dir.join("sensor_baseline.json"));
    // Static pages resolve through base_dir: pin it at the package root so
    // build_router serves the real web/ tree deterministically.
    std::env::set_var("ROVER_HOME", env!("CARGO_MANIFEST_DIR"));
    let (bcast, rx) = broadcast::channel(256);
    let data = Arc::new(RwLock::new(DataHandler::default()));
    let history = Arc::new(RwLock::new(HistoryStore::new(&dir)));
    let cal = Arc::new(RwLock::new(CalibrationManager::new()));
    let motor_status = Arc::new(RwLock::new(Value::Object(Default::default())));
    let env = Arc::new(RwLock::new(Value::Null));
    let imu = Arc::new(RwLock::new(Value::Null));
    let servo = Arc::new(RwLock::new(Value::Null));
    let lidar = Arc::new(RwLock::new(Value::Null));
    let lidar_cloud = Arc::new(RwLock::new(Value::Null));
    let lidar_display_cloud = Arc::new(RwLock::new(Value::Null));
    let lidar_service = Arc::new(RwLock::new(Value::Null));
    let scan_state = Arc::new(RwLock::new(Value::Null));
    let pose = Arc::new(RwLock::new(Value::Null));
    let nav_grid = Arc::new(RwLock::new(Value::Null));
    let autonomy = Arc::new(RwLock::new(Value::Null));
    let mission = Arc::new(RwLock::new(Value::Null));
    let perception = Arc::new(RwLock::new(Value::Null));
    let manual = Arc::new(std::sync::Mutex::new(
        digital_twin_dashboard::manual::ManualHold::default(),
    ));
    let scans = Arc::new(RwLock::new(
        digital_twin_dashboard::scan_store::ScanStore::new(dir.join("scans")),
    ));
    let cfg = Arc::new(RwLock::new(AppConfig {
        auth_token: TEST_TOKEN.into(),
        ..AppConfig::default()
    }));
    let scan_sync_lock = Arc::new(tokio::sync::Mutex::new(()));
    let o2 = Arc::new(std::sync::Mutex::new(
        digital_twin_dashboard::o2_estimate::O2Estimator::new(),
    ));
    let sensors = Arc::new(std::sync::Mutex::new(SensorLog::default()));
    let device = Arc::new(RwLock::new(mqtt::DeviceState::default()));
    let ctx = mqtt::Ctx {
        data: data.clone(),
        history: history.clone(),
        cal: cal.clone(),
        motor_status: motor_status.clone(),
        env: env.clone(),
        imu: imu.clone(),
        servo: servo.clone(),
        lidar: lidar.clone(),
        lidar_cloud: lidar_cloud.clone(),
        lidar_display_cloud: lidar_display_cloud.clone(),
        lidar_service: lidar_service.clone(),
        scan_state: scan_state.clone(),
        pose: pose.clone(),
        nav_grid: nav_grid.clone(),
        autonomy: autonomy.clone(),
        mission: mission.clone(),
        perception: perception.clone(),
        manual: manual.clone(),
        scans: scans.clone(),
        cfg: cfg.clone(),
        scan_sync_lock: scan_sync_lock.clone(),
        o2: o2.clone(),
        sensors: sensors.clone(),
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
        env,
        imu,
        servo,
        lidar,
        lidar_cloud,
        lidar_display_cloud,
        lidar_service,
        scan_state,
        pose,
        nav_grid,
        autonomy,
        mission,
        perception,
        manual,
        scans,
        scan_sync_lock,
        o2,
        sensors,
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
    let state = AppState {
        data: f.data,
        history: f.history,
        cal: f.cal,
        clients: Arc::new(AtomicUsize::new(0)),
        motor_status: f.motor_status,
        env: f.env,
        imu: f.imu,
        servo: f.servo,
        lidar: f.lidar,
        lidar_cloud: f.lidar_cloud,
        lidar_display_cloud: f.lidar_display_cloud,
        lidar_service: f.lidar_service,
        scan_state: f.scan_state,
        pose: f.pose,
        nav_grid: f.nav_grid,
        autonomy: f.autonomy,
        mission: f.mission,
        perception: f.perception,
        manual: f.manual,
        scans: f.scans,
        scan_sync_lock: f.scan_sync_lock,
        o2: f.o2,
        bench: digital_twin_dashboard::bench::new_bench(),
        sensors: f.sensors,
        device: f.device,
        mqtt: Arc::new(RwLock::new(slot)),
        bcast: f.ctx.bcast.clone(),
        cfg: f.ctx.cfg.clone(),
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
