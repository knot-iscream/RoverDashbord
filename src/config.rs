//! Backend-owned runtime config — the user never edits files or env vars.
//! Precedence: env (`MQTT_BROKER` / `MQTT_PORT` / `PORT`), then
//! `rover_config.json`, then defaults. Changed broker settings apply live
//! (client restarts, no reboot).
//! Home WiFi is NOT here: it lives on the rover itself (phone portal → NVS);
//! the backend only needs the broker address.

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "rover_config.json";

/// App base directory (portable layout): `ROVER_HOME` if set, else the exe's
/// folder when it holds `web/` (packaged app), else the CWD (`cargo run`).
/// Every data path derives from this — no bare CWD-relative paths, so a
/// shortcut with a different "Start in" can never lose data.
pub fn base_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ROVER_HOME") {
        return std::path::PathBuf::from(d);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            if p.join("web").is_dir() {
                return p.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Config path: `ROVER_CONFIG` override if set (integration tests point it
/// at temp dirs so the suite never touches the real file — a test-only
/// `POST /api/config` once overwrote it), else the CWD file.
pub fn config_path() -> std::path::PathBuf {
    std::env::var("ROVER_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| base_dir().join(CONFIG_FILE))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mqtt_broker: String,
    pub mqtt_port: u16,
    /// Listen port for this backend (change needs a restart to take effect).
    pub http_port: u16,
    /// Shared secret for mutating endpoints + WS. Always enforced (cookie,
    /// `?token=`, or `Authorization: Bearer`). The app is localhost-only, so
    /// this defends against drive-by websites, not remote attackers.
    #[serde(default)]
    pub auth_token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mqtt_broker: "localhost".into(),
            mqtt_port: 1883,
            http_port: 8000,
            auth_token: String::new(),
        }
    }
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_port(key: &str) -> Option<u16> {
    env_str(key)?.parse::<u16>().ok().filter(|p| *p > 0)
}

/// 32 alphanumeric chars from a CSPRNG. Generated once, then persisted —
/// never derived from time/pid (predictable) and never logged.
fn new_token() -> String {
    use rand::{distributions::Alphanumeric, Rng};
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

impl AppConfig {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        if let Ok(text) = std::fs::read_to_string(config_path()) {
            if let Ok(file) = serde_json::from_str::<AppConfig>(&text) {
                cfg = file;
            }
        }
        if let Some(b) = env_str("MQTT_BROKER") {
            cfg.mqtt_broker = b;
        }
        if let Some(p) = env_port("MQTT_PORT") {
            cfg.mqtt_port = p;
        }
        if let Some(p) = env_port("PORT") {
            cfg.http_port = p;
        }
        if cfg.auth_token.trim().is_empty() {
            cfg.auth_token = new_token();
            cfg.save();
        }
        cfg
    }

    pub fn save(&self) {
        let Ok(text) = serde_json::to_string_pretty(self) else {
            return;
        };
        // Temp file + rename: a crash mid-write must never truncate config.
        let path = config_path();
        let tmp = format!("{}.tmp", path.display());
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
            tracing::warn!("could not save {}: {e}", path.display());
        }
    }
}

/// Validated update from `POST /api/config` (broker fields only; `http_port`
/// is shown read-only — changing the listen port needs a restart).
pub fn apply_update(cfg: &mut AppConfig, body: &serde_json::Value) -> Result<bool, String> {
    let mut changed = false;
    if let Some(b) = body.get("mqtt_broker").and_then(|v| v.as_str()) {
        let b = b.trim();
        if b.is_empty() {
            return Err("mqtt_broker must not be empty".into());
        }
        if cfg.mqtt_broker != b {
            cfg.mqtt_broker = b.to_string();
            changed = true;
        }
    }
    if let Some(p) = body.get("mqtt_port") {
        let port = p
            .as_u64()
            .filter(|p| *p >= 1 && *p <= 65535)
            .ok_or_else(|| "mqtt_port must be 1..65535".to_string())? as u16;
        if cfg.mqtt_port != port {
            cfg.mqtt_port = port;
            changed = true;
        }
    }
    Ok(changed)
}

/// Replace the token (Setup page "regenerate"). Returns the new value.
pub fn rotate_token(cfg: &mut AppConfig) -> String {
    cfg.auth_token = new_token();
    cfg.save();
    cfg.auth_token.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn update_validates_and_detects_change() {
        let mut c = AppConfig::default();
        assert_eq!(
            apply_update(&mut c, &json!({"mqtt_broker": "192.168.0.96"})),
            Ok(true)
        );
        assert_eq!(c.mqtt_broker, "192.168.0.96");
        assert_eq!(
            apply_update(&mut c, &json!({"mqtt_broker": "192.168.0.96"})),
            Ok(false)
        );
        assert!(apply_update(&mut c, &json!({"mqtt_broker": "  "})).is_err());
        assert!(apply_update(&mut c, &json!({"mqtt_port": 0})).is_err());
        assert!(apply_update(&mut c, &json!({"mqtt_port": 99999})).is_err());
        assert_eq!(apply_update(&mut c, &json!({"mqtt_port": 1884})), Ok(true));
        assert_eq!(c.mqtt_port, 1884);
        // Old config files (new fields missing) still load.
        let old: AppConfig = serde_json::from_str(
            r#"{"mqtt_broker":"192.168.0.5","mqtt_port":1883,"http_port":8000,"lan_access":true}"#,
        )
        .unwrap();
        assert_eq!(old.mqtt_broker, "192.168.0.5");
        assert!(old.auth_token.is_empty());
        // Tokens are 32 alphanumeric chars.
        let t = new_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(t, new_token());
    }
}
