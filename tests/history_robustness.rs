//! History store robustness: corrupt input, missing files, and day-file
//! edge cases. The contract: never panic, never lose good rows, skip junk.

mod common;

use digital_twin_dashboard::history::HistoryStore;
use serde_json::json;

fn store(tag: &str) -> (HistoryStore, PathBufGuard) {
    let dir = common::tmpdir(tag);
    let h = HistoryStore::new(&dir);
    (h, PathBufGuard(dir))
}

struct PathBufGuard(std::path::PathBuf);
impl Drop for PathBufGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn corrupt_blank_and_garbage_lines_are_skipped() {
    let (mut h, _g) = store("hist-corrupt");
    h.add_sample(&json!({"motor": 1, "speed": 10, "temp": 40.0}))
        .unwrap();
    let day_file = h.dir.join(format!(
        "history_{}.jsonl",
        chrono::Local::now().format("%Y-%m-%d")
    ));
    std::fs::write(
        &day_file,
        "{not json\n\n   \n[1,2\n{\"ts\":1,\"motor\":1}\n",
    )
    .unwrap();
    // read_day_file is free-standing: point it at the mangled file.
    let rows = digital_twin_dashboard::history::read_day_file(&day_file);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["motor"], 1);
}

#[test]
fn missing_file_and_dir_yield_empty() {
    let (h, _g) = store("hist-missing");
    assert!(h.read_day("2099-01-01").is_empty());
    assert!(h.segments("2099-01-01").is_empty());
    let gone = common::tmpdir("hist-gone");
    let h2 = HistoryStore::new(gone.join("nope"));
    std::fs::remove_dir_all(gone.join("nope")).ok();
    assert!(h2.days().is_empty());
}

#[test]
fn invalid_utf8_file_reads_empty_without_panic() {
    let (h, _g) = store("hist-utf8");
    let day_file = h.dir.join("history_2099-01-02.jsonl");
    std::fs::write(&day_file, b"\xff\xfe\x00bad\x01\n").unwrap();
    // read_to_string fails on invalid UTF-8 → whole file skipped (pinned).
    assert!(digital_twin_dashboard::history::read_day_file(&day_file).is_empty());
}

#[test]
fn day_listing_ignores_non_history_files() {
    let (h, _g) = store("hist-list");
    std::fs::write(h.dir.join("history_2099-01-03.jsonl"), "").unwrap();
    std::fs::write(h.dir.join("notes.txt"), "x").unwrap();
    std::fs::write(h.dir.join("history_tmp"), "x").unwrap();
    assert_eq!(h.days(), vec!["2099-01-03".to_string()]);
}

#[test]
fn samples_accumulate_in_todays_file() {
    let (mut h, _g) = store("hist-accum");
    for speed in [10, 20, 30] {
        h.add_sample(&json!({"motor": 1, "speed": speed, "temp": 40.0}))
            .unwrap();
    }
    let days = h.days();
    assert_eq!(days.len(), 1);
    assert_eq!(h.read_day(&days[0]).len(), 3);
    let segs = h.segments(&days[0]);
    assert_eq!(segs.len(), 3); // every speed change splits
    assert_eq!(segs.iter().map(|s| s.samples).sum::<usize>(), 3);
}
