//! Sensor baselines: what each sensor was reading when it was captured.
//!
//! The motor baselines come from `calibration_baseline.json` and feed the
//! health drift math. These are the other half of the Calibration page: the
//! gas cells, the flame module, the MLX pair, the IMU and the AS5600 under
//! the swing arm. They feed nothing but the display, so they live in their
//! own store (`sensor_baseline.json`) and touch no alarm rule.
//!
//! The O2 row is gone with the galvanic cell. The derived O2 is a model
//! output, not a sensor, so there is nothing to capture or drift against.
//!
//! Everything here is pure math over frames the backend already holds, so the
//! whole thing is testable with no boards attached.

use serde_json::{json, Map, Value};

/// How much history a capture averages. 10 s is ~10 ENV frames (1 Hz) and
/// ~40 IMU poses (4 Hz): long enough to smooth the gas cells, short enough
/// that the number still means "right now".
pub const CAPTURE_WINDOW_S: i64 = 10;

/// The cells on the ENV board's gas module, in the order the board publishes
/// them (`base_v0` and the response share this order).
pub const GAS_CELLS: [&str; 5] = ["co", "lpg", "ch4", "h2", "air"];

/// What the page shows per sensor: the board that owns it and the command
/// that re-captures the reference there. `None` means the app records the
/// value and the board has nothing to capture (the IMU today: its rest offsets
/// need the bench-day firmware, so we only write down what it is reading).
pub struct SensorSpec {
    pub key: &'static str,
    pub board: &'static str,
    pub command: Option<(&'static str, &'static str)>,
}

/// Board order, so the page reads the same way the wiring does.
pub const SPECS: [SensorSpec; 5] = [
    SensorSpec {
        key: "gas",
        board: "ESP-3 ENV",
        command: Some(("rover/env/command", "baseline")),
    },
    SensorSpec {
        key: "flame",
        board: "ESP-3 ENV",
        command: Some(("rover/env/command", "baseline")),
    },
    SensorSpec {
        key: "heat",
        board: "ESP-3 ENV",
        command: Some(("rover/env/command", "baseline")),
    },
    SensorSpec {
        key: "imu",
        board: "ESP-2 MID",
        command: None,
    },
    SensorSpec {
        key: "servo",
        board: "ESP-2 MID",
        command: Some(("rover/lidar/command", "servo_cal")),
    },
];

/// Round to 4 decimals: enough for a baseline, short enough that the stored
/// JSON stays readable and two identical captures compare equal.
fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Mean of one field across the window. Frames that lack the field or hold
/// null (sensor down) are skipped, not counted as zero.
fn avg(frames: &[Value], key: &str) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0usize;
    for f in frames {
        if let Some(v) = f.get(key).and_then(|v| v.as_f64()) {
            sum += v;
            n += 1;
        }
    }
    (n > 0).then(|| round(sum / n as f64))
}

/// Mean of every cell that has at least one reading. A single dead channel
/// should not throw the whole gas baseline away, so the cells are independent.
fn gas_reference(frames: &[Value]) -> Option<Value> {
    let mut out = Map::new();
    for cell in GAS_CELLS {
        if let Some(v) = avg(frames, cell) {
            out.insert(cell.to_string(), json!(v));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn flame_reference(frames: &[Value]) -> Option<Value> {
    avg(frames, "flame_lvl").map(|v| json!({"flame_lvl": v}))
}

fn heat_reference(frames: &[Value]) -> Option<Value> {
    let mut out = Map::new();
    for field in ["heat_l", "heat_r", "heat_l_amb", "heat_r_amb"] {
        if let Some(v) = avg(frames, field) {
            out.insert(field.to_string(), json!(v));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn imu_reference(frames: &[Value]) -> Option<Value> {
    let mut out = Map::new();
    for field in ["ax", "ay", "az", "gz"] {
        if let Some(v) = avg(frames, field) {
            out.insert(field.to_string(), json!(v));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// The AS5600 result. Its raw calibration points stay on the board (it sweeps
/// the arm to both hard limits); what the app can record is the measured angle
/// and whether a calibration exists at all.
fn servo_reference(frame: &Value) -> Option<Value> {
    let mut out = Map::new();
    if let Some(v) = frame.get("cal_ok").and_then(|v| v.as_f64()) {
        out.insert("cal_ok".into(), json!(v != 0.0));
    }
    for field in ["angle_real", "angle"] {
        if let Some(v) = frame.get(field).and_then(|v| v.as_f64()) {
            out.insert(field.into(), json!(round(v)));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// Build one sensor's reference from the frames on hand. Returns `None` when
/// that sensor has said nothing in the window: "not captured" is a real state
/// and the page says so, rather than storing a zero that reads like a clean
/// room.
pub fn reference_for(key: &str, env: &[Value], imu: &[Value], servo: &Value) -> Option<Value> {
    match key {
        "gas" => gas_reference(env),
        "flame" => flame_reference(env),
        "heat" => heat_reference(env),
        "imu" => imu_reference(imu),
        "servo" => servo_reference(servo),
        _ => None,
    }
}

/// Every reference the current frames can produce, in page order. Used by the
/// one-press bench run.
pub fn all_references(env: &[Value], imu: &[Value], servo: &Value) -> Vec<(&'static str, Value)> {
    SPECS
        .iter()
        .filter_map(|spec| reference_for(spec.key, env, imu, servo).map(|r| (spec.key, r)))
        .collect()
}

/// The keys that exist, for validating a `:key` path segment.
pub fn is_known(key: &str) -> bool {
    SPECS.iter().any(|s| s.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(co: f64, flame: f64, amb: f64) -> Value {
        json!({
            "co": co, "lpg": co, "ch4": co, "h2": co, "air": co,
            "flame_lvl": flame,
            "heat_l": amb + 3.0, "heat_r": amb + 2.5,
            "heat_l_amb": amb, "heat_r_amb": amb + 0.1,
        })
    }

    #[test]
    fn gas_averages_every_cell_over_the_window() {
        let frames = vec![env(1.0, 0.04, 21.0), env(1.4, 0.04, 21.0)];
        let r = gas_reference(&frames).unwrap();
        assert_eq!(r["co"], 1.2);
        assert_eq!(r["h2"], 1.2);
    }

    #[test]
    fn null_and_missing_frames_are_skipped_not_zeroed() {
        // The cell dropped out for the second frame: mean of what arrived.
        let frames = vec![env(2.0, 0.04, 21.0), json!({"co": 4.0})];
        let r = gas_reference(&frames).unwrap();
        assert_eq!(r["co"], 3.0);
        assert_eq!(r["lpg"], 2.0, "only one reading, still captured");
    }

    #[test]
    fn a_dead_channel_does_not_void_the_gas_baseline() {
        let frames = vec![json!({"co": 1.0})];
        let r = gas_reference(&frames).unwrap();
        assert_eq!(r.as_object().unwrap().len(), 1);
    }

    #[test]
    fn silent_sensor_yields_no_reference() {
        assert!(gas_reference(&[]).is_none());
        assert!(flame_reference(&[json!({"co": 1.0})]).is_none());
        assert!(imu_reference(&[json!({"imu_ok": 1})]).is_none());
    }

    #[test]
    fn imu_reference_covers_the_four_published_axes() {
        let frames = vec![json!({"ax": 0.0, "ay": 0.3, "az": 0.98, "gz": -1.2})];
        let r = imu_reference(&frames).unwrap();
        assert_eq!(r["ay"], 0.3);
        assert_eq!(r["gz"], -1.2);
    }

    #[test]
    fn servo_needs_a_frame_and_reports_whether_cal_exists() {
        assert!(servo_reference(&Value::Null).is_none());
        let r = servo_reference(&json!({"cal_ok": 1, "angle_real": -12.3456})).unwrap();
        assert_eq!(r["cal_ok"], true);
        assert_eq!(r["angle_real"], -12.3456);
    }

    #[test]
    fn dispatch_covers_every_spec_key_and_nothing_else() {
        let env = vec![env(1.0, 0.04, 21.0)];
        let imu = vec![json!({"ax": 0.0, "ay": 0.1, "az": 1.0, "gz": 0.0})];
        let servo = json!({"cal_ok": 1, "angle_real": 0.0});
        for spec in SPECS.iter() {
            assert!(
                reference_for(spec.key, &env, &imu, &servo).is_some(),
                "{} produced nothing",
                spec.key
            );
        }
        assert!(reference_for("nope", &env, &imu, &servo).is_none());
    }
}
