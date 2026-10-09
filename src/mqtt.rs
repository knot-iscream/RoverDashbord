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

pub const DEVICE_OFFLINE_TIMEOUT_S: i64 = 8;
const SUB_TOPICS: [&str; 2] = ["rover/motor/#", "rover/calibration/status"];

/// The shared pieces the ingest loop needs. `AppState` (main.rs) builds one
/// via `ctx()`; this breaks the state-needs-slot / slot-task-needs-state cycle.
#[derive(Clone)]
pub struct Ctx {
    pub data: Arc<tokio::sync::RwLock<DataHandler>>,
    pub history: Arc<tokio::sync::RwLock<HistoryStore>>,
    pub cal: Arc<tokio::sync::RwLock<CalibrationManager>>,
    pub motor_status: Arc<tokio::sync::RwLock<Value>>,
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
    let (client, mut eventloop) = AsyncClient::new(opts, 16);
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
                Err(_) => {
                    task_handle.connected.store(false, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
    MqttSlot { handle, task }
}

/// 1:1 with `server.py::on_mqtt_message`.
pub async fn on_mqtt_message(ctx: &Ctx, topic: &str, mut data: Value) {
    // Any rover message proves the ESP32 is alive — except our own echoes on
    // `*/command` (we subscribe to `rover/motor/#` too).
    if !topic.ends_with("/command") {
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

    // ── Motor telemetry ──
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

    broadcast(
        ctx,
        json!({
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
            "health": health.health,
            "anomalies": health.anomalies,
        }),
    );
}

fn py_bool(v: &Option<&Value>) -> bool {
    crate::data::py_bool(*v)
}

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
