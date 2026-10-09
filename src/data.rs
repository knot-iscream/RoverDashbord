//! Exact port of `backend/data_handler.py`.
//! Contract: `None` means "--" in UI; never fabricate defaults for display.
//! Penalties apply ONLY when that sensor vouches for itself
//! (`temp_valid` / `ina_ok` / `vibration_valid`) — never from missing sensors.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CALIBRATION_FILE: &str = "calibration_baseline.json";

/// Baseline path: `ROVER_CALIBRATION` override if set (integration tests
/// point it at temp dirs so the suite never touches the real file), else
/// the CWD file.
pub fn calibration_path() -> std::path::PathBuf {
    std::env::var("ROVER_CALIBRATION")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| crate::config::base_dir().join(CALIBRATION_FILE))
}

/// Baseline math inputs (resolved from the stored arbitrary dict with
/// Python's defaults: `avg_temp` 35, `avg_voltage` 12).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    pub avg_temp: f64,
    pub avg_voltage: f64,
}

impl Baseline {
    fn from_value(v: &Value) -> Self {
        Self {
            avg_temp: coerce_num(v.get("avg_temp"), 35.0),
            avg_voltage: coerce_num(v.get("avg_voltage"), 12.0),
        }
    }
}

/// One motor's latest sample — mirrors the `latest_data[motor_id]` dict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotorSample {
    pub id: u8,
    /// 0/1 like Python's `1 if data.get("vibration") else 0`.
    pub vibration: u8,
    pub vibration_valid: bool,
    pub ina_ok: bool,
    pub temp_valid: bool,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub temp: Option<f64>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Default)]
pub struct DataHandler {
    // BTreeMap, not HashMap: /api/motors + WS snapshot list motors sorted by
    // id (Python dicts preserve insertion order; HashMap would randomize it).
    pub latest: BTreeMap<u8, MotorSample>,
    /// Raw baseline dicts (full fidelity with `calibration_baseline.json`).
    pub baselines: HashMap<u8, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Health {
    pub health: Option<f64>,
    pub anomalies: Vec<String>,
}

/// Python `float(value)` coercion for health math: missing/NaN → default.
/// Numeric strings parse (`float("35.5")` works in Python); anything else → default.
pub fn coerce_num(v: Option<&Value>, default: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()).unwrap_or(default),
        Some(Value::String(s)) => s
            .parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
            .unwrap_or(default),
        _ => default,
    }
}

/// Python `_raw`: preserve the raw sensor value for display; NaN/None → None
/// so WS snapshots carry null (pages render '--').
pub fn raw_num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()),
        Some(Value::String(s)) => s.parse::<f64>().ok().filter(|x| x.is_finite()),
        _ => None,
    }
}

/// Python `int(value or 0)` coercion for `speed`: floats truncate (128.0 → 128),
/// numeric strings parse, missing/null/bool-ish → 0. `serde_json::as_i64`
/// returns None for floats, so using it directly would silently read a float
/// speed as 0 (dropped history, wrong IDLE direction) where Python records it.
pub fn coerce_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .map(|f| f.trunc() as i64)
            .unwrap_or(0),
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// Python `round(x, n)` (round-half-to-even) for aggregated values.
/// Rust `f64::round` is half-away-from-zero, which disagrees on exact ties
/// (e.g. 96.25 → Python 96.2, Rust 96.3). Only exact float ties take the
/// even branch; everything else rounds normally, matching Python on every
/// value this codebase rounds (health, temps, timestamps).
/// Known limit: the 2.675 class. Python rounds the exact binary expansion
/// (`2.675` is really `2.67499999…`, so `round(2.675, 2) == 2.67`), while any
/// float-scaled approach sees `2.675*100 == 267.5` exactly and returns 2.68.
/// That needs a decimal within half-an-ulp of a tie point — unreachable from
/// real sensor math — so the exact-tie check is the right tradeoff.
pub fn py_round(x: f64, digits: u32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let m = 10f64.powi(digits as i32);
    let s = x * m;
    let f = s.fract();
    let r = if f == 0.5 || f == -0.5 {
        let t = s.trunc();
        if t % 2.0 == 0.0 {
            t
        } else if s > 0.0 {
            t + 1.0
        } else {
            t - 1.0
        }
    } else {
        s.round()
    };
    r / m
}

/// Python `bool(value)` truthiness.
pub fn py_bool(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Motor id from `"motor"` (fallback `"id"`), like Python. Non-numeric → 0.
pub fn motor_id_of(data: &Value) -> u8 {
    let v = data
        .get("motor")
        .or_else(|| data.get("id"))
        .and_then(|n| n.as_i64())
        .unwrap_or(0);
    v.clamp(0, 255) as u8
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl DataHandler {
    pub fn new() -> Self {
        let mut h = Self::default();
        h.load_calibration();
        h
    }

    /// 1:1 with `process_motor_data`: stores the sample, returns its health.
    pub fn process_motor_data(&mut self, data: &Value) -> Health {
        let motor = motor_id_of(data);
        let vibration_valid = py_bool(
            data.get("vibration_valid")
                .or_else(|| data.get("vib_valid")),
        );
        let ina_ok = py_bool(data.get("ina_ok"));
        let temp_valid = py_bool(data.get("temp_valid"));
        self.latest.insert(
            motor,
            MotorSample {
                id: motor,
                vibration: u8::from(py_bool(data.get("vibration"))),
                vibration_valid,
                ina_ok,
                temp_valid,
                voltage: raw_num(data.get("voltage")),
                current: raw_num(data.get("current")),
                temp: raw_num(data.get("temp").or_else(|| data.get("temperature"))),
                timestamp: now_secs(),
            },
        );
        self.compute_health(motor)
    }

    /// 1:1 with `compute_health(motor_id)`: unknown motor → 100, no anomalies.
    pub fn compute_health(&self, motor_id: u8) -> Health {
        let data = match self.latest.get(&motor_id) {
            Some(d) => d,
            None => {
                return Health {
                    health: Some(100.0),
                    anomalies: vec![],
                };
            }
        };
        let baseline = self.baselines.get(&motor_id).map(Baseline::from_value);
        Self::health_of(
            data.temp,
            data.voltage,
            data.current,
            data.vibration == 1,
            data.ina_ok,
            data.temp_valid,
            data.vibration_valid,
            baseline.as_ref(),
        )
    }

    /// Pure health math (unit-testable). Penalty gates + message strings are
    /// verbatim from `data_handler.py`.
    /// Eight params by design: the signature mirrors Python's sensor bundle
    /// 1:1 so parity review stays mechanical.
    #[allow(clippy::too_many_arguments)]
    pub fn health_of(
        temp: Option<f64>,
        voltage: Option<f64>,
        current: Option<f64>,
        vibration: bool,
        ina_ok: bool,
        temp_valid: bool,
        vibration_valid: bool,
        baseline: Option<&Baseline>,
    ) -> Health {
        // No trustworthy sensor → report Nothing so the UI shows "--".
        if !(ina_ok || temp_valid || vibration_valid) {
            return Health {
                health: None,
                anomalies: vec![],
            };
        }
        let t = temp.filter(|x| x.is_finite()).unwrap_or(35.0);
        let v = voltage.filter(|x| x.is_finite()).unwrap_or(12.0);
        let c = current.filter(|x| x.is_finite()).unwrap_or(0.5);
        let mut score = 100.0;
        let mut anomalies = vec![];
        if temp_valid && t > 50.0 {
            score -= (t - 50.0) * 2.0;
            anomalies.push(format!("High temperature: {t:.1}°C"));
        }
        if ina_ok {
            if v < 11.5 {
                score -= (11.5 - v) * 10.0;
                anomalies.push(format!("Low voltage: {v:.2}V"));
            }
            if c > 2.0 {
                score -= (c - 2.0) * 20.0;
                anomalies.push(format!("Over-current: {c:.3}A"));
            }
        }
        if vibration_valid && vibration {
            score -= 5.0;
            anomalies.push("Vibration spike detected".into());
        }
        // Baseline drift subtracts silently (no anomaly strings in Python).
        if let Some(b) = baseline {
            if temp_valid && t > b.avg_temp + 10.0 {
                score -= 3.0;
            }
            if ina_ok && v < b.avg_voltage * 0.9 {
                score -= 3.0;
            }
        }
        let clamped = score.clamp(0.0, 100.0);
        Health {
            health: Some(py_round(clamped, 1)),
            anomalies,
        }
    }

    pub fn set_baseline(&mut self, motor_id: u8, baseline: Value) {
        self.baselines.insert(motor_id, baseline);
        self.save_calibration();
    }

    pub fn get_latest(&self) -> Vec<Value> {
        self.latest
            .values()
            .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
            .collect()
    }

    pub fn load_calibration(&mut self) {
        let Ok(text) = std::fs::read_to_string(calibration_path()) else {
            return;
        };
        if let Ok(map) = serde_json::from_str::<HashMap<String, Value>>(&text) {
            for (k, v) in map {
                if let Ok(id) = k.parse::<u8>() {
                    self.baselines.insert(id, v);
                }
            }
        }
    }

    pub fn save_calibration(&self) {
        let map: HashMap<String, &Value> = self
            .baselines
            .iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let Ok(text) = serde_json::to_string_pretty(&map) else {
            return;
        };
        // Temp file + rename: a crash mid-write must never truncate baselines.
        let path = calibration_path();
        let tmp = format!("{}.tmp", path.display());
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
            tracing::warn!("could not save {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_valid_sensors_gives_none() {
        let h = DataHandler::health_of(None, None, None, false, false, false, false, None);
        assert_eq!(h.health, None);
        assert!(h.anomalies.is_empty());
    }

    #[test]
    fn nominal_scores_100() {
        let h = DataHandler::health_of(
            Some(35.0),
            Some(12.0),
            Some(0.5),
            false,
            true,
            true,
            true,
            None,
        );
        assert_eq!(h.health, Some(100.0));
    }

    #[test]
    fn anomaly_strings_match_python_format() {
        let h = DataHandler::health_of(
            Some(55.0),
            Some(11.0),
            Some(2.5),
            true,
            true,
            true,
            true,
            None,
        );
        // 100 -10 -5 -10 -5 = 70
        assert_eq!(h.health, Some(70.0));
        assert_eq!(
            h.anomalies,
            vec![
                "High temperature: 55.0°C",
                "Low voltage: 11.00V",
                "Over-current: 2.500A",
                "Vibration spike detected",
            ]
        );
    }

    #[test]
    fn penalties_gated_by_validity_flags() {
        // Temp high but temp_valid=false → no penalty; voltage/current/vib same.
        let h = DataHandler::health_of(
            Some(80.0),
            Some(9.0),
            Some(5.0),
            true,
            false,
            false,
            false,
            None,
        );
        // ina_ok=false, temp_valid=false, vibration_valid=false → None
        assert_eq!(h.health, None);
        // Only vibration valid: only the -5 applies despite terrible temp/V/A.
        let h = DataHandler::health_of(
            Some(80.0),
            Some(9.0),
            Some(5.0),
            true,
            false,
            false,
            true,
            None,
        );
        assert_eq!(h.health, Some(95.0));
        assert_eq!(h.anomalies, vec!["Vibration spike detected"]);
    }

    #[test]
    fn baseline_drift_subtracts_silently() {
        let b = Baseline {
            avg_temp: 35.0,
            avg_voltage: 12.0,
        };
        let h = DataHandler::health_of(
            Some(46.0),
            Some(10.0),
            Some(0.5),
            false,
            true,
            true,
            true,
            Some(&b),
        );
        // 100 -15 (low V) -3 (temp drift) -3 (voltage drift) = 79
        assert_eq!(h.health, Some(79.0));
        assert_eq!(h.anomalies, vec!["Low voltage: 10.00V"]);
    }

    #[test]
    fn unknown_motor_scores_100() {
        let h = DataHandler::new();
        let health = h.compute_health(3);
        assert_eq!(health.health, Some(100.0));
    }

    #[test]
    fn process_packet_aliases_and_raw_nulls() {
        let mut h = DataHandler::new();
        let pkt = json!({
            "id": 2, "vib_valid": true, "temperature": 42.5,
            "voltage": null, "vibration": 1
        });
        let health = h.process_motor_data(&pkt);
        let s = &h.latest[&2];
        assert_eq!(s.vibration, 1);
        assert!(s.vibration_valid);
        assert!(!s.ina_ok);
        assert_eq!(s.temp, Some(42.5));
        assert_eq!(s.voltage, None); // raw null preserved, not 0.0
                                     // ina missing + temp invalid → only vibration valid → 100-5
        assert_eq!(health.health, Some(95.0));
    }

    #[test]
    fn coerce_i64_matches_python_int_or_0() {
        use serde_json::json;
        assert_eq!(coerce_i64(Some(&json!(128))), 128);
        assert_eq!(coerce_i64(Some(&json!(128.0))), 128); // float truncates, never 0
        assert_eq!(coerce_i64(Some(&json!(128.9))), 128);
        assert_eq!(coerce_i64(Some(&json!(-64.0))), -64);
        assert_eq!(coerce_i64(Some(&json!("64"))), 64);
        assert_eq!(coerce_i64(Some(&json!(true))), 1);
        assert_eq!(coerce_i64(Some(&json!(null))), 0);
        assert_eq!(coerce_i64(None), 0);
        assert_eq!(coerce_i64(Some(&json!("abc"))), 0);
    }

    #[test]
    fn py_round_matches_python_bankers() {
        assert_eq!(py_round(96.25, 1), 96.2); // Rust round() gives 96.3 here
        assert_eq!(py_round(97.75, 1), 97.8);
        assert_eq!(py_round(2.5, 0), 2.0);
        assert_eq!(py_round(3.5, 0), 4.0);
        assert_eq!(py_round(-2.5, 0), -2.0);
        assert_eq!(py_round(-3.5, 0), -4.0);
        assert_eq!(py_round(0.125, 2), 0.12); // exact tie → even
        assert_eq!(py_round(0.375, 2), 0.38); // exact tie → odd rounds up
        assert_eq!(py_round(40.125, 2), 40.12);
        assert_eq!(py_round(100.0, 1), 100.0);
        // NOTE: py_round(2.675, 2) == 2.68 here, Python says 2.67 — the
        // documented half-ulp limit (see py_round docs). Unreachable in practice.
    }
}
