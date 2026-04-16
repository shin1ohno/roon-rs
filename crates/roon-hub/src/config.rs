use std::path::Path;

use serde::Deserialize;

/// Hub configuration loaded from TOML with env var overrides.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub roon: RoonConfig,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RoonConfig {
    pub extension_id: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub email: String,
    /// Direct connection host (skip discovery if set).
    pub host: Option<String>,
    /// Direct connection port.
    pub port: Option<u16>,
    /// Path to token store file.
    pub token_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    /// Base topic prefix for all MQTT messages.
    pub topic_prefix: String,
}

impl Default for RoonConfig {
    fn default() -> Self {
        Self {
            extension_id: "com.roon-rs.hub".into(),
            display_name: "roon-hub".into(),
            display_version: env!("CARGO_PKG_VERSION").into(),
            publisher: "roon-rs".into(),
            email: "dev@example.com".into(),
            host: None,
            port: None,
            token_path: "tokens.json".into(),
        }
    }
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 1883,
            client_id: "roon-hub".into(),
            topic_prefix: "roon".into(),
        }
    }
}

impl Config {
    /// Load config from a TOML file, then apply env var overrides.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut config: Config = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            toml::from_str(&content)?
        } else {
            Config::default()
        };

        // Env var overrides (prefix: ROON_HUB_)
        if let Ok(v) = std::env::var("ROON_HUB_MQTT_HOST") {
            config.mqtt.host = v;
        }
        if let Ok(v) = std::env::var("ROON_HUB_MQTT_PORT")
            && let Ok(port) = v.parse()
        {
            config.mqtt.port = port;
        }
        if let Ok(v) = std::env::var("ROON_HUB_ROON_HOST") {
            config.roon.host = Some(v);
        }
        if let Ok(v) = std::env::var("ROON_HUB_ROON_PORT")
            && let Ok(port) = v.parse()
        {
            config.roon.port = Some(port);
        }
        if let Ok(v) = std::env::var("ROON_HUB_TOPIC_PREFIX") {
            config.mqtt.topic_prefix = v;
        }

        Ok(config)
    }
}
