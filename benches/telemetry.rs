//! Backend throughput benches — real numbers for "what did Rust gain".
//! Run with `cargo bench`. Compares nothing against Python directly (that
//! needs the .venv side); it quantifies the Rust side's per-message cost so
//! the "backend is not the bottleneck" claim is measured, not asserted.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::json;

use digital_twin_dashboard::{
    data::DataHandler,
    export::export_excel_dir,
    history::{segments_of, HistoryStore},
};

fn packet(motor: i64, speed: i64, temp: f64) -> serde_json::Value {
    json!({"motor": motor, "speed": speed, "temp": temp, "temp_valid": true,
           "voltage": 12.0, "current": 0.5, "ina_ok": true,
           "vibration": 0, "vibration_valid": true})
}

fn bench_health(c: &mut Criterion) {
    c.bench_function("health_of nominal", |b| {
        b.iter(|| {
            DataHandler::health_of(
                black_box(Some(35.0)),
                black_box(Some(12.0)),
                black_box(Some(0.5)),
                black_box(false),
                black_box(true),
                black_box(true),
                black_box(true),
                black_box(None),
            )
        })
    });
    c.bench_function("process_motor_data full packet", |b| {
        let mut h = DataHandler::default();
        let p = packet(1, 128, 42.5);
        b.iter(|| h.process_motor_data(black_box(&p)))
    });
}

fn bench_segments(c: &mut Criterion) {
    // 10k rows, one segment (steady cruise) — the common dashboard query.
    let rows: Vec<serde_json::Value> = (0..10_000)
        .map(|i| {
            json!({"ts": 1_000_000.0 + i as f64 * 0.25, "motor": 1, "speed": 128,
                   "temp": 40.0, "voltage": 12.0, "current": 0.5, "vibration": 0})
        })
        .collect();
    c.bench_function("segments_of 10k rows", |b| {
        b.iter(|| segments_of(black_box(&rows)))
    });
    // End-to-end like the real `/api/history/segments` path (file read + split).
    let dir = std::env::temp_dir().join(format!(
        "dtd-bench-seg-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history_b.jsonl");
    let text = rows
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &text).unwrap();
    c.bench_function("segments end-to-end 10k rows", |b| {
        b.iter(|| {
            let rows = digital_twin_dashboard::history::read_day_file(black_box(&path));
            segments_of(&rows)
        })
    });
    std::fs::remove_dir_all(&dir).ok();
}

fn bench_export(c: &mut Criterion) {
    let dir = std::env::temp_dir().join(format!(
        "dtd-bench-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let h = HistoryStore::new(&dir);
    let mut store = h;
    for i in 0..1_000 {
        store
            .add_sample(&packet(1, 128, 40.0 + (i % 10) as f64 * 0.1))
            .unwrap();
    }
    drop(store);
    c.bench_function("export_excel_dir 1k rows", |b| {
        b.iter(|| {
            export_excel_dir(
                black_box(&dir),
                black_box(Some(&dir.join("bench.xlsx"))),
                black_box(&dir),
            )
        })
    });
    std::fs::remove_dir_all(&dir).ok();
}

criterion_group!(benches, bench_health, bench_segments, bench_export);
criterion_main!(benches);
