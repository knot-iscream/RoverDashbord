//! THROWAWAY — fake ESP32 telemetry for pre-hardware UI checks.
//!
//! Mimics `firmware/ESP32-WROOM-32/` on the wire: `rover/motor/1..4`
//! `{motor,vibration,vib_valid,ina_ok,temp_valid,voltage,current,temp,speed,
//! vib_mag,ax,ay,az}` in 250ms round-robin (each motor ~1/s, like
//! `SAMPLE_INTERVAL_MS`), plus the blind middles on `rover/mid/1..2`
//! (validity 0, null readings — backend remaps to motors 5-6).
//! Bench-idle scenario: speed 0, wandering temp/voltage/current, rare
//! vibration blips, occasional `temp_valid:0` windows to exercise the
//! dashboard `--` gates.
//!
//! DELETE BEFORE HARDWARE DAY. Never ships, never commits to product.
//! Broker: `MQTT_BROKER`/`MQTT_PORT` env, else `rover_config.json` (same
//! file the app reads), else `127.0.0.1:1883`.
//! Also fills the home gas panel and the Detailed sensor sections
//! (`rover/env/telemetry` (1 Hz, gas cells wandering
//! around the clean-air baseline, a flame burst every ~30s), `rover/env/status`
//! (2 s) and `rover/imu` (4 Hz), each from its own task so the rates stay true.
//! `rover/lidar/servo` (1 Hz) fills the swing arm's row on the Calibration
//! page.
//! `FAKE_GAS=1` walks the five gas cells from clean air up past the 3.5 high
//! line on a staggered 25 s ramp, so the bar color ramp (orange -> red) can be
//! checked without hardware.
//! Modes: double-click and all four corners shake continuously
//! bench-idle walk with staggered ~9s offroad bursts every ~18s.
//! `FAKE_SWEEP=1` runs a 120s scripted full-range sweep per motor
//! (nominal -> heat -> meltdown -> recovery, staggered ~7.5s) so HUD
//! fills travel health 100 -> 0 -> 100 end to end.
//! Stop with Ctrl+C (or close the window).

use rand::Rng;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::json;
use std::time::Duration;

struct Motor {
    temp: f64,
    volt: f64,
    curr: f64,
    temp_invalid_left: u32,
    offroad_left: u32,
}

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// Broker from `MQTT_BROKER` / `MQTT_PORT`, else the app's
/// `rover_config.json`, else localhost. The dashboard already reads that
/// file, so a bare `cargo run --example fake_telemetry` lands on the same
/// broker as the app without a second env var.
fn configured_broker() -> (String, u16) {
    let mut broker = "127.0.0.1".to_string();
    let mut port = 1883;
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("ROVER_CONFIG") {
        candidates.push(std::path::PathBuf::from(p));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("rover_config.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            let Some(d) = dir else { break };
            candidates.push(d.join("rover_config.json"));
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    if let Some(text) = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(b) = v.get("mqtt_broker").and_then(|x| x.as_str()) {
                if !b.trim().is_empty() {
                    broker = b.trim().to_string();
                }
            }
            if let Some(p) = v.get("mqtt_port").and_then(|x| x.as_u64()) {
                if let Ok(p) = u16::try_from(p) {
                    port = p;
                }
            }
        }
    }
    if let Ok(b) = std::env::var("MQTT_BROKER") {
        if !b.trim().is_empty() {
            broker = b.trim().to_string();
        }
    }
    if let Some(p) = std::env::var("MQTT_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        if p > 0 {
            port = p;
        }
    }
    (broker, port)
}

#[tokio::main]
async fn main() {
    eprintln!("[fake] THROWAWAY publisher — delete before hardware day");
    let (broker, port) = configured_broker();
    let mut opts = MqttOptions::new("fake-esp32-throwaway", broker.clone(), port);
    opts.set_keep_alive(Duration::from_secs(30));
    let (client, mut events) = AsyncClient::new(opts, 64);
    // Wait for a real ConnAck. A blind sleep hid the usual failure (no broker
    // on the configured host) behind a publisher that looked alive.
    let mut connected = false;
    for attempt in 1..=15 {
        match events.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                eprintln!("[fake] connected to {broker}:{port}");
                connected = true;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                if attempt == 1 || attempt % 5 == 0 {
                    eprintln!("[fake] broker {broker}:{port} unreachable ({e}); retrying");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    if !connected {
        eprintln!("[fake] no ConnAck after 30 s, publishing anyway — dashboard stays offline");
    }
    tokio::spawn(async move {
        loop {
            let _ = events.poll().await;
        }
    });

    let sweep = std::env::var("FAKE_SWEEP").is_ok_and(|v| v == "1");
    if sweep {
        eprintln!("[fake] SWEEP mode: 120s full-range script per motor (staggered)");
    }
    // Rattle-by-default: double-click verification. FAKE_CALM=1 opts
    // back into the realistic periodic-burst stream.
    let rattle = !std::env::var("FAKE_CALM").is_ok_and(|v| v == "1");
    if rattle {
        eprintln!("[fake] RATTLE mode: continuous violent vibration on motors 1-4 (FAKE_CALM=1 for realistic stream)");
    }

    // ENV + IMU have their own cadence, so they get their own tasks instead
    // of hitching a ride on the 250 ms motor loop.
    tokio::spawn(fake_env(client.clone()));
    tokio::spawn(fake_imu(client.clone()));
    tokio::spawn(fake_servo(client.clone()));

    let mut rng = rand::thread_rng();
    let mut motors = [
        Motor {
            temp: 33.5,
            volt: 12.42,
            curr: 0.31,
            temp_invalid_left: 0,
            offroad_left: 0,
        },
        Motor {
            temp: 34.1,
            volt: 12.38,
            curr: 0.29,
            temp_invalid_left: 0,
            offroad_left: 0,
        },
        Motor {
            temp: 32.8,
            volt: 12.45,
            curr: 0.33,
            temp_invalid_left: 0,
            offroad_left: 0,
        },
        Motor {
            temp: 35.2,
            volt: 12.35,
            curr: 0.36,
            temp_invalid_left: 0,
            offroad_left: 0,
        },
    ];
    let mut ticks: u64 = 0;
    let t0 = std::time::Instant::now();
    loop {
        for (i, m) in motors.iter_mut().enumerate() {
            let id = (i + 1) as u8;
            let mut vib = u8::from(rng.gen_bool(0.02));
            let mut temp_valid = 1;
            if sweep {
                // Scripted 120s loop, staggered 7.5s per motor: nominal (Q0)
                // -> heat ramp (Q1) -> meltdown (Q2) -> recovery (Q3).
                // Backend health math turns this into ~100 -> ~50 -> 0 -> 100.
                let t = (t0.elapsed().as_secs_f64() + i as f64 * 7.5) % 120.0;
                let q = (t / 30.0) as u64;
                let k = ((t % 30.0) / 30.0).clamp(0.0, 1.0);
                // Small jitter so traces look alive, not plotted.
                let mut j = || rng.gen_range(-0.4..0.4);
                match q {
                    0 => {
                        m.temp = 35.0 + j();
                        m.volt = 12.4 + j() * 0.1;
                        m.curr = 0.3 + j() * 0.05;
                        vib = 0;
                    }
                    1 => {
                        m.temp = 35.0 + 40.0 * k + j();
                        m.volt = 12.4 + j() * 0.1;
                        m.curr = 0.3 + j() * 0.05;
                        vib = 0;
                    }
                    2 => {
                        m.temp = 75.0 + j();
                        m.volt = 12.4 - 2.4 * k + j() * 0.1;
                        m.curr = 0.3 + 3.2 * k + j() * 0.05;
                        vib = u8::from(k > 0.2);
                    }
                    _ => {
                        m.temp = 75.0 - 40.0 * k + j();
                        m.volt = 10.0 + 2.4 * k + j() * 0.1;
                        m.curr = 3.5 - 3.2 * k + j() * 0.05;
                        vib = u8::from(k < 0.5);
                    }
                }
            } else {
                // Slow random walk, bench-idle ranges.
                m.temp = clamp(m.temp + rng.gen_range(-0.25..0.25), 28.0, 48.0);
                m.volt = clamp(m.volt + rng.gen_range(-0.03..0.03), 11.8, 13.2);
                m.curr = clamp(m.curr + rng.gen_range(-0.02..0.02), 0.1, 1.2);
                // Every ~40s, knock one probe out for ~4 samples (~4s).
                if ticks % 160 == (i as u64) * 40 && m.temp_invalid_left == 0 {
                    m.temp_invalid_left = 4;
                    eprintln!("[fake] motor {id}: temp probe blip (temp_valid:0 x4)");
                }
                if m.temp_invalid_left > 0 {
                    m.temp_invalid_left -= 1;
                    temp_valid = 0;
                }
                // Offroad burst: every ~18s one motor shakes ~9s, staggered
                // so the Test beats + Detailed chart spike visibly in turn.
                // RATTLE mode skips the schedule: always shaking.
                if rattle {
                    vib = 1;
                } else {
                    if ticks % 12 == (i as u64) * 3 && m.offroad_left == 0 {
                        m.offroad_left = 6;
                        eprintln!("[fake] motor {id}: OFFROAD burst (vib=1 x6)");
                    }
                    if m.offroad_left > 0 {
                        m.offroad_left -= 1;
                        vib = 1;
                    }
                }
            }
            let mut mag = 1.0 + rng.gen_range(-0.03f64..0.03f64);
            let mut ax = rng.gen_range(-0.06f64..0.06f64);
            let mut ay = rng.gen_range(-0.06f64..0.06f64);
            let mut az = 0.98 + rng.gen_range(-0.03f64..0.03f64);
            if vib == 1 && !sweep {
                // Violent shaking: magnitude swings hard, axes jump.
                mag = 1.6 + rng.gen_range(0.0f64..0.8f64);
                ax = rng.gen_range(-0.8f64..0.8f64);
                ay = rng.gen_range(-0.8f64..0.8f64);
                az = 0.9 + rng.gen_range(-0.2f64..0.2f64);
            }
            let payload = json!({
                "motor": id,
                "vibration": vib,
                "vib_valid": 1,
                "ina_ok": 1,
                "temp_valid": temp_valid,
                "voltage": (m.volt * 100.0).round() / 100.0,
                "current": (m.curr * 1000.0).round() / 1000.0,
                "temp": if temp_valid == 1 {
                    serde_json::Value::from((m.temp * 10.0).round() / 10.0)
                } else {
                    serde_json::Value::Null
                },
                "speed": 0,
                "vib_mag": (mag * 100.0).round() / 100.0,
                "ax": (ax * 100.0).round() / 100.0,
                "ay": (ay * 100.0).round() / 100.0,
                "az": (az * 100.0).round() / 100.0,
            });
            let topic = format!("rover/motor/{id}");
            if let Err(e) = client
                .publish(topic, QoS::AtMostOnce, false, payload.to_string())
                .await
            {
                eprintln!("[fake] publish failed: {e}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // Blind middles (ESP-2 MID shape): no sensors, speed 0.
        for mid in 1u8..=2u8 {
            let payload = json!({
                "motor": mid,
                "vibration": 0,
                "vib_valid": 0,
                "ina_ok": 0,
                "temp_valid": 0,
                "voltage": serde_json::Value::Null,
                "current": serde_json::Value::Null,
                "temp": serde_json::Value::Null,
                "speed": 0,
            });
            let topic = format!("rover/mid/{mid}");
            if let Err(e) = client
                .publish(topic, QoS::AtMostOnce, false, payload.to_string())
                .await
            {
                eprintln!("[fake] publish failed: {e}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        ticks += 1;
    }
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Slow sine around a centre, `period` seconds per cycle (gas cells breathe
/// with the room, they do not step).
fn wave(el: f64, period: f64, amp: f64) -> f64 {
    1.0 + amp * (el * std::f64::consts::TAU / period).sin()
}

async fn publish(client: &AsyncClient, topic: &str, payload: serde_json::Value) {
    if let Err(e) = client
        .publish(topic, QoS::AtMostOnce, false, payload.to_string())
        .await
    {
        eprintln!("[fake] publish failed: {e}");
    }
}

/// ENV node (ESP-3) shape: 1 Hz telemetry, 2 s heartbeat.
/// Gas values are R0/Rs with 1.0 = clean air, no O2 cell is fitted (`o2_ok:0`)
/// so the backend derives O2 from these frames, and every ~30 s the flame
/// level bursts while the direction walks 1..5, so the alarm state, the
/// direction dots and the derived O2 all get exercised.
async fn fake_env(client: AsyncClient) {
    let t0 = std::time::Instant::now();
    let mut last_status = f64::NEG_INFINITY;
    let mut last_dir = 0i64;
    // Color-ramp check: ramp each cell 1.0 -> 3.9 over 25 s, staggered 5 s
    // apart, so warn (2.0) and high (3.5) both get crossed live.
    let gas_ramp = std::env::var("FAKE_GAS").is_ok_and(|v| v == "1");
    if gas_ramp {
        eprintln!("[fake] GAS mode: cells ramp 1.0 -> 3.9 on a 25 s stagger (bar orange -> red)");
    }
    loop {
        let el = t0.elapsed().as_secs_f64();
        // ThreadRng is !Send: build the frame in a scope so the rng is gone
        // before the awaits below.
        let frame = {
            let mut rng = rand::thread_rng();
            let burst = el % 30.0 < 4.0;
            let dir = if burst {
                ((el % 30.0) / 4.0 * 5.0).floor() as i64 % 5 + 1
            } else {
                0
            };
            if burst && dir != last_dir {
                eprintln!("[fake] ENV: flame burst, direction {dir}, level rising");
            }
            last_dir = dir;
            let mut ramp = |i: f64| {
                r2(1.0 + (el - i * 5.0).rem_euclid(25.0) / 25.0 * 2.9 + rng.gen_range(-0.02..0.02))
            };
            let (co, lpg, ch4, h2, air) = if gas_ramp {
                (ramp(0.0), ramp(1.0), ramp(2.0), ramp(3.0), ramp(4.0))
            } else {
                (
                    r2(wave(el, 95.0, 0.35) + rng.gen_range(-0.03..0.03)),
                    r2(wave(el, 140.0, 0.12) + rng.gen_range(-0.02..0.02)),
                    r2(wave(el, 170.0, 0.08) + rng.gen_range(-0.02..0.02)),
                    r2(wave(el, 120.0, 0.20) + rng.gen_range(-0.02..0.02)),
                    r2(1.05
                        + 0.5 * (el * std::f64::consts::TAU / 110.0).sin()
                        + rng.gen_range(-0.04..0.04)),
                )
            };
            json!({
                "ads_ok": 1, "mlx_ok": 1, "flame_ok": 1, "baseline_ok": 1,
                "co": co,
                "lpg": lpg,
                "ch4": ch4,
                "h2": h2,
                "air": air,
                "o2_ok": 0,
                "flame_lvl": if burst { r2(1.6 + rng.gen_range(0.0..1.4)) } else { r2(rng.gen_range(0.02..0.12)) },
                "flame_dir": dir,
                "heat_l": r2(24.6 + (el * std::f64::consts::TAU / 190.0).sin() * 0.9 + rng.gen_range(-0.05..0.05)),
                "heat_r": r2(24.2 + (el * std::f64::consts::TAU / 260.0).cos() * 0.7 + rng.gen_range(-0.05..0.05)),
                "heat_l_amb": r2(22.4 + rng.gen_range(-0.05..0.05)),
                "heat_r_amb": r2(22.5 + rng.gen_range(-0.05..0.05)),
            })
        };
        publish(&client, "rover/env/telemetry", frame).await;
        if el - last_status >= 2.0 {
            last_status = el;
            publish(
                &client,
                "rover/env/status",
                json!({"state": "running", "source": "env",
                       "burn_in_s": (el + 7200.0) as u64, "baseline_ok": 1}),
            )
            .await;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

/// MID board IMU (ESP-2) shape: 4 Hz orientation frames.
async fn fake_imu(client: AsyncClient) {
    let t0 = std::time::Instant::now();
    loop {
        let el = t0.elapsed().as_secs_f64();
        let frame = {
            let mut rng = rand::thread_rng();
            json!({
                "imu_ok": 1,
                "ax": r2(rng.gen_range(-0.03..0.03)),
                "ay": r2(rng.gen_range(-0.03..0.03)),
                "az": r2(0.98 + rng.gen_range(-0.02..0.02)),
                "gz": r2((el * std::f64::consts::TAU / 9.0).sin() * 6.0),
                "yaw": r2((el * 8.0) % 360.0),
                "pitch": r2((el * std::f64::consts::TAU / 17.0).sin() * 4.0),
                "roll": r2((el * std::f64::consts::TAU / 23.0).cos() * 3.0),
            })
        };
        publish(&client, "rover/imu", frame).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// MID swing arm (ESP-2): 1 Hz. No encoder is fitted, so the frame carries
/// only the commanded angle and the two compatibility flags at 0, exactly
/// as `publishLidarServo` sends.
async fn fake_servo(client: AsyncClient) {
    let t0 = std::time::Instant::now();
    loop {
        let el = t0.elapsed().as_secs_f64();
        // Slow drift either side of centre, like an arm settling on its stop.
        let angle = r2((el * std::f64::consts::TAU / 47.0).sin() * 18.0);
        let frame = json!({
            "servo_ok": 1,
            "angle": angle,
            "as5600_ok": 0,
            "cal_ok": 0,
            "settled": 1,
            "sweeping": 0,
            "step": 0,
            "steps": 0,
        });
        publish(&client, "rover/lidar/servo", frame).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}
