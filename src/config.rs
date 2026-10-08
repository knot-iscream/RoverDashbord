//! Backend-owned runtime config — the user never edits files or env vars.
//! Precedence: env (`MQTT_BROKER` / `MQTT_PORT` / `PORT`) > `rover_config.json`
//! > defaults. Changed broker settings apply live (client restarts, no reboot).
//! SSID/password are NOT here: they live on the rover itself (phone portal
//! `Rover-Setup` → NVS), the backend only needs the broker address.

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "rover_config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mqtt_broker: String,
    pub mqtt_port: u16,
    /// Listen port for this backend (change needs a restart to take effect).
    pub http_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mqtt_broker: "localhost".into(),
            mqtt_port: 1883,
            http_port: 8000,
        }
    }
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_port(key: &str) -> Option<u16> {
    env_str(key)?.parse::<u16>().ok().filter(|p| *p > 0)
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
        cfg
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_FILE, text);
        }
    }

    pub fn broker_label(&self) -> String {
        format!("{}:{}", self.mqtt_broker, self.mqtt_port)
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
        let port = p.as_u64().filter(|p| *p >= 1 && *p <= 65535).ok_or_else(|| {
            "mqtt_port must be 1..65535".to_string()
        })? as u16;
        if cfg.mqtt_port != port {
            cfg.mqtt_port = port;
            changed = true;
        }
    }
    Ok(changed)
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
        assert_eq!(apply_update(&mut c, &json!({"mqtt_broker": "192.168.0.96"})), Ok(false));
        assert!(apply_update(&mut c, &json!({"mqtt_broker": "  "})).is_err());
        assert!(apply_update(&mut c, &json!({"mqtt_port": 0})).is_err());
        assert!(apply_update(&mut c, &json!({"mqtt_port": 99999})).is_err());
        assert_eq!(apply_update(&mut c, &json!({"mqtt_port": 1884})), Ok(true));
        assert_eq!(c.mqtt_port, 1884);
    }
}
