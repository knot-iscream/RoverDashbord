//! Backend-owned runtime config — the user never edits files or env vars.
//! Precedence: env (`MQTT_BROKER` / `MQTT_PORT` / `PORT`), then
//! `rover_config.json`, then defaults. Changed broker settings apply live
//! (client restarts, no reboot).
//! Home WiFi is NOT here: it lives on the rover itself (phone portal → NVS);
//! the backend only needs the broker address.

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "rover_config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mqtt_broker: String,
    pub mqtt_port: u16,
    /// Listen port for this backend (change needs a restart to take effect).
    pub http_port: u16,
    /// false = bind 127.0.0.1 (this PC only). true = bind 0.0.0.0, and the
    /// token below gates every mutating endpoint + WS (needs a restart).
    #[serde(default)]
    pub lan_access: bool,
    /// Shared secret for mutating endpoints + WS. Always enforced (cookie,
    /// `?token=`, or `Authorization: Bearer`), on localhost and LAN alike.
    #[serde(default)]
    pub auth_token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mqtt_broker: "localhost".into(),
            mqtt_port: 1883,
            http_port: 8000,
            lan_access: false,
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

fn env_bool(key: &str) -> Option<bool> {
    match env_str(key)?.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
        if let Ok(text) = std::fs::read_to_string(CONFIG_FILE) {
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
        if let Some(lan) = env_bool("ROVER_LAN") {
            cfg.lan_access = lan;
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
        let tmp = format!("{CONFIG_FILE}.tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, CONFIG_FILE))
        {
            tracing::warn!("could not save {CONFIG_FILE}: {e}");
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
    if let Some(lan) = body.get("lan_access").and_then(|v| v.as_bool()) {
        if cfg.lan_access != lan {
            cfg.lan_access = lan;
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
        assert_eq!(apply_update(&mut c, &json!({"lan_access": true})), Ok(true));
        assert!(c.lan_access);
        assert_eq!(
            apply_update(&mut c, &json!({"lan_access": true})),
            Ok(false)
        );
        // Old config files (no new fields) still load: broker survives upgrade.
        let old: AppConfig = serde_json::from_str(
            r#"{"mqtt_broker":"192.168.0.5","mqtt_port":1883,"http_port":8000}"#,
        )
        .unwrap();
        assert_eq!(old.mqtt_broker, "192.168.0.5");
        assert!(!old.lan_access);
        assert!(old.auth_token.is_empty());
        // Tokens are 32 alphanumeric chars.
        let t = new_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(t, new_token());
    }
}
