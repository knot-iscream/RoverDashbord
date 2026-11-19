//! Shared ENV presentation for the home gas panel and the Detailed sensor
//! sections: the gas channel table, the gas bar bands, and the small value
//! formatters. Severities come from the backend `alarms` block riding on every
//! `env_update`; the bar colors are baked into the scale, see `GAS_SEGMENTS`.

use serde_json::Value;

/// Gas channels in display order. MQ cells are relative indicators (`R0/Rs`),
/// never ppm.
pub const GAS: [(&str, &str); 5] = [
    ("co", "CO"),
    ("lpg", "LPG"),
    ("ch4", "CH4"),
    ("h2", "H2"),
    ("air", "Air"),
];
/// Bar scale: `R0/Rs` 3.0 fills the track (vanilla GAS_MAX).
pub const GAS_MAX: f64 = 3.0;
/// A gas bar is 30 segment ticks of 0.1 each, so clean air (1.0) and the warn
/// line (2.0) land exactly on seams (`GAS_SEAMS`). The color ramp is baked into
/// the scale: 5 bands of 6 ticks, blue -> green -> yellow -> orange -> red
/// (`--band-1`..`--band-5` in `web/style.css`, drawn by `.env-seg.cN`).
pub const GAS_SEGMENTS: usize = 30;
pub const GAS_BANDS: usize = 5;
/// First tick of each graduation: clean air 1.0 and the warn line 2.0
/// (`GAS_WARN_RATIO` in `src/env_alarm.rs`). They get a wider gap.
const GAS_SEAMS: [usize; 2] = [10, 20];
/// Freshness window for the ENV/IMU feeds (4 s, vanilla whole-page rule).
pub const FRESH_MS: f64 = 4000.0;

/// One color language everywhere: ok green, warn orange, alarm red, unknown
/// (nothing readable) dim. Mirrors the `web/style.css` tokens.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Severity {
    Ok,
    Warn,
    Alarm,
    Unknown,
}

impl Severity {
    pub fn color(self) -> &'static str {
        match self {
            Severity::Ok => "var(--dot-green)",
            Severity::Warn => "var(--sev-warn)",
            Severity::Alarm => "var(--dot-red)",
            Severity::Unknown => "var(--gray-500)",
        }
    }

    pub fn dot_class(self) -> &'static str {
        match self {
            Severity::Ok => "status-dot dot-green",
            Severity::Warn => "status-dot dot-orange",
            Severity::Alarm => "status-dot dot-red",
            Severity::Unknown => "status-dot dot-dim",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Ok => "OK",
            Severity::Warn => "WARN",
            Severity::Alarm => "ALARM",
            Severity::Unknown => "no data",
        }
    }
}

/// The `alarms` block inside a flat `env_update` payload (Null when absent).
pub fn frame_alarms(frame: &Value) -> &Value {
    frame.get("alarms").unwrap_or(&Value::Null)
}

/// Fold the `GET /api/env` shape (`{"env": .., "alarms": ..}`) into the flat
/// `env_update` shape the WS feeds, so one code path renders both. Null env
/// stays Null: the board has not spoken yet.
pub fn fold_api_env(v: &Value) -> Value {
    let env = v.get("env").cloned().unwrap_or(Value::Null);
    if env.is_null() {
        return Value::Null;
    }
    let mut frame = env;
    if let (Value::Object(out), Some(alarms)) = (&mut frame, v.get("alarms")) {
        out.insert("alarms".into(), alarms.clone());
    }
    frame
}

/// Overall level from the frame's verdict.
pub fn level_of(frame: &Value) -> Severity {
    match frame_alarms(frame).get("level").and_then(|v| v.as_str()) {
        Some("ok") => Severity::Ok,
        Some("warn") => Severity::Warn,
        Some("alarm") => Severity::Alarm,
        _ => Severity::Unknown,
    }
}

/// Severity of one alarm item by key, `None` when that key has no item.
pub fn item_severity(frame: &Value, key: &str) -> Option<Severity> {
    frame_alarms(frame)
        .get("items")?
        .as_array()?
        .iter()
        .find(|it| it.get("key").and_then(|k| k.as_str()) == Some(key))
        .map(|it| match it.get("severity").and_then(|s| s.as_str()) {
            Some("alarm") => Severity::Alarm,
            _ => Severity::Warn,
        })
}

pub fn num(frame: &Value, key: &str) -> Option<f64> {
    frame
        .get(key)
        .and_then(|x| x.as_f64())
        .filter(|v| v.is_finite())
}

/// `--` for null/missing, else fixed decimals.
pub fn fmt(v: Option<f64>, d: usize) -> String {
    match v {
        Some(x) => format!("{x:.d$}"),
        None => "--".to_string(),
    }
}

/// Freshness check on a monotonic (`performance.now`) stamp.
pub fn stale_at(last_rx: Option<f64>, now: f64) -> bool {
    match last_rx {
        None => true,
        Some(t) => now - t > FRESH_MS,
    }
}

/// Band 1..=5 of a reading: fifths of the 0..3.0 scale. `None` when the cell
/// has no value, so the bar stays empty and the number stays dim.
pub fn gas_band(frame: &Value, key: &str) -> Option<usize> {
    num(frame, key).map(|v| {
        let band = (v / GAS_MAX * GAS_BANDS as f64).floor() as i64 + 1;
        band.clamp(1, GAS_BANDS as i64) as usize
    })
}

/// Lit tick count: any non-zero reading lights at least one, so a small rise
/// is never invisible. 0 when the cell has no value.
pub fn gas_lit(frame: &Value, key: &str) -> usize {
    let lit = match num(frame, key) {
        Some(v) => (v / GAS_MAX * GAS_SEGMENTS as f64).ceil() as i64,
        None => 0,
    };
    lit.clamp(0, GAS_SEGMENTS as i64) as usize
}

/// Class for tick `i` (0-based): band color, graduation seam, lit state.
pub fn gas_seg_class(frame: &Value, key: &str, i: usize) -> String {
    let band = i / (GAS_SEGMENTS / GAS_BANDS) + 1;
    let mut c = format!("env-seg c{band}");
    if GAS_SEAMS.contains(&i) {
        c.push_str(" seam");
    }
    if i < gas_lit(frame, key) {
        c.push_str(" lit");
    }
    c
}

pub fn gas_text(frame: &Value, key: &str) -> String {
    fmt(num(frame, key), 2)
}

/// Number color: the reading's band, dim gray when the cell has no value.
pub fn gas_color(frame: &Value, key: &str) -> String {
    match gas_band(frame, key) {
        Some(band) => format!("var(--band-{band})"),
        None => Severity::Unknown.color().to_string(),
    }
}

/// Color for a value guarded by one of `alarm_keys` (`o2_low` / `flame` /
/// `heat_delta`); a missing value stays dim.
pub fn guarded_color(frame: &Value, value: Option<f64>, alarm_keys: &[&str]) -> &'static str {
    if value.is_none() {
        return Severity::Unknown.color();
    }
    for k in alarm_keys {
        if let Some(sev) = item_severity(frame, k) {
            return sev.color();
        }
    }
    Severity::Ok.color()
}

/// True/1 flag -> Some(bool); a missing field stays None (chip shows `--`).
pub fn ok_flag(frame: &Value, key: &str) -> Option<bool> {
    frame
        .get(key)
        .map(|v| v.as_bool().unwrap_or(false) || v.as_u64().is_some_and(|x| x == 1))
}

/// Level chip text with staleness folded in: a stopped feed reads
/// "no data", never a stale green.
pub fn level_text(frame: &Value, stale: bool) -> String {
    if stale {
        "no data".to_string()
    } else {
        level_of(frame).label().to_string()
    }
}

/// Level chip dot class with staleness folded in; alarm level pulses.
pub fn level_dot(frame: &Value, stale: bool) -> String {
    if stale {
        return "status-dot dot-dim".to_string();
    }
    let sev = level_of(frame);
    let mut c = sev.dot_class().to_string();
    if sev == Severity::Alarm {
        c.push_str(" pulse");
    }
    c
}
