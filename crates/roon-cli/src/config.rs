use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CliConfig {
    pub server: Option<ServerConfig>,
    pub zone: Option<ZoneConfig>,
    pub output: Option<OutputConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneConfig {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub name: String,
}

fn config_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("roon-rs")
        .join("cli.toml")
}

pub fn token_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json")
}

fn session_path(key: &str) -> PathBuf {
    let sanitized: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("roon-rs")
        .join("sessions")
        .join(format!("{}.toml", sanitized))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub hierarchy: Option<String>,
}

pub fn load_session(key: &str) -> SessionState {
    let path = session_path(key);
    if !path.exists() {
        return SessionState::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_session(key: &str, state: &SessionState) -> Result<()> {
    let path = session_path(key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(state)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn load() -> Result<CliConfig> {
    let path = config_path();
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn save(config: &CliConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}
