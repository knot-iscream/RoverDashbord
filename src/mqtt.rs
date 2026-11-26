//! Port of `backend/mqtt_handler.py` + `server.py::on_mqtt_message` ingest.
//! rumqttc async client (auto-reconnect), topic routing, device presence,
//! history gating, and WS fan-out. Firmware contract: see firmware skill —
//! Rust matches the ESP32, never the reverse.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::{json, Value};
use tokio::task::JoinHandle;

use crate::calibration::{CalState, CalibrationManager};
use crate::data::DataHandler;
use crate::history::HistoryStore;
use crate::sensor_log::SensorLog;

pub const DEVICE_OFFLINE_TIMEOUT_S: i64 = 8;
/// `rover/env/#` covers the ENV board's telemetry + heartbeat; its command
/// echoes are dropped by the `*/command` guard below, same as motor commands.
/// `rover/lidar/servo` is the swing arm state: commanded angle and sweep
/// flags, no measured angle because no encoder is fitted.
const SUB_TOPICS: [&str; 17] = [
    "rover/motor/#",
    "rover/mid/#",
    "rover/calibration/status",
    "rover/imu",
    "rover/env/#",
    "rover/lidar/servo",
    // Pi-side RPLIDAR A1 bridge, one full 360 degree turn per frame.
    "rover/lidar/scan",
    // ROS-built 3D cloud: [[x, y, z], ...] in metres.
    "rover/lidar/cloud",
    // OctoMap voxel centres: a lighter surface feed for the dashboard.
    "rover/lidar/display_cloud",
    // Pi agent state: container running, mapping loop, USB presence.
    "rover/lidar/service/status",
    // Auto-scan state and one-shot "session saved" events.
    "rover/scan/state",
    "rover/scan/saved",
    // Autonomy feeds: pose and 2D grid from the ROS bridge, state from the
    // Pi autopilot. Read-only for this backend.
    "rover/pose",
    "rover/map/grid",
    "rover/autonomy/state",
    "rover/mission/state",
    // Camera detector boxes from the Pi perception service.
    "rover/perception/detections",
];

/// The shared pieces the ingest loop needs. `AppState` (main.rs) builds one
/// via `ctx()`; this breaks the state-needs-slot / slot-task-needs-state cycle.
#[derive(Clone)]
pub struct Ctx {
    pub data: Arc<tokio::sync::RwLock<DataHandler>>,
    pub history: Arc<tokio::sync::RwLock<HistoryStore>>,
    pub cal: Arc<tokio::sync::RwLock<CalibrationManager>>,
    pub motor_status: Arc<tokio::sync::RwLock<Value>>,
    /// Latest raw frame per new feed; `Value::Null` until the first frame.
    pub env: Arc<tokio::sync::RwLock<Value>>,
    pub imu: Arc<tokio::sync::RwLock<Value>>,
    pub servo: Arc<tokio::sync::RwLock<Value>>,
    pub lidar: Arc<tokio::sync::RwLock<Value>>,
    pub lidar_cloud: Arc<tokio::sync::RwLock<Value>>,
    /// OctoMap voxel centres for the app display feed. Same frame shape as
    /// `lidar_cloud`, but much smaller; null until the bridge publishes one.
    pub lidar_display_cloud: Arc<tokio::sync::RwLock<Value>>,
    /// Pi-side lidar agent status: container, mapping loop, USB presence.
    pub lidar_service: Arc<tokio::sync::RwLock<Value>>,
    /// Auto-scan state from the Pi sweep loop (retained).
    pub scan_state: Arc<tokio::sync::RwLock<Value>>,
    /// Latest pose from the Pi (`rover/pose`, map frame).
    pub pose: Arc<tokio::sync::RwLock<Value>>,
    /// Occupancy grid from the ROS bridge for waypoint planning.
    pub nav_grid: Arc<tokio::sync::RwLock<Value>>,
    /// Autonomy state from the Pi autopilot.
    pub autonomy: Arc<tokio::sync::RwLock<Value>>,
    /// Last mission state (retained by the autopilot).
    pub mission: Arc<tokio::sync::RwLock<Value>>,
    /// Latest camera detections from the Pi perception service.
    pub perception: Arc<tokio::sync::RwLock<Value>>,
    /// Scan history archive on this laptop.
    pub scans: Arc<tokio::sync::RwLock<crate::scan_store::ScanStore>>,
    /// Backend config, needed for the Pi HTTP token during scan sync.
    pub cfg: Arc<tokio::sync::RwLock<crate::config::AppConfig>>,
    /// Serializes Pi scan syncs; two overlapping fetches must not race on the
    /// same temporary folder.
    pub scan_sync_lock: Arc<tokio::sync::Mutex<()>>,
    /// Derived O2 filter for the ENV node. Brief std::sync lock, no await.
    pub o2: Arc<std::sync::Mutex<crate::o2_estimate::O2Estimator>>,
    /// Rolling ENV + IMU logs behind the Detailed charts and the Excel
    /// export. Brief std::sync locks only; never held across an await.
    pub sensors: Arc<std::sync::Mutex<SensorLog>>,
    /// Last manual duty per motor; republished by the hold task.
    pub manual: Arc<std::sync::Mutex<crate::manual::ManualHold>>,
    pub device: Arc<tokio::sync::RwLock<DeviceState>>,
    pub bcast: tokio::sync::broadcast::Sender<Value>,
}

/// ESP32 presence. `last_seen` = wall-clock UTC (payload shows `...Z` like Python).
#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    pub last_seen: Option<DateTime<Utc>>,
    pub online: bool,
}

impl DeviceState {
    /// Recompute `online` from `last_seen`; returns true when it flipped.
    pub fn refresh(&mut self) -> bool {
        let was = self.online;
        self.online = self
            .last_seen
            .is_some_and(|t| (Utc::now() - t).num_seconds() < DEVICE_OFFLINE_TIMEOUT_S);
        self.online != was
    }

    pub fn payload(&self) -> Value {
        json!({
            "type": "device_status",
            "online": self.online,
            "last_seen": self.last_seen.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        })
    }
}

/// Cloneable publish handle; the poll task lives in `MqttSlot::task`.
#[derive(Clone)]
pub struct MqttHandle {
    pub client: AsyncClient,
    pub connected: Arc<AtomicBool>,
    pub broker: String,
}

impl MqttHandle {
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// 1:1 with `MQTTClient.publish`: false when offline, else publish + bool.
    pub async fn publish(&self, topic: &str, payload: &Value) -> bool {
        if !self.is_connected() {
            return false;
        }
        let text = payload.to_string();
        self.client
            .publish(topic, QoS::AtMostOnce, false, text)
            .await
            .is_ok()
    }

    /// Publish a retained setting, so a restarted Pi loop sees the last value.
    pub async fn publish_retained(&self, topic: &str, payload: &Value) -> bool {
        if !self.is_connected() {
            return false;
        }
        self.client
            .publish(topic, QoS::AtMostOnce, true, payload.to_string())
            .await
            .is_ok()
    }
}

pub struct MqttSlot {
    pub handle: MqttHandle,
    pub task: JoinHandle<()>,
}

/// Start the client + poll loop. rumqttc reconnects on its own (replaces
/// paho `connect_async` + `reconnect_delay_set(1,30)`); every `ConnAck`
/// re-subscribes like Python's `_on_connect`.
pub async fn spawn_mqtt(ctx: Ctx, broker: &str, port: u16) -> MqttSlot {
    // Unique per process: two backends sharing one client id kick each other
    // off the broker in a reconnect loop (paho's default is broker-assigned).
    let client_id = format!("rover-backend-rust-{}", std::process::id());
    let mut opts = MqttOptions::new(client_id, broker, port);
    // 15s, not Python's 60s: narrows the window where /api/health can report
    // a stale `mqtt_connected: true` after a silent TCP drop.
    opts.set_keep_alive(Duration::from_secs(15));
    // Lidar scan (~20 KB) and ROS cloud can be several MB after the tilt-ring
    // budget was raised. rumqttc's small default triggers a reconnect loop.
    opts.set_max_packet_size(16 * 1024 * 1024, 16 * 1024 * 1024);
    let (client, mut eventloop) = AsyncClient::new(opts, 256);
    let handle = MqttHandle {
        client: client.clone(),
        connected: Arc::new(AtomicBool::new(false)),
        broker: format!("{broker}:{port}"),
    };
    let task_ctx = ctx.clone();
    let task_handle = handle.clone();
    let task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    task_handle.connected.store(true, Ordering::SeqCst);
                    // A reconnect must never resume an old manual duty. Clear
                    // the hold and publish a zero before anything new arrives.
                    task_ctx.manual.lock().unwrap().clear();
                    let zero = "{\"motor\":0,\"speed\":0}".to_string();
                    let _ = task_handle
                        .client
                        .publish("rover/motor/command", QoS::AtMostOnce, false, zero.clone())
                        .await;
                    let _ = task_handle
                        .client
                        .publish("rover/mid/command", QoS::AtMostOnce, false, zero)
                        .await;
                    for t in SUB_TOPICS {
                        let _ = task_handle.client.subscribe(t, QoS::AtMostOnce).await;
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    match serde_json::from_slice::<Value>(&p.payload) {
                        Ok(body) => on_mqtt_message(&task_ctx, &p.topic, body).await,
                        Err(e) => eprintln!("[MQTT] Invalid JSON on {}: {e}", p.topic),
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[MQTT] event error: {e}");
                    task_handle.connected.store(false, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
    MqttSlot { handle, task }
}

/// Topics published by the Pi-side processes, not by an ESP32. They must not
/// refresh the ESP presence flag, or the dashboard reports a dead board as
/// online for as long as the autopilot keeps talking.
fn is_pi_topic(topic: &str) -> bool {
    topic.starts_with("rover/pose")
        || topic.starts_with("rover/map/")
        || topic.starts_with("rover/autonomy/")
        || topic.starts_with("rover/mission/")
        || topic.starts_with("rover/control/")
        || topic.starts_with("rover/perception/")
}

/// 1:1 with `server.py::on_mqtt_message`.
pub async fn on_mqtt_message(ctx: &Ctx, topic: &str, mut data: Value) {
    // Any rover message proves the ESP32 is alive — except our own echoes on
    // `*/command` (we subscribe to `rover/motor/#` too) and the Pi-side
    // autonomy feeds.
    if !topic.ends_with("/command") && !is_pi_topic(topic) {
        let mut dev = ctx.device.write().await;
        let now_online = dev
            .last_seen
            .is_none_or(|t| (Utc::now() - t).num_seconds() >= DEVICE_OFFLINE_TIMEOUT_S);
        dev.last_seen = Some(Utc::now());
        if now_online {
            dev.online = true;
            drop(dev);
            broadcast(ctx, ctx.device.read().await.payload());
        }
    }

    // Our own command echoes (`rover/motor/command` matches our own
    // `rover/motor/#` subscription): stop here. Falling through would treat
    // the echo as telemetry — overwriting the motor's live sample with
    // sensor-less data and writing a junk history row. v1 had this defect;
    // 2.0 deliberately diverges (documented in README).
    if topic.ends_with("/command") {
        return;
    }

    // ── Calibration status from ESP32 ──
    if topic.starts_with("rover/calibration/status") {
        ctx.cal.write().await.handle_esp32_status(&data);
        broadcast(ctx, calibration_payload(ctx).await);
        return;
    }

    // ── Motor driver status from ESP32 (merge keys, like `dict.update`) ──
    if topic.starts_with("rover/motor/status") {
        if let Value::Object(ref mut cache) = *ctx.motor_status.write().await {
            if let Value::Object(map) = data {
                cache.extend(map);
                data = Value::Object(cache.clone());
            }
        }
        let mut payload = json!({"type": "motor_status"});
        if let Value::Object(map) = data {
            if let Value::Object(out) = &mut payload {
                out.extend(map);
            }
        }
        broadcast(ctx, payload);
        return;
    }

    // ── MID driver status (ESP-2, blind middles): broadcast with source
    // tag so pages can tell it apart; never merged into the corners cache.
    if topic.starts_with("rover/mid/status") {
        let mut payload = json!({"type": "motor_status", "source": "mid"});
        if let Value::Object(map) = data {
            if let Value::Object(out) = &mut payload {
                out.extend(map);
            }
        }
        broadcast(ctx, payload);
        return;
    }

    // ── Localization: IMU frames (ESP-2, own topic) ──
    // Never motor data: falling through would file an IMU frame as a
    // sensor-less motor sample and overwrite live corner telemetry.
    if topic.starts_with("rover/imu") {
        *ctx.imu.write().await = data.clone();
        ctx.sensors.lock().unwrap().push_imu(data.clone());
        broadcast(ctx, tagged("imu_update", data));
        return;
    }

    // ── ENV node (ESP-3): telemetry frame (1 Hz) ──
    if topic.starts_with("rover/env/telemetry") {
        // No galvanic O2 cell on the board. Derive it here, once, before the
        // frame is stored: /api/env, the WS relay, the chart ring, the export
        // and the alarm verdict then all share the same numbers.
        let data = crate::o2_estimate::apply(&ctx.o2, data);
        *ctx.env.write().await = data.clone();
        ctx.sensors.lock().unwrap().push_env(data.clone());
        broadcast(ctx, env_update_payload(data));
        return;
    }

    // ── RPLIDAR A1 scan (Pi bridge): one whole 360 turn per frame ──
    // The sensor reports its own angle, so no yaw is invented here. Keep the
    // raw JSON intact: /api/lidar and the WS replay read the same frame.
    if topic.starts_with("rover/lidar/scan") {
        *ctx.lidar.write().await = data.clone();
        broadcast(ctx, tagged("lidar_update", data));
        return;
    }

    // ── ROS point cloud (Pi container): already transformed into 3D ──
    if topic.starts_with("rover/lidar/cloud") {
        // Raw cloud stays available on /api/lidar/cloud for recording and
        // RViz, but it is several MB and is not fanned out to pages: the
        // OctoMap display feed below is the WS payload.
        *ctx.lidar_cloud.write().await = data.clone();
        return;
    }

    // OctoMap surface feed: the dashboard paints this instead of the raw
    // cloud when it is available. Raw stays on `lidar_cloud` for recording.
    if topic.starts_with("rover/lidar/display_cloud") {
        *ctx.lidar_display_cloud.write().await = data.clone();
        broadcast(ctx, tagged("lidar_display_update", data));
        return;
    }

    if topic.starts_with("rover/lidar/service/status") {
        *ctx.lidar_service.write().await = data.clone();
        broadcast(ctx, tagged("lidar_service_update", data));
        return;
    }

    // Auto-scan state from the Pi sweep loop. Retained, so a page that loads
    // between edits still sees armed / scanning / paused.
    if topic.starts_with("rover/scan/state") {
        *ctx.scan_state.write().await = data.clone();
        broadcast(ctx, tagged("scan_state_update", data));
        return;
    }

    // ── Autonomy feeds (Pi autopilot / ROS bridge) ──
    if topic.starts_with("rover/pose") {
        *ctx.pose.write().await = data.clone();
        broadcast(ctx, tagged("pose_update", data));
        return;
    }
    if topic.starts_with("rover/map/grid") {
        // Cache only: the grid is tens of KB and no page needs it live at
        // 1 Hz. `/api/nav/grid` serves it on demand.
        *ctx.nav_grid.write().await = data;
        return;
    }
    if topic.starts_with("rover/autonomy/state") {
        *ctx.autonomy.write().await = data.clone();
        broadcast(ctx, tagged("autonomy_update", data));
        return;
    }
    if topic.starts_with("rover/mission/state") {
        *ctx.mission.write().await = data.clone();
        broadcast(ctx, tagged("mission_update", data));
        return;
    }
    if topic.starts_with("rover/perception/detections") {
        *ctx.perception.write().await = data.clone();
        broadcast(ctx, tagged("perception_update", data));
        return;
    }

    // A save finished on the Pi. Relay the event, then pull the bundle into
    // the laptop archive in the background.
    if topic.starts_with("rover/scan/saved") {
        broadcast(ctx, tagged("scan_saved", data));
        let c = ctx.clone();
        tokio::spawn(async move { sync_scans(&c).await });
        return;
    }

    // ── Swing arm servo (ESP-2): state only. No angle encoder is fitted,
    // so cal_ok is always 0 and there is no stored reference to clear. ──
    if topic.starts_with("rover/lidar/servo") {
        *ctx.servo.write().await = data.clone();
        broadcast(ctx, tagged("servo_update", data));
        return;
    }

    // ── ENV heartbeat (2 s). Presence was refreshed above; this relay keeps
    // the ENV board's own state (burn-in counter, baseline flag) visible, and
    // it is the only source for fire/O2/gas liveness.
    if topic.starts_with("rover/env/status") {
        broadcast(ctx, tagged("env_status", data));
        return;
    }

    // ── Motor telemetry ──
    // ESP-2 publishes mid motor 1-2; remap to backend ids 5-6 so they can
    // never collide with the four corners on rover/motor/1..4.
    if topic.starts_with("rover/mid/") {
        let remapped = match data
            .get("motor")
            .or_else(|| data.get("id"))
            .and_then(|m| m.as_i64())
        {
            Some(1) => json!(5),
            Some(2) => json!(6),
            _ => data
                .get("motor")
                .or_else(|| data.get("id"))
                .cloned()
                .unwrap_or(json!(0)),
        };
        if let Value::Object(map) = &mut data {
            map.insert("motor".into(), remapped);
        }
    }

    // Last gate before motor telemetry: ONLY motor topics may pass. A topic we
    // do not know yet (a typo, or a new board added without its own branch)
    // gets dropped with a log line instead of being filed as a motor sample
    // with made-up sensor values. Without this, adding a topic and forgetting
    // the branch above silently corrupts the dashboard and its history.
    if !topic.starts_with("rover/motor/") && !topic.starts_with("rover/mid/") {
        tracing::warn!("unknown topic ignored (not motor data): {topic}");
        return;
    }

    let running = crate::data::coerce_i64(data.get("speed")) != 0;
    let cal_state = ctx.cal.read().await.state.clone();
    let calibrating = cal_state == CalState::Warmup || cal_state == CalState::Sweep;

    // History only when the rover is doing something real (never idle junk).
    if running || calibrating {
        ctx.history.write().await.add_sample(&data);
    }
    if calibrating {
        ctx.cal.write().await.count_sample();
    }

    let health = ctx.data.write().await.process_motor_data(&data);
    let motor = data
        .get("motor")
        .or_else(|| data.get("id"))
        .cloned()
        .unwrap_or(json!(0));
    let ina_ok = py_bool(&data.get("ina_ok"));
    let vib_valid = py_bool(
        &data
            .get("vibration_valid")
            .or_else(|| data.get("vib_valid")),
    );
    let temp_valid = py_bool(&data.get("temp_valid"));

    let mut payload = json!({
        "type": "motor_update",
        "motor": motor,
        "vibration": data.get("vibration").cloned().unwrap_or(json!(0)),
        "vibration_valid": vib_valid,
        "ina_ok": ina_ok,
        "temp_valid": temp_valid,
        "voltage": if ina_ok { data.get("voltage").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "current": if ina_ok { data.get("current").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "temp": if temp_valid { data.get("temp").or_else(|| data.get("temperature")).cloned().unwrap_or(Value::Null) } else { Value::Null },
        "speed": data.get("speed").cloned().unwrap_or(json!(0)),
        "vib_mag": data.get("vib_mag").cloned().unwrap_or(Value::Null),
        "ax": data.get("ax").cloned().unwrap_or(Value::Null),
        "ay": data.get("ay").cloned().unwrap_or(Value::Null),
        "az": data.get("az").cloned().unwrap_or(Value::Null),
        "health": health.health,
        "anomalies": health.anomalies,
    });
    // The pump rides the MID frames only; corners do not send it.
    if let Some(p) = data.get("pump") {
        if let Value::Object(map) = &mut payload {
            map.insert("pump".into(), p.clone());
        }
    }
    broadcast(ctx, payload);
}

/// `{"type": kind, **frame}`: merges a raw payload object into a typed
/// envelope, the same shape the motor/mid relays already use.
fn tagged(kind: &str, data: Value) -> Value {
    let mut payload = json!({"type": kind});
    if let Value::Object(map) = data {
        if let Value::Object(out) = &mut payload {
            out.extend(map);
        }
    }
    payload
}

/// `env_update` = the raw frame plus the indicative alarm verdict, so the UI
/// colors from the exact math `GET /api/env` serves instead of a second copy
/// of the thresholds. The WS replay in `ws.rs` reuses this verbatim.
pub fn env_update_payload(data: Value) -> Value {
    let alarms = crate::env_alarm::evaluate(&data);
    let mut payload = tagged("env_update", data);
    if let Value::Object(out) = &mut payload {
        out.insert("alarms".into(), alarms);
    }
    payload
}

fn py_bool(v: &Option<&Value>) -> bool {
    crate::data::py_bool(*v)
}

// ── Captured sensor references ──
// The app captures these itself, by averaging the frames it already holds
// (`sensor_cal`), so there is no passive watcher here. Board-side captures
// are requested by publishing a command from the capture route.

/// `{"type":"calibration_status", **status}` like `broadcast_calibration_status`.
pub async fn calibration_payload(ctx: &Ctx) -> Value {
    let mut v = status_value(ctx).await;
    if let Value::Object(out) = &mut v {
        out.insert("type".into(), json!("calibration_status"));
    }
    v
}

/// Bare status object for `GET /api/calibration/status` (no `type` key, like Python).
/// Takes the write lock on purpose: `status()` auto-advances warmup→sweep,
/// so a read lock would serve a stale state. Held for microseconds.
pub async fn status_value(ctx: &Ctx) -> Value {
    let st = ctx.cal.write().await.status();
    serde_json::to_value(&st).unwrap_or(json!({}))
}

pub fn broadcast(ctx: &Ctx, payload: Value) {
    let _ = ctx.bcast.send(payload);
}

/// Pull any scan session the Pi has but this laptop does not. Called after a
/// `rover/scan/saved` event and by the startup/periodic sync task.
pub async fn sync_scans(ctx: &Ctx) {
    let _guard = ctx.scan_sync_lock.lock().await;
    let url = ctx
        .lidar_service
        .read()
        .await
        .get("http_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return;
    }
    let token = ctx.cfg.read().await.auth_token.clone();
    match crate::scan_store::sync_from_pi(&ctx.scans, &url, &token).await {
        Ok(_) => {
            let list = ctx.scans.read().await.list();
            broadcast(ctx, json!({"type": "scan_list_update", "scans": list}));
        }
        Err(e) => broadcast(ctx, json!({"type": "scan_sync_error", "message": e})),
    }
}

/// Background loop: broadcast `device_status` only when the state flips.
pub async fn monitor_device(ctx: Ctx) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let changed = ctx.device.write().await.refresh();
        if changed {
            broadcast(&ctx, ctx.device.read().await.payload());
        }
    }
}

/// Republish the last manual duty while a dashboard is connected. The
/// firmware stops the rover if this loop dies, the backend dies, or the
/// broker drops. It never runs without a WS client, so closing the Control
/// page also stops a held command.
pub async fn manual_hold_loop(state: crate::AppState) {
    let mut iv = tokio::time::interval(Duration::from_millis(200));
    loop {
        iv.tick().await;
        if state.clients.load(Ordering::SeqCst) == 0 {
            state.manual.lock().unwrap().clear();
            continue;
        }
        let Some(speeds) = state.manual.lock().unwrap().snapshot() else {
            continue;
        };
        let handle = state.mqtt.read().await.handle.clone();
        if !handle.is_connected() {
            continue;
        }
        for (i, speed) in speeds.iter().enumerate() {
            if *speed == 0 {
                continue;
            }
            let (topic, motor) = if i < 4 {
                ("rover/motor/command", i as i64 + 1)
            } else {
                ("rover/mid/command", i as i64 - 3)
            };
            let _ = handle
                .publish(
                    topic,
                    &json!({
                        "motor": motor,
                        "speed": speed,
                        "arm_ms": crate::manual::MANUAL_ARM_MS,
                    }),
                )
                .await;
        }
    }
}

/// Publish the operator heartbeat. The Pi autopilot treats a few seconds of
/// silence as a hazard signal and returns home, so this runs for as long as
/// the app is up, broker or not (the publish is skipped while offline).
pub async fn heartbeat_loop(state: crate::AppState) {
    let mut iv = tokio::time::interval(Duration::from_secs(1));
    loop {
        iv.tick().await;
        let handle = state.mqtt.read().await.handle.clone();
        if handle.is_connected() {
            let _ = handle
                .publish(
                    "rover/control/heartbeat",
                    &json!({
                        "role": "operator",
                        "ts": chrono::Utc::now().timestamp_millis(),
                    }),
                )
                .await;
        }
    }
}
