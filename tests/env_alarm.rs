//! ENV alarm-verdict tests: thresholds, baseline gating, and the two honesty
//! rules (nulls never alarm; a frame with nothing evaluable is `unknown`, not
//! `ok`).

use digital_twin_dashboard::env_alarm;
use serde_json::{json, Value};

fn level(v: &Value) -> &str {
    v["level"].as_str().expect("level is a string")
}

fn keys(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|i| i["key"].as_str().expect("key is a string").to_string())
        .collect()
}

fn clean_frame() -> Value {
    json!({
        "baseline_ok": 1, "co": 1.1, "lpg": 1.0, "ch4": 1.0, "h2": 0.9,
        "air": 1.2, "o2_pct": 20.8, "flame_dir": 0, "heat_l": 25.0,
        "heat_r": 24.5
    })
}

#[test]
fn no_frame_reports_unknown() {
    let v = env_alarm::evaluate(&json!(null));
    assert_eq!(level(&v), "unknown");
    assert!(keys(&v).is_empty());
}

#[test]
fn clean_frame_is_ok() {
    let v = env_alarm::evaluate(&clean_frame());
    assert_eq!(level(&v), "ok");
    assert!(keys(&v).is_empty());
}

#[test]
fn all_null_frame_is_unknown_not_ok() {
    // `ads_ok:0` nulls every cell; "no data" must not read as all clear.
    let v = env_alarm::evaluate(&json!({
        "ads_ok": 0, "mlx_ok": 0, "flame_ok": 0, "baseline_ok": 0,
        "co": null, "o2_pct": null, "flame_dir": null,
        "heat_l": null, "heat_r": null
    }));
    assert_eq!(level(&v), "unknown");
    assert_eq!(v["uncalibrated"], true);
}

#[test]
fn null_fields_never_alarm() {
    let mut frame = clean_frame();
    frame["co"] = Value::Null;
    frame["flame_dir"] = Value::Null;
    frame["heat_l"] = Value::Null;
    let v = env_alarm::evaluate(&frame);
    assert_eq!(level(&v), "ok");
}

#[test]
fn o2_low_warns_then_alarms() {
    let warn = env_alarm::evaluate(&json!({"o2_pct": 19.8}));
    assert_eq!(level(&warn), "warn");
    assert_eq!(keys(&warn), vec!["o2_low"]);
    let alarm = env_alarm::evaluate(&json!({"o2_pct": 19.1}));
    assert_eq!(level(&alarm), "alarm");
    assert_eq!(alarm["items"][0]["severity"], "alarm");
}

#[test]
fn gas_bars_need_the_baseline() {
    let warn = env_alarm::evaluate(&json!({"baseline_ok": 1, "co": 2.4}));
    assert_eq!(level(&warn), "warn");
    let alarm = env_alarm::evaluate(&json!({"baseline_ok": 1, "co": 4.0}));
    assert_eq!(level(&alarm), "alarm");
}

#[test]
fn uncalibrated_skips_gas_but_keeps_o2() {
    let v = env_alarm::evaluate(&json!({"baseline_ok": 0, "co": 9.9, "o2_pct": 20.9}));
    assert_eq!(
        level(&v),
        "ok",
        "O2 still evaluates; gas has no reference yet"
    );
    assert!(keys(&v).is_empty());
    assert_eq!(v["uncalibrated"], true);
}

#[test]
fn flame_direction_is_fire_now() {
    let v = env_alarm::evaluate(&json!({"baseline_ok": 1, "flame_dir": 3}));
    assert_eq!(level(&v), "alarm");
    assert_eq!(keys(&v), vec!["flame"]);
}

#[test]
fn heat_delta_tracks_the_mlx_pair() {
    let warn = env_alarm::evaluate(&json!({"heat_l": 33.0, "heat_r": 25.0}));
    assert_eq!(level(&warn), "warn");
    let alarm = env_alarm::evaluate(&json!({"heat_l": 45.0, "heat_r": 25.0}));
    assert_eq!(level(&alarm), "alarm");
    let calm = env_alarm::evaluate(&json!({"heat_l": 27.0, "heat_r": 25.0}));
    assert_eq!(level(&calm), "ok");
}
