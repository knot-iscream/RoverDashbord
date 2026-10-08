//! Port of `HistoryStore.export_excel` (openpyxl → rust_xlsxwriter).
//! Exports ALL recorded telemetry to one `Telemetry` sheet. Returns `None`
//! when there is nothing to export (route then answers 400 like Python).

use std::path::{Path, PathBuf};

use rust_xlsxwriter::Workbook;

use crate::data::{coerce_num, py_bool};
use crate::history::HistoryStore;

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

pub fn export_excel(store: &HistoryStore, filepath: Option<&Path>, base_dir: &Path) -> Option<PathBuf> {
    let mut rows = vec![];
    for day in store.days() {
        rows.extend(store.read_day(&day));
    }
    if rows.is_empty() {
        return None;
    }
    let path = match filepath {
        Some(p) => p.to_path_buf(),
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            base_dir.join(format!("history_export_{ts}.xlsx"))
        }
    };

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Telemetry").ok()?;
    ws.write_row(0, 0, HEADERS).ok()?;

    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        let speed = r.get("speed").and_then(|v| v.as_i64()).unwrap_or(0);
        let speed_pct = ((speed.abs() as f64) / 255.0 * 100.0).round() as i64;
        let _ = ws.write_number(row, 0, coerce_num(r.get("motor"), 0.0));
        let ts = coerce_num(r.get("ts").or_else(|| r.get("timestamp")), 0.0);
        let _ = ws.write_number(row, 1, ts);
        let iso = r
            .get("iso")
            .or_else(|| r.get("timestamp"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let _ = ws.write_string(row, 2, iso);
        let _ = ws.write_number(row, 3, speed as f64);
        let _ = ws.write_number(row, 4, speed_pct as f64);
        let _ = ws.write_string(row, 5, direction_of(speed));
        let _ = ws.write_number(row, 6, coerce_num(r.get("temp"), 0.0));
        let _ = ws.write_number(row, 7, coerce_num(r.get("voltage"), 0.0));
        let _ = ws.write_number(row, 8, coerce_num(r.get("current"), 0.0));
        let _ = ws.write_number(row, 9, f64::from(py_bool(r.get("vibration"))));
    }

    wb.save(&path).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(export_excel(&h, None, &dir).is_none());
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
        let h = HistoryStore::new(&dir);
        h.add_sample(&json!({"motor": 1, "speed": -64, "temp": 41.0,
                             "voltage": 12.2, "current": 0.7, "vibration": 0}));
        let out = dir.join("out.xlsx");
        let got = export_excel(&h, Some(&out), &dir).expect("export path");
        assert_eq!(got, out);
        assert!(out.exists());
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
