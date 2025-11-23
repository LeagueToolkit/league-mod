use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub league_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            league_path: None,
        }
    }
}

/// Returns the directory where the current executable resides.
pub fn install_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    exe.parent().map(|p| p.to_path_buf())
}

/// Returns a config file path located next to the executable.
pub fn config_path(file_name: &str) -> Option<PathBuf> {
    install_dir().map(|dir| dir.join(file_name))
}

pub fn default_config_path() -> Option<PathBuf> {
    config_path("config.toml")
}

pub fn load_config() -> AppConfig {
    if let Some(path) = default_config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(cfg) = toml::from_str(&content) {
                    return cfg;
                }
            }
        }
    }
    AppConfig::default()
}

pub fn save_config(cfg: &AppConfig) -> io::Result<()> {
    if let Some(path) = default_config_path() {
        let content = toml::to_string_pretty(cfg).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, content)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "Could not determine config path"))
    }
}

pub fn load_or_create_config() -> io::Result<(AppConfig, PathBuf)> {
    let path = default_config_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not determine config path"))?;
    
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let cfg = toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok((cfg, path))
    } else {
        let cfg = AppConfig::default();
        save_config(&cfg)?;
        Ok((cfg, path))
    }
}

/// Reads JSON from a path into type T. Returns Ok(None) if file cannot be read or parsed.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        },
        Err(_) => Ok(None),
    }
}

/// Writes pretty JSON to the given path, overwriting if it exists.
pub fn write_json_pretty<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let data = serde_json::to_vec_pretty(value).unwrap_or_else(|_| b"{}".to_vec());
    fs::write(path, data)
}

/// Returns current UNIX epoch seconds.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
