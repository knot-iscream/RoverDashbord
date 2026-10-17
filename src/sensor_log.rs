//! Rolling in-memory logs for the new sensor feeds: ENV at 1 Hz and IMU at
//! 4 Hz, two hours each. They feed the Detailed page's range charts and the
//! Excel export. In-memory on purpose: the log resets with the process, and
//! the ENV board is not running 24/7 yet. Disk persistence is the planned
//! Pi-session item; this module is what it will read from.

use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Two hours of each feed. ENV runs at 1 Hz, the MID IMU at 4 Hz.
pub const ENV_CAP: usize = 2 * 3600;
pub const IMU_CAP: usize = 2 * 4 * 3600;
/// A chart response never carries more points than this, whatever the range:
/// 900 points is already beyond the pixels a chart card has.
pub const MAX_CHART_POINTS: usize = 900;

/// One received frame. `at` is the backend's receive time: ENV/IMU frames
/// carry no clock of their own.
#[derive(Debug, Clone)]
pub struct Sample {
    pub at: DateTime<Utc>,
    pub data: Value,
}

#[derive(Default)]
pub struct SensorLog {
    env: VecDeque<Sample>,
    imu: VecDeque<Sample>,
}

impl SensorLog {
    pub fn push_env(&mut self, data: Value) {
        push_capped(&mut self.env, data, ENV_CAP);
    }

    pub fn push_imu(&mut self, data: Value) {
        push_capped(&mut self.imu, data, IMU_CAP);
    }

    /// ENV samples from the last `minutes`, oldest first, thinned by stride
    /// so the response stays small. A short spike can fall between strides on
    /// the 1 h view; the Excel export reads the raw log untouched.
    pub fn env_chart(&self, minutes: i64) -> Vec<Value> {
        chart_rows(&self.env, minutes)
    }

    pub fn env_snapshot(&self) -> Vec<Sample> {
        self.env.iter().cloned().collect()
    }

    pub fn imu_snapshot(&self) -> Vec<Sample> {
        self.imu.iter().cloned().collect()
    }

    pub fn env_len(&self) -> usize {
        self.env.len()
    }

    pub fn imu_len(&self) -> usize {
        self.imu.len()
    }
}

fn push_capped(ring: &mut VecDeque<Sample>, data: Value, cap: usize) {
    ring.push_back(Sample {
        at: Utc::now(),
        data,
    });
    while ring.len() > cap {
        ring.pop_front();
    }
}

fn chart_rows(ring: &VecDeque<Sample>, minutes: i64) -> Vec<Value> {
    let cutoff = Utc::now() - chrono::Duration::minutes(minutes.max(1));
    let fresh: Vec<&Sample> = ring.iter().filter(|s| s.at >= cutoff).collect();
    if fresh.is_empty() {
        return Vec::new();
    }
    let step = fresh.len().div_ceil(MAX_CHART_POINTS);
    fresh
        .iter()
        .step_by(step)
        .map(|s| {
            let mut row = json!({"at": s.at.timestamp_millis()});
            if let (Value::Object(map), Value::Object(out)) = (&s.data, &mut row) {
                out.extend(map.clone());
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(minutes_ago: i64, co: f64) -> Sample {
        Sample {
            at: Utc::now() - chrono::Duration::minutes(minutes_ago),
            data: json!({"co": co}),
        }
    }

    #[test]
    fn ring_trims_to_capacity() {
        let mut ring = VecDeque::new();
        for i in 0..5 {
            push_capped(&mut ring, json!({"n": i}), 3);
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.front().unwrap().data["n"], 2, "oldest dropped first");
    }

    #[test]
    fn chart_window_drops_old_samples() {
        let mut ring = VecDeque::new();
        ring.push_back(sample(30, 1.0));
        ring.push_back(sample(0, 2.0));
        let rows = chart_rows(&ring, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["co"], 2.0);
        assert!(rows[0]["at"].is_i64());
    }

    #[test]
    fn chart_thins_long_windows() {
        let mut ring = VecDeque::new();
        for _ in 0..(MAX_CHART_POINTS * 2 + 7) {
            ring.push_back(sample(0, 1.0));
        }
        assert!(chart_rows(&ring, 60).len() <= MAX_CHART_POINTS);
    }

    #[test]
    fn empty_ring_gives_empty_chart() {
        assert!(chart_rows(&VecDeque::new(), 10).is_empty());
    }
}
