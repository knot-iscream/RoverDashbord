//! Derived O2 for the ENV node.
//!
//! The galvanic cell is not fitted (not purchasable here), so O2 is inferred
//! from what the board does measure: the five MQ ratios, the flame channels
//! and the MLX object temperatures. This is an INDICATOR, never a
//! measurement. The MQ cells read `R0/Rs` (1.0 = clean air), which cannot
//! tell oxygen concentration; a real fire does displace O2, and that is the
//! only thing this model claims to track.
//!
//! Shape: a 0..1 "combustion load" from the strongest available evidence,
//! mapped onto 20.9 % -> 16.9 %. Attack is fast (2 s) so a fire shows up
//! immediately, release is slow (20 s) so one clean frame does not erase it.
//!
//! Honesty rules:
//! - no evaluable input -> no estimate at all (the UI keeps `--`),
//! - a real cell (`o2_ok:1`) always wins, the estimate never overwrites it,
//! - the frame carries `o2_est:1` so the alarm and the UI can say "estimated",
//! - gas ratios are skipped while `baseline_ok:0`: without a clean-air
//!   reference the ratios are not comparable and would invent a depletion.

use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

/// Clean air, the anchor of the whole scale.
pub const O2_AIR_PCT: f64 = 20.9;
/// Worst case the model will claim. 19.5 % is already "oxygen deficient"
/// on a real meter, so the floor stays close to it instead of pretending to
/// model a smoke-filled room.
pub const O2_FLOOR_PCT: f64 = 16.9;

/// Gas ratio that counts as a full gas contribution. Reuses the alarm's
/// high line so the number and the verdict cannot disagree.
pub const GAS_REF_HIGH: f64 = 3.5;
pub const W_GAS: f64 = 0.35;
pub const W_FLAME: f64 = 0.80;
pub const W_HEAT: f64 = 0.40;

/// Flame analog output. Below the floor is the module's ambient level;
/// `flame_dir > 0` is the module's own threshold and counts as a full hit.
pub const FLAME_FLOOR_V: f64 = 0.20;
pub const FLAME_FULL_V: f64 = 1.00;

/// MLX object temperature window. A fire in the room pushes well past 120 C;
/// body temperature and sunlight stay under the 40 C reference.
pub const HEAT_REF_C: f64 = 40.0;
pub const HEAT_FULL_C: f64 = 120.0;

/// EMA time constants, seconds. Rising = a fire started, falling = it went
/// out. Asymmetric on purpose.
pub const EMA_UP_S: f64 = 2.0;
pub const EMA_DOWN_S: f64 = 20.0;
/// A feed hole longer than this resets the filter: the ENV board rebooted or
/// the broker dropped, and the old value says nothing about now.
pub const FEED_GAP_S: f64 = 10.0;

/// One frame's inputs, already normalised.
struct Target {
    load: f64,
    src: &'static str,
}

fn num(frame: &Value, key: &str) -> Option<f64> {
    frame
        .get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
}

fn flag(frame: &Value, key: &str) -> bool {
    num(frame, key).is_some_and(|v| v != 0.0)
}

fn src_label(gas: bool, flame: bool, heat: bool) -> &'static str {
    match (gas, flame, heat) {
        (true, true, true) => "gas+flame+heat",
        (true, true, false) => "gas+flame",
        (true, false, true) => "gas+heat",
        (false, true, true) => "flame+heat",
        (true, false, false) => "gas",
        (false, true, false) => "flame",
        (false, false, true) => "heat",
        (false, false, false) => "none",
    }
}

/// Normalise one frame. Every term is optional; the load is the weighted sum
/// of whatever the board can actually vouch for, clamped to 1.
fn target(frame: &Value) -> Option<Target> {
    let mut load = 0.0;

    let mut gas_on = false;
    if flag(frame, "baseline_ok") {
        let mut peak: Option<f64> = None;
        for key in ["co", "lpg", "ch4", "h2", "air"] {
            let Some(ratio) = num(frame, key) else {
                continue;
            };
            let n = ((ratio - 1.0) / (GAS_REF_HIGH - 1.0)).clamp(0.0, 1.0);
            peak = Some(peak.map_or(n, |m: f64| m.max(n)));
        }
        if let Some(g) = peak {
            load += W_GAS * g;
            gas_on = true;
        }
    }

    let mut flame_on = false;
    let mut flame: Option<f64> = None;
    if num(frame, "flame_dir").is_some_and(|d| d > 0.0) {
        flame = Some(1.0);
    } else if let Some(lvl) = num(frame, "flame_lvl") {
        let span = FLAME_FULL_V - FLAME_FLOOR_V;
        let n = ((lvl - FLAME_FLOOR_V) / span).clamp(0.0, 1.0);
        // The analog channel alone is weaker evidence than the module's own
        // digital trip, so it only ever carries half the flame weight.
        flame = Some(n * 0.5);
    }
    if let Some(f) = flame {
        load += W_FLAME * f;
        flame_on = true;
    }

    let mut heat_on = false;
    let heat = [num(frame, "heat_l"), num(frame, "heat_r")]
        .into_iter()
        .flatten()
        .reduce(f64::max)
        .map(|t| ((t - HEAT_REF_C) / (HEAT_FULL_C - HEAT_REF_C)).clamp(0.0, 1.0));
    if let Some(h) = heat {
        load += W_HEAT * h;
        heat_on = true;
    }

    (gas_on || flame_on || heat_on).then(|| Target {
        load: load.clamp(0.0, 1.0),
        src: src_label(gas_on, flame_on, heat_on),
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Stateful because of the EMA and the feed-hole reset.
#[derive(Debug, Clone)]
pub struct O2Estimator {
    value: f64,
    seen: bool,
    last: Option<Instant>,
}

impl Default for O2Estimator {
    fn default() -> Self {
        Self {
            value: O2_AIR_PCT,
            seen: false,
            last: None,
        }
    }
}

impl O2Estimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the filter. Used by tests and after a deliberate feed reset.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Ingest-time update. Returns the fields to merge into the frame, or
    /// `None` when this frame must keep its own readings (real cell fitted)
    /// or carries nothing the model can use.
    pub fn update(&mut self, frame: &Value) -> Option<Value> {
        let now = Instant::now();
        let dt = self
            .last
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        self.last = Some(now);
        self.update_with_dt(frame, dt)
    }

    /// Same as [`update`] with an explicit step, so tests can drive the
    /// filter without sleeping.
    pub fn update_with_dt(&mut self, frame: &Value, dt_s: f64) -> Option<Value> {
        // A real cell always wins, and a frame that already carries an
        // estimate is not fed through the filter a second time.
        if flag(frame, "o2_ok") || flag(frame, "o2_est") {
            return None;
        }
        let t = target(frame)?;
        let goal = O2_AIR_PCT - (O2_AIR_PCT - O2_FLOOR_PCT) * t.load;

        if !self.seen || dt_s <= 0.0 || dt_s > FEED_GAP_S {
            self.value = goal;
            self.seen = true;
        } else {
            let tau = if goal < self.value {
                EMA_UP_S
            } else {
                EMA_DOWN_S
            };
            let alpha = 1.0 - (-dt_s / tau).exp();
            self.value += (goal - self.value) * alpha;
        }

        Some(json!({
            "o2_pct": round2(self.value),
            "o2_est": 1,
            "o2_load": round2(t.load),
            "o2_src": t.src,
        }))
    }

    pub fn value(&self) -> f64 {
        self.value
    }
}

/// Merge the estimate into a raw ENV frame. Called once, at ingest, so the
/// stored frame, the WS relay, `/api/env`, the chart ring, the export and the
/// alarm verdict all see the same numbers.
pub fn apply(state: &Mutex<O2Estimator>, frame: Value) -> Value {
    if !frame.is_object() {
        return frame;
    }
    let patch = {
        let mut est = state.lock().unwrap_or_else(|e| e.into_inner());
        est.update(&frame)
    };
    let Some(patch) = patch else {
        return frame;
    };
    let mut out = frame;
    if let (Value::Object(obj), Value::Object(add)) = (&mut out, patch) {
        obj.extend(add);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> Value {
        json!({
            "baseline_ok": 1,
            "co": 1.0, "lpg": 1.0, "ch4": 1.0, "h2": 1.0, "air": 1.0,
            "flame_dir": 0, "flame_lvl": 0.05,
            "heat_l": 25.0, "heat_r": 24.0,
        })
    }

    fn fire() -> Value {
        json!({"baseline_ok": 0, "flame_dir": 1, "heat_l": 25.0, "heat_r": 25.0})
    }

    fn pct(out: &Value) -> f64 {
        out["o2_pct"].as_f64().unwrap()
    }

    #[test]
    fn clean_air_is_the_anchor() {
        let out = O2Estimator::new().update_with_dt(&clean(), 1.0).unwrap();
        assert_eq!(out["o2_pct"], json!(20.9));
        assert_eq!(out["o2_load"], json!(0.0));
        assert_eq!(out["o2_est"], json!(1));
        assert_eq!(out["o2_src"], json!("gas+flame+heat"));
    }

    #[test]
    fn gas_at_the_high_line_hits_the_warn_threshold() {
        let mut f = clean();
        f["co"] = json!(GAS_REF_HIGH);
        let out = O2Estimator::new().update_with_dt(&f, 1.0).unwrap();
        assert!((pct(&out) - 19.5).abs() < 0.001);
        assert!((out["o2_load"].as_f64().unwrap() - W_GAS).abs() < 0.001);
    }

    #[test]
    fn a_lit_flame_direction_is_the_strongest_term() {
        let out = O2Estimator::new().update_with_dt(&fire(), 1.0).unwrap();
        assert!((pct(&out) - 17.7).abs() < 0.001);
        assert_eq!(out["o2_src"], json!("flame+heat"));
    }

    #[test]
    fn hot_objects_alone_deplete_the_index() {
        let mut f = clean();
        f["heat_l"] = json!(120.0);
        f["heat_r"] = json!(60.0);
        let out = O2Estimator::new().update_with_dt(&f, 1.0).unwrap();
        assert!((pct(&out) - 19.3).abs() < 0.001);
    }

    #[test]
    fn an_empty_frame_produces_no_estimate() {
        assert!(O2Estimator::new().update_with_dt(&json!({}), 1.0).is_none());
    }

    #[test]
    fn gas_is_skipped_without_a_clean_air_baseline() {
        let f = json!({
            "baseline_ok": 0,
            "co": 3.5, "lpg": 3.5, "ch4": 3.5, "h2": 3.5, "air": 3.5,
            "heat_l": 120.0, "heat_r": 120.0,
        });
        let out = O2Estimator::new().update_with_dt(&f, 1.0).unwrap();
        assert!((pct(&out) - 19.3).abs() < 0.001, "heat only");
        assert_eq!(out["o2_src"], json!("heat"));
    }

    #[test]
    fn a_real_cell_is_never_overwritten() {
        let f = json!({"o2_ok": 1, "o2_pct": 20.5, "flame_dir": 1});
        assert!(O2Estimator::new().update_with_dt(&f, 1.0).is_none());
    }

    #[test]
    fn an_already_estimated_frame_is_left_alone() {
        let f = json!({"o2_est": 1, "o2_pct": 17.0, "flame_dir": 1});
        assert!(O2Estimator::new().update_with_dt(&f, 1.0).is_none());
    }

    #[test]
    fn attack_is_faster_than_release() {
        let mut e = O2Estimator::new();
        e.update_with_dt(&clean(), 1.0);

        let a = e.update_with_dt(&fire(), 1.0).unwrap();
        let av = pct(&a);
        // one second into a 2 s attack: 20.9 - 3.2 * (1 - e^-0.5)
        assert!((av - 19.64).abs() < 0.02, "attack {av}");

        let b = e.update_with_dt(&clean(), 1.0).unwrap();
        let bv = pct(&b);
        // one second into a 20 s release, the climb barely starts
        assert!(bv > av && bv < 19.75, "release {bv}");
    }

    #[test]
    fn a_feed_hole_resets_the_filter() {
        let mut e = O2Estimator::new();
        e.update_with_dt(&clean(), 1.0);
        e.update_with_dt(&fire(), 1.0);
        let out = e.update_with_dt(&clean(), FEED_GAP_S + 1.0).unwrap();
        assert_eq!(pct(&out), 20.9);
    }

    #[test]
    fn apply_merges_the_fields_and_keeps_the_rest() {
        let state = Mutex::new(O2Estimator::new());
        let out = apply(&state, fire());
        assert_eq!(out["flame_dir"], json!(1));
        assert_eq!(out["o2_est"], json!(1));
        assert!(out["o2_pct"].as_f64().unwrap() < 18.0);
    }

    #[test]
    fn apply_passes_non_objects_through() {
        let state = Mutex::new(O2Estimator::new());
        assert!(apply(&state, Value::Null).is_null());
    }
}
