//! Port of `backend/calibration_manager.py` — pure state + status tracker.
//! Telemetry persistence lives in `HistoryStore`; this FSM only follows the
//! ESP32 sweep (`warmup` 120s → `sweep`) and relays its progress.
//!
//! Faithful quirk: Python computes `warmup_remaining_s` from `elapsed = 0`
//! whenever the state is not `warmup`, so idle/sweep report **120** —
//! replicated here so `/api/calibration/status` diffs byte-identical.

use serde::Serialize;
use serde_json::Value;
use std::time::Instant;

pub const WARMUP_S: f64 = 120.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalState {
    Idle,
    Warmup,
    Sweep,
}

#[derive(Debug)]
pub struct CalibrationManager {
    pub state: CalState,
    pub warmup_start: Option<Instant>,
    pub samples: u64,
    pub speed_pct: i64,
    pub direction: i64,
    pub step_remaining_s: i64,
    pub cycle: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalStatus {
    pub state: CalState,
    pub warmup_pct: f64,
    pub warmup_remaining_s: f64,
    pub samples_collected: u64,
    pub speed_pct: i64,
    pub direction: i64,
    pub step_remaining_s: i64,
    pub cycle: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartResult {
    pub state: CalState,
    pub warmup_duration_s: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopResult {
    pub state: CalState,
    pub samples_collected: u64,
}

fn round1(x: f64) -> f64 {
    // Python `round(x, 1)` (banker's) in `get_status`, not half-away.
    crate::data::py_round(x, 1)
}

fn coerce_i64(v: Option<&Value>, keep: i64) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(keep),
        Some(Value::String(s)) => match s.as_str() {
            "FWD" => 1,
            "REV" => -1,
            "IDLE" => 0,
            _ => s.parse::<i64>().unwrap_or(keep),
        },
        Some(Value::Bool(b)) => i64::from(*b),
        _ => keep,
    }
}

impl Default for CalibrationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CalibrationManager {
    pub fn new() -> Self {
        Self {
            state: CalState::Idle,
            warmup_start: None,
            samples: 0,
            speed_pct: 0,
            direction: 0,
            step_remaining_s: 0,
            cycle: 0,
        }
    }

    /// 1:1 with `start()`.
    pub fn start(&mut self) -> StartResult {
        self.state = CalState::Warmup;
        self.warmup_start = Some(Instant::now());
        self.samples = 0;
        // Published by routes::calibration_start (needs the client handle).
        StartResult {
            state: CalState::Warmup,
            warmup_duration_s: 120,
        }
    }

    /// 1:1 with `stop()`.
    pub fn stop(&mut self) -> StopResult {
        self.state = CalState::Idle;
        self.speed_pct = 0;
        self.direction = 0;
        self.step_remaining_s = 0;
        // Published by routes::calibration_stop (needs the client handle).
        StopResult {
            state: CalState::Idle,
            samples_collected: self.samples,
        }
    }

    pub fn count_sample(&mut self) {
        self.samples += 1;
    }

    /// 1:1 with `get_status()` — mutates: warmup auto-advances to sweep at 120s.
    pub fn status(&mut self) -> CalStatus {
        let mut elapsed = 0.0;
        let mut warmup_pct = 0.0;
        if self.state == CalState::Warmup {
            if let Some(t0) = self.warmup_start {
                elapsed = t0.elapsed().as_secs_f64();
                warmup_pct = (elapsed / WARMUP_S * 100.0).min(100.0);
                warmup_pct = round1(warmup_pct);
                if elapsed >= WARMUP_S {
                    self.state = CalState::Sweep;
                    warmup_pct = 100.0;
                }
            }
        }
        CalStatus {
            state: self.state.clone(),
            warmup_pct,
            warmup_remaining_s: (WARMUP_S - round1(elapsed)).max(0.0),
            samples_collected: self.samples,
            speed_pct: self.speed_pct,
            direction: self.direction,
            step_remaining_s: self.step_remaining_s,
            cycle: self.cycle,
        }
    }

    /// 1:1 with `handle_esp32_status`.
    pub fn handle_esp32_status(&mut self, status: &Value) {
        let esp_state = status.get("state").and_then(|v| v.as_str()).unwrap_or("");
        match esp_state {
            "sweep" => {
                self.state = CalState::Sweep;
                self.speed_pct = coerce_i64(status.get("speed_pct"), self.speed_pct);
                self.direction = coerce_i64(
                    status.get("dir").or_else(|| status.get("direction")),
                    self.direction,
                );
                self.step_remaining_s =
                    coerce_i64(status.get("step_remaining_s"), self.step_remaining_s);
                self.cycle = coerce_i64(status.get("cycle"), self.cycle);
            }
            "warmup" => {
                self.state = CalState::Warmup;
            }
            "idle" if self.state != CalState::Idle => {
                self.state = CalState::Idle;
                self.speed_pct = 0;
                self.direction = 0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn start_enters_warmup_with_full_remaining() {
        let mut c = CalibrationManager::new();
        let r = c.start();
        assert_eq!(r.state, CalState::Warmup);
        assert_eq!(r.warmup_duration_s, 120);
        let st = c.status();
        assert_eq!(st.state, CalState::Warmup);
        assert_eq!(st.warmup_remaining_s, 120.0);
        assert!(st.warmup_pct < 1.0);
    }

    #[test]
    fn warmup_auto_advances_to_sweep_at_120s() {
        let mut c = CalibrationManager::new();
        c.start();
        c.warmup_start = Some(Instant::now() - Duration::from_secs(121));
        let st = c.status();
        assert_eq!(st.state, CalState::Sweep);
        assert_eq!(st.warmup_pct, 100.0);
    }

    #[test]
    fn idle_reports_python_remaining_quirk() {
        // Python quirk: elapsed=0 when not warming → remaining reads 120.
        let mut c = CalibrationManager::new();
        let st = c.status();
        assert_eq!(st.warmup_remaining_s, 120.0);
        assert_eq!(st.warmup_pct, 0.0);
    }

    #[test]
    fn esp32_status_relay_and_idle_reset() {
        let mut c = CalibrationManager::new();
        c.start();
        c.handle_esp32_status(&json!({
            "state": "sweep", "speed_pct": 57,
            "dir": 1, "step_remaining_s": 45, "cycle": 3,
        }));
        let st = c.status();
        assert_eq!(st.state, CalState::Sweep);
        assert_eq!(st.speed_pct, 57);
        assert_eq!(st.direction, 1);
        assert_eq!(st.step_remaining_s, 45);
        assert_eq!(st.cycle, 3);
        c.handle_esp32_status(&json!({"state": "idle"}));
        assert_eq!(c.status().state, CalState::Idle);
        assert_eq!(c.speed_pct, 0);
    }

    #[test]
    fn stop_returns_sample_count() {
        let mut c = CalibrationManager::new();
        c.start();
        c.count_sample();
        c.count_sample();
        let r = c.stop();
        assert_eq!(r.samples_collected, 2);
        assert_eq!(c.status().state, CalState::Idle);
    }
}
