//! Port of `HistoryStore.export_excel` (openpyxl → rust_xlsxwriter).
//! Exports ALL recorded telemetry to one `Telemetry` sheet. Returns `None`
//! when there is nothing to export (route then answers 400 like Python).

use std::path::{Path, PathBuf};

use rust_xlsxwriter::Workbook;
use serde_json::Value;

use crate::data::coerce_num;
use crate::sensor_log::Sample;

const HEADERS: [&str; 10] = [
    "motor_id",
    "epoch",
    "timestamp",
    "speed",
    "speed_pct",
    "direction",
    "temp",
    "voltage",
    "current",
    "vibration",
];
/// Fixed column lists for the sensor sheets: a new firmware field joins the
/// workbook only when it is added here on purpose.
const ENV_KEYS: [&str; 17] = [
    "co",
    "lpg",
    "ch4",
    "h2",
    "air",
    "o2_mv",
    "o2_pct",
    "flame_lvl",
    "flame_dir",
    "heat_l",
    "heat_r",
    "heat_l_amb",
    "heat_r_amb",
    "ads_ok",
    "mlx_ok",
    "flame_ok",
    "baseline_ok",
];
const IMU_KEYS: [&str; 8] = ["yaw", "pitch", "roll", "ax", "ay", "az", "gz", "imu_ok"];

fn direction_of(speed: i64) -> &'static str {
    if speed > 0 {
        "FWD"
    } else if speed < 0 {
        "REV"
    } else {
        "IDLE"
    }
}

/// Workbook build from a history dir, without touching the store lock.
/// Streams one day file at a time (never loads all of history into RAM),
/// so the route can run this on a blocking thread with ingest unblocked.
pub fn export_excel_dir(dir: &Path, filepath: Option<&Path>, base_dir: &Path) -> Option<PathBuf> {
    let path = match filepath {
        Some(p) => p.to_path_buf(),
        None => default_path(base_dir),
    };

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Telemetry").ok()?;
    ws.write_row(0, 0, HEADERS).ok()?;

    if write_telemetry_rows(ws, dir) == 1 {
        return None; // no data: don't leave an empty workbook behind
    }

    wb.save(&path).ok()?;
    Some(path)
}

/// Detailed-page workbook: `Telemetry` (motors, the same rows the calibration
/// export writes) + `Sensors` (ENV ring) + `IMU` (ring). `None` only when
/// there is nothing at all to write, so the route keeps its 400 semantics.
pub fn export_excel_all(
    dir: &Path,
    env: &[Sample],
    imu: &[Sample],
    filepath: Option<&Path>,
    base_dir: &Path,
) -> Option<PathBuf> {
    let path = match filepath {
        Some(p) => p.to_path_buf(),
        None => default_path(base_dir),
    };

    let mut wb = Workbook::new();
    let mut wrote = false;

    let ws = wb.add_worksheet();
    ws.set_name("Telemetry").ok()?;
    ws.write_row(0, 0, HEADERS).ok()?;
    if write_telemetry_rows(ws, dir) > 1 {
        wrote = true;
    }
    if !env.is_empty() {
        let ws = wb.add_worksheet();
        ws.set_name("Sensors").ok()?;
        write_sensor_sheet(ws, env, &ENV_KEYS);
        wrote = true;
    }
    if !imu.is_empty() {
        let ws = wb.add_worksheet();
        ws.set_name("IMU").ok()?;
        write_sensor_sheet(ws, imu, &IMU_KEYS);
        wrote = true;
    }
    if !wrote {
        return None; // nothing anywhere: no empty workbook behind
    }

    wb.save(&path).ok()?;
    Some(path)
}

fn default_path(base_dir: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    base_dir.join(format!("history_export_{ts}_{}.xlsx", std::process::id()))
}

/// Stream one day file at a time (never loads all of history into RAM).
/// Returns the next free row, so the caller can tell "header only" apart.
fn write_telemetry_rows(ws: &mut rust_xlsxwriter::Worksheet, dir: &Path) -> u32 {
    let mut row: u32 = 1;
    for day in crate::history::day_list(dir) {
        let day_path = dir.join(format!("history_{day}.jsonl"));
        for r in crate::history::read_day_file(&day_path) {
            write_row(ws, row, &r);
            row += 1;
        }
    }
    row
}

/// One sensor sheet: timestamp + epoch_ms then the fixed key list. Missing or
/// null values stay empty cells, same rule as the Telemetry sheet.
fn write_sensor_sheet(ws: &mut rust_xlsxwriter::Worksheet, rows: &[Sample], keys: &[&str]) {
    let mut header: Vec<String> = vec!["timestamp".into(), "epoch_ms".into()];
    header.extend(keys.iter().map(|k| (*k).to_string()));
    let _ = ws.write_row(0, 0, header);
    for (i, s) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        let _ = ws.write_string(r, 0, s.at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
        let _ = ws.write_number(r, 1, s.at.timestamp_millis() as f64);
        for (c, key) in keys.iter().enumerate() {
            write_cell(ws, r, (c + 2) as u16, s.data.get(*key));
        }
    }
}

fn write_row(ws: &mut rust_xlsxwriter::Worksheet, row: u32, r: &Value) {
    let speed = crate::data::coerce_i64(r.get("speed"));
    let speed_pct = crate::data::py_round((speed.abs() as f64) / 255.0 * 100.0, 0) as i64;
    // Raw values, like openpyxl's `ws.append`: strings stay strings, numbers
    // stay numbers, null/missing stays an empty cell. (The one exception is
    // bool → 1/0: openpyxl writes TRUE/FALSE cells. Unreachable in practice —
    // firmware sends 0/1 — and pinned here so it can't drift silently.)
    write_cell(ws, row, 0, r.get("motor"));
    write_cell(ws, row, 1, r.get("ts").or_else(|| r.get("timestamp")));
    let iso = r
        .get("iso")
        .or_else(|| r.get("timestamp"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let _ = ws.write_string(row, 2, iso);
    let _ = ws.write_number(row, 3, speed as f64);
    let _ = ws.write_number(row, 4, speed_pct as f64);
    let _ = ws.write_string(row, 5, direction_of(speed));
    write_cell(ws, row, 6, r.get("temp"));
    write_cell(ws, row, 7, r.get("voltage"));
    write_cell(ws, row, 8, r.get("current"));
    write_cell(ws, row, 9, r.get("vibration"));
}

/// Mirror of openpyxl's untyped `ws.append` for one cell. Missing/null writes
/// nothing (empty cell, reads back as None — exactly like openpyxl).
fn write_cell(ws: &mut rust_xlsxwriter::Worksheet, row: u32, col: u16, v: Option<&Value>) {
    match v {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => {
            let _ = ws.write_string(row, col, s);
        }
        Some(Value::Bool(b)) => {
            let _ = ws.write_number(row, col, f64::from(*b));
        }
        Some(Value::Number(_)) => {
            let _ = ws.write_number(row, col, coerce_num(v, 0.0));
        }
        Some(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryStore;
    use serde_json::json;

    #[test]
    fn empty_store_exports_nothing() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-xlsx-empty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h = HistoryStore::new(&dir);
        assert!(export_excel_dir(&h.dir, None, &dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_writes_telemetry_sheet() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-xlsx-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut h = HistoryStore::new(&dir);
        h.add_sample(&json!({"motor": 1, "speed": -64, "temp": 41.0,
                             "voltage": 12.2, "current": 0.7, "vibration": 0}));
        let out = dir.join("out.xlsx");
        let got = export_excel_dir(&h.dir, Some(&out), &dir).expect("export path");
        assert_eq!(got, out);
        assert!(out.exists());
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_all_adds_sensor_and_imu_sheets() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-xlsx-all-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h = HistoryStore::new(&dir);
        let at = chrono::Utc::now();
        let env = vec![Sample {
            at,
            data: json!({"co": 1.2, "o2_pct": 20.9}),
        }];
        let imu = vec![Sample {
            at,
            data: json!({"yaw": 1.5}),
        }];
        let out = dir.join("all.xlsx");
        let got = export_excel_all(&h.dir, &env, &imu, Some(&out), &dir).expect("export path");
        assert_eq!(got, out);
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_all_none_when_everything_empty() {
        let dir = std::env::temp_dir().join(format!(
            "dtd-xlsx-none-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h = HistoryStore::new(&dir);
        assert!(export_excel_all(&h.dir, &[], &[], None, &dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
