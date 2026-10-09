//! Port of `backend/history_store.py` (writing + queries).
//! Excel export lives in `export.rs` (rust_xlsxwriter).
//! Day files: `history/history_YYYY-MM-DD.jsonl`; local-tz dates like Python.

use chrono::Local;
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::data::{coerce_num, py_bool};

pub const GAP_SECONDS: f64 = 5.0;

/// One JSONL row. `motor` stays raw (Python stores `data.get(...)` verbatim).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub ts: f64,
    pub iso: String,
    pub motor: Value,
    pub speed: i64,
    pub temp: f64,
    pub voltage: f64,
    pub current: f64,
    pub vibration: u8,
}

/// Aggregated `(motor, direction, speed)` run — keys match Python exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub start_ts: f64,
    pub start_iso: String,
    pub duration_s: f64,
    pub motor: Value,
    pub speed: i64,
    pub speed_pct: i64,
    pub direction: String,
    pub avg_temp: f64,
    pub max_temp: f64,
    pub avg_voltage: f64,
    pub avg_current: f64,
    pub vibration_count: i64,
    pub samples: usize,
}

pub struct HistoryStore {
    pub dir: PathBuf,
    /// Open append handle for the current day. Previously every sample did a
    /// full open/write/close (~16×/s on the async executor); now the handle
    /// persists and rotates on day rollover. Flushed per sample, so durability
    /// matches the old close-per-write behavior.
    out: Option<(String, BufWriter<std::fs::File>)>,
}

fn day_str(ts: f64) -> String {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn time_str(ts: f64) -> String {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|d| d.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

fn datetime_str(ts: f64) -> String {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn direction_of(speed: i64) -> &'static str {
    if speed > 0 {
        "FWD"
    } else if speed < 0 {
        "REV"
    } else {
        "IDLE"
    }
}

impl HistoryStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        Self { dir, out: None }
    }

    fn day_path(&self, day: &str) -> PathBuf {
        self.dir.join(format!("history_{day}.jsonl"))
    }

    /// 1:1 with `add_sample`: append one motor sample to today's JSONL.
    /// Returns the written row (useful for tests).
    pub fn add_sample(&mut self, data: &Value) -> Option<Sample> {
        let now = now_secs();
        let row = Sample {
            ts: now,
            iso: datetime_str(now),
            motor: data
                .get("motor")
                .or_else(|| data.get("id"))
                .cloned()
                .unwrap_or(Value::from(0)),
            speed: crate::data::coerce_i64(data.get("speed")),
            temp: coerce_num(data.get("temp").or_else(|| data.get("temperature")), 0.0),
            voltage: coerce_num(data.get("voltage"), 0.0),
            current: coerce_num(data.get("current"), 0.0),
            vibration: u8::from(py_bool(data.get("vibration"))),
        };
        let line = serde_json::to_string(&row).ok()?;
        let day = day_str(now);
        let rotated = self.out.as_ref().is_none_or(|(d, _)| *d != day);
        if rotated {
            let path = self.day_path(&day);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            self.out = Some((day, BufWriter::new(file)));
        }
        let (_, w) = self.out.as_mut()?;
        {
            use std::io::Write as _;
            writeln!(w, "{line}").ok()?;
            w.flush().ok()?;
        }
        Some(row)
    }

    /// 1:1 with `days()`: day strings, newest first.
    pub fn days(&self) -> Vec<String> {
        day_list(&self.dir)
    }

    /// 1:1 with `_read_day`: parsed rows, skipping blank/corrupt lines.
    pub fn read_day(&self, day: &str) -> Vec<Value> {
        read_day_file(&self.day_path(day))
    }

    /// 1:1 with `segments(day)`: splits on motor/direction/speed change or
    /// a gap `> 5s`, then aggregates per segment.
    pub fn segments(&self, day: &str) -> Vec<Segment> {
        segments_of(&self.read_day(day))
    }
}

/// Day strings in `dir`, newest first (backing `HistoryStore::days`, also
/// usable without holding the store lock — see the export path).
pub fn day_list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = vec![];
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("history_") && name.ends_with(".jsonl") {
            out.push(name["history_".len()..name.len() - ".jsonl".len()].to_string());
        }
    }
    out.sort();
    out.reverse();
    out
}

pub fn read_day_file(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn segments_of(rows: &[Value]) -> Vec<Segment> {
    #[derive(Debug)]
    struct Acc {
        motor: Value,
        speed: i64,
        direction: String,
        start_ts: f64,
        end_ts: f64,
        sum_temp: f64,
        max_temp: Option<f64>,
        sum_voltage: f64,
        sum_current: f64,
        vib_count: i64,
        samples: usize,
    }
    let mut accs: Vec<Acc> = vec![];
    let mut cur: Option<usize> = None;

    for r in rows {
        let ts = r.get("ts").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let motor = r.get("motor").cloned().unwrap_or(Value::from(0));
        let speed = crate::data::coerce_i64(r.get("speed"));
        let dir = direction_of(speed).to_string();
        let temp = coerce_num(r.get("temp"), 0.0);
        let voltage = coerce_num(r.get("voltage"), 0.0);
        let current = coerce_num(r.get("current"), 0.0);
        let vib = i64::from(py_bool(r.get("vibration")));

        let split = match cur {
            None => true,
            Some(i) => {
                let c = &accs[i];
                c.motor != motor
                    || c.direction != dir
                    || c.speed != speed
                    || (ts - c.end_ts) > GAP_SECONDS
            }
        };
        if split {
            accs.push(Acc {
                motor,
                speed,
                direction: dir,
                start_ts: ts,
                end_ts: ts,
                sum_temp: 0.0,
                max_temp: None,
                sum_voltage: 0.0,
                sum_current: 0.0,
                vib_count: 0,
                samples: 0,
            });
            cur = Some(accs.len() - 1);
        }
        let c = &mut accs[cur.unwrap_or(0)];
        c.end_ts = ts;
        c.samples += 1;
        c.sum_temp += temp;
        c.sum_voltage += voltage;
        c.sum_current += current;
        c.vib_count += vib;
        if c.max_temp.is_none_or(|m| temp > m) {
            c.max_temp = Some(temp);
        }
    }

    accs.into_iter()
        .map(|s| {
            let n = s.samples.max(1) as f64;
            Segment {
                start_ts: crate::data::py_round(s.start_ts, 2),
                start_iso: time_str(s.start_ts),
                duration_s: crate::data::py_round(s.end_ts - s.start_ts, 1),
                motor: s.motor,
                speed: s.speed,
                speed_pct: crate::data::py_round((s.speed.abs() as f64) / 255.0 * 100.0, 0) as i64,
                direction: s.direction,
                avg_temp: crate::data::py_round(s.sum_temp / n, 2),
                max_temp: crate::data::py_round(s.max_temp.unwrap_or(0.0), 2),
                avg_voltage: crate::data::py_round(s.sum_voltage / n, 2),
                avg_current: crate::data::py_round(s.sum_current / n, 2),
                vibration_count: s.vib_count,
                samples: s.samples,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(ts: f64, motor: i64, speed: i64, temp: f64) -> Value {
        json!({"ts": ts, "motor": motor, "speed": speed,
               "temp": temp, "voltage": 12.0, "current": 0.5, "vibration": 0})
    }

    #[test]
    fn splits_on_dir_speed_motor_and_gap() {
        let rows = vec![
            row(100.0, 1, 128, 40.0),
            row(101.0, 1, 128, 41.0),  // same segment
            row(102.0, 1, -128, 42.0), // dir change
            row(103.0, 1, 64, 43.0),   // speed change
            row(104.0, 2, 64, 44.0),   // motor change
            row(200.0, 2, 64, 45.0),   // gap > 5s
        ];
        let segs = segments_of(&rows);
        assert_eq!(segs.len(), 5);
        assert_eq!(segs[0].samples, 2);
        assert_eq!(segs[0].direction, "FWD");
        assert_eq!(segs[0].speed_pct, 50); // round(128/255*100)
        assert_eq!(segs[0].avg_temp, 40.5);
        assert_eq!(segs[0].max_temp, 41.0);
        assert_eq!(segs[0].duration_s, 1.0);
        assert_eq!(segs[1].direction, "REV");
        assert_eq!(segs[4].duration_s, 0.0);
    }

    #[test]
    fn empty_and_missing_day_give_no_segments() {
        assert!(segments_of(&[]).is_empty());
        let h = HistoryStore::new(std::env::temp_dir().join("dtd-nonexistent-xyz"));
        assert!(h.segments("2099-01-01").is_empty());
    }

    #[test]
    fn days_sorted_newest_first() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-days-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h = HistoryStore::new(&dir);
        for d in ["2026-10-06", "2026-10-08", "2026-10-07"] {
            std::fs::write(h.day_path(d), "").unwrap();
        }
        assert_eq!(h.days(), vec!["2026-10-08", "2026-10-07", "2026-10-06"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_sample_appends_jsonl_row() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-add-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut h = HistoryStore::new(&dir);
        let pkt = json!({"motor": 1, "speed": 128, "temp": 42.5,
                         "voltage": 12.1, "current": 0.6, "vibration": 1});
        let row = h.add_sample(&pkt).expect("write row");
        assert_eq!(row.speed, 128);
        assert_eq!(row.vibration, 1);
        // Row lands in today's file and segments() sees it.
        let today = day_str(row.ts);
        let segs = h.segments(&today);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].samples, 1);
        assert_eq!(segs[0].speed_pct, 50);
        std::fs::remove_dir_all(&dir).ok();
    }
}
