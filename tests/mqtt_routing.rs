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

#[tokio::test]
async fn env_telemetry_stored_and_never_motor_data() {
    // ESP-3 frames must reach the env cache + their own update type, and must
    // NEVER fall through to the motor path (that would file a sensor-less
    // motor sample and overwrite live corner telemetry).
    let mut f = common::fixture("mqtt-env");
    let frame = json!({"ads_ok": 1, "mlx_ok": 1, "flame_ok": 1, "baseline_ok": 1,
                       "co": 1.4, "lpg": 1.0, "ch4": null, "h2": 1.2, "air": 1.1,
                       "o2_mv": 11.8, "o2_pct": 20.6, "flame_lvl": 0.2,
                       "flame_dir": 0, "heat_l": 34.2, "heat_r": 31.7});
    mqtt::on_mqtt_message(&f.ctx, "rover/env/telemetry", frame.clone()).await;
    assert_eq!(*f.env.read().await, frame, "raw frame cached");
    let d = common::drain(&mut f.rx);
    let u = d
        .iter()
        .find(|m| m["type"] == "env_update")
        .expect("env relay");
    assert_eq!(u["co"], 1.4);
    assert_eq!(
        u["alarms"]["level"], "ok",
        "quiet frame carries its verdict"
    );
    assert_eq!(
        f.sensors.lock().unwrap().env_len(),
        1,
        "ring logged the frame"
    );
    assert!(
        f.data.read().await.latest.is_empty(),
        "env frames must not create motor samples"
    );
    assert!(history_rows(&f).await.is_empty());
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn env_nulls_stay_null() {
    // No clean-air baseline yet -> the firmware sends null; the relay must
    // keep it null so the page renders `--` instead of a fake 0.
    let mut f = common::fixture("mqtt-envnull");
    mqtt::on_mqtt_message(
        &f.ctx,
        "rover/env/telemetry",
        json!({"baseline_ok": 0, "co": null, "o2_pct": null}),
    )
    .await;
    let d = common::drain(&mut f.rx);
    let u = d.iter().find(|m| m["type"] == "env_update").expect("relay");
    assert!(u["co"].is_null());
    assert!(u["o2_pct"].is_null());
    assert_eq!(u["alarms"]["level"], "unknown", "nulls never alarm");
    assert_eq!(u["alarms"]["uncalibrated"], true, "baseline 0 marked");
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn env_status_relays_and_command_echo_dropped() {
    let mut f = common::fixture("mqtt-envstat");
    mqtt::on_mqtt_message(
        &f.ctx,
        "rover/env/status",
        json!({"state": "running", "source": "env", "burn_in_s": 7200, "baseline_ok": 1}),
    )
    .await;
    let d = common::drain(&mut f.rx);
    let st = d.iter().find(|m| m["type"] == "env_status").expect("relay");
    assert_eq!(st["burn_in_s"], 7200);
    // Our own baseline command echo must vanish: no broadcast, no presence
    // refresh, no env cache write (same rule as rover/motor/command).
    *f.env.write().await = json!({"keep": 1});
    let seen = f.device.read().await.last_seen;
    mqtt::on_mqtt_message(&f.ctx, "rover/env/command", json!({"action": "baseline"})).await;
    assert!(common::drain(&mut f.rx).is_empty());
    assert_eq!(f.env.read().await["keep"], 1);
    assert_eq!(f.device.read().await.last_seen, seen);
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn unknown_topic_with_motor_payload_never_becomes_a_sample() {
    // Allowlist gate: a topic the backend does not know must not reach the
    // motor path just because its payload looks like telemetry. Presence
    // still flips (`device_status`), nothing else may happen.
    let mut f = common::fixture("mqtt-unknown");
    mqtt::on_mqtt_message(&f.ctx, "rover/foo/bar", pkt(1, 128)).await;
    let d = common::drain(&mut f.rx);
    assert_eq!(d.len(), 1, "presence flip only");
    assert_eq!(d[0]["type"], "device_status");
    assert!(
        f.data.read().await.latest.is_empty(),
        "unknown topic must not cache a motor sample"
    );
    assert!(history_rows(&f).await.is_empty(), "nothing recorded");
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn near_miss_motor_topics_are_dropped() {
    // Topics that merely look like motor topics (plural, extra segment) are
    // not in the allowlist either; all three must stay out of the cache, the
    // history, and the broadcast channel.
    let mut f = common::fixture("mqtt-nearmiss");
    for topic in ["rover/motors/1", "rover/midway/1", "rover/envx/telemetry"] {
        mqtt::on_mqtt_message(&f.ctx, topic, pkt(1, 128)).await;
    }
    let d = common::drain(&mut f.rx);
    assert!(
        d.iter().all(|m| m["type"] != "motor_update"),
        "near-miss topics must not broadcast motor_update"
    );
    assert!(f.data.read().await.latest.is_empty());
    assert!(history_rows(&f).await.is_empty());
    std::fs::remove_dir_all(&f.dir).ok();
}

#[tokio::test]
async fn imu_frames_stored_and_never_motor_data() {
    let mut f = common::fixture("mqtt-imu");
    mqtt::on_mqtt_message(
        &f.ctx,
        "rover/imu",
        json!({"imu_ok": 1, "ax": 0.1, "ay": -0.05, "az": 0.98, "gz": 1.2,
               "yaw": 45.3, "pitch": 0.4, "roll": -0.2}),
    )
    .await;
    assert_eq!(f.imu.read().await["yaw"], 45.3);
    let d = common::drain(&mut f.rx);
    let u = d.iter().find(|m| m["type"] == "imu_update").expect("relay");
    assert_eq!(u["imu_ok"], 1);
    assert!(
        f.data.read().await.latest.is_empty(),
        "imu frames are not motor samples"
    );
    assert!(history_rows(&f).await.is_empty());
    std::fs::remove_dir_all(&f.dir).ok();
}
