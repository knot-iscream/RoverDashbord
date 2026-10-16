//! MQTT ingest routing tests: `on_mqtt_message` called directly (no broker),
//! asserting topic routing, history gating, presence flips, and the B1/B2
//! regression pins. Broadcasts are collected from the shared channel.

mod common;

use digital_twin_dashboard::mqtt;
use serde_json::json;

fn pkt(motor: i64, speed: i64) -> serde_json::Value {
    json!({"motor": motor, "speed": speed, "temp": 40.0, "temp_valid": true,
           "voltage": 12.0, "current": 0.5, "ina_ok": true,
           "vibration": 0, "vibration_valid": true})
}

async fn history_rows(f: &common::Fixture) -> Vec<serde_json::Value> {
    let store = f.history.read().await;
    let days = store.days();
    assert!(days.len() <= 1);
    days.first().map(|d| store.read_day(d)).unwrap_or_default()
}

#[tokio::test]
async fn telemetry_routes_history_and_fanout() {
    let mut f = common::fixture("mqtt-full");
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 128)).await;
    // Presence flip (fresh device) + motor_update, in that order.
    let d = common::drain(&mut f.rx);
    assert_eq!(d.len(), 2);
    assert_eq!(d[0]["type"], "device_status");
    assert_eq!(d[0]["online"], true);
    assert_eq!(d[1]["type"], "motor_update");
    assert_eq!(d[1]["motor"], 1);
    assert_eq!(d[1]["health"], 100.0);
    // History written (running) and latest cached.
    assert_eq!(history_rows(&f).await.len(), 1);
    assert!(f.data.read().await.latest.contains_key(&1));
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn idle_packets_skip_history_but_still_broadcast() {
    let mut f = common::fixture("mqtt-idle");
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 0)).await;
    let d = common::drain(&mut f.rx);
    assert!(d.iter().any(|m| m["type"] == "motor_update"));
    assert!(
        history_rows(&f).await.is_empty(),
        "idle junk must not be recorded"
    );
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn calibrating_records_idle_and_counts() {
    let f = common::fixture("mqtt-cal");
    f.cal.write().await.start();
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 0)).await;
    assert_eq!(history_rows(&f).await.len(), 1);
    assert_eq!(f.cal.read().await.samples, 1);
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn command_echo_is_fully_ignored() {
    // B1 regression: our own `rover/motor/command` echo (which matches our
    // `rover/motor/#` subscription) must not touch presence, history, the
    // latest cache, or the broadcast channel.
    let mut f = common::fixture("mqtt-echo");
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 128)).await;
    let before = f.data.read().await.latest.get(&1).cloned().unwrap();
    let seen_before = f.device.read().await.last_seen;
    let _ = common::drain(&mut f.rx);
    mqtt::on_mqtt_message(
        &f.ctx,
        "rover/motor/command",
        json!({"motor": 1, "speed": 200}),
    )
    .await;
    assert!(
        common::drain(&mut f.rx).is_empty(),
        "echo must not broadcast"
    );
    assert_eq!(
        history_rows(&f).await.len(),
        1,
        "echo must not append history"
    );
    assert_eq!(
        f.data
            .read()
            .await
            .latest
            .get(&1)
            .cloned()
            .unwrap()
            .timestamp,
        before.timestamp,
        "echo must not overwrite the live sample"
    );
    assert_eq!(
        f.device.read().await.last_seen,
        seen_before,
        "echo must not refresh presence"
    );
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn calibration_status_relayed() {
    let mut f = common::fixture("mqtt-calstat");
    mqtt::on_mqtt_message(
        &f.ctx,
        "rover/calibration/status",
        json!({"state": "sweep", "speed_pct": 57, "dir": 1, "step_remaining_s": 45, "cycle": 3}),
    )
    .await;
    assert_eq!(
        f.cal.read().await.state,
        digital_twin_dashboard::calibration::CalState::Sweep
    );
    let d = common::drain(&mut f.rx);
    let cal = d
        .iter()
        .find(|m| m["type"] == "calibration_status")
        .expect("relay broadcast");
    assert_eq!(cal["speed_pct"], 57);
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn motor_status_merges_keys() {
    let f = common::fixture("mqtt-mstat");
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/status", json!({"a": 1})).await;
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/status", json!({"b": 2})).await;
    let cache = f.motor_status.read().await.clone();
    assert_eq!(cache["a"], 1);
    assert_eq!(cache["b"], 2);
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn float_speed_counts_as_running() {
    // B2 regression: serde `as_i64` reads 128.0 as missing → 0; the shared
    // coercion must treat it as running like Python's `int(x or 0)`.
    let f = common::fixture("mqtt-float");
    let mut p = pkt(1, 0);
    p["speed"] = json!(128.0);
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", p).await;
    assert_eq!(history_rows(&f).await.len(), 1);
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn second_message_within_window_broadcasts_no_presence() {
    let mut f = common::fixture("mqtt-pres");
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 10)).await;
    let _ = common::drain(&mut f.rx);
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", pkt(1, 10)).await;
    let d = common::drain(&mut f.rx);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["type"], "motor_update");
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn mid_telemetry_remapped_to_5_and_6() {
    // ESP-2 publishes mid motor 1-2; backend ids must be 5-6 so they can
    // never collide with the four corners.
    let mut f = common::fixture("mqtt-mid");
    mqtt::on_mqtt_message(&f.ctx, "rover/mid/1", pkt(1, 128)).await;
    mqtt::on_mqtt_message(&f.ctx, "rover/mid/2", pkt(2, 0)).await;
    let cache = f.data.read().await;
    assert!(cache.latest.contains_key(&5), "mid 1 -> motor 5");
    assert!(cache.latest.contains_key(&6), "mid 2 -> motor 6");
    assert!(!cache.latest.contains_key(&1), "corners untouched by mid");
    drop(cache);
    let d = common::drain(&mut f.rx);
    let updates: Vec<_> = d.iter().filter(|m| m["type"] == "motor_update").collect();
    assert_eq!(updates.len(), 2);
    assert!(updates.iter().any(|m| m["motor"] == 5));
    assert!(updates.iter().any(|m| m["motor"] == 6));
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn mid_status_broadcast_with_source_not_merged() {
    // MID status must not pollute the corners motor_status cache.
    let mut f = common::fixture("mqtt-midstat");
    mqtt::on_mqtt_message(&f.ctx, "rover/mid/status", json!({"speed": [10, 20]})).await;
    assert!(
        f.motor_status.read().await.get("speed").is_none(),
        "corners cache untouched"
    );
    let d = common::drain(&mut f.rx);
    let st = d
        .iter()
        .find(|m| m["type"] == "motor_status")
        .expect("mid status broadcast");
    assert_eq!(st["source"], "mid");
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn vib_mag_passes_through_without_changing_health() {
    // Range fields are display-only: health still follows the bool.
    let mut f = common::fixture("mqtt-vibmag");
    let mut p = pkt(1, 128);
    p["vib_mag"] = json!(1.83);
    p["ax"] = json!(0.11);
    p["ay"] = json!(-0.05);
    p["az"] = json!(0.98);
    mqtt::on_mqtt_message(&f.ctx, "rover/motor/1", p).await;
    let s = f.data.read().await.latest.get(&1).cloned().unwrap();
    assert_eq!(s.vib_mag, Some(1.83));
    assert_eq!(s.ax, Some(0.11));
    let d = common::drain(&mut f.rx);
    let u = d
        .iter()
        .find(|m| m["type"] == "motor_update")
        .expect("motor_update");
    assert_eq!(u["vib_mag"], 1.83);
    assert_eq!(u["health"], 100.0, "vibration bool 0 -> no penalty");
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn broker_disconnect_clears_connected_flag() {
    // B6 regression: a broker-initiated DISCONNECT must clear `connected`
    // promptly (Python's `_on_disconnect`), not linger. Verified mechanism:
    // rumqttc v4 rejects an incoming DISCONNECT with `WrongPacket`, so it
    // surfaces through the poll `Err` arm (a dedicated `Packet::Disconnect`
    // arm was tried and proven dead code — it can never fire). Fake broker
    // speaks just enough MQTT: CONNACK, then a v4 DISCONNECT (0xE0 0x00)
    // with the socket held open, so only prompt clearing passes.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        drop(listener); // reconnects refuse; the flag must stay cleared
        let mut buf = [0u8; 256];
        let _ = sock.read(&mut buf).await; // CONNECT, content ignored
        sock.write_all(&[0x20, 0x02, 0x00, 0x00]).await.ok(); // CONNACK ok
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        sock.write_all(&[0xE0, 0x00]).await.ok(); // DISCONNECT, socket stays open
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });

    let f = common::fixture("mqtt-disc");
    let slot = mqtt::spawn_mqtt(f.ctx.clone(), "127.0.0.1", port).await;
    for _ in 0..50 {
        if slot.handle.is_connected() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(slot.handle.is_connected(), "ConnAck should set connected");
    for _ in 0..25 {
        if !slot.handle.is_connected() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        !slot.handle.is_connected(),
        "DISCONNECT must clear connected within 500ms"
    );
    slot.task.abort();
    std::fs::remove_dir_all(&f.dir).ok();
}
