//! Port of `HistoryStore.export_excel` (openpyxl → rust_xlsxwriter).
//! Exports ALL recorded telemetry to one `Telemetry` sheet. Returns `None`
//! when there is nothing to export (route then answers 400 like Python).

use std::path::{Path, PathBuf};

use rust_xlsxwriter::Workbook;
use serde_json::Value;

use crate::data::coerce_num;

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
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            base_dir.join(format!("history_export_{ts}_{}.xlsx", std::process::id()))
        }
    };

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Telemetry").ok()?;
    ws.write_row(0, 0, HEADERS).ok()?;

    let mut row: u32 = 1;
    for day in crate::history::day_list(dir) {
        let day_path = dir.join(format!("history_{day}.jsonl"));
        for r in crate::history::read_day_file(&day_path) {
            write_row(ws, row, &r);
            row += 1;
        }
    }
    if row == 1 {
        return None; // no data: don't leave an empty workbook behind
    }

    wb.save(&path).ok()?;
    Some(path)
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
}
