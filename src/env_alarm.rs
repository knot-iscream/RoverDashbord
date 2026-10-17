//! Indicative ENV alarms for `GET /api/env`.
//!
//! Pure verdict over the latest `rover/env/telemetry` frame. These are bench
//! defaults, NOT safety limits: gas bars are relative `R0/Rs` (1.0 = clean
//! air, no ppm claims) and the whole system treats them as indicative. Two
//! honesty rules mirror the rest of the project:
//! - a null field never alarms (an unreadable cell must not read as clean),
//! - when nothing in the frame is evaluable the level is `unknown`, never
//!   `ok` ("no data" must not be dressed up as "all clear").
//!
//! `baseline_ok:0` means the gas ratios have no reference yet, so the gas
//! rules are skipped and the verdict carries `"uncalibrated": true`.

use serde_json::{json, Value};

/// O2 cell, percent. Fresh air reads near 20.9% but aged galvanic cells sag,
/// so the warn line sits close to air on purpose. Tune on the bench.
pub const O2_WARN_PCT: f64 = 20.0;
pub const O2_LOW_PCT: f64 = 19.5;
/// Gas bars: `R0/Rs` against the captured clean-air baseline (thin tick at
/// 1.0). Indicative defaults; retune after the MQ burn-in day.
pub const GAS_WARN_RATIO: f64 = 2.0;
pub const GAS_HIGH_RATIO: f64 = 3.5;
/// MLX pair: |left - right| object temperature, Celsius. The pair is toed
/// out, so a heat source off to one side shows as a delta between the two
/// before either sensor reads hot on its own.
pub const HEAT_DELTA_WARN_C: f64 = 8.0;
pub const HEAT_DELTA_HIGH_C: f64 = 15.0;

const GAS_KEYS: [(&str, &str); 5] = [
    ("co", "CO raised"),
    ("lpg", "LPG raised"),
    ("ch4", "CH4 raised"),
    ("h2", "H2 raised"),
    ("air", "Air quality raised"),
];

fn num(frame: &Value, key: &str) -> Option<f64> {
    frame.get(key).and_then(Value::as_f64)
}

fn item(key: &str, severity: &str, label: &str, value: f64, threshold: f64) -> Value {
    json!({
        "key": key,
        "severity": severity,
        "label": label,
        // 2 decimals keep the payload tidy; the raw frame keeps full detail.
        "value": (value * 100.0).round() / 100.0,
        "threshold": threshold,
    })
}

/// Evaluate one frame. Returns `{"level", "items"}` plus `"uncalibrated"`
/// when the gas rules were skipped for `baseline_ok:0`. Pure and total: any
/// JSON in, verdict out, no panics.
pub fn evaluate(frame: &Value) -> Value {
    if !frame.is_object() {
        return json!({"level": "unknown", "items": []});
    }
    let mut items: Vec<Value> = Vec::new();
    let mut evaluated = false;

    // O2 is the one reading in real units; evaluate it first.
    if let Some(pct) = num(frame, "o2_pct") {
        evaluated = true;
        if pct < O2_LOW_PCT {
            items.push(item("o2_low", "alarm", "O2 low", pct, O2_LOW_PCT));
        } else if pct < O2_WARN_PCT {
            items.push(item("o2_low", "warn", "O2 low", pct, O2_WARN_PCT));
        }
    }

    // Gas ratios only mean anything against the captured clean-air baseline.
    let baseline = num(frame, "baseline_ok");
    let calibrated = baseline == Some(1.0);
    if calibrated {
        for (key, label) in GAS_KEYS {
            let Some(ratio) = num(frame, key) else {
                continue;
            };
            evaluated = true;
            if ratio >= GAS_HIGH_RATIO {
                items.push(item(key, "alarm", label, ratio, GAS_HIGH_RATIO));
            } else if ratio >= GAS_WARN_RATIO {
                items.push(item(key, "warn", label, ratio, GAS_WARN_RATIO));
            }
        }
    }

    // Fire right now = a lit direction; `flame_ok` only means the module
    // answers, never conflate the two.
    if let Some(dir) = num(frame, "flame_dir") {
        evaluated = true;
        if dir > 0.0 {
            items.push(item("flame", "alarm", "Flame seen", dir, 0.0));
        }
    }

    if let (Some(l), Some(r)) = (num(frame, "heat_l"), num(frame, "heat_r")) {
        evaluated = true;
        let delta = (l - r).abs();
        if delta >= HEAT_DELTA_HIGH_C {
            items.push(item(
                "heat_delta",
                "alarm",
                "Heat uneven",
                delta,
                HEAT_DELTA_HIGH_C,
            ));
        } else if delta >= HEAT_DELTA_WARN_C {
            items.push(item(
                "heat_delta",
                "warn",
                "Heat uneven",
                delta,
                HEAT_DELTA_WARN_C,
            ));
        }
    }

    let level = if items.iter().any(|i| i["severity"] == "alarm") {
        "alarm"
    } else if items.iter().any(|i| i["severity"] == "warn") {
        "warn"
    } else if evaluated {
        "ok"
    } else {
        "unknown"
    };

    let mut out = json!({"level": level, "items": items});
    if baseline.is_some() && !calibrated {
        out["uncalibrated"] = json!(true);
    }
    out
}
