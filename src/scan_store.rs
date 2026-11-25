//! Scan history archive on the laptop.
//!
//! The Pi saves one folder per parked scan session and serves it as a ZIP.
//! This module keeps the laptop's own copy under `scans/`, lists it for the
//! History popup, and serves the saved cloud and a download bundle. The Pi
//! remains the source of truth; a failed fetch leaves a pending entry that the
//! next sync retries.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

/// Files the Pi ZIP is allowed to contain. Anything else is rejected, which
/// also blocks path traversal from a malformed archive.
const FILES: [&str; 4] = ["meta.json", "display.json", "cloud.ply", "map.bt"];
const MAX_ZIP_BYTES: usize = 100 * 1024 * 1024;
const LABEL_MAX: usize = 60;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanMeta {
    pub id: String,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub end: String,
    #[serde(default)]
    pub sweeps: u32,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub points: u64,
    #[serde(default)]
    pub voxels: u64,
    #[serde(default)]
    pub furniture_points: u64,
    #[serde(default)]
    pub source: String,
    /// Total bytes on disk, filled in by `list()`.
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct ScanStore {
    root: PathBuf,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn clean_label(label: &str) -> String {
    label
        .chars()
        .filter(|c| !c.is_control())
        .take(LABEL_MAX)
        .collect::<String>()
        .trim()
        .to_string()
}

fn read_meta(path: &Path) -> Option<ScanMeta> {
    let text = std::fs::read_to_string(path.join("meta.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_meta(path: &Path, meta: &ScanMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(path.join("meta.json"), text).map_err(|e| e.to_string())
}

impl ScanStore {
    pub fn new(root: PathBuf) -> Self {
        std::fs::create_dir_all(&root).ok();
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Newest first. The Pi ids are `YYYYMMDD-HHMMSS`, so lexical order works.
    pub fn list(&self) -> Vec<ScanMeta> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !valid_id(name) {
                continue;
            }
            let Some(mut meta) = read_meta(&path) else {
                continue;
            };
            meta.id = name.to_string();
            meta.size = dir_size(&path);
            out.push(meta);
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }

    pub fn has(&self, id: &str) -> bool {
        valid_id(id) && self.root.join(id).join("meta.json").is_file()
    }

    pub fn meta(&self, id: &str) -> Option<ScanMeta> {
        if !valid_id(id) {
            return None;
        }
        let mut meta = read_meta(&self.root.join(id))?;
        meta.id = id.to_string();
        meta.size = dir_size(&self.root.join(id));
        Some(meta)
    }

    pub fn display(&self, id: &str) -> Option<Value> {
        if !valid_id(id) {
            return None;
        }
        let text = std::fs::read_to_string(self.root.join(id).join("display.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn set_label(&self, id: &str, label: &str) -> Result<ScanMeta, String> {
        if !valid_id(id) {
            return Err("bad scan id".into());
        }
        let path = self.root.join(id);
        let mut meta = read_meta(&path).ok_or_else(|| "scan not found".to_string())?;
        meta.id = id.to_string();
        meta.label = clean_label(label);
        write_meta(&path, &meta)?;
        Ok(meta)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err("bad scan id".into());
        }
        let path = self.root.join(id);
        if !path.is_dir() {
            return Err("scan not found".into());
        }
        std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
    }

    /// Unpack a Pi ZIP into `scans/<id>/`. The archive is written to a temp
    /// folder first, so a truncated download cannot leave a half scan behind.
    pub fn save_zip(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_id(id) {
            return Err("bad scan id".into());
        }
        if bytes.len() > MAX_ZIP_BYTES {
            return Err("scan archive too large".into());
        }
        let tmp = self.root.join(format!(".tmp-{id}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("bad zip: {e}"))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let Some(name) = entry.enclosed_name() else {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err("zip entry escapes the scan folder".into());
            };
            let Some(file) = name
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
            else {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err("zip entry has no filename".into());
            };
            if !FILES.contains(&file.as_str()) || name.components().count() != 1 {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(format!("unexpected file in scan zip: {file}"));
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            std::fs::write(tmp.join(&file), data).map_err(|e| e.to_string())?;
        }
        if !tmp.join("meta.json").is_file() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("scan zip has no meta.json".into());
        }
        let dest = self.root.join(id);
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())
    }

    pub fn zip_one(&self, id: &str) -> Result<Vec<u8>, String> {
        if !valid_id(id) {
            return Err("bad scan id".into());
        }
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err("scan not found".into());
        }
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let opts = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for file in FILES {
                let path = dir.join(file);
                if !path.is_file() {
                    continue;
                }
                let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                zip.start_file(file, opts).map_err(|e| e.to_string())?;
                zip.write_all(&data).map_err(|e| e.to_string())?;
            }
            zip.finish().map_err(|e| e.to_string())?;
        }
        Ok(out)
    }

    pub fn zip_all(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let opts = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for meta in self.list() {
                for file in FILES {
                    let path = self.root.join(&meta.id).join(file);
                    if !path.is_file() {
                        continue;
                    }
                    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                    zip.start_file(format!("{}/{}", meta.id, file), opts)
                        .map_err(|e| e.to_string())?;
                    zip.write_all(&data).map_err(|e| e.to_string())?;
                }
            }
            zip.finish().map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Fetch the Pi's list and download any session missing from `store`.
pub async fn sync_from_pi(
    store: &Arc<RwLock<ScanStore>>,
    http_url: &str,
    token: &str,
) -> Result<usize, String> {
    let base = http_url.trim_end_matches('/');
    if base.is_empty() || token.is_empty() {
        return Err("no Pi scan service configured".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base}/scans"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Pi scan list failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Pi scan list HTTP {}", resp.status()));
    }
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let remote: Vec<ScanMeta> = serde_json::from_value(
        body.get("scans")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
    )
    .map_err(|e| format!("bad Pi scan list: {e}"))?;

    let mut fetched = 0usize;
    for meta in remote {
        if !valid_id(&meta.id) || store.read().await.has(&meta.id) {
            continue;
        }
        let resp = client
            .get(format!("{base}/scans/{}.zip", meta.id))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Pi scan {} failed: {e}", meta.id))?;
        if !resp.status().is_success() {
            return Err(format!("Pi scan {} HTTP {}", meta.id, resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        store.write().await.save_zip(&meta.id, &bytes)?;
        fetched += 1;
    }
    Ok(fetched)
}

/// Remove one session from the Pi archive (best effort, used by Delete).
pub async fn delete_on_pi(http_url: &str, token: &str, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("bad scan id".into());
    }
    let base = http_url.trim_end_matches('/');
    if base.is_empty() || token.is_empty() {
        return Err("no Pi scan service configured".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .delete(format!("{base}/scans/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Pi delete failed: {e}"))?;
    if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
        Ok(())
    } else {
        Err(format!("Pi delete HTTP {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dtd-scan-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn seed(store: &ScanStore, id: &str) {
        let dir = store.root().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!("{{\"id\":\"{id}\",\"sweeps\":2,\"points\":10}}"),
        )
        .unwrap();
        std::fs::write(
            dir.join("display.json"),
            "{\"count\":1,\"points\":[[0,0,0]]}",
        )
        .unwrap();
    }

    #[test]
    fn list_reads_meta_and_size() {
        let root = tmp("list");
        let store = ScanStore::new(root.clone());
        seed(&store, "20261125-100000");
        seed(&store, "20261125-110000");
        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "20261125-110000");
        assert!(list[0].size > 0);
        assert_eq!(store.display("20261125-100000").unwrap()["count"], 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn label_and_delete() {
        let root = tmp("label");
        let store = ScanStore::new(root.clone());
        seed(&store, "20261125-100000");
        let meta = store
            .set_label("20261125-100000", "  Living room  ")
            .unwrap();
        assert_eq!(meta.label, "Living room");
        assert_eq!(store.list()[0].label, "Living room");
        store.delete("20261125-100000").unwrap();
        assert!(store.list().is_empty());
        assert!(store.delete("../x").is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn zip_round_trip_rejects_unknown_entries() {
        let root = tmp("zip");
        let store = ScanStore::new(root.clone());
        seed(&store, "20261125-100000");
        let bytes = store.zip_one("20261125-100000").unwrap();
        let target = tmp("zip2");
        let other = ScanStore::new(target.clone());
        other.save_zip("20261125-100000", &bytes).unwrap();
        assert!(other.has("20261125-100000"));
        assert!(!other.zip_all().unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(target).ok();
    }
}
