use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the directory where the current executable resides.
pub fn install_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    exe.parent().map(|p| p.to_path_buf())
}

/// Returns a config file path located next to the executable.
pub fn config_path(file_name: &str) -> Option<PathBuf> {
    install_dir().map(|dir| dir.join(file_name))
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
