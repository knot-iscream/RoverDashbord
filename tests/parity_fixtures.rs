//! Golden-fixture parity vs the v1 Python backend.
//!
//! Fixtures (`tests/fixtures/*.json`) were captured by replaying crafted
//! packets, recorded history, and export output through the ORIGINAL Python
//! modules. Python is never needed again: these tests replay the same inputs
//! through the Rust port and demand the same outputs. If a fixture fails,
//! either the port drifted or v1's behavior was deliberately fixed — both
//! demand a conscious decision, never a silent diff.

use std::path::{Path, PathBuf};

use digital_twin_dashboard::{
    calibration::CalibrationManager, data::DataHandler, history::segments_of,
};
use serde_json::{json, Value};

fn fixture(name: &str) -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Numbers compare numerically, recursively (Python `100` == Rust `100.0`
/// even nested); everything else — notably string-vs-number (`"1"` vs `1`)
/// and null — compares exactly.
fn norm_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| norm_eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| norm_eq(v, w)))
        }
        _ => a == b,
    }
}

fn assert_norm(got: &Value, want: &Value, ctx: &str) {
    assert!(
        norm_eq(got, want),
        "{ctx} mismatch:\n  got:  {got}\n  want: {want}"
    );
}

fn test_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dtd-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn health_matches_goldens() {
    for c in fixture("health") {
        let name = c["case"].as_str().unwrap();
        if name == "unknown_motor" {
            let h = DataHandler::default();
            let res = h.compute_health(99);
            assert_norm(&json!(res.health), &c["health"], "unknown_motor.health");
            assert!(res.anomalies.is_empty());
            continue;
        }
        // Default, not new(): hermetic — never touches calibration_baseline.json.
        let mut h = DataHandler::default();
        if name == "baseline_drift" {
            h.baselines
                .insert(1, json!({"avg_temp": 35.0, "avg_voltage": 12.0}));
        }
        let res = h.process_motor_data(&c["packet"]);
        assert_norm(&json!(res.health), &c["health"], &format!("{name}.health"));
        assert_norm(
            &json!(res.anomalies),
            &c["anomalies"],
            &format!("{name}.anomalies"),
        );
        if name == "string_motor" {
            // Documented divergence: Python keys latest_data by the RAW motor
            // id ("4"); Rust normalizes to u8 (non-numeric → 0). Health is
            // identical; only the cache key differs. Firmware always sends
            // ints 1-4, so this never occurs on the wire.
            assert_eq!(h.latest.get(&0).map(|s| s.id), Some(0));
            continue;
        }
        let key = c["packet"]
            .get("motor")
            .or_else(|| c["packet"].get("id"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        let mut got = serde_json::to_value(&h.latest[&key]).unwrap();
        got.as_object_mut().unwrap().remove("timestamp");
        assert_norm(&got, &c["latest"], &format!("{name}.latest"));
    }
}

fn val<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn calibration_matches_goldens() {
    let cases = fixture("calibration");
    let by_name = |n: &str| cases.iter().find(|c| c["case"] == n).unwrap().clone();
    let mut c = CalibrationManager::new();
    assert_norm(
        &val(&c.status()),
        &by_name("fresh_status")["status"],
        "fresh_status",
    );
    assert_norm(
        &val(&c.start()),
        &by_name("start_result")["result"],
        "start_result",
    );
    // Warmup pct rounds a microsecond elapsed to 0.0 — deterministic.
    assert_norm(
        &val(&c.status()),
        &by_name("warming_status")["status"],
        "warming_status",
    );
    c.handle_esp32_status(&by_name("sweep_status")["input"]);
    assert_norm(
        &val(&c.status()),
        &by_name("sweep_status")["status"],
        "sweep_status",
    );
    c.handle_esp32_status(&json!({"state": "idle"}));
    assert_norm(
        &val(&c.status()),
        &by_name("idle_after_sweep")["status"],
        "idle_after_sweep",
    );
    let mut c2 = CalibrationManager::new();
    c2.start();
    c2.count_sample();
    c2.count_sample();
    assert_norm(
        &val(&c2.stop()),
        &by_name("stop_result")["result"],
        "stop_result",
    );
}

#[test]
fn segments_match_goldens() {
    for c in fixture("segments") {
        let name = c["case"].as_str().unwrap();
        let rows: Vec<Value> = serde_json::from_value(c["input_rows"].clone()).unwrap();
        let got = serde_json::to_value(segments_of(&rows)).unwrap();
        assert_norm(&got, &c["segments"], &format!("{name}.segments"));
    }
}

// ── xlsx reader (test-only): unzip + parse sheet XML ─────────────────────

fn read_zip(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = vec![];
    std::io::Read::read_to_end(&mut f, &mut buf).ok()?;
    Some(buf)
}

fn parse_sst(xml: &[u8]) -> Vec<String> {
    use quick_xml::{events::Event, Reader};
    let mut r = Reader::from_reader(xml);
    r.trim_text(true);
    let mut buf = vec![];
    let mut out = vec![];
    let mut cur = String::new();
    let mut in_t = false;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"t" => in_t = true,
            Ok(Event::End(e)) if e.name().as_ref() == b"t" => in_t = false,
            Ok(Event::End(e)) if e.name().as_ref() == b"si" => {
                out.push(std::mem::take(&mut cur));
            }
            Ok(Event::Text(e)) if in_t => cur.push_str(&e.unescape().unwrap()),
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn col_idx(cell_ref: &[u8]) -> usize {
    cell_ref
        .iter()
        .take_while(|b| b.is_ascii_alphabetic())
        .fold(0usize, |a, b| a * 26 + (b - b'A' + 1) as usize)
        .saturating_sub(1)
}

/// Sheet rows as cell matrices, placed by `r` attribute (robust to omitted
/// empties), padded to 10 columns. Numbers → f64, shared/inline strings →
/// string, empty → null.
fn parse_sheet(xml: &[u8], shared: &[String]) -> Vec<Vec<Value>> {
    use quick_xml::{events::Event, Reader};
    let mut r = Reader::from_reader(xml);
    r.trim_text(true);
    let mut buf = vec![];
    let mut rows: Vec<Vec<Value>> = vec![];
    let mut cur: Option<Vec<Value>> = None;
    let mut ctype: Vec<u8> = vec![];
    let mut cref: Vec<u8> = vec![];
    let mut text = String::new();
    let mut in_v = false;
    let mut in_t = false;
    let push_cell = |row: &mut Vec<Value>, typ: &[u8], rf: &[u8], t: &str| {
        let v = match typ {
            b"s" => t
                .parse::<usize>()
                .ok()
                .and_then(|i| shared.get(i))
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null),
            _ => t
                .parse::<f64>()
                .map(Value::from)
                .unwrap_or_else(|_| Value::String(t.to_string())),
        };
        if !rf.is_empty() {
            let idx = col_idx(rf);
            while row.len() < idx {
                row.push(Value::Null);
            }
            if row.len() == idx {
                row.push(v);
            } else {
                row[idx] = v;
            }
        } else {
            row.push(v);
        }
    };
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"row" => cur = Some(vec![]),
                b"c" => {
                    ctype.clear();
                    cref.clear();
                    for a in e.attributes().flatten() {
                        match a.key.as_ref() {
                            b"t" => ctype = a.value.into_owned(),
                            b"r" => cref = a.value.into_owned(),
                            _ => {}
                        }
                    }
                    text.clear();
                    in_v = false;
                    in_t = false;
                }
                b"v" => in_v = true,
                b"t" => in_t = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.name().as_ref() == b"c" => {
                if let Some(row) = cur.as_mut() {
                    let mut rf = vec![];
                    for a in e.attributes().flatten() {
                        if a.key.as_ref() == b"r" {
                            rf = a.value.into_owned();
                        }
                    }
                    push_cell(row, b"", &rf, "");
                }
            }
            Ok(Event::Text(e)) => {
                if in_v || in_t {
                    text.push_str(&e.unescape().unwrap());
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"v" => in_v = false,
                b"t" => in_t = false,
                b"c" => {
                    if let Some(row) = cur.as_mut() {
                        push_cell(row, &ctype.clone(), &cref.clone(), &text.clone());
                    }
                }
                b"row" => {
                    if let Some(row) = cur.take() {
                        rows.push(row);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    for row in &mut rows {
        while row.len() < 10 {
            row.push(Value::Null);
        }
    }
    rows
}

fn read_xlsx_matrix(path: &Path) -> Vec<Vec<Value>> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let sst = read_zip(&mut zip, "xl/sharedStrings.xml")
        .map(|x| parse_sst(&x))
        .unwrap_or_default();
    let sheet = read_zip(&mut zip, "xl/worksheets/sheet1.xml").unwrap();
    parse_sheet(&sheet, &sst)
}

#[test]
fn export_matches_golden_cells() {
    let exp = &fixture("export")[0];
    let seg = fixture("segments");
    let synth = seg.iter().find(|c| c["case"] == "synthetic").unwrap();
    let rows: Vec<Value> = serde_json::from_value(synth["input_rows"].clone()).unwrap();

    // Write rows RAW (bypass add_sample coercion — the point is cell-level
    // parity on exactly what Python read, including null temp and "1" motor).
    // The corrupt + blank lines the golden file had are included to pin
    // line-skipping on both sides.
    let dir = test_tmp("parity-export");
    let mut text = String::new();
    for (i, r) in rows.iter().enumerate() {
        text.push_str(&serde_json::to_string(r).unwrap());
        text.push('\n');
        if i == 1 {
            text.push_str("{corrupt line\n\n");
        }
    }
    std::fs::write(dir.join("history_synth.jsonl"), &text).unwrap();

    let out =
        digital_twin_dashboard::export::export_excel_dir(&dir, Some(&dir.join("got.xlsx")), &dir)
            .expect("export produced a file");
    let matrix = read_xlsx_matrix(&out);

    let want_header: Vec<Value> = exp["header"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Value::String(v.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(matrix[0], want_header, "header mismatch");

    let want_rows = exp["rows"].as_array().unwrap();
    assert_eq!(
        matrix.len() - 1,
        want_rows.len(),
        "row count: got {}, want {}",
        matrix.len() - 1,
        want_rows.len()
    );
    for (i, want_row) in want_rows.iter().enumerate() {
        for (j, want) in want_row.as_array().unwrap().iter().enumerate() {
            let got = &matrix[i + 1][j];
            // openpyxl reads empty cells as None while the sheet holds "";
            // same meaning, unified here, nowhere else.
            let ok = norm_eq(got, want) || (want.is_null() && got == &json!(""));
            assert!(ok, "cell r{}c{}: got {got}, want {want}", i + 1, j);
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}
